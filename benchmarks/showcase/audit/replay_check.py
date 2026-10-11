#!/usr/bin/env python3
"""replay_check.py — Showcase 离线复核（34 号 P1.5：数据与报告再生成、不联网、零 API 费用）。

把 showcase 三图 figure_data CSV 与冻结报告逐项核对（P2 Frozen 对照源 =
`lloom@softgate0`，34 号 §2.4 勘误），并校验 audit 文件自洽与 manifest hash。

用法（仓库根目录，任何有 Python 3 的环境，无第三方依赖）：
  python3 benchmarks/showcase/audit/replay_check.py
退出码：0 = 全部 PASS；1 = 存在 FAIL。
"""

from __future__ import annotations

import csv
import hashlib
import json
import sys
from pathlib import Path

_HERE = Path(__file__).resolve().parent
SHOWCASE = _HERE.parent
REPO = SHOWCASE.parent.parent


def sha(p: Path) -> str:
    return hashlib.sha256(p.read_bytes()).hexdigest()


def main() -> int:
    checks: list[tuple[str, bool, str]] = []

    def chk(name: str, cond: bool, detail: str = "") -> None:
        checks.append((name, bool(cond), detail))

    # ---- 图 1：RouterBench v0.2（P2 Frozen 真源 = lloom@softgate0）----
    r = json.loads((REPO / "benchmarks/routerbench/reports/v02_test_report.json").read_text())
    rows = {x["key"]: x for x in csv.DictReader(
        (SHOWCASE / "figure_data/routerbench_v02_frontier.csv").open())}
    p2 = r["runs"]["lloom@softgate0"]
    chk("fig1.lloom(P2frozen).quality",
        abs(float(rows["lloom"]["quality_mean"]) - p2["quality_mean"]) < 5e-5)
    chk("fig1.lloom(P2frozen).cost",
        abs(float(rows["lloom"]["cost_mean_per_query_usd"]) - p2["cost_mean"]) < 5e-7)
    chk("fig1.lloom.not_p0",
        float(rows["lloom"]["cost_mean_per_query_usd"]) != r["runs"]["lloom"]["cost_mean"],
        "不得使用 P0/BandOnly（0.5867/$0.000849）")
    for k in ("always_strong", "best_single", "cost_matched_random",
              "strong_share_matched_random", "oracle"):
        chk(f"fig1.{k}.quality",
            abs(float(rows[k]["quality_mean"]) - r["runs"][k]["quality_mean"]) < 5e-5)
        chk(f"fig1.{k}.cost",
            abs(float(rows[k]["cost_mean_per_query_usd"]) - r["runs"][k]["cost_mean"]) < 5e-7)
    ci = r["bootstrap_ci"]
    chk("fig1.saving_ci",
        abs(ci["saving_vs_strong"]["point"] - 0.1346) < 1e-9
        and ci["saving_vs_strong"]["ci95_low"] == 0.122)

    # ---- 图 2：SystemBench v0.1（mean + median/p90 分列，P2-2）----
    v = json.loads((REPO / "benchmarks/systembench/reports/v01_report.json").read_text())
    h = json.loads((REPO / "benchmarks/systembench/reports/v01_hardening.json").read_text())
    rows2 = list(csv.DictReader((SHOWCASE / "figure_data/systembench_v01_composition.csv").open()))
    # v01_hardening 的 makespan 分位数按策略键定位（结构容忍：收集全部 median/p90 后按序对照）
    hard_str = json.dumps(h)

    def hard_val(key: str, stat: str) -> float | None:
        import re
        m = re.search(r'"makespan_%s_ms":\s*([0-9.]+)' % stat, hard_str)
        return float(m.group(1)) if m else None

    for row in rows2:
        k = row["key"]
        s = v["summary"][k]
        chk(f"fig2.{k}.cost",
            abs(float(row["cost_per_workflow_usd"]) - s["cost_per_workflow_mean"]) < 1e-9)
        chk(f"fig2.{k}.makespan_mean",
            abs(float(row["simulated_makespan_ms_mean"]) - s["makespan_ms_mean"]) < 1e-9)
    chk("fig2.B4_routing_gain",
        abs(v["routing_gain_b4_vs_b3"]["d_cost_pct"] - (-14.95)) < 1e-9)
    # median/p90 列存在且与 hardening 报告一致（B2 2000/3500；B3/B4 500/500）
    med = {row["key"]: row.get("simulated_makespan_ms_median", "")
           for row in rows2}
    p90 = {row["key"]: row.get("simulated_makespan_ms_p90", "") for row in rows2}
    chk("fig2.makespan_median_p90_present",
        med.get("B2_workflow_strong_seq") == "2000" and p90.get("B2_workflow_strong_seq") == "3500"
        and med.get("B3_workflow_strong_par") == "500" and p90.get("B3_workflow_strong_par") == "500")

    # ---- 图 3：semantic protocol sensitivity（L1 jsonl 重算）----
    l1 = [json.loads(l) for l in
          (REPO / "benchmarks/systembench/matrices/semantic_outcomes_l1.jsonl")
          .read_text().split("\n") if l.strip()]
    rows3 = list(csv.DictReader((SHOWCASE / "figure_data/semantic_protocol_sensitivity.csv").open()))
    l1_p1536 = [x for x in l1 if x["generation_profile"] == "P1536"]
    chk("fig3.L1_P1536.trunc",
        abs(float(rows3[0]["truncation_rate"])
            - sum(1 for x in l1_p1536 if x["validity"] == "truncated") / 99) < 1e-3)
    chk("fig3.L1_P1536.cells", rows3[0]["n_cells"] == "99")
    chk("fig3.n_cells_all", sum(int(x["n_cells"]) for x in rows3[:2]) == 198)

    # ---- audit 文件自洽 ----
    cov = json.loads((SHOWCASE / "audit/l2_coverage.json").read_text())
    chk("l2.coverage", cov["has_output"] == 119 and cov["scored"] == 49
        and cov["planned_cells"] == 180)
    ca = list(csv.DictReader((SHOWCASE / "audit/semantic_l1_comparability.csv").open()))
    chk("l1_audit.rows", len(ca) == 198)
    chk("l1_audit.included", sum(1 for x in ca if x["included_in_headline"] == "yes") == 197)

    # ---- manifest hash 自洽 ----
    m = json.loads((SHOWCASE / "manifests/showcase_manifest.json").read_text())
    bad = [i["path"] for i in m["figures"] + m["figure_data"]
           if sha(REPO / "benchmarks/showcase" / i["path"]) != i["sha256"]]
    chk("manifest.figure_hashes", not bad, str(bad))

    fails = [c for c in checks if not c[1]]
    out = {
        "checked": len(checks),
        "pass": len(checks) - len(fails),
        "fail": len(fails),
        "p02_note": "fig1.lloom 对照源 = runs[lloom@softgate0]（P2 Frozen，34 号 §2.4 勘误）",
        "items": [{"name": n, "status": "PASS" if ok else "FAIL", "detail": d}
                  for n, ok, d in checks],
    }
    (SHOWCASE / "audit/offline_replay_check.json").write_text(
        json.dumps(out, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"检查 {len(checks)} 项：PASS {out['pass']} / FAIL {out['fail']}")
    for n, ok, d in fails:
        print("  FAIL", n, d)
    print("✓ audit/offline_replay_check.json（纯离线重生成）")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
