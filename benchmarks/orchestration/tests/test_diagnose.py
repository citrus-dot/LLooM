"""tests/test_diagnose.py — O-Day 13 failure injection + recovery 状态机单测。"""

from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from diagnose import inject_by_hash, run_recovery  # noqa: E402


def load(name: str) -> dict:
    return json.loads((HERE.parent / "manifests" / "fixtures" / name).read_text(encoding="utf-8"))


class TestRecovery(unittest.TestCase):
    def setUp(self):
        self.wf = load("fixture_fanout.json")

    def test_no_failure_task_success(self):
        r = run_recovery(self.wf, {})
        self.assertEqual(r["metrics"]["workflow_final"], "task_success")
        self.assertIsNone(r["metrics"]["recovery_rate"])

    def test_f1_retry(self):
        r = run_recovery(self.wf, {"t1": "F1_timeout"})
        t = r["trace"]["t1"]
        self.assertEqual(t["final"], "SUCCEEDED_RECOVERED")
        self.assertEqual(t["attempts"], 2)
        self.assertEqual(t["effective_duration_ms"], 2 * self.wf["nodes"][0]["duration_ms"])

    def test_f2_fallback_switch(self):
        r_off = run_recovery(self.wf, {"t3": "F2_model_unavailable"}, {"fallback_model": False})
        self.assertEqual(r_off["trace"]["t3"]["final"], "FAILED_FINAL")
        self.assertEqual(r_off["metrics"]["workflow_final"], "partial_success")
        r_on = run_recovery(self.wf, {"t3": "F2_model_unavailable"})
        self.assertEqual(r_on["trace"]["t3"]["final"], "SUCCEEDED_RECOVERED")
        self.assertEqual(r_on["metrics"]["fallback_success_rate"], 1.0)

    def test_f4_dependency_propagation(self):
        r = run_recovery(self.wf, {"t1": "F1_timeout", "t3": "F4_missing_dependency"})
        self.assertEqual(r["trace"]["t3"]["final"], "SUCCEEDED_RECOVERED")
        r2 = run_recovery(
            self.wf,
            {"t1": "F1_timeout", "t2": "F1_timeout", "t3": "F4_missing_dependency"},
            {"max_retries": 0},
        )
        self.assertEqual(r2["trace"]["t3"]["final"], "FAILED_FINAL")

    def test_f5_isolation_within_wave(self):
        r = run_recovery(self.wf, {"t2": "F5_parallel_branch_failure"})
        self.assertEqual(r["trace"]["t1"]["final"], "SUCCEEDED")
        self.assertEqual(r["trace"]["t2"]["final"], "SUCCEEDED_RECOVERED")

    def test_f6_partial_aggregation(self):
        r = run_recovery(self.wf, {"t4": "F6_partial_aggregation"})
        self.assertEqual(r["trace"]["t4"]["final"], "PARTIAL")
        self.assertEqual(r["metrics"]["workflow_final"], "partial_success")

    def test_f7_budget_abort(self):
        r = run_recovery(self.wf, {"t4": "F7_budget_exhausted"})
        self.assertEqual(r["trace"]["t4"]["final"], "ABORTED")
        self.assertTrue(r["metrics"]["budget_aborted"])
        self.assertEqual(r["trace"]["t1"]["final"], "SUCCEEDED")

    def test_determinism_hash_injection(self):
        inj = inject_by_hash(self.wf, 0.5, seed=3407)
        a = json.dumps(run_recovery(self.wf, inj), sort_keys=True)
        b = json.dumps(run_recovery(self.wf, inject_by_hash(self.wf, 0.5, seed=3407)), sort_keys=True)
        self.assertEqual(a, b)

    def test_hash_injection_reproducible(self):
        self.assertEqual(inject_by_hash(self.wf, 0.5), inject_by_hash(self.wf, 0.5))
        self.assertNotEqual(inject_by_hash(self.wf, 0.5), inject_by_hash(self.wf, 0.5, seed=3408))


if __name__ == "__main__":
    unittest.main()
