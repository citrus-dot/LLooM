#!/usr/bin/env python3
"""Check whether the benchmark checkout is usable.

This command deliberately has no third-party dependencies and never makes a
network request.  Replay is expected to work without the external raw-data
disk; live collection is not.

Examples (from the repository root):
    python3 benchmarks/tools/doctor.py
    python3 benchmarks/tools/doctor.py --run-replay
    python3 benchmarks/tools/doctor.py --mode live --strict
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
REPLAY = ROOT / "benchmarks/showcase/audit/replay_check.py"
CONTRACT = ROOT / "benchmarks/benchmark_contract.json"
SHOWCASE_MANIFEST = ROOT / "benchmarks/showcase/manifests/showcase_manifest.json"


class Doctor:
    def __init__(self, mode: str, strict: bool) -> None:
        self.mode = mode
        self.strict = strict
        self.failures: list[str] = []
        self.warnings: list[str] = []

    def ok(self, message: str) -> None:
        print(f"PASS  {message}")

    def warn(self, message: str) -> None:
        self.warnings.append(message)
        print(f"WARN  {message}")

    def fail(self, message: str) -> None:
        self.failures.append(message)
        print(f"FAIL  {message}")

    def required_file(self, relative: str) -> None:
        path = ROOT / relative
        if path.is_file():
            self.ok(relative)
        else:
            self.fail(f"missing file: {relative}")

    def check_symlink(self, relative: str, required_for_live: bool = True) -> None:
        path = ROOT / relative
        if not path.is_symlink():
            if path.exists():
                self.ok(f"{relative} (materialized)")
            elif self.mode == "live" and required_for_live:
                self.fail(f"missing live data: {relative}")
            else:
                self.warn(f"external data unavailable (replay is still usable): {relative}")
            return
        if path.exists():
            self.ok(f"{relative} -> {os.readlink(path)}")
        elif self.mode == "live" and required_for_live:
            self.fail(f"broken symlink: {relative} -> {os.readlink(path)}")
        else:
            self.warn(f"broken external symlink (allowed for replay): {relative} -> {os.readlink(path)}")

    def run(self, argv: list[str]) -> bool:
        print("RUN   " + " ".join(argv))
        result = subprocess.run(argv, cwd=ROOT, check=False)
        if result.returncode == 0:
            self.ok("replay check")
            return True
        self.fail(f"replay check exited with {result.returncode}")
        return False

    def check(self, run_replay: bool) -> int:
        print(f"Benchmark doctor: mode={self.mode}, root={ROOT}")
        if sys.version_info < (3, 9):
            self.fail(f"Python >= 3.9 required, found {sys.version.split()[0]}")
        else:
            self.ok(f"Python {sys.version.split()[0]}")

        self.required_file("benchmarks/showcase/audit/replay_check.py")
        self.required_file("benchmarks/showcase/manifests/showcase_manifest.json")
        self.required_file("benchmarks/routerbench/reports/v02_test_report.json")
        self.required_file("benchmarks/systembench/reports/v01_report.json")
        self.required_file("benchmarks/systembench/matrices/semantic_outcomes_l1.jsonl")

        try:
            contract = json.loads(CONTRACT.read_text(encoding="utf-8"))
            showcase = json.loads(SHOWCASE_MANIFEST.read_text(encoding="utf-8"))
            expected_status = "showcase_snapshot_not_formal_v02_release"
            if contract.get("status") == expected_status:
                self.ok("benchmark contract status is non-release snapshot")
            else:
                self.fail("benchmark contract must explicitly remain non-release")
            if showcase.get("status") == contract.get("status"):
                self.ok("contract status matches showcase manifest")
            else:
                self.fail("contract status does not match showcase manifest")
            if (contract.get("snapshots", {}).get("routerbench", {}).get("id")
                    == showcase.get("routerbench_id")):
                self.ok("RouterBench snapshot identity matches")
            else:
                self.fail("RouterBench snapshot identity does not match")
            if (contract.get("snapshots", {}).get("systembench", {}).get("id")
                    == showcase.get("systembench_v01_id")):
                self.ok("SystemBench snapshot identity matches")
            else:
                self.fail("SystemBench snapshot identity does not match")
        except (OSError, json.JSONDecodeError) as exc:
            self.fail(f"invalid benchmark contract: {exc}")

        # These are intentionally warnings in replay mode.  A clone without
        # the external disk must still be able to verify the frozen showcase.
        self.check_symlink("benchmarks/routerbench/raw")
        self.check_symlink("benchmarks/routerbench/bench.db")
        self.check_symlink("benchmarks/routerbench/normalized")

        if self.mode == "live":
            if os.environ.get("DASHSCOPE_API_KEY") or os.environ.get("OPENAI_API_KEY"):
                self.ok("provider API key is present")
            else:
                self.fail("live mode requires DASHSCOPE_API_KEY or OPENAI_API_KEY")

        if run_replay:
            self.run([sys.executable, str(REPLAY)])

        if self.failures:
            return 1
        if self.strict and self.warnings:
            print(f"{len(self.warnings)} warning(s) treated as failures (--strict)")
            return 1
        print(f"Doctor complete: PASS ({len(self.warnings)} warning(s))")
        return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=("replay", "live"), default="replay")
    parser.add_argument("--run-replay", action="store_true", help="run the offline 31-check replay")
    parser.add_argument("--strict", action="store_true", help="treat missing external data warnings as failures")
    args = parser.parse_args()
    return Doctor(args.mode, args.strict).check(args.run_replay)


if __name__ == "__main__":
    raise SystemExit(main())
