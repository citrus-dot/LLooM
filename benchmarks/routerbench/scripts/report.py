#!/usr/bin/env python3
"""report.py — RouterBench v0.1 报表层：Pareto 前沿 + AIQ + 汇总表（Day 5）。

从 bench.db 读取各策略最新 run 的 summary/bench_run_items，产出：
  reports/bench_report.json   机器可读：每策略/每 dataset 指标、Pareto 前沿、AIQ
  reports/bench_report.md     人读汇总（10 号 §四十一 表格式）

AIQ 算法（10 号 §13.9，RouterBench quality-cost integration 思路）：
  全部策略点 (cost, quality) → Pareto 非支配前沿 → 按 cost 升序折线 →
  在共同成本区间 [c_lo, c_hi] 上梯形积分归一化质量 → 除以区间宽度得 [0,1] 标量。
  注：v0.1 点集稀疏（3 池 + λ 折中），AIQ 仅作对照参考，不作唯一 headline（§13.9）。

用法（仓库根目录）：
  benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/report.py
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path


def latest_run(conn: sqlite3.Connection, strategy: str) -> tuple[int, dict] | None:
    row = conn.execute(
        "SELECT id, summary_json FROM bench_runs WHERE strategy=? ORDER BY id DESC LIMIT 1",
        (strategy,),
    ).fetchone()
    if not row:
        return None
    return row[0], json.loads(row[1])


def items_of(conn: sqlite3.Connection, run_id: int) -> list[dict]:
    rows = conn.execute(
        "SELECT dataset_id, score, cost, regret FROM bench_run_items WHERE run_id=?",
        (run_id,),
    ).fetchall()
    return [{"dataset_id": r[0], "score": r[1], "cost": r[2], "regret": r[3]} for r in rows]


def pareto_front(points: list[dict]) -> list[dict]:
    """非支配前沿（最小化 cost，最大化 quality）。"""
    pts = sorted(points, key=lambda p: (p["cost"], -p["quality"]))
    front: list[dict] = []
    best_q = -1.0
    for p in pts:
        if p["quality"] > best_q:
            front.append(p)
            best_q = p["quality"]
    return front


def aiq(front: list[dict]) -> float | None:
    """前沿折线在共同成本区间上的归一化质量积分（梯形法）→ [0,1]。"""
    if len(front) < 1:
        return None
    if len(front) == 1:
        return front[0]["quality"]
    c_lo, c_hi = front[0]["cost"], front[-1]["cost"]
    if c_hi - c_lo < 1e-15:
        return None
    area = 0.0
    for a, b in zip(front, front[1:]):
        area += (a["quality"] + b["quality"]) / 2.0 * (b["cost"] - a["cost"])
    return area / (c_hi - c_lo)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--bench-db", default="benchmarks/routerbench/bench.db")
    ap.add_argument("--output-dir", default="benchmarks/routerbench/reports")
    args = ap.parse_args()

    db_path = Path(args.bench_db)
    if not db_path.exists():
        print(f"✗ bench.db 不存在: {db_path}", file=sys.stderr)
        return 2
    conn = sqlite3.connect(db_path)

    strategies = ["always_weak", "always_mid", "always_strong", "cheapest",
                  "random", "best_single", "oracle", "lloom"]
    runs: dict[str, tuple[int, dict]] = {}
    for s in strategies:
        r = latest_run(conn, s)
        if r:
            runs[s] = r
    if "lloom" not in runs or "always_strong" not in runs or "always_weak" not in runs:
        print("✗ 缺关键 run（lloom/always_strong/always_weak）——先跑 lloom-cli bench replay", file=sys.stderr)
        return 2

    # λ sweep 点（run 的 parameter_json 里 cost_weight 去重取最新）
    sweep_points: list[dict] = []
    for rid, pjson in conn.execute(
        "SELECT id, parameter_json FROM bench_runs WHERE strategy='lloom' ORDER BY id"
    ):
        p = json.loads(pjson)
        cw = p.get("policy", {}).get("cost_weight")
        if cw is not None:
            summary = json.loads(conn.execute(
                "SELECT summary_json FROM bench_runs WHERE id=?", (rid,)
            ).fetchone()[0])
            sweep_points.append({
                "run_id": rid, "cost_weight": cw,
                "cost": summary["cost_mean"], "quality": summary["quality_mean"],
                "selection_dist": summary["selection_dist"],
            })
    # 同 λ 取最新
    by_lambda: dict[float, dict] = {}
    for p in sweep_points:
        by_lambda[p["cost_weight"]] = p
    sweep_points = [by_lambda[k] for k in sorted(by_lambda)]

    # 汇总点集（常量策略 + best_single + oracle + lloom λ 曲线）
    def pt(name: str, run: tuple[int, dict]) -> dict:
        return {"strategy": name, "run_id": run[0],
                "cost": run[1]["cost_mean"], "quality": run[1]["quality_mean"]}

    all_points = [pt(s, runs[s]) for s in ("always_weak", "always_mid", "always_strong",
                                           "best_single", "oracle") if s in runs]
    for i, sp in enumerate(sweep_points):
        all_points.append({"strategy": f"lloom(λ={sp['cost_weight']:.1f})",
                           "run_id": sp["run_id"], "cost": sp["cost"], "quality": sp["quality"]})
    front = pareto_front(all_points)
    aiq_val = aiq(front)

    # LLooM 对比指标（默认 λ=0.5 run，即 parameter_json policy.cost_weight=0.5 的最新）
    lloom_default = sweep_points[[p["cost_weight"] for p in sweep_points].index(0.5)] \
        if 0.5 in [p["cost_weight"] for p in sweep_points] else None
    qs = runs["always_strong"][1]["quality_mean"]
    cs = runs["always_strong"][1]["cost_mean"]
    qw = runs["always_weak"][1]["quality_mean"]
    if lloom_default:
        ql, cl = lloom_default["quality"], lloom_default["cost"]
        compare = {
            "lloom_run": lloom_default["run_id"],
            "cost_weight": 0.5,
            "saving_vs_strong": round(1 - cl / cs, 4) if cs > 0 else None,
            "retention_vs_strong": round(ql / qs, 4) if qs > 0 else None,
            "gap_recovery": round((ql - qw) / (qs - qw), 4) if qs - qw > 1e-12 else None,
        }
    else:
        compare = None

    # per-dataset 表（§四十一）：各策略 × dataset 的 Q/C
    per_dataset: dict[str, dict[str, dict]] = {}
    for s, (rid, _) in runs.items():
        for it in items_of(conn, rid):
            agg = per_dataset.setdefault(it["dataset_id"], {}).setdefault(s, {"sum_q": 0.0, "sum_c": 0.0, "n": 0})
            agg["sum_q"] += it["score"]
            agg["sum_c"] += it["cost"]
            agg["n"] += 1
    per_dataset_clean = {
        ds: {s: {"quality": round(a["sum_q"] / a["n"], 4), "cost": round(a["sum_c"] / a["n"], 6), "n": a["n"]}
             for s, a in strat_map.items()}
        for ds, strat_map in sorted(per_dataset.items())
    }

    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "strategies": {s: {"run_id": r[0], **{k: v for k, v in r[1].items()}} for s, r in runs.items()},
        "lloom_lambda_curve": sweep_points,
        "pareto_front": front,
        "aiq": round(aiq_val, 4) if aiq_val is not None else None,
        "aiq_note": "前沿折线在共同成本区间的归一化质量积分（梯形）；点集稀疏仅作对照（10 号 §13.9）",
        "lloom_compare": compare,
        "per_dataset": per_dataset_clean,
    }

    out_dir = Path(args.output_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    json_path = out_dir / "bench_report.json"
    json_path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    # ── markdown 汇总（§四十一 表格式）──
    lines = [
        "# RouterBench v0.1 · Public Track 报告",
        "",
        f"> 生成于 {report['generated_at']} · 数据源 bench.db（各策略最新 run）",
        "",
        "## 全量（Core 1000 test 实例）",
        "",
        "| Strategy | Quality | Cost / query | Regret P50 | 选择分布 |",
        "|---|---:|---:|---:|---|",
    ]
    for s in strategies:
        if s not in runs:
            continue
        sm = runs[s][1]
        dist = ", ".join(f"{k.split('_')[-1] if '_' in k else k}:{v}" for k, v in sm["selection_dist"].items())
        lines.append(f"| {s} | {sm['quality_mean']:.4f} | ${sm['cost_mean']:.6f} | "
                     f"{sm['regret_p50']:.2f} | {dist} |")
    if compare:
        lines += [
            "",
            f"**LLooM（λ=0.5，run {compare['lloom_run']}）**：saving {compare['saving_vs_strong']:.1%} · "
            f"retention {compare['retention_vs_strong']:.1%} · gap_recovery {compare['gap_recovery']:.1%}",
        ]
    if aiq_val is not None:
        lines += ["", f"**AIQ** = {aiq_val:.4f}（{len(front)} 点前沿）"]
    lines += ["", "## Pareto 前沿（cost ↑ 排序）", ""]
    for p in front:
        lines.append(f"- {p['strategy']}: C=${p['cost']:.6f} Q={p['quality']:.4f}")
    lines += ["", "## λ sweep（LLooM cost_weight 曲线）", "", "| λ | Quality | Cost/query |", "|---:|---:|---:|"]
    for sp in sweep_points:
        lines.append(f"| {sp['cost_weight']:.1f} | {sp['quality']:.4f} | ${sp['cost']:.6f} |")
    lines += ["", "## Per-dataset（Q / C per query）", ""]
    for ds, strat_map in per_dataset_clean.items():
        lines += [f"### {ds}", "", "| Strategy | Quality | Cost |", "|---|---:|---:|"]
        for s in strategies:
            if s in strat_map:
                a = strat_map[s]
                lines.append(f"| {s} | {a['quality']:.4f} | ${a['cost']:.6f} |")
        lines.append("")
    md_path = out_dir / "bench_report.md"
    md_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

    print(f"✓ {json_path}")
    print(f"✓ {md_path}")
    if compare:
        print(f"  LLooM λ=0.5: saving {compare['saving_vs_strong']:.1%} / "
              f"retention {compare['retention_vs_strong']:.1%} / gap_recovery {compare['gap_recovery']:.1%}")
    if aiq_val is not None:
        print(f"  AIQ = {aiq_val:.4f} | Pareto 前沿 {len(front)} 点")
    return 0


if __name__ == "__main__":
    sys.exit(main())
