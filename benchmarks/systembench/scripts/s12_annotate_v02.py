#!/usr/bin/env python3
"""s12_annotate_v02.py — Phase S12.3：Semantic Workflow 标注（workflow_annotation_v1.yaml）。

D2-b 流程（24 号 §三 D2 / 25 号 §六）：
  qwen-plus 预标注（每 workflow 一次调用，temp=0，few-shot）
    → 确定性校验（词表/contract 一致性/atomicity 启发式）
    → agent 复核（分歧标记，不改 LLM 判定除非机械违规）
    → 产物 provenance 全记录（prelabel_model / review_status / flags）
  人工抽检：20% 节点（哈希确定性抽样）+ 全部 flagged 节点 → reports/s12_annotation_report.json
  分歧/不一致率 >30% → 回退人工全量（24 号 D2-b 回退条款）。

红线：
  - 标注不改 node prompt（exact binding 不动；本脚本产出后重跑 binding 校验）；
  - consumes.from 以 edges 投影为权威（spec：edges 与 consumes 必须一致）；
  - 词表封闭：semantic_role 10 词 / artifact type 8 词，越界即 spec_violation flag。

产物：
  cases/annotations_v02.jsonl         每 node 一行标注台账（含 provenance）
  cases/system_cases_v02.jsonl        原地回填 semantic_role/atomic_ok/artifact_contract/completion_condition
  reports/s12_annotation_report.json  一致性统计 + 抽检包

用法（仓库根目录）：
  DASHSCOPE_API_KEY=… benchmarks/routerbench/.venv/bin/python \
    benchmarks/systembench/scripts/s12_annotate_v02.py            # 首次：LLM 预标注
  … s12_annotate_v02.py --replay                                  # 重放（零成本）
  … s12_annotate_v02.py --merge-only                              # 只做确定性复核 + 回填（不调 API）
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import statistics
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

import requests

_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))
from s11_decompose_pilot import ENDPOINT  # noqa: E402  同源 endpoint 常量

sys.path.insert(0, str(_HERE.parent / "adapters"))
from annotate import ROLE_KEYWORDS, infer_role  # noqa: E402  S12.1 启发式（一致性对照）

CASES = Path("benchmarks/systembench/cases/system_cases_v02.jsonl")
OUT_ANNOT = Path("benchmarks/systembench/cases/annotations_v02.jsonl")
OUT_REPORT = Path("benchmarks/systembench/reports/s12_annotation_report.json")
PRELABEL_MODEL = "qwen-plus"

ROLES = [r for r, _ in ROLE_KEYWORDS]
ARTIFACT_TYPES = ["facts", "code_patch", "calculation", "plan_outline",
                  "analysis", "structured_record", "draft_content", "verification_report"]
COMPOSITE_MARKERS = ["且", "并", "再", "然后", "同时", "以及", "接着"]

PROMPT_SYSTEM = f"""你是语义工作流标注员，严格按照标注规范 workflow_annotation_v1 为工作流的每个节点标注。

## semantic_role 词表（每节点恰选一个主 role）
{chr(10).join('- ' + r for r in ROLES)}
区分要点：calculation=可确定性重放的计算；reasoning=需模型判断的推理/规划/权衡；synthesis=多源合并成最终产出；execution=生成可运行产物/调度指令；search=生成式探索枚举备选。

## atomicity
一个 node 只承担一个明确职责。描述中出现并列动词（且/并/再/然后/同时）承担多个独立动作时 atomic_ok=false，并说明 reason。
注意：同一职责内的自然连接词（如「提取并汇总同一批数据」）不算复合；只有当动作无法用一个 role 概括时才算。

## artifact types 词表（produces/consumes 条目的 type 只能取这些）
{chr(10).join('- ' + t for t in ARTIFACT_TYPES)}

## 规则
1. consumes.from 必须恰好等于该节点 depends_on 的节点 id 列表（无依赖则 consumes=[]）
2. consumes.type 应能由所 from 上游节点的 produces.type 提供
3. 每节点 completion_role ∈ required | optional（默认 required；仅当该节点失败不影响最终目标时 optional）
4. 输出严格 JSON，无围栏无解释：
{{"nodes": [{{"node_id": "...", "semantic_role": "...", "atomic_ok": true, "atomic_reason": "...", "produces": [{{"type": "...", "required": true}}], "consumes": [{{"type": "...", "from": ["..."], "required": true}}], "completion_role": "required", "confidence": 0.9}}], "aggregation_condition": "synthesis|all_required"}}

## 示例
输入节点：n1「解析所有任务与车辆基础数据，提取站点名称拼音首音」 depends_on=[] →
{{"node_id": "n1", "semantic_role": "extraction", "atomic_ok": true, "atomic_reason": "单一抽取职责", "produces": [{{"type": "facts", "required": true}}], "consumes": [], "completion_role": "required", "confidence": 0.9}}
输入节点：n2「基于合规初筛结果与优先级规则排定最终运输方案」 depends_on=["n1"] →
{{"node_id": "n2", "semantic_role": "reasoning", "atomic_ok": true, "atomic_reason": "「基于…排定」是单一规划职责，依赖引用不算复合", "produces": [{{"type": "plan_outline", "required": true}}], "consumes": [{{"type": "verification_report", "from": ["n1"], "required": true}}], "completion_role": "required", "confidence": 0.85}}
输入节点：n3「核对方案与时间窗约束并生成最终排班表」 depends_on=["n2"] →
{{"node_id": "n3", "semantic_role": "verification", "atomic_ok": false, "atomic_reason": "「核对…并生成」是校验与产出两个独立动作，无法用一个 role 概括", "produces": [{{"type": "structured_record", "required": true}}], "consumes": [{{"type": "plan_outline", "from": ["n2"], "required": true}}], "completion_role": "required", "confidence": 0.8}}"""

PROMPT_USER = "## 根目标（背景语境，不参与标注对象）\n{goal}\n\n## 待标注节点\n{nodes_json}"


def call_llm(system: str, user: str, api_key: str) -> tuple[str, int, int]:
    resp = requests.post(
        ENDPOINT,
        headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"},
        json={"model": PRELABEL_MODEL, "temperature": 0,
              "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}]},
        timeout=90)
    resp.raise_for_status()
    d = resp.json()
    usage = d.get("usage", {})
    return d["choices"][0]["message"]["content"], usage.get("prompt_tokens", 0), usage.get("completion_tokens", 0)


def parse_json_block(raw: str) -> dict | None:
    text = raw.strip()
    m = re.search(r"```(?:json)?\s*(\{.*?\})\s*```", text, re.S)
    if m:
        text = m.group(1)
    else:
        m = re.search(r"\{.*\}", text, re.S)
        if m:
            text = m.group(0)
    try:
        obj = json.loads(text)
        return obj if isinstance(obj, dict) else None
    except json.JSONDecodeError:
        return None


def deps_of(case: dict) -> dict[str, list[str]]:
    deps: dict[str, list[str]] = {n["node_id"]: [] for n in case["nodes"]}
    for e in case["edges"]:
        deps[e["to"]].append(e["from"])
    return deps


def review_node(case: dict, node: dict, ann: dict, deps: dict[str, list[str]]) -> tuple[dict, list[str]]:
    """确定性复核 + 机械修正。返回 (修正后标注, flags)。机械违规直接修正；语义分歧只标记。"""
    flags: list[str] = []
    nid = node["node_id"]
    dep_ids = deps[nid]

    # 1. 词表封闭校验
    if ann.get("semantic_role") not in ROLES:
        flags.append(f"spec_violation:semantic_role={ann.get('semantic_role')}")
        heur, _ = infer_role(node["description"])
        ann["semantic_role"] = heur if heur != "unclassified" else "reasoning"
        flags.append("mechanical_fix:role_fallback_heuristic")
    for c in ann.get("consumes", []):
        if c.get("type") not in ARTIFACT_TYPES:
            flags.append(f"spec_violation:consumes.type={c.get('type')}")
            c["type"] = "facts"
            flags.append("mechanical_fix:consumes.type→facts")
    for p in ann.get("produces", []):
        if p.get("type") not in ARTIFACT_TYPES:
            flags.append(f"spec_violation:produces.type={p.get('type')}")
            p["type"] = "draft_content"
            flags.append("mechanical_fix:produces.type→draft_content")

    # 2. consumes.from == edges 投影（spec 权威）
    cf = [f for c in ann.get("consumes", []) for f in c.get("from", [])]
    if sorted(set(cf)) != sorted(set(dep_ids)):
        flags.append(f"contract_fix:consumes.from={sorted(set(cf))}≠edges投影{sorted(set(dep_ids))}")
        if dep_ids:
            ann["consumes"] = [{"type": "facts", "from": dep_ids, "required": True}]
            flags.append("mechanical_fix:consumes.type 待跨节点检查")
        else:
            ann["consumes"] = []

    # 3. atomicity 并列动词启发式（只标记，不推翻 LLM 判定）
    desc = node["description"]
    markers = [m for m in COMPOSITE_MARKERS if m in desc]
    if markers and ann.get("atomic_ok", True):
        flags.append(f"atomicity_review:并列标记{markers}（LLM 判 atomic，请抽检确认）")
    if not ann.get("atomic_ok", True):
        flags.append("atomicity_false:复合节点禁入 v0.2 headline（spec §atomicity on_violation）")

    # 4. completion_role 缺省
    if ann.get("completion_role") not in ("required", "optional"):
        ann["completion_role"] = "required"
        flags.append("mechanical_fix:completion_role→required")
    return ann, flags


ann_by_node: dict[str, dict] = {}


def rebuild_report_from_cases() -> None:
    """拆分后同步：ledger 缺失的节点（split 子节点）从 case 字段补行，重算统计+抽检包。零 LLM。
    消费 reports/s12_agent_review.json：agent 已复核放行的拆分组降级为 informational。"""
    review_path = Path("benchmarks/systembench/reports/s12_agent_review.json")
    cleared: set[tuple[str, str]] = set()
    cleared_nodes: set[tuple[str, str]] = set()
    disputed_nodes: set[tuple[str, str]] = set()
    if review_path.exists():
        rv = json.loads(review_path.read_text(encoding="utf-8"))
        for g in rv.get("cleared_groups", []):
            cid, nid = g.split("/", 1)
            cleared.add((cid, nid))
        for nk in rv.get("round2", {}).get("cleared_nodes", []):
            cid, nid = nk.split("/", 1)
            cleared_nodes.add((cid, nid))
        for d in rv.get("round2", {}).get("disputed_nodes", []):
            cid, nid = d["node_key"].split("/", 1)
            disputed_nodes.add((cid, nid))

    cases = [json.loads(l) for l in CASES.read_text(encoding="utf-8").split("\n") if l.strip()]
    existing: dict[tuple[str, str], dict] = {}
    if OUT_ANNOT.exists():
        for line in OUT_ANNOT.read_text(encoding="utf-8").split("\n"):
            if line.strip():
                r = json.loads(line)
                existing[(r["case_id"], r["node_id"])] = r
    sem_cases = [c for c in cases if c["case_role"] == "headline_candidate"]
    deps: dict[str, dict[str, list[str]]] = {c["system_case_id"]: deps_of(c) for c in sem_cases}
    ledger: list[dict] = []
    for c in sem_cases:
        for n in c["nodes"]:
            key = (c["system_case_id"], n["node_id"])
            if key in existing and not n.get("split_provenance"):
                row = existing[key]
                # agent 复核政策（s12_agent_review.json atomicity_policy）：仅连接词提示
                # 不构成复合证据（spec：复合 = 无法用一个 role 概括），降级为信息性
                fl = row["provenance"]["flags"]
                if row["provenance"]["review_status"] == "flagged_for_human" and \
                        fl and all(f.startswith("atomicity_review") for f in fl):
                    row = json.loads(json.dumps(row))
                    row["provenance"]["review_status"] = "agent_reviewed"
                    row["provenance"]["flags"] = [
                        "agent_clear:仅并列连接词、LLM 判单一职责维持（信息性，抽检可推翻）"] + fl
                ledger.append(row)
                continue
            split_prov = n.get("split_provenance") or {}
            from_node = split_prov.get("from_node")
            node_key = (c["system_case_id"], n["node_id"])
            if node_key in disputed_nodes:
                flags = ["arbitration_needed:疑似过拆（可与同级子节点合并），人工仲裁"]
                review_status = "flagged_for_human"
            elif node_key in cleared_nodes:
                flags = [f"agent_clear:再拆分复核放行（s12_agent_review.json round2，抽检可推翻）"]
                review_status = "agent_reviewed"
            elif (c["system_case_id"], from_node) in cleared:
                flags = [f"agent_clear:拆分组 {from_node} 已复核放行（s12_agent_review.json，抽检可推翻）"]
                review_status = "agent_reviewed"
            else:
                flags = [f"split_node:由复合节点 {from_node} 拆分，人工抽检拆分质量"]
                review_status = "flagged_for_human"
            prov = {
                "annotator": "llm_split",
                "prelabel_model": split_prov.get("model", PRELABEL_MODEL),
                "review_status": review_status,
                "source": "s12_composite_split",
                "final": "pending_human_spotcheck",
                "heuristic_role": None, "heuristic_agree": False,
                "flags": flags,
            }
            prelabel = {
                "semantic_role": n.get("semantic_role", "reasoning"),
                "atomic_ok": bool(n.get("atomic_ok", True)),
                "atomic_reason": n.get("atomic_reason", ""),
                "produces": (n.get("artifact_contract") or {}).get("produces", []),
                "consumes": (n.get("artifact_contract") or {}).get("consumes", []),
                "completion_role": "required", "confidence": None,
            }
            ledger.append({"case_id": c["system_case_id"], "node_id": n["node_id"],
                           "description_excerpt": n["description"][:80],
                           "prelabel": prelabel, "provenance": prov,
                           "spec_version": "workflow_annotation_v1"})
    OUT_ANNOT.write_text("\n".join(json.dumps(r, ensure_ascii=False) for r in ledger) + "\n", encoding="utf-8")

    role_dist = defaultdict(int)
    agree_n = heur_covered = 0
    mandatory = [r for r in ledger if r["provenance"]["review_status"] == "flagged_for_human"]
    informational = [r for r in ledger
                     if r["provenance"]["review_status"] == "agent_reviewed" and r["provenance"]["flags"]]
    for r in ledger:
        role_dist[r["prelabel"]["semantic_role"]] += 1
        if r["provenance"]["heuristic_role"] and r["provenance"]["heuristic_role"] != "unclassified":
            heur_covered += 1
            agree_n += r["provenance"]["heuristic_agree"]
    spot = sorted(ledger, key=lambda r: hashlib.sha256(
        f"lloom-sbspot:3407:{r['case_id']}:{r['node_id']}".encode()).digest())
    n_spot = max(1, round(len(ledger) * 0.2))
    spot_pack = spot[:n_spot]
    mandatory_keys = {(r["case_id"], r["node_id"]) for r in mandatory}
    pack = [r for r in ledger
            if (r["case_id"], r["node_id"]) in mandatory_keys or r in spot_pack]
    atomic_false = [r for r in ledger if not r["prelabel"].get("atomic_ok", True)]
    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "spec_version": "workflow_annotation_v1",
        "prelabel_model": PRELABEL_MODEL,
        "n_workflows": len(sem_cases), "n_nodes": len(ledger),
        "role_distribution": dict(sorted(role_dist.items(), key=lambda kv: -kv[1])),
        "heuristic_agreement": {"covered": heur_covered, "agree": agree_n,
                                "rate": round(agree_n / heur_covered, 3) if heur_covered else None},
        "review_tiers": {
            "mandatory_review": len(mandatory),
            "informational_flags": len(informational),
            "clean": len(ledger) - len(mandatory) - len(informational),
        },
        "atomic_false_nodes": len(atomic_false),
        "human_spotcheck_pack": {
            "rule": "哈希排序前 20%（确定性）∪ 全部 mandatory_review；informational 不强制",
            "n_nodes": len(pack),
            "rows": [{"case_id": r["case_id"], "node_id": r["node_id"],
                      "description": r["description_excerpt"],
                      "semantic_role": r["prelabel"]["semantic_role"],
                      "atomic_ok": r["prelabel"]["atomic_ok"],
                      "flags": r["provenance"]["flags"]} for r in pack],
        },
        "notes": [
            "review_status=flagged_for_human 的节点需人工确认；agent_reviewed 的 informational flag 可抽查",
            "D2-b 回退判据：抽检不一致率 >30% → 回退人工全量（24 号 §三 D2）",
            "consumes.from 以 edges 投影为权威，LLM 不一致处已机械修正并记 flag",
        ],
    }
    OUT_REPORT.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"✓ ledger 重建: {len(ledger)} nodes（净增 {len(ledger) - len(existing)} 行）")
    print(f"  分层: 必审={len(mandatory)} 信息性={len(informational)} | 抽检包={len(pack)} 节点")


def main() -> int:
    global ann_by_node
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--replay", action="store_true", help="重用已有 annotations（零成本）")
    ap.add_argument("--merge-only", action="store_true", help="跳过 LLM，仅确定性复核+回填")
    ap.add_argument("--rebuild-report", action="store_true",
                    help="拆分后同步 ledger/统计/抽检包（零 LLM，case 字段为真源）")
    args = ap.parse_args()
    if args.rebuild_report:
        rebuild_report_from_cases()
        return 0

    cases = [json.loads(l) for l in CASES.read_text(encoding="utf-8").split("\n") if l.strip()]
    sem_cases = [c for c in cases if c["case_role"] == "headline_candidate"]
    print(f"S12.3 标注: {len(sem_cases)} semantic workflows / "
          f"{sum(len(c['nodes']) for c in sem_cases)} nodes | model={PRELABEL_MODEL} temp=0")

    prev: dict[str, dict] = {}
    if args.replay and OUT_ANNOT.exists():
        for line in OUT_ANNOT.read_text(encoding="utf-8").split("\n"):
            if line.strip():
                r = json.loads(line)
                prev[(r["case_id"], r["node_id"])] = r
        print(f"  replay: {len(prev)} 条已有标注")

    api_key = os.environ.get("DASHSCOPE_API_KEY")
    ledger: list[dict] = []
    tokens = {"input": 0, "output": 0}
    call_fail = 0

    for c in sem_cases:
        deps = deps_of(c)
        # S12.1 启发式对照（一致性统计用）
        heur_roles = {}
        for n in c["nodes"]:
            heur_roles[n["node_id"]], _ = infer_role(n["description"])

        batch_anns: dict[str, dict] = {}
        if not args.merge_only:
            if all((c["system_case_id"], n["node_id"]) in prev for n in c["nodes"]):
                for n in c["nodes"]:
                    batch_anns[n["node_id"]] = prev[(c["system_case_id"], n["node_id"])]["prelabel"]
            else:
                nodes_brief = [{"node_id": n["node_id"], "description": n["description"],
                                "task_type": n["task_type"],
                                "depends_on": deps[n["node_id"]]} for n in c["nodes"]]
                try:
                    raw, in_tok, out_tok = call_llm(
                        PROMPT_SYSTEM,
                        PROMPT_USER.format(goal=c["root_goal"][:1500],
                                           nodes_json=json.dumps(nodes_brief, ensure_ascii=False)),
                        api_key)
                    tokens["input"] += in_tok
                    tokens["output"] += out_tok
                    obj = parse_json_block(raw)
                    if obj and isinstance(obj.get("nodes"), list):
                        for a in obj["nodes"]:
                            if isinstance(a, dict) and "node_id" in a:
                                batch_anns[a["node_id"]] = a
                except Exception as e:  # 网络/解析失败：不阻塞，节点降级为待人工
                    call_fail += 1
                    print(f"  {c['system_case_id']}: ✗ LLM 调用失败 {e}")
        else:
            for n in c["nodes"]:
                key = (c["system_case_id"], n["node_id"])
                if key in prev:
                    batch_anns[n["node_id"]] = prev[key]["prelabel"]

        for n in c["nodes"]:
            nid = n["node_id"]
            ann = batch_anns.get(nid) or {"semantic_role": "unclassified", "atomic_ok": True,
                                          "atomic_reason": "llm_missing", "produces": [],
                                          "consumes": [], "completion_role": "required", "confidence": 0.0}
            ann, flags = review_node(c, n, ann, deps)
            heur = heur_roles.get(nid)
            agree = bool(heur and heur != "unclassified" and heur == ann.get("semantic_role"))
            provenance = {
                "annotator": "llm_prelabel",
                "prelabel_model": PRELABEL_MODEL if nid in batch_anns else "none",
                "review_status": "flagged_for_human" if flags else "agent_reviewed",
                "source": "llm_prelabel",
                "final": "pending_human_spotcheck",
                "heuristic_role": heur,
                "heuristic_agree": agree,
                "flags": flags,
            }
            ledger.append({
                "case_id": c["system_case_id"], "node_id": nid,
                "description_excerpt": n["description"][:80],
                "prelabel": ann, "provenance": provenance,
                "spec_version": "workflow_annotation_v1",
            })
            ann_by_node[nid] = ann

        # 跨节点 contract 一致性：consumes.type 须可由上游 produces 满足
        produces_by_node = {n["node_id"]: [p["type"] for p in
                            (batch_anns.get(n["node_id"], {}).get("produces") or [])]
                            for n in c["nodes"]}
        for row in ledger[-len(c["nodes"]):]:
            for cons in row["prelabel"].get("consumes", []):
                for u in cons.get("from", []):
                    if cons["type"] not in produces_by_node.get(u, []) and produces_by_node.get(u):
                        row["provenance"]["flags"].append(
                            f"contract_mismatch:{u} produces {produces_by_node[u]} 但下游要 {cons['type']}")
                        row["provenance"]["review_status"] = "flagged_for_human"

    # 回填 case 文件（标注不改 prompt——回填后重验 binding）
    ann_lookup = {(r["case_id"], r["node_id"]): r for r in ledger}
    for c in cases:
        if c["case_role"] != "headline_candidate":
            continue
        for n in c["nodes"]:
            row = ann_lookup[(c["system_case_id"], n["node_id"])]
            a = row["prelabel"]
            n["semantic_role"] = a["semantic_role"]
            n["atomic_ok"] = bool(a.get("atomic_ok", True))
            n["atomic_reason"] = a.get("atomic_reason", "")
            n["artifact_contract"] = {"produces": a.get("produces", []),
                                      "consumes": a.get("consumes", [])}
            n["annotation_provenance"] = row["provenance"]
        # completion_condition：synthesis 存在时以其完成为准（spec aggregation_rule）
        syn = [n["node_id"] for n in c["nodes"] if n["semantic_role"] == "synthesis"]
        optional = [n["node_id"] for n in c["nodes"]
                    if ann_lookup[(c["system_case_id"], n["node_id"])]["prelabel"].get("completion_role") == "optional"]
        required = [n["node_id"] for n in c["nodes"] if n["node_id"] not in optional]
        c["completion_condition"] = {
            "required_nodes": required, "optional_nodes": optional,
            "success_condition": "workflow_completion",
            "aggregation_condition": ("synthesis_node:" + ",".join(syn)) if syn else "all_required",
        }
    CASES.write_text("\n".join(json.dumps(c, ensure_ascii=False) for c in cases) + "\n", encoding="utf-8")
    OUT_ANNOT.write_text("\n".join(json.dumps(r, ensure_ascii=False) for r in ledger) + "\n", encoding="utf-8")

    # binding 完整性重验（标注不得破坏 exact binding）
    for c in cases:
        for n in c["nodes"]:
            h = hashlib.sha256(n["prompt"].encode("utf-8")).hexdigest()
            if h != n["outcome_binding"]["prompt_hash"]:
                raise SystemExit(f"✗ 标注破坏 binding: {c['system_case_id']}/{n['node_id']}")

    # 统计 + 抽检包（20% 哈希确定性 + 全部 flagged）
    role_dist = defaultdict(int)
    agree_n = heur_covered = 0
    flagged = [r for r in ledger if r["provenance"]["flags"]]
    for r in ledger:
        role_dist[r["prelabel"]["semantic_role"]] += 1
        if r["provenance"]["heuristic_role"] and r["provenance"]["heuristic_role"] != "unclassified":
            heur_covered += 1
            agree_n += r["provenance"]["heuristic_agree"]
    spot = sorted(ledger, key=lambda r: hashlib.sha256(
        f"lloom-sbspot:3407:{r['case_id']}:{r['node_id']}".encode()).digest())
    n_spot = max(1, round(len(ledger) * 0.2))
    spot_pack = spot[:n_spot]
    flagged_keys = {(r["case_id"], r["node_id"]) for r in flagged}
    spot_plus_flagged = [r for r in ledger
                         if (r["case_id"], r["node_id"]) in flagged_keys or r in spot_pack]
    atomic_false = [r for r in ledger if not r["prelabel"].get("atomic_ok", True)]

    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "spec_version": "workflow_annotation_v1",
        "prelabel_model": PRELABEL_MODEL,
        "n_workflows": len(sem_cases), "n_nodes": len(ledger),
        "llm_tokens": tokens, "llm_call_failures": call_fail,
        "role_distribution": dict(sorted(role_dist.items(), key=lambda kv: -kv[1])),
        "heuristic_agreement": {"covered": heur_covered, "agree": agree_n,
                                "rate": round(agree_n / heur_covered, 3) if heur_covered else None},
        "flagged_nodes": len(flagged),
        "flag_rate": round(len(flagged) / len(ledger), 3),
        "atomic_false_nodes": len(atomic_false),
        "human_spotcheck_pack": {
            "rule": "哈希排序前 20%（确定性）∪ 全部 flagged",
            "n_nodes": len(spot_plus_flagged),
            "rows": [{"case_id": r["case_id"], "node_id": r["node_id"],
                      "description": r["description_excerpt"],
                      "semantic_role": r["prelabel"]["semantic_role"],
                      "atomic_ok": r["prelabel"]["atomic_ok"],
                      "flags": r["provenance"]["flags"]} for r in spot_plus_flagged],
        },
        "notes": [
            "review_status=flagged_for_human 的节点需人工确认；其余 agent_reviewed（final=pending_human_spotcheck）",
            "flag 率即 D2-b 回退判据：抽检不一致率 >30% → 回退人工全量（24 号 §三 D2）",
            "consumes.from 以 edges 投影为权威，LLM 不一致处已机械修正并记 flag",
        ],
    }
    OUT_REPORT.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    print(f"✓ {OUT_ANNOT}  ({len(ledger)} nodes)")
    print(f"✓ {CASES}（已回填标注，binding 校验通过）")
    print(f"  role 分布: {dict(sorted(role_dist.items(), key=lambda kv: -kv[1]))}")
    print(f"  启发式一致率: {report['heuristic_agreement']['rate']} | flagged={len(flagged)}"
          f" ({report['flag_rate']:.1%}) | atomic_false={len(atomic_false)}")
    if tokens["input"] or tokens["output"]:
        print(f"  tokens in={tokens['input']} out={tokens['output']}（~$0.01 量级）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
