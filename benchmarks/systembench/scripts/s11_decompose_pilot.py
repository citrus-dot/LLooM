#!/usr/bin/env python3
"""s11_decompose_pilot.py — Phase S11：真实 decomposition pilot（21 号 §十五，10-20 PlanningBench 实例）。

定位（21 号 §2.4）：真实 planning calibration + 真实 workflow morphology + Semantic Workflow seeds；
**不用于刷新 v0.1 headline**。固定：模型（qwen-plus）/ prompt（生产 ai_client::decompose 原文
ai_client.rs:350）/ schema（canonical workflow v1）。

流程：
  PlanningBench root_goal（15 实例，确定性均匀采样）
    → 生产 decompose prompt（原文）
    → DashScope compatible chat（temperature=0）
    → JSON 解析 → canonical workflow（nodes/edges from depends_on）
    → validate.py 校验 + planning_eval 打分（obligation recall/precision → tau 校准数据）
    → morphology 统计（vs synthetic 模板对比）
    → Semantic Workflow seeds（raw planner output 存档，artifact_contract 人工标注留 v0.2）

成本核算：15 × (~600 in + ~400 out tokens) × qwen-plus 单价 ≈ $0.02（授权量级内）。
安全：key 从环境变量读取（不硬编码）；endpoint 为模块常量；无动态 URL 拼接。

用法（仓库根目录）：
  DASHSCOPE_API_KEY=… benchmarks/routerbench/.venv/bin/python \
    benchmarks/systembench/scripts/s11_decompose_pilot.py --n 15
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sqlite3
import statistics
import sys
from datetime import datetime, timezone
from pathlib import Path

import requests

_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE.parent))
sys.path.insert(0, str(_HERE.parent.parent / "orchestration"))
from validate import validate_workflow  # noqa: E402
from adapters.planning_eval import evaluate  # noqa: E402  确定性 evaluator（tau 校准对象）

ENDPOINT = "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions"
MODEL = "qwen-plus"
TEMPERATURE = 0
N_INSTANCES = 15
# 生产 decompose prompt 原文（crates/lloom-core/src/ai_client.rs:350，逐字——决策真源）
DECOMPOSE_PROMPT = ("你是一个任务分解专家。将用户的复杂任务分解为2-5个子任务。\n规则：\n"
                    "1. 每个子任务应该是独立的、可执行的\n2. 标注子任务之间的依赖关系（depends_on）\n"
                    "3. 类型只能是 simple_qa / general / coding / math_logic / complex_reasoning\n"
                    "4. 估算每个子任务的输出 token 数\n"
                    "只输出JSON数组，字段为 id, description, task_type, depends_on, estimated_output_tokens。")


def pick_instances(db: Path, n: int) -> list[dict]:
    """PlanningBench 实例确定性均匀采样（跨 sample_id 排序步进）；reference workflow 为 obligations 真源。
    SQL 只做裸 SELECT（Mimosa 红线），benchmark 过滤在 Python 侧。"""
    conn = sqlite3.connect(db)
    rows = [
        (r[0], r[1], r[2]) for r in conn.execute(
            "SELECT sample_id, root_goal, workflow_json, benchmark_id FROM bench_workflows ORDER BY sample_id"
        )
        if "planningbench" in r[3]
    ]
    step = max(1, len(rows) // n)
    picked = rows[::step][:n]
    return [{"sample_id": r[0], "root_goal": r[1],
             "reference_workflow": json.loads(r[2])} for r in picked]


def call_decompose(goal: str, api_key: str) -> tuple[str, int, int]:
    """生产 decompose prompt 原文 → DashScope compatible chat。返回 (raw_output, in_tok, out_tok)。"""
    resp = requests.post(
        ENDPOINT,
        headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"},
        json={
            "model": MODEL,
            "temperature": TEMPERATURE,
            "messages": [
                {"role": "system", "content": DECOMPOSE_PROMPT},
                {"role": "user", "content": goal},
            ],
        },
        timeout=60,
    )
    resp.raise_for_status()
    d = resp.json()
    usage = d.get("usage", {})
    return (d["choices"][0]["message"]["content"],
            usage.get("prompt_tokens", 0), usage.get("completion_tokens", 0))


def parse_plan(raw: str) -> list[dict] | None:
    """从 planner 输出提取 JSON 数组（容忍 ```json 围栏）。"""
    text = raw.strip()
    m = re.search(r"```(?:json)?\s*(\[.*?\])\s*```", text, re.S)
    if m:
        text = m.group(1)
    else:
        m = re.search(r"\[.*\]", text, re.S)
        if m:
            text = m.group(0)
    try:
        arr = json.loads(text)
    except json.JSONDecodeError:
        return None
    if not isinstance(arr, list) or not arr:
        return None
    return arr


def to_workflow(sample_id: str, goal: str, plan: list[dict]) -> dict | None:
    """planner JSON 数组 → canonical workflow v1（补齐 schema 必填字段：schema_version/
    node.depends_on/edge.type——planner 只产 depends_on 数组，边由此派生）。"""
    ids = {p.get("id") for p in plan}
    nodes, edges = [], []
    for p in plan:
        pid = p.get("id")
        if pid is None or pid not in ids:
            return None
        deps = [d for d in (p.get("depends_on") or []) if d in ids and d != pid]
        nodes.append({
            "id": f"n{pid}", "description": str(p.get("description", "")),
            "task_type": str(p.get("task_type", "general")),
            "depends_on": [f"n{d}" for d in deps],
            "estimated_output_tokens": int(p.get("estimated_output_tokens", 500)),
        })
        for d in deps:
            edges.append({"from": f"n{d}", "to": f"n{pid}", "type": "data"})
    wf = {"schema_version": 1, "workflow_id": f"wf_{sample_id}", "root_goal": goal,
          "nodes": nodes, "edges": edges, "waves": None,
          "aggregation": {"type": "final_synthesis"}}
    issues = validate_workflow(wf)
    return wf if not issues else None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--db", default="benchmarks/orchestration/orchbench.db")
    ap.add_argument("--n", type=int, default=N_INSTANCES)
    ap.add_argument("--output", default="benchmarks/systembench/reports/s11_decomposition_pilot.json")
    ap.add_argument("--replay", action="store_true",
                    help="从已有报告的 raw_output 重放管线（零成本，不调 API）")
    args = ap.parse_args()

    api_key = os.environ.get("DASHSCOPE_API_KEY")
    # --replay：从上一次报告的 raw_output 重放管线（不调 API 零成本；reference 仍从 db 取）
    prev_raw: dict[str, str] = {}
    prev_tok: dict[str, tuple[int, int]] = {}
    if args.replay and Path(args.output).exists():
        prev = json.loads(Path(args.output).read_text(encoding="utf-8"))
        for r in prev["results"]:
            prev_raw[r["sample_id"]] = r["raw_output"]
            prev_tok[r["sample_id"]] = (r.get("prompt_tokens", 0), r.get("completion_tokens", 0))

    instances = pick_instances(Path(args.db), args.n)
    print(f"S11 pilot: {len(instances)} PlanningBench 实例 | model={MODEL} temp={TEMPERATURE} "
          f"| prompt=生产 decompose 原文（ai_client.rs:350）")

    results = []
    morph = {"node_counts": [], "edge_counts": [], "depths": []}
    eval_rows = []
    for inst in instances:
        if args.replay:
            if inst["sample_id"] not in prev_raw:
                continue
            raw, in_tok, out_tok = prev_raw[inst["sample_id"]], *prev_tok[inst["sample_id"]]
        else:
            raw, in_tok, out_tok = call_decompose(inst["root_goal"], api_key)
        plan = parse_plan(raw)
        entry = {"sample_id": inst["sample_id"], "raw_output": raw,
                 "prompt_tokens": in_tok, "completion_tokens": out_tok, "model": MODEL}
        if plan is None:
            entry.update({"status": "parse_failed", "workflow": None})
            results.append(entry)
            print(f"  {inst['sample_id']}: ✗ 解析失败")
            continue
        wf = to_workflow(inst["sample_id"], inst["root_goal"], plan)
        if wf is None:
            entry.update({"status": "invalid_workflow", "workflow": None})
            results.append(entry)
            print(f"  {inst['sample_id']}: ✗ validator 拒收")
            continue
        entry.update({"status": "ok", "workflow": wf})
        results.append(entry)
        morph["node_counts"].append(len(wf["nodes"]))
        morph["edge_counts"].append(len(wf["edges"]))
        # tau 校准数据：predicted workflow vs reference workflow（reference 仅 live 模式可得）
        ref_wf = inst.get("reference_workflow")
        if ref_wf and ref_wf.get("nodes"):
            try:
                ev = evaluate(ref_wf, wf)
                eval_rows.append({"sample_id": inst["sample_id"],
                                  "subtask_recall": ev.get("subtask_recall"),
                                  "subtask_precision": ev.get("subtask_precision"),
                                  "invalid_plan_rate": ev.get("invalid_plan_rate")})
            except Exception as e:  # evaluator 接口差异不阻塞 pilot
                eval_rows.append({"sample_id": inst["sample_id"], "error": str(e)})
        print(f"  {inst['sample_id']}: ✓ {len(wf['nodes'])} nodes / {len(wf['edges'])} edges")

    ok_n = sum(1 for r in results if r["status"] == "ok")
    total_in = sum(r.get("prompt_tokens", 0) for r in results)
    total_out = sum(r.get("completion_tokens", 0) for r in results)
    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "model": MODEL, "temperature": TEMPERATURE,
        "prompt_source": "生产 decompose 原文（ai_client.rs:350 逐字）",
        "n_instances": len(instances),
        "ok": ok_n, "parse_failed": sum(1 for r in results if r["status"] == "parse_failed"),
        "invalid_workflow": sum(1 for r in results if r["status"] == "invalid_workflow"),
        "tokens": {"input": total_in, "output": total_out},
        "cost_note": "qwen-plus 计价（DashScope）；实际成本以账单为准，量级 $0.01-0.04",
        "morphology": {
            "node_count_mean": round(statistics.fmean(morph["node_counts"]), 2) if morph["node_counts"] else None,
            "node_count_min": min(morph["node_counts"]) if morph["node_counts"] else None,
            "node_count_max": max(morph["node_counts"]) if morph["node_counts"] else None,
            "edge_count_mean": round(statistics.fmean(morph["edge_counts"]), 2) if morph["edge_counts"] else None,
            "vs_synthetic": "对比 80-case 模板（2-8 节点）——真实 decomposition 的 depth/width 分布是 v0.2 semantic set 的形态依据",
        },
        "planning_eval_calibration": {
            "rows": eval_rows,
            "note": "lexical 2-gram Jaccard / tau=0.45 的首批真实校准数据；"
                    "修改 tau = 新 evaluator hash = 新 benchmark identity（21 号 §十五.4A），本轮不修改",
        },
        "semantic_workflow_seeds": [
            {"sample_id": r["sample_id"], "workflow": r["workflow"]}
            for r in results if r["status"] == "ok"
        ],
        "results": results,
    }
    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    print(f"\n✓ {out}")
    print(f"  ok={ok_n}/{len(instances)} | tokens in={total_in} out={total_out} | 成本量级 ≈$0.02")
    if morph["node_counts"]:
        print(f"  morphology: nodes mean={report['morphology']['node_count_mean']} "
              f"[{report['morphology']['node_count_min']}–{report['morphology']['node_count_max']}]")
    if eval_rows:
        recalls = [r["subtask_recall"] for r in eval_rows if r.get("subtask_recall") is not None]
        precs = [r["subtask_precision"] for r in eval_rows if r.get("subtask_precision") is not None]
        if recalls:
            print(f"  tau 校准数据: subtask_recall mean={statistics.fmean(recalls):.3f} "
                  f"subtask_precision mean={statistics.fmean(precs):.3f}")
    return 0


if __name__ == "__main__":
    main()
