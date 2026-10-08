#!/usr/bin/env python3
"""s12_5_5_pool_reaudit.py — S12.5.5 Pool Re-audit（29 号 §三十三）。

在两个 generation profile 口径下分别重算 pool 可路由性指标（§十五），
P1536 用 probe 24 节点全量、P4096 用 protocol probe 6 节点（样本小，方向性参考）。
零成本（只读已有 outcomes）。

产出：reports/s12_5_5_pool_reaudit.json
"""

from __future__ import annotations

import json
import math
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

_HERE = Path(__file__).resolve().parent
SB = _HERE.parent
BASE = SB / "matrices" / "semantic_outcomes_probe.jsonl"          # P1536 24 节点
PROTO = SB / "matrices" / "semantic_outcomes_protocol_probe.jsonl"  # P4096 6 节点
ASSIGN = Path("/tmp/sb_v02_probe_assignments.jsonl")               # R1 band（P2）
REPORT = SB / "reports" / "s12_5_5_pool_reaudit.json"
TIERS = ("weak", "mid", "strong")
EPS = 0.1


def load(path: Path) -> list[dict]:
    return [json.loads(l) for l in path.read_text(encoding="utf-8").split("\n")
            if l.strip()]


def metrics(rows: list[dict], bands: dict[str, str]) -> dict:
    cells: dict[tuple[str, str], dict[str, dict]] = defaultdict(dict)
    for r in rows:
        cells[(r["system_case_id"], r["node_id"])][r["tier"]] = r
    headrooms, win = [], defaultdict(int)
    cheap_hits = cheap_elig = 0
    cost = defaultdict(list)
    score = defaultdict(list)
    per_case = defaultdict(lambda: defaultdict(int))
    mid_elig = mid_oracle = mid_sel = weak_elig = weak_oracle = weak_sel = 0
    n = 0
    for key, bytier in sorted(cells.items()):
        if len(bytier) < 3 or any(bytier[t].get("judge_score") is None for t in TIERS):
            continue
        n += 1
        s = {t: bytier[t]["judge_score"] for t in TIERS}
        c = {t: bytier[t]["cost_usd"] for t in TIERS}
        for t in TIERS:
            cost[t].append(c[t])
            score[t].append(s[t])
        headrooms.append(max(s.values()) - min(s.values()))
        w = sorted(TIERS, key=lambda t: (-s[t], c[t]))[0]
        win[w] += 1
        per_case[key[0]][w] += 1
        if "weak" in s and "strong" in s:
            cheap_elig += 1
            if s["weak"] >= s["strong"] - EPS:
                cheap_hits += 1
        band = bands.get(f"{key[0]}__{key[1]}")
        sel = None
        a = ASSIGN.exists() and {
            json.loads(l)["query_id"]: json.loads(l)["selected_model"]
            for l in ASSIGN.read_text().split("\n") if l.strip()}
        sel = a.get(f"{key[0]}__{key[1]}")
        if band != "hard":
            weak_elig += 1
            mid_elig += 1
            if w == "weak":
                weak_oracle += 1
            if w == "mid":
                mid_oracle += 1
            if sel == "deephermes_3_llama_3_8b_preview":
                weak_sel += 1
            if sel == "deepseek_r1_distill_qwen_7b":
                mid_sel += 1
    mean = lambda xs: round(sum(xs) / len(xs), 4) if xs else None  # noqa: E731
    ent = (-sum((v / n) * math.log2(v / n) for v in win.values())
           if n and len(win) > 1 else 0.0)
    dominant = {}
    for cid, v in per_case.items():
        tot = sum(v.values())
        dominant[cid] = ("mixed" if max(v.values()) * 2 <= tot
                         else max(v, key=v.get))
    return {
        "n_nodes": n,
        "quality_headroom_mean": mean(headrooms),
        "nonzero_headroom_rate": mean([1.0 if h > 1e-9 else 0.0 for h in headrooms]),
        "mean_score": {t: mean(score[t]) for t in TIERS},
        "mean_cost": {t: mean(cost[t]) for t in TIERS},
        "cost_ratio_strong_over_weak": (round(mean(cost["strong"]) / mean(cost["weak"]), 2)
                                        if mean(cost["weak"]) else None),
        "cheap_safe_rate": round(cheap_hits / cheap_elig, 4) if cheap_elig else None,
        "weak_opportunity": {"eligible_rate": round(weak_elig / n, 4) if n else None,
                             "oracle_share": round(weak_oracle / weak_elig, 4) if weak_elig else None,
                             "p2_selected_share": round(weak_sel / weak_elig, 4) if weak_elig else None},
        "mid_opportunity": {"eligible_rate": round(mid_elig / n, 4) if n else None,
                            "oracle_share": round(mid_oracle / mid_elig, 4) if mid_elig else None,
                            "p2_selected_share": round(mid_sel / mid_elig, 4) if mid_elig else None},
        "winner_distribution": dict(win),
        "winner_entropy_bits": round(ent, 4),
        "per_case_dominant": dominant,
    }


def main() -> None:
    bands = {}
    if ASSIGN.exists():
        for l in ASSIGN.read_text().split("\n"):
            if l.strip():
                a = json.loads(l)
                bands[a["query_id"]] = a["band"]
    base_rows = load(BASE)
    proto_rows = load(PROTO)
    # P1536 口径：probe 24 节点全量；P4096 口径：6 节点（方向性）
    m_a = metrics(base_rows, bands)
    m_b = metrics(proto_rows, bands)
    # 共同 6 节点上的 P1536 对照（同节点同 band 可比）
    six = {(r["system_case_id"], r["node_id"]) for r in proto_rows}
    m_a6 = metrics([r for r in base_rows if (r["system_case_id"], r["node_id"]) in six],
                   bands)
    report = {
        "phase": "S12.5.5 Pool Re-audit",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "context": "S12.5.4 verdict=REVIEW（agreement 0.667）→ 双 profile 口径重审 pool 可路由性",
        "P1536_probe24": m_a,
        "P1536_same6": m_a6,
        "P4096_same6": m_b,
        "reading": {
            "pool_still_complementary_P4096": (
                m_b["winner_entropy_bits"] > 0
                and m_b["quality_headroom_mean"] and m_b["quality_headroom_mean"] > 0),
            "note": "P4096 仅 6 节点，方向性参考；S12.6-A 双 profile 全量后再正式判定",
        },
    }
    REPORT.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n",
                      encoding="utf-8")
    for k in ("P1536_probe24", "P1536_same6", "P4096_same6"):
        m = report[k]
        print(f"[{k}] n={m['n_nodes']} headroom={m['quality_headroom_mean']} "
              f"score={m['mean_score']} winner={m['winner_distribution']} "
              f"entropy={m['winner_entropy_bits']} cheap_safe={m['cheap_safe_rate']}")
    print(f"✓ {REPORT}")


if __name__ == "__main__":
    main()
