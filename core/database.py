"""SQLite database layer — schema definitions and CRUD operations."""

import sqlite3
from pathlib import Path
from core.config import get_db_path

SCHEMA = """
CREATE TABLE IF NOT EXISTS models (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT UNIQUE NOT NULL,
    provider TEXT NOT NULL,
    litellm_model TEXT NOT NULL,
    api_base TEXT,
    api_key_env TEXT,
    task_type TEXT,
    input_cost_per_token REAL DEFAULT 0,
    output_cost_per_token REAL DEFAULT 0,
    rpm INTEGER DEFAULT 60,
    is_active INTEGER DEFAULT 1,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS usage_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    model_name TEXT NOT NULL,
    user_id TEXT DEFAULT 'default',
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cost REAL NOT NULL,
    task_type TEXT,
    cache_hit INTEGER DEFAULT 0,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS budgets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scope TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    max_budget REAL NOT NULL,
    duration TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(scope, scope_id)
);

CREATE INDEX IF NOT EXISTS idx_usage_model ON usage_records(model_name);
CREATE INDEX IF NOT EXISTS idx_usage_created ON usage_records(created_at);
CREATE INDEX IF NOT EXISTS idx_usage_user ON usage_records(user_id);
"""

def get_connection() -> sqlite3.Connection:
    db_path = get_db_path()
    conn = sqlite3.connect(str(db_path))
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA foreign_keys=ON")
    return conn

def init_db() -> None:
    """Initialize database schema."""
    conn = get_connection()
    conn.executescript(SCHEMA)
    conn.close()

if __name__ == "__main__":
    init_db()
    print(f"Database initialized at {get_db_path()}")
