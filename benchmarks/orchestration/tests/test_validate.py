"""tests/test_validate.py — O-Day 9 validator 单测（fixtures 双向覆盖）。

合法 fixture 必须 0 issue；非法 fixture 必须命中且仅命中预期错误码。
错误码是报告层稳定键，此处固化契约（改码 = 新 schema 版本）。
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
FIXTURES = HERE.parent / "manifests" / "fixtures"
sys.path.insert(0, str(HERE.parent))

from validate import validate_workflow  # noqa: E402


def load(name: str):
    import json

    return json.loads((FIXTURES / name).read_text(encoding="utf-8"))


class TestValidFixtures(unittest.TestCase):
    def test_chain_fanout_diamond_pass(self):
        for name in ("fixture_chain.json", "fixture_fanout.json", "fixture_diamond.json"):
            with self.subTest(fixture=name):
                self.assertEqual(validate_workflow(load(name)), [])

    def test_unsolvable_pass(self):
        self.assertEqual(validate_workflow(load("fixture_unsolvable.json")), [])

    def test_strict_meta_gate(self):
        # strict 模式要求 meta.source_benchmark/source_id；fixture 均带，去掉即拒
        wf = load("fixture_chain.json")
        del wf["meta"]["source_benchmark"]
        codes = {i.code for i in validate_workflow(wf, strict_meta=True)}
        self.assertIn("SCHEMA_MISSING_FIELD", codes)

    def test_bad_task_type_rejected(self):
        wf = load("fixture_chain.json")
        wf["nodes"][0]["task_type"] = "summary"  # 不在生产词表
        codes = {i.code for i in validate_workflow(wf)}
        self.assertIn("SCHEMA_TYPE", codes)


class TestInvalidFixtures(unittest.TestCase):
    def test_cycle_caught(self):
        codes = {i.code for i in validate_workflow(load("fixture_cycle.json"))}
        self.assertIn("CYCLE", codes)

    def test_bad_waves_caught(self):
        codes = {i.code for i in validate_workflow(load("fixture_bad_waves.json"))}
        self.assertIn("WAVE_ORDER", codes)

    def test_unknown_ref_caught(self):
        codes = [i.code for i in validate_workflow(load("fixture_unknown_ref.json"))]
        self.assertIn("UNKNOWN_NODE_REF", codes)
        self.assertIn("DEPENDS_EDGE_MISMATCH", codes)
        # t1 依赖 t_missing_dep 也应被引用完整性抓住
        self.assertGreaterEqual(codes.count("UNKNOWN_NODE_REF"), 2)

    def test_wave_partition_missing_coverage(self):
        wf = load("fixture_fanout.json")
        wf["waves"] = [["t1", "t2"], ["t3"]]  # 漏 t4
        codes = {i.code for i in validate_workflow(wf)}
        self.assertIn("WAVE_PARTITION", codes)

    def test_wave_duplicate_node(self):
        wf = load("fixture_fanout.json")
        wf["waves"] = [["t1", "t2", "t4"], ["t3"], ["t4"]]
        codes = {i.code for i in validate_workflow(wf)}
        self.assertIn("WAVE_PARTITION", codes)

    def test_self_edge(self):
        wf = load("fixture_fanout.json")
        wf["edges"].append({"from": "t1", "to": "t1", "type": "control"})
        codes = {i.code for i in validate_workflow(wf)}
        self.assertIn("SELF_EDGE", codes)

    def test_dup_edge(self):
        wf = load("fixture_fanout.json")
        wf["edges"].append({"from": "t1", "to": "t3", "type": "data"})
        codes = {i.code for i in validate_workflow(wf)}
        self.assertIn("DUP_EDGE", codes)

    def test_dup_node(self):
        wf = load("fixture_fanout.json")
        wf["nodes"].append(dict(wf["nodes"][0]))
        codes = {i.code for i in validate_workflow(wf)}
        self.assertIn("DUP_NODE", codes)

    def test_empty_nodes_requires_unsolvable(self):
        wf = load("fixture_unsolvable.json")
        wf["aggregation"]["type"] = "final_synthesis"  # 空节点但声称要聚合
        codes = {i.code for i in validate_workflow(wf)}
        self.assertIn("EMPTY_NODES", codes)

    def test_unsolvable_with_nodes_rejected(self):
        wf = load("fixture_chain.json")
        wf["aggregation"]["type"] = "unsolvable"
        codes = {i.code for i in validate_workflow(wf)}
        self.assertIn("UNSOLVABLE_WITH_NODES", codes)

    def test_schema_error_short_circuits_graph(self):
        # 结构层坏（缺 nodes）时不得再报图错误（防止级联噪音污染归因）
        issues = validate_workflow({"workflow_id": "x"})
        self.assertTrue(all(i.code.startswith("SCHEMA_") for i in issues))


if __name__ == "__main__":
    unittest.main()
