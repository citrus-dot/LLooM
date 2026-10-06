"""router_bridge.py — workflow node ↔ RouterBench subtask 桥（O-Day 12，方案 §十四/§十八 O4）。

角色（方案 §十四）：Orchestrator → subtask → router::plan() → model——LLooM 的子任务
模型分派**不需要另造选择系统**，直接复用 RouterBench 冻结矩阵回放。

边界（协议 §3 红线，本文件如何守住）：
- **不改 router.rs、不复制评分逻辑**：模型分派决策真源 = production `router::plan()`
  （Rust 侧 `bench.rs::replay` 已实现 parity 调用）；本桥只做纯数据变换与结果回填：
    a) forward：canonical workflow → RouterBench instances.jsonl 同构查询行；
    b) backward：外部分派结果 → 回填 node.model 字段（再过 validator 闸门）。
- **fixed-tier 分派是 baseline 生成器**（方案 §十五 Model assignment baselines：
  Fixed Strong/Mid/Weak/Cheapest），不是被测路由器，不构成第二套决策逻辑；
- bench.db readback 是 DB provider：只读 RouterBench 车道的 bench_run_items，
  按 sample_id 逐节点精确匹配，不改对方库。

sample_id 约定（跨车道契约）：`obx__{workflow_id}__{node_id}`——双下划线分隔，
slug 形态（RouterBench slugify 兼容：workflow_id/node_id 均 [a-z0-9_-]）。
split 约定：查询行全部 "test"（编排评测实例不参与 router tuning，泄漏红线）。

指标接口（方案 §十四，纯聚合无决策）：
  assignment_report() → per-node score/cost、global workflow cost、routing regret
  （outcome 由抽象 lookup 注入——正式数据源 = RouterBench FrozenMatrix replay 产出）。

零成本红线：纯 stdlib、无网络、无 LLM；SQLite 只读（mode=ro）。
"""

from __future__ import annotations

import copy
import json
import sqlite3
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from validate import validate_workflow  # noqa: E402

QUERY_PREFIX = "obx"
SEPARATOR = "__"
TIER_KEYS = ("weak", "mid", "strong")


def sample_id_of(workflow_id: str, node_id: str) -> str:
    """跨车道 sample_id 契约（forward/assign_from_* 三方共守）。"""
    return f"{QUERY_PREFIX}{SEPARATOR}{workflow_id}{SEPARATOR}{node_id}"


def node_id_from_sample_id(sid: str) -> tuple[str, str] | None:
    """反解 sample_id → (workflow_id, node_id)；非本桥形态返回 None。"""
    parts = sid.split(SEPARATOR)
    if len(parts) != 3 or parts[0] != QUERY_PREFIX:
        return None
    return parts[1], parts[2]


def forward_queries(workflow: dict, *, dataset_id: str = "orchbench_bridge_v1") -> list[dict]:
    """workflow → RouterBench instances.jsonl 同构查询行（纯数据变换）。

    prompt 组装（最小上下文）：根目标 + 本节点描述——subtask 级路由的输入形态，
    与生产 `/api/routing/plan-subtask` 的 per-subtask plan() 输入同构。
    """
    rows = []
    root_goal = str(workflow.get("root_goal", "")).strip()
    for node in workflow.get("nodes") or []:
        description = str(node.get("description", "")).strip()
        prompt = f"{root_goal}\n子任务：{description}" if root_goal else description
        rows.append(
            {
                "dataset_id": dataset_id,
                "sample_id": sample_id_of(workflow["workflow_id"], node["id"]),
                "task_type": node.get("task_type", "general"),
                "prompt": prompt,
                "split": "test",
                "meta": {
                    "workflow_id": workflow["workflow_id"],
                    "node_id": node["id"],
                    "bridge": "orchbench_v0.1",
                },
            }
        )
    return rows


def assign_fixed_tier(workflow: dict, tier: str, pool: dict[str, str]) -> dict:
    """baseline 分派（方案 §十五）：全节点 → pool[tier]；返回深拷贝（不改输入）。

    tier ∈ weak/mid/strong；cheapest 由调用方预先解析进 pool（本函数不做单价判断）。
    """
    if tier not in TIER_KEYS:
        raise ValueError(f"tier 必须是 {TIER_KEYS}，实际 {tier!r}")
    if tier not in pool:
        raise KeyError(f"pool 缺少 {tier!r} 槽位")
    assigned = copy.deepcopy(workflow)
    for node in assigned["nodes"]:
        node["model"] = pool[tier]
    return assigned


def assign_from_decisions(workflow: dict, decisions: dict[str, str]) -> tuple[dict, dict]:
    """外部决策表 {sample_id: model_id} → 回填 node.model（backward 主路径）。

    返回 (assigned_workflow, report)；unmatched 节点保持 model=None（回填后仍须过
    validator——BAD_MODEL_REF 由 validator 把关）。
    """
    assigned = copy.deepcopy(workflow)
    unmatched: list[str] = []
    for node in assigned["nodes"]:
        sid = sample_id_of(assigned["workflow_id"], node["id"])
        model = decisions.get(sid)
        if model is None:
            unmatched.append(node["id"])
        else:
            node["model"] = model
    report = {
        "n_nodes": len(assigned["nodes"]),
        "n_assigned": len(assigned["nodes"]) - len(unmatched),
        "n_unmatched": len(unmatched),
        "unmatched_nodes": unmatched,
    }
    return assigned, report


def assign_from_bench_db(workflow: dict, db_path: Path, *, strategy: str | None = None) -> tuple[dict, dict]:
    """从 RouterBench bench.db 读回分派（backward 的 DB provider；只读）。

    bench_run_items 是 RouterBench 车道 replay 的逐题决策（selected_model 列）；
    逐节点单占位符精确参数化查询（同 sid 多 run 取 run_id 最新行），绝不写对方库。
    """
    if not Path(db_path).exists():
        raise FileNotFoundError(f"bench.db 不存在: {db_path}")
    nodes = workflow.get("nodes") or []
    if not nodes:
        return workflow, {"n_nodes": 0, "n_assigned": 0, "n_unmatched": 0, "unmatched_nodes": []}
    conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    try:
        decisions: dict[str, str] = {}
        for node in nodes:
            sid = sample_id_of(workflow["workflow_id"], node["id"])
            if strategy is None:
                sql_latest = "SELECT selected_model FROM bench_run_items WHERE sample_id = ? ORDER BY run_id DESC LIMIT 1"
                params_one = (sid,)
                row = conn.execute(sql_latest, params_one).fetchone()
            else:
                sql_latest_strategy = "SELECT selected_model FROM bench_run_items WHERE sample_id = ? AND strategy = ? ORDER BY run_id DESC LIMIT 1"
                params_one = (sid, strategy)
                row = conn.execute(sql_latest_strategy, params_one).fetchone()
            if row is not None:
                decisions[sid] = row[0]
    finally:
        conn.close()
    return assign_from_decisions(workflow, decisions)


def assignment_report(
    workflow: dict,
    outcome_lookup,
    oracle_lookup=None,
) -> dict:
    """分派后指标（方案 §十四）：routing regret + 全局成本 + 缺口计数。

    outcome_lookup(sample_id, model_id) -> (score, cost) | None（缺 outcome 计数）；
    oracle_lookup(sample_id) -> float | None（hindsight 上界；缺省则 regret 恒 None）。
    纯聚合：本函数不做任何模型选择。
    """
    per_node = []
    total_cost = 0.0
    n_missing = 0
    regret_sum = 0.0
    n_regret = 0
    for node in workflow.get("nodes") or []:
        sid = sample_id_of(workflow["workflow_id"], node["id"])
        model = node.get("model")
        if model is None:
            per_node.append({"node_id": node["id"], "model": None, "outcome": None})
            n_missing += 1
            continue
        outcome = outcome_lookup(sid, model)
        if outcome is None:
            n_missing += 1
            per_node.append({"node_id": node["id"], "model": model, "outcome": None})
            continue
        score, cost = outcome
        total_cost += cost
        entry: dict = {"node_id": node["id"], "model": model, "score": score, "cost": cost}
        if oracle_lookup is not None:
            oracle = oracle_lookup(sid)
            if oracle is not None:
                entry["oracle_score"] = oracle
                entry["regret"] = oracle - score
                regret_sum += entry["regret"]
                n_regret += 1
        per_node.append(entry)
    return {
        "n_nodes": len(per_node),
        "n_missing_outcome": n_missing,
        "global_workflow_cost": total_cost,
        "mean_routing_regret": (regret_sum / n_regret) if n_regret else None,
        "per_node": per_node,
    }


def _self_check() -> int:
    """接口链路自检：forward → fixed assign → decisions 回填 → report（lookup 注入）。"""
    ok = True

    def expect(cond: bool, label: str, detail: str = "") -> None:
        nonlocal ok
        print(("PASS " if cond else "FAIL ") + label + (f" {detail}" if detail else ""))
        ok = ok and cond

    wf = json.loads((HERE / "manifests" / "fixtures" / "fixture_fanout.json").read_text(encoding="utf-8"))

    # forward：4 节点 → 4 查询行，sample_id 契约
    queries = forward_queries(wf)
    expect(len(queries) == 4, "forward 行数=4")
    expect(queries[0]["sample_id"] == "obx__ob_fixture_fanout_v1__t1", "sample_id 契约", queries[0]["sample_id"])
    expect(all(q["split"] == "test" for q in queries), "split 恒 test")
    expect(all(q["task_type"] == n["task_type"] for q, n in zip(queries, wf["nodes"])), "task_type 透传")
    expect(node_id_from_sample_id(queries[2]["sample_id"]) == ("ob_fixture_fanout_v1", "t3"), "反解对称")

    # fixed baseline 分派 → 回填后仍过 validator，且输入不被修改
    pool = {"weak": "weak_m", "mid": "mid_m", "strong": "strong_m"}
    assigned = assign_fixed_tier(wf, "strong", pool)
    expect(all(n["model"] == "strong_m" for n in assigned["nodes"]), "fixed strong 全回填")
    expect(not validate_workflow(assigned), "回填后过 validator")
    expect(wf["nodes"][0].get("model") is None, "输入 workflow 不被修改（深拷贝）")

    # decisions 回填 + unmatched 计数
    decisions = {q["sample_id"]: "mid_m" for q in queries[:3]}  # 故意缺 t4
    assigned2, report = assign_from_decisions(wf, decisions)
    expect(report["n_assigned"] == 3 and report["n_unmatched"] == 1, "unmatched 计数", str(report))
    expect(assigned2["nodes"][3]["model"] is None, "缺失节点保持 None")

    # report：outcome 注入（确定性查表），oracle regret 只对已分派节点
    table = {}
    for i, q in enumerate(queries):
        table[(q["sample_id"], "mid_m")] = (0.8 + i * 0.01, 0.001 * (i + 1))
        table[(q["sample_id"], "strong_m")] = (0.9, 0.003)
    oracle = {q["sample_id"]: 1.0 for q in queries}
    rep = assignment_report(assigned2, lambda s, m: table.get((s, m)), lambda s: oracle.get(s))
    expect(rep["n_missing_outcome"] == 1, "缺 model 节点计 missing", str(rep["n_missing_outcome"]))
    expect(abs(rep["global_workflow_cost"] - 0.001 * (1 + 2 + 3)) < 1e-12, "global cost 求和")
    expect(rep["mean_routing_regret"] is not None and rep["mean_routing_regret"] > 0, "regret>0")
    expect(len([p for p in rep["per_node"] if "regret" in p]) == 3, "regret 只对已分派节点")

    return 0 if ok else 1


def main() -> int:
    import argparse

    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--self-check", action="store_true")
    args = ap.parse_args()
    if args.self_check:
        return _self_check()
    ap.print_help()
    return 2


if __name__ == "__main__":
    sys.exit(main())
