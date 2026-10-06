"""report.py — OrchestrationBench offline report + case-level attribution（O-Day 14）。

角色（方案 §二十四/§十三）：把 O1 Planning / O2 Scheduling / O3 Recovery 三层评估输出
聚合成 per-case 归因（primary_error + secondary_errors，P1-P11 分类），落独立库
bench_orchestration_runs/items，并产出 reports/offline_v0.1.{json,md}。

P1-P11 归因映射（方案 §十三，口径固化）：
  P1 Missing Subtask        planning subtask_recall < 1
  P2 Spurious Subtask       redundancy_rate > 0
  P3 Wrong Dependency       dependency_precision < 1
  P4 Invalid Parallelism    illegal_parallelism_rate > 0
  P5 Unnecessary Serial     sequentialization_waste_ms > 0
  P6 Wrong Model Assign     mean_routing_regret > 0（bridge 层报告时）
  P7 Execution Failure      注入后 FAILED_FINAL 节点数 > 0
  P8 Fallback Failure       F2 注入且 fallback 后仍 FAILED_FINAL
  P9 Recovery Failure       注入节点未恢复（final ∉ OK_STATES）
  P10 Aggregation Failure   PARTIAL 节点 > 0
  P11 Final Goal Failure    workflow_final != task_success（仅当无更早归因时作 primary）

primary_error 取最早层（O1→O2→routing→O3→goal，方案 §十三因果链方向）；
其余全部进 secondary_errors。

demo 用法（fixture 4 case 全链路）：
  python3 report.py --demo          # 扰动 predicted + 注入失败 → 归因报告
正式用法（O-6 online 后）：调用方传入真实 predicted workflow。

零成本红线：纯 stdlib、无网络、无 LLM。
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import sqlite3
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from adapters.planning_eval import evaluate  # noqa: E402
from diagnose import run_recovery  # noqa: E402
from schedule import evaluate_schedule  # noqa: E402
from validate import validate_workflow  # noqa: E402

BENCHMARK_ID = "lloom-orchbench-v0.1"


def attribute_case(
    planning: dict | None,
    scheduling: dict | None,
    recovery: dict | None,
) -> dict:
    """三层评估输出 → P1-P11 归因（primary = 最早层，secondary = 全集）。"""
    errors: list[str] = []
    if planning:
        if planning.get("subtask_recall") is not None and planning["subtask_recall"] < 1.0:
            errors.append("P1")
        if planning.get("redundancy_rate") is not None and planning["redundancy_rate"] > 0:
            errors.append("P2")
        if planning.get("dependency_precision") is not None and planning["dependency_precision"] < 1.0:
            errors.append("P3")
        elif planning.get("dependency_recall") is not None and planning["dependency_recall"] < 1.0:
            # 漏依赖边：依赖表达不完整的依赖面（方案 §九 9.4 dependency accuracy 两分量）
            errors.append("P3")
    if scheduling:
        if scheduling.get("illegal_parallelism_rate", 0) > 0:
            errors.append("P4")
        if scheduling.get("sequentialization_waste_ms", 0) > 0:
            errors.append("P5")
    if recovery:
        m = recovery["metrics"]
        trace = recovery["trace"]
        f2_final_failed = [
            t for t in trace.values() if t.get("failure") == "F2_model_unavailable" and t["final"] == "FAILED_FINAL"
        ]
        if f2_final_failed:
            errors.append("P8")
        unrecovered = [
            t for t in trace.values() if t.get("failure") and t["final"] not in ("SUCCEEDED", "SUCCEEDED_RECOVERED", "PARTIAL", "ABORTED")
        ]
        if unrecovered:
            errors.append("P9")
        if any(t["final"] == "FAILED_FINAL" for t in trace.values()):
            errors.append("P7")
        if m.get("n_partial_nodes", 0) > 0:
            errors.append("P10")
    primary = errors[0] if errors else None
    wf_final = recovery["metrics"]["workflow_final"] if recovery else None
    if wf_final is not None and wf_final != "task_success":
        errors.append("P11")
        if primary is None:
            primary = "P11"
    return {"primary_error": primary, "secondary_errors": errors[1:] if primary and primary in errors else errors}


def run_case(workflow: dict, *, predicted: dict | None = None, failures: dict[str, str] | None = None, policy: str = "reference", recovery_policy: dict | None = None) -> dict:
    """单 case 全链评估：planning（predicted vs reference）+ scheduling + recovery。"""
    out: dict = {"workflow_id": workflow["workflow_id"]}
    out["planning"] = evaluate(workflow, predicted) if predicted else None
    out["scheduling"] = evaluate_schedule(workflow, policy)
    out["recovery"] = run_recovery(workflow, failures, recovery_policy) if failures is not None else None
    out["attribution"] = attribute_case(out["planning"], out["scheduling"], out["recovery"])
    return out


def manifest_hash() -> str:
    """组件 hash 集合 → manifest（方案 §十九：schema/evaluator/schedule/diagnose 任一变化
    = 新 benchmark identity）。"""
    h = hashlib.sha256()
    for rel in (
        "manifests/workflow.schema.json",
        "validate.py",
        "adapters/planning_eval.py",
        "schedule.py",
        "diagnose.py",
        "router_bridge.py",
    ):
        p = HERE / rel
        h.update(rel.encode())
        h.update(hashlib.sha256(p.read_bytes()).digest())
    return h.hexdigest()[:16]


def save_run(items: list[dict], db_path: Path, *, strategy: str, variant: str, parameter_json: str) -> int:
    """落库 bench_orchestration_runs + items（独立库红线：拒绝 data/lloom.db）。"""
    if db_path.name == "lloom.db" or "data" in db_path.parts:
        raise SystemExit("✗ 拒绝：orchbench 只允许独立库")
    conn = sqlite3.connect(db_path)
    try:
        summary = {
            "n_cases": len(items),
            "n_task_success": sum(1 for i in items if i["workflow_final"] == "task_success"),
            "n_partial": sum(1 for i in items if i["workflow_final"] == "partial_success"),
            "n_failed": sum(1 for i in items if i["workflow_final"] == "failed"),
            "primary_error_dist": _dist([i["primary_error"] for i in items]),
        }
        cur = conn.execute(
            "INSERT INTO bench_orchestration_runs (benchmark_id, manifest_hash, strategy, variant, parameter_json, summary_json) VALUES (?,?,?,?,?,?)",
            (
                BENCHMARK_ID,
                manifest_hash(),
                strategy,
                variant,
                parameter_json,
                json.dumps(summary, ensure_ascii=False),
            ),
        )
        run_id = cur.lastrowid
        for it in items:
            cur_item = conn.execute(
                "INSERT INTO bench_orchestration_items (run_id, sample_id, predicted_workflow_json, execution_trace_json, task_success, planning_score, scheduling_score, execution_score, cost, makespan_ms, primary_error, secondary_errors_json, decision_json) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
                (
                    run_id,
                    it["workflow_id"],
                    json.dumps(it.get("predicted"), ensure_ascii=False) if it.get("predicted") else None,
                    json.dumps(it.get("recovery_trace"), ensure_ascii=False) if it.get("recovery_trace") else None,
                    1 if it["workflow_final"] == "task_success" else 0,
                    it.get("planning_score"),
                    it.get("scheduling_score"),
                    it.get("execution_score"),
                    None,
                    it.get("makespan_ms"),
                    it["primary_error"],
                    json.dumps(it["secondary_errors"], ensure_ascii=False),
                    json.dumps(it.get("decision"), ensure_ascii=False) if it.get("decision") else None,
                ),
            )
            _ = cur_item
        conn.commit()
        return run_id
    finally:
        conn.close()


def _dist(values: list) -> dict:
    out: dict = {}
    for v in values:
        out[v if v is not None else "none"] = out.get(v if v is not None else "none", 0) + 1
    return out


def demo() -> int:
    """4 fixture case 全链路 demo：扰动 predicted + 注入失败 → 归因报告。"""
    ok = True

    def expect(cond: bool, label: str, detail: str = "") -> None:
        nonlocal ok
        print(("PASS " if cond else "FAIL ") + label + (f" {detail}" if detail else ""))
        ok = ok and cond

    def load(name: str) -> dict:
        return json.loads((HERE / "manifests" / "fixtures" / name).read_text(encoding="utf-8"))

    cases = []
    chain = load("fixture_chain.json")

    # case 1：clean（predicted=reference，无注入）→ 全绿无归因
    c1 = run_case(chain, predicted=chain, failures={})
    c1["case_id"] = "ob_fixture_chain_v1__clean"
    cases.append(c1)

    # case 2：planning 缺节点（P1）+ 漏依赖边（P3）
    broken_plan = copy.deepcopy(chain)
    broken_plan["nodes"] = broken_plan["nodes"][:2]
    broken_plan["edges"] = [{"from": "t1", "to": "t2", "type": "data"}]
    broken_plan["nodes"][1]["depends_on"] = ["t1"]
    c2 = run_case(chain, predicted=broken_plan, failures={})
    c2["case_id"] = "ob_fixture_chain_v1__missing_node"
    cases.append(c2)

    # case 3：scheduling 非法并行（P4）——bad_waves 消费 lloom_waves 划分的结论
    bad_waves = load("fixture_bad_waves.json")
    case3 = run_case(bad_waves, predicted=bad_waves, failures={})
    case3["scheduling"]["illegal_parallelism_rate"] = 1.0  # 消费 lloom_waves 划分的结论
    case3["attribution"] = attribute_case(case3["planning"], case3["scheduling"], case3["recovery"])
    case3["case_id"] = "ob_fixture_bad_waves_v1__illegal_parallelism"
    cases.append(case3)

    # case 4：recovery F2 关 fallback（P8/P9/P7）→ partial
    c4 = run_case(
        chain,
        predicted=chain,
        failures={"t3": "F2_model_unavailable"},
        recovery_policy={"fallback_model": False},
    )
    c4["case_id"] = "ob_fixture_chain_v1__f2_no_fallback"
    cases.append(c4)

    expect(len(cases) == 4, "4 case 全链评估")
    expect(cases[0]["attribution"]["primary_error"] is None, "case1 clean 无归因", str(cases[0]["attribution"]))
    expect(cases[0]["attribution"]["secondary_errors"] == [], "case1 无 secondary")
    expect(cases[1]["attribution"]["primary_error"] == "P1", "case2 primary=P1", str(cases[1]["attribution"]))
    expect("P3" in cases[1]["attribution"]["secondary_errors"], "case2 secondary 含 P3")
    expect(cases[2]["attribution"]["primary_error"] == "P4", "case3 primary=P4")
    c4 = cases[3]["attribution"]
    expect(c4["primary_error"] in ("P7", "P8", "P9"), "case4 recovery 归因", str(c4))

    # 落库 + 报告
    db = HERE / "orchbench.db"
    items = []
    for c in cases:
        items.append(
            {
                "workflow_id": c["case_id"],
                "predicted": None,
                "recovery_trace": c["recovery"]["trace"] if c["recovery"] else None,
                "workflow_final": c["recovery"]["metrics"]["workflow_final"] if c["recovery"] else "task_success",
                "planning_score": c["planning"]["subtask_recall"] if c["planning"] else None,
                "scheduling_score": 1.0 - c["scheduling"]["illegal_parallelism_rate"],
                "execution_score": None,
                "makespan_ms": c["scheduling"]["actual_makespan_ms"],
                "primary_error": c["attribution"]["primary_error"],
                "secondary_errors": c["attribution"]["secondary_errors"],
                "decision": {"policy": c["scheduling"]["policy"]},
            }
        )
    run_id = save_run(items, db, strategy="offline_demo", variant="fixture_perturbation", parameter_json="{}")
    expect(run_id > 0, f"落库 run_id={run_id}")

    report = {
        "benchmark_id": BENCHMARK_ID,
        "manifest_hash": manifest_hash(),
        "n_cases": len(items),
        "summary": {
            "task_success": sum(1 for i in items if i["workflow_final"] == "task_success"),
            "partial": sum(1 for i in items if i["workflow_final"] == "partial_success"),
            "failed": sum(1 for i in items if i["workflow_final"] == "failed"),
            "primary_error_dist": _dist([i["primary_error"] for i in items]),
        },
        "cases": [
            {
                "workflow_id": i["workflow_id"],
                "workflow_final": i["workflow_final"],
                "planning_recall": i["planning_score"],
                "makespan_ms": i["makespan_ms"],
                "primary_error": i["primary_error"],
                "secondary_errors": i["secondary_errors"],
            }
            for i in items
        ],
    }
    out_dir = HERE / "reports"
    out_dir.mkdir(exist_ok=True)
    (out_dir / "offline_v0.1.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    lines = [
        "# OrchestrationBench v0.1 · Offline Report（demo）",
        "",
        f"- benchmark_id: {report['benchmark_id']}",
        f"- manifest_hash: {report['manifest_hash'][:8]}…",
        f"- cases: {report['n_cases']}（task_success {report['summary']['task_success']} / partial {report['summary']['partial']} / failed {report['summary']['failed']}）",
        f"- primary_error 分布: {report['summary']['primary_error_dist']}",
        "",
        "| case | final | planning recall | makespan ms | primary | secondary |",
        "|---|---|---|---|---|---|",
    ]
    for c in report["cases"]:
        lines.append(
            f"| {c['workflow_id']} | {c['workflow_final']} | {c['planning_recall']} | {c['makespan_ms']} | {c['primary_error']} | {','.join(c['secondary_errors']) or '-'} |"
        )
    lines += [
        "",
        "> demo 口径：predicted = 手工扰动 fixture（planning 侧演示），失败注入显式 schedule；",
        "> 正式数据源等 O-6 online pilot 的真实 predicted plan（零成本红线，本阶段不跑）。",
        "",
    ]
    (out_dir / "offline_v0.1.md").write_text("\n".join(lines), encoding="utf-8")
    print(f"✓ 报告落盘 {out_dir / 'offline_v0.1.md'} 与 .json；run_id={run_id}")
    return 0 if ok else 1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--demo", action="store_true")
    args = ap.parse_args()
    if args.demo:
        return demo()
    ap.print_help()
    return 2


if __name__ == "__main__":
    sys.exit(main())
