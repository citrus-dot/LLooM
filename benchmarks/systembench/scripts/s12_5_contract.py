#!/usr/bin/env python3
"""s12_5_contract.py — S12.5.1 Contract Freeze：生成 routing_inputs_v02.jsonl sidecar。

27 号决策 ①：schema 正式分离 routing_input / execution_input（各带 mode/text/hash）。
- routing_input.R1 (description)          —— headline 路由口径（production-faithful）
- routing_input.R2 (root_goal+description) —— diagnostic 路由口径
- execution_input：base_text = node.prompt（== R2 文本）；artifact_dependent 节点的
  final_text/final_hash 在 stage 0 冻结 artifacts 后回填（S12.5.2 probe 集 / S12.6 全量）。
contract 权威：manifests/routing_input_contract_v1.yaml（本脚本是确定性投影）。

只处理 headline_candidate（semantic）cases；control 的 routing/execution 口径即
llmrb 原样 prompt，已在 case 文件内，不进 sidecar。

用法（仓库根目录）：
  benchmarks/routerbench/.venv/bin/python benchmarks/systembench/scripts/s12_5_contract.py
幂等：重复运行产出相同内容（final_* 段以已冻结值为准不覆盖）。
"""

from __future__ import annotations

import hashlib
import json
from datetime import datetime, timezone
from pathlib import Path

_HERE = Path(__file__).resolve().parent
CASES = _HERE.parent / "cases" / "system_cases_v02.jsonl"
OUT = _HERE.parent / "cases" / "routing_inputs_v02.jsonl"
SEM_PROMPT_TEMPLATE = "{root_goal}\n\n---\n## 子任务\n{description}"


def sha256(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def main() -> None:
    rows: list[dict] = []
    n_indep = n_dep = 0
    for line in CASES.read_text(encoding="utf-8").split("\n"):
        if not line.strip():
            continue
        c = json.loads(line)
        if c.get("case_role") != "headline_candidate":
            continue
        cid = c["system_case_id"]
        for n in c["nodes"]:
            r1_text = n["description"]
            r2_text = SEM_PROMPT_TEMPLATE.format(
                root_goal=c["root_goal"], description=n["description"])
            is_dep = n.get("dependency_semantics") == "artifact_dependent"
            refs: list[str] = []
            for item in (n.get("artifact_contract") or {}).get("consumes") or []:
                refs.extend(item.get("from") or []) if isinstance(item, dict) else None
                if not isinstance(item, dict):
                    refs.extend(item if isinstance(item, list) else [item])
            rows.append({
                "system_case_id": cid,
                "node_id": n["node_id"],
                "routing_input": {
                    "mode": "description", "text": r1_text, "hash": sha256(r1_text)},
                "routing_input_r2_diagnostic": {
                    "mode": "root_goal_plus_description", "text": r2_text,
                    "hash": sha256(r2_text)},
                "execution_input": {
                    "mode": ("root_goal_plus_description_plus_artifacts" if is_dep
                             else "root_goal_plus_description"),
                    "base_text": n["prompt"],
                    "base_text_hash": sha256(n["prompt"]),
                    "artifact_refs": refs,
                    "final_text": None, "final_hash": None, "final_frozen": False,
                },
                "contract_id": "sb_v02_routing_input_contract_v1",
            })
            if is_dep:
                n_dep += 1
            else:
                n_indep += 1
    OUT.write_text("\n".join(json.dumps(r, ensure_ascii=False) for r in rows) + "\n",
                   encoding="utf-8")
    # 完整性校验：base_text 必须与 R2 文本一致（node.prompt 是同一模板的冻结产物）
    mismatch = sum(
        1 for r in rows
        if r["execution_input"]["base_text"] != r["routing_input_r2_diagnostic"]["text"])
    summary = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "contract_id": "sb_v02_routing_input_contract_v1",
        "semantic_nodes": len(rows),
        "independent": n_indep, "artifact_dependent": n_dep,
        "base_text_vs_r2_mismatch": mismatch,
    }
    print(json.dumps(summary, ensure_ascii=False))
    if mismatch:
        raise SystemExit(f"FAIL: {mismatch} 个节点 base_text != R2 文本（模板漂移）")
    print(f"✓ {OUT}")


if __name__ == "__main__":
    main()
