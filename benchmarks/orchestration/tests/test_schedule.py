"""tests/test_schedule.py — O-Day 11 调度模拟器单测（方案 §十 口径固化）。"""

from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from schedule import evaluate_schedule, reference_waves, simulate_events  # noqa: E402


def load(name: str) -> dict:
    return json.loads((HERE.parent / "manifests" / "fixtures" / name).read_text(encoding="utf-8"))


class TestReferenceWaves(unittest.TestCase):
    def test_diamond_layering(self):
        wf = load("fixture_diamond.json")
        self.assertEqual(reference_waves(wf), [["t1"], ["t2", "t3"], ["t4"]])

    def test_chain_fully_serial(self):
        wf = load("fixture_chain.json")
        self.assertEqual(reference_waves(wf), [["t1"], ["t2"], ["t3"]])

    def test_fanout_first_wave_two_nodes(self):
        wf = load("fixture_fanout.json")
        self.assertEqual(reference_waves(wf), [["t1", "t2"], ["t3"], ["t4"]])


class TestMetrics(unittest.TestCase):
    def test_diamond_reference(self):
        # t1=1000 / t2=800 / t3=1200 / t4=1000：ideal=critical path t1→t3→t4=3200
        r = evaluate_schedule(load("fixture_diamond.json"), "reference")
        self.assertEqual(r["ideal_legal_makespan_ms"], 3200)
        self.assertEqual(r["actual_makespan_ms"], 3200)
        self.assertEqual(r["serial_baseline_ms"], 4000)
        self.assertEqual(r["parallelization_utilization"], 1.0)
        self.assertEqual(r["sequentialization_waste_ms"], 0)
        self.assertEqual(r["illegal_parallelism_rate"], 0.0)
        self.assertEqual(r["critical_path_nodes"], ["t1", "t3", "t4"])

    def test_fanout_greedy_concurrency(self):
        wf = load("fixture_fanout.json")
        r1 = evaluate_schedule(wf, "greedy_ready_queue", max_concurrency=1)
        self.assertEqual(r1["actual_makespan_ms"], 4200)  # 退化串行
        self.assertEqual(r1["parallelization_utilization"], 0.0)
        r2 = evaluate_schedule(wf, "greedy_ready_queue", max_concurrency=2)
        self.assertEqual(r2["actual_makespan_ms"], 3400)  # 达到 ideal

    def test_lloom_waves_consumes_precomputed_partition(self):
        wf = load("fixture_fanout.json")
        r = evaluate_schedule(wf, "lloom_waves")
        self.assertEqual(r["waves"], [["t1", "t2"], ["t3"], ["t4"]])

    def test_lloom_waves_null_falls_back(self):
        wf = load("fixture_diamond.json")  # waves=null
        r = evaluate_schedule(wf, "lloom_waves")
        self.assertEqual(r["waves"], [["t1"], ["t2", "t3"], ["t4"]])
        self.assertIn("退回 reference", r["note"])

    def test_illegal_parallelism_detected(self):
        r = evaluate_schedule(load("fixture_bad_waves.json"), "lloom_waves")
        self.assertEqual(r["illegal_parallelism_rate"], 1.0)
        self.assertIn(("t1", "t2"), r["illegal_parallel_pairs"])

    def test_determinism(self):
        wf = load("fixture_fanout.json")
        a = json.dumps(evaluate_schedule(wf, "greedy_ready_queue"), sort_keys=True)
        b = json.dumps(evaluate_schedule(wf, "greedy_ready_queue"), sort_keys=True)
        self.assertEqual(a, b)

    def test_event_schedule_dependency_order(self):
        wf = load("fixture_fanout.json")
        sim = simulate_events(wf, {n["id"]: n["duration_ms"] for n in wf["nodes"]}, None)
        s = sim["schedule"]
        self.assertGreaterEqual(s["t3"]["start_ms"], s["t2"]["end_ms"])
        self.assertGreaterEqual(s["t4"]["start_ms"], s["t3"]["end_ms"])
        # t1/t2 同波并行：start 相同
        self.assertEqual(s["t1"]["start_ms"], s["t2"]["start_ms"])


if __name__ == "__main__":
    unittest.main()
