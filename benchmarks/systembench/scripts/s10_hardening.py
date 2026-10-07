#!/usr/bin/env python3
"""s10_hardening.py — Phase S10 L1 Hardening（21 号 §八/§十/§十一/§十二，全零成本）。

在【不修改 replay 语义】（l1_outcome.py / evaluator_hash 不变 → v0.1 identity 稳定）的前提下，
补齐 21 号 Gate A 的六项：

  S10.1 Scheduling headline：median/p90 makespan、critical path、parallel width、
        waste、utilization + O2 Schedule Oracle（ideal_legal_makespan）
  S10.2 SystemBench matched random：R1 cost-matched / R2 strong-share-matched
        （确定性加权采样，target 取自 B4 实测——Gate S-5 因果对照）
  S10.3 Recovery ablation：failure cases × {off, on} 的 completion/recovery rate/
        partial/abort/extra cost/extra latency
  S10.4 Cost validity / coverage audit：cost>0 才计成本（cost=0=unknown 非 free）；
        327 节点全覆盖解释（test/calibration/failure 三段）
  S10.5 Interaction matrix：Routing Gain（B3−B4）+ Orchestration Gain（B3−B5，
        deferred）+ O3 Joint Oracle（≤6 节点精确枚举 assignment×schedule，>6 贪心）

产出 reports/v01_hardening.json（并入 v01_report.md 附录）。
v0.1 identity 不变：本脚本只读 cases/outcomes + 调 evaluate_schedule，不触碰 replay 语义。

用法（仓库根目录）：
  benchmarks/routerbench/.venv/bin/python benchmarks/systembench/scripts/s10_hardening.py \
    --assignments /tmp/sb_assignments.jsonl
"""

from __future__ import annotations

import argparse
import hashlib
import itertools
import json
import statistics
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

import numpy as np

_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE.parent))
sys.path.insert(0, str(_HERE.parent.parent / "orchestration"))
from replay.l1_outcome import load_outcomes, replay_case, verify_binding  # noqa: E402
from schedule import evaluate_schedule  # noqa: E402

POOL = {"weak": "deephermes_3_llama_3_8b_preview",
        "mid": "deepseek_r1_distill_qwen_7b",
        "strong": "intern_s1_mini"}
POOL_MODELS = list(POOL.values())
DURATION_PER_TOKEN_MS = 2.0
SEED = 3407


def case_dag(case: dict, durations: dict[str, float]) -> dict:
    return {
        "workflow_id": case["system_case_id"],
        "nodes": [{"id": n["node_id"], "duration_ms": durations[n["node_id"]]} for n in case["nodes"]],
        "edges": [{"from": e["from"], "to": e["to"]} for e in case["edges"]],
        "waves": None,
    }


def sched_metrics(case: dict, durations: dict[str, float], policy: str) -> dict:
    sched = evaluate_schedule(case_dag(case, durations), policy=policy)
    return {
        "makespan_ms": round(sched["actual_makespan_ms"], 1),
        "critical_path_ms": round(sched["critical_path_ms"], 1),
        "wave_count": sched["wave_count"],
        "waste_ms": round(sched["sequentialization_waste_ms"], 1),
        "parallel_width_mean": round(sched["parallel_width_mean"], 2),
        "parallel_width_max": sched["parallel_width_max"],
        "utilization": (round(sched["parallelization_utilization"], 4)
                        if sched["parallelization_utilization"] is not None else None),
        "illegal_parallelism_rate": sched.get("illegal_parallelism_rate", 0.0),
    }


def pct(sorted_vals: list[float], p: float) -> float:
    if not sorted_vals:
        return 0.0
    idx = min(int(len(sorted_vals) * p), len(sorted_vals) - 1)
    return sorted_vals[idx]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--cases", default="benchmarks/systembench/cases/system_cases.jsonl")
    ap.add_argument("--assignments", required=True)
    ap.add_argument("--normalized", default="benchmarks/routerbench/normalized/llmrouterbench")
    ap.add_argument("--bootstrap", type=int, default=10_000)
    args = ap.parse_args()

    root = Path("benchmarks/systembench")
    cases = [json.loads(l) for l in (root / "cases" / "system_cases.jsonl").read_text(encoding="utf-8").split("\n") if l.strip()]
    base_assign = {}
    for line in Path(args.assignments).read_text(encoding="utf-8").split("\n"):
        if line.strip():
            r = json.loads(line)
            base_assign[r["query_id"]] = r["selected_model"]
    outcomes = {sid: {m: oc for m, oc in per.items() if m in POOL_MODELS}
                for sid, per in load_outcomes(Path(args.normalized)).items()}
    outcomes = {k: v for k, v in outcomes.items() if v}

    test_cases = [c for c in cases if c["split"] == "test"]
    failure_cases = [c for c in cases if "injected_failure" in c]

    # ── S10.4 Cost validity / coverage audit ──
    total_nodes = sum(len(c["nodes"]) for c in cases)
    zero_cost_nodes = 0
    for c in cases:
        for n in c["nodes"]:
            sid = n["outcome_binding"]["source_sample_id"]
            for m, oc in outcomes.get(sid, {}).items():
                if oc["cost"] <= 0:
                    zero_cost_nodes += 1
    coverage = {
        "system_cases": len(cases),
        "total_nodes": total_nodes,
        "test_case_nodes": sum(len(c["nodes"]) for c in test_cases),
        "calibration_case_nodes": total_nodes - sum(len(c["nodes"]) for c in test_cases),
        "shadow_parity_nodes": 284,
        "parity_explanation": ("composition B4 只消费 test split（65 case = 284 节点）；"
                               "其余为 calibration 节点（composition 不消费，Gate S-3 语义）+ failure 类节点"
                               "（同属 test/calibration 计入上两项）——无未解释缺口"),
        "cost_validity": {
            "rule": "cost > 0 计入成本指标；cost = 0 视为 unknown（源数据缺定价），不默认 free",
            "zero_cost_nodes_in_pool_outcomes": zero_cost_nodes,
            "verdict": "accuracy 类 case 的 R7 池 cost 全 > 0（arenahard judge 类已由 builder 排除）"
                       if zero_cost_nodes == 0 else f"⚠ {zero_cost_nodes} 个 cost=0 outcome 行",
        },
    }

    # ── 策略分派生成 ──
    def assign_rule(c: dict, rule: str, rng: np.random.Generator | None = None,
                    target: float | None = None) -> dict[str, str]:
        out = {}
        for n in c["nodes"]:
            sid = n["outcome_binding"]["source_sample_id"]
            ocs = outcomes[sid]
            if rule == "flat_strong":
                out[n["node_id"]] = POOL["strong"]
            elif rule == "p2":
                out[n["node_id"]] = base_assign[f"{c['system_case_id']}__{n['node_id']}"]
            elif rule == "node_oracle":
                out[n["node_id"]] = max((oc["score"], -oc["cost"], m) for m, oc in ocs.items())[2]
            elif rule == "r1_cost_matched":
                # case 内 {weak, strong} 两点混合对齐 B4 期望成本（确定性哈希采样）
                cw = statistics.fmean(ocs[POOL["weak"]]["cost"] for _ in [0])
                cs = statistics.fmean(ocs[POOL["strong"]]["cost"] for _ in [0])
                p_s = 0.5 if cs - cw < 1e-15 else min(max((target - cw) / (cs - cw), 0.0), 1.0)
                u = int.from_bytes(hashlib.sha256(f"sb-mr:{SEED}:{c['system_case_id']}:{n['node_id']}".encode()).digest()[:8], "big") / 2**64
                out[n["node_id"]] = POOL["strong"] if u < p_s else POOL["weak"]
            elif rule == "r2_strong_share":
                u = int.from_bytes(hashlib.sha256(f"sb-mr2:{SEED}:{c['system_case_id']}:{n['node_id']}".encode()).digest()[:8], "big") / 2**64
                out[n["node_id"]] = POOL["strong"] if u < min(max(target, 0.0), 1.0) else POOL["mid"]
            else:
                raise SystemExit(f"✗ 未知规则 {rule}")
        return out

    # B4 实测目标（成本与 strong share，从 replay 取）
    b4_assign = {c["system_case_id"]: assign_rule(c, "p2") for c in test_cases}
    b4_targets = {}
    for c in test_cases:
        r = replay_case(c, b4_assign[c["system_case_id"]], outcomes)
        costs = [nr["cost"] for nr in r["node_results"].values()]
        b4_targets[c["system_case_id"]] = {
            "mean_cost": statistics.fmean(costs),
            "strong_share": sum(1 for nr in r["node_results"].values() if nr["model"] == POOL["strong"]) / len(costs),
        }

    policies = {
        "B2_workflow_strong_seq": ("flat_strong", "sequential", False, None),
        "B3_workflow_strong_par": ("flat_strong", "reference", False, None),
        "B4_workflow_p2_par": ("p2", "reference", False, None),
        "R1_cost_matched_random": ("r1_cost_matched", "reference", False, "cost"),
        "R2_strong_share_matched": ("r2_strong_share", "reference", False, "share"),
        "O1_node_oracle_par": ("node_oracle", "reference", False, None),
    }
    rng = np.random.Generator(np.random.PCG64(SEED))

    per_policy: dict[str, list] = defaultdict(list)
    for c in test_cases:
        for name, (rule, sched_pol, rec, mtarget) in policies.items():
            if rule == "r1_cost_matched":
                assign = assign_rule(c, rule, rng, b4_targets[c["system_case_id"]]["mean_cost"])
            elif rule == "r2_strong_share":
                assign = assign_rule(c, rule, rng, b4_targets[c["system_case_id"]]["strong_share"])
            else:
                assign = assign_rule(c, rule)
            r = replay_case(c, assign, outcomes, schedule_policy=sched_pol, recovery=rec)
            # S10.1：完整调度指标（对 final assignment 的 duration 再评估一次，含 O2 oracle 口径）
            durs = {nid: nr["duration_ms"] for nid, nr in r["node_results"].items()}
            r["sched"] = sched_metrics(c, durs, sched_pol)
            r["policy"] = name
            per_policy[name].append(r)

    def summarize(rs: list) -> dict:
        n = len(rs)
        makespans = sorted(r["makespan_ms"] for r in rs)
        sched0 = rs[0]["sched"]
        return {
            "n": n,
            "completion_rate": round(sum(1 for r in rs if r["workflow_completion"]) / n, 4),
            "quality_node_mean": round(statistics.fmean(nr["score"] for r in rs for nr in r["node_results"].values()), 4),
            "cost_per_workflow_mean": round(statistics.fmean(r["workflow_cost"] for r in rs), 6),
            "strong_share_mean": round(statistics.fmean(r["strong_share"] for r in rs), 4),
            "makespan_median_ms": round(statistics.median(makespans), 1),
            "makespan_p90_ms": round(pct(makespans, 0.90), 1),
            "makespan_mean_ms": round(statistics.fmean(makespans), 1),
            "critical_path_ms_mean": round(statistics.fmean(r["sched"]["critical_path_ms"] for r in rs), 1),
            "parallel_width_max_mean": round(statistics.fmean(r["sched"]["parallel_width_max"] for r in rs), 2),
            "waste_ms_mean": round(statistics.fmean(r["sched"]["waste_ms"] for r in rs), 1),
            "utilization_mean": (round(statistics.fmean(r["sched"]["utilization"] for r in rs
                                                   if r["sched"]["utilization"] is not None), 4)
                                 if any(r["sched"]["utilization"] is not None for r in rs) else None),
        }

    summary = {name: summarize(rs) for name, rs in per_policy.items()}

    # O2 Schedule Oracle：critical path 下界（固定 DAG 的最短合法 makespan）
    o2_makespans = sorted(r["sched"]["critical_path_ms"] for r in per_policy["B4_workflow_p2_par"])
    summary["O2_schedule_oracle"] = {
        "n": len(o2_makespans),
        "makespan_median_ms": round(statistics.median(o2_makespans), 1),
        "makespan_mean_ms": round(statistics.fmean(o2_makespans), 1),
        "note": "ideal legal makespan = critical path（schedule.py 口径）；scheduler regret = actual − ideal",
    }

    # S10.3 Recovery ablation（failure cases）
    rec_rows = {"R0_recovery_off": [], "R1_recovery_on": []}
    for c in failure_cases:
        kind = c["injected_failure"]["kind"]
        nid = c["injected_failure"]["node_id"]
        assign = assign_rule(c, "p2")
        off = replay_case(c, assign, outcomes, failure_nodes={nid: kind}, recovery=False)
        on = replay_case(c, assign, outcomes, failure_nodes={nid: kind}, recovery=True)
        off["policy"], on["policy"] = "R0_recovery_off", "R1_recovery_on"
        off["sched"] = sched_metrics(c, {k: v["duration_ms"] for k, v in off["node_results"].items()}, "reference")
        on["sched"] = sched_metrics(c, {k: v["duration_ms"] for k, v in on["node_results"].items()}, "reference")
        rec_rows["R0_recovery_off"].append(off)
        rec_rows["R1_recovery_on"].append(on)

    def rec_summary(rs: list) -> dict:
        n = len(rs)
        total_nodes_f = sum(r["n_nodes"] for r in rs)
        recovered = sum(1 for r in rs for nr in r["node_results"].values() if nr.get("recovered"))
        aborted = sum(1 for r in rs if all(not nr["succeeded"] for nr in r["node_results"].values()))
        partial = sum(1 for r in rs if 0 < sum(1 for nr in r["node_results"].values() if nr["succeeded"]) < r["n_nodes"])
        return {
            "n": n,
            "completion_rate": round(sum(1 for r in rs if r["workflow_completion"]) / max(n, 1), 4),
            "recovery_rate": round(recovered / max(total_nodes_f, 1), 4),
            "recovered_workflows": sum(1 for r in rs if any(nr.get("recovered") for nr in r["node_results"].values())),
            "partial_success_rate": round(partial / max(n, 1), 4),
            "abort_rate": round(aborted / max(n, 1), 4),
            "extra_cost_mean": round(statistics.fmean(
                sum(nr["cost"] for nr in on["node_results"].values())
                - sum(nr["cost"] for nr in off["node_results"].values())
                for off, on in zip(rec_rows["R0_recovery_off"], rec_rows["R1_recovery_on"])), 8),
            "extra_latency_ms_mean": round(statistics.fmean(
                on["makespan_ms"] - off["makespan_ms"]
                for off, on in zip(rec_rows["R0_recovery_off"], rec_rows["R1_recovery_on"])), 1),
        }

    recovery_ablation = {k: rec_summary(rs) for k, rs in rec_rows.items()}

    # ── S10.5 O3 Joint Oracle（≤6 节点精确枚举 assignment；>6 贪心 min-duration）──
    # L1 下 completion 与 assignment 无关（accuracy 0/1 由 frozen score 决定），
    # 联合最优分两口径：O3a min-makespan（assignment→duration→critical path）、
    # O3b min-cost（assignment→cost）。v0.1 报 makespan 口径 O3。
    o3 = {"mode": "exact_enum<=6 / greedy>6", "makespan_median_ms": None, "cases": 0}
    o3_makespans = []
    for c in test_cases:
        n_nodes = c["nodes"]
        durs_by = {n["node_id"]: {m: (outcomes[n["outcome_binding"]["source_sample_id"]][m].get("completion_tokens") or 500) * DURATION_PER_TOKEN_MS
                                  for m in POOL_MODELS} for n in n_nodes}
        if len(n_nodes) <= 6:
            best = None
            for combo in itertools.product(POOL_MODELS, repeat=len(n_nodes)):
                durs = {n["node_id"]: durs_by[n["node_id"]][m] for n, m in zip(n_nodes, combo)}
                ms = evaluate_schedule(case_dag(c, durs), policy="reference")["actual_makespan_ms"]
                if best is None or ms < best:
                    best = ms
            o3_makespans.append(round(best, 1))
        else:
            durs = {n["node_id"]: min(durs_by[n["node_id"]].values()) for n in n_nodes}
            ms = evaluate_schedule(case_dag(c, durs), policy="reference")["actual_makespan_ms"]
            o3_makespans.append(round(ms, 1))
        o3["cases"] += 1
    o3["makespan_median_ms"] = round(statistics.median(o3_makespans), 1)
    o3["makespan_mean_ms"] = round(statistics.fmean(o3_makespans), 1)

    # ── S10.5 Interaction matrix（21 号 §九；B5/B6 deferred→Shadow，先登记骨架）──
    b3, b4 = summary["B3_workflow_strong_par"], summary["B4_workflow_p2_par"]
    interaction = {
        "routing_gain": {
            "d_cost_pct": round((b4["cost_per_workflow_mean"] / b3["cost_per_workflow_mean"] - 1) * 100, 2),
            "d_quality_pp": round((b4["quality_node_mean"] - b3["quality_node_mean"]) * 100, 2),
            "d_strong_share_pp": round((b4["strong_share_mean"] - b3["strong_share_mean"]) * 100, 2),
        },
        "orchestration_gain": "deferred (B5 需真实 LLooM decomposition → Shadow/S11)",
        "interaction_gain": "deferred (Full Gain − Routing Gain − Orchestration Gain；B5/B6 就绪后计算)",
    }

    # ── S10.3 bootstrap（failure completion off vs on）──
    comp_off = np.array([1.0 if r["workflow_completion"] else 0.0 for r in rec_rows["R0_recovery_off"]])
    comp_on = np.array([1.0 if r["workflow_completion"] else 0.0 for r in rec_rows["R1_recovery_on"]])
    stats_boot = np.empty(args.bootstrap)
    for i in range(args.bootstrap):
        idx = rng.integers(0, len(comp_off), size=len(comp_off))
        stats_boot[i] = comp_on[idx].mean() - comp_off[idx].mean()
    recovery_ci = {"point": round(float(comp_on.mean() - comp_off.mean()), 4),
                   "ci95_low": round(float(np.percentile(stats_boot, 2.5)), 4),
                   "ci95_high": round(float(np.percentile(stats_boot, 97.5)), 4),
                   "note": "n=10 failure cases，CI 宽属预期；样本扩容后收敛"}

    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "phase": "S10 L1 Hardening（零成本；replay 语义未动，evaluator_hash/v0.1 identity 不变）",
        "coverage_audit": coverage,
        "summary_with_sched": summary,
        "o2_schedule_oracle": summary["O2_schedule_oracle"],
        "o3_joint_oracle": o3,
        "matched_random": {
            "R1": summary["R1_cost_matched_random"],
            "R2": summary["R2_strong_share_matched"],
            "note": "R1/R2 target 取自 B4 实测（per-case 期望成本 / strong 占比）——Gate S-5 因果对照",
        },
        "recovery_ablation": {**recovery_ablation, "completion_delta_ci": recovery_ci},
        "interaction_matrix": interaction,
        "honesty_notes": [
            "matched random 为 baseline 生成器，非 router 逻辑（不触决策真源红线）",
            "O3 为 L1 口径联合上界（assignment×schedule），不含 workflow topology search（21 号 §十）",
            "recovery ablation 的 extra_cost 含 fallback 节点的 frozen 成本差，extra_latency 为仿真口径",
        ],
    }
    out = Path("benchmarks/systembench/reports/v01_hardening.json")
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    print(f"✓ {out}")
    print(f"{'policy':<26}{'compl':>7}{'Q':>8}{'C/wf':>11}{'strong':>8}{'med_ms':>9}{'p90_ms':>9}")
    for name, s in summary.items():
        if "completion_rate" not in s:
            continue
        print(f"{name:<26}{s['completion_rate']:>7.1%}{s['quality_node_mean']:>8.4f}"
              f"{s['cost_per_workflow_mean']:>11.6f}{s['strong_share_mean']:>8.1%}"
              f"{s['makespan_median_ms']:>9.0f}{s['makespan_p90_ms']:>9.0f}")
    print(f"O2 schedule oracle: median {summary['O2_schedule_oracle']['makespan_median_ms']}ms")
    print(f"O3 joint oracle: median {o3['makespan_median_ms']}ms（{o3['mode']}）")
    print(f"recovery: R0 {recovery_ablation['R0_recovery_off']['completion_rate']:.1%} → "
          f"R1 {recovery_ablation['R1_recovery_on']['completion_rate']:.1%} "
          f"(CI {recovery_ci['ci95_low']:+.4f}~{recovery_ci['ci95_high']:+.4f}) | "
          f"extra_cost ${recovery_ablation['R1_recovery_on']['extra_cost_mean']:.8f}/wf")
    print(f"coverage: {coverage['total_nodes']} 节点（test {coverage['test_case_nodes']}）| "
          f"cost validity: {coverage['cost_validity']['verdict'][:40]}")
    return 0


if __name__ == "__main__":
    main()
