#!/usr/bin/env python3
"""schedule.py — OrchestrationBench 确定性调度模拟器（方案 §二十/§十，O-Day 11）。

角色：Offline scheduler 是 **deterministic reference implementation**，用于验证生产
scheduling policy（方案 §二十）——不是 mock。同输入必须得到相同 schedule。

与生产真源的关系（协议 §3 红线）：
- 生产波次语义真源 = `crates/lloom-core/src/orchestrator.rs::dependency_waves`；
- 本文件不复制该逻辑作为「被测对象」：策略 `lloom_waves` 只消费 workflow.waves
  （生产 policy 的预存输出）；waves=null 时退回 `reference`（Kahn 分层参照）；
- 生产 parity 由 orchestrator.rs 的 `#[cfg(test)]` 缝实现：用同一批 fixture 断言
  dependency_waves 输出与 fixture.waves 一致（零改动生产代码）。

策略集（方案 §十五 Scheduling baselines 的 offline 可实现子集）：
  reference            Kahn 分层波次（合法性参照；wave 内并发、wave 间 barrier）
  sequential           全串行（Static Sequential baseline，makespan 上界）
  greedy_ready_queue   事件驱动 ready 队列（max_concurrency 可调；无 wave barrier）
  lloom_waves          消费 workflow.waves 预存划分（生产 policy 输出的载体）

指标（方案 §十 10.1-10.7，口径在代码处固化）：
  wave_count / serial_baseline_ms / ideal_legal_makespan_ms（= critical path，无限并发下限）
  actual_makespan_ms / sequentialization_waste = actual − ideal
  illegal_parallelism_rate：同波依赖对数 / 波内节点对数（对划分型策略）；事件型策略恒 0（天然合法）
  parallelization_utilization = (serial − actual) / (serial − ideal) ∈ 可能负（比串行差）
  critical_path_ms + 节点序列；parallel_width mean/max（时间线并发宽度）

确定性：无随机源；同时刻就绪按节点声明序；duration 缺省 DEFAULT_DURATION_MS。
零成本红线：纯 stdlib、无网络、无 LLM。

用法：
  python3 schedule.py manifests/fixtures/fixture_diamond.json --policy reference
  python3 schedule.py --self-check
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from validate import validate_workflow  # noqa: E402

DEFAULT_DURATION_MS = 500
POLICIES = ("reference", "sequential", "greedy_ready_queue", "lloom_waves")


def _durations(workflow: dict) -> dict[str, int]:
    out = {}
    for n in workflow.get("nodes") or []:
        d = n.get("duration_ms")
        out[n["id"]] = int(d) if isinstance(d, int) and d >= 0 else DEFAULT_DURATION_MS
    return out


def _order_deps(workflow: dict) -> dict[str, set[str]]:
    """data+control 先序依赖（resource 不参与；与 validator 同口径）。"""
    deps: dict[str, set[str]] = {n["id"]: set() for n in workflow.get("nodes") or []}
    for e in workflow.get("edges") or []:
        if e.get("type") in ("data", "control") and e.get("from") in deps and e.get("to") in deps:
            deps[e["to"]].add(e["from"])
    for n in workflow.get("nodes") or []:
        for d in n.get("depends_on") or []:
            if d in deps:
                deps[n["id"]].add(d)
    return deps


def reference_waves(workflow: dict) -> list[list[str]]:
    """Kahn 分层（评测参照）：每层=当时全部就绪节点；节点声明序。输入必须已过 validator。"""
    deps = _order_deps(workflow)
    order = [n["id"] for n in workflow.get("nodes") or []]
    settled: set[str] = set()
    pending = list(order)
    waves: list[list[str]] = []
    while pending:
        ready = [nid for nid in pending if deps[nid] <= settled]
        if not ready:  # 环（validator 已拒；防御：声明序破环）
            ready = [pending[0]]
        waves.append(ready)
        settled.update(ready)
        pending = [nid for nid in pending if nid not in settled]
    return waves


def _illegal_parallel_pairs(waves: list[list[str]], deps: dict[str, set[str]]) -> list[tuple[str, str]]:
    """同波内依赖对（P4 Invalid Parallelism 的划分层证据）。"""
    bad = []
    for wave in waves:
        in_wave = set(wave)
        for nid in wave:
            for d in deps.get(nid, ()):  # 依赖同波即非法
                if d in in_wave:
                    bad.append((d, nid))
    return bad


def _critical_path(workflow: dict, durations: dict[str, int], deps: dict[str, set[str]]) -> tuple[int, list[str]]:
    """DAG 最长路径（duration 和）+ 节点序列（唯一化：并列取声明序前者）。"""
    nodes = [n["id"] for n in workflow.get("nodes") or []]
    best: dict[str, tuple[int, list[str]]] = {}
    for nid in nodes:  # 拓扑序保证（validator 已拒环）
        preds = [best[d] for d in deps[nid] if d in best]
        if preds:
            top = max(preds, key=lambda t: (t[0], [-nodes.index(x) for x in t[1]]))
            best[nid] = (top[0] + durations[nid], top[1] + [nid])
        else:
            best[nid] = (durations[nid], [nid])
    if not best:
        return 0, []
    top = max(best.values(), key=lambda t: t[0])
    return top[0], top[1]


def simulate_waves(waves: list[list[str]], durations: dict[str, int]) -> dict:
    """波次策略模拟：wave 内并发、wave 间 barrier。返回 start/end 时间线。"""
    schedule: dict[str, dict] = {}
    t = 0
    for wi, wave in enumerate(waves):
        wave_end = t + max((durations.get(nid, DEFAULT_DURATION_MS) for nid in wave), default=0)
        for nid in wave:
            schedule[nid] = {"start_ms": t, "end_ms": t + durations.get(nid, DEFAULT_DURATION_MS), "wave": wi}
        t = wave_end
    return {"schedule": schedule, "makespan_ms": t}


def simulate_events(
    workflow: dict, durations: dict[str, int], max_concurrency: int | None = None
) -> dict:
    """事件驱动模拟：依赖就绪即调度（无 wave barrier），同时就绪按声明序，确定性。"""
    deps = _order_deps(workflow)
    nodes = [n["id"] for n in workflow.get("nodes") or []]
    dependents: dict[str, list[str]] = {nid: [] for nid in nodes}
    indeg: dict[str, int] = {}
    for nid in nodes:
        indeg[nid] = len(deps[nid])
        for d in deps[nid]:
            dependents[d].append(nid)
    cap = max_concurrency if (max_concurrency is not None and max_concurrency >= 1) else len(nodes)
    schedule: dict[str, dict] = {}
    running: list[tuple[int, str]] = []  # (end_ms, nid)
    ready = [nid for nid in nodes if indeg[nid] == 0]
    t = 0
    while ready or running:
        while ready and len(running) < cap:
            nid = ready.pop(0)
            start = t
            end = start + durations.get(nid, DEFAULT_DURATION_MS)
            schedule[nid] = {"start_ms": start, "end_ms": end}
            running.append((end, nid))
        if not running:
            break
        running.sort(key=lambda x: (x[0], nodes.index(x[1])))
        t = running[0][0]
        finished = [r for r in running if r[0] <= t]
        running = [r for r in running if r[0] > t]
        for _, nid in finished:
            for nxt in dependents[nid]:
                indeg[nxt] -= 1
                if indeg[nxt] == 0:
                    ready.append(nxt)
    makespan = max((s["end_ms"] for s in schedule.values()), default=0)
    return {"schedule": schedule, "makespan_ms": makespan}


def _parallel_width(schedule: dict[str, dict], makespan: int) -> tuple[float, int]:
    """时间线并发宽度 mean/max（事件点扫描，确定性）。"""
    if not schedule:
        return 0.0, 0
    points = sorted({s["start_ms"] for s in schedule.values()} | {s["end_ms"] for s in schedule.values()})
    widths = []
    for i in range(len(points) - 1):
        mid = (points[i] + points[i + 1]) / 2
        w = sum(1 for s in schedule.values() if s["start_ms"] <= mid < s["end_ms"])
        widths.append(w)
    return (sum(widths) / len(widths) if widths else 0.0), (max(widths) if widths else 0)


def evaluate_schedule(workflow: dict, policy: str = "reference", max_concurrency: int | None = None) -> dict:
    """对单个 workflow 按策略模拟并产出方案 §十 指标。输入必须已过 validator。"""
    if policy not in POLICIES:
        raise ValueError(f"未知策略 {policy!r}，可选 {POLICIES}")
    durations = _durations(workflow)
    deps = _order_deps(workflow)
    nodes = [n["id"] for n in workflow.get("nodes") or []]

    cp_ms, cp_nodes = _critical_path(workflow, durations, deps)
    serial = sum(durations.get(nid, DEFAULT_DURATION_MS) for nid in nodes)

    violation_note = None
    if policy == "sequential":
        waves = [[nid] for nid in nodes]
        sim = simulate_waves(waves, durations)
    elif policy == "reference":
        waves = reference_waves(workflow)
        sim = simulate_waves(waves, durations)
    elif policy == "greedy_ready_queue":
        waves = None
        sim = simulate_events(workflow, durations, max_concurrency)
    else:  # lloom_waves：消费生产 policy 预存输出；null 退回 reference
        waves = workflow.get("waves")
        if waves is None:
            waves = reference_waves(workflow)
            violation_note = "waves=null → 退回 reference（无生产划分可评测）"
        sim = simulate_waves(waves, durations)

    actual = sim["makespan_ms"]
    ideal = cp_ms
    waste = actual - ideal
    utilization = (serial - actual) / (serial - ideal) if (serial - ideal) > 0 else (0.0 if actual <= serial else None)
    width_mean, width_max = _parallel_width(sim["schedule"], actual)

    out: dict = {
        "policy": policy,
        "max_concurrency": max_concurrency,
        "wave_count": len(waves) if waves is not None else None,
        "waves": waves,
        "serial_baseline_ms": serial,
        "ideal_legal_makespan_ms": ideal,
        "actual_makespan_ms": actual,
        "sequentialization_waste_ms": waste,
        "parallelization_utilization": utilization,
        "critical_path_ms": cp_ms,
        "critical_path_nodes": cp_nodes,
        "parallel_width_mean": width_mean,
        "parallel_width_max": width_max,
        "schedule": sim["schedule"],
        "note": violation_note,
    }
    # illegal parallelism：仅划分型策略可从 waves 判定（事件型天然依赖合法）
    if waves is not None:
        bad = _illegal_parallel_pairs(waves, deps)
        pairs_in_waves = sum(len(w) * (len(w) - 1) // 2 for w in waves)
        out["illegal_parallel_pairs"] = bad
        out["illegal_parallelism_rate"] = (len(bad) / pairs_in_waves) if pairs_in_waves else 0.0
    else:
        out["illegal_parallel_pairs"] = []
        out["illegal_parallelism_rate"] = 0.0
    return out


def self_check() -> int:
    """fixture 全链自评（方案 §十 口径固化；预期值手算）。"""
    ok = True

    def load(name: str) -> dict:
        return json.loads((HERE / "manifests" / "fixtures" / name).read_text(encoding="utf-8"))

    def expect(cond: bool, label: str, detail: str = "") -> None:
        nonlocal ok
        print(("PASS " if cond else "FAIL ") + label + (f" {detail}" if detail else ""))
        ok = ok and cond

    # diamond：ideal=1000+1200+1000=3200；reference waves=[[t1],[t2,t3],[t4]] → actual=3200；serial=4000
    wf = load("fixture_diamond.json")
    r = evaluate_schedule(wf, "reference")
    expect(r["wave_count"] == 3 and r["waves"] == [["t1"], ["t2", "t3"], ["t4"]], "diamond reference waves", str(r["waves"]))
    expect(r["ideal_legal_makespan_ms"] == 3200 and r["actual_makespan_ms"] == 3200, "diamond makespan", f"ideal={r['ideal_legal_makespan_ms']} actual={r['actual_makespan_ms']}")
    expect(r["serial_baseline_ms"] == 4000, "diamond serial baseline")
    expect(r["parallelization_utilization"] == 1.0 and r["sequentialization_waste_ms"] == 0, "diamond utilization=1.0 waste=0")
    expect(r["illegal_parallelism_rate"] == 0.0, "diamond no illegal parallelism")

    # fanout：t1=800,t2=1000,t3=900,t4=1500 → ideal=3400（t2→t3→t4），reference actual=3400，serial=4200
    wf = load("fixture_fanout.json")
    r = evaluate_schedule(wf, "reference")
    expect(r["ideal_legal_makespan_ms"] == 3400 and r["actual_makespan_ms"] == 3400, "fanout makespan=3400")
    expect(r["serial_baseline_ms"] == 4200 and r["parallelization_utilization"] == 1.0, "fanout serial/utilization")
    # greedy max_concurrency=1 → 退化为串行 makespan=4200，utilization=0
    rg = evaluate_schedule(wf, "greedy_ready_queue", max_concurrency=1)
    expect(rg["actual_makespan_ms"] == 4200 and rg["parallelization_utilization"] == 0.0, "fanout greedy cap=1 == serial")
    # greedy cap=2：t1(800)+t2(1000) 并行；t3 900；t4 1500 → t2 完 1000 → t3 完 1900 → t4 完 3400？critical path 3400
    rg2 = evaluate_schedule(wf, "greedy_ready_queue", max_concurrency=2)
    expect(rg2["actual_makespan_ms"] == 3400, "fanout greedy cap=2 reaches ideal", f"actual={rg2['actual_makespan_ms']}")

    # bad_waves：lloom_waves 消费非法划分 → illegal_parallelism_rate > 0
    wf = load("fixture_bad_waves.json")
    r = evaluate_schedule(wf, "lloom_waves")
    expect(r["illegal_parallelism_rate"] > 0.0, "bad_waves illegal parallelism caught", str(r["illegal_parallelism_rate"]))

    # chain：全串行 → utilization=0，waste=0
    wf = load("fixture_chain.json")
    r = evaluate_schedule(wf, "reference")
    expect(r["wave_count"] == 3 and r["parallelization_utilization"] == 0.0 and r["sequentialization_waste_ms"] == 0, "chain serial workflow utilization=0")

    # 确定性：同输入两次 identical
    wf = load("fixture_fanout.json")
    a = json.dumps(evaluate_schedule(wf, "greedy_ready_queue"), sort_keys=True, ensure_ascii=False)
    b = json.dumps(evaluate_schedule(wf, "greedy_ready_queue"), sort_keys=True, ensure_ascii=False)
    expect(a == b, "determinism (same input → same schedule)")

    return 0 if ok else 1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("inputs", nargs="*", help="workflow json 文件/目录（须在 benchmarks/orchestration/ 内）")
    ap.add_argument("--policy", default="reference", choices=POLICIES)
    ap.add_argument("--max-concurrency", type=int, default=None)
    ap.add_argument("--self-check", action="store_true")
    args = ap.parse_args()
    if args.self_check:
        return self_check()
    root = HERE.resolve()
    for raw in args.inputs:
        p = Path(raw).resolve()
        try:
            p.relative_to(root)
        except ValueError:
            print(f"✗ 拒收路径 {p}（只接受 benchmarks/orchestration/ 内的路径）")
            return 2
        files = sorted([p] if p.is_file() else (x for x in p.iterdir() if x.suffix == ".json" and x.is_file()))
        for f in files:
            wf = json.loads(f.read_text(encoding="utf-8"))
            if validate_workflow(wf):
                print(f"SKIP {f.name}（validator 拒收）")
                continue
            r = evaluate_schedule(wf, args.policy, args.max_concurrency)
            print(f"{f.name}: policy={r['policy']} waves={r['wave_count']} serial={r['serial_baseline_ms']} "
                  f"ideal={r['ideal_legal_makespan_ms']} actual={r['actual_makespan_ms']} "
                  f"waste={r['sequentialization_waste_ms']} util={r['parallelization_utilization']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
