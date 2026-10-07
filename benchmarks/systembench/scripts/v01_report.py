#!/usr/bin/env python3
"""v01_report.py — Day S8：SystemBench v0.1 冻结统计与报告（19 号 §十五/§二十九/§三十五/§三十六）。

输入：cases/system_cases.jsonl（80-case）+ /tmp/sb_assignments.jsonl（P2 分派）+ llmrb outcomes。
产出 reports/v01_report.{json,md} + manifests/frozen_manifest_v01.json：

  B2/B3/B4/O1 baseline 矩阵（test split, accuracy 类, workflow_completion 语义）
  Coordination Tax（B4 cost / Σ per-node min-cost − 1，19 号 §十五）
  Routing Gain（B4 vs B3：Δcost/Δquality/Δstrong_share，19 号 §十六）
  depth/width 分层（1-2 / 3-5 / 6-8 node，19 号 §二十九）
  failure/recovery（10 failure cases：recovery on/off 的 completion 对比 + primary_error，§六/§十八）
  workflow-group paired bootstrap（unit=system_case，B=10,000 seed=3407，§三十五）
  frozen_manifest_v01.json（10 项 hash + 引用不复制，§二十一/§二十二）

用法（仓库根目录）：
  benchmarks/routerbench/.venv/bin/python benchmarks/systembench/scripts/v01_report.py \
    --assignments /tmp/sb_assignments.jsonl
"""

from __future__ import annotations

import argparse
import hashlib
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

POOL = {"weak": "deephermes_3_llama_3_8b_preview",
        "mid": "deepseek_r1_distill_qwen_7b",
        "strong": "intern_s1_mini"}


def depth_width(case: dict) -> dict:
    nodes = [n["node_id"] for n in case["nodes"]]
    deps = {n["node_id"]: [] for n in case["nodes"]}
    for e in case["edges"]:
        deps[e["to"]].append(e["from"])
    depth = {}
    def d(nid):
        if nid in depth:
            return depth[nid]
        depth[nid] = 1 + max((d(x) for x in deps[nid]), default=0)
        return depth[nid]
    for n in nodes:
        d(n)
    widths: dict[int, int] = defaultdict(int)
    for n in nodes:
        widths[depth[n]] += 1
    return {"node_count": len(nodes), "edge_count": len(case["edges"]),
            "depth": max(depth.values()), "max_width": max(widths.values()) if widths else 0}


def assignments_for_case(c: dict, rule: str, base_assign: dict, outcomes: dict) -> dict[str, str]:
    out = {}
    for n in c["nodes"]:
        if rule == "flat_strong":
            out[n["node_id"]] = POOL["strong"]
        elif rule == "p2":
            out[n["node_id"]] = base_assign[f"{c['system_case_id']}__{n['node_id']}"]
        elif rule == "node_oracle":
            sid = n["outcome_binding"]["source_sample_id"]
            cands = [(oc["score"], -oc["cost"], m) for m, oc in outcomes[sid].items()]
            out[n["node_id"]] = max(cands)[2]
        elif rule == "node_min_cost":
            sid = n["outcome_binding"]["source_sample_id"]
            out[n["node_id"]] = min(outcomes[sid].items(), key=lambda kv: kv[1]["cost"])[0]
        else:
            raise SystemExit(f"✗ 未知规则 {rule}")
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--cases", default="benchmarks/systembench/cases/system_cases.jsonl")
    ap.add_argument("--assignments", required=True)
    ap.add_argument("--normalized", default="benchmarks/routerbench/normalized/llmrouterbench")
    ap.add_argument("--bootstrap", type=int, default=10_000)
    ap.add_argument("--seed", type=int, default=3407)
    args = ap.parse_args()

    root = Path("benchmarks/systembench")
    cases = [json.loads(l) for l in (root / "cases" / "system_cases.jsonl").read_text(encoding="utf-8").split("\n") if l.strip()]
    base_assign = {}
    for line in Path(args.assignments).read_text(encoding="utf-8").split("\n"):
        if line.strip():
            r = json.loads(line)
            base_assign[r["query_id"]] = r["selected_model"]
    outcomes = {sid: {m: oc for m, oc in per.items() if m in POOL.values()}
                for sid, per in load_outcomes(Path(args.normalized)).items()}
    outcomes = {k: v for k, v in outcomes.items() if v}  # 只留 R7 池三模型（policy 语义；cost=0 的池外行剔除）

    test_cases = [c for c in cases if c["split"] == "test"]
    failure_cases = [c for c in cases if "injected_failure" in c]

    # ── baseline 矩阵（test cases；failure cases 单独跑 recovery 对照）──
    policies = {
        "B2_workflow_strong_seq": ("flat_strong", "sequential", False),
        "B3_workflow_strong_par": ("flat_strong", "reference", False),
        "B4_workflow_p2_par": ("p2", "reference", False),
        "O1_node_oracle_par": ("node_oracle", "reference", False),
        "O0_node_min_cost": ("node_min_cost", "reference", False),
    }
    per_policy: dict[str, list] = defaultdict(list)
    for c in test_cases:
        for name, (rule, sched_pol, rec) in policies.items():
            assign = assignments_for_case(c, rule, base_assign, outcomes)
            r = replay_case(c, assign, outcomes, schedule_policy=sched_pol, recovery=rec)
            r["policy"] = name
            per_policy[name].append(r)

    # ── failure/recovery（10 failure cases，全部 test 假定——builder 未按 split 排除）──
    failure_runs = {"recovery_off": [], "recovery_on": []}
    for c in failure_cases:
        kind = c["injected_failure"]["kind"]
        nid = c["injected_failure"]["node_id"]
        assign = assignments_for_case(c, "p2", base_assign, outcomes)
        off = replay_case(c, assign, outcomes, failure_nodes={nid: kind}, recovery=False)
        on = replay_case(c, assign, outcomes, failure_nodes={nid: kind}, recovery=True)
        off["policy"], on["policy"] = "B4_failure_off", "B4_failure_on"
        off["injected"], on["injected"] = kind, kind
        failure_runs["recovery_off"].append(off)
        failure_runs["recovery_on"].append(on)

    # ── 汇总 ──
    def summarize(rs: list) -> dict:
        n = len(rs)
        if n == 0:
            return {"n": 0}
        return {
            "n": n,
            "completion_rate": round(sum(1 for r in rs if r["workflow_completion"]) / n, 4),
            "quality_node_mean": round(statistics.fmean(nr["score"] for r in rs for nr in r["node_results"].values()), 4),
            "cost_per_workflow_mean": round(statistics.fmean(r["workflow_cost"] for r in rs), 6),
            "strong_share_mean": round(statistics.fmean(r["strong_share"] for r in rs), 4),
            "makespan_ms_mean": round(statistics.fmean(r["makespan_ms"] for r in rs), 1),
        }

    summary = {name: summarize(rs) for name, rs in per_policy.items()}

    # Coordination Tax：B4 cost / Σ per-node min-cost − 1（19 号 §十五；分母=独立最优成本和）
    taxes = []
    for r in per_policy["B4_workflow_p2_par"]:
        c = next(x for x in test_cases if x["system_case_id"] == r["system_case_id"])
        min_sum = sum(min(oc["cost"] for oc in outcomes[n["outcome_binding"]["source_sample_id"]].values())
                      for n in c["nodes"])
        if min_sum > 1e-12:
            taxes.append(r["workflow_cost"] / min_sum - 1)
    coordination_tax = {
        "mean": round(statistics.fmean(taxes), 4) if taxes else None,
        "median": round(statistics.median(taxes), 4) if taxes else None,
        "note": "B4 workflow cost / Σ per-node independently-min-cost − 1（19 号 §十五）；>0 = 编排比逐节点独立最优贵多少",
    }

    # Routing Gain（B4 vs B3）
    b3, b4 = summary["B3_workflow_strong_par"], summary["B4_workflow_p2_par"]
    routing_gain = {
        "d_cost_pct": round((b4["cost_per_workflow_mean"] / b3["cost_per_workflow_mean"] - 1) * 100, 2),
        "d_quality_pp": round((b4["quality_node_mean"] - b3["quality_node_mean"]) * 100, 2),
        "d_strong_share_pp": round((b4["strong_share_mean"] - b3["strong_share_mean"]) * 100, 2),
    }

    # depth/width 分层（19 号 §二十九）
    layers = defaultdict(list)
    for r in per_policy["B4_workflow_p2_par"]:
        c = next(x for x in test_cases if x["system_case_id"] == r["system_case_id"])
        dw = depth_width(c)
        band = "1-2" if dw["node_count"] <= 2 else ("3-5" if dw["node_count"] <= 5 else "6-8")
        layers[band].append(r)
    depth_layers = {band: {
        "n": len(rs),
        "completion_rate": round(sum(1 for r in rs if r["workflow_completion"]) / len(rs), 4),
        "cost_mean": round(statistics.fmean(r["workflow_cost"] for r in rs), 6),
        "routing_gain_vs_flat_pct": round(
            (statistics.fmean(r["workflow_cost"] for r in rs)
             / statistics.fmean(x["workflow_cost"] for x in per_policy["B3_workflow_strong_par"]
                                if x["system_case_id"] == r["system_case_id"]) - 1) * 100, 2),
    } for band, rs in sorted(layers.items())}

    # ── workflow-group paired bootstrap（unit=system_case，§三十五）──
    case_ids = [r["system_case_id"] for r in per_policy["B4_workflow_p2_par"]]
    idx_of = {cid: i for i, cid in enumerate(case_ids)}
    b3_by = {r["system_case_id"]: r for r in per_policy["B3_workflow_strong_par"]}
    b4_by = {r["system_case_id"]: r for r in per_policy["B4_workflow_p2_par"]}
    o1_by = {r["system_case_id"]: r for r in per_policy["O1_node_oracle_par"]}
    cost_b3 = np.array([b3_by[c]["workflow_cost"] for c in case_ids])
    cost_b4 = np.array([b4_by[c]["workflow_cost"] for c in case_ids])
    comp_b3 = np.array([1.0 if b3_by[c]["workflow_completion"] else 0.0 for c in case_ids])
    comp_b4 = np.array([1.0 if b4_by[c]["workflow_completion"] else 0.0 for c in case_ids])
    comp_o1 = np.array([1.0 if o1_by[c]["workflow_completion"] else 0.0 for c in case_ids])
    rng = np.random.Generator(np.random.PCG64(args.seed))

    def paired_ci(stat) -> dict:
        point = float(stat(np.arange(len(case_ids))))
        stats = np.empty(args.bootstrap)
        for i in range(args.bootstrap):
            idx = rng.integers(0, len(case_ids), size=len(case_ids))
            stats[i] = stat(idx)
        return {"point": round(point, 4),
                "ci95_low": round(float(np.percentile(stats, 2.5)), 4),
                "ci95_high": round(float(np.percentile(stats, 97.5)), 4)}

    bootstrap = {
        "unit": "system_case（workflow-group，19 号 §三十五）",
        "B": args.bootstrap, "seed": args.seed,
        "routing_d_cost": paired_ci(lambda i: (cost_b4[i].mean() / cost_b3[i].mean() - 1) if cost_b3[i].mean() > 0 else 0.0),
        "completion_delta_b4_vs_b3": paired_ci(lambda i: comp_b4[i].mean() - comp_b3[i].mean()),
        "completion_delta_o1_vs_b4": paired_ci(lambda i: comp_o1[i].mean() - comp_b4[i].mean()),
    }

    # recovery 汇总（Gate S-6 雏形：primary_error 分布）
    rec_summary = {}
    for key, rs in failure_runs.items():
        rec_summary[key] = {
            "n": len(rs),
            "completion_rate": round(sum(1 for r in rs if r["workflow_completion"]) / max(len(rs), 1), 4),
            "primary_errors": {k: sum(1 for r in rs if r["primary_error"] == k)
                                for k in {r["primary_error"] for r in rs} if k},
        }
    recovered_n = sum(1 for r in failure_runs["recovery_on"]
                      if any(nr.get("recovered") for nr in r["node_results"].values()))

    # ── frozen_manifest_v01（§二十一：10 项 hash + 引用）──
    def sha_file(p: Path) -> str:
        return hashlib.sha256(p.read_bytes()).hexdigest()
    def canon(o) -> bytes:
        return json.dumps(o, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()

    hashes = {
        "routerbench_ref": "cbebcefd05b14057deb789449cc0512944733dc1696d43b9d30b2a142dc9db5a",
        "workflow_schema_hash": sha_file(Path("benchmarks/orchestration/manifests/workflow.schema.json")),
        "model_pool_hash": hashlib.sha256(canon(POOL)).hexdigest(),
        "routing_policy_hash": hashlib.sha256(canon({"policy_id": "routerbench_v02_p2", "family": "P2_softgate0"})).hexdigest(),
        "scheduler_policy_hash": hashlib.sha256(canon(["reference", "sequential", "greedy_ready_queue", "lloom_waves"])).hexdigest(),
        "failure_policy_hash": hashlib.sha256(canon(["F1_timeout", "F2_model_unavailable", "F3_malformed_output",
                                                     "F4_dependency_missing", "F5_branch_failure",
                                                     "F6_partial_aggregate", "F7_budget_exhausted"])).hexdigest(),
        "evaluator_hash": sha_file(_HERE.parent / "replay" / "l1_outcome.py"),
        "split_hash": hashlib.sha256(canon({"unit": "system_case", "seed": 3407, "ratio": 0.2})).hexdigest(),
        "cost_basis_hash": hashlib.sha256(canon({"cost_basis": "llmrouterbench_source"})).hexdigest(),
        "cases_hash": sha_file(root / "cases" / "system_cases.jsonl"),
    }
    frozen = {
        "systembench_id": "lloom-systembench-v0.1",
        "track_scope": "frozen_composition_L1",
        "frozen_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "manifest_hash": hashlib.sha256(canon(hashes)).hexdigest(),
        "hashes": hashes,
        "references": {
            "routerbench_manifest_ref": "benchmarks/routerbench/manifests/frozen_manifest_v02.json",
            "workflow_track_root": "benchmarks/orchestration",
        },
        "test_freeze": {"n_test_cases": len(test_cases), "n_failure_cases": len(failure_cases),
                        "total_nodes": sum(len(c["nodes"]) for c in cases)},
    }
    (root / "manifests" / "frozen_manifest_v01.json").write_text(
        json.dumps(frozen, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    report = {
        "generated_at": frozen["frozen_at"],
        "systembench_id": frozen["systembench_id"],
        "manifest_hash": frozen["manifest_hash"],
        "n_cases": len(cases), "n_test": len(test_cases),
        "summary": summary,
        "coordination_tax": coordination_tax,
        "routing_gain_b4_vs_b3": routing_gain,
        "depth_layers_b4": depth_layers,
        "bootstrap_ci": bootstrap,
        "failure_recovery": {**rec_summary, "recovered_workflows": recovered_n},
        "honesty_notes": [
            "L1 Outcome Replay ≠ live agent result（Gate S-8）：node outcome 为 frozen 查表，makespan 为仿真口径",
            "planning 维 N/A（v0.1 无真实 LLM decomposition）——B5/B6/B7 留 Shadow",
            "Oracle = upper bound under fixed workflow and replay assumptions（§四十三），非 globally optimal agent",
            "policy_id = routerbench_v02_p2（bench variant），不写 production LLooM policy（§四十二）",
        ],
    }
    out = Path("benchmarks/systembench/reports/v01_report.json")
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    lines = [
        "# SystemBench v0.1 · Frozen Composition 报告（L1）",
        "",
        f"> {report['generated_at']} · `{frozen['systembench_id']}` · manifest_hash `{frozen['manifest_hash'][:16]}…`",
        f"> {len(cases)} cases（test {len(test_cases)}）· {sum(len(c['nodes']) for c in cases)} nodes · policy `routerbench_v02_p2`",
        "",
        "| policy | completion | Q_node | cost/wf | strong | makespan |",
        "|---|---:|---:|---:|---:|---:|",
    ]
    for name, s in summary.items():
        lines.append(f"| {name} | {s['completion_rate']:.1%} | {s['quality_node_mean']:.4f} | "
                     f"${s['cost_per_workflow_mean']:.6f} | {s['strong_share_mean']:.1%} | {s['makespan_ms_mean']}ms |")
    lines += ["", f"**Routing Gain（B4 vs B3）**：cost {routing_gain['d_cost_pct']}% · "
              f"quality {routing_gain['d_quality_pp']:+.2f}pp · strong share {routing_gain['d_strong_share_pp']:+.2f}pp",
              "", f"**Coordination Tax**：mean {coordination_tax['mean']}（B4 相对逐节点独立最优成本和）",
              "", "## Bootstrap CI（workflow-group paired，B=10k）", ""]
    for k, v in bootstrap.items():
        if isinstance(v, dict):
            lines.append(f"- {k}: {v['point']:+.4f} [{v['ci95_low']:+.4f}, {v['ci95_high']:+.4f}]")
    lines += ["", "## depth 分层（B4）", "", "| nodes | n | completion | cost | routing gain |", "|---|---:|---:|---:|---:|"]
    for band, s in depth_layers.items():
        lines.append(f"| {band} | {s['n']} | {s['completion_rate']:.1%} | ${s['cost_mean']:.6f} | {s['routing_gain_vs_flat_pct']:+.2f}% |")
    lines += ["", f"**Failure/Recovery**（10 cases）：recovery_off completion {rec_summary['recovery_off']['completion_rate']:.1%}"
              f" → recovery_on {rec_summary['recovery_on']['completion_rate']:.1%}（{recovered_n} workflow 恢复）；"
              f"primary_error {rec_summary['recovery_off']['primary_errors']}", ""]
    (root / "reports" / "v01_report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")

    print(f"✓ {out} + v01_report.md + frozen_manifest_v01.json")
    print(f"  manifest_hash={frozen['manifest_hash'][:16]}…")
    for name, s in summary.items():
        print(f"  {name:<24} completion={s['completion_rate']:.1%} Q={s['quality_node_mean']:.4f} "
              f"C=${s['cost_per_workflow_mean']:.6f} strong={s['strong_share_mean']:.1%}")
    print(f"  routing_gain={routing_gain} | coordination_tax={coordination_tax['mean']}")
    return 0


if __name__ == "__main__":
    main()
