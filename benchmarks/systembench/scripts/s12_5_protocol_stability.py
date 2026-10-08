#!/usr/bin/env python3
"""s12_5_protocol_stability.py — S12.5.4 Protocol Stability Probe（29 号 §九/§十）。

背景（29 号终审）：72 cells 中 90.3% truncated（P1536）且排名 weak>mid>strong 反直觉，
无法把「模型能力差异」与「输出预算/截断效应」分离 → Pool Audit 降级 CONDITIONAL GO，
先验证 protocol stability 再决定 Full Harvest。

设计（只变一个变量 output budget，其余全冻结）：
  节点：确定性 6 个（6 role 槽位各一；选择规则=槽位内「所在 case 已选最少→case_id→node_id」，
        全部来自现有 3 个 probe workflow，全部 artifact_dependent，final_text 已冻结）
  Profile A  P1536：thinking=false, max_output=1536 —— 复用 S12.5.2 已有 18 cells（零成本）
  Profile B  P4096：thinking=false, max_output=4096 —— 新采 18 cells（~$0.25）
  judge：qwen-turbo（rubric v2 sha256 8b8fc8a2…，temp=0）同协议重评 P4096 输出；
         P1536 分数直接复用（同 rubric v2 已评）。
  其余（prompt/model/system prompt/temperature/judge）两 profile 完全一致。

Gate（29 号 §十，决策规则先于结果冻结）：
  winner agreement = 6 节点中 P1536 与 P4096 winner tier 相同的比例（argmax score，平手取低成本）
  GO     agreement ≥ 0.70 且 P4096 truncation 显著下降 → P1536=Budget Track / P4096=sensitivity
  REVIEW 0.40 ≤ agreement < 0.70 → budget 是实质 routing variable，headline 双 profile 报告
  STOP   agreement < 0.40 或大量 winner inversion → 暂停 Full Harvest 重新设计 protocol

产出：matrices/semantic_outcomes_protocol_probe.jsonl（P4096 cells）
     + reports/s12_5_protocol_stability.json（gate 判定）。

用法（仓库根目录）：
  DASHSCOPE_API_KEY=… benchmarks/routerbench/.venv/bin/python \
    benchmarks/systembench/scripts/s12_5_protocol_stability.py --probe   # P4096 采集
  benchmarks/routerbench/.venv/bin/python \
    benchmarks/systembench/scripts/s12_5_protocol_stability.py --gate    # 判定（零成本）
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
SB = _HERE.parent
CASES = SB / "cases" / "system_cases_v02.jsonl"
SIDECAR = SB / "cases" / "routing_inputs_v02.jsonl"
RUBRIC = SB / "evaluator" / "judge_rubric_v2.md"
BASE_OUTCOME = SB / "matrices" / "semantic_outcomes_probe.jsonl"      # P1536（复用）
PROTO_OUTCOME = SB / "matrices" / "semantic_outcomes_protocol_probe.jsonl"  # P4096
REPORT = SB / "reports" / "s12_5_protocol_stability.json"

ENDPOINT = "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions"
JUDGE_MODEL = "qwen-turbo"
COLLECT_MODELS = [("weak", "qwen3.6-flash", 0.1655, 0.9931),
                  ("mid", "qwen3.6-plus", 0.2759, 1.6552),
                  ("strong", "qwen3.6-max-preview", 1.2414, 7.4483)]
EXEC_SYSTEM_PROMPT = "你是一个专业的AI助手。请认真完成以下任务。"
EXEC_TEMPERATURE = 0.3
EXEC_EXTRA_PARAMS = {"enable_thinking": False}
JUDGE_TEMPERATURE = 0.0
PROFILE_A = {"profile_id": "P1536", "max_output": 1536}
PROFILE_B = {"profile_id": "P4096", "max_output": 4096}
# 29 号 §十 冻结阈值（结果前定）
GO_THRESHOLD = 0.70
REVIEW_THRESHOLD = 0.40

# 确定性 6 节点（--gate 依赖同一函数复算；与 --probe 一致）
ROLE_SLOTS = ["calculation", "extraction", "reasoning", "search",
              "verification", "synthesis"]
PROBE_CASES = ["sb_v02_sem_006", "sb_v02_sem_023", "sb_v02_sem_030"]


def sha(text: str) -> str:
    import hashlib
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def load_jsonl(path: Path) -> list[dict]:
    return [json.loads(l) for l in path.read_text(encoding="utf-8").split("\n")
            if l.strip()]


def select_nodes(cases: dict) -> list[tuple[str, str]]:
    """槽位 → 案例均衡确定性选择（与干跑脚本同规则）。"""
    picked: list[tuple[str, str]] = []
    for slot in ROLE_SLOTS:
        cands = [(cid, n["node_id"]) for cid in sorted(PROBE_CASES)
                 for n in cases[cid]["nodes"] if n["semantic_role"] == slot]
        if not cands:
            continue
        picked.append(min(cands, key=lambda t: (
            sum(1 for p in picked if p[0] == t[0]), t[0], t[1])))
    return picked


def api_call(model: str, system: str, user: str, max_tokens: int,
             temperature: float, api_key: str, extra: dict | None = None,
             retries: int = 1) -> dict:
    rec: dict = {"latency_s": None, "error": None}
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
        usage = d.get("usage") or {}
        rec.update({"http_status": 200,
                    "content": ch.get("message", {}).get("content") or "",
                    "finish_reason": ch.get("finish_reason"),
                    "prompt_tokens": usage.get("prompt_tokens"),
                    "completion_tokens": usage.get("completion_tokens")})
        return rec
    return rec


def validity(rec: dict) -> str:
    if rec.get("error"):
        return "api_error"
    text = (rec.get("content") or "").strip()
    if len(text) < 10:
        return "empty_or_too_short"
    if any(p in text[:60] for p in ("抱歉", "对不起", "作为AI", "作为 AI", "无法完成")):
        return "refusal_pattern"
    return "truncated" if rec.get("finish_reason") == "length" else "ok"


def probe(api_key: str) -> None:
    cases = {c["system_case_id"]: c for c in load_jsonl(CASES)}
    side = {(r["system_case_id"], r["node_id"]): r for r in load_jsonl(SIDECAR)}
    nodes = select_nodes(cases)
    print(f"确定性 6 节点: {[f'{c}/{n}' for c, n in nodes]}")
    rubric = RUBRIC.read_text(encoding="utf-8")
    done: set[tuple[str, str]] = set()
    rows: list[dict] = []
    if PROTO_OUTCOME.exists():
        rows = load_jsonl(PROTO_OUTCOME)
        done = {(r["system_case_id"], r["node_id"], r["tier"]) for r in rows}
        if rows:
            print(f"断点续采：已有 {len(rows)} cells")
    for cid, nid in nodes:
        final_text = side[(cid, nid)]["execution_input"]["final_text"]
        if not final_text:
            raise SystemExit(f"{cid}/{nid}: final_text 未冻结")
        n = next(x for x in cases[cid]["nodes"] if x["node_id"] == nid)
        for tier, model, pin, pout in COLLECT_MODELS:
            if (cid, nid, tier) in done:
                continue
            rec = api_call(model, EXEC_SYSTEM_PROMPT, final_text,
                           PROFILE_B["max_output"], EXEC_TEMPERATURE,
                           api_key, EXEC_EXTRA_PARAMS)
            st = validity(rec)
            pt, ct = rec.get("prompt_tokens") or 0, rec.get("completion_tokens") or 0
            score, jst = None, "skipped"
            if st in ("ok", "truncated"):
                jp = f"{rubric}\n\n## 任务要求\n{final_text}\n\n## 助手输出\n{rec.get('content','')}"
                jrec = api_call(JUDGE_MODEL, "", jp, 300, JUDGE_TEMPERATURE, api_key)
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
                "system_case_id": cid, "node_id": nid,
                "semantic_role": n.get("semantic_role"),
                "dependency_semantics": n.get("dependency_semantics"),
                "tier": tier, "model": model,
                "generation_profile": PROFILE_B["profile_id"],
                "max_output_tokens": PROFILE_B["max_output"],
                "execution_input_hash": side[(cid, nid)]["execution_input"]["final_hash"],
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
            print(f"  [{cid}/{nid} × {tier}] {st} score={score} "
                  f"out={ct} lat={rec['latency_s']}s", flush=True)
            PROTO_OUTCOME.write_text(
                "\n".join(json.dumps(r, ensure_ascii=False) for r in rows) + "\n",
                encoding="utf-8")
    print(f"✓ {PROTO_OUTCOME}（{len(rows)} cells）——下一步跑 --gate")


def winner(cells: dict[tuple[str, str], dict[str, dict]],
           profile_rows: list[dict]) -> list[dict]:
    """每节点 winner tier（argmax score，平手取低成本）；分数缺失的节点剔除。"""
    by_node: dict[tuple[str, str], dict[str, dict]] = {}
    for r in profile_rows:
        by_node.setdefault((r["system_case_id"], r["node_id"]), {})[r["tier"]] = r
    out = []
    for key in sorted(cells):
        bytier = by_node.get(key, {})
        scores = {t: bytier[t].get("judge_score") for t in ("weak", "mid", "strong")
                  if t in bytier}
        if len(scores) < 3 or any(v is None for v in scores.values()):
            continue
        win = sorted(("weak", "mid", "strong"),
                     key=lambda t: (-scores[t], bytier[t]["cost_usd"]))[0]
        out.append({"case": key[0], "node": key[1], "winner": win, "scores": scores})
    return out


def gate() -> None:
    base_rows = load_jsonl(BASE_OUTCOME)
    proto_rows = load_jsonl(PROTO_OUTCOME) if PROTO_OUTCOME.exists() else []
    if not proto_rows:
        raise SystemExit("缺 P4096 cells——先跑 --probe")
    cases = {c["system_case_id"]: c for c in load_jsonl(CASES)}
    nodes = select_nodes(cases)
    want = {(c, n) for c, n in nodes}
    have_a = {(r["system_case_id"], r["node_id"]) for r in base_rows}
    have_b = {(r["system_case_id"], r["node_id"]) for r in proto_rows}
    missing = want - (have_a & have_b)
    if missing:
        raise SystemExit(f"6 节点 cells 不齐: {sorted(missing)}")
    w_a = winner(have_a, [r for r in base_rows
                          if (r["system_case_id"], r["node_id"]) in want])
    w_b = winner(have_b, [r for r in proto_rows
                          if (r["system_case_id"], r["node_id"]) in want])
    agree = sum(1 for a, b in zip(w_a, w_b) if a["winner"] == b["winner"])
    n_scored = len(w_a)
    agreement = round(agree / n_scored, 4) if n_scored else None
    # tier 均分排序（两 profile）
    def tier_means(rows: list[dict]) -> dict:
        by = {}
        for r in rows:
            if (r["system_case_id"], r["node_id"]) in want and r["judge_score"] is not None:
                by.setdefault(r["tier"], []).append(r["judge_score"])
        return {t: round(sum(v) / len(v), 4) for t, v in by.items()}
    means_a, means_b = tier_means(base_rows), tier_means(proto_rows)
    order_a = sorted(means_a, key=means_a.get, reverse=True)
    order_b = sorted(means_b, key=means_b.get, reverse=True)
    rank_agree = order_a == order_b
    # truncation
    trunc_a = sum(1 for r in base_rows
                  if (r["system_case_id"], r["node_id"]) in want
                  and r["validity"] == "truncated") / 18
    trunc_b = sum(1 for r in proto_rows
                  if (r["system_case_id"], r["node_id"]) in want
                  and r["validity"] == "truncated") / 18
    # headroom 两 profile
    def headroom(ws: list[dict]) -> float:
        hs = [max(w["scores"].values()) - min(w["scores"].values()) for w in ws]
        return round(sum(hs) / len(hs), 4) if hs else 0.0

    inversions = [{"case": a["case"], "node": a["node"],
                   "p1536": a["winner"], "p4096": b["winner"]}
                  for a, b in zip(w_a, w_b) if a["winner"] != b["winner"]]
    if agreement is None:
        verdict = "STOP"
    elif agreement >= GO_THRESHOLD and trunc_b < trunc_a:
        verdict = "GO"
    elif agreement >= REVIEW_THRESHOLD:
        verdict = "REVIEW"
    else:
        verdict = "STOP"
    report = {
        "phase": "S12.5.4 Protocol Stability Probe",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "design": "6 nodes × 3 models × 2 profiles（只变 max_output：1536→4096）",
        "frozen_thresholds": {"go": GO_THRESHOLD, "review": REVIEW_THRESHOLD,
                              "note": "29 号 §十，结果前冻结"},
        "nodes": [f"{c}/{n}" for c, n in nodes],
        "winner_agreement": agreement, "n_scored": n_scored,
        "pairwise_inversions": inversions,
        "tier_means": {"P1536": means_a, "P4096": means_b},
        "tier_order": {"P1536": order_a, "P4096": order_b},
        "tier_order_stable": rank_agree,
        "truncation_rate": {"P1536": round(trunc_a, 4),
                            "P4096": round(trunc_b, 4)},
        "headroom_mean": {"P1536": headroom(w_a), "P4096": headroom(w_b)},
        "verdict": verdict,
    }
    REPORT.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n",
                      encoding="utf-8")
    print(json.dumps({k: report[k] for k in
                      ("winner_agreement", "tier_order", "truncation_rate",
                       "headroom_mean", "verdict")},
                     ensure_ascii=False, indent=2))
    print(f"✓ {REPORT}")
    if verdict == "STOP":
        return 2
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument("--probe", action="store_true")
    g.add_argument("--gate", action="store_true")
    args = ap.parse_args()
    if args.gate:
        return gate()
    api_key = os.environ.get("DASHSCOPE_API_KEY", "")
    if not api_key:
        print("STOP: DASHSCOPE_API_KEY 未设置", file=sys.stderr)
        return 2
    probe(api_key)
    return 0


if __name__ == "__main__":
    sys.exit(main())
