"""tests/test_router_bridge.py — O-Day 12 Router bridge 单测（接口链路 + 决策真源边界）。"""

from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from router_bridge import (  # noqa: E402
    assign_fixed_tier,
    assign_from_decisions,
    assignment_report,
    forward_queries,
    node_id_from_sample_id,
    sample_id_of,
)
from validate import validate_workflow  # noqa: E402


def load(name: str) -> dict:
    return json.loads((HERE.parent / "manifests" / "fixtures" / name).read_text(encoding="utf-8"))


class TestForward(unittest.TestCase):
    def setUp(self):
        self.wf = load("fixture_fanout.json")

    def test_sample_id_contract(self):
        self.assertEqual(sample_id_of("wf_a", "t1"), "obx__wf_a__t1")
        self.assertEqual(node_id_from_sample_id("obx__wf_a__t1"), ("wf_a", "t1"))
        self.assertIsNone(node_id_from_sample_id("other__wf_a__t1"))
        self.assertIsNone(node_id_from_sample_id("obx__only_two"))

    def test_forward_shape(self):
        queries = forward_queries(self.wf)
        self.assertEqual(len(queries), 4)
        self.assertEqual({q["split"] for q in queries}, {"test"})
        self.assertEqual([q["task_type"] for q in queries], [n["task_type"] for n in self.wf["nodes"]])
        self.assertIn(self.wf["root_goal"], queries[0]["prompt"])
        self.assertIn(self.wf["nodes"][0]["description"], queries[0]["prompt"])


class TestAssign(unittest.TestCase):
    def setUp(self):
        self.wf = load("fixture_chain.json")

    def test_fixed_tier_baselines(self):
        pool = {"weak": "w", "mid": "m", "strong": "s"}
        for tier, expect_model in (("weak", "w"), ("mid", "m"), ("strong", "s")):
            a = assign_fixed_tier(self.wf, tier, pool)
            self.assertTrue(all(n["model"] == expect_model for n in a["nodes"]))
        with self.assertRaises(ValueError):
            assign_fixed_tier(self.wf, "ultra", pool)
        # 输入不被修改
        self.assertTrue(all(n.get("model") is None for n in self.wf["nodes"]))

    def test_decisions_backfill(self):
        queries = forward_queries(self.wf)
        full = {q["sample_id"]: "strong_m" for q in queries}
        a, r = assign_from_decisions(self.wf, full)
        self.assertEqual(r["n_unmatched"], 0)
        self.assertEqual(validate_workflow(a), [])
        partial = dict(list(full.items())[:1])
        a2, r2 = assign_from_decisions(self.wf, partial)
        self.assertEqual(r2["n_unmatched"], 2)
        self.assertEqual(a2["nodes"][1]["model"], None)


class TestReport(unittest.TestCase):
    def test_report_pure_aggregation(self):
        wf = load("fixture_fanout.json")
        pool = {"weak": "weak_m", "mid": "mid_m", "strong": "strong_m"}
        a = assign_fixed_tier(wf, "mid", pool)
        lookup = lambda s, m: (0.5, 0.002)  # noqa: E731 确定性注入
        rep = assignment_report(a, lookup, lambda s: 0.9)
        self.assertEqual(rep["n_missing_outcome"], 0)
        self.assertAlmostEqual(rep["global_workflow_cost"], 0.008)
        self.assertAlmostEqual(rep["mean_routing_regret"], 0.4)
        # 无 oracle → regret 恒 None
        rep2 = assignment_report(a, lookup)
        self.assertIsNone(rep2["mean_routing_regret"])
        # 缺 outcome 计数
        rep3 = assignment_report(a, lambda s, m: None)
        self.assertEqual(rep3["n_missing_outcome"], 4)


if __name__ == "__main__":
    unittest.main()
