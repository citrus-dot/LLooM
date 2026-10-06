"""tests/test_report.py — O-Day 14 case-level attribution（P1-P11）单测。"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from diagnose import run_recovery  # noqa: E402
from report import attribute_case  # noqa: E402


def planning(recall=1.0, precision=1.0, redundancy=0.0, dep_p=1.0, dep_r=1.0):
    return {
        "subtask_recall": recall,
        "subtask_precision": precision,
        "redundancy_rate": redundancy,
        "dependency_precision": dep_p,
        "dependency_recall": dep_r,
    }


def scheduling(illegal=0.0, waste=0):
    return {"illegal_parallelism_rate": illegal, "sequentialization_waste_ms": waste}


def recovery(workflow_final="task_success", n_partial=0, f2_final="SUCCEEDED"):
    # f2_final 默认 SUCCEEDED = F2 注入但 fallback 成功（无 P8/P9/P7）
    fake_trace = {
        "t": {"failure": None, "final": "SUCCEEDED"},
        "f2node": {"failure": "F2_model_unavailable", "final": f2_final},
    }
    m = {"workflow_final": workflow_final, "n_partial_nodes": n_partial}
    return {"trace": fake_trace, "metrics": m}


class TestAttribution(unittest.TestCase):
    def test_clean_case_no_errors(self):
        a = attribute_case(planning(), scheduling(), recovery())
        self.assertIsNone(a["primary_error"])
        self.assertEqual(a["secondary_errors"], [])

    def test_planning_layer(self):
        a = attribute_case(planning(recall=2 / 3), scheduling(), None)
        self.assertEqual(a["primary_error"], "P1")
        a = attribute_case(planning(redundancy=0.25), scheduling(), None)
        self.assertEqual(a["primary_error"], "P2")
        a = attribute_case(planning(dep_p=0.5), scheduling(), None)
        self.assertEqual(a["primary_error"], "P3")
        # 漏边（recall<1, precision=1）也归 P3
        a = attribute_case(planning(dep_r=0.5), scheduling(), None)
        self.assertEqual(a["primary_error"], "P3")

    def test_scheduling_layer(self):
        a = attribute_case(planning(), scheduling(illegal=1.0), None)
        self.assertEqual(a["primary_error"], "P4")
        a = attribute_case(planning(), scheduling(waste=800), None)
        self.assertEqual(a["primary_error"], "P5")

    def test_recovery_layer_priority(self):
        # P8/P9/P7 由 trace 驱动；P11 只做兜底 primary
        a = attribute_case(None, None, recovery(workflow_final="partial_success", f2_final="FAILED_FINAL"))
        self.assertEqual(a["primary_error"], "P8")
        self.assertIn("P11", a["secondary_errors"])

    def test_earliest_layer_wins_primary(self):
        # planning P1 + recovery P7 → primary=P1（O1 最早）
        a = attribute_case(planning(recall=0.5), scheduling(), recovery(f2_final="FAILED_FINAL"))
        self.assertEqual(a["primary_error"], "P1")
        self.assertIn("P7", a["secondary_errors"])


class TestRecoveryIntegration(unittest.TestCase):
    def test_real_trace_attribution(self):
        import json

        wf = json.loads((HERE.parent / "manifests" / "fixtures" / "fixture_fanout.json").read_text(encoding="utf-8"))
        r = run_recovery(wf, {"t3": "F2_model_unavailable"}, {"fallback_model": False})
        a = attribute_case(None, None, r)
        self.assertEqual(a["primary_error"], "P8")
        self.assertIn("P9", a["secondary_errors"])
        self.assertIn("P7", a["secondary_errors"])
        # 恢复成功时无 execution 归因
        r2 = run_recovery(wf, {"t1": "F1_timeout"})
        a2 = attribute_case(None, None, r2)
        self.assertIsNone(a2["primary_error"])


if __name__ == "__main__":
    unittest.main()
