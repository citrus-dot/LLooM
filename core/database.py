"""SQLite database layer — schema, CRUD, and query helpers."""

import sqlite3
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
    conn = sqlite3.connect(str(get_db_path()))
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA journal_mode=WAL")
    return conn


def init_db() -> None:
    conn = get_connection()
    conn.executescript(SCHEMA)
    conn.close()


# ── Model CRUD ──

def insert_model(data: dict) -> int | None:
    conn = get_connection()
    try:
        cur = conn.execute(
            """INSERT INTO models (name, provider, litellm_model, api_base, api_key_env,
               task_type, input_cost_per_token, output_cost_per_token, rpm, is_active)
               VALUES (:name, :provider, :litellm_model, :api_base, :api_key_env,
               :task_type, :input_cost_per_token, :output_cost_per_token, :rpm, 1)""",
            data,
        )
        conn.commit()
        return cur.lastrowid
    except sqlite3.IntegrityError:
        return None
    finally:
        conn.close()


def get_model(name: str) -> dict | None:
    conn = get_connection()
    row = conn.execute("SELECT * FROM models WHERE name = ?", (name,)).fetchone()
    conn.close()
    return dict(row) if row else None


def list_models(active_only: bool = True) -> list[dict]:
    conn = get_connection()
    if active_only:
        rows = conn.execute("SELECT * FROM models WHERE is_active = 1 ORDER BY name").fetchall()
    else:
        rows = conn.execute("SELECT * FROM models ORDER BY name").fetchall()
    conn.close()
    return [dict(r) for r in rows]


def update_model(name: str, updates: dict) -> bool:
    conn = get_connection()
    set_clauses = ", ".join(f"{k} = ?" for k in updates)
    values = list(updates.values()) + [name]
    cur = conn.execute(f"UPDATE models SET {set_clauses} WHERE name = ?", values)
    conn.commit()
    conn.close()
    return cur.rowcount > 0


def delete_model(name: str) -> bool:
    conn = get_connection()
    cur = conn.execute("UPDATE models SET is_active = 0 WHERE name = ?", (name,))
    conn.commit()
    conn.close()
    return cur.rowcount > 0


# ── Usage CRUD ──

def insert_usage(model_name: str, input_tokens: int, output_tokens: int,
                 cost: float, user_id: str = "default",
                 task_type: str | None = None, cache_hit: bool = False) -> int:
    conn = get_connection()
    cur = conn.execute(
        """INSERT INTO usage_records
           (model_name, user_id, input_tokens, output_tokens, cost, task_type, cache_hit)
           VALUES (?, ?, ?, ?, ?, ?, ?)""",
        (model_name, user_id, input_tokens, output_tokens, cost,
         task_type, int(cache_hit)),
    )
    conn.commit()
    conn.close()
    return cur.lastrowid


def get_usage_stats(model_name: str | None = None, user_id: str | None = None,
                    since: str | None = None) -> list[dict]:
    query = """SELECT model_name,
                      SUM(input_tokens) as total_input_tokens,
                      SUM(output_tokens) as total_output_tokens,
                      SUM(cost) as total_cost,
                      COUNT(*) as request_count,
                      SUM(cache_hit) as cache_hits
               FROM usage_records WHERE 1=1"""
    params = []
    if model_name:
        query += " AND model_name = ?"
        params.append(model_name)
    if user_id:
        query += " AND user_id = ?"
        params.append(user_id)
    if since:
        query += " AND created_at >= ?"
        params.append(since)
    query += " GROUP BY model_name ORDER BY total_cost DESC"
    conn = get_connection()
    rows = conn.execute(query, params).fetchall()
    conn.close()
    return [dict(r) for r in rows]


def get_total_spend(
    user_id: str | None = None,
    model_name: str | None = None,
    since: str | None = None,
) -> float:
    query = "SELECT SUM(cost) as total FROM usage_records WHERE 1=1"
    params = []
    if user_id:
        query += " AND user_id = ?"
        params.append(user_id)
    if model_name:
        query += " AND model_name = ?"
        params.append(model_name)
    if since:
        query += " AND created_at >= ?"
        params.append(since)
    conn = get_connection()
    row = conn.execute(query, params).fetchone()
    conn.close()
    return row["total"] or 0.0


# ── Budget CRUD ──

def upsert_budget(scope: str, scope_id: str, max_budget: float, duration: str) -> None:
    conn = get_connection()
    conn.execute(
        """INSERT INTO budgets (scope, scope_id, max_budget, duration)
           VALUES (?, ?, ?, ?)
           ON CONFLICT(scope, scope_id)
           DO UPDATE SET max_budget = excluded.max_budget, duration = excluded.duration""",
        (scope, scope_id, max_budget, duration),
    )
    conn.commit()
    conn.close()


def get_budget(scope: str, scope_id: str) -> dict | None:
    conn = get_connection()
    row = conn.execute(
        "SELECT * FROM budgets WHERE scope = ? AND scope_id = ?",
        (scope, scope_id),
    ).fetchone()
    conn.close()
    return dict(row) if row else None


def list_budgets() -> list[dict]:
    conn = get_connection()
    rows = conn.execute("SELECT * FROM budgets ORDER BY scope, scope_id").fetchall()
    conn.close()
    return [dict(r) for r in rows]
