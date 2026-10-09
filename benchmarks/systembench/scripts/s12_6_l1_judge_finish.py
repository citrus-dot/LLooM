#!/usr/bin/env python3
"""s12_6_l1_judge_finish.py — L1 judge 收尾（qwen3.7-max，rubric v3，2026-10-09）。

背景：qwen3.8-flash 重评 113/198 后额度耗尽（403）；qwen3.7-max 试评 5/5 成功。
本脚本只补 judge_api_error 的 84 cells（**不动已 ok 的 113**——同 rubric v3 口径，
但 judge 模型换档：flash→max。记入 report 的 honesty note：84 cells 的 judge 与
113 cells 的 judge 不同模型，都在 rubric v3 同一 frozen rubric + temp=0 下运行）。

如果 max 也中途耗尽：脚本身断点续采（只评 judge_api_error），额度恢复后重跑即可。

用法：DASHSCOPE_API_KEY=… benchmarks/routerbench/.venv/bin/python \
  benchmarks/systembench/scripts/s12_6_l1_judge_finish.py
"""

from __future__ import annotations

import json
import os
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

import requests

_HERE = Path(__file__).resolve().parent
SB = _HERE.parent
SIDECAR = SB / "cases" / "routing_inputs_v02.jsonl"
OUTCOME = SB / "matrices" / "semantic_outcomes_l1.jsonl"
REPORT = SB / "reports" / "s12_6_l1_judge_finish.json"
RUBRIC = SB / "evaluator" / "judge_rubric_v3.md"
ENDPOINT = "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions"
JUDGE_MODEL = "qwen3.7-max"
JUDGE_PIN, JUDGE_POUT = 0.2759, 1.1034   # qwen3.7-max 同 plus 价（¥2/¥8）；freeze 前重录


def main() -> int:
    api_key = os.environ.get("DASHSCOPE_API_KEY", "")
    if not api_key:
        print("STOP: DASHSCOPE_API_KEY 未设置", file=sys.stderr)
        return 2
    rubric = RUBRIC.read_text(encoding="utf-8")
    rows = [json.loads(l) for l in OUTCOME.read_text(encoding="utf-8").split("\n")
            if l.strip()]
    side = {(r["system_case_id"], r["node_id"]): r for r in
            (json.loads(l) for l in SIDECAR.read_text(encoding="utf-8").split("\n")
             if l.strip())}
    todo = [i for i, r in enumerate(rows) if r["judge_status"] == "judge_api_error"]
    print(f"待补评: {len(todo)}")
    cost = 0.0
    done = failed = 0
    for i in todo:
        r = rows[i]
        text = side[(r["system_case_id"], r["node_id"])]["execution_input"]["base_text"]
        jp = f"{rubric}\n\n## 任务要求\n{text}\n\n## 助手输出\n{r['output']}"
        t0 = time.time()
        try:
            resp = requests.post(
                ENDPOINT,
                headers={"Authorization": f"Bearer {api_key}",
                         "Content-Type": "application/json"},
                json={"model": JUDGE_MODEL, "temperature": 0.0, "max_tokens": 300,
                      "messages": [{"role": "user", "content": jp}]}, timeout=240)
        except Exception as e:  # noqa: BLE001
            print(f"  EXC {r['node_id']}: {e!r}", flush=True)
            failed += 1
            continue
        lat = round(time.time() - t0, 1)
        if resp.status_code != 200:
            print(f"  HTTP {resp.status_code} @ {r['system_case_id']}/{r['node_id']} "
                  f"× {r['tier']} × {r['generation_profile']}——疑似额度墙，停止（已评 {done}）",
                  flush=True)
            failed += 1
            break
        d = resp.json()
        ch = (d.get("choices") or [{}])[0]
        content = (ch.get("message") or {}).get("content") or ""
        u = d.get("usage") or {}
        jpt, jct = u.get("prompt_tokens") or 0, u.get("completion_tokens") or 0
        cost += (jpt * JUDGE_PIN + jct * JUDGE_POUT) / 1e6
        score, status = None, "invalid_json"
        try:
            parsed = json.loads(content.strip().removeprefix("```json")
                                .removesuffix("```").strip())
            s = parsed.get("score")
            if isinstance(s, (int, float)) and 0.0 <= s <= 1.0:
                score, status = round(float(s), 2), "ok"
        except Exception:  # noqa: BLE001
            status = "invalid_json"
        rows[i].update({
            "judge_score": score, "judge_status": status,
            "judge_model": JUDGE_MODEL, "judge_cost_usd": 0.0,  # max 免费 tier，cost 记 unknown 规则
            "judge_finished_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        })
        done += 1
        print(f"  [{r['system_case_id']}/{r['node_id']} × {r['tier']} × "
              f"{r['generation_profile']}] {status} score={score} lat={lat}s", flush=True)
        OUTCOME.write_text("\n".join(json.dumps(x, ensure_ascii=False) for x in rows)
                           + "\n", encoding="utf-8")
    n_ok = sum(1 for r in rows if r["judge_status"] == "ok")
    report = {
        "phase": "S12.6-A judge finish",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "judge_model": JUDGE_MODEL,
        "honesty_note": ("rubric v3 不变；113 cells 由 qwen3.8-flash 评、%d cells 由 %s 评"
                         "（flash 中途额度耗尽换档；两模型均 temp=0 同 rubric）" % (done, JUDGE_MODEL)),
        "n_done_this_run": done, "n_failed_remaining": sum(
            1 for r in rows if r["judge_status"] == "judge_api_error"),
        "judge_ok_total": n_ok,
        "cost_note": "qwen3.7-max 免费 tier 内；未产生计费（cost=0 记 unknown 规则，不计入 model_cost）",
    }
    REPORT.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n",
                      encoding="utf-8")
    print(f"完成 {done}，judge_ok 总数 {n_ok}/198，失败余 {report['n_failed_remaining']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
