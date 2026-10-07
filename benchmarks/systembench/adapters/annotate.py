#!/usr/bin/env python3
"""annotate.py — Semantic Workflow 标注器（S12.1/S12.3，workflow_annotation_v1.yaml）。

两步：
  1. 自动预标注：对 S11 seeds 的每个 node，按 description 关键词推断 semantic_role
     （启发式，confidence 低时标 unclassified 交人工）；depends_on → consumes 边投影；
     depth/width 统计。
  2. 人工复核文件生成：unclassified/低置信节点汇总，供人工补标（结果回填 _manual.json 后
     重跑 --merge 合并）。

红线：
  - exact binding 不动（node prompt 原样，artifact_contract 标注不改 prompt）；
  - 不引入 random；一切确定性。

用法：
  benchmarks/routerbench/.venv/bin/python benchmarks/systembench/adapters/annotate.py \
    --seeds benchmarks/systembench/reports/s11_decomposition_pilot.json \
    --out benchmarks/systembench/cases/semantic_seed_audit.json
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "orchestration"))

ROLE_KEYWORDS = [
    ("calculation", ["计算", "求和", "总和", "统计", "汇总数", "笔画", "换算", "得出"]),
    ("verification", ["校验", "合规", "检查", "核验", "对账", "一致性", "审核", "初筛", "验证"]),
    ("extraction", ["提取", "解析", "抽取", "识别出", "筛选出"]),
    ("classification", ["分类", "归类", "判定", "区分", "评级"]),
    ("retrieval", ["查询", "检索", "调取", "获取", "读取"]),
    ("transformation", ["转换", "翻译", "改写", "格式化", "生成表", "整理成"]),
    ("synthesis", ["合并", "汇总成", "整合", "总结成", "合成", "聚合"]),
    ("reasoning", ["规划", "权衡", "评估", "分析", "设计", "推理", "决策", "排定", "制定"]),
    ("execution", ["输出最终", "生成最终", "执行", "落地方案", "交付"]),
    ("search", ["枚举", "探索", "备选方案", "候选"]),
]
ROLE_ORDER = [r for r, _ in ROLE_KEYWORDS]


def infer_role(description: str) -> tuple[str, float]:
    """关键词命中打分：返回 (role, confidence=hits/len(ROLE_ORDER) 归一)。低置信 → unclassified。"""
    hits: dict[str, int] = {}
    for role, kws in ROLE_KEYWORDS:
        hits[role] = sum(1 for kw in kws if kw in description)
    ranked = sorted(hits.items(), key=lambda kv: -kv[1])
    if not ranked or ranked[0][1] == 0:
        return "unclassified", 0.0
    role, top = ranked[0]
    second = ranked[1][1] if len(ranked) > 1 else 0
    conf = round(min(1.0, (top - 0.5 * second) / 3.0), 2)
    return (role, conf) if conf >= 0.2 else ("unclassified", conf)


def depth_width(case: dict) -> dict:
    deps: dict[str, list[str]] = {n["node_id"]: [] for n in case["nodes"]}
    for e in case["edges"]:
        deps[e["to"]].append(e["from"])
    depth = {}
    def d(nid):
        if nid not in depth:
            depth[nid] = 1 + max((d(x) for x in deps[nid]), default=0)
        return depth[nid]
    for n in nodes_ids(case):
        d(n)
    widths: dict[int, int] = defaultdict(int)
    for n in nodes_ids(case):
        widths[depth[n]] += 1
    return {"depth": max(depth.values()), "max_width": max(widths.values()),
            "dependency_density": round(len(case["edges"]) / max(len(case["nodes"]), 1), 2)}


def nodes_ids(case: dict) -> list[str]:
    return [n["node_id"] for n in case["nodes"]]


def annotate_workflow(wf: dict, case_class: str) -> dict:
    deps: dict[str, list[str]] = {n["id"]: [] for n in wf["nodes"]}
    # planner 的 node.depends_on（canonical 化时已写入）优先；否则从 edges 投影
    for n in wf["nodes"]:
        if n.get("depends_on"):
            deps[n["id"]] = list(n["depends_on"])
    for e in wf.get("edges") or []:
        if e["from"] not in deps[e["to"]]:
            deps[e["to"]].append(e["from"])

    nodes_out = []
    for n in wf["nodes"]:
        role, conf = infer_role(n["description"])
        consumes = [{"type": "upstream_artifact", "from": deps[n["id"]], "required": True}] if deps[n["id"]] else []
        nodes_out.append({
            "node_id": n["id"],
            "semantic_role": role,
            "role_confidence": conf,
            "atomic_ok": "pending_manual",
            "dependency_semantics": "artifact_dependent" if deps[n["id"]] else "independent",
            "artifact_contract": {
                "produces": [{"type": "upstream_artifact", "required": True}],
                "consumes": consumes,
            },
            "description_excerpt": n["description"][:60],
        })
    # artifact type 精化：按上游 role 推断 produces 类型（calculation→calculation 等）
    role_to_artifact = {
        "calculation": "calculation", "extraction": "facts", "retrieval": "facts",
        "verification": "verification_report", "synthesis": "draft_content",
        "reasoning": "analysis", "classification": "facts", "transformation": "draft_content",
        "search": "facts", "execution": "structured_record", "unclassified": "upstream_artifact",
    }
    role_by_node = {n["node_id"]: n["semantic_role"] for n in nodes_out}
    for n in nodes_out:
        for dep in deps.get(n["node_id"], []):
            for c in n["artifact_contract"]["consumes"]:
                if dep in c["from"]:
                    c["type"] = role_to_artifact.get(role_by_node.get(dep, ""), "upstream_artifact")

    dw = depth_width({"nodes": [{"node_id": n["id"]} for n in wf["nodes"]],
                      "edges": wf["edges"]})
    unclassified = sum(1 for n in nodes_out if n["semantic_role"] == "unclassified")
    return {
        "workflow": wf,
        "annotations": nodes_out,
        "structure": dw,
        "needs_manual": unclassified > 0,
        "unclassified_count": unclassified,
        "case_class_suggest": ("simple" if len(wf["nodes"]) <= 2 else
                               "medium" if len(wf["nodes"]) <= 5 else "complex"),
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--seeds", default="benchmarks/systembench/reports/s11_decomposition_pilot.json")
    ap.add_argument("--out", default="benchmarks/systembench/reports/semantic_seed_audit.json")
    args = ap.parse_args()

    pilot = json.loads(Path(args.seeds).read_text(encoding="utf-8"))
    audit_rows = []
    for seed in pilot.get("semantic_workflow_seeds", []):
        wf = seed["workflow"]
        a = annotate_workflow(wf, case_class="medium")
        audit_rows.append({
            "sample_id": seed["sample_id"],
            "case_class_suggest": a["case_class_suggest"],
            "structure": a["structure"],
            "needs_manual": a["needs_manual"],
            "unclassified_count": a["unclassified_count"],
            "annotations": a["annotations"],
        })

    total_nodes = sum(len(r["annotations"]) for r in audit_rows)
    role_dist: dict[str, int] = defaultdict(int)
    for r in audit_rows:
        for a in r["annotations"]:
            role_dist[a["semantic_role"]] += 1
    need_manual_n = sum(1 for r in audit_rows if r["needs_manual"])

    report = {
        "generated_at": __import__("datetime").datetime.now(__import__("datetime").timezone.utc).isoformat(timespec="seconds"),
        "spec_version": "workflow_annotation_v1",
        "n_seeds": len(audit_rows),
        "n_nodes": total_nodes,
        "role_distribution": dict(sorted(role_dist.items(), key=lambda kv: -kv[1])),
        "workflows_needing_manual": need_manual_n,
        "audits": audit_rows,
        "notes": [
            "semantic_role 为关键词启发式预标注（confidence<0.2 → unclassified 待人工）",
            "artifact_contract.type 按上游 role 推断（初版）；consumes.from 与 edges 严格一致",
            "dependency_semantics：有上游=artifact_dependent（L2 范围），无=independent（L1 counterfactual）",
            "atomicity 判定 pending_manual——人工复核后回填",
        ],
    }
    out = Path(args.out)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    print(f"✓ {out}")
    print(f"seeds={len(audit_rows)} nodes={total_nodes} 需人工={need_manual_n}")
    print(f"role 分布: {dict(sorted(role_dist.items(), key=lambda kv: -kv[1]))}")
    return 0


if __name__ == "__main__":
    main()
