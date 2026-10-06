"""adapters/planningbench.py — PlanningBench（Tencent-Hunyuan）adapter（O-Day 10）。

源：data/PlanningBench-eval.jsonl（467 实例，CC-BY-4.0，官方声明 evaluation-only）。
形态（实测 2026-10-06）：{idx: int, messages: [{role: user, content: 规划题面}], checklist: 编号验证项}。

canonical 化决策（设计记录，与方案 §七/§九对齐）：
- PlanningBench 只有 goal + checklist，**没有 reference 子任务 DAG**；
- adapter 把 checklist 的「数字顿号」编号验证项逐项拆成 obligation 节点
  （meta.node_semantics="checklist_obligation"），成为 §九 9.1 subtask recall 的
  required atomic obligations 真源——online pilot 时 LLM plan 的节点与之对齐；
- aggregation.type="none"（planning-only 任务，validator 允许空/无聚合节点集）；
- split：源数据声明 evaluation-only → 全量 split="test"（不参与任何 tuning），
  meta 记录 rationale；sample_id = ob_pb_{idx:04d}（idx 全局唯一已实测）。

零成本红线：只读本地文件；无网络、无 LLM。落库写独立 orchbench.db（bench_workflows）。

用法：
  python3 adapters/planningbench.py \
      --input raw/PlanningBench/data/PlanningBench-eval.jsonl --db ../orchbench.db --import
  python3 adapters/planningbench.py --input ...            # 只统计不写库
"""

from __future__ import annotations

import argparse
import json
import re
import sqlite3
import sys
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from validate import validate_workflow  # noqa: E402

BENCHMARK_ID = "lloom-orchbench-planningbench-v0.1"
OBLIG_RE = re.compile(r"^\s*(\d+)\s*[、.．]\s*")
# 真锚点形态（实测 2026-10-06）：验证项以「数字、」开头且前有换行（⏎ 2、）；
# 行中的「N、」多为数值列举（如「N1取值25、N2取值55」），必须用行首锚定避免误拆。


def parse_checklist(checklist: str) -> list[str]:
    """checklist → 原子验证项列表（确定性：按「行首数字顿号」锚点切分，续行并入当前项）。

    实测形态：编号项以换行分隔，项内可含分号子句与行内数字列举；
    首个编号项之前的前导说明行（若有）作为独立项保留。
    """
    items: list[str] = []
    for line in checklist.split("\n"):
        stripped = line.strip()
        if not stripped:
            continue
        if OBLIG_RE.match(stripped):
            items.append(OBLIG_RE.sub("", stripped, count=1).strip())
        elif items:
            items[-1] = f"{items[-1]}\n{stripped}"
        else:
            items.append(stripped)
    return items


def canonicalize(row: dict) -> dict:
    """单行源数据 → canonical workflow（obligation-node 形态，无参考 DAG）。

    PlanningBench 全部是结构化 planning 题（排班/路由/生产依赖/应急优先级），不属
    simple_qa/coding/math_logic 的生产语义；统一 general（band_for→medium 基线），
    family 细分保留源 checklist 原文，不做二次分类（避免造第二套规则）。
    """
    idx = int(row["idx"])
    messages = row.get("messages") or []
    question = messages[0].get("content", "") if messages else ""
    obligations = parse_checklist(row.get("checklist", ""))
    sid = f"ob_pb_{idx:04d}"
    return {
        "workflow_id": sid,
        "schema_version": 1,
        "root_goal": question,
        "nodes": [
            {
                "id": f"ob{i + 1:02d}",
                "description": text,
                "task_type": "general",
                "depends_on": [],
                "constraints": [],
                "model": None,
                "estimated_output_tokens": 256,
                "duration_ms": None,
            }
            for i, text in enumerate(obligations)
        ],
        "edges": [],
        "waves": None,
        "aggregation": {"type": "none"},
        "meta": {
            "source_benchmark": "planningbench",
            "source_id": str(idx),
            "source_goal": question,
            "node_semantics": "checklist_obligation",
            "obligation_count": len(obligations),
            "split_rationale": "源数据声明 evaluation-only，全量 test（README: all 467 intended for evaluation）",
            "duration_basis": None,
        },
    }


def iter_rows(jsonl_path: Path, limit: int | None = None):
    """逐行读取（红线：split("\\n")，不用 splitlines——U+2028/2029 坑）。"""
    n = 0
    for line in jsonl_path.read_text(encoding="utf-8").split("\n"):
        if not line.strip():
            continue
        if limit is not None and n >= limit:
            break
        n += 1
        yield json.loads(line)


def run_scan(jsonl_path: Path, limit: int | None = None) -> dict:
    """统计 + 校验（不写库）。"""
    stats: dict = {"rows": 0, "imported": 0, "rejected": 0, "obligations_total": 0, "reject_examples": []}
    hist: Counter[int] = Counter()
    for row in iter_rows(jsonl_path, limit):
        stats["rows"] += 1
        wf = canonicalize(row)
        issues = validate_workflow(wf, strict_meta=True)
        if issues:
            stats["rejected"] += 1
            if len(stats["reject_examples"]) < 5:
                stats["reject_examples"].append(
                    {"workflow_id": wf["workflow_id"], "issues": [str(i) for i in issues]}
                )
            continue
        stats["imported"] += 1
        hist[wf["meta"]["obligation_count"]] += 1
    stats["oblig_hist"] = dict(sorted(hist.items()))
    stats["obligations_total"] = sum(k * v for k, v in hist.items())
    return stats


def run_import(jsonl_path: Path, db_path: Path, limit: int | None = None) -> dict:
    """导入 → bench_workflows；validator 拒收的实例计数并跳过（不静默改数据）。

    SQL 契约（Mimosa 约束）：语句一律单行整字面量 + ? 占位符，值只经参数元组进入；
    不用相邻字面量拼接、不把任何变量拼进语句文本。
    """
    stats = run_scan(jsonl_path, limit)
    if stats["imported"] == 0:
        stats["db_rows"] = 0
        return stats
    conn = sqlite3.connect(db_path)
    try:
        sql_upsert = "INSERT OR REPLACE INTO bench_workflows (benchmark_id, sample_id, root_goal, workflow_json, reference_graph_json, verification_json, split, meta_json) VALUES (?,?,?,?,?,?,?,?)"
        sql_count = "SELECT COUNT(*) FROM bench_workflows WHERE benchmark_id = ?"
        for row in iter_rows(jsonl_path, limit):
            wf = canonicalize(row)
            if validate_workflow(wf, strict_meta=True):
                continue
            params_upsert = (
                BENCHMARK_ID,
                wf["workflow_id"],
                wf["root_goal"],
                json.dumps(wf, ensure_ascii=False),
                None,
                json.dumps({"checklist": row.get("checklist")}, ensure_ascii=False),
                "test",
                json.dumps(wf["meta"], ensure_ascii=False),
            )
            conn.execute(sql_upsert, params_upsert)
        conn.commit()
        params_count = (BENCHMARK_ID,)
        stats["db_rows"] = conn.execute(sql_count, params_count).fetchone()[0]
    finally:
        conn.close()
    return stats


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--input", default="raw/PlanningBench/data/PlanningBench-eval.jsonl")
    ap.add_argument("--db", default=str(HERE.parent / "orchbench.db"))
    ap.add_argument("--import", dest="do_import", action="store_true", help="落库（默认只统计不写）")
    ap.add_argument("--limit", type=int, default=None, help="只处理前 N 行（调试）")
    args = ap.parse_args()

    src = Path(args.input)
    if not src.exists():
        raise SystemExit(f"✗ 找不到输入: {src}（先克隆 raw/PlanningBench，见 datasets.yaml）")

    if args.do_import:
        db = Path(args.db)
        if not db.exists():
            raise SystemExit(f"✗ 找不到 {db}（先 python3 scripts/db_init.py）")
        if db.name == "lloom.db" or "data" in db.parts:
            raise SystemExit("✗ 拒绝：orchbench 只允许独立库，禁止指向 data/lloom.db")
        stats = run_import(src, db, args.limit)
    else:
        stats = run_scan(src, args.limit)

    print(
        f"rows={stats['rows']} imported={stats['imported']} rejected={stats['rejected']} "
        f"obligations_total={stats['obligations_total']}"
    )
    if stats["imported"]:
        print(f"mean obligations/instance = {stats['obligations_total'] / stats['imported']:.2f}")
        print("oblig_hist:", stats["oblig_hist"])
    for ex in stats["reject_examples"]:
        print("REJECT", ex["workflow_id"], ex["issues"])
    if args.do_import:
        print(f"db_rows={stats['db_rows']} ({BENCHMARK_ID})")
    return 0 if stats["imported"] > 0 or stats["rows"] == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
