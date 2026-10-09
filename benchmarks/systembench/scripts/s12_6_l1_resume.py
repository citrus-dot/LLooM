#!/usr/bin/env python3
"""s12_6_l1_resume.py — S12.6-A b 方案补采 + judge 统一重评（Nironta 拍板 b，2026-10-09）。

额度事实（2026-10-09 实测）：qwen-plus/qwen-turbo/qwen3.6-plus 403 Arrearage；
qwen3.6-flash / qwen3.6-max-preview 仍 200 OK；qwen3.7-plus / qwen3.8-flash / qwen3.8-max
200 OK（新一代独立免费额度）。

b 方案配置（本脚本固化）：
  weak   = qwen3.6-flash        （原池，额度仍在；L1 已采 65/66）
  mid    = qwen3.7-plus          （mid 候选代采 qwen3.6-plus 缺口，¥2/¥8，同档代际对照；
                                  S12.7 freeze 时作为 system_pool_v01 变更申报审议）
  strong = qwen3.6-max-preview   （原池，额度仍在；L1 已采 66/66）
  judge  = qwen3.8-flash        （池外 judge，¥0.8/¥2.7，rubric 独立性保持；
                                  qwen-turbo 额度耗尽换档 → rubric v3）

动作：
  1) 补采 mid 54 个 api_error cells + 1 weak api_error（双 profile 同协议）
  2) judge 统一重评：**全部 198 cells**（rubric v3 = v2 正文逐字不变仅换 judge 模型；
     旧 turbo 72 个分数作废换档——避免跨 judge 口径混算）
  3) 汇总双 profile × split 全指标

产出：semantic_outcomes_l1.jsonl（原文件 cell 级 update：mid 重采覆盖 api_error 行，
judge 字段统一重评）+ reports/s12_6_l1_resume.json + evaluator/judge_rubric_v3.md

用法：DASHSCOPE_API_KEY=… benchmarks/routerbench/.venv/bin/python \
  benchmarks/systembench/scripts/s12_6_l1_resume.py
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
OUTCOME = SB / "matrices" / "semantic_outcomes_l1.jsonl"
REPORT = SB / "reports" / "s12_6_l1_resume.json"
RUBRIC_V2 = SB / "evaluator" / "judge_rubric_v2.md"
RUBRIC_V3 = SB / "evaluator" / "judge_rubric_v3.md"

ENDPOINT = "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions"
MID_MODEL = ("mid", "qwen3.7-plus", 0.2759, 1.1034)   # ¥2/¥8 ÷ 7.25
JUDGE_MODEL = "qwen3.8-flash"
JUDGE_PIN, JUDGE_POUT = 0.1103, 0.3724                # ¥0.8/¥2.7 ÷ 7.25
WEAK = ("weak", "qwen3.6-flash", 0.1655, 0.9931)
STRONG = ("strong", "qwen3.6-max-preview", 1.2414, 7.4483)
ALL_MODELS = {WEAK[0]: WEAK, MID_MODEL[0]: MID_MODEL, STRONG[0]: STRONG}
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
            if resp.status_code >= 500 or resp.status_code == 429 or resp.status_code == 403:
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
                    "model_echo": d.get("model"),
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


def judge_one(rubric, task_text, output_text, api_key):
    jp = f"{rubric}\n\n## 任务要求\n{task_text}\n\n## 助手输出\n{output_text}"
    jrec = api_call(JUDGE_MODEL, "", jp, 300, JUDGE_TEMPERATURE, api_key)
    jpt, jct = jrec.get("prompt_tokens") or 0, jrec.get("completion_tokens") or 0
    jcost = (jpt * JUDGE_PIN + jct * JUDGE_POUT) / 1e6
    if not jrec.get("content"):
        return None, "judge_api_error", jcost
    try:
        parsed = json.loads(jrec["content"].strip()
                            .removeprefix("```json").removesuffix("```").strip())
        s = parsed.get("score")
        if isinstance(s, (int, float)) and 0.0 <= s <= 1.0:
            return round(float(s), 2), "ok", jcost
        return None, "invalid_score", jcost
    except Exception:  # noqa: BLE001
        return None, "invalid_json", jcost


def main() -> int:
    api_key = os.environ.get("DASHSCOPE_API_KEY", "")
    if not api_key:
        print("STOP: DASHSCOPE_API_KEY 未设置", file=sys.stderr)
        return 2
    # rubric v3 = v2 正文逐字不变，仅换 judge 模型行（避免跨 judge 混算）
    v2 = RUBRIC_V2.read_text(encoding="utf-8")
    v3 = v2.replace("# Judge Rubric v2（frozen）", "# Judge Rubric v3（frozen）")
    v3 = v3.replace("- 版本：v2，冻结于 2026-10-08。变更：judge 模型 qwen-plus → qwen-turbo\n"
                    "  （v1 冻结当日 qwen-plus 免费额度耗尽 403 Arrearage，额度约束下换档；\n"
                    "  rubric 正文/锚点/附加规则与 v1 逐字一致，仅 judge 模型与版本号变化）。",
                    "- 版本：v3，冻结于 2026-10-09。变更：judge 模型 qwen-turbo → qwen3.8-flash\n"
                    "  （turbo 免费额度同日耗尽；b 方案 Nironta 拍板，judge 换新一代池外模型；\n"
                    "  正文/锚点/附加规则与 v2 逐字一致）。\n"
                    "- 历史换档：v1 qwen-plus（耗尽）→ v2 qwen-turbo（耗尽）→ v3 qwen3.8-flash。")
    if "v3" not in v3 or "qwen3.8-flash" not in v3:
        raise SystemExit("rubric v3 生成失败（v2 文本与预期不符）")
    RUBRIC_V3.write_text(v3, encoding="utf-8")
    sha_v3 = __import__("hashlib").sha256(v3.encode()).hexdigest()
    print(f"rubric v3 落盘 sha256={sha_v3[:16]}")

    rows = [json.loads(l) for l in OUTCOME.read_text(encoding="utf-8").split("\n")
            if l.strip()]
    side = {(r["system_case_id"], r["node_id"]): r for r in
            (json.loads(l) for l in SIDECAR.read_text(encoding="utf-8").split("\n")
             if l.strip())}
    cases = {}
    for l in CASES.read_text(encoding="utf-8").split("\n"):
        if l.strip():
            c = json.loads(l)
            cases[c["system_case_id"]] = c
    cost_collect = cost_judge = 0.0
    n_resampled = n_rejudged = 0

    # ---- 1) 补采 api_error cells（mid 54 + weak 1）----
    for i, r in enumerate(rows):
        if r["validity"] != "api_error":
            continue
        tier = r["tier"]
        _, model, pin, pout = ALL_MODELS[tier]
        text = side[(r["system_case_id"], r["node_id"])]["execution_input"]["base_text"]
        rec = api_call(model, EXEC_SYSTEM_PROMPT, text, r["max_output_tokens"],
                       EXEC_TEMPERATURE, api_key, EXEC_EXTRA_PARAMS)
        st = validity(rec)
        pt, ct = rec.get("prompt_tokens") or 0, rec.get("completion_tokens") or 0
        rows[i].update({
            "output": (rec.get("content") or "")[:6000],
            "finish_reason": rec.get("finish_reason"),
            "validity": st,
            "prompt_tokens": pt, "completion_tokens": ct,
            "cost_usd": round((pt * pin + ct * pout) / 1e6, 6),
            "latency_s": rec.get("latency_s"),
            "model": model,
            "model_substitute_note": ("qwen3.6-plus 额度耗尽，qwen3.7-plus 代采"
                                      if tier == "mid" else None) if tier == "mid" else None,
            "collected_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        })
        cost_collect += rows[i]["cost_usd"]
        n_resampled += 1
        print(f"  [补采 {r['system_case_id']}/{r['node_id']} × {tier} × {r['generation_profile']}] "
              f"{st} out={ct}", flush=True)
        OUTCOME.write_text("\n".join(json.dumps(x, ensure_ascii=False) for x in rows)
                           + "\n", encoding="utf-8")

    # ---- 2) judge 统一重评全部 198 cells（rubric v3）----
    for i, r in enumerate(rows):
        text = (side[(r["system_case_id"], r["node_id"])]["execution_input"].get("base_text")
                or side[(r["system_case_id"], r["node_id"])]["execution_input"].get("final_text"))
        score, jst, jcost = None, "skipped", 0.0
        if r["validity"] in ("ok", "truncated") and text and r.get("output"):
            score, jst, jcost = judge_one(v3, text, r["output"], api_key)
            cost_judge += jcost
            n_rejudged += 1
        rows[i].update({"judge_score": score, "judge_status": jst,
                        "judge_model": JUDGE_MODEL, "judge_cost_usd": round(jcost, 6),
                        "rubric_id": "sb_v02_judge_rubric_v3"})
        if n_rejudged % 20 == 0:
            print(f"  judge 进度 {n_rejudged}…", flush=True)
            OUTCOME.write_text("\n".join(json.dumps(x, ensure_ascii=False) for x in rows)
                               + "\n", encoding="utf-8")
    OUTCOME.write_text("\n".join(json.dumps(x, ensure_ascii=False) for x in rows)
                       + "\n", encoding="utf-8")

    # ---- 3) 汇总 ----
    agg = {}
    by = defaultdict(list)
    for r in rows:
        by[(r["generation_profile"], r["split"])].append(r)
    for (profile, split), rs in sorted(by.items()):
        scores = [r["judge_score"] for r in rs if r["judge_score"] is not None]
        agg[f"{profile}/{split}"] = {
            "n_cells": len(rs),
            "validity_ok_rate": round(sum(1 for r in rs if r["validity"] in ("ok", "truncated"))
                                      / len(rs), 4),
            "truncated_rate": round(sum(1 for r in rs if r["validity"] == "truncated") / len(rs), 4),
            "judge_ok_rate": round(sum(1 for r in rs if r["judge_status"] == "ok") / len(rs), 4),
            "judge_mean": round(sum(scores) / len(scores), 4) if scores else None,
            "by_tier_mean": {
                t: (lambda ss: round(sum(ss) / len(ss), 4) if ss else None)(
                    [r["judge_score"] for r in rs if r["tier"] == t
                     and r["judge_score"] is not None])
                for t in ("weak", "mid", "strong")},
        }
    report = {
        "phase": "S12.6-A resume（b 方案）",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "decision": "Nironta 拍板 b（不充值）：mid=qwen3.7-plus 代采、judge=qwen3.8-flash（池外）",
        "rubric_id": "sb_v02_judge_rubric_v3", "rubric_sha256": sha_v3,
        "note": "旧 turbo 72 个分数作废统一重评（避免跨 judge 混算）；mid 代际替换在 S12.7 申报 system_pool_v01 变更",
        "n_resampled": n_resampled, "n_rejudged": n_rejudged,
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
