"""adapters/planbench.py — PlanBench（harshakokel/PlanBench，MIT）adapter（O-Day 10）。

源：plan-bench/instances/{blocksworld,logistics}/generated/*.pddl（PDDL problem 文件）。
角色（方案 §4.4）：结构化 planning sanity + **reference dependency graph 真源**
（方案 §九 9.4：dependency edge P/R 只对有 reference graph 的 benchmark——
Blocksworld 的 goal 堆叠结构天然给出确定性依赖，Logistics 的 at 目标彼此独立）。

canonical 化建模（确定性，不引入 solver）：
- Blocksworld：goal 每个原子 (on X Y) 一个节点；依赖 = Y 的最终位置节点
  （goal 中若存在 (on Y Z)，则 on X Y 依赖 on Y Z）——堆叠链 DAG；
- Logistics：goal 每个原子 (at o l) 一个节点，包裹目标相互独立（零依赖参照）；
- obfuscated 变体目录仅登记抽样（robustness sanity 预留），canonical 结构同 logistics。

split：哈希桶 sha256("lloom-obsplit:3407:"+sample_id)，calibration 20 / test 80
（PlanBench 无 evaluation-only 声明，按 RouterBench 治理惯例切分；切分可复现）。

零成本红线：只读本地文件；无网络、无 LLM。落库独立 orchbench.db。

用法：
  python3 adapters/planbench.py --input raw/PlanBench --db ../orchbench.db --import
  python3 adapters/planbench.py --input ...            # 只统计不写库
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sqlite3
import sys
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from validate import validate_workflow  # noqa: E402

BENCHMARK_ID = "lloom-orchbench-planbench-v0.1"
SPLIT_SEED = 3407
CALIBRATION_RATIO = 0.2
MAX_PER_VARIANT = 200  # 单 (domain/variant) 导入上限（防 db 膨胀；文件序升序前 N）

_ATOM_RE = re.compile(r"\(([a-zA-Z0-9_-]+)((?:\s+[a-zA-Z0-9_-]+)*)\)")
_SECTION_RE = lambda name: re.compile(r"\(:" + name + r"\b")


def parse_pddl(text: str) -> dict:
    """PDDL problem → {problem, objects, init_atoms, goal_atoms}（确定性正则解析）。

    section 截取按匹配起点切片（(:init 到 (:goal 起点；(:goal 到文末），
    避免 init/goal 双计。
    """
    problem = re.search(r"\(define\s*\(problem\s+([^\s)]+)", text)
    objects_m = _SECTION_RE("objects").search(text)
    init_m = _SECTION_RE("init").search(text)
    goal_m = _SECTION_RE("goal").search(text)
    goal_start = goal_m.start() if goal_m else len(text)
    objects = _ATOM_RE.findall(objects_m.group(0)) if objects_m else []
    obj_tokens = [t for _, ts in objects for t in ts.split()]

    def atoms(section: str) -> list[tuple[str, list[str]]]:
        out = []
        for pred, args in _ATOM_RE.findall(section):
            args_l = args.split()
            if pred not in ("and", "not", "exists", "forall") and args_l:
                out.append((pred, args_l))
        return out

    init_section = text[init_m.start():goal_start] if init_m else ""
    goal_section = text[goal_m.start():] if goal_m else ""
    return {
        "problem": problem.group(1) if problem else "unknown",
        "objects": obj_tokens,
        "init_atoms": atoms(init_section),
        "goal_atoms": atoms(goal_section),
    }


def build_blocksworld_nodes(goal_atoms: list[tuple[str, list[str]]]) -> tuple[list[dict], list[dict]]:
    """goal (on X Y) 原子集 → (nodes, edges)；依赖：on X Y 依赖 on Y Z（若存在）。"""
    ons = [(a[0], a[1]) for p, a in goal_atoms if p == "on" and len(a) == 2]
    below = {x: y for x, y in ons}
    nodes = [
        {
            "id": f"on_{x}_{y}",
            "description": f"on {x} {y}",
            "task_type": "general",
            "depends_on": [],
            "constraints": [],
            "model": None,
            "estimated_output_tokens": 128,
            "duration_ms": None,
        }
        for x, y in ons
    ]
    edges = []
    for node in nodes:
        _, x, y = node["id"].split("_", 2)
        if y in below:
            dep_id = f"on_{y}_{below[y]}"
            node["depends_on"].append(dep_id)
            edges.append({"from": dep_id, "to": node["id"], "type": "data"})
    return nodes, edges


def build_flat_nodes(goal_atoms: list[tuple[str, list[str]]]) -> tuple[list[dict], list[dict]]:
    """goal 原子（如 logistics 的 at o l）→ 全独立节点（零依赖参照）。"""
    nodes = []
    for i, (pred, args) in enumerate(goal_atoms):
        nodes.append(
            {
                "id": f"g{i + 1:02d}",
                "description": f"{pred} {' '.join(args)}",
                "task_type": "general",
                "depends_on": [],
                "constraints": [],
                "model": None,
                "estimated_output_tokens": 128,
                "duration_ms": None,
            }
        )
    return nodes, []


def canonicalize(domain: str, variant: str, instance_name: str, parsed: dict) -> dict | None:
    goal_atoms = parsed["goal_atoms"]
    if not goal_atoms:
        return None
    goal_text = "; ".join(f"({p} {' '.join(a)})" for p, a in goal_atoms)
    if domain == "blocksworld":
        nodes, edges = build_blocksworld_nodes(goal_atoms)
        node_semantics = "goal_stack_dependency"
    else:
        nodes, edges = build_flat_nodes(goal_atoms)
        node_semantics = "goal_atom_flat"
    sid = f"ob_planbench_{domain}_{variant}_{instance_name}"
    return {
        "workflow_id": sid,
        "schema_version": 1,
        "root_goal": goal_text,
        "nodes": nodes,
        "edges": edges,
        "waves": None,
        "aggregation": {"type": "final_synthesis"},
        "meta": {
            "source_benchmark": "planbench",
            "source_id": parsed["problem"],
            "domain": domain,
            "variant": variant,
            "node_semantics": node_semantics,
            "goal_atom_count": len(goal_atoms),
            "duration_basis": None,
        },
    }


def split_of(sample_id: str, seed: int = SPLIT_SEED, ratio: float = CALIBRATION_RATIO) -> str:
    """哈希桶确定性切分（RouterBench import_frozen 同公式，禁 random 模块）。"""
    digest = hashlib.sha256(f"lloom-obsplit:{seed}:{sample_id}".encode("utf-8")).digest()
    return "calibration" if int.from_bytes(digest[:16], "big") / 2**128 < ratio else "test"


def iter_problem_files(root: Path):
    """枚举 (domain, variant, instance_name, path)；iterdir 递归，不用 glob 模式注入面。"""
    instances_root = root / "plan-bench" / "instances"
    if not instances_root.is_dir():
        raise SystemExit(f"✗ 未找到 instances 目录: {instances_root}")
    for domain_dir in sorted(instances_root.iterdir()):
        if not domain_dir.is_dir() or domain_dir.name.startswith("."):
            continue
        for variant_dir in sorted(domain_dir.iterdir()):
            if not variant_dir.is_dir() or variant_dir.name.startswith("."):
                continue
            for f in sorted(variant_dir.iterdir()):
                if f.is_file() and f.suffix == ".pddl":
                    yield domain_dir.name, variant_dir.name, f.stem, f


def run_scan(root: Path, limit: int | None) -> dict:
    stats: dict = {"scanned": 0, "imported": 0, "skipped_no_goal": 0, "rejected": 0, "reject_examples": [], "per_variant": Counter()}
    for domain, variant, inst, path in iter_problem_files(root):
        if limit is not None and stats["scanned"] >= limit:
            break
        parsed = parse_pddl(path.read_text(encoding="utf-8", errors="replace"))
        stats["scanned"] += 1
        wf = canonicalize(domain, variant, inst, parsed)
        if wf is None:
            stats["skipped_no_goal"] += 1
            continue
        issues = validate_workflow(wf, strict_meta=True)
        if issues:
            stats["rejected"] += 1
            if len(stats["reject_examples"]) < 5:
                stats["reject_examples"].append({"workflow_id": wf["workflow_id"], "issues": [str(i) for i in issues]})
            continue
        stats["imported"] += 1
        stats["per_variant"][f"{domain}/{variant}"] += 1
    stats["per_variant"] = dict(sorted(stats["per_variant"].items()))
    return stats


def run_import(root: Path, db_path: Path, limit: int | None) -> dict:
    """扫描 + 落库；每 (domain/variant) 确定性截断 MAX_PER_VARIANT（文件序升序取前 N）。

    幂等导入：先清同 benchmark_id 旧行再插，db 状态恒等于当前代码状态。
    """
    stats = run_scan(root, limit)
    if stats["imported"] == 0:
        return stats
    conn = sqlite3.connect(db_path)
    try:
        sql_upsert = "INSERT OR REPLACE INTO bench_workflows (benchmark_id, sample_id, root_goal, workflow_json, reference_graph_json, verification_json, split, meta_json) VALUES (?,?,?,?,?,?,?,?)"
        sql_count = "SELECT COUNT(*) FROM bench_workflows WHERE benchmark_id = ?"
        sql_clear = "DELETE FROM bench_workflows WHERE benchmark_id = ?"
        params_clear = (BENCHMARK_ID,)
        conn.execute(sql_clear, params_clear)
        seen: Counter[str] = Counter()
        for domain, variant, inst, path in iter_problem_files(root):
            variant_key = f"{domain}/{variant}"
            if seen[variant_key] >= MAX_PER_VARIANT:
                continue
            parsed = parse_pddl(path.read_text(encoding="utf-8", errors="replace"))
            wf = canonicalize(domain, variant, inst, parsed)
            if wf is None or validate_workflow(wf, strict_meta=True):
                continue
            seen[variant_key] += 1
            sid = wf["workflow_id"]
            ref_graph = {"nodes": [n["id"] for n in wf["nodes"]], "edges": [(e["from"], e["to"]) for e in wf["edges"]]}
            params = (
                BENCHMARK_ID,
                sid,
                wf["root_goal"],
                json.dumps(wf, ensure_ascii=False),
                json.dumps(ref_graph, ensure_ascii=False),
                None,
                split_of(sid),
                json.dumps(wf["meta"], ensure_ascii=False),
            )
            conn.execute(sql_upsert, params)
        conn.commit()
        params_count = (BENCHMARK_ID,)
        stats["db_rows"] = conn.execute(sql_count, params_count).fetchone()[0]
        stats["imported_after_cap"] = sum(seen.values())
        stats["per_variant_capped"] = dict(sorted(seen.items()))
    finally:
        conn.close()
    return stats


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--input", default="raw/PlanBench")
    ap.add_argument("--db", default=str(HERE.parent / "orchbench.db"))
    ap.add_argument("--import", dest="do_import", action="store_true")
    ap.add_argument("--limit", type=int, default=None)
    args = ap.parse_args()

    root = Path(args.input)
    if not root.exists():
        raise SystemExit(f"✗ 找不到输入: {root}（先克隆 raw/PlanBench，见 datasets.yaml）")
    if args.do_import:
        db = Path(args.db)
        if not db.exists():
            raise SystemExit(f"✗ 找不到 {db}（先 python3 scripts/db_init.py）")
        if db.name == "lloom.db" or "data" in db.parts:
            raise SystemExit("✗ 拒绝：orchbench 只允许独立库")
        stats = run_import(root, db, args.limit)
    else:
        stats = run_scan(root, args.limit)

    print(f"scanned={stats['scanned']} imported={stats.get('imported_after_cap', stats['imported'])} "
          f"no_goal={stats['skipped_no_goal']} rejected={stats['rejected']}")
    print("per_variant:", stats["per_variant"])
    for ex in stats["reject_examples"]:
        print("REJECT", ex["workflow_id"], ex["issues"])
    if args.do_import:
        print(f"db_rows={stats['db_rows']} ({BENCHMARK_ID})")
    return 0 if stats["rejected"] == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
