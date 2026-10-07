#!/usr/bin/env python3
"""l1_outcome.py — SystemBench L1 Outcome Replay engine（19 号 §七 Track S1 / §十四）。

L1 语义（fidelity 分层）：
  node outcome = frozen (score, cost, tokens)（llmrb exact binding，无任何新推理）
  scheduling   = 复用 orchestration schedule.py 的 evaluate_schedule（不重写）
  成功语义     = workflow_completion：全部 required node succeeded（accuracy 类 score≥1）
                 AND 依赖满足 AND 无 illegal parallelism（§十四：不包装成 semantic quality）

routing assignment 来源（注入式，三选一）：
  - `lloom-cli bench route-batch` 产物 = LLooM P2 真实分派（policy_id=routerbench_v02_p2）
  - baseline 规则（flat strong / fixed best-single / per-node oracle）——composition.py 生成

用法：
  benchmarks/routerbench/.venv/bin/python benchmarks/systembench/replay/l1_outcome.py --self-check
"""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE.parent.parent / "orchestration"))
sys.path.insert(0, str(_HERE.parent))
from schedule import evaluate_schedule  # noqa: E402  复用 Workflow Track 调度引擎（不重写）

DURATION_PER_TOKEN_MS = 2.0  # 仿真口径（19 号 §十七：离线只报 simulated makespan，不伪装真实 latency）


def load_outcomes(norm_dir: Path) -> dict:
    outcomes: dict[str, dict] = {}
    with (norm_dir / "outcomes.jsonl").open(encoding="utf-8") as f:
        for line in f:
            if not line.strip():
                continue
            o = json.loads(line)
            outcomes.setdefault(o["sample_id"], {})[o["model_id"]] = {
                "score": float(o["score"]), "cost": float(o["cost"]),
                "completion_tokens": o.get("completion_tokens"),
            }
    return outcomes


def verify_binding(case: dict) -> None:
    """Gate S-2 自检：node prompt_hash == sha256(node.prompt)（exact binding 完整性）。"""
    for n in case["nodes"]:
        h = hashlib.sha256(n["prompt"].encode("utf-8")).hexdigest()
        if h != n["outcome_binding"]["prompt_hash"]:
            raise SystemExit(f"✗ binding 破损: {case['system_case_id']}/{n['node_id']}")


def replay_case(case: dict, assignments: dict[str, str], outcomes: dict,
                schedule_policy: str = "reference", failure_nodes: dict | None = None) -> dict:
    """单 case L1 replay：分派 → frozen outcome 查表 → evaluate_schedule → completion/cost。

    assignments: node_id -> model_id
    failure_nodes: node_id -> failure kind（F1-F7；命中节点本 replay 判 failed——recovery 细粒度归 diagnose.py）
    """
    verify_binding(case)
    nodes = case["nodes"]
    node_ids = {n["node_id"] for n in nodes}
    for e in case["edges"]:
        if e["from"] not in node_ids or e["to"] not in node_ids:
            raise SystemExit(f"✗ 边引用未知节点: {case['system_case_id']} {e}")

    node_results = {}
    wf_nodes = []
    for n in nodes:
        sid = n["outcome_binding"]["source_sample_id"]
        model = assignments[n["node_id"]]
        oc = outcomes.get(sid, {}).get(model)
        if oc is None:
            raise SystemExit(f"✗ outcome 缺失: {sid}/{model}（Gate S-2 覆盖破损）")
        failed = bool(failure_nodes and n["node_id"] in failure_nodes)
        dur = (oc.get("completion_tokens") or 500) * DURATION_PER_TOKEN_MS
        node_results[n["node_id"]] = {
            "model": model, "score": oc["score"], "cost": oc["cost"],
            "duration_ms": dur, "succeeded": (oc["score"] >= 1.0) and not failed,
            "injected_failure": failed,
        }
        wf_nodes.append({"id": n["node_id"], "duration_ms": dur})

    wf = {
        "workflow_id": case["system_case_id"],
        "nodes": wf_nodes,
        "edges": [{"from": e["from"], "to": e["to"]} for e in case["edges"]],
        "waves": None,
    }
    sched = evaluate_schedule(wf, policy=schedule_policy)

    all_ok = all(r["succeeded"] for r in node_results.values())
    completion = all_ok and sched.get("illegal_parallelism_rate", 0.0) == 0.0

    total_cost = sum(r["cost"] for r in node_results.values())
    strong_id = "intern_s1_mini"
    strong_share = sum(1 for r in node_results.values() if r["model"] == strong_id) / len(nodes)

    return {
        "system_case_id": case["system_case_id"],
        "split": case["split"],
        "n_nodes": len(nodes),
        "structure_template": case["structure_template"],
        "schedule_policy": schedule_policy,
        "workflow_completion": completion,
        "workflow_cost": round(total_cost, 6),
        "strong_share": round(strong_share, 4),
        "makespan_ms": round(sched["actual_makespan_ms"], 1),
        "wave_count": sched["wave_count"],
        "critical_path_ms": round(sched["critical_path_ms"], 1),
        "sequentialization_waste_ms": round(sched["sequentialization_waste_ms"], 1),
        "node_results": node_results,
    }


def main() -> int:
    if "--self-check" not in sys.argv:
        print("本模块由 runner/composition.py 调用；自检：--self-check")
        return 0
    root = Path("benchmarks/systembench")
    cases = [json.loads(l) for l in (root / "cases" / "system_cases.jsonl").open(encoding="utf-8")]
    outcomes = load_outcomes(Path("benchmarks/routerbench/normalized/llmrouterbench"))
    case = next(c for c in cases if len(c["nodes"]) >= 4)
    assignments = {n["node_id"]: "intern_s1_mini" for n in case["nodes"]}
    r1 = replay_case(case, assignments, outcomes)
    r2 = replay_case(case, assignments, outcomes)
    assert r1 == r2, "❌ L1 replay 非确定性"
    verify_binding(case)
    # counterfactual 冒烟：换 weak 再跑，成本应变化（strict independent 前提下合法）
    aw = {n["node_id"]: "deephermes_3_llama_3_8b_preview" for n in case["nodes"]}
    r3 = replay_case(case, aw, outcomes)
    assert r3["workflow_cost"] != r1["workflow_cost"], "❌ counterfactual 成本无差异（binding 可疑）"
    print(f"PASS L1 determinism + counterfactual（case={case['system_case_id']} "
          f"strong makespan={r1['makespan_ms']}ms cost=${r1['workflow_cost']:.6f} → "
          f"weak cost=${r3['workflow_cost']:.6f}）")
    return 0


if __name__ == "__main__":
    main()
