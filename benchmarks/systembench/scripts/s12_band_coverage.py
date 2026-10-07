#!/usr/bin/env python3
"""s12_band_coverage.py — Phase S12.4：Band Coverage Gate（23 号 §六 S12-C / 25 号 §八）。

任何 Semantic Outcome Matrix 采集（花钱）前的强制 Gate：用生产 route-batch seam
（bench::route_single → router::plan()，policy_id=routerbench_v02_p2，--soft-gate 0）
实测 v0.2 全部节点的 band 分布与多模型可行性。

两段式（本脚本不做进程编排，cargo 调用由仓库根目录 Bash 执行，v0.1 同法）：
  1. --emit-queries   写 /tmp/sb_v02_queries_{full,desc}.jsonl
  2. cargo run -q -p lloom-cli -- bench route-batch --input … --output … \
       --models deephermes_3_llama_3_8b_preview deepseek_r1_distill_qwen_7b intern_s1_mini \
       --soft-gate 0    （full/desc 各一次）
  3. --collect        汇总产物 → reports/band_coverage_v02.json

双口径（headline 口径决策留给 Nironta，S12.2-S12.4 检查点汇报）：
  full_prompt      routing 输入 = node prompt（root_goal+description，与 outcome 采集同文本）
  description_only routing 输入 = 节点 description（贴近生产 ai_client.rs:526 子任务现状）
control 节点只有 full_prompt 视图（llmrb query 逐字即其 prompt）。

eligible 判定依据（router.rs tier_req = band_tier(band)）：
  easy/medium → tier_req ≤ 2 → P2 下 weak(nominal tier=2)/mid/strong 全 eligible（3 模型）
  hard        → tier_req = 3 → 仅 strong（strong-only）

Gate（spec band_coverage_gate）：routing-relevant case 需 ≥1 个非 hard 节点；
全 strong-only case 进 diagnostic 不进 routing headline。

用法（仓库根目录）：
  benchmarks/routerbench/.venv/bin/python benchmarks/systembench/scripts/s12_band_coverage.py --emit-queries
  cargo run -q -p lloom-cli -- bench route-batch --input /tmp/sb_v02_queries_full.jsonl \
    --output /tmp/sb_v02_assignments_full.jsonl \
    --models deephermes_3_llama_3_8b_preview deepseek_r1_distill_qwen_7b intern_s1_mini --soft-gate 0
  … 同上 desc …
  benchmarks/routerbench/.venv/bin/python benchmarks/systembench/scripts/s12_band_coverage.py --collect
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

CASES = Path("benchmarks/systembench/cases/system_cases_v02.jsonl")
OUT_REPORT = Path("benchmarks/systembench/reports/band_coverage_v02.json")
Q_FULL = Path("/tmp/sb_v02_queries_full.jsonl")
Q_DESC = Path("/tmp/sb_v02_queries_desc.jsonl")
A_FULL = Path("/tmp/sb_v02_assignments_full.jsonl")
A_DESC = Path("/tmp/sb_v02_assignments_desc.jsonl")


def emit_queries() -> None:
    cases = [json.loads(l) for l in CASES.read_text(encoding="utf-8").split("\n") if l.strip()]
    full_q, desc_q = [], []
    for c in cases:
        for n in c["nodes"]:
            qid = f"{c['system_case_id']}__{n['node_id']}"
            full_q.append({"query_id": qid, "task_type": n["task_type"], "prompt": n["prompt"]})
            if c["case_role"] == "headline_candidate" and n.get("description"):
                desc_q.append({"query_id": qid, "task_type": n["task_type"], "prompt": n["description"]})
    Q_FULL.write_text("\n".join(json.dumps(q, ensure_ascii=False) for q in full_q) + "\n", encoding="utf-8")
    Q_DESC.write_text("\n".join(json.dumps(q, ensure_ascii=False) for q in desc_q) + "\n", encoding="utf-8")
    print(f"✓ {Q_FULL}  ({len(full_q)} 节点，含 control)")
    print(f"✓ {Q_DESC}  ({len(desc_q)} 节点，仅 semantic)")


def load_assignments(path: Path) -> dict[str, dict]:
    out: dict[str, dict] = {}
    for line in path.read_text(encoding="utf-8").split("\n"):
        if line.strip():
            r = json.loads(line)
            out[r["query_id"]] = {"selected_model": r["selected_model"], "band": r["band"]}
    return out


def eligible_count(band: str) -> int:
    return 1 if band == "hard" else 3  # P2（--soft-gate 0）：weak nominal tier=2 放行 medium


def view_metrics(rows: list[dict]) -> dict:
    n = len(rows)
    band_dist = defaultdict(int)
    model_dist = defaultdict(int)
    eligible_hist = defaultdict(int)
    for r in rows:
        band_dist[r["band"]] += 1
        model_dist[r["selected_model"]] += 1
        eligible_hist[eligible_count(r["band"])] += 1
    hard_rate = band_dist["hard"] / n
    ent = -sum((c / n) * math.log2(c / n) for c in model_dist.values())
    return {
        "n_nodes": n,
        "band_distribution": {k: round(v / n, 4) for k, v in sorted(band_dist.items())},
        "eligible_model_count_dist": {f"{k}_models": round(v / n, 4) for k, v in sorted(eligible_hist.items())},
        "strong_only_node_rate": round(hard_rate, 4),
        "weak_eligible_node_rate": round(1 - hard_rate, 4),
        "mid_eligible_node_rate": round(1 - hard_rate, 4),  # P2 下 mid 与 weak 的 tier 门槛同为 ≤2
        "multi_model_eligible_rate": round(1 - hard_rate, 4),
        "selected_model_distribution": dict(sorted(model_dist.items())),
        "winner_entropy_bits": round(ent, 4),
    }


def case_gate(rows: list[dict], node_meta: dict) -> dict:
    by_case: dict[str, list[dict]] = defaultdict(list)
    for r in rows:
        c, _ = node_meta[r["query_id"]]
        by_case[c["system_case_id"]].append(r)
    relevant, diagnostic = [], []
    for cid, rs in by_case.items():
        role = node_meta[rs[0]["query_id"]][0]["case_role"]
        entry = {"case_id": cid, "case_role": role, "bands": sorted({r["band"] for r in rs})}
        (relevant if any(r["band"] != "hard" for r in rs) else diagnostic).append(entry)
    return {
        "n_cases": len(by_case),
        "routing_relevant": len(relevant),
        "strong_only_diagnostic": len(diagnostic),
        "routing_relevant_semantic": sum(1 for e in relevant if e["case_role"] == "headline_candidate"),
        "strong_only_semantic": sum(1 for e in diagnostic if e["case_role"] == "headline_candidate"),
        "strong_only_case_ids": [e["case_id"] for e in diagnostic],
    }


def collect() -> None:
    cases = [json.loads(l) for l in CASES.read_text(encoding="utf-8").split("\n") if l.strip()]
    node_meta = {}
    for c in cases:
        for n in c["nodes"]:
            node_meta[f"{c['system_case_id']}__{n['node_id']}"] = (c, n)

    full_res, desc_res = load_assignments(A_FULL), load_assignments(A_DESC)
    sem_full = [{"query_id": qid, **full_res[qid]} for qid, (c, _) in node_meta.items()
                if c["case_role"] == "headline_candidate" and qid in full_res]
    ctl_full = [{"query_id": qid, **full_res[qid]} for qid, (c, _) in node_meta.items()
                if c["case_role"] == "control" and qid in full_res]
    sem_desc = [{"query_id": qid, **desc_res[qid]} for qid, (c, _) in node_meta.items()
                if c["case_role"] == "headline_candidate" and qid in desc_res]

    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "seam": "bench::route_single → router::plan()（policy_id=routerbench_v02_p2，soft_gate=0）",
        "views": {
            "full_prompt": {
                "semantic_nodes": view_metrics(sem_full),
                "control_nodes": view_metrics(ctl_full),
                "case_gate_all": case_gate(sem_full + ctl_full, node_meta),
                "case_gate_semantic_only": case_gate(sem_full, node_meta),
            },
            "description_only": {
                "semantic_nodes": view_metrics(sem_desc),
                "case_gate_semantic_only": case_gate(sem_desc, node_meta),
                "note": "贴近生产 ai_client.rs:526 子任务 routing 现状（不发 root_goal）；headline 口径决策留 Nironta",
            },
        },
        "honesty_notes": [
            "band 基于当前冻结 prompt 文本现算（band_for 语义，S9a parity 同源）",
            "artifact_dependent 节点 S12.6 stage 0 注入冻结参考产物后 prompt 变长 → S12.6 花钱前需零成本重测 band（gate 复核）",
            "spec 阈值为工程判断非统计定理：multi_model_eligible_rate <20% → diagnostic only（25 号 §8.2）",
        ],
    }
    OUT_REPORT.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    for view in ("full_prompt", "description_only"):
        m = report["views"][view]["semantic_nodes"]
        print(f"[{view}] semantic: band={m['band_distribution']} "
              f"eligible_dist={m['eligible_model_count_dist']} "
              f"multi_model={m['multi_model_eligible_rate']:.1%} entropy={m['winner_entropy_bits']}bits "
              f"dist={m['selected_model_distribution']}")
    g = report["views"]["full_prompt"]["case_gate_semantic_only"]
    gd = report["views"]["description_only"]["case_gate_semantic_only"]
    print(f"Gate(full_prompt): relevant={g['routing_relevant']}/{g['n_cases']} "
          f"(semantic {g['routing_relevant_semantic']} rele / {g['strong_only_semantic']} diag)")
    print(f"Gate(desc_only):   relevant={gd['routing_relevant']}/{gd['n_cases']} "
          f"(semantic {gd['routing_relevant_semantic']} rele / {gd['strong_only_semantic']} diag)")
    print(f"✓ {OUT_REPORT}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    group = ap.add_mutually_exclusive_group(required=True)
    group.add_argument("--emit-queries", action="store_true")
    group.add_argument("--collect", action="store_true")
    args = ap.parse_args()
    if args.emit_queries:
        emit_queries()
    else:
        collect()
    return 0


if __name__ == "__main__":
    sys.exit(main())
