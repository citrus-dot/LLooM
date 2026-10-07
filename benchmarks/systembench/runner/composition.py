#!/usr/bin/env python3
"""composition.py — SystemBench v0.1 Frozen Composition runner（19 号 §十二/§二十六，Day S5）。

baseline 因果隔离矩阵（v0.1 实装收缩版——planning 维留给 Shadow，见 §六"v0.1 只证 control plane"）：
  B0 Flat-Strong            1-node 参考（root goal 无 frozen outcome → 只报形态，不进 headline 对比）
  B2 FixedWorkflow-Strong-Seq    reference workflow × 全 strong × sequential
  B3 FixedWorkflow-Strong-Par    reference workflow × 全 strong × parallel(reference waves)
  B4 FixedWorkflow-P2Routed-Par  reference workflow × LLooM P2 分派（route-batch 产物）× parallel
  O1 Node-Oracle-Par             per-node argmax quality（tie 取 min cost）——hindsight 上界
headline 口径：test split cases、accuracy 类、completion/cost/strong_share/makespan + Routing Gain（B4 vs B3）。

用法（仓库根目录）：
  benchmarks/routerbench/.venv/bin/python benchmarks/systembench/runner/composition.py \
    --assignments /tmp/sb_assignments.jsonl \
    --output benchmarks/systembench/reports/composition_v01.json
"""

from __future__ import annotations

import argparse
import json
import statistics
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE.parent))
sys.path.insert(0, str(_HERE.parent.parent / "orchestration"))
from replay.l1_outcome import load_outcomes, replay_case  # noqa: E402


def load_assignments(path: Path) -> dict[str, str]:
    out = {}
    for line in path.read_text(encoding="utf-8").split("\n"):
        if not line.strip():
            continue
        r = json.loads(line)
        out[r["query_id"]] = r["selected_model"]
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--cases", default="benchmarks/systembench/cases/system_cases.jsonl")
    ap.add_argument("--assignments", required=True, help="route-batch 产物（LLooM P2 分派）")
    ap.add_argument("--normalized", default="benchmarks/routerbench/normalized/llmrouterbench")
    ap.add_argument("--output", default="benchmarks/systembench/reports/composition_v01.json")
    args = ap.parse_args()

    from pathlib import Path as P
    registry = (P("benchmarks/systembench/manifests/system.yaml")).read_text(encoding="utf-8")
    assert "routerbench_v02_p2" in registry, "policy identity 缺失（system.yaml 损坏？）"

    cases = [json.loads(l) for l in P(args.cases).read_text(encoding="utf-8").split("\n") if l.strip()]
    test_cases = [c for c in cases if c["split"] == "test"]
    outcomes = load_outcomes(P(args.normalized))
    base_assign = load_assignments(P(args.assignments))
    pool = {"weak": "deephermes_3_llama_3_8b_preview",
            "mid": "deepseek_r1_distill_qwen_7b",
            "strong": "intern_s1_mini"}

    policies = {
        "B2_workflow_strong_seq": ("flat_strong", "sequential"),
        "B3_workflow_strong_par": ("flat_strong", "reference"),
        "B4_workflow_p2_par": ("p2", "reference"),
        "O1_node_oracle_par": ("node_oracle", "reference"),
    }

    def assignments_for_case(c: dict, rule: str) -> dict[str, str]:
        out = {}
        for n in c["nodes"]:
            if rule == "flat_strong":
                out[n["node_id"]] = pool["strong"]
            elif rule == "p2":
                qid = f"{c['system_case_id']}__{n['node_id']}"
                out[n["node_id"]] = base_assign[qid]
            elif rule == "node_oracle":
                sid = n["outcome_binding"]["source_sample_id"]
                cands = [(oc["score"], -oc["cost"], m) for m, oc in outcomes[sid].items()]
                # hindsight 上界：quality 最大（tie 取 cost 最小）——仅供上界对照
                out[n["node_id"]] = max(cands)[2]
            else:
                raise SystemExit(f"✗ 未知规则 {rule}")
        return out

    per_policy: dict[str, list] = defaultdict(list)
    for c in test_cases:
        for name, (rule, sched_policy) in policies.items():
            assign = assignments_for_case(c, rule)
            r = replay_case(c, assign, outcomes, schedule_policy=sched_policy)
            r["policy"] = name
            per_policy[name].append(r)

    summary = {}
    for name, rs in per_policy.items():
        n = len(rs)
        comp = sum(1 for r in rs if r["workflow_completion"]) / n
        summary[name] = {
            "n_cases": n,
            "completion_rate": round(comp, 4),
            "quality_node_mean": round(statistics.fmean(
                nr["score"] for r in rs for nr in r["node_results"].values()), 4),
            "cost_per_workflow_mean": round(statistics.fmean(r["workflow_cost"] for r in rs), 6),
            "strong_share_mean": round(statistics.fmean(r["strong_share"] for r in rs), 4),
            "makespan_ms_mean": round(statistics.fmean(r["makespan_ms"] for r in rs), 1),
            "coordination_tax": None,  # Phase B：相对独立最优节点成本和（O1 定义见 19 号 §十五）
        }

    # Routing Gain（B4 vs B3：固定 workflow/schedule，只差 routing）
    b3 = summary["B3_workflow_strong_par"]
    b4 = summary["B4_workflow_p2_par"]
    routing_gain = {
        "d_cost_pct": round((b4["cost_per_workflow_mean"] / b3["cost_per_workflow_mean"] - 1) * 100, 2),
        "d_quality_pp": round((b4["quality_node_mean"] - b3["quality_node_mean"]) * 100, 2),
        "d_strong_share_pp": round((b4["strong_share_mean"] - b3["strong_share_mean"]) * 100, 2),
    }

    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "policy_id": "routerbench_v02_p2",
        "n_test_cases": len(test_cases),
        "summary": summary,
        "routing_gain_b4_vs_b3": routing_gain,
        "honesty_notes": [
            "v0.1 Frozen 的 planning 维 N/A（无真实 LLM decomposition）——B5/B6/B7 留 Shadow track（19 号 §六）",
            "B0 Flat-Strong 仅形态参考：goal-level frozen outcome 不存在，不进 headline 对比（19 号 §十四 禁伪装）",
            "O1 Node Oracle 是 hindsight 上界（per-node argmax quality），Oracle = upper bound under fixed workflow（§四十三）",
            "routing assignment 全 strong 时 B3≡B4 属预期（llmrb 长 prompt → is_complex → hard 带）——"
            "band 门槛主导的第三次独立复现，routing gain 的真实验场在短 query/Shadow",
        ],
        "per_case": {name: rs for name, rs in per_policy.items()},
    }
    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    print(f"✓ {out}")
    for name, s in summary.items():
        print(f"  {name:<24} n={s['n_cases']} completion={s['completion_rate']:.1%} "
              f"Q_node={s['quality_node_mean']:.4f} C/wf=${s['cost_per_workflow_mean']:.6f} "
              f"strong={s['strong_share_mean']:.1%} makespan={s['makespan_ms_ms' if False else 'makespan_ms_mean']}ms")
    print(f"Routing Gain (B4 vs B3): {routing_gain}")
    return 0


if __name__ == "__main__":
    main()
