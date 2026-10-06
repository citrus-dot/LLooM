#!/usr/bin/env python3
"""db_init.py — OrchestrationBench 独立库 DDL（方案 §十八，O-Day 9）。

红线：独立库 benchmarks/orchestration/orchbench.db，生产 data/lloom.db 零接触。
表结构照 16-OrchestrationBench §十八 DDL 原文；幂等（IF NOT EXISTS，重跑无副作用）。

用法：python scripts/db_init.py [--db benchmarks/orchestration/orchbench.db]
"""

from __future__ import annotations

import argparse
import sqlite3
import sys
from pathlib import Path

DDL = """
CREATE TABLE IF NOT EXISTS bench_workflows (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  benchmark_id TEXT NOT NULL,
  sample_id TEXT NOT NULL,
  root_goal TEXT NOT NULL,
  workflow_json TEXT NOT NULL,
  reference_graph_json TEXT,
  verification_json TEXT,
  split TEXT NOT NULL,
  meta_json TEXT,
  UNIQUE(benchmark_id, sample_id)
);

CREATE TABLE IF NOT EXISTS bench_orchestration_runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  benchmark_id TEXT NOT NULL,
  manifest_hash TEXT NOT NULL,
  strategy TEXT NOT NULL,
  variant TEXT NOT NULL,
  parameter_json TEXT,
  created_at TEXT DEFAULT (datetime('now')),
  summary_json TEXT
);

CREATE TABLE IF NOT EXISTS bench_orchestration_items (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id INTEGER NOT NULL,
  sample_id TEXT NOT NULL,
  predicted_workflow_json TEXT,
  execution_trace_json TEXT,
  task_success INTEGER NOT NULL,
  planning_score REAL,
  scheduling_score REAL,
  execution_score REAL,
  cost REAL,
  makespan_ms REAL,
  primary_error TEXT,
  secondary_errors_json TEXT,
  decision_json TEXT,
  UNIQUE(run_id, sample_id)
);

CREATE INDEX IF NOT EXISTS idx_boi_run ON bench_orchestration_items(run_id);
CREATE INDEX IF NOT EXISTS idx_bw_bench ON bench_workflows(benchmark_id, split);
"""


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--db", default=str(Path(__file__).resolve().parent.parent / "orchbench.db"))
    args = ap.parse_args()
    db_path = Path(args.db)
    if args.db.endswith("lloom.db") or "data/" in db_path.parts:
        raise SystemExit("✗ 拒绝：orchbench DDL 只允许独立库，禁止指向 data/lloom.db")
    db_path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(db_path)
    try:
        conn.executescript("PRAGMA journal_mode=WAL;" + DDL)
        conn.commit()
        tables = [r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")]
        print(f"✓ orchbench.db 就绪: {db_path}")
        print(f"  tables: {tables}")
    finally:
        conn.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
