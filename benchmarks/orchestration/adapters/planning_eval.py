"""adapters/planning_eval.py — 确定性 planning evaluator（方案 §九，O-Day 10）。

零成本红线：无 LLM、无网络——全部指标由集合运算与字符 n-gram 匹配确定性导出。

指标（方案 §九 9.1-9.6）：
- Subtask Recall       预测节点覆盖的 required obligations / 总 obligations
- Subtask Precision    预测节点中对上 reference 的 / 全部预测节点
- Redundancy Rate      未对上的预测节点 / 全部预测节点（= 1 - precision，单列防止口径漂移）
- Dependency Edge P/R  有 reference graph 时，预测边映射到 reference 节点后比对
- Invalid Plan Rate    batch 层：predicted workflow 过 validator 的失败比例
- Unsolvable Recognition  reference 标注 unsolvable 时预测也判 unsolvable 即正确

匹配算子（确定性）：字符 2-gram Jaccard + 贪心一对一分配（按相似度降序，平分按
节点声明序——禁 random 模块红线）。阈值 tau 默认 0.45，变更=新 evaluator 版本
（方案 §十九 evaluator_hash 治理）。

用法：
  python3 adapters/planning_eval.py --self-check     # 自评（reference vs reference ≡ 1.0）
  import 侧： evaluate(reference, predicted) / evaluate_batch(pairs)
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from validate import validate_workflow  # noqa: E402

DEFAULT_TAU = 0.45


def bigrams(text: str) -> set[str]:
    """字符 2-gram 集合（中文友好；单字符退化为 1-gram 自身）。"""
    t = "".join(text.split())
    if len(t) < 2:
        return {t} if t else set()
    return {t[i : i + 2] for i in range(len(t) - 1)}


def similarity(a: str, b: str) -> float:
    ba, bb = bigrams(a), bigrams(b)
    if not ba or not bb:
        return 0.0
    return len(ba & bb) / len(ba | bb)


def align_nodes(ref_nodes: list[dict], pred_nodes: list[dict], tau: float) -> tuple[dict[str, str], set[str], set[str]]:
    """贪心一对一节点对齐。返回 (pred_id → ref_id 映射, 已匹配 ref 集, 已匹配 pred 集)。

    确定性：全部 pair 按 (-sim, ref 序, pred 序) 排序后依次分配未占用项。
    """
    scored = []
    for ri, rn in enumerate(ref_nodes):
        for pi, pn in enumerate(pred_nodes):
            s = similarity(str(rn.get("description", "")), str(pn.get("description", "")))
            if s >= tau:
                scored.append((-s, ri, pi, rn["id"], pn["id"]))
    scored.sort()
    matched_pred: dict[str, str] = {}
    used_ref: set[str] = set()
    used_pred: set[str] = set()
    for _, ri, pi, ref_id, pred_id in scored:
        if ref_id in used_ref or pred_id in used_pred:
            continue
        matched_pred[pred_id] = ref_id
        used_ref.add(ref_id)
        used_pred.add(pred_id)
    return matched_pred, used_ref, used_pred


def evaluate(reference: dict, predicted: dict, tau: float = DEFAULT_TAU) -> dict:
    """单 workflow 对评测。返回指标 dict（含 per-node 明细键，报告层直接用）。"""
    ref_nodes = reference.get("nodes") or []
    pred_nodes = predicted.get("nodes") or []
    ref_agg = (reference.get("aggregation") or {}).get("type")
    pred_agg = (predicted.get("aggregation") or {}).get("type")

    out: dict = {
        "n_reference_nodes": len(ref_nodes),
        "n_predicted_nodes": len(pred_nodes),
    }

    # 不可解识别（§九 9.6）：ref 标注 unsolvable 时，评的是「是否正确拒绝」
    if ref_agg == "unsolvable":
        out["unsolvable_recognized"] = pred_agg == "unsolvable"
        out["subtask_recall"] = None
        out["subtask_precision"] = None
        out["redundancy_rate"] = None
        out["dependency_precision"] = None
        out["dependency_recall"] = None
        return out
    out["unsolvable_recognized"] = None

    matched_pred, used_ref, used_pred = align_nodes(ref_nodes, pred_nodes, tau)
    n_ref, n_pred = len(ref_nodes), len(pred_nodes)
    recall = len(used_ref) / n_ref if n_ref else None
    precision = len(used_pred) / n_pred if n_pred else None
    redundancy = (n_pred - len(used_pred)) / n_pred if n_pred else None
    out["subtask_recall"] = recall
    out["subtask_precision"] = precision
    out["redundancy_rate"] = redundancy

    # 依赖边 P/R（§九 9.4）：pred 边 (a,b) → 对齐后 (ref_a, ref_b)；resource 边不参与
    ref_edges = {
        (e["from"], e["to"])
        for e in reference.get("edges") or []
        if e.get("type") in ("data", "control") and isinstance(e.get("from"), str) and isinstance(e.get("to"), str)
    }
    pred_edges_raw = [
        (e["from"], e["to"])
        for e in predicted.get("edges") or []
        if e.get("type") in ("data", "control") and isinstance(e.get("from"), str) and isinstance(e.get("to"), str)
    ]
    mapped = {(matched_pred.get(a), matched_pred.get(b)) for a, b in pred_edges_raw}
    mapped = {(a, b) for a, b in mapped if a is not None and b is not None}
    if ref_edges:
        out["dependency_precision"] = (len(mapped & ref_edges) / len(mapped)) if mapped else 0.0
        out["dependency_recall"] = (len(mapped & ref_edges) / len(ref_edges)) if ref_edges else None
    else:
        # reference 无边（planning-obligation 形态）：任何 pred 边都是 spurious 依赖
        out["dependency_precision"] = 0.0 if mapped else None
        out["dependency_recall"] = None
    out["n_spurious_edges"] = len(mapped - ref_edges) if ref_edges else len(mapped)
    return out


def evaluate_batch(pairs: list[tuple[str, dict, dict]], tau: float = DEFAULT_TAU) -> dict:
    """批量评测 → 汇总指标（invalid plan rate / 均值 / 不可解计数）。

    pairs: (sample_id, reference, predicted)
    """
    per_case = []
    invalid = 0
    unsolvable_total = 0
    unsolvable_correct = 0
    recalls: list[float] = []
    precisions: list[float] = []
    dep_p: list[float] = []
    dep_r: list[float] = []
    for sid, ref, pred in pairs:
        if validate_workflow(pred):
            invalid += 1  # predicted 不是合法 canonical workflow → Invalid Plan（§九 9.5）
            per_case.append({"sample_id": sid, "invalid_plan": True})
            continue
        r = evaluate(ref, pred, tau)
        r["sample_id"] = sid
        r["invalid_plan"] = False
        per_case.append(r)
        if r["unsolvable_recognized"] is not None:
            unsolvable_total += 1
            unsolvable_correct += 1 if r["unsolvable_recognized"] else 0
            continue
        if r["subtask_recall"] is not None:
            recalls.append(r["subtask_recall"])
        if r["subtask_precision"] is not None:
            precisions.append(r["subtask_precision"])
        if r["dependency_precision"] is not None:
            dep_p.append(r["dependency_precision"])
        if r["dependency_recall"] is not None:
            dep_r.append(r["dependency_recall"])

    def mean(xs: list[float]) -> float | None:
        return sum(xs) / len(xs) if xs else None

    return {
        "n_cases": len(pairs),
        "n_invalid_plan": invalid,
        "invalid_plan_rate": invalid / len(pairs) if pairs else None,
        "subtask_recall_mean": mean(recalls),
        "subtask_precision_mean": mean(precisions),
        "dependency_precision_mean": mean(dep_p),
        "dependency_recall_mean": mean(dep_r),
        "n_unsolvable": unsolvable_total,
        "unsolvable_recognition_rate": (unsolvable_correct / unsolvable_total) if unsolvable_total else None,
        "per_case": per_case,
    }


def _load_fixture(name: str) -> dict:
    return json.loads((HERE.parent / "manifests" / "fixtures" / name).read_text(encoding="utf-8"))


def self_check() -> int:
    """自评：fixture 与 reference 的确定性对照（全 1.0 / 预期缺失场景）。"""
    ok = True
    for name in ("fixture_chain.json", "fixture_fanout.json", "fixture_diamond.json"):
        wf = _load_fixture(name)
        r = evaluate(wf, wf)
        good = (
            r["subtask_recall"] == 1.0
            and r["subtask_precision"] == 1.0
            and r["redundancy_rate"] == 0.0
            and r["dependency_precision"] == 1.0
            and r["dependency_recall"] == 1.0
        )
        print(("PASS " if good else "FAIL ") + f"self {name}: {r}")
        ok = ok and good

    # 缺 1 节点 → recall=3/4；多 1 个无关节点 → precision=3/4
    chain = _load_fixture("fixture_chain.json")
    missing = json.loads(json.dumps(chain))
    missing["nodes"] = missing["nodes"][:2]
    missing["edges"] = []
    r = evaluate(chain, missing)
    good = abs(r["subtask_recall"] - 2 / 3) < 1e-9 and abs(r["subtask_precision"] - 1.0) < 1e-9
    print(("PASS " if good else "FAIL ") + f"missing-node: recall={r['subtask_recall']}")
    ok = ok and good

    extra = json.loads(json.dumps(chain))
    extra["nodes"].append({"id": "tx", "description": "顺便写一首诗", "task_type": "general", "depends_on": []})
    r = evaluate(chain, extra)
    good = abs(r["subtask_precision"] - 3 / 4) < 1e-9 and abs(r["redundancy_rate"] - 1 / 4) < 1e-9
    print(("PASS " if good else "FAIL ") + f"spurious-node: precision={r['subtask_precision']} redundancy={r['redundancy_rate']}")
    ok = ok and good

    # 错依赖：t2→t3 边改为 t1→t3（对齐自洽时 edge precision 应 0.5）
    wrong = json.loads(json.dumps(chain))
    wrong["edges"][1] = {"from": "t1", "to": "t3", "type": "data"}
    wrong["nodes"][2]["depends_on"] = ["t1"]
    r = evaluate(chain, wrong)
    good = abs(r["dependency_precision"] - 0.5) < 1e-9 and abs(r["dependency_recall"] - 0.5) < 1e-9
    print(("PASS " if good else "FAIL ") + f"wrong-dependency: P={r['dependency_precision']} R={r['dependency_recall']}")
    ok = ok and good

    # 不可解识别
    uns = _load_fixture("fixture_unsolvable.json")
    r = evaluate(uns, json.loads(json.dumps(uns)))
    good = r["unsolvable_recognized"] is True
    print(("PASS " if good else "FAIL ") + "unsolvable-recognized")
    ok = ok and good
    pred_plan = json.loads(json.dumps(chain))
    r = evaluate(uns, pred_plan)
    good = r["unsolvable_recognized"] is False
    print(("PASS " if good else "FAIL ") + "unsolvable-missed")
    ok = ok and good

    return 0 if ok else 1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--self-check", action="store_true")
    args = ap.parse_args()
    if args.self_check:
        return self_check()
    ap.print_help()
    return 2


if __name__ == "__main__":
    sys.exit(main())
