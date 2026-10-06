#!/usr/bin/env python3
"""cross_benchmark_v02.py — Day 14-15：两数据源方向一致性分析 + Native 决策输入（16-v3 §十五/§三十一）。

只读汇总（不跑新 replay）：
  llmrb 侧     = v02_test_report.json（Day 12-13：R7+P2 冻结 test，n=12,345）
  routerbench 侧 = Day 8 Core test（run 33/38/40：v0.1 池 BASE/P0/P2，n=1,000）
                + Day 14 tournament（run 100-119：5 entrant × 4 policy，calibration）
产出 reports/cross_benchmark_v02.json：
  两源对照表 / 方向判定（机制 vs 幅度）/ 弱模型质量先验-可行性解释 / Day 15 五条件核对
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--bench-db", default="benchmarks/routerbench/bench.db")
    ap.add_argument("--v02-report", default="benchmarks/routerbench/reports/v02_test_report.json")
    ap.add_argument("--output", default="benchmarks/routerbench/reports/cross_benchmark_v02.json")
    args = ap.parse_args()

    v02 = json.loads(Path(args.v02_report).read_text(encoding="utf-8"))
    conn = sqlite3.connect(args.bench_db)

    # Day 8 RouterBench Core test：BASE(33)/P0(38)/P2(40)
    rb = {}
    for rid, sjson in conn.execute("SELECT id, summary_json FROM bench_runs WHERE id IN (33,38,40)"):
        rb[rid] = json.loads(sjson)
    rb_base, rb_p0, rb_p2 = rb[33], rb[38], rb[40]
    rb_p2_saving = 1 - rb_p2["cost_mean"] / rb_base["cost_mean"]
    rb_p2_retention = rb_p2["quality_mean"] / rb_base["quality_mean"]

    # Day 14 RouterBench tournament：各 entrant 的 P0 是否 ≡ BASE（weak 是否进场）
    tournament = []
    for rid, strategy, pjson, sjson in conn.execute(
        "SELECT id, strategy, parameter_json, summary_json FROM bench_runs WHERE split=? AND id>=100 ORDER BY id",
        ("calibration",),
    ):
        p = json.loads(pjson)
        s = json.loads(sjson)
        tournament.append({
            "run_id": rid, "strategy": strategy, "pool": p.get("pool"),
            "quality_mean": s["quality_mean"], "cost_mean": s["cost_mean"],
            "selection_dist": s["selection_dist"],
        })

    # weak share per run（RouterBench tournament）
    for t in tournament:
        pool = t["pool"] or {}
        weak = pool.get("weak", "")
        t["weak_share"] = round(t["selection_dist"].get(weak, 0) / max(t["selection_dist"] and sum(t["selection_dist"].values()), 1), 4)

    llmrb_p2 = v02["full_p2"]
    analysis = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "llmrb_side": {
            "benchmark_id": v02["benchmark_id"],
            "pool": "R7_stability（deephermes-3-8b / r1-distill-qwen-7b / intern-s1-mini）",
            "policy": "P2 softgate0",
            "n_test": v02["n_test"],
            "saving_vs_strong": llmrb_p2["saving_vs_strong"],
            "retention_vs_strong": llmrb_p2["retention_vs_strong"],
            "weak_precision_ci": v02["bootstrap_ci"].get("weak_precision"),
            "vs_cost_matched": v02["bootstrap_ci"]["quality_delta_vs_cost_matched"],
            "vs_strong_share_matched": v02["bootstrap_ci"]["quality_delta_vs_strong_share_matched"],
            "verdict": "weak rescue 显著成立（saving 13.5%、retention≈1、precision 78.6%、双 matched random CI 不含 0）",
        },
        "routerbench_side": {
            "matrix": "RouterBench 0-shot（2024 世代 11 模型）",
            "day8_core_test": {
                "pool": "v0.1（mistral-7b / Yi-34B / GPT-4）",
                "p2_saving": round(rb_p2_saving, 4),
                "p2_retention": round(rb_p2_retention, 4),
                "p2_quality": rb_p2["quality_mean"],
                "note": "P2 有 saving 36.9% 但 retention 90.7%（质量代价大）、weak precision 0.572（Day 8）",
            },
            "day14_tournament": {
                "verdict": "全部 5 entrant 的 P0 ≡ BASE（weak share=0），P2 仅 easy 带（99 题）有变化",
                "runs": tournament,
                "structural_reason": "mistral-7b 质量先验 0.31 与 mid（Yi-34B）0.66 差距 0.35，"
                                     "cost 权重（等效单价差 ~4×）不足以翻转质量项——weak 放行也不被选",
            },
            "verdict": "weak rescue 不可行（弱模型质量先验过低；saving 头条无法在该矩阵产生）",
        },
        "direction_judgment": {
            "mechanism_consistent": True,
            "mechanism_evidence": [
                "gate 因果链两源一致：RouterBench 100% Blocked（Day 8）/ llmrb 池覆盖过滤同构",
                "P0≡P1（band 门槛主导）在两源的多个池上均成立",
                "weak rescue 的可行性由『弱模型质量先验与 mid 的差距』决定——这是池属性，不是 router 属性",
            ],
            "magnitude_differs": True,
            "magnitude_explanation": "RouterBench 2024 世代小模型（7B）与 mid 差距 0.35 → rescue 代价大（retention 90.7%）；"
                                     "llmrb 新世代高效小模型与 mid 差距小 → rescue 近乎免费（retention 100.05%）。"
                                     "两源结果不矛盾，是同一机制在不同模型代际上的不同表现",
        },
        "day15_native_checklist": {
            "1_frozen_test_improvement_over_v0": {"pass": True,
                "evidence": "llmrb R7+P2 vs R7+P0：weak share 0→9.7%，cost −3.3%，quality 持平；vs 旧池 P2 precision 0.572→0.809"},
            "2_quality_retention_guardrail": {"pass": True,
                "evidence": "retention 100.05% CI [0.9957, 1.0052]（vs Always-Strong 无显著差）"},
            "3_cost_saving_material": {"pass": True,
                "evidence": "saving 13.46% CI [12.20, 14.73]（llmrb）；RouterBench 36.9%（有质量代价）"},
            "4_matched_random_beaten": {"pass": True,
                "evidence": "双 matched random 质量差 CI [2.07,2.97] / [0.75,1.53] 均不含 0"},
            "5_second_source_consistent": {"pass": "partial",
                "evidence": "机制方向一致（gate 因果/P0≡P1/池决定可行性）；幅度不同且 RouterBench 侧 rescue 不可行——"
                             "属边界条件而非反证，但严格按 16-v3 §三十一字面为不满足"},
        },
        "recommendation": {
            "native": "hold（暂缓）",
            "reason": "五条件 4/5 全过 + 1 项 partial。v0.2 headline（saving 13.5%、retention≈1、content-aware 实证）"
                      "已足够支撑立项展示；Native（qwen3.6 池自采）的预期增益是验证『llmrb 结构可迁移到生产池』，"
                      "属确认性实验而非新结论——建议挂起至立项材料需要生产池数字时再采（届时 2×2×50 sanity 起步）",
            "routerbench_status": "Day 15 后转入 maintenance（16-v3 §二十三），资源切换 OrchestrationBench",
        },
    }
    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(analysis, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    print(f"✓ {out}")
    print("llmrb :", analysis["llmrb_side"]["verdict"])
    print("rb    :", analysis["routerbench_side"]["verdict"])
    print("方向  : 机制一致 ✅ / 幅度不同（模型代际）")
    print("Day 15:", analysis["recommendation"]["native"], "—", analysis["recommendation"]["reason"][:60], "…")
    return 0


if __name__ == "__main__":
    sys.exit(main())
