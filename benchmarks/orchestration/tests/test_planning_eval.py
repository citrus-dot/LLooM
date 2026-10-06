"""tests/test_planning_eval.py — O-Day 10 planning evaluator 单测（自评场景固化）。"""

from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from adapters.planning_eval import evaluate, evaluate_batch  # noqa: E402
from adapters.planningbench import canonicalize, parse_checklist  # noqa: E402
from adapters.planbench import canonicalize as pb_canonicalize  # noqa: E402
from adapters.planbench import parse_pddl  # noqa: E402


def load_fixture(name: str) -> dict:
    return json.loads((HERE.parent / "manifests" / "fixtures" / name).read_text(encoding="utf-8"))


class TestPlanningEval(unittest.TestCase):
    def test_self_consistent(self):
        for name in ("fixture_chain.json", "fixture_fanout.json", "fixture_diamond.json"):
            with self.subTest(fixture=name):
                wf = load_fixture(name)
                r = evaluate(wf, wf)
                self.assertEqual(r["subtask_recall"], 1.0)
                self.assertEqual(r["subtask_precision"], 1.0)
                self.assertEqual(r["redundancy_rate"], 0.0)
                self.assertEqual(r["dependency_precision"], 1.0)
                self.assertEqual(r["dependency_recall"], 1.0)

    def test_missing_and_spurious_nodes(self):
        chain = load_fixture("fixture_chain.json")
        missing = json.loads(json.dumps(chain))
        missing["nodes"] = missing["nodes"][:2]
        missing["edges"] = []
        r = evaluate(chain, missing)
        self.assertAlmostEqual(r["subtask_recall"], 2 / 3)

        extra = json.loads(json.dumps(chain))
        extra["nodes"].append({"id": "tx", "description": "顺便写一首诗", "task_type": "general", "depends_on": []})
        r = evaluate(chain, extra)
        self.assertAlmostEqual(r["subtask_precision"], 3 / 4)
        self.assertAlmostEqual(r["redundancy_rate"], 1 / 4)

    def test_wrong_dependency(self):
        chain = load_fixture("fixture_chain.json")
        wrong = json.loads(json.dumps(chain))
        wrong["edges"][1] = {"from": "t1", "to": "t3", "type": "data"}
        wrong["nodes"][2]["depends_on"] = ["t1"]
        r = evaluate(chain, wrong)
        self.assertAlmostEqual(r["dependency_precision"], 0.5)
        self.assertAlmostEqual(r["dependency_recall"], 0.5)

    def test_unsolvable_recognition(self):
        uns = load_fixture("fixture_unsolvable.json")
        self.assertIs(evaluate(uns, json.loads(json.dumps(uns)))["unsolvable_recognized"], True)
        pred_plan = load_fixture("fixture_chain.json")
        self.assertIs(evaluate(uns, pred_plan)["unsolvable_recognized"], False)

    def test_batch_invalid_plan_rate(self):
        chain = load_fixture("fixture_chain.json")
        broken = {"workflow_id": "x", "schema_version": 1}  # 缺字段
        batch = evaluate_batch([("a", chain, chain), ("b", chain, broken)])
        self.assertEqual(batch["n_invalid_plan"], 1)
        self.assertAlmostEqual(batch["invalid_plan_rate"], 0.5)


class TestPlanningBenchAdapter(unittest.TestCase):
    def test_checklist_line_start_anchor(self):
        # 行内数字列举（「N1取值25、N2取值55」）不得误拆；换行编号项必须拆
        ck = "1、模型的回答中，N1取值25、N2取值55；S0取值15。\n2、模型的回答必须包含 T2。\n3、模型必须说明理由。"
        items = parse_checklist(ck)
        self.assertEqual(len(items), 3)
        self.assertIn("N1取值25、N2取值55", items[0])

    def test_canonicalize_shape(self):
        row = {
            "idx": 42,
            "messages": [{"role": "user", "content": "规划任务题面"}],
            "checklist": "1、第一项。\n2、第二项。",
        }
        wf = canonicalize(row)
        self.assertEqual(wf["workflow_id"], "ob_pb_0042")
        self.assertEqual(wf["aggregation"]["type"], "none")
        self.assertEqual(len(wf["nodes"]), 2)
        self.assertEqual(wf["meta"]["node_semantics"], "checklist_obligation")


class TestPlanBenchAdapter(unittest.TestCase):
    BLOCKSWORLD = """(define (problem BW-test)
(:domain blocksworld-4ops)(:objects j f i g)(:init (handempty)(ontable j)(ontable f)(clear j)(clear f))
(:goal (and (on j f) (on f i))))"""

    def test_parse_and_dependency(self):
        parsed = parse_pddl(self.BLOCKSWORLD)
        self.assertEqual(parsed["problem"], "BW-test")
        self.assertEqual(parsed["goal_atoms"], [("on", ["j", "f"]), ("on", ["f", "i"])])
        wf = pb_canonicalize("blocksworld", "generated", "instance-1", parsed)
        self.assertEqual(wf["workflow_id"], "ob_planbench_blocksworld_generated_instance-1")
        # on j f 依赖 on f i（Y=f 的最终位置）
        self.assertEqual(wf["nodes"][0]["depends_on"], ["on_f_i"])
        self.assertEqual(wf["meta"]["node_semantics"], "goal_stack_dependency")

    def test_flat_domain_zero_dependency(self):
        parsed = parse_pddl("""(define (problem LG-t)
(:domain logistics-strips)(:objects p1 p2 l1 l2)
(:init (OBJ p1)(OBJ p2)(at p1 l1)(at p2 l2))
(:goal (and (at p1 l2) (at p2 l1))))""")
        wf = pb_canonicalize("logistics", "generated", "instance-9", parsed)
        self.assertEqual(len(wf["nodes"]), 2)
        self.assertTrue(all(n["depends_on"] == [] for n in wf["nodes"]))
        self.assertEqual(wf["edges"], [])


if __name__ == "__main__":
    unittest.main()
