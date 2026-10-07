#!/usr/bin/env python3
"""s12_semantic_expand.py — Phase S12.2：Semantic Workflow 扩采（24 号 D1-c'/25 号 §五）。

对 15 个 PlanningBench **未用** root_goal 再跑生产 decompose（与 S11 同管线：qwen-plus /
temperature=0 / 生产 ai_client.rs:350 prompt 原文 / canonical workflow v1），产出 v0.2 semantic
candidate pool 的第二批。S12.1 已审计的 15 个 S11 seeds 保留为第一批，合计 30 个 semantic
candidates → S12.3 标注后筛选 ≥20 进 v0.2 set（freeze gate 缓冲）。

与 S11 的差异（仅两点，管线零复制决策真源不变）：
  1. 实例采样排除 S11 已用的 15 个（确定性重算 S11 采样集合后差集步进）；
  2. 产物独立存档 s12_decomposition_batch.json（S11 报告只读，--replay 语义不破坏）。

用法（仓库根目录）：
  DASHSCOPE_API_KEY=… benchmarks/routerbench/.venv/bin/python \
    benchmarks/systembench/scripts/s12_semantic_expand.py --n 15
  # 重放（零成本）：
  benchmarks/routerbench/.venv/bin/python benchmarks/systembench/scripts/s12_semantic_expand.py --replay
"""

from __future__ import annotations

import argparse
import json
import statistics
import sys
from datetime import datetime, timezone
from pathlib import Path

_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))
from s11_decompose_pilot import (  # noqa: E402  决策真源复用：prompt/call/parse/canonical 全部同一实现
    MODEL, TEMPERATURE, call_decompose, parse_plan, to_workflow,
)

S11_REPORT = _HERE.parent / "reports" / "s11_decomposition_pilot.json"
DEFAULT_OUT = _HERE.parent / "reports" / "s12_decomposition_batch.json"


def s11_used_ids(db_rows: list[tuple[str, str]], n_s11: int = 15) -> set[str]:
    """确定性重算 S11 的采样集合（s11_decompose_pilot.pick_instances 同法：排序 + 步进）。"""
    pb = [sid for sid, bid in db_rows if "planningbench" in bid]
    step = max(1, len(pb) // n_s11)
    return set(pb[::step][:n_s11])


def pick_new_instances(db: Path, n: int, exclude: set[str]) -> list[dict]:
    """PlanningBench 未用实例确定性均匀采样（排除集外排序步进；SQL 裸 SELECT，过滤在 Python 侧）。"""
    import sqlite3
    conn = sqlite3.connect(db)
    rows = [
        (r[0], r[1], r[2]) for r in conn.execute(
            "SELECT sample_id, root_goal, workflow_json, benchmark_id FROM bench_workflows ORDER BY sample_id"
        )
        if "planningbench" in r[3] and r[0] not in exclude
    ]
    step = max(1, len(rows) // n)
    picked = rows[::step][:n]
    return [{"sample_id": r[0], "root_goal": r[1],
             "reference_workflow": json.loads(r[2])} for r in picked]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--db", default="benchmarks/orchestration/orchbench.db")
    ap.add_argument("--n", type=int, default=15)
    ap.add_argument("--output", default=str(DEFAULT_OUT))
    ap.add_argument("--replay", action="store_true",
                    help="从已有 batch 报告的 raw_output 重放管线（零成本，不调 API）")
    args = ap.parse_args()

    import os
    import sqlite3
    conn = sqlite3.connect(args.db)
    db_rows = [(r[0], r[1]) for r in conn.execute(
        "SELECT sample_id, benchmark_id FROM bench_workflows ORDER BY sample_id")]
    used = s11_used_ids(db_rows)

    prev_raw: dict[str, str] = {}
    prev_tok: dict[str, tuple[int, int]] = {}
    if args.replay:
        prev = json.loads(Path(args.output).read_text(encoding="utf-8"))
        for r in prev["results"]:
            prev_raw[r["sample_id"]] = r["raw_output"]
            prev_tok[r["sample_id"]] = (r.get("prompt_tokens", 0), r.get("completion_tokens", 0))

    instances = pick_new_instances(Path(args.db), args.n, exclude=used)
    print(f"S12.2 扩采: {len(instances)} PlanningBench 未用实例（排除 S11 {len(used)} 个）"
          f" | model={MODEL} temp={TEMPERATURE} | prompt=生产 decompose 原文（ai_client.rs:350）")

    api_key = os.environ.get("DASHSCOPE_API_KEY")
    results = []
    morph = {"node_counts": [], "edge_counts": [], "depths": []}
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
        print(f"  {inst['sample_id']}: ✓ {len(wf['nodes'])} nodes / {len(wf['edges'])} edges")

    ok_n = sum(1 for r in results if r["status"] == "ok")
    total_in = sum(r.get("prompt_tokens", 0) for r in results)
    total_out = sum(r.get("completion_tokens", 0) for r in results)
    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "batch": "s12_expansion",
        "model": MODEL, "temperature": TEMPERATURE,
        "prompt_source": "生产 decompose 原文（ai_client.rs:350 逐字，与 S11 同源）",
        "excluded_s11_sample_ids": sorted(used),
        "n_instances": len(instances),
        "ok": ok_n, "parse_failed": sum(1 for r in results if r["status"] == "parse_failed"),
        "invalid_workflow": sum(1 for r in results if r["status"] == "invalid_workflow"),
        "tokens": {"input": total_in, "output": total_out},
        "cost_note": "qwen-plus 计价（DashScope）；实际成本以账单为准（授权 ~$0.04 内）",
        "morphology": {
            "node_count_mean": round(statistics.fmean(morph["node_counts"]), 2) if morph["node_counts"] else None,
            "node_count_min": min(morph["node_counts"]) if morph["node_counts"] else None,
            "node_count_max": max(morph["node_counts"]) if morph["node_counts"] else None,
            "edge_count_mean": round(statistics.fmean(morph["edge_counts"]), 2) if morph["edge_counts"] else None,
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
    print(f"  ok={ok_n}/{len(instances)} | tokens in={total_in} out={total_out}")
    if morph["node_counts"]:
        print(f"  morphology: nodes mean={report['morphology']['node_count_mean']} "
              f"[{report['morphology']['node_count_min']}–{report['morphology']['node_count_max']}]")
    return 0


if __name__ == "__main__":
    sys.exit(main())
