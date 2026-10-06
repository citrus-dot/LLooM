#!/usr/bin/env python3
"""tournament_select.py — Day 9 Calibration Tournament 的 entrant 挑选（16-v3 §5.1）。

从 pool_search_full.json（全部枚举池 × dataset 指标）选出四种池型冠军 + 参考 entrant：

  R4 Weak Opportunity Champion   max mean(weak_oracle_share)
  R5 Cost Leverage Champion      max geo-mean(cost_ratio)，守卫 cheap_safe_mean ≥ 0.70
                                 （防止选出"便宜但基本不会做"的池）
  R6 Diversity Champion          max mean(winner_entropy)
  R7 Stability Champion          max median(pool_score)（cross-dataset stability，
                                 覆盖 ≥5 datasets，防"单 dataset 极优"）
  REF Global Top                 max mean(pool_score)（可能与某冠军重合，去重后保留）

全部只读 calibration 侧产物（pool_search 本身 calibration-only）；输出 tournament_entrants.json
供 `lloom-cli bench tournament`（或 replay 循环）消费。

用法（仓库根目录）：
  benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/tournament_select.py \
    --full benchmarks/routerbench/reports/pool_search_full.json \
    --min-datasets 5 \
    --output benchmarks/routerbench/reports/tournament_entrants.json
"""

from __future__ import annotations

import argparse
import json
import math
import statistics
import sys
from pathlib import Path

CHAMPIONS = ("R4_weak_opportunity", "R5_cost_leverage", "R6_diversity", "R7_stability", "REF_global_top")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--full", default="benchmarks/routerbench/reports/pool_search_full.json")
    ap.add_argument("--min-datasets", type=int, default=5)
    ap.add_argument("--output", default="benchmarks/routerbench/reports/tournament_entrants.json")
    args = ap.parse_args()

    full = json.loads(Path(args.full).read_text(encoding="utf-8"))

    cands = []
    for key, per_ds in full.items():
        if len(per_ds) < args.min_datasets:
            continue
        w, m, s = key.split("|")
        agg = {
            "pool": {"weak": w, "mid": m, "strong": s},
            "datasets": len(per_ds),
            "pool_score_mean": statistics.fmean(d["pool_score"] for d in per_ds.values()),
            "stability_median": statistics.median(d["pool_score"] for d in per_ds.values()),
            "weak_oracle_share_mean": statistics.fmean(d["weak_oracle_share"] for d in per_ds.values()),
            "cheap_safe_mean": statistics.fmean(d["cheap_safe"] for d in per_ds.values()),
            "winner_entropy_mean": statistics.fmean(d["winner_entropy"] for d in per_ds.values()),
            "oracle_gain_mean": statistics.fmean(d["oracle_gain"] for d in per_ds.values()),
            "cost_ratio_geo": math.exp(
                sum(math.log(d["cost_ratio"]) for d in per_ds.values()
                    if d["cost_ratio"] and math.isfinite(d["cost_ratio"]))
                / max(1, sum(1 for d in per_ds.values()
                             if d["cost_ratio"] and math.isfinite(d["cost_ratio"])))
            ),
        }
        cands.append((key, agg))
    if not cands:
        raise SystemExit("✗ 无满足 min-datasets 的池")

    def pick(fn) -> tuple[str, dict]:
        return max(cands, key=lambda kv: fn(kv[1]))

    selected: dict[str, tuple[str, dict]] = {}
    selected["R4_weak_opportunity"] = pick(lambda a: a["weak_oracle_share_mean"])
    # R5：cost leverage + 质量守卫（cheap_safe_mean ≥ 0.70，防止"weak 便宜但几乎不会做"）
    eligible_r5 = [kv for kv in cands if kv[1]["cheap_safe_mean"] >= 0.70] or cands
    best_r5 = max(eligible_r5, key=lambda kv: kv[1]["cost_ratio_geo"])
    selected["R5_cost_leverage"] = best_r5
    selected["R6_diversity"] = pick(lambda a: a["winner_entropy_mean"])
    selected["R7_stability"] = pick(lambda a: a["stability_median"])
    selected["REF_global_top"] = pick(lambda a: a["pool_score_mean"])

    # 去重（同名池只保留首个冠军头衔，其余头衔顺延给次优——保持 4+1 个不同 entrant）
    used: set[str] = set()
    entrants = []
    for name in CHAMPIONS:
        key, agg = selected[name]
        if key in used:
            # 找下一个不重复的最优
            if name == "R5_cost_leverage":
                pool_cands = sorted(eligible_r5, key=lambda kv: -kv[1]["cost_ratio_geo"])
            elif name == "R4_weak_opportunity":
                pool_cands = sorted(cands, key=lambda kv: -kv[1]["weak_oracle_share_mean"])
            elif name == "R6_diversity":
                pool_cands = sorted(cands, key=lambda kv: -kv[1]["winner_entropy_mean"])
            elif name == "R7_stability":
                pool_cands = sorted(cands, key=lambda kv: -kv[1]["stability_median"])
            else:
                pool_cands = sorted(cands, key=lambda kv: -kv[1]["pool_score_mean"])
            for k2, a2 in pool_cands:
                if k2 not in used:
                    key, agg = k2, a2
                    break
        used.add(key)
        w, m, s = key.split("|")
        entrants.append({
            "name": name,
            "pool": {"weak": w, "mid": m, "strong": s},
            "basis": {k: round(v, 4) if isinstance(v, float) else v for k, v in agg.items()
                      if k != "pool"},
        })

    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps({
        "selection_rule": ("R4=max weak_oracle_share_mean；R5=max cost_ratio_geo（cheap_safe_mean≥0.70 守卫）；"
                           "R6=max winner_entropy_mean；R7=max median(pool_score)（cross-dataset stability）；"
                           "REF=max pool_score_mean；同名池去重顺延。全部 calibration-only（16-v3 §5.1/5.2）"),
        "min_datasets": args.min_datasets,
        "candidates_considered": len(cands),
        "entrants": entrants,
    }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    print(f"✓ {out}（{len(entrants)} entrants，候选池 {len(cands)}）")
    for e in entrants:
        p = e["pool"]
        b = e["basis"]
        print(f"  {e['name']:<22} {p['weak'][:26]:<26}/{p['mid'][:24]:<24}/{p['strong'][:26]:<26} "
              f"ds={b['datasets']} wos={b['weak_oracle_share_mean']:.2f} safe={b['cheap_safe_mean']:.2f} "
              f"cr={b['cost_ratio_geo']:.1f} stab={b['stability_median']:.3f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
