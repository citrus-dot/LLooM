#!/usr/bin/env python3
"""weak_diagnosis.py — Weak Eligibility Audit / Error Taxonomy（Day 8，13-v2 §二十二-二十四）。

对 bench.db 中指定 run 做逐题诊断（数据：run 的 decision_json + 冻结矩阵，只读）：
  weak_oracle_equivalent : weak 分数 ∈ oracle-equivalent tie 集（矩阵直查）
  weak_eligible          : 该 run 的 decision_json.candidates 含 weak（gate 实况）
  weak_selected          : selected_model == weak
  error_class：
    blocked_weak        weak 是 oracle-equivalent 但无资格（gate 问题）
    missed_weak         有资格但未选中（scoring 问题）
    correct_weak        选中且 oracle-equivalent
    risky_weak          选中但非 oracle-equivalent
    optimal             选中模型 ∈ oracle-equivalent（含非 weak 的正确选择）
    under_routing       选中模型不在 oracle-equivalent（漏升级/错选）

指标：
  Weak Blocking Rate = P(oracle-equivalent ∧ 无资格)
  Weak Recall        = P(选中 | oracle-equivalent)
  Weak Precision     = P(oracle-equivalent | 选中 weak)
  Cost Regret        = selected_cost − min(cost | oracle-equivalent)（均值/P50/P95）

用法（仓库根目录）：
  benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/weak_diagnosis.py \
      --run 38 [--run 40] --normalized benchmarks/routerbench/normalized \
      --output benchmarks/routerbench/reports/weak_diagnosis.json
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import statistics
import sys
from datetime import datetime, timezone
from pathlib import Path

EPS = 1e-9


def classify(item: dict, weak_id: str, scores: dict[str, float]) -> tuple[str, float]:
    oracle_score = max(scores.values())
    equiv = {m for m, s in scores.items() if s >= oracle_score - EPS}
    weak_equiv = weak_id in equiv
    eligible = any(c["model"] == weak_id for c in item["candidates"])
    selected = item["selected_model"]
    if weak_equiv and not eligible:
        return "blocked_weak", oracle_score
    if weak_equiv and eligible and selected != weak_id:
        return "missed_weak", oracle_score
    if weak_equiv and selected == weak_id:
        return "correct_weak", oracle_score
    if not weak_equiv and selected == weak_id:
        return "risky_weak", oracle_score
    if selected in equiv:
        return "optimal", oracle_score
    return "under_routing", oracle_score


def diagnose_run(
    conn: sqlite3.Connection, run_id: int,
    matrix: dict[tuple[str, str], float], costs: dict[tuple[str, str], float],
    pool: dict,
) -> dict:
    weak_id = pool["weak"]
    meta = json.loads(conn.execute("SELECT parameter_json FROM bench_runs WHERE id=?", (run_id,)).fetchone()[0])
    rows = conn.execute(
        "SELECT dataset_id, sample_id, selected_model, score, cost, decision_json "
        "FROM bench_run_items WHERE run_id=?", (run_id,)
    ).fetchall()
    counts: dict[str, int] = {}
    per_dataset_counts: dict[str, dict[str, int]] = {}
    cost_regrets: list[float] = []
    weak_equiv_total = weak_selected_total = weak_eligible_equiv = 0
    selected_weak_total = weak_prec_ok = 0
    per_class_examples: dict[str, list] = {}
    for dataset_id, sample_id, selected, _score, cost, djson in rows:
        d = json.loads(djson)
        candidates = d.get("candidates") or []
        # 候选集 = 该 run 实际可竞争的模型（gate 实况）；分数从矩阵直查。
        # 常量策略（Always*/BandOnly）无 candidates 字段 → 资格集退化为 {selected}。
        models_in_cands = {c["model"] for c in candidates} or {selected}
        scores = {m: matrix.get((sample_id, m)) for m in (pool["weak"], pool["mid"], pool["strong"])}
        scores = {m: s for m, s in scores.items() if s is not None}
        if len(scores) < 3:
            continue
        cls, _ = classify(
            {"candidates": [{"model": m} for m in models_in_cands], "selected_model": selected},
            weak_id, scores,
        )
        counts[cls] = counts.get(cls, 0) + 1
        oracle_score = max(scores.values())
        equiv = {m for m, s in scores.items() if s >= oracle_score - EPS}
        weak_equiv = weak_id in equiv
        if weak_equiv:
            weak_equiv_total += 1
            if any(c["model"] == weak_id for c in candidates):
                weak_eligible_equiv += 1
                if selected == weak_id:
                    weak_selected_total += 1
        if selected == weak_id:
            selected_weak_total += 1
            if weak_equiv:
                weak_prec_ok += 1
        # Cost Regret：oracle-equivalent 集合内最小成本与实际成本之差
        equiv_min_cost = min(costs.get((sample_id, m), cost) for m in equiv)
        cost_regrets.append(max(0.0, cost - equiv_min_cost))
        per_ds = per_dataset_counts.setdefault(dataset_id, {})
        per_ds[cls] = per_ds.get(cls, 0) + 1
        if cls in ("blocked_weak", "missed_weak") and len(per_class_examples.get(cls, [])) < 3:
            per_class_examples.setdefault(cls, []).append(
                {"dataset_id": dataset_id, "sample_id": sample_id, "band": d.get("band")}
            )

    n = sum(counts.values())
    blocking_rate = counts.get("blocked_weak", 0) / n if n else 0.0
    recall = weak_selected_total / weak_equiv_total if weak_equiv_total else None
    precision = weak_prec_ok / selected_weak_total if selected_weak_total else None
    regrets_sorted = sorted(cost_regrets)
    p50 = regrets_sorted[len(regrets_sorted) // 2] if regrets_sorted else 0.0
    p95 = regrets_sorted[int(len(regrets_sorted) * 0.95)] if regrets_sorted else 0.0
    return {
        "run_id": run_id,
        "parameter": meta,
        "n": n,
        "error_counts": counts,
        "per_dataset": {ds: dict(c) for ds, c in sorted(per_dataset_counts.items())},
        "weak_oracle_equivalent_total": weak_equiv_total,
        "weak_blocking_rate": round(blocking_rate, 4),
        "weak_recall": round(recall, 4) if recall is not None else None,
        "weak_precision": round(precision, 4) if precision is not None else None,
        "cost_regret_mean": round(sum(cost_regrets) / len(cost_regrets), 6) if cost_regrets else None,
        "cost_regret_p50": round(p50, 6),
        "cost_regret_p95": round(p95, 6),
        "examples": per_class_examples,
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--run", type=int, nargs="+", required=True)
    ap.add_argument("--bench-db", default="benchmarks/routerbench/bench.db")
    ap.add_argument("--normalized", default="benchmarks/routerbench/normalized")
    ap.add_argument("--output", default="benchmarks/routerbench/reports/weak_diagnosis.json")
    args = ap.parse_args()

    # 冻结矩阵（score + cost 都要，供 oracle-equivalent 成本）
    matrix: dict[tuple[str, str], float] = {}
    costs: dict[tuple[str, str], float] = {}
    with (Path(args.normalized) / "outcomes.jsonl").open(encoding="utf-8") as f:
        for line in f:
            if not line.strip():
                continue
            o = json.loads(line)
            matrix[(o["sample_id"], o["model_id"])] = float(o["score"])
            costs[(o["sample_id"], o["model_id"])] = float(o["cost"])

    conn = sqlite3.connect(args.bench_db)
    runs = []
    for rid in args.run:
        pool_row = json.loads(conn.execute("SELECT parameter_json FROM bench_runs WHERE id=?", (rid,)).fetchone()[0])["pool"]
        runs.append(diagnose_run(conn, rid, matrix, costs, pool_row))

    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "runs": runs,
        "class_definitions": {
            "blocked_weak": "weak oracle-equivalent 但 gate 未放行（修 gate）",
            "missed_weak": "有资格但未选中（修 scoring/query-suitability）",
            "correct_weak": "选中 weak 且 oracle-equivalent",
            "risky_weak": "选中 weak 但非 oracle-equivalent",
            "optimal": "选中模型 ∈ oracle-equivalent",
            "under_routing": "选中模型 ∉ oracle-equivalent",
        },
    }
    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    print(f"✓ {out}")
    for r in runs:
        print(f"\nrun {r['run_id']} (n={r['n']}):")
        print(f"  error_counts : {r['error_counts']}")
        print(f"  blocking_rate={r['weak_blocking_rate']}  recall={r['weak_recall']}  precision={r['weak_precision']}")
        print(f"  cost_regret  mean={r['cost_regret_mean']} p50={r['cost_regret_p50']} p95={r['cost_regret_p95']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
