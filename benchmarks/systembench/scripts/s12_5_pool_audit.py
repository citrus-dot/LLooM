#!/usr/bin/env python3
"""s12_5_pool_audit.py — S12.5.3 System Pool Audit（27 号 §11.1/§12）。

只读 calibration（probe 5 case 全在 calibration split，split 纪律 ✓）。
两段式（无 subprocess，Mimosa 兼容）：
  --emit-queries  写 probe 节点的 R1（description）routing 查询 → /tmp
  --collect       读 cargo route-batch 输出（R1 band/assignment）+ outcomes probe
                  jsonl → §11.1 全指标 + §12 GO/STOP 判定

Rust 侧（手动跑，--emit-queries 会打印命令）：
  cargo run -p lloom-cli -- route-batch --queries /tmp/sb_v02_probe_queries.jsonl \
    --models deephermes_3_llama_3_8b_preview deepseek_r1_distill_qwen_7b intern_s1_mini \
    --soft-gate 0 --out /tmp/sb_v02_probe_assignments.jsonl

§11.1 指标：
  quality headroom / cost ratio / cheap-safe / weak opportunity / mid opportunity
  （eligible_*_rate、eligible_*_oracle_share、eligible_*_selected_share——区分
  「mid 不可用」vs「mid 可用但 router 不选」）/ winner entropy / cross-workflow
  stability / multi-model eligibility。
§12 GO/STOP 判定规则见 §12（GO 五条件、STOP 三信号），输出逐条对照。

oracle 规则：node 内 argmax(judge_score)，平手取低成本（unit price×usage）。
"""

from __future__ import annotations

import argparse
import json
import math
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

_HERE = Path(__file__).resolve().parent
SB = _HERE.parent
OUTCOME = SB / "matrices" / "semantic_outcomes_probe.jsonl"
CASES = SB / "cases" / "system_cases_v02.jsonl"
REPORT = SB / "reports" / "s12_5_pool_audit.json"
Q_OUT = Path("/tmp/sb_v02_probe_queries.jsonl")
A_IN = Path("/tmp/sb_v02_probe_assignments.jsonl")

POOL_TIERS = ["weak", "mid", "strong"]
TIER_MODEL = {"weak": "qwen3.6-flash", "mid": "qwen3.6-plus",
              "strong": "qwen3.6-max-preview"}
# P2 router 池（policy routerbench_v02_p2）：路由候选 = RouterBench frozen 三层
# （与采集池不同族——router 的选择空间由 RouterBench 池定义，headline 质量对照
# 用采集池实测 score。selected_share 用 P2 assignment 的 selected_model。）
P2_MODELS = ["deephermes_3_llama_3_8b_preview", "deepseek_r1_distill_qwen_7b",
             "intern_s1_mini"]
EPS_CHEAPSAFE = 0.1   # cheap-safe 容差（协议常量）


def load_outcomes() -> list[dict]:
    return [json.loads(l) for l in OUTCOME.read_text(encoding="utf-8").split("\n")
            if l.strip()]


def load_cases() -> dict[str, dict]:
    out = {}
    for l in CASES.read_text(encoding="utf-8").split("\n"):
        if l.strip():
            c = json.loads(l)
            out[c["system_case_id"]] = c
    return out


def emit_queries() -> None:
    cases = load_cases()
    ref = json.loads((SB / "matrices" / "probe_reference_artifacts_v02.json")
                     .read_text(encoding="utf-8"))
    qid_meta = {}
    rows = []
    for cid in ref["probe_cases"]:
        for n in cases[cid]["nodes"]:
            qid = f"{cid}__{n['node_id']}"
            rows.append({"query_id": qid, "task_type": n["task_type"],
                         "prompt": n["description"]})
            qid_meta[qid] = cid
    Q_OUT.write_text("\n".join(json.dumps(r, ensure_ascii=False) for r in rows)
                     + "\n", encoding="utf-8")
    print(f"✓ {len(rows)} 查询 → {Q_OUT}")
    print("然后运行：")
    print(f"  cargo run -p lloom-cli --quiet -- bench route-batch --input {Q_OUT} "
          f"--models {' '.join(P2_MODELS)} --soft-gate 0 "
          f"--output {A_IN}")


def collect() -> None:
    if not A_IN.exists():
        raise SystemExit(f"缺 {A_IN}——先跑 --emit-queries + cargo route-batch")
    assign = {}
    for l in A_IN.read_text(encoding="utf-8").split("\n"):
        if l.strip():
            a = json.loads(l)
            assign[a["query_id"]] = a
    outcomes = load_outcomes()
    # cell 索引：(case, node) → {tier: row}
    cells: dict[tuple[str, str], dict[str, dict]] = defaultdict(dict)
    for r in outcomes:
        cells[(r["system_case_id"], r["node_id"])][r["tier"]] = r

    # ---------- §11.1 指标 ----------
    n_cells = len(outcomes)
    n_nodes = len(cells)
    headrooms, winners = [], []
    cheap_safe_hits = cheap_safe_elig = 0
    cost_by_tier = defaultdict(list)
    score_by_tier = defaultdict(list)
    gain_contribs = []
    per_case = defaultdict(lambda: {t: 0 for t in POOL_TIERS})
    for (cid, nid), bytier in sorted(cells.items()):
        scores = {t: (bytier[t]["judge_score"] if bytier[t]["judge_score"] is not None
                      else None) for t in POOL_TIERS if t in bytier}
        if any(v is None for v in scores.values()) or len(scores) < 3:
            continue  # judge 失效/缺 cell 的节点不计入 oracle 统计（如实记录数）
        costs = {t: bytier[t]["cost_usd"] for t in POOL_TIERS}
        for t in POOL_TIERS:
            cost_by_tier[t].append(costs[t])
            score_by_tier[t].append(scores[t])
        best = max(scores.values())
        worst = min(scores.values())
        headrooms.append(best - worst)
        gain_contribs.append({"case": cid, "node": nid, "gain": best - worst})
        # oracle：score 最高，平手取低成本
        win = sorted(POOL_TIERS, key=lambda t: (-scores[t], costs[t]))[0]
        winners.append({"case": cid, "node": nid, "winner": win,
                        "gap_to_second": scores[win] - sorted(
                            (s for t, s in scores.items() if t != win), reverse=True)[0]
                        if len(scores) > 1 else 0.0})
        per_case[cid][win] += 1
        # cheap-safe：weak eligible（score 非空已保证）且 weak ≥ strong - ε
        if "weak" in scores and "strong" in scores:
            cheap_safe_elig += 1
            if scores["weak"] >= scores["strong"] - EPS_CHEAPSAFE:
                cheap_safe_hits += 1

    def rate(x: float, n: int) -> float | None:
        return round(x / n, 4) if n else None

    # R1 eligibility（band）：assignment 里带 band
    band_by_node = {qid: a.get("band") for qid, a in assign.items()}
    elig = {t: 0 for t in POOL_TIERS}
    eligible_mid_oracle = eligible_mid_total = mid_selected = 0
    eligible_weak_oracle = eligible_weak_total = weak_selected = 0
    for w in winners:
        band = band_by_node.get(f"{w['case']}__{w['node']}")
        selected = assign.get(f"{w['case']}__{w['node']}", {}).get("selected_model")
        # tier 门槛：hard → 仅 strong；其余全池 eligible（P2 soft-gate 0）
        if band != "hard":
            elig["weak"] += 1
            elig["mid"] += 1
            elig["strong"] += 1
        else:
            elig["strong"] += 1
        if band != "hard":
            eligible_weak_total += 1
            eligible_mid_total += 1
            if w["winner"] == "mid":
                eligible_mid_oracle += 1
            if w["winner"] == "weak":
                eligible_weak_oracle += 1
            # P2 selected：weak≈deephermes / mid≈deepseek / strong≈intern（ nominal tier 2/2/3? ）
            # P2 池 nominal tier：deephermes=weak(2) deepseek=mid(2) intern=strong(3)
            if selected == "deepseek_r1_distill_qwen_7b":
                mid_selected += 1
            if selected == "deephermes_3_llama_3_8b_preview":
                weak_selected += 1
    n_win_nodes = len(winners)

    def entropy(counts: list[int]) -> float:
        n = sum(counts)
        return round(-sum((c / n) * math.log2(c / n) for c in counts if c), 4) if n else 0.0

    win_counts = [sum(1 for w in winners if w["winner"] == t) for t in POOL_TIERS]
    # cross-workflow stability：每 case 的 winner 主导 tier
    dominant = {}
    for cid, v in per_case.items():
        tot = sum(v.values())
        dominant[cid] = ("mixed" if max(v.values()) * 2 <= tot else
                         max(v, key=lambda t: v[t]))

    # gain 集中度：top-20% 节点贡献占比
    gains = sorted((g["gain"] for g in gain_contribs), reverse=True)
    top20 = gains[:max(1, int(len(gains) * 0.2))]
    gain_concentration = round(sum(top20) / sum(gains), 4) if sum(gains) > 0 else None

    mean = lambda xs: round(sum(xs) / len(xs), 4) if xs else None  # noqa: E731
    metrics = {
        "n_nodes_scored": n_win_nodes,
        "n_nodes_total": n_nodes,
        "quality_headroom": {"mean": mean(headrooms),
                             "nonzero_rate": rate(sum(1 for h in headrooms if h > 1e-9),
                                                  len(headrooms)),
                             "max": max(headrooms) if headrooms else None},
        "cost_ratio_strong_over_weak": (
            round(mean(cost_by_tier["strong"]) / mean(cost_by_tier["weak"]), 2)
            if mean(cost_by_tier["weak"]) else None),
        "cost_ratio_mid_over_weak": (
            round(mean(cost_by_tier["mid"]) / mean(cost_by_tier["weak"]), 2)
            if mean(cost_by_tier["weak"]) else None),
        "mean_score_by_tier": {t: mean(score_by_tier[t]) for t in POOL_TIERS},
        "cheap_safe": {"eps": EPS_CHEAPSAFE, "hits": cheap_safe_hits,
                       "eligible": cheap_safe_elig,
                       "rate": rate(cheap_safe_hits, cheap_safe_elig)},
        "weak_opportunity": {
            "eligible_rate": rate(elig["weak"], n_win_nodes),
            "eligible_oracle_share": rate(eligible_weak_oracle, eligible_weak_total),
            "p2_selected_share": rate(weak_selected, eligible_weak_total)},
        "mid_opportunity": {
            "eligible_rate": rate(elig["mid"], n_win_nodes),
            "eligible_oracle_share": rate(eligible_mid_oracle, eligible_mid_total),
            "p2_selected_share": rate(mid_selected, eligible_mid_total),
            "note": "eligible_oracle_share 高而 p2_selected_share 低 = mid 可用但 router 不选"},
        "winner_entropy_bits": entropy(win_counts),
        "winner_distribution": dict(zip(POOL_TIERS, win_counts)),
        "cross_workflow_dominant_winner": dominant,
        "gain_concentration_top20pct": gain_concentration,
        "quality_gap_nonzero": bool(headrooms and max(headrooms) > 1e-9),
    }

    # ---------- §12 GO/STOP ----------
    go_flags = {
        "two_tier_complement": len([t for t in POOL_TIERS
                                    if metrics["winner_distribution"][t] > 0]) >= 2,
        "quality_gap_nonzero": metrics["quality_gap_nonzero"],
        "cost_leverage_nonzero": bool(metrics["cost_ratio_strong_over_weak"]
                                      and metrics["cost_ratio_strong_over_weak"] > 1.5),
        "winner_diversity_across_workflows": len(
            {v for v in dominant.values()}) >= 2,
        "gain_not_outlier_driven": bool(gain_concentration is not None
                                        and gain_concentration < 0.5),
    }
    stop_flags = {
        "models_homogeneous": (win_counts[POOL_TIERS.index(
            max(POOL_TIERS, key=lambda t: metrics["winner_distribution"][t]))]
            == n_win_nodes and not metrics["quality_gap_nonzero"]),
        "outlier_driven": bool(gain_concentration is not None
                               and gain_concentration >= 0.5),
        "eligibility_only_source": (metrics["winner_entropy_bits"] == 0.0
                                    and metrics["quality_gap_nonzero"] is False),
    }
    verdict = "GO" if all(go_flags.values()) and not any(stop_flags.values()) else (
        "STOP" if any(stop_flags.values()) or not go_flags["quality_gap_nonzero"]
        else "CONDITIONAL_GO")

    report = {
        "phase": "S12.5.3 System Pool Audit",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "scope": "calibration split only（probe 5 case / 35 节点 / 105 cells）",
        "router_seam": "R1 desc + P2 policy（routerbench_v02_p2，soft-gate 0）",
        "metrics": metrics,
        "go_flags": go_flags,
        "stop_flags": stop_flags,
        "verdict": verdict,
    }
    REPORT.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n",
                      encoding="utf-8")
    print(json.dumps({"verdict": verdict, "go_flags": go_flags,
                      "stop_flags": stop_flags}, ensure_ascii=False, indent=2))
    print(f"✓ {REPORT}")


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument("--emit-queries", action="store_true")
    g.add_argument("--collect", action="store_true")
    args = ap.parse_args()
    (emit_queries if args.emit_queries else collect)()
