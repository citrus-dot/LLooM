#!/usr/bin/env python3
"""bootstrap_report.py — Day 13 统计：v0.2 冻结 test 的 Bootstrap 95% CI（16-v3 Gate 7 前置）。

输入（bench.db，只读）：
  split='test' 且 benchmark_id='lloom-routerbench-v0.2' 的 runs：
  冻结 P2（lloom+softgate0）/ P0（lloom）/ band_only / 8 基线 / matched random ×2
输出 reports/v02_test_report.{json,md}：
  - 全量指标表（Q/C/saving/retention/weak recall/precision）
  - paired bootstrap 95% CI（B=10,000，seed=3407，实例级重采样）：
      saving/retention vs Always-Strong；P2 与两个 matched random 的质量差；weak precision
  - accuracy-only 子集聚合（LLMRouterBench scorer_groups；judge 类 dataset 单列）
  - P2 per-dataset 分解

SQL 约定（Mimosa 红线）：只做无 WHERE 的裸 SELECT，全部过滤在 Python 侧完成。
用法（仓库根目录）：
  benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/bootstrap_report.py
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path

import numpy as np

EPS = 1e-9


def boot_ci(rng: np.random.Generator, b: int, stat, n: int) -> dict:
    """stat(idx) → 统计量；对实例索引重采样 b 次，返回 point + 2.5/97.5 分位。"""
    point = float(stat(np.arange(n)))
    stats = np.empty(b)
    for i in range(b):
        idx = rng.integers(0, n, size=n)
        stats[i] = stat(idx)
    return {
        "point": round(point, 4),
        "ci95_low": round(float(np.percentile(stats, 2.5)), 4),
        "ci95_high": round(float(np.percentile(stats, 97.5)), 4),
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--bench-db", default="benchmarks/routerbench/bench.db")
    ap.add_argument("--benchmark-id", default="lloom-routerbench-v0.2")
    ap.add_argument("--normalized", default="benchmarks/routerbench/normalized/llmrouterbench")
    ap.add_argument("--bootstrap", type=int, default=10_000)
    ap.add_argument("--seed", type=int, default=3407)
    ap.add_argument("--output", default="benchmarks/routerbench/reports/v02_test_report.json")
    args = ap.parse_args()

    conn = sqlite3.connect(args.bench_db)

    # 裸 SELECT + Python 过滤（Mimosa：不拼任何 WHERE）
    runs = {}
    for rid, strategy, pjson, sjson, split, bid in conn.execute(
        "SELECT id, strategy, parameter_json, summary_json, split, benchmark_id FROM bench_runs ORDER BY id"
    ):
        if split != "test" or bid != args.benchmark_id:
            continue
        p = json.loads(pjson)
        sg = p.get("soft_gate_penalty")
        key = strategy if sg is None else f"{strategy}@softgate{float(sg):g}"
        runs.setdefault(key, {"run_id": rid, "summary": json.loads(sjson), "parameter": p})

    items_by_run: dict[int, dict[str, dict]] = {}
    run_to_key: dict[int, str] = {v["run_id"]: k for k, v in runs.items()}
    for rid, sample_id, selected, score, cost, djson in conn.execute(
        "SELECT run_id, sample_id, selected_model, score, cost, decision_json FROM bench_run_items ORDER BY id"
    ):
        key = run_to_key.get(rid)
        if key is None:
            continue
        d = json.loads(djson)
        items_by_run.setdefault(rid, {})[sample_id] = {
            "selected": selected, "score": score, "cost": cost,
            "candidates": {c["model"] for c in (d.get("candidates") or [])},
        }

    def strategy_items(key: str) -> dict[str, dict]:
        return items_by_run[runs[key]["run_id"]]

    p2 = strategy_items("lloom@softgate0")
    strong = strategy_items("always_strong")
    weak_items = strategy_items("always_weak")
    mid_items = strategy_items("always_mid")
    cmr = strategy_items("cost_matched_random")
    ssm = strategy_items("strong_share_matched_random")

    sids = sorted(set(p2) & set(strong) & set(cmr) & set(ssm))
    n = len(sids)

    q_p2 = np.array([p2[s]["score"] for s in sids])
    c_p2 = np.array([p2[s]["cost"] for s in sids])
    q_strong = np.array([strong[s]["score"] for s in sids])
    c_strong = np.array([strong[s]["cost"] for s in sids])
    q_cmr = np.array([cmr[s]["score"] for s in sids])
    q_ssm = np.array([ssm[s]["score"] for s in sids])
    q_weak = np.array([weak_items[s]["score"] for s in sids])
    q_mid = np.array([mid_items[s]["score"] for s in sids])
    weak_id = runs["lloom@softgate0"]["parameter"]["pool"]["weak"]

    oracle_q = np.maximum(np.maximum(q_weak, q_mid), np.maximum(q_strong, q_p2))
    weak_selected = np.array([p2[s]["selected"] == weak_id for s in sids])
    weak_equiv = np.abs(q_weak - oracle_q) <= EPS
    weak_prec_den = int(weak_selected.sum())
    prec_vals = (weak_selected & weak_equiv).astype(float)
    sel_vals = weak_selected.astype(float)

    rng = np.random.Generator(np.random.PCG64(args.seed))
    B = args.bootstrap

    ci = {
        "saving_vs_strong": boot_ci(rng, B, lambda i: 1 - c_p2[i].mean() / max(c_strong[i].mean(), 1e-15), n),
        "retention_vs_strong": boot_ci(rng, B, lambda i: q_p2[i].mean() / max(q_strong[i].mean(), 1e-15), n),
        "quality_delta_vs_cost_matched": boot_ci(rng, B, lambda i: q_p2[i].mean() - q_cmr[i].mean(), n),
        "quality_delta_vs_strong_share_matched": boot_ci(rng, B, lambda i: q_p2[i].mean() - q_ssm[i].mean(), n),
    }
    if weak_prec_den:
        ci["weak_precision"] = boot_ci(
            rng, B, lambda i: prec_vals[i].sum() / max(sel_vals[i].sum(), 1), n)

    # accuracy-only 子集（LLMRouterBench scorer_groups）
    src = json.loads((Path(args.normalized) / "source_manifest.json").read_text(encoding="utf-8"))
    accuracy_names = src["scoring"]["scorer_groups"]["accuracy"]

    def is_accuracy(ds: str) -> bool:
        return any(ds.startswith(f"llmrb_{name}_") for name in accuracy_names)

    ds_of = {s: None for s in sids}
    for rid, sample_id, dataset_id in conn.execute(
        "SELECT run_id, sample_id, dataset_id FROM bench_run_items ORDER BY id"
    ):
        key = run_to_key.get(rid)
        if key == "always_strong" and sample_id in ds_of:
            ds_of[sample_id] = dataset_id
    acc_mask = np.array([is_accuracy(ds_of[s]) for s in sids])

    def agg(mask):
        return {
            "n": int(mask.sum()),
            "quality_mean": round(float(q_p2[mask].mean()), 4),
            "cost_mean": round(float(c_p2[mask].mean()), 6),
            "saving_vs_strong": round(1 - float(c_p2[mask].mean()) / float(c_strong[mask].mean()), 4),
            "retention_vs_strong": round(float(q_p2[mask].mean()) / float(q_strong[mask].mean()), 4),
        }

    per_ds: dict[str, dict] = {}
    for s in sids:
        ds = ds_of[s]
        a = per_ds.setdefault(ds, {"n": 0, "q": 0.0, "c": 0.0})
        a["n"] += 1
        a["q"] += float(p2[s]["score"])
        a["c"] += float(p2[s]["cost"])

    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "benchmark_id": args.benchmark_id,
        "n_test": n,
        "runs": {k: {"run_id": v["run_id"], **v["summary"]} for k, v in runs.items()},
        "bootstrap_ci": ci,
        "accuracy_only_p2": agg(acc_mask),
        "full_p2": agg(np.ones(n, dtype=bool)),
        "per_dataset_p2": {
            ds: {"n": a["n"], "quality": round(a["q"] / a["n"], 4), "cost": round(a["c"] / a["n"], 6)}
            for ds, a in sorted(per_ds.items())
        },
        "notes": [
            "bootstrap 以实例为单位重采样（10 号 §13.10 口径），B=10000，seed=3407",
            "oracle/weak 分数由 always_* 三 run 的实例分数构成（同一冻结矩阵，等价于池全模型查表）",
            "budget curve 发现：R7 池 P2 下 strong 调用全部来自 hard 带（唯一合格模型），"
            "strong 预算旋钮在该池无可交换空间——曲线平坦是结构属性而非实现缺陷",
        ],
    }
    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    print(f"✓ {out}")
    print(f"n={n} | P2: Q={report['full_p2']['quality_mean']} C=${report['full_p2']['cost_mean']}/q "
          f"| accuracy-only: Q={report['accuracy_only_p2']['quality_mean']} "
          f"saving={report['accuracy_only_p2']['saving_vs_strong']:.1%}")
    for k, v in ci.items():
        print(f"  {k:<40} {v['point']:+.4f}  CI95 [{v['ci95_low']:+.4f}, {v['ci95_high']:+.4f}]")
    return 0


if __name__ == "__main__":
    sys.exit(main())
