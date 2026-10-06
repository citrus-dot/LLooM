#!/usr/bin/env python3
"""pool_search.py — Model Pool Search（Day 7，13-v2 §七/§二十五/§二十六）。

对每个 dataset 的 calibration split，在该 dataset 可用模型宇宙内枚举**成本单调**三元池
（mean_cost(weak) < mean_cost(mid) < mean_cost(strong)，成本=calibration 侧均值），
计算池级指标并按 PoolScore 排序：

  PoolScore = 0.30 × norm(oracle_gain) + 0.25 × weak_oracle_share + 0.20 × cheap_safe
            + 0.15 × winner_entropy + 0.10 × norm(log1p(cost_ratio))

  - norm = 数据集内 min-max（枚举池之间归一，公式只用于 screening 排序，不进生产路由）
  - weak_oracle_share = P(weak ∈ oracle-equivalent set)（并列按 tie 集合计入）
  - winner_entropy：argmax 并列按 1/k 平分记账后取熵（与 screening.py 同口径）
  - Gate 1 前置：池三模型必须对该 dataset calibration 实例 100% 覆盖

输出 pool_candidates.json：
  per_dataset：每 dataset Top-K + 三个命名冠军（max_cost_leverage / max_weak_opportunity /
               max_diversity）
  global：跨 dataset 聚合（池在≥min_datasets 个 dataset 上全覆盖时参与；PoolScore 取
          dataset 均值，权重按 calibration 实例数加权）

用法（仓库根目录）：
  benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/pool_search.py \
      --input benchmarks/routerbench/normalized/llmrouterbench \
      --output benchmarks/routerbench/reports/pool_candidates.json
"""

from __future__ import annotations

import argparse
import json
import math
from collections import defaultdict
from datetime import datetime, timezone
from itertools import combinations
from pathlib import Path

import numpy as np

# 非裸模型条目：聚合路由服务/垃圾目录名——保留在 canonical，但不参与池枚举
EXCLUDE_MODELS = {"openrouter"}


def minmax(vals: list[float]) -> list[float]:
    lo, hi = min(vals), max(vals)
    if hi - lo < 1e-12:
        return [0.0] * len(vals)
    return [(v - lo) / (hi - lo) for v in vals]


def load_matrix(norm_dir: Path, split: str):
    """返回 {dataset_id: {"instances": [sample_id], "scores": ndarray(n,M), "costs": ndarray(n,M),
    "models": [model_id]}}——仅保留 calibration 全覆盖模型（Gate 1）。"""
    instances = [
        json.loads(l)
        for l in (norm_dir / "instances.jsonl").read_text(encoding="utf-8").split("\n")
        if l.strip()
    ]
    inst_of = {i["sample_id"]: i for i in instances if i.get("split") == split}
    by_ds: dict[str, list[str]] = defaultdict(list)
    for i in instances:
        if i.get("split") == split:
            by_ds[i["dataset_id"]].append(i["sample_id"])

    # (dataset, model) → {sample_id: (score, cost)}
    per_model: dict[tuple[str, str], dict[str, tuple[float, float]]] = defaultdict(dict)
    with (norm_dir / "outcomes.jsonl").open(encoding="utf-8") as f:
        for line in f:
            if not line.strip():
                continue
            o = json.loads(line)
            if o["sample_id"] in inst_of:
                per_model[(o["dataset_id"], o["model_id"])][o["sample_id"]] = (
                    float(o["score"]), float(o["cost"]),
                )

    matrices = {}
    for ds, sids in by_ds.items():
        sids = sorted(set(sids))
        models = sorted(m for (d, m) in per_model if d == ds
                        and m not in EXCLUDE_MODELS
                        and len(per_model[(d, m)]) == len(sids))  # 全覆盖模型
        if len(models) < 3:
            continue
        S = np.array([[per_model[(ds, m)][s][0] for m in models] for s in sids], dtype=float)
        C = np.array([[per_model[(ds, m)][s][1] for m in models] for s in sids], dtype=float)
        matrices[ds] = {"instances": sids, "models": models, "scores": S, "costs": C, "n": len(sids)}
    return matrices


def eval_pool(S: np.ndarray, C: np.ndarray, iw: int, im: int, ist: int) -> dict:
    sw, sm, ss = S[:, iw], S[:, im], S[:, ist]
    cw, cm, cs = C[:, iw], C[:, im], C[:, ist]
    max3 = np.maximum(np.maximum(sw, sm), ss)
    oracle_gain = float(max3.mean() - max(sw.mean(), sm.mean(), ss.mean()))
    weak_oracle_share = float((sw >= max3 - 1e-12).mean())
    cheap_safe = float((sw >= ss - 1e-12).mean())
    upgrade = float((sw < ss - 1e-12).mean())
    # winner 熵（并列 1/k 记账，聚合份额）
    masks = np.stack([sw >= max3 - 1e-12, sm >= max3 - 1e-12, ss >= max3 - 1e-12], axis=1)
    credit = masks.sum(axis=0).astype(float)
    shares = credit / credit.sum()
    H = float(-sum(p * math.log(p) for p in shares if p > 0))
    mw, ms = float(cw.mean()), float(cs.mean())
    cost_ratio = (ms / mw) if mw > 0 else float("inf")
    return {
        "oracle_gain": oracle_gain,
        "weak_oracle_share": weak_oracle_share,
        "cheap_safe": cheap_safe,
        "upgrade_opportunity": upgrade,
        "winner_entropy": H,
        "cost_ratio": cost_ratio,
        "weak_mean": float(sw.mean()), "mid_mean": float(sm.mean()), "strong_mean": float(ss.mean()),
        "cost_weak": mw, "cost_mid": float(cm.mean()), "cost_strong": ms,
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--input", default="benchmarks/routerbench/normalized/llmrouterbench")
    ap.add_argument("--split", default="calibration")
    ap.add_argument("--top-k", type=int, default=10)
    ap.add_argument("--min-datasets", type=int, default=5, help="global 池至少覆盖的 dataset 数")
    ap.add_argument("--dump-full", default=None,
                    help="可选：导出全部枚举池 × dataset 的 PoolScore 矩阵（R7 stability 计算用）")
    ap.add_argument("--output", default="benchmarks/routerbench/reports/pool_candidates.json")
    args = ap.parse_args()

    matrices = load_matrix(Path(args.input), args.split)
    if not matrices:
        raise SystemExit("✗ 无可用 dataset（覆盖不足）")

    per_dataset_out = {}
    global_accum: dict[tuple[str, str, str], list[dict]] = defaultdict(list)
    for ds in sorted(matrices):
        M = matrices[ds]
        models, S, C, n = M["models"], M["scores"], M["costs"], M["n"]
        mean_cost = C.mean(axis=0)
        triples = [
            (iw, im, ist)
            for iw, im, ist in combinations(range(len(models)), 3)
            if mean_cost[iw] < mean_cost[im] < mean_cost[ist]
            and mean_cost[iw] > 0
        ]
        if not triples:
            continue
        raw = []
        for iw, im, ist in triples:
            m = eval_pool(S, C, iw, im, ist)
            m["pool"] = {"weak": models[iw], "mid": models[im], "strong": models[ist]}
            raw.append(m)
        # 数据集内归一
        norm_og = minmax([m["oracle_gain"] for m in raw])
        norm_cr = minmax([math.log1p(m["cost_ratio"]) for m in raw if math.isfinite(m["cost_ratio"])])
        finite_idx = [i for i, m in enumerate(raw) if math.isfinite(m["cost_ratio"])]
        for i, m in enumerate(raw):
            if i in finite_idx:
                cr = norm_cr[finite_idx.index(i)]
            else:
                cr = 1.0
            m["pool_score"] = round(
                0.30 * norm_og[i] + 0.25 * m["weak_oracle_share"] + 0.20 * m["cheap_safe"]
                + 0.15 * m["winner_entropy"] + 0.10 * cr, 4)
            m["n"] = n
        ranked = sorted(raw, key=lambda m: -m["pool_score"])
        winners = {
            "max_cost_leverage": max(raw, key=lambda m: m["cost_ratio"] if math.isfinite(m["cost_ratio"]) else -1),
            "max_weak_opportunity": max(raw, key=lambda m: m["weak_oracle_share"]),
            "max_diversity": max(raw, key=lambda m: m["winner_entropy"]),
        }
        per_dataset_out[ds] = {"n_calibration": n, "models_universe": len(models),
                               "pools_enumerated": len(raw),
                               "top": ranked[: args.top_k], "winners": winners}
        for m in raw:
            m["dataset"] = ds
            global_accum[(m["pool"]["weak"], m["pool"]["mid"], m["pool"]["strong"])].append(m)

    # global：跨 dataset 聚合（简单算术均值；n 加权均值作参考）
    global_pools = []
    for (w, m_, s), entries in global_accum.items():
        if len(entries) < args.min_datasets:
            continue
        agg = {
            "pool": {"weak": w, "mid": m_, "strong": s},
            "datasets_covered": len(entries),
            "pool_score_mean": round(sum(e["pool_score"] for e in entries) / len(entries), 4),
            "oracle_gain_mean": round(sum(e["oracle_gain"] for e in entries) / len(entries), 4),
            "weak_oracle_share_mean": round(sum(e["weak_oracle_share"] for e in entries) / len(entries), 4),
            "cheap_safe_mean": round(sum(e["cheap_safe"] for e in entries) / len(entries), 4),
            "winner_entropy_mean": round(sum(e["winner_entropy"] for e in entries) / len(entries), 4),
            "cost_ratio_geo": round(math.exp(sum(math.log(e["cost_ratio"]) for e in entries
                                                 if math.isfinite(e["cost_ratio"]))
                                             / max(1, sum(1 for e in entries if math.isfinite(e["cost_ratio"])))), 2),
        }
        global_pools.append(agg)
    global_pools.sort(key=lambda g: -g["pool_score_mean"])

    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "input": str(args.input),
        "split": args.split,
        "pool_score_formula": ("0.30×norm(oracle_gain) + 0.25×weak_oracle_share + 0.20×cheap_safe "
                               "+ 0.15×winner_entropy + 0.10×norm(log1p(cost_ratio))；norm=dataset 内 min-max；"
                               "仅 screening 排序用，不进生产路由（13-v2 §26）"),
        "leakage_note": "全部指标只算 calibration split；池冻结后才允许见 test（13-v2 Gate 8）",
        "datasets_screened": len(per_dataset_out),
        "global_top": global_pools[: args.top_k],
        "per_dataset": per_dataset_out,
    }
    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    if args.dump_full:
        full = {
            "|".join(pool): {
                e["dataset"]: {
                    "pool_score": e["pool_score"],
                    "oracle_gain": e["oracle_gain"],
                    "weak_oracle_share": e["weak_oracle_share"],
                    "cheap_safe": e["cheap_safe"],
                    "winner_entropy": e["winner_entropy"],
                    "cost_ratio": e["cost_ratio"],
                    "n": e["n"],
                }
                for e in entries
            }
            for pool, entries in global_accum.items()
        }
        Path(args.dump_full).write_text(
            json.dumps(full, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(f"✓ full matrix → {args.dump_full}（{len(full)} 池）")

    print(f"✓ {out}")
    print(f"datasets={len(per_dataset_out)} | global 池（≥{args.min_datasets} datasets）{len(global_pools)} 个，Top-10：")
    for g in global_pools[:10]:
        print(f"  {g['pool_score_mean']:.4f}  {g['pool']['weak'][:28]:<28} / {g['pool']['mid'][:28]:<28} / "
              f"{g['pool']['strong'][:28]:<28}  ds={g['datasets_covered']} og={g['oracle_gain_mean']:.3f} "
              f"wos={g['weak_oracle_share_mean']:.2f} safe={g['cheap_safe_mean']:.2f} cr={g['cost_ratio_geo']}")
    return 0


if __name__ == "__main__":
    main()
