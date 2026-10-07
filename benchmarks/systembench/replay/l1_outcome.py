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
                schedule_policy: str = "reference", failure_nodes: dict | None = None,
                recovery: bool = False, pool_order: list[str] | None = None) -> dict:
    """单 case L1 replay：分派 → frozen outcome 查表 → evaluate_schedule → completion/cost。

    assignments: node_id -> model_id
    failure_nodes: node_id -> failure kind（F1-F7；命中节点 primary model outcome 不可用）
    recovery: True 时 failed 节点按 pool_order（原分派优先级之外的池序）fallback 重查 frozen
              outcome——recovered 计入 succeeded_recovered；False 直接 failed 并向下游传播。
    传播语义（L1，independent 前提）：节点 failed/skipped → 其下游全部 skipped（依赖未满足）。
    """
    verify_binding(case)
    nodes = case["nodes"]
    node_ids = {n["node_id"] for n in nodes}
    for e in case["edges"]:
        if e["from"] not in node_ids or e["to"] not in node_ids:
            raise SystemExit(f"✗ 边引用未知节点: {case['system_case_id']} {e}")

    deps: dict[str, list[str]] = {n["node_id"]: [] for n in nodes}
    for e in case["edges"]:
        deps[e["to"]].append(e["from"])

    fallback_order = pool_order or ["intern_s1_mini", "deepseek_r1_distill_qwen_7b", "deephermes_3_llama_3_8b_preview"]
    node_results = {}
    wf_nodes = []
    propagated: set[str] = set()

    def resolve(n) -> dict:
        sid = n["outcome_binding"]["source_sample_id"]
        model = assignments[n["node_id"]]
        kind = (failure_nodes or {}).get(n["node_id"])
        # F2：primary model outcome 不可用 → recovery 时按池序 fallback 重查
        if kind == "F2_model_unavailable":
            if recovery:
                for alt in fallback_order:
                    if alt != model and outcomes.get(sid, {}).get(alt):
                        oc = outcomes[sid][alt]
                        return {"model": alt, "score": oc["score"], "cost": oc["cost"],
                                "duration_ms": (oc.get("completion_tokens") or 500) * DURATION_PER_TOKEN_MS,
                                "succeeded": oc["score"] >= 1.0, "injected_failure": True,
                                "recovered": True, "fallback_from": model}
            return {"model": model, "score": 0.0, "cost": 0.0, "duration_ms": 0.0,
                    "succeeded": False, "injected_failure": True, "recovered": False,
                    "fallback_from": model}
        oc = outcomes.get(sid, {}).get(model)
        if oc is None:
            raise SystemExit(f"✗ outcome 缺失: {sid}/{model}（Gate S-2 覆盖破损）")
        return {"model": model, "score": oc["score"], "cost": oc["cost"],
                "duration_ms": (oc.get("completion_tokens") or 500) * DURATION_PER_TOKEN_MS,
                "succeeded": (oc["score"] >= 1.0) and not bool(kind),
                "injected_failure": bool(kind), "recovered": False}

    # 依赖序解析（传播：上游 failed/skipped → 下游 skipped）
    status: dict[str, str] = {}
    order_stack = []
    def visit(nid: str):
        if nid in status:
            return
        for d in deps[nid]:
            visit(d)
        order_stack.append(nid)
        status[nid] = "pending"
    for n in nodes:
        visit(n["node_id"])

    for n in nodes:
        nid = n["node_id"]
        upstream_failed = any(status[d] in ("failed", "skipped") for d in deps[nid])
        if upstream_failed:
            status[nid] = "skipped"
            node_results[nid] = {"model": assignments[nid], "score": 0.0, "cost": 0.0,
                                 "duration_ms": 0.0, "succeeded": False,
                                 "injected_failure": False, "recovered": False, "skipped": True}
            wf_nodes.append({"id": nid, "duration_ms": 0.0})
            continue
        r = resolve(n)
        r["skipped"] = False
        status[nid] = "succeeded" if r["succeeded"] else ("recovered" if r.get("recovered") else "failed")
        node_results[nid] = r
        wf_nodes.append({"id": nid, "duration_ms": r["duration_ms"]})

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

    # primary error（19 号 §十九 priority：execution → recovery → goal；本 replay 无 planning/routing 注入）
    primary_error = None
    if not completion:
        if any(r.get("injected_failure") and not r.get("recovered") for r in node_results.values()):
            primary_error = "execution"
        elif any(r.get("skipped") for r in node_results.values()):
            primary_error = "recovery"
        else:
            primary_error = "goal"

    return {
        "system_case_id": case["system_case_id"],
        "case_class": case["case_class"],
        "split": case["split"],
        "n_nodes": len(nodes),
        "structure_template": case["structure_template"],
        "schedule_policy": schedule_policy,
        "recovery": recovery,
        "workflow_completion": completion,
        "workflow_cost": round(total_cost, 6),
        "strong_share": round(strong_share, 4),
        "makespan_ms": round(sched["actual_makespan_ms"], 1),
        "wave_count": sched["wave_count"],
        "critical_path_ms": round(sched["critical_path_ms"], 1),
        "sequentialization_waste_ms": round(sched["sequentialization_waste_ms"], 1),
        "node_results": node_results,
        "primary_error": primary_error,
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
