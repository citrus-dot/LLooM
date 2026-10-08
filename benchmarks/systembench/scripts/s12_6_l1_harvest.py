#!/usr/bin/env python3
"""s12_6_l1_harvest.py — S12.6-A：L1 Full Harvest（29 号 §三十三）。

33 independent 节点 × 3 模型 × 双 generation profile（P1536/P4096，29 号 §七/§八：
REVIEW 判定后 headline 必须双 profile 报告）= 198 cells。
节点口径：L1 = dependency_semantics=independent（base_text 即 execution_input，
无 artifact 依赖 → 严格 L1 counterfactual-capable，v0.2 routing headline 数据）。
split 纪律：calibration 7 节点 / test 26 节点分别可分列；采集本身不选节点（全量）。

judge：qwen-turbo（rubric v2，temp=0）；model_cost/judge_cost 分账。
断点续采：每 cell 落盘 + 启动跳过已有 (case,node,tier,profile)。

产出：matrices/semantic_outcomes_l1.jsonl
用法（仓库根目录）：
  DASHSCOPE_API_KEY=… benchmarks/routerbench/.venv/bin/python \
    benchmarks/systembench/scripts/s12_6_l1_harvest.py
"""

from __future__ import annotations

import json
import os
import sys
import time
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

import requests

_HERE = Path(__file__).resolve().parent
SB = _HERE.parent
CASES = SB / "cases" / "system_cases_v02.jsonl"
SIDECAR = SB / "cases" / "routing_inputs_v02.jsonl"
RUBRIC = SB / "evaluator" / "judge_rubric_v2.md"
OUTCOME = SB / "matrices" / "semantic_outcomes_l1.jsonl"
REPORT = SB / "reports" / "s12_6_l1_harvest.json"

ENDPOINT = "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions"
JUDGE_MODEL = "qwen-turbo"
COLLECT_MODELS = [("weak", "qwen3.6-flash", 0.1655, 0.9931),
                  ("mid", "qwen3.6-plus", 0.2759, 1.6552),
                  ("strong", "qwen3.6-max-preview", 1.2414, 7.4483)]
PROFILES = [("P1536", 1536), ("P4096", 4096)]
EXEC_SYSTEM_PROMPT = "你是一个专业的AI助手。请认真完成以下任务。"
EXEC_TEMPERATURE = 0.3
EXEC_EXTRA_PARAMS = {"enable_thinking": False}
JUDGE_TEMPERATURE = 0.0


def api_call(model, system, user, max_tokens, temperature, api_key, extra=None,
             retries=1):
    rec = {"latency_s": None, "error": None}
    body = {"model": model, "temperature": temperature, "max_tokens": max_tokens,
            "messages": [{"role": "system", "content": system},
                         {"role": "user", "content": user}]}
    if extra:
        body.update(extra)
    for attempt in range(retries + 1):
        t0 = time.time()
        try:
            resp = requests.post(
                ENDPOINT,
                headers={"Authorization": f"Bearer {api_key}",
                         "Content-Type": "application/json"},
                json=body, timeout=240)
        except Exception as e:  # noqa: BLE001
            rec["error"] = repr(e)
            if attempt < retries:
                time.sleep(5)
                continue
            return rec
        rec["latency_s"] = round(time.time() - t0, 2)
        if resp.status_code != 200:
            rec["error"] = f"HTTP {resp.status_code}: {resp.text[:200]}"
            if resp.status_code >= 500 or resp.status_code == 429:
                if attempt < retries:
                    time.sleep(8)
                    continue
            return rec
        d = resp.json()
        ch = (d.get("choices") or [{}])[0]
        u = d.get("usage") or {}
        rec.update({"http_status": 200,
                    "content": ch.get("message", {}).get("content") or "",
                    "finish_reason": ch.get("finish_reason"),
                    "prompt_tokens": u.get("prompt_tokens"),
                    "completion_tokens": u.get("completion_tokens")})
        return rec
    return rec


def validity(rec):
    if rec.get("error"):
        return "api_error"
    text = (rec.get("content") or "").strip()
    if len(text) < 10:
        return "empty_or_too_short"
    if any(p in text[:60] for p in ("抱歉", "对不起", "作为AI", "作为 AI", "无法完成")):
        return "refusal_pattern"
    return "truncated" if rec.get("finish_reason") == "length" else "ok"


def main() -> int:
    api_key = os.environ.get("DASHSCOPE_API_KEY", "")
    if not api_key:
        print("STOP: DASHSCOPE_API_KEY 未设置", file=sys.stderr)
        return 2
    cases = {}
    for l in CASES.read_text(encoding="utf-8").split("\n"):
        if l.strip():
            c = json.loads(l)
            cases[c["system_case_id"]] = c
    side = {(r["system_case_id"], r["node_id"]): r for r in
            (json.loads(l) for l in SIDECAR.read_text(encoding="utf-8").split("\n")
             if l.strip())}
    rubric = RUBRIC.read_text(encoding="utf-8")
    # L1 节点清单（case_id/node_id/split/role）
    l1 = []
    for cid, c in sorted(cases.items()):
        if c.get("case_role") != "headline_candidate":
            continue
        for n in c["nodes"]:
            if n.get("dependency_semantics") == "independent":
                l1.append({"cid": cid, "split": c["split"], "nid": n["node_id"],
                           "role": n.get("semantic_role"),
                           "text": side[(cid, n["node_id"])]["execution_input"]["base_text"]})
    assert len(l1) == 33, f"L1 应为 33 节点，实得 {len(l1)}"
    done: set[tuple] = set()
    rows: list[dict] = []
    if OUTCOME.exists():
        rows = [json.loads(l) for l in OUTCOME.read_text(encoding="utf-8").split("\n")
                if l.strip()]
        done = {(r["system_case_id"], r["node_id"], r["tier"], r["generation_profile"])
                for r in rows}
        if rows:
            print(f"断点续采：已有 {len(rows)} cells")
    cost_collect = cost_judge = 0.0
    n_new = 0
    for item in l1:
        cid, nid, text = item["cid"], item["nid"], item["text"]
        n = next(x for x in cases[cid]["nodes"] if x["node_id"] == nid)
        for profile, max_out in PROFILES:
            for tier, model, pin, pout in COLLECT_MODELS:
                if (cid, nid, tier, profile) in done:
                    continue
                rec = api_call(model, EXEC_SYSTEM_PROMPT, text, max_out,
                               EXEC_TEMPERATURE, api_key, EXEC_EXTRA_PARAMS)
                st = validity(rec)
                pt, ct = rec.get("prompt_tokens") or 0, rec.get("completion_tokens") or 0
                cost_collect += (pt * pin + ct * pout) / 1e6
                score, jst = None, "skipped"
                if st in ("ok", "truncated"):
                    jp = f"{rubric}\n\n## 任务要求\n{text}\n\n## 助手输出\n{rec.get('content','')}"
                    jrec = api_call(JUDGE_MODEL, "", jp, 300, JUDGE_TEMPERATURE, api_key)
                    jpt, jct = jrec.get("prompt_tokens") or 0, jrec.get("completion_tokens") or 0
                    judge_cost = (jpt * 0.0414 + jct * 0.0828) / 1e6
                    cost_judge += judge_cost
                    if jrec.get("content"):
                        try:
                            parsed = json.loads(jrec["content"].strip()
                                                .removeprefix("```json")
                                                .removesuffix("```").strip())
                            s = parsed.get("score")
                            if isinstance(s, (int, float)) and 0.0 <= s <= 1.0:
                                score, jst = round(float(s), 2), "ok"
                            else:
                                jst = "invalid_score"
                        except Exception:  # noqa: BLE001
                            jst = "invalid_json"
                    else:
                        jst = "judge_api_error"
                rows.append({
                    "system_case_id": cid, "split": item["split"],
                    "node_id": nid, "semantic_role": n.get("semantic_role"),
                    "dependency_semantics": "independent",
                    "tier": tier, "model": model,
                    "generation_profile": profile,
                    "max_output_tokens": max_out,
                    "execution_input_hash":
                        side[(cid, nid)]["execution_input"]["base_text_hash"],
                    "output": (rec.get("content") or "")[:6000],
                    "finish_reason": rec.get("finish_reason"),
                    "validity": st,
                    "prompt_tokens": pt, "completion_tokens": ct,
                    "cost_usd": round((pt * pin + ct * pout) / 1e6, 6),
                    "latency_s": rec.get("latency_s"),
                    "judge_score": score, "judge_status": jst,
                    "judge_model": JUDGE_MODEL,
                    "collected_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
                })
                n_new += 1
                print(f"  [{cid}/{nid} × {tier} × {profile}] {st} score={score} "
                      f"out={ct} lat={rec['latency_s']}s", flush=True)
                OUTCOME.write_text(
                    "\n".join(json.dumps(r, ensure_ascii=False) for r in rows) + "\n",
                    encoding="utf-8")
    # 汇总（双 profile × split 分列）
    summary_rows = rows
    by = defaultdict(list)
    for r in summary_rows:
        by[(r["generation_profile"], r["split"])].append(r)
    agg = {}
    for (profile, split), rs in sorted(by.items()):
        scores = [r["judge_score"] for r in rs if r["judge_score"] is not None]
        agg[f"{profile}/{split}"] = {
            "n_cells": len(rs),
            "truncated_rate": round(sum(1 for r in rs if r["validity"] == "truncated")
                                    / len(rs), 4),
            "judge_mean": round(sum(scores) / len(scores), 4) if scores else None,
            "by_tier_mean": {
                t: round(sum(r["judge_score"] for r in rs
                             if r["tier"] == t and r["judge_score"] is not None)
                         / max(1, sum(1 for r in rs
                                      if r["tier"] == t and r["judge_score"] is not None)), 4)
                for t in ("weak", "mid", "strong")},
        }
    report = {
        "phase": "S12.6-A L1 Full Harvest",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "n_cells": len(rows), "n_new_this_run": n_new,
        "target": "33 L1 independent × 3 models × 2 profiles = 198",
        "cost_this_run_usd": round(cost_collect + cost_judge, 4),
        "cost_collect_usd": round(cost_collect, 4),
        "cost_judge_usd": round(cost_judge, 4),
        "agg": agg,
    }
    REPORT.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n",
                      encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
