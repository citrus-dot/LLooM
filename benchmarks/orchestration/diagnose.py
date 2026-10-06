"""diagnose.py — Failure injection（F1-F7）+ recovery 状态机（O-Day 13，方案 §二十一/§十一/§十三）。

零成本红线：无 LLM、无网络——失败注入与恢复全部确定性模拟。
确定性红线（方案 §二十）：固定 seed / clock（duration 因子）/ node ordering / failure
schedule——同输入必须得到相同 trace；自动注入用哈希桶 sha256("lloom-obfail:{seed}:{wf}:{node}")，
禁 random 模块。

失败族（方案 §二十一 F1-F7）与恢复动作映射（v0.1 固定策略，可参数开关）：
  F1 timeout               → retry（≤max_retries）
  F2 model unavailable     → fallback（pool 降一档，≤1 次）→ retry
  F3 malformed output      → retry
  F4 missing dependency    → 不消耗自身配额：上游 final 成功后自动重跑一次；上游终败则传播 FAILED
  F5 parallel branch fail  → 波内其他节点不受影响；本节点按 retry
  F6 partial aggregation   → 聚合节点输入缺失时标 PARTIAL（不 retry）
  F7 budget exhausted      → 剩余节点 ABORTED（不恢复）

状态机（节点级，方案 §十八 O5 收束形态）：
  READY → RUNNING → SUCCEEDED
                  → FAILED → RETRYING → (SUCCEEDED_RECOVERED | FAILED_FINAL)
                           → FALLBACK → (SUCCEEDED_RECOVERED | FAILED_FINAL)
  （F4 传播：FAILED_DEPENDENCY → (RETRYING after upstream | FAILED_FINAL)）
  （F7: ABORTED——运行前直接拦截）
工作流级终态：task_success / partial / failed（方案 §十一）。

耗时口径（确定性 clock）：成功=duration；retry 成功=2×duration；fallback 成功=2×duration。
makespan 用 schedule.simulate_waves 按各节点最终耗时重放（波次 barrier 语义）。

用法：
  python3 diagnose.py --self-check
  import 侧： run_recovery(workflow, failures, policy) / inject_by_hash(workflow, rate)
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from schedule import DEFAULT_DURATION_MS, reference_waves  # noqa: E402
from validate import validate_workflow  # noqa: E402

FAILURE_KINDS = (
    "F1_timeout",
    "F2_model_unavailable",
    "F3_malformed_output",
    "F4_missing_dependency",
    "F5_parallel_branch_failure",
    "F6_partial_aggregation",
    "F7_budget_exhausted",
)
RETRYABLE = {"F1_timeout", "F3_malformed_output", "F5_parallel_branch_failure"}

DEFAULT_POLICY = {
    "max_retries": 1,          # F1/F3/F5 重试上限
    "fallback_model": True,    # F2 允许降档 fallback 一次
    "replan_on_missing_dep": True,  # F4 上游恢复后自动重跑
    "budget_guard": True,      # F7 后 ABORT 剩余节点
}

# 节点终态
OK_STATES = ("SUCCEEDED", "SUCCEEDED_RECOVERED")


def _durations(workflow: dict) -> dict[str, int]:
    out = {}
    for n in workflow.get("nodes") or []:
        d = n.get("duration_ms")
        out[n["id"]] = int(d) if isinstance(d, int) and d >= 0 else DEFAULT_DURATION_MS
    return out


def inject_by_hash(workflow: dict, rate: float, seed: int = 3407) -> dict[str, str]:
    """哈希桶自动注入：每节点独立判定（确定性、可重复、与行序无关）。

    sha256(f"lloom-obfail:{seed}:{workflow_id}:{node_id}") 前 64bit 归一 < rate → 注入；
    注入种类由同一摘要下一 8bit 取模 F1-F7（F7 不做节点注入——它是全局预算事件，
    由显式 schedule 携带）。
    """
    out: dict[str, str] = {}
    kinds = [k for k in FAILURE_KINDS if k != "F7_budget_exhausted"]
    for n in workflow.get("nodes") or []:
        digest = hashlib.sha256(f"lloom-obfail:{seed}:{workflow['workflow_id']}:{n['id']}".encode("utf-8")).digest()
        u = int.from_bytes(digest[:8], "big")
        if u / 2**64 < rate:
            out[n["id"]] = kinds[digest[8] % len(kinds)]
    return out


def _deps_of(workflow: dict) -> dict[str, set[str]]:
    deps: dict[str, set[str]] = {n["id"]: set() for n in workflow.get("nodes") or []}
    for n in workflow.get("nodes") or []:
        for d in n.get("depends_on") or []:
            if d in deps:
                deps[n["id"]].add(d)
    return deps


def run_recovery(workflow: dict, failures: dict[str, str] | None = None, policy: dict | None = None) -> dict:
    """波次执行 + 确定性恢复状态机。failures: node_id → F*；返回 trace + 指标。

    trace 每节点：{"node_id", "failure", "transitions": [...], "final", "attempts",
    "effective_duration_ms"}——同输入逐字节可复现。
    """
    pol = {**DEFAULT_POLICY, **(policy or {})}
    fails = dict(failures or {})
    # F7 是全局事件：显式出现在任一节点 → 该节点起（含）按波次顺序 ABORT
    budget_exhausted_at = next((nid for nid, k in fails.items() if k == "F7_budget_exhausted"), None)
    deps = _deps_of(workflow)
    durations = _durations(workflow)
    nodes = [n["id"] for n in workflow.get("nodes") or []]
    node_index = {nid: i for i, nid in enumerate(nodes)}
    waves = reference_waves(workflow)
    budget_aborted = budget_exhausted_at is not None and pol["budget_guard"]

    trace: dict[str, dict] = {nid: {"node_id": nid, "failure": fails.get(nid), "transitions": [], "attempts": 0} for nid in nodes}
    for nid in nodes:
        t = trace[nid]
        if budget_aborted and node_index[nid] >= node_index[budget_exhausted_at]:
            t["transitions"] = ["READY", "ABORTED(F7 budget)"]
            t["final"] = "ABORTED"
            t["effective_duration_ms"] = 0
            continue

        kind = fails.get(nid)
        final: str | None = None
        if kind is None:
            t["transitions"] = ["READY", "RUNNING", "SUCCEEDED"]
            t["final"] = "SUCCEEDED"
            t["attempts"] = 1
            t["effective_duration_ms"] = durations[nid]
            continue

        # F4 缺依赖：取决于上游（全依赖 final ∈ OK_STATES 则可重跑一次，否则传播失败）
        if kind == "F4_missing_dependency":
            t["transitions"].append("READY")
            t["transitions"].append(f"FAILED_DEPENDENCY({kind})")
            upstream_ok = all(False for _ in ())  # 上游状态在波次循环中更新——此分支在第二遍处理
            t["final"] = None  # 占位，第二遍按上游终态裁决
            t["attempts"] = 0
            t["_pending_dep"] = True
            continue

        t["transitions"].extend(["READY", "RUNNING"])
        t["attempts"] = 1
        t["transitions"].append(f"FAILED({kind})")

        if kind in RETRYABLE:
            retries_left = int(pol["max_retries"])
            if retries_left > 0:
                t["transitions"].append(f"RETRYING({retries_left} left)")
                t["attempts"] += 1
                t["transitions"].append("SUCCEEDED_RECOVERED")
                t["final"] = "SUCCEEDED_RECOVERED"
                t["effective_duration_ms"] = 2 * durations[nid]
                continue
            t["transitions"].append("FAILED_FINAL")
            t["final"] = "FAILED_FINAL"
            t["effective_duration_ms"] = durations[nid]
            continue

        if kind == "F2_model_unavailable":
            if pol["fallback_model"]:
                t["transitions"].append("FALLBACK(pool tier-1)")
                t["attempts"] += 1
                t["transitions"].append("SUCCEEDED_RECOVERED")
                t["final"] = "SUCCEEDED_RECOVERED"
                t["effective_duration_ms"] = 2 * durations[nid]
            else:
                t["transitions"].append("FAILED_FINAL")
                t["final"] = "FAILED_FINAL"
                t["effective_duration_ms"] = durations[nid]
            continue

        if kind == "F6_partial_aggregation":
            # 聚合输入缺失：不 retry，标 PARTIAL（工作流级 partial 的来源之一）
            t["transitions"].append("PARTIAL")
            t["final"] = "PARTIAL"
            t["effective_duration_ms"] = durations[nid]
            continue

        t["transitions"].append("FAILED_FINAL")
        t["final"] = "FAILED_FINAL"
        t["effective_duration_ms"] = durations[nid]

    # 第二遍：F4 节点按上游终态裁决（波次序保证上游已定）
    for wave in waves:
        for nid in wave:
            t = trace[nid]
            if not t.get("_pending_dep"):
                continue
            upstream = [trace[d]["final"] for d in deps[nid] if d in trace]
            if pol["replan_on_missing_dep"] and all(u in OK_STATES for u in upstream):
                t["transitions"].append("RETRYING(after upstream recovered)")
                t["attempts"] = 1
                t["transitions"].append("SUCCEEDED_RECOVERED")
                t["final"] = "SUCCEEDED_RECOVERED"
                t["effective_duration_ms"] = 2 * durations[nid]
            else:
                t["transitions"].append("FAILED_FINAL(dep upstream not recovered)")
                t["final"] = "FAILED_FINAL"
                t["effective_duration_ms"] = 0
            del t["_pending_dep"]

    # 工作流级指标（方案 §十一）
    finals = [trace[nid]["final"] for nid in nodes]
    injected = [nid for nid in nodes if fails.get(nid) and fails[nid] != "F7_budget_exhausted"]
    recovered = [nid for nid in injected if trace[nid]["final"] == "SUCCEEDED_RECOVERED"]
    f2_injected = [nid for nid in injected if fails[nid] == "F2_model_unavailable"]
    f2_ok = [nid for nid in f2_injected if trace[nid]["final"] == "SUCCEEDED_RECOVERED"]
    n_aborted = sum(1 for f in finals if f == "ABORTED")
    n_ok = sum(1 for f in finals if f in OK_STATES)
    if all(f in OK_STATES for f in finals) and not budget_aborted:
        workflow_final = "task_success"
    elif n_ok > 0 and (n_ok >= len(nodes) / 2 or any(f == "PARTIAL" for f in finals)):
        workflow_final = "partial_success"
    else:
        workflow_final = "failed"
    metrics = {
        "n_nodes": len(nodes),
        "n_injected": len(injected),
        "recovery_rate": (len(recovered) / len(injected)) if injected else None,
        "fallback_success_rate": (len(f2_ok) / len(f2_injected)) if f2_injected else None,
        "abort_rate": (n_aborted / len(nodes)) if nodes else None,
        "n_ok": n_ok,
        "n_failed_final": sum(1 for f in finals if f == "FAILED_FINAL"),
        "n_partial_nodes": sum(1 for f in finals if f == "PARTIAL"),
        "workflow_final": workflow_final,
        "budget_aborted": budget_aborted,
    }
    total_ms = sum(trace[nid]["effective_duration_ms"] for nid in nodes)
    metrics["total_effective_ms"] = total_ms
    return {"trace": trace, "metrics": metrics}


def _self_check() -> int:
    """F1-F7 全族确定性自检（手算预期）。"""
    ok = True

    def expect(cond: bool, label: str, detail: str = "") -> None:
        nonlocal ok
        print(("PASS " if cond else "FAIL ") + label + (f" {detail}" if detail else ""))
        ok = ok and cond

    wf = json.loads((HERE / "manifests" / "fixtures" / "fixture_fanout.json").read_text(encoding="utf-8"))

    # 无失败 → task_success
    r = run_recovery(wf, {})
    expect(r["metrics"]["workflow_final"] == "task_success", "无注入 → task_success")

    # F1 retry 恢复（fanout t1）
    r = run_recovery(wf, {"t1": "F1_timeout"})
    t = r["trace"]["t1"]
    expect(t["final"] == "SUCCEEDED_RECOVERED" and t["attempts"] == 2, "F1 retry 恢复")
    expect(r["metrics"]["recovery_rate"] == 1.0, "F1 recovery_rate=1.0")
    expect(r["metrics"]["workflow_final"] == "task_success", "F1 恢复后 task_success")

    # F2 fallback 关闭 → 终败；开启 → 恢复
    r = run_recovery(wf, {"t3": "F2_model_unavailable"}, {"fallback_model": False})
    expect(r["trace"]["t3"]["final"] == "FAILED_FINAL", "F2 关 fallback → FAILED_FINAL")
    expect(r["metrics"]["workflow_final"] == "partial_success", "F2 终败 → partial_success")
    r = run_recovery(wf, {"t3": "F2_model_unavailable"})
    expect(r["trace"]["t3"]["final"] == "SUCCEEDED_RECOVERED", "F2 fallback 恢复")
    expect(r["metrics"]["fallback_success_rate"] == 1.0, "fallback_success_rate=1.0")

    # F4：上游恢复 → 下游重跑成功；上游禁恢复 → 传播失败
    r = run_recovery(wf, {"t1": "F1_timeout", "t3": "F4_missing_dependency"})
    expect(r["trace"]["t3"]["final"] == "SUCCEEDED_RECOVERED", "F4 上游恢复 → 下游重跑")
    r = run_recovery(wf, {"t1": "F1_timeout", "t2": "F1_timeout", "t3": "F4_missing_dependency"}, {"max_retries": 0})
    expect(r["trace"]["t3"]["final"] == "FAILED_FINAL", "F4 上游终败 → 传播 FAILED_FINAL")

    # F5：波内其他节点不受影响
    r = run_recovery(wf, {"t2": "F5_parallel_branch_failure"})
    expect(r["trace"]["t1"]["final"] == "SUCCEEDED" and r["trace"]["t2"]["final"] == "SUCCEEDED_RECOVERED", "F5 隔离波内")

    # F6：聚合节点 PARTIAL
    r = run_recovery(wf, {"t4": "F6_partial_aggregation"})
    expect(r["trace"]["t4"]["final"] == "PARTIAL", "F6 聚合节点 PARTIAL")
    expect(r["metrics"]["workflow_final"] == "partial_success", "F6 → partial_success")

    # F7：预算耗尽，t4 起全 ABORT（声明序 t4 在最后）
    r = run_recovery(wf, {"t4": "F7_budget_exhausted"})
    expect(r["trace"]["t4"]["final"] == "ABORTED", "F7 ABORTED")
    expect(r["metrics"]["budget_aborted"] is True and r["metrics"]["workflow_final"] in ("partial_success", "failed"), "F7 全局终态")
    expect(r["trace"]["t1"]["final"] == "SUCCEEDED", "F7 前序节点不受影响")

    # 确定性：同输入两次 identical
    a = json.dumps(run_recovery(wf, inject_by_hash(wf, 0.5)), sort_keys=True)
    b = json.dumps(run_recovery(wf, inject_by_hash(wf, 0.5)), sort_keys=True)
    expect(a == b, "确定性（哈希注入两次 identical）")
    # 哈希注入可重复性：同 workflow 同 seed 同 node → 同判定
    inj1 = inject_by_hash(wf, 0.5, seed=3407)
    inj2 = inject_by_hash(wf, 0.5, seed=3407)
    expect(inj1 == inj2, "哈希注入可重复")

    return 0 if ok else 1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--self-check", action="store_true")
    args = ap.parse_args()
    if args.self_check:
        return _self_check()
    ap.print_help()
    return 2


if __name__ == "__main__":
    sys.exit(main())
