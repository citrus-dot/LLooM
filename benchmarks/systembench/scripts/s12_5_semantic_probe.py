#!/usr/bin/env python3
"""s12_5_semantic_probe.py — S12.5.2 5-workflow Semantic Probe（27 号 §十）。

两个子命令（--stage0 / --collect），共享确定性 probe 集选择。

probe 集选择（确定性，可复算）：
  候选 = calibration split × semantic case × 4 ≤ 节点数 ≤ 12（预算控制）
  贪心 5 轮：每轮选「新增 semantic_role 数最多」者，平手取 case_id 升序。
  不硬编码 case 列表；band 覆盖由 Pool Audit 复核。

--stage0（参考执行，qwen3.6-plus）：
  5 workflow 按拓扑序全节点执行（independent 用 base_text；dependent 注入上游
  参考输出），冻结 reference artifacts + 回填 routing_inputs_v02.jsonl 的
  final_text/final_hash（probe 5 case）。执行协议 = contract v1：
  system prompt 生产原文 / temp 0.3 / max_tokens=estimated / enable_thinking=false。

--collect（3 模型采集 + judge）：
  probe 节点 × 3 池内模型（同协议；dependent 一律用冻结 final_text——L2 conditional
  cells，conditioned on 同一 artifact 状态，不级联被评模型自身输出）。
  deterministic validity 先行（27 号 §14 优先级：非空/拒绝模式），validity fail
  不调 judge（score=0）；其余 judge（qwen-plus temp=0，rubric v1）打分。
  产出：matrices/semantic_outcomes_probe.jsonl + reports/s12_5_semantic_probe.json。

成本（thinking-off 实测口径）：stage0 ≈ $0.05 + 采集 ≈ $0.27 + judge ≈ $0.06 ≈ $0.38。
"""

from __future__ import annotations

import argparse
import hashlib
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
ARTIFACTS = SB / "matrices" / "probe_reference_artifacts_v02.json"
OUTCOME = SB / "matrices" / "semantic_outcomes_probe.jsonl"
REPORT = SB / "reports" / "s12_5_semantic_probe.json"

ENDPOINT = "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions"
EXEC_MODEL = "qwen3.6-plus"          # stage 0 参考执行（contract v1）
JUDGE_MODEL = "qwen-turbo"           # rubric v2：qwen-plus 额度耗尽换档（2026-10-08）
COLLECT_MODELS = [("weak", "qwen3.6-flash", 0.1655, 0.9931),
                  ("mid", "qwen3.6-plus", 0.2759, 1.6552),
                  ("strong", "qwen3.6-max-preview", 1.2414, 7.4483)]
# 执行协议（contract v1）：system prompt 生产原文 + temp 0.3 + enable_thinking=false
EXEC_SYSTEM_PROMPT = "你是一个专业的AI助手。请认真完成以下任务。"
EXEC_TEMPERATURE = 0.3
EXEC_EXTRA_PARAMS = {"enable_thinking": False}   # ★ contract v1 显式偏差
# max_tokens 协议：max(estimated, 1536)。首跑 stage0 实测 35/35 finish=length——
# planner 估的 estimated_output_tokens 系统性低于真实输出需求，content 全截断，
# 质量信号被截断伪影污染；1536（≈2500 汉字）为协议常量。与生产差异并入
# production finding（生产 estimated 机制过于激进 + enable_thinking 默认开）。
MAX_TOKENS_FLOOR = 1536
JUDGE_TEMPERATURE = 0.0
N_PROBE_CASES = 3   # 额度缩减：qwen-plus 耗尽后 judge 走 qwen-turbo，probe 5→3（2026-10-08）
MIN_NODES, MAX_NODES = 4, 12


def sha(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def load_cases() -> list[dict]:
    return [json.loads(l) for l in CASES.read_text(encoding="utf-8").split("\n")
            if l.strip()]


def api_call(model: str, system: str, user: str, max_tokens: int,
             temperature: float, api_key: str, extra: dict | None = None,
             retries: int = 1) -> dict:
    """compatible chat 调用 → 记录 dict（不抛异常）。"""
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
                json=body, timeout=180)
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
        rec.update({
            "http_status": 200,
            "content": ch.get("message", {}).get("content") or "",
            "finish_reason": ch.get("finish_reason"),
            "model_echo": d.get("model"),
            "prompt_tokens": usage.get("prompt_tokens"),
            "completion_tokens": usage.get("completion_tokens"),
        })
        return rec
    return rec


def select_probe_cases(cases: list[dict]) -> list[str]:
    """确定性贪心：候选 = calibration×semantic×4-12 节点；每轮选新增 role 最多，
    平手 case_id 升序。"""
    cand = []
    for c in cases:
        if c.get("split") != "calibration" or c.get("case_role") != "headline_candidate":
            continue
        roles = {n.get("semantic_role") for n in c["nodes"]}
        if MIN_NODES <= len(c["nodes"]) <= MAX_NODES:
            cand.append((c["system_case_id"], roles, len(c["nodes"])))
    picked: list[str] = []
    covered: set[str] = set()
    for _ in range(N_PROBE_CASES):
        best = None
        for cid, roles, nn in cand:
            if cid in picked:
                continue
            key = (-len(roles - covered), cid)
            if best is None or key < best[0]:
                best = (key, cid, roles)
        if best is None:
            break
        picked.append(best[1])
        covered |= best[2]
    return picked


def topo_order(c: dict) -> list[str]:
    """Kahn 拓扑序（consumes.from 为依赖边，全库已修复无 dangling）。"""
    deps: dict[str, set[str]] = {
        n["node_id"]: set(consumed_refs(n)) for n in c["nodes"]}
    order, done = [], set()
    while len(order) < len(deps):
        ready = sorted(k for k, v in deps.items() if k not in done and v <= done)
        if not ready:
            raise SystemExit(f"{c['system_case_id']}: 拓扑排序失败（环？）")
        order.extend(ready)
        done |= set(ready)
    return order


def consumed_refs(n: dict) -> list[str]:
    refs = []
    for item in (n.get("artifact_contract") or {}).get("consumes") or []:
        if isinstance(item, dict):
            refs.extend(item.get("from") or [])
        elif isinstance(item, list):
            refs.extend(item)
        else:
            refs.append(item)
    return refs


def build_final_text(c: dict, n: dict, artifacts: dict[str, str] | None) -> str:
    """execution_input.final_text：independent = base_text；dependent = 注入冻结 artifacts。"""
    desc = n["description"]
    if not artifacts:
        return f"{c['root_goal']}\n\n---\n## 子任务\n{desc}"
    refs = consumed_refs(n)
    block = "\n".join(f"[子任务{rid}] {artifacts.get(rid, '')}" for rid in refs)
    return (f"{c['root_goal']}\n\n---\n## 前置任务结果\n{block}\n\n"
            f"## 子任务\n{desc}")


def validity_check(rec: dict) -> tuple[bool, str]:
    """确定性 validity（27 号 §14：deterministic evaluator 优先）。
    返回 (validity_pass, status)。"""
    if rec.get("error"):
        return False, "api_error"
    text = (rec.get("content") or "").strip()
    if len(text) < 10:
        return False, "empty_or_too_short"
    if any(p in text[:60] for p in ("抱歉", "对不起", "作为AI", "作为 AI", "无法完成")):
        return False, "refusal_pattern"
    return True, ("truncated" if rec.get("finish_reason") == "length" else "ok")


def stage0(api_key: str) -> None:
    cases = load_cases()
    picked = select_probe_cases(cases)
    print(f"probe 集（确定性选择）: {picked}")
    by_id = {c["system_case_id"]: c for c in cases}
    artifacts: dict[str, dict[str, str]] = {}
    total_cost = 0.0
    for cid in picked:
        c = by_id[cid]
        art: dict[str, str] = {}
        for nid in topo_order(c):
            n = next(x for x in c["nodes"] if x["node_id"] == nid)
            user = build_final_text(c, n, art if consumed_refs(n) else None)
            mt = max(int(n.get("estimated_output_tokens") or 512), MAX_TOKENS_FLOOR)
            rec = api_call(EXEC_MODEL, EXEC_SYSTEM_PROMPT, user,
                           mt, EXEC_TEMPERATURE, api_key, EXEC_EXTRA_PARAMS)
            ok, status = validity_check(rec)
            art[nid] = rec.get("content", "") if ok else ""
            pt, ct = rec.get("prompt_tokens") or 0, rec.get("completion_tokens") or 0
            cost = (pt * 0.2759 + ct * 1.6552) / 1e6
            total_cost += cost
            print(f"  [{cid}/{nid}] {status} lat={rec['latency_s']}s "
                  f"in={pt} out={ct} ${cost:.4f}")
            if not ok:
                print(f"  !! 参考执行失败 {cid}/{nid}: {rec.get('error') or status}")
        artifacts[cid] = art
    ARTIFACTS.parent.mkdir(parents=True, exist_ok=True)
    ARTIFACTS.write_text(json.dumps({
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "reference_model": EXEC_MODEL,
        "protocol": {"system_prompt": EXEC_SYSTEM_PROMPT,
                     "temperature": EXEC_TEMPERATURE,
                     "enable_thinking": False,
                     "max_tokens": "max(node.estimated_output_tokens, 1536)"},
        "probe_cases": picked,
        "artifacts": artifacts,
    }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    # 回填 sidecar final_text/final_hash（probe 5 case 的全部节点：independent 的
    # final == base_text 也冻结，便于 cell 级 hash 统一）
    rows = [json.loads(l) for l in SIDECAR.read_text(encoding="utf-8").split("\n")
            if l.strip()]
    nfrozen = 0
    for r in rows:
        cid = r["system_case_id"]
        if cid not in artifacts:
            continue
        c = by_id[cid]
        n = next(x for x in c["nodes"] if x["node_id"] == r["node_id"])
        final = build_final_text(c, n, artifacts[cid] if consumed_refs(n) else None)
        r["execution_input"]["final_text"] = final
        r["execution_input"]["final_hash"] = sha(final)
        r["execution_input"]["final_frozen"] = True
        nfrozen += 1
    SIDECAR.write_text("\n".join(json.dumps(r, ensure_ascii=False) for r in rows)
                       + "\n", encoding="utf-8")
    print(f"✓ 冻结 artifacts {sum(len(a) for a in artifacts.values())} 个 → {ARTIFACTS}")
    print(f"✓ sidecar 回填 final_* {nfrozen} 行 → {SIDECAR}")
    print(f"stage0 成本 ≈ ${total_cost:.4f}")


def collect(api_key: str) -> None:
    if not ARTIFACTS.exists():
        raise SystemExit("先跑 --stage0 冻结参考 artifacts")
    ref = json.loads(ARTIFACTS.read_text(encoding="utf-8"))
    # 额度缩减：只采确定性贪心序前 N_PROBE_CASES 个（023/030/006）
    picked = ref["probe_cases"][:N_PROBE_CASES]
    cases = {c["system_case_id"]: c for c in load_cases()}
    side = {(r["system_case_id"], r["node_id"]): r
            for r in (json.loads(l) for l in
                      SIDECAR.read_text(encoding="utf-8").split("\n") if l.strip())}
    rubric_text = RUBRIC.read_text(encoding="utf-8")
    rubric_hash = sha(rubric_text)

    # 断点续采：已有 (case,node,model) cell 跳过（judge 也一并跳过——同一 cell 不重评）
    done_cells: set[tuple[str, str, str]] = set()
    prev_rows: list[dict] = []
    if OUTCOME.exists():
        for l in OUTCOME.read_text(encoding="utf-8").split("\n"):
            if l.strip():
                r = json.loads(l)
                prev_rows.append(r)
                done_cells.add((r["system_case_id"], r["node_id"], r["tier"]))
        if prev_rows:
            print(f"断点续采：已有 {len(prev_rows)} cells，跳过重采")

    rows: list[dict] = prev_rows
    cost_collect = cost_judge = 0.0
    n_judged = 0
    for cid in picked:
        c = cases[cid]
        for n in c["nodes"]:
            eid = (cid, n["node_id"])
            final_text = side[eid]["execution_input"].get("final_text")
            if not final_text:
                raise SystemExit(f"{eid}: final_text 未冻结（先 --stage0）")
            for tier, model, pin, pout in COLLECT_MODELS:
                if (cid, n["node_id"], tier) in done_cells:
                    continue
                mt = max(int(n.get("estimated_output_tokens") or 512), MAX_TOKENS_FLOOR)
                rec = api_call(model, EXEC_SYSTEM_PROMPT, final_text,
                               mt, EXEC_TEMPERATURE, api_key, EXEC_EXTRA_PARAMS)
                ok, status = validity_check(rec)
                pt, ct = rec.get("prompt_tokens") or 0, rec.get("completion_tokens") or 0
                cost_collect += (pt * pin + ct * pout) / 1e6
                score, judge_status, judge_cost = None, "skipped", 0.0
                if ok:
                    judge_prompt = (f"{rubric_text}\n\n## 任务要求\n{final_text}\n\n"
                                    f"## 助手输出\n{rec.get('content', '')}")
                    jrec = api_call(JUDGE_MODEL, "", judge_prompt, 300,
                                    JUDGE_TEMPERATURE, api_key)
                    jpt, jct = jrec.get("prompt_tokens") or 0, jrec.get("completion_tokens") or 0
                    # qwen-turbo 官方价 ¥0.3/¥0.6 每百万 tokens（fx 7.25 → $0.0414/$0.0828）
                    judge_cost = (jpt * 0.0414 + jct * 0.0828) / 1e6
                    cost_judge += judge_cost
                    n_judged += 1
                    if jrec.get("content"):
                        try:
                            parsed = json.loads(jrec["content"].strip()
                                                .removeprefix("```json")
                                                .removesuffix("```").strip())
                            s = parsed.get("score")
                            if isinstance(s, (int, float)) and 0.0 <= s <= 1.0:
                                score, judge_status = round(float(s), 2), "ok"
                            else:
                                judge_status = "invalid_score"
                        except Exception:  # noqa: BLE001
                            judge_status = "invalid_json"
                    else:
                        judge_status = "judge_api_error"
                rows.append({
                    "system_case_id": cid, "node_id": n["node_id"],
                    "semantic_role": n.get("semantic_role"),
                    "dependency_semantics": n.get("dependency_semantics"),
                    "tier": tier, "model": model,
                    "execution_input_hash": side[eid]["execution_input"]["final_hash"],
                    "output": (rec.get("content") or "")[:4000],
                    "finish_reason": rec.get("finish_reason"),
                    "validity": status,
                    "prompt_tokens": pt, "completion_tokens": ct,
                    "cost_usd": round((pt * pin + ct * pout) / 1e6, 6),
                    "latency_s": rec.get("latency_s"),
                    "judge_score": score, "judge_status": judge_status,
                    "judge_cost_usd": round(judge_cost, 6),
                    "collected_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
                })
                print(f"  [{cid}/{n['node_id']} × {model}] {status} "
                      f"score={score} lat={rec['latency_s']}s ${rows[-1]['cost_usd']:.4f}",
                      flush=True)
                # 每 cell 落盘（中断最多丢当前 cell）
                OUTCOME.write_text(
                    "\n".join(json.dumps(r, ensure_ascii=False) for r in rows) + "\n",
                    encoding="utf-8")
    summary = {
        "phase": "S12.5.2 Semantic Probe",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "probe_cases": picked,
        "n_cells": len(rows),
        "n_judged_this_run": n_judged,
        "n_judged_total": sum(1 for r in rows if r["judge_status"] == "ok"),
        "rubric_id": "sb_v02_judge_rubric_v2", "rubric_sha256": rubric_hash,
        "judge_model": JUDGE_MODEL,
        "cost_this_run_usd": round(cost_collect + cost_judge, 4),
        "cost_collect_this_run_usd": round(cost_collect, 4),
        "cost_judge_this_run_usd": round(cost_judge, 4),
        "validity_fail": sum(1 for r in rows if r["validity"] not in ("ok", "truncated")),
        "truncated": sum(1 for r in rows if r["validity"] == "truncated"),
        "resume_note": "断点续采：cost_*_this_run 仅本 run 新增 cells；judge=qwen-turbo",
    }
    REPORT.write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n",
                      encoding="utf-8")
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    print(f"✓ {OUTCOME}")


def main() -> int:
    ap = argparse.ArgumentParser()
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument("--stage0", action="store_true")
    g.add_argument("--collect", action="store_true")
    args = ap.parse_args()
    api_key = os.environ.get("DASHSCOPE_API_KEY", "")
    if not api_key:
        print("STOP: DASHSCOPE_API_KEY 未设置", file=sys.stderr)
        return 2
    if args.stage0:
        stage0(api_key)
    else:
        collect(api_key)
    return 0


if __name__ == "__main__":
    sys.exit(main())
