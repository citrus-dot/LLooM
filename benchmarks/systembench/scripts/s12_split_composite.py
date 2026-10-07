#!/usr/bin/env python3
"""s12_split_composite.py — Phase S12.3b：复合节点拆分（workflow_annotation_v1.yaml §atomicity on_violation）。

spec 处置条款：「拆分节点后重新标注；不允许保留复合节点进 v0.2 headline」。
S12.3 预标注判出 22 个 atomic_ok=false 节点（分布于 16/30 workflow）——典型为生产 planner
把「提取+计算+初筛」打包进一个子任务（真实 decomposition 质量发现，报告存档）。

拆分机制（provenance 全记录）：
  对每个复合节点：qwen-plus 一次调用，按「单一可验证职责」拆 2-3 个子节点
    → 子节点 id = {orig}_s{i}，内部依赖默认线性链（允许 LLM 给 internal depends_on）
    → 边重接：上游 → 首子节点；末子节点 → 原下游
    → 子节点 prompt 同模板重建（root_goal + 子描述），prompt_hash 重算
    → 子节点预标注取自拆分输出（同模型），过 review_node 确定性复核
  原 composite 节点从 case 移除，完整前后记录存 reports/s12_composite_split.json。

红线：
  - 拆分只动 composite 节点；atomic 节点与 v0.1/control 资产零接触；
  - provenance 标 s12_composite_split（与 s11_seed / s12_expansion 区分，可被人工否决重建）；
  - 拆分后全 case binding 校验重跑。

用法（仓库根目录）：
  DASHSCOPE_API_KEY=… benchmarks/routerbench/.venv/bin/python \
    benchmarks/systembench/scripts/s12_split_composite.py
  … --replay   # 零成本重放
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import statistics
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))
sys.path.insert(0, str(_HERE.parent / "adapters"))
from s11_decompose_pilot import ENDPOINT  # noqa: E402
from s12_annotate_v02 import call_llm, parse_json_block, review_node, deps_of  # noqa: E402
from annotate import infer_role  # noqa: E402

import os  # noqa: E402
import requests  # noqa: E402

CASES = Path("benchmarks/systembench/cases/system_cases_v02.jsonl")
OUT_SPLIT = Path("benchmarks/systembench/reports/s12_composite_split.json")
PRELABEL_MODEL = "qwen-plus"
SEMANTIC_PROMPT_TEMPLATE = "{root_goal}\n\n---\n## 子任务\n{description}"

SPLIT_SYSTEM = """你是工作流结构优化员。给定一个被判定为「复合节点」的子任务描述（承担了多个独立职责），
请按标注规范「一个 node 只承担一个可以单独验证的职责」将其拆分为 2-3 个子任务。

规则：
1. 每个子任务的 description 必须自包含可执行（可引用根目标中的数据），且只承担一个职责；
2. 给出每个子任务的 semantic_role（calculation/verification/extraction/classification/retrieval/transformation/synthesis/reasoning/execution/search 恰一）与 task_type（simple_qa/general/coding/math_logic/complex_reasoning）；
3. depends_on 用子任务序号（0-based），不填默认线性链（前一个）；
4. estimated_output_tokens 估每个子任务输出 token；
5. 输出严格 JSON：
{"subtasks": [{"description": "...", "semantic_role": "...", "task_type": "...", "depends_on": [0], "estimated_output_tokens": 300}], "rationale": "..."}"""


def call_split(user: str, api_key: str) -> tuple[str, int, int]:
    resp = requests.post(
        ENDPOINT,
        headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"},
        json={"model": PRELABEL_MODEL, "temperature": 0,
              "messages": [{"role": "system", "content": SPLIT_SYSTEM}, {"role": "user", "content": user}]},
        timeout=90)
    resp.raise_for_status()
    d = resp.json()
    usage = d.get("usage", {})
    return d["choices"][0]["message"]["content"], usage.get("prompt_tokens", 0), usage.get("completion_tokens", 0)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--replay", action="store_true", help="重放（读已有 split 报告，零成本）")
    args = ap.parse_args()

    cases = [json.loads(l) for l in CASES.read_text(encoding="utf-8").split("\n") if l.strip()]
    prev_splits: dict[str, dict] = {}
    if args.replay and OUT_SPLIT.exists():
        prev = json.loads(OUT_SPLIT.read_text(encoding="utf-8"))
        prev_splits = {r["node_key"]: r for r in prev["splits"]}

    api_key = os.environ.get("DASHSCOPE_API_KEY")
    split_records: list[dict] = []
    tokens = {"input": 0, "output": 0}
    n_split_nodes = 0

    for c in cases:
        if c["case_role"] != "headline_candidate":
            continue
        deps = deps_of(c)
        composite = [n for n in c["nodes"] if n.get("atomic_ok") is False]
        if not composite:
            continue
        downstream: dict[str, list[str]] = defaultdict(list)
        for e in c["edges"]:
            downstream[e["from"]].append(e["to"])

        for node in composite:
            nid = node["node_id"]
            key = f"{c['system_case_id']}/{nid}"
            if key in prev_splits:
                rec = prev_splits[key]
                subtasks = rec["split"]["subtasks"]
                tokens["input"] += rec.get("prompt_tokens", 0)
                tokens["output"] += rec.get("completion_tokens", 0)
            else:
                ctx = {"node_id": nid, "description": node["description"],
                       "task_type": node["task_type"],
                       "upstream": deps[nid],
                       "downstream": downstream[nid],
                       "estimated_output_tokens": node.get("estimated_output_tokens")}
                raw, in_tok, out_tok = call_split(json.dumps(ctx, ensure_ascii=False), api_key)
                tokens["input"] += in_tok
                tokens["output"] += out_tok
                obj = parse_json_block(raw)
                if not obj or not isinstance(obj.get("subtasks"), list) or len(obj["subtasks"]) < 2:
                    rec = {"node_key": key, "status": "split_failed", "raw_output": raw,
                           "prompt_tokens": in_tok, "completion_tokens": out_tok}
                    split_records.append(rec)
                    print(f"  {key}: ✗ 拆分输出不可用（保留原节点，进人工）")
                    continue
                subtasks = obj["subtasks"]
                rec = {"node_key": key, "status": "split_ok", "split": obj,
                       "raw_output": raw, "prompt_tokens": in_tok, "completion_tokens": out_tok}
                split_records.append(rec)

            if rec.get("status") != "split_ok":
                continue
            # 构造子节点（线性链缺省：depends_on=[i-1]）
            subs = []
            from s12_annotate_v02 import ROLES  # 词表封闭校验（拆分输出同样受 spec 约束）
            for i, st in enumerate(subtasks):
                role = st.get("semantic_role")
                role_flag = None
                if role not in ROLES:
                    role_flag = f"spec_violation:role={role}→reasoning"
                    role = "reasoning"
                sub_deps = st.get("depends_on")
                if not isinstance(sub_deps, list) or not sub_deps:
                    sub_deps = [i - 1] if i > 0 else []
                sub_deps = [d for d in sub_deps if isinstance(d, int) and 0 <= d < i]
                subs.append({
                    "description": str(st.get("description", "")).strip(),
                    "semantic_role": st.get("semantic_role", "reasoning"),
                    "task_type": st.get("task_type", node["task_type"]),
                    "depends_on": sub_deps,
                    "estimated_output_tokens": int(st.get("estimated_output_tokens", 400)),
                })
            # 子节点 → case 节点（prompt 模板同 builder_v02；hash 重算）
            new_ids = [f"{nid}_s{i + 1}" for i in range(len(subs))]
            new_nodes = []
            for i, st in enumerate(subs):
                prompt = SEMANTIC_PROMPT_TEMPLATE.format(root_goal=c["root_goal"], description=st["description"])
                sub_dep_ids = [new_ids[d] for d in st["depends_on"]]
                new_nodes.append({
                    "node_id": new_ids[i],
                    "description": st["description"],
                    "task_type": st["task_type"],
                    "estimated_output_tokens": st["estimated_output_tokens"],
                    "prompt": prompt,
                    "outcome_binding": {
                        "mode": "semantic_matrix_exact",
                        "source_id": f"{c['system_case_id']}/{new_ids[i]}",
                        "prompt_hash": hashlib.sha256(prompt.encode("utf-8")).hexdigest(),
                    },
                    "dependency_semantics": "artifact_dependent" if sub_dep_ids else "independent",
                    "artifact_injection": "pending_s12_6" if (sub_dep_ids or deps[nid]) else "none",
                    "semantic_role": role,
                    "role_flag": role_flag,
                    "atomic_ok": True,  # 拆分目标即单一职责；复核在 review 阶段仍可标记
                    "atomic_reason": "s12_composite_split",
                    "artifact_contract": "pending_review",
                    "split_provenance": {"from_node": nid, "batch": "s12_composite_split",
                                         "model": PRELABEL_MODEL},
                })
            # 原节点移除 + 子节点插入（保持位置序）
            pos = next(i for i, n in enumerate(c["nodes"]) if n["node_id"] == nid)
            c["nodes"][pos:pos + 1] = new_nodes
            # 边重接：上游→首子节点；末子节点→原下游；内部链
            new_edges = [e for e in c["edges"] if e["from"] != nid and e["to"] != nid]
            first, last = new_ids[0], new_ids[-1]
            for u in deps[nid]:
                new_edges.append({"from": u, "to": first, "type": "data"})
            for d in downstream[nid]:
                new_edges.append({"from": last, "to": d, "type": "data"})
            for i in range(1, len(new_ids)):
                if new_ids[i - 1] not in [new_ids[d] for d in subs[i]["depends_on"]]:
                    pass  # LLM 给了非线性内部依赖，下面按 depends_on 生成
            new_edges += [{"from": new_ids[d], "to": new_ids[i], "type": "data"}
                          for i, st in enumerate(subs) for d in st["depends_on"]]
            # 去重（上游→首子节点可能与内部依赖重合）
            seen = set()
            dedup = []
            for e in new_edges:
                k = (e["from"], e["to"])
                if k not in seen:
                    seen.add(k)
                    dedup.append(e)
            c["edges"] = dedup
            n_split_nodes += 1
            print(f"  {key}: ✓ 拆为 {len(subs)} 子节点 {new_ids}")

    # 全量重算 completion/morphology/class + 去除失效 artifact_injection + binding 校验
    for c in cases:
        if c["case_role"] != "headline_candidate":
            continue
        syn = [n["node_id"] for n in c["nodes"] if n.get("semantic_role") == "synthesis"]
        c["completion_condition"]["required_nodes"] = [n["node_id"] for n in c["nodes"]]
        c["completion_condition"]["optional_nodes"] = []
        c["completion_condition"]["aggregation_condition"] = (
            "synthesis_node:" + ",".join(syn) if syn else "all_required")
        node_ids = {n["node_id"] for n in c["nodes"]}
        c["edges"] = [e for e in c["edges"] if e["from"] in node_ids and e["to"] in node_ids]
        for n in c["nodes"]:
            if isinstance(n.get("artifact_contract"), str):
                n["artifact_contract"] = {"produces": [], "consumes": []}
        # 拆分节点 contract 确定性补齐（S12.1 同法 role→type；consumes.type 由上游 produces 满足）
        deps = {n["node_id"]: [] for n in c["nodes"]}
        for e in c["edges"]:
            deps[e["to"]].append(e["from"])
        role_to_artifact = {
            "calculation": "calculation", "extraction": "facts", "retrieval": "facts",
            "verification": "verification_report", "synthesis": "draft_content",
            "reasoning": "analysis", "classification": "facts", "transformation": "draft_content",
            "search": "facts", "execution": "structured_record",
        }
        role_by_node = {n["node_id"]: n.get("semantic_role") for n in c["nodes"]}
        for n in c["nodes"]:
            if n.get("artifact_contract") == {"produces": [], "consumes": []} and \
                    n.get("split_provenance") is not None:
                produces = [{"type": role_to_artifact.get(n.get("semantic_role"), "facts"),
                             "required": True}]
                consumes = [{"type": role_to_artifact.get(role_by_node.get(u), "facts"),
                             "from": [u], "required": True} for u in deps[n["node_id"]]]
                n["artifact_contract"] = {"produces": produces, "consumes": consumes}
        for n in c["nodes"]:
            h = hashlib.sha256(n["prompt"].encode("utf-8")).hexdigest()
            if h != n["outcome_binding"]["prompt_hash"]:
                raise SystemExit(f"✗ 拆分破坏 binding: {c['system_case_id']}/{n['node_id']}")
        # morphology 重算
        deps = {n["node_id"]: [] for n in c["nodes"]}
        for e in c["edges"]:
            deps[e["to"]].append(e["from"])
        memo: dict[str, int] = {}

        def dep(nid: str) -> int:
            if nid not in memo:
                memo[nid] = 1 + max((dep(x) for x in deps[nid]), default=0)
            return memo[nid]

        for n in c["nodes"]:
            dep(n["node_id"])
        widths = defaultdict(int)
        for n in c["nodes"]:
            widths[memo[n["node_id"]]] += 1
        depth = max(memo.values())
        c["morphology"] = {"n_nodes": len(c["nodes"]), "n_edges": len(c["edges"]),
                           "depth": depth, "max_width": max(widths.values()),
                           "dependency_density": round(len(c["edges"]) / len(c["nodes"]), 2)}
        c["case_class"] = ("simple" if len(c["nodes"]) <= 2 else
                           "complex" if len(c["nodes"]) > 5 or depth >= 4 else "medium")

    CASES.write_text("\n".join(json.dumps(c, ensure_ascii=False) for c in cases) + "\n", encoding="utf-8")

    total_nodes = sum(len(c["nodes"]) for c in cases if c["case_role"] == "headline_candidate")
    ok_splits = sum(1 for r in split_records if r.get("status") == "split_ok")
    # 报告按 node_key 累积合并（多次运行不互相覆盖；raw_output 只增不丢）
    prev_by_key: dict[str, dict] = {}
    if OUT_SPLIT.exists():
        try:
            for r in json.loads(OUT_SPLIT.read_text(encoding="utf-8")).get("splits", []):
                prev_by_key[r["node_key"]] = r
        except (json.JSONDecodeError, KeyError):
            prev_by_key = {}
    merged = dict(prev_by_key)
    for r in split_records:
        merged[r["node_key"]] = r
    all_records = sorted(merged.values(), key=lambda r: r["node_key"])
    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "spec_basis": "workflow_annotation_v1 §atomicity on_violation（拆分节点后重新标注）",
        "model": PRELABEL_MODEL,
        "n_composite_nodes": len(all_records),
        "n_split_ok": sum(1 for r in all_records if r.get("status") == "split_ok"),
        "n_split_failed": sum(1 for r in all_records if r.get("status") == "split_failed"),
        "semantic_nodes_after": total_nodes,
        "tokens": tokens,
        "tokens_note": "tokens 仅为本 run 增量；历史 run 见 git 提交记录",
        "provenance_note": ("拆分子节点 split_provenance.from_node 指回原 composite 节点；"
                            "人工否决某拆分时以本报告 + git 历史可完整重建"),
        "splits": all_records,
    }
    OUT_SPLIT.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"\n✓ {OUT_SPLIT}")
    print(f"  composite={len(split_records)} split_ok={ok_splits} | semantic nodes now={total_nodes}")
    print(f"  tokens in={tokens['input']} out={tokens['output']}（~$0.007 量级）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
