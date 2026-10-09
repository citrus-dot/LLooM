#!/usr/bin/env python3
"""s12_6_b_l2_probe.py — S12.6-B：L2 Conditional Probe（29 号 §三十三）。

30 artifact_dependent 节点（stage0 冻结 final_text 全部就绪，零成本复用）
× 3 模型 × 双 profile（P1536/P4096）= 180 cells。
b 方案模型配置（同 s12_6_l1_resume）：weak=qwen3.6-flash / mid=qwen3.7-plus /
strong=qwen3.6-max-preview；judge=qwen3.7-max（rubric v3；flash 已耗尽）。

研究问题（29 号 §S12.6-B）：artifact 是否真正改变 winner——
对比同一模型在 L1（base_text）与 L2（final_text=base+冻结 artifacts）条件下的
分数位移与 winner 分布差异。产出 conditional quality / artifact-conditioned
routing 的第一版证据（L2 冻结产物不冒充 end-to-end live）。

断点续采：每 cell 落盘 + 启动跳过已有 (case,node,tier,profile)。
产出：matrices/semantic_outcomes_l2.jsonl + reports/s12_6_b_l2_probe.json
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
RUBRIC = SB / "evaluator" / "judge_rubric_v3.md"
OUTCOME = SB / "matrices" / "semantic_outcomes_l2.jsonl"
REPORT = SB / "reports" / "s12_6_b_l2_probe.json"

ENDPOINT = "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions"
MODELS = [("weak", "qwen3.6-flash", 0.1655, 0.9931),
          ("mid", "qwen3.7-plus", 0.2759, 1.1034),
          ("strong", "qwen3.6-max-preview", 1.2414, 7.4483)]
JUDGE_MODEL = "qwen3.7-max"
JUDGE_PIN, JUDGE_POUT = 0.2759, 1.1034
PROFILES = [("P1536", 1536), ("P4096", 4096)]
EXEC_SYSTEM_PROMPT = "你是一个专业的AI助手。请认真完成以下任务。"
EXEC_TEMPERATURE = 0.3
EXEC_EXTRA_PARAMS = {"enable_thinking": False}
JUDGE_TEMPERATURE = 0.0


def api_call(model, system, user, max_tokens, temperature, api_key, extra=None,
             retries=2):
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
                time.sleep(8)
                continue
            return rec
        rec["latency_s"] = round(time.time() - t0, 2)
        if resp.status_code != 200:
            rec["error"] = f"HTTP {resp.status_code}: {resp.text[:200]}"
            if resp.status_code in (429, 500, 502, 503) or (
                    resp.status_code == 403 and "Free quota" not in resp.text):
                if attempt < retries:
                    time.sleep(10)
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
    l2 = []
    for (cid, nid), r in sorted(side.items()):
        if not r["execution_input"].get("final_frozen"):
            continue
        n = next(x for x in cases[cid]["nodes"] if x["node_id"] == nid)
        if n["dependency_semantics"] != "artifact_dependent":
            continue
        l2.append({"cid": cid, "split": cases[cid]["split"], "nid": nid,
                   "role": n.get("semantic_role"),
                   "final_text": r["execution_input"]["final_text"],
                   "base_text": r["execution_input"]["base_text"]})
    assert len(l2) == 30, f"L2 应 30 节点，实得 {len(l2)}"
    print(f"L2 conditional probe: {len(l2)} 节点 × 3 模型 × 2 profile = {len(l2)*6} cells")
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
    for item in l2:
        cid, nid = item["cid"], item["nid"]
        n = next(x for x in cases[cid]["nodes"] if x["node_id"] == nid)
        for profile, max_out in PROFILES:
            for tier, model, pin, pout in MODELS:
                if (cid, nid, tier, profile) in done:
                    continue
                rec = api_call(model, EXEC_SYSTEM_PROMPT, item["final_text"],
                               max_out, EXEC_TEMPERATURE, api_key, EXEC_EXTRA_PARAMS)
                st = validity(rec)
                pt, ct = rec.get("prompt_tokens") or 0, rec.get("completion_tokens") or 0
                cost_collect += (pt * pin + ct * pout) / 1e6
                score, jst = None, "skipped"
                if st in ("ok", "truncated"):
                    jp = f"{rubric}\n\n## 任务要求\n{item['final_text']}\n\n## 助手输出\n{rec.get('content','')}"
                    jrec = api_call(JUDGE_MODEL, "", jp, 300, JUDGE_TEMPERATURE, api_key)
                    jpt, jct = jrec.get("prompt_tokens") or 0, jrec.get("completion_tokens") or 0
                    cost_judge += (jpt * JUDGE_PIN + jct * JUDGE_POUT) / 1e6
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
                    "dependency_semantics": "artifact_dependent",
                    "tier": tier, "model": model,
                    "generation_profile": profile,
                    "max_output_tokens": max_out,
                    "execution_input_hash":
                        side[(cid, nid)]["execution_input"]["final_hash"],
                    "upstream_artifact_source": "stage0_frozen_qwen36_plus",
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
                print(f"  [{cid}/{nid} × {tier} × {profile}] {st} score={score} "
                      f"out={ct} lat={rec['latency_s']}s", flush=True)
                OUTCOME.write_text(
                    "\n".join(json.dumps(x, ensure_ascii=False) for x in rows) + "\n",
                    encoding="utf-8")
    # 汇总：L2 winner 分布 + L1 vs L2 同模型分数位移（artifact 效应第一版证据）
    l1_rows = [json.loads(l) for l in
               (SB / "matrices" / "semantic_outcomes_l1.jsonl").read_text(encoding="utf-8").split("\n")
               if l.strip()] if (SB / "matrices" / "semantic_outcomes_l1.jsonl").exists() else []
    l1_by = {(r["system_case_id"], r["node_id"], r["tier"], r["generation_profile"]): r
             for r in l1_rows if r["judge_status"] == "ok"}
    deltas = []
    for r in rows:
        if r["judge_status"] != "ok":
            continue
        k = (r["system_case_id"], r["node_id"], r["tier"], r["generation_profile"])
        # 同 case 的 L1 对照取该 case 的 independent 节点（artifact 无→有是 case 级对照）
        l1_same_case = [v for kk, v in l1_by.items()
                        if kk[0] == r["system_case_id"] and kk[2] == r["tier"]
                        and kk[3] == r["generation_profile"]]
        if l1_same_case:
            deltas.append(r["judge_score"] - l1_same_case[0]["judge_score"])
    agg = {}
    for profile, max_out in PROFILES:
        rs = [r for r in rows if r["generation_profile"] == profile]
        scores = [r["judge_score"] for r in rs if r["judge_score"] is not None]
        by_tier = {}
        for t in ("weak", "mid", "strong"):
            ss = [r["judge_score"] for r in rs
                  if r["tier"] == t and r["judge_score"] is not None]
            by_tier[t] = round(sum(ss) / len(ss), 4) if ss else None
        agg[profile] = {
            "n_cells": len(rs), "truncated_rate": round(
                sum(1 for r in rs if r["validity"] == "truncated") / len(rs), 4),
            "judge_ok_rate": round(
                sum(1 for r in rs if r["judge_status"] == "ok") / len(rs), 4),
            "judge_mean": round(sum(scores) / len(scores), 4) if scores else None,
            "by_tier_mean": by_tier,
        }
    report = {
        "phase": "S12.6-B L2 Conditional Probe",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "n_cells": len(rows),
        "upstream_artifacts": "stage0 冻结（qwen3.6-plus 参考执行，routing_inputs_v02.jsonl final_hash）",
        "artifact_effect_delta": {
            "n": len(deltas),
            "mean_delta_l2_minus_l1": round(sum(deltas) / len(deltas), 4) if deltas else None,
            "nonzero_rate": round(sum(1 for d in deltas if abs(d) > 1e-9) / len(deltas), 4) if deltas else None,
            "note": "L1 对照 = 同 case 的 independent 节点同模型同 profile 分数；方向性证据",
        },
        "cost_this_run_usd": round(cost_collect + cost_judge, 4),
        "agg": agg,
    }
    REPORT.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n",
                      encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
