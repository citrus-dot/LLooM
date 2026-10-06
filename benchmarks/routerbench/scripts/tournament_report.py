#!/usr/bin/env python3
"""tournament_report.py — Day 9 Calibration Tournament 汇总（16-v3 §5/§十六）。

输入：
  bench.db 中 split='calibration' 的 runs（entrants × {P0 lloom / P1 band_only / P2 softgate-0 /
  BASE always_strong}，由 `lloom-cli bench replay --split calibration` 产生）
  tournament_entrants.json（池来源与选择依据）
  normalized/llmrouterbench/outcomes.jsonl（分数/成本矩阵，只读）

产出 pool_tournament_calibration.{json,csv}：
  每 entrant × policy：Q / C / 选择分布 / weak recall / precision / blocking rate
  每 entrant（池属性）：weak regret potential 分布（best_pool − weak 的 mean/median/p90/max、
  P(regret=0)=weak_oracle_share）
  派生：saving / retention vs 池内 Always-Strong；P2 vs P0 的 Q/C delta
  排序：P2 视角（16-v3 主问题 = "新池 × Soft Eligibility 能否救回 Weak"）

用法（仓库根目录）：
  benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/tournament_report.py \
    --normalized benchmarks/routerbench/normalized/llmrouterbench \
    --output benchmarks/routerbench/reports/pool_tournament_calibration.json
"""

from __future__ import annotations

import argparse
import csv
import json
import statistics
import sqlite3
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from weak_diagnosis import diagnose_run  # noqa: E402  复用同一套分类与指标口径

EPS = 1e-9


def policy_of(strategy: str, parameter: dict) -> str | None:
    if strategy == "always_strong":
        return "BASE"
    if strategy == "band_only":
        return "P1_band_only"
    if strategy == "lloom":
        sg = parameter.get("soft_gate_penalty")
        return "P0_current" if sg is None else ("P2_softgate0" if sg == 0 else None)
    return None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--bench-db", default="benchmarks/routerbench/bench.db")
    ap.add_argument("--entrants", default="benchmarks/routerbench/reports/tournament_entrants.json")
    ap.add_argument("--normalized", default="benchmarks/routerbench/normalized/llmrouterbench")
    ap.add_argument("--output", default="benchmarks/routerbench/reports/pool_tournament_calibration.json")
    args = ap.parse_args()

    entrants = json.loads(Path(args.entrants).read_text(encoding="utf-8"))["entrants"]
    pool_to_name = {"|".join([e["pool"]["weak"], e["pool"]["mid"], e["pool"]["strong"]]): e["name"]
                    for e in entrants}

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
    rows = conn.execute(
        "SELECT id, strategy, parameter_json, summary_json FROM bench_runs WHERE split='calibration' ORDER BY id"
    ).fetchall()

    # runs → (entrant, policy)
    grouped: dict[tuple[str, str], dict] = {}
    for rid, strategy, pjson, sjson in rows:
        p = json.loads(pjson)
        pol = policy_of(strategy, p)
        if pol is None:
            continue
        pool = p.get("pool") or {}
        key = "|".join([pool.get("weak", ""), pool.get("mid", ""), pool.get("strong", "")])
        name = pool_to_name.get(key)
        if name is None:
            continue
        if (name, pol) in grouped:
            continue  # 同 entrant × policy 取首个（重跑不覆盖，幂等）
        grouped[(name, pol)] = {"run_id": rid, "summary": json.loads(sjson), "pool": pool}

    # 池属性：weak regret potential（quality 维度；weak_regret(q)=max_pool−weak）
    pool_regret: dict[str, dict] = {}
    for (name, pol), g in grouped.items():
        if name in pool_regret or pol != "P0_current":
            continue
        pool = g["pool"]
        sample_ids = sorted({r[0] for r in conn.execute(
            "SELECT sample_id FROM bench_run_items WHERE run_id=?", (g["run_id"],))})
        regs = []
        for sid in sample_ids:
            scores = [matrix.get((sid, pool[m])) for m in ("weak", "mid", "strong")]
            if any(s is None for s in scores):
                continue
            regs.append(max(scores) - scores[0])
        regs.sort()
        n = len(regs)
        pool_regret[name] = {
            "n": n,
            "mean": round(statistics.fmean(regs), 4) if n else None,
            "median": round(regs[n // 2], 4) if n else None,
            "p90": round(regs[int(n * 0.90)], 4) if n else None,
            "max": round(regs[-1], 4) if n else None,
            "p_regret_zero": round(sum(1 for r in regs if r <= EPS) / n, 4) if n else None,
            "note": "weak_regret(q)=max_pool_score(q)−weak_score(q)；P(regret=0)=weak_oracle_share；"
                    "accuracy 类 dataset 上 regret∈{0,1}，judge 类连续——解读须分 dataset（16-v3 §八）",
        }

    # 每 entrant × policy 汇总
    results = []
    for e in entrants:
        name = e["name"]
        base = grouped.get((name, "BASE"))
        base_q = base["summary"]["quality_mean"] if base else None
        base_c = base["summary"]["cost_mean"] if base else None
        for pol in ("P0_current", "P1_band_only", "P2_softgate0", "BASE"):
            g = grouped.get((name, pol))
            if not g:
                continue
            rid = g["run_id"]
            s = g["summary"]
            diag = diagnose_run(conn, rid, matrix, costs, g["pool"])
            q = s["quality_mean"]
            c = s["cost_mean"]
            results.append({
                "entrant": name,
                "policy": pol,
                "run_id": rid,
                "n": s["n"],
                "quality_mean": round(q, 4),
                "cost_mean": round(c, 6),
                "saving_vs_base": round(1 - c / base_c, 4) if (base_c and pol != "BASE") else None,
                "retention_vs_base": round(q / base_q, 4) if (base_q and pol != "BASE") else None,
                "weak_recall": diag["weak_recall"],
                "weak_precision": diag["weak_precision"],
                "weak_blocking_rate": diag["weak_blocking_rate"],
                "error_counts": diag["error_counts"],
                "selection_dist": s["selection_dist"],
                "per_dataset": diag["per_dataset"],
                "pool_regret_potential": pool_regret.get(name),
                "pool": g["pool"],
            })

    # P2 vs P0 delta（每 entrant）
    deltas = []
    for e in entrants:
        name = e["name"]
        p0 = next((r for r in results if r["entrant"] == name and r["policy"] == "P0_current"), None)
        p2 = next((r for r in results if r["entrant"] == name and r["policy"] == "P2_softgate0"), None)
        if p0 and p2:
            deltas.append({
                "entrant": name,
                "d_quality_pp": round((p2["quality_mean"] - p0["quality_mean"]) * 100, 2),
                "d_cost_pct": round((p2["cost_mean"] / p0["cost_mean"] - 1) * 100, 2) if p0["cost_mean"] else None,
                "weak_share_p0": round(sum(v for k, v in p0["selection_dist"].items()
                                           if k == p0["pool"]["weak"]) / p0["n"], 4),
                "weak_share_p2": round(sum(v for k, v in p2["selection_dist"].items()
                                           if k == p2["pool"]["weak"]) / p2["n"], 4),
                "pareto_vs_base": (p2["retention_vs_base"] is not None and p2["retention_vs_base"] >= 1.0
                                   and p2["saving_vs_base"] is not None and p2["saving_vs_base"] > 0),
            })

    report = {
        "generated_at": __import__("datetime").datetime.now(__import__("datetime").timezone.utc).isoformat(timespec="seconds"),
        "split": "calibration（test 未触碰，16-v3 §十四）",
        "entrants_file": args.entrants,
        "results": results,
        "p2_vs_p0": deltas,
        "notes": [
            "策略族：P0=Current（weak tier1，medium 被拒）/ P1=BandOnly / P2=Soft Eligibility（weak tier2，penalty=0）"
            " / BASE=池内 Always-Strong（16-v3 §六）",
            "aggregate 为池覆盖 dataset 的实例合并值；dataset 间 n 与分数量纲差异大，判读须结合 per_dataset",
            "weak regret potential 是池属性（与 policy 无关），口径见字段 note",
        ],
    }
    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    csv_path = out.with_suffix(".csv")
    flat = [{k: v for k, v in r.items() if k not in ("per_dataset", "pool", "error_counts", "selection_dist",
                                                    "pool_regret_potential")} for r in results]
    with csv_path.open("w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=list(flat[0].keys()))
        w.writeheader()
        w.writerows(flat)

    print(f"✓ {out} (+ {csv_path.name})")
    print(f"{'entrant':<22}{'policy':<16}{'n':>6}{'Q':>8}{'C/q':>11}{'save':>7}{'reten':>7}"
          f"{'w_recall':>9}{'w_prec':>8}")
    for r in sorted(results, key=lambda r: (r["entrant"], r["policy"])):
        sv = f"{r['saving_vs_base']:.1%}" if r["saving_vs_base"] is not None else "-"
        rt = f"{r['retention_vs_base']:.1%}" if r["retention_vs_base"] is not None else "-"
        print(f"{r['entrant']:<22}{r['policy']:<16}{r['n']:>6}{r['quality_mean']:>8.4f}"
              f"{r['cost_mean']:>11.6f}{sv:>7}{rt:>7}"
              f"{str(r['weak_recall']):>9}{str(r['weak_precision']):>8}")
    print("\nP2 vs P0：")
    for d in deltas:
        print(f"  {d['entrant']:<22} dQ={d['d_quality_pp']:+.2f}pp dC={d['d_cost_pct']:+.2f}% "
              f"weak_share {d['weak_share_p0']}→{d['weak_share_p2']}"
              f"{' ⭐Pareto vs BASE' if d['pareto_vs_base'] else ''}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
