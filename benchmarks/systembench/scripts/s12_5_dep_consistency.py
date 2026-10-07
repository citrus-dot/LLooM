#!/usr/bin/env python3
"""s12_5_dep_consistency.py — S12.5 前置：semantic 节点依赖语义全库审计与确定性修复。

背景（2026-10-07 S12.5.0 前置审计发现，s12_composite_split.py 拆分遗留）：
  A. 拆分首子节点（*_s1）上游依赖被重接到自身（入边+consumes 非空），但节点级
     dependency_semantics 仍为 independent —— 违反 spec v1：sem 是 artifact_contract
     的投影（consumes.from 非空 ⇒ artifact_dependent）。
  B. dangling consumes 引用：拆分后节点 consumes.from 仍引用拆分前的父 id
     （如 023 的 n2 consumes=['n1']，n1 已拆为 n1_s1/s2/s3）—— 重映射规则：
     父组最终产物 = 拆分链末子节点（拓扑序最后一个孩子，组的最终输出）。

规则（确定性，spec §dependency_semantics）：
  consumes.from 非空            → artifact_dependent（进 L2 conditional cells）
  consumes.from 空 且 无入边    → independent（进 L1 cells）
  有入边但 consumes 空          → 投影不一致，计数报告（不自动改，人工看）
  state_dependent               → 全库应为 0（无 L3 节点；有则报错停）

只动 semantic case（case_class=semantic_workflow）；control case 不触碰。
写回 system_cases_v02.jsonl + annotations_v02.jsonl（同字段回填），并在
builder_manifest_v02.json 追加 dependency_fix 记录（hash 以 S12.7 freeze 为准）。

用法（仓库根目录）：
  benchmarks/routerbench/.venv/bin/python \
    benchmarks/systembench/scripts/s12_5_dep_consistency.py --audit   # 只审计
  benchmarks/routerbench/.venv/bin/python \
    benchmarks/systembench/scripts/s12_5_dep_consistency.py --fix     # 审计+修复写回
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

_HERE = Path(__file__).resolve().parent
CASES = _HERE.parent / "cases" / "system_cases_v02.jsonl"
ANNO = _HERE.parent / "cases" / "annotations_v02.jsonl"
SPLIT_REPORT = _HERE.parent / "reports" / "s12_composite_split.json"
BUILDER_MANIFEST = _HERE.parent / "cases" / "builder_manifest_v02.json"


def load_jsonl(path: Path) -> list[dict]:
    text = path.read_text(encoding="utf-8")
    return [json.loads(l) for l in text.split("\n") if l.strip()]


def dump_jsonl(path: Path, rows: list[dict]) -> None:
    path.write_text("\n".join(json.dumps(r, ensure_ascii=False) for r in rows) + "\n",
                    encoding="utf-8")


def build_split_index() -> dict[str, dict[str, str]]:
    """case_id → {父节点id: 拆分链末子节点}。

    splits[i].node_key = "case_id/父节点id"；子节点按 s12_composite_split 的
    {parent}_s{i} 模式生成（i 从 1 到 len(split.subtasks)），链末 = 组的最终输出。"""
    index: dict[str, dict[str, str]] = {}
    if not SPLIT_REPORT.exists():
        return index
    rep = json.loads(SPLIT_REPORT.read_text(encoding="utf-8"))
    for r in rep.get("splits", []):
        if r.get("status") != "split_ok" and "split_ok" not in str(r.get("status", "")):
            continue
        nk = r.get("node_key", "")
        if "/" not in nk:
            continue
        case_id, parent_id = nk.rsplit("/", 1)
        n = len((r.get("split") or {}).get("subtasks") or [])
        if n <= 0:
            continue
        index.setdefault(case_id, {})[parent_id] = f"{parent_id}_s{n}"
    return index


def audit_and_fix(fix: bool) -> int:
    cases = load_jsonl(CASES)
    annos = {(a["case_id"], a["node_id"]): a
             for a in load_jsonl(ANNO)} if ANNO.exists() else {}
    split_index = build_split_index()

    # node_key → node_id（同 case 内唯一；node_id 全局不唯一，先建 case 内映射）
    problems: list[str] = []
    fixes = {"sem_upgraded": 0, "dangling_remapped": 0, "edge_no_consumes": 0, "state_dep": 0}
    for c in cases:
        if c.get("case_role") != "headline_candidate":
            continue  # semantic workflow cases；control 不触碰
        ids = {n["node_id"] for n in c["nodes"]}
        incoming = {e["to"] for e in c.get("edges", [])}
        for n in c["nodes"]:
            cid = c["system_case_id"]
            key = f"{cid}:{n['node_id']}"
            anno = annos.get((cid, n["node_id"]))
            ac = n.get("artifact_contract") or {}
            cons = ac.get("consumes") or []
            # consumes 规范形态：[{"type": str, "from": [node_id...], "required": bool}]；
            # 容忍 list/str 项。flat = 全部 from 的并集（sem 判定用）。
            flat: list[str] = []
            for item in cons:
                if isinstance(item, dict):
                    flat.extend(item.get("from") or [])
                elif isinstance(item, list):
                    flat.extend(item)
                else:
                    flat.append(item)

            def map_src(src: str) -> tuple[str, bool]:
                """(映射后 id, 是否 unresolved)。dangling 父引用 → 拆分链末子节点。"""
                if src in ids:
                    return src, False
                last = split_index.get(cid, {}).get(src)
                if last is None or last not in ids:
                    return src, True
                return last, False

            remapped_flat: list[str] = []
            unresolved: list[str] = []
            for src in flat:
                m, un = map_src(src)
                remapped_flat.append(m)
                if un:
                    unresolved.append(m)
                    problems.append(f"{key}: dangling consumes.from={src} 且无拆分记录")
                elif m != src:
                    fixes["dangling_remapped"] += 1
            # B 写回：保持 dict 项结构，仅替换 from 内元素
            if fix and remapped_flat != flat:
                new_cons = []
                ri = 0
                for item in cons:
                    if isinstance(item, dict):
                        k = len(item.get("from") or [])
                        item = dict(item)
                        item["from"] = remapped_flat[ri:ri + k]
                        ri += k
                    elif isinstance(item, list):
                        k = len(item)
                        item = remapped_flat[ri:ri + k]
                        ri += k
                    else:
                        item = remapped_flat[ri]
                        ri += 1
                    new_cons.append(item)
                ac["consumes"] = new_cons
                if anno is not None:
                    aac = anno.get("artifact_contract")
                    if isinstance(aac, dict):
                        aac["consumes"] = new_cons
            elif fix and not remapped_flat:
                ac["consumes"] = []
            # A: sem 重算（spec §dependency_semantics：sem 是 artifact_contract 的投影）
            old_sem = n.get("dependency_semantics")
            if old_sem == "state_dependent":
                fixes["state_dep"] += 1
                problems.append(f"{key}: state_dependent 节点出现在 v0.2（L3 未建）")
            new_sem = "artifact_dependent" if remapped_flat else "independent"
            if n["node_id"] in incoming and not remapped_flat:
                fixes["edge_no_consumes"] += 1
            if old_sem != new_sem:
                problems.append(
                    f"{key}: sem {old_sem} -> {new_sem} (consumes={remapped_flat})")
                if fix:
                    n["dependency_semantics"] = new_sem
                    if anno is not None:
                        anno["dependency_semantics"] = new_sem
                fixes["sem_upgraded"] += 1

    print(f"审计: {len(problems)} 个问题 | 修复计数: {fixes}")
    for p in problems[:60]:
        print("  -", p)
    if len(problems) > 60:
        print(f"  ...（其余 {len(problems) - 60} 条省略）")

    if fix:
        dump_jsonl(CASES, cases)
        if annos:
            dump_jsonl(ANNO, list(annos.values()))
        if BUILDER_MANIFEST.exists():
            m = json.loads(BUILDER_MANIFEST.read_text(encoding="utf-8"))
            m["dependency_fix_2026_10_07"] = {
                "script": "s12_5_dep_consistency.py --fix",
                "sem_upgraded": fixes["sem_upgraded"],
                "dangling_remapped": fixes["dangling_remapped"],
                "edge_no_consumes_flagged": fixes["edge_no_consumes"],
                "rule": "consumes.from 非空→artifact_dependent；dangling 父引用→拆分链末子节点",
                "note": "hash 以 S12.7 freeze 快照为准",
            }
            BUILDER_MANIFEST.write_text(
                json.dumps(m, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print("✓ 已写回 cases + annotations + builder_manifest（dependency_fix 记录）")
    else:
        print("（--audit 只读模式，未写回；确认后加 --fix）")
    return 1 if any("无拆分记录" in p for p in problems) else 0


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument("--audit", action="store_true")
    g.add_argument("--fix", action="store_true")
    args = ap.parse_args()
    sys.exit(audit_and_fix(fix=args.fix))
