#!/usr/bin/env python3
"""s12_5_provider_probe.py — S12.5.0 Provider Probe（27 号 §九）。

3 candidate models（native_qwen36_3tier_v1，价格已 P2 重录）× 2 representative nodes
+ 1 次 judge JSON validity 调用。只验证 API 机制；**数据不入正式矩阵**（non_matrix=true，
S12.5.2 以冻结 execution_input 统一采集）。

检查项（27 号 §九，任何关键项失败 → exit 2 = STOP）：
  provider reachability   HTTP 200
  model identity          response.model echo == 请求 provider_model_id
  schema stability        choices[0].message.content:str、finish_reason、usage 三 token 字段齐全
  JSON validity           judge 协议调用输出可 json.loads 且 score∈[0,1]
  latency                 wall clock 记录（报告项，不设阈值）
  cost recording          cost = usage × 官方单价 > 0；usage 缺失 → cost=0 + cost_status=unknown
                          （unknown 按红线不进 Oracle/Tax/frontier，此处只验证路径存在）

代表节点（确定性）：sb_v02_sem_006 第一个 independent + 第一个 artifact_dependent 节点；
dependent 节点此时用 base prompt（artifacts 未冻结，S12.5.1 之后才注入）。
调用参数与生产子任务执行一致（ai_client.rs:516-533）：system prompt 原文、temp=0.3、
max_tokens=节点 estimated_output_tokens。

安全：key 从环境变量读取；endpoint 模块常量；无动态 URL 拼接。
成本：6 次 probe + 1 次 judge ≈ $0.006（max-preview 2 次为大头）。
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

import requests

_HERE = Path(__file__).resolve().parent
CASES = _HERE.parent / "cases" / "system_cases_v02.jsonl"
REPORT = _HERE.parent / "reports" / "s12_5_provider_probe.json"

ENDPOINT = "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions"
PROBE_CASE = "sb_v02_sem_006"
# 池与单价（USD/M tokens）——source of truth：benchmarks/routerbench/manifests/model_pool_native.yaml v2
# （2026-10-07 官方页重录，华北2，fx 7.25；脚本内副本仅为 probe 成本核算，不参与 freeze）
POOL = [
    ("weak", "qwen3.6-flash", 0.1655, 0.9931),
    ("mid", "qwen3.6-plus", 0.2759, 1.6552),
    ("strong", "qwen3.6-max-preview", 1.2414, 7.4483),
]
JUDGE_MODEL = "qwen-plus"  # 不在采集池内，避免自评偏差（26 号 §judge 协议）
# 生产子任务执行 system prompt 原文（ai_client.rs 子任务分支）
EXEC_SYSTEM_PROMPT = "你是一个专业的AI助手。请认真完成以下任务。"
EXEC_TEMPERATURE = 0.3

# judge JSON validity 校验用的 mini rubric（与 S12.5.2 正式 rubric 同构：严格 JSON 输出）
JUDGE_MINI_PROMPT = (
    "你是任务完成质量评审。根据任务要求对助手输出打分（0-1 浮点，1=完全满足）。\n"
    "只输出JSON对象：{\"score\": <0-1浮点>, \"reason\": \"<一句话依据>\"}，不要输出其他内容。\n\n"
    "任务要求：列出 1 个颜色。\n助手输出：红色。"
)


def pick_nodes() -> tuple[dict, dict]:
    """sb_v02_sem_006 第一个 independent + 第一个 artifact_dependent 节点。"""
    indep = dep = None
    for line in CASES.read_text(encoding="utf-8").split("\n"):
        if not line.strip():
            continue
        c = json.loads(line)
        if c.get("system_case_id") != PROBE_CASE:
            continue
        for n in c["nodes"]:
            if n.get("dependency_semantics") == "independent" and indep is None:
                indep = n
            if (n.get("dependency_semantics") == "artifact_dependent"
                    and dep is None):
                dep = n
        break
    if indep is None or dep is None:
        raise SystemExit(f"{PROBE_CASE} 缺 independent/artifact_dependent 代表节点")
    return indep, dep


def call_model(model: str, user_text: str, max_tokens: int,
               api_key: str, temperature: float = EXEC_TEMPERATURE,
               system_prompt: str = EXEC_SYSTEM_PROMPT, retries: int = 1) -> dict:
    """一次 compatible chat 调用 → 检查记录 dict（不抛异常，失败记入 checks）。
    瞬时故障（网络/5xx/限流）重试 1 次；probe 验证机制，不因偶发抖动误 STOP。"""
    rec: dict = {"model": model, "latency_s": None, "checks": {}, "retries_used": 0}
    for attempt in range(retries + 1):
        rec["retries_used"] = attempt
        t0 = time.time()
        try:
            resp = requests.post(
                ENDPOINT,
                headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"},
                json={
                    "model": model,
                    "temperature": temperature,
                    "max_tokens": max_tokens,
                    "messages": [
                        {"role": "system", "content": system_prompt},
                        {"role": "user", "content": user_text},
                    ],
                },
                timeout=180,
            )
        except Exception as e:  # noqa: BLE001 — probe 要把任何失败变成记录而非中断
            rec["checks"]["reachability"] = {"pass": False, "error": repr(e)}
            if attempt < retries:
                time.sleep(5)
                continue
            return rec
        rec["latency_s"] = round(time.time() - t0, 2)
        rec["http_status"] = resp.status_code
        if resp.status_code != 200:
            rec["checks"]["reachability"] = {
                "pass": False, "error": f"HTTP {resp.status_code}: {resp.text[:200]}"}
            if resp.status_code >= 500 or resp.status_code == 429:
                if attempt < retries:
                    time.sleep(8)
                    continue
            return rec
        rec["checks"]["reachability"] = {"pass": True}
        break
    d = resp.json()
    rec["response_model"] = d.get("model")
    rec["checks"]["model_identity"] = {
        "pass": d.get("model") == model,
        "echo": d.get("model"), "requested": model}
    choices = d.get("choices") or []
    msg = choices[0].get("message", {}) if choices else {}
    content = msg.get("content")
    usage = d.get("usage") or {}
    rec["has_reasoning_content"] = bool(msg.get("reasoning_content"))
    rec["reasoning_chars"] = len(msg.get("reasoning_content") or "")
    schema_ok = (
        isinstance(content, str) and bool(content)
        and choices[0].get("finish_reason") is not None
        and all(usage.get(k) is not None for k in
                ("prompt_tokens", "completion_tokens", "total_tokens"))
    )
    rec["checks"]["schema_stability"] = {
        "pass": schema_ok,
        "finish_reason": choices[0].get("finish_reason") if choices else None,
        "content_type": type(content).__name__,
        "usage": usage}
    rec["output_excerpt"] = (content or "")[:120]
    rec["n_output_chars"] = len(content or "")
    # cost recording：usage 齐 → tokens×单价>0；usage 缺 → cost=0 + unknown（红线路径）
    pin, pout = PRICE_BY_MODEL[model]
    if usage.get("prompt_tokens") and usage.get("completion_tokens"):
        cost = (usage["prompt_tokens"] * pin + usage["completion_tokens"] * pout) / 1e6
        rec["cost_usd"] = round(cost, 6)
        rec["cost_status"] = "known"
        rec["checks"]["cost_recording"] = {"pass": cost > 0}
    else:
        rec["cost_usd"] = 0.0
        rec["cost_status"] = "unknown"
        rec["checks"]["cost_recording"] = {
            "pass": True,  # 路径存在即过；unknown 数据按红线不进 Oracle/Tax/frontier
            "note": "usage 缺失 → cost=0/unknown（不进 headline 统计）"}
    return rec


PRICE_BY_MODEL = {m: (pin, pout) for _, m, pin, pout in POOL}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--probe-case", default=PROBE_CASE)
    args = ap.parse_args()
    api_key = os.environ.get("DASHSCOPE_API_KEY", "")
    if not api_key:
        print("STOP: DASHSCOPE_API_KEY 未设置", file=sys.stderr)
        return 2

    indep, dep = pick_nodes()
    nodes = [
        {"role": "independent", "case": args.probe_case, "node": indep},
        {"role": "artifact_dependent_base_prompt", "case": args.probe_case, "node": dep},
    ]
    calls: list[dict] = []
    for m_role, m_id, _, _ in POOL:
        for nd in nodes:
            rec = call_model(m_id, nd["node"]["prompt"],
                             int(nd["node"].get("estimated_output_tokens") or 512),
                             api_key)
            rec.update({"tier": m_role, "node_id": nd["node"]["node_id"],
                        "node_sem": nd["role"], "non_matrix": True})
            calls.append(rec)
            ck = rec["checks"]
            print(f"[{m_id} × {nd['node']['node_id']}/{nd['role']}] "
                  f"reach={ck.get('reachability', {}).get('pass')} "
                  f"identity={ck.get('model_identity', {}).get('pass')} "
                  f"schema={ck.get('schema_stability', {}).get('pass')} "
                  f"cost={ck.get('cost_recording', {}).get('pass')} "
                  f"latency={rec['latency_s']}s out_chars={rec.get('n_output_chars')} "
                  f"reasoning={rec.get('has_reasoning_content')} "
                  f"err={(ck.get('reachability') or {}).get('error', '')[:80]}")
    # judge JSON validity（qwen-plus，temp=0 正式协议同款；rubric 走 user 消息）
    jraw = None
    jcontent = ""
    resp = requests.post(
        ENDPOINT,
        headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"},
        json={"model": JUDGE_MODEL, "temperature": 0.0, "max_tokens": 200,
              "messages": [{"role": "user", "content": JUDGE_MINI_PROMPT}]},
        timeout=60)
    ok_json = False
    score_val = None
    if resp.status_code == 200:
        jraw = resp.json()
        jcontent = jraw["choices"][0]["message"]["content"] or ""
        try:
            parsed = json.loads(jcontent.strip().removeprefix("```json").removesuffix("```").strip())
            score_val = parsed.get("score")
            ok_json = isinstance(score_val, (int, float)) and 0.0 <= score_val <= 1.0
        except Exception:  # noqa: BLE001
            ok_json = False
    judge_check = {"model": JUDGE_MODEL, "json_valid": ok_json, "score": score_val,
                   "raw_excerpt": jcontent[:120], "pass": ok_json}
    print(f"[judge {JUDGE_MODEL}] json_valid={ok_json} score={score_val}")

    # 汇总判定：三模型 reachability/identity/schema/cost + judge JSON 全过 → PASS
    critical_fail = []
    for rec in calls:
        for k in ("reachability", "model_identity", "schema_stability", "cost_recording"):
            if not rec["checks"][k]["pass"]:
                critical_fail.append(f"{rec['model']}×{rec['node_id']}: {k}")
    if not judge_check["pass"]:
        critical_fail.append(f"judge {JUDGE_MODEL}: json_validity")
    total_cost = round(sum(r.get("cost_usd", 0.0) for r in calls) +
                       (jraw["usage"]["prompt_tokens"] * 0 + 0 if jraw else 0), 6)

    report = {
        "phase": "S12.5.0 Provider Probe",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "pool_id": "native_qwen36_3tier_v1 (v2, official_verified_2026-10-07)",
        "probe_case": args.probe_case,
        "non_matrix": True,
        "calls": calls,
        "judge_check": judge_check,
        "critical_failures": critical_fail,
        "verdict": "PASS" if not critical_fail else "STOP",
        "probe_cost_usd_est": total_cost,
    }
    REPORT.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n",
                      encoding="utf-8")
    print(f"\nverdict: {report['verdict']} | failures: {critical_fail or '无'}")
    print(f"✓ {REPORT}")
    return 0 if not critical_fail else 2


if __name__ == "__main__":
    sys.exit(main())
