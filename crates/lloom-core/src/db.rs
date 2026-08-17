//! SQLite database layer — schema, CRUD, and query helpers.
//! Strongly-typed port of `core/database.py`.

use crate::error::{AppError, Result};
use crate::models::{Budget, Model, UsageStats};
use rusqlite::{params, Connection};

const SCHEMA: &str = r#"
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

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS cache_calibration (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    sim REAL NOT NULL,
    decision TEXT NOT NULL,
    model TEXT,
    label INTEGER,
    source TEXT
);

CREATE INDEX IF NOT EXISTS idx_cal_label ON cache_calibration(label);
"#;

pub fn init_db() -> Result<()> {
    let conn = open()?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch(SCHEMA)?;
    Ok(())
}

fn open() -> Result<Connection> {
    let conn = Connection::open(crate::config::db_path())?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    Ok(conn)
}

// ── Model CRUD ──

pub fn insert_model(m: &Model) -> Result<i64> {
    let conn = open()?;
    let res = conn.execute(
        "INSERT INTO models (name, provider, litellm_model, api_base, api_key_env, task_type,
                             input_cost_per_token, output_cost_per_token, rpm, is_active)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            m.name,
            m.provider,
            m.litellm_model,
            m.api_base,
            m.api_key_env,
            m.task_type,
            m.input_cost_per_token,
            m.output_cost_per_token,
            m.rpm,
            m.is_active,
        ],
    );
    match res {
        Ok(_) => Ok(conn.last_insert_rowid()),
        Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation => {
            Err(AppError::Conflict(m.name.clone()))
        }
        Err(e) => Err(e.into()),
    }
}

pub fn get_model(name: &str) -> Result<Model> {
    let conn = open()?;
    let mut stmt = conn.prepare("SELECT * FROM models WHERE name = ?1 AND is_active = 1")?;
    let row = stmt
        .query_row(params![name], model_from_row)
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => AppError::NotFound(name.to_string()),
            other => other.into(),
        })?;
    Ok(row)
}

pub fn list_models(active_only: bool) -> Result<Vec<Model>> {
    let conn = open()?;
    let q = if active_only {
        "SELECT * FROM models WHERE is_active = 1 ORDER BY name"
    } else {
        "SELECT * FROM models ORDER BY name"
    };
    let mut stmt = conn.prepare(q)?;
    let rows = stmt.query_map([], model_from_row)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn update_model(name: &str, updates: &serde_json::Map<String, serde_json::Value>) -> Result<bool> {
    if updates.is_empty() {
        return Ok(false);
    }
    // Whitelist of updatable columns. Any key outside this set is rejected to
    // prevent SQL injection — column names can't be parameterized, so an
    // attacker-controlled key like `name='x' WHERE 1=1--` would otherwise be
    // interpolated directly into the statement.
    const ALLOWED: &[&str] = &[
        "name",
        "provider",
        "litellm_model",
        "api_base",
        "api_key_env",
        "task_type",
        "input_cost_per_token",
        "output_cost_per_token",
        "rpm",
        "is_active",
    ];
    for k in updates.keys() {
        if !ALLOWED.contains(&k.as_str()) {
            return Err(AppError::InvalidRequest(format!(
                "unknown column '{k}'; allowed: {}",
                ALLOWED.join(", ")
            )));
        }
    }
    let conn = open()?;
    let mut sql = String::from("UPDATE models SET ");
    let mut vals: Vec<rusqlite::types::Value> = Vec::new();
    let mut first = true;
    for (k, v) in updates {
        if !first {
            sql.push_str(", ");
        }
        sql.push_str(&format!("{k} = ?"));
        first = false;
        vals.push(json_to_sqlite(v));
    }
    sql.push_str(" WHERE name = ?");
    vals.push(rusqlite::types::Value::Text(name.to_string()));
    let n = conn.execute(&sql, rusqlite::params_from_iter(vals.iter().cloned()))?;
    Ok(n > 0)
}

/// Soft-delete a model (is_active = 0).
pub fn delete_model(name: &str) -> Result<bool> {
    let conn = open()?;
    let n = conn.execute("UPDATE models SET is_active = 0 WHERE name = ?1", params![name])?;
    Ok(n > 0)
}

fn model_from_row(row: &rusqlite::Row) -> rusqlite::Result<Model> {
    Ok(Model {
        id: row.get("id")?,
        name: row.get("name")?,
        provider: row.get("provider")?,
        litellm_model: row.get("litellm_model")?,
        api_base: row.get::<_, Option<String>>("api_base")?.unwrap_or_default(),
        api_key_env: row.get::<_, Option<String>>("api_key_env")?.unwrap_or_default(),
        task_type: row.get::<_, Option<String>>("task_type")?.unwrap_or_default(),
        input_cost_per_token: row.get("input_cost_per_token")?,
        output_cost_per_token: row.get("output_cost_per_token")?,
        rpm: row.get("rpm")?,
        is_active: row.get("is_active")?,
    })
}

// ── Usage ──

#[allow(clippy::too_many_arguments)]
pub fn insert_usage(
    model_name: &str,
    user_id: &str,
    input_tokens: i64,
    output_tokens: i64,
    cost: f64,
    task_type: Option<&str>,
    cache_hit: bool,
) -> Result<i64> {
    let conn = open()?;
    conn.execute(
        "INSERT INTO usage_records (model_name, user_id, input_tokens, output_tokens, cost, task_type, cache_hit)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            model_name,
            user_id,
            input_tokens,
            output_tokens,
            cost,
            task_type,
            if cache_hit { 1 } else { 0 }
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_usage_stats(
    model_name: Option<&str>,
    user_id: Option<&str>,
    since: Option<&str>,
) -> Result<Vec<UsageStats>> {
    let conn = open()?;
    let mut sql = String::from(
        "SELECT model_name,
                SUM(input_tokens) as total_input_tokens,
                SUM(output_tokens) as total_output_tokens,
                SUM(cost) as total_cost,
                COUNT(*) as request_count,
                SUM(cache_hit) as cache_hits
         FROM usage_records WHERE 1=1",
    );
    let mut vals: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(m) = model_name {
        sql.push_str(" AND model_name = ?");
        vals.push(rusqlite::types::Value::Text(m.to_string()));
    }
    if let Some(u) = user_id {
        sql.push_str(" AND user_id = ?");
        vals.push(rusqlite::types::Value::Text(u.to_string()));
    }
    if let Some(s) = since {
        sql.push_str(" AND created_at >= ?");
        vals.push(rusqlite::types::Value::Text(s.to_string()));
    }
    sql.push_str(" GROUP BY model_name ORDER BY total_cost DESC");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(vals.iter().cloned()), |row| {
        Ok(UsageStats {
            model_name: row.get("model_name")?,
            total_input_tokens: row.get("total_input_tokens")?,
            total_output_tokens: row.get("total_output_tokens")?,
            total_cost: row.get("total_cost")?,
            request_count: row.get("request_count")?,
            cache_hits: row.get("cache_hits")?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get_total_spend(user_id: Option<&str>, model_name: Option<&str>, since: Option<&str>) -> Result<f64> {
    let conn = open()?;
    let mut sql = String::from("SELECT COALESCE(SUM(cost), 0.0) as total FROM usage_records WHERE 1=1");
    let mut vals: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(u) = user_id {
        sql.push_str(" AND user_id = ?");
        vals.push(rusqlite::types::Value::Text(u.to_string()));
    }
    if let Some(m) = model_name {
        sql.push_str(" AND model_name = ?");
        vals.push(rusqlite::types::Value::Text(m.to_string()));
    }
    if let Some(s) = since {
        sql.push_str(" AND created_at >= ?");
        vals.push(rusqlite::types::Value::Text(s.to_string()));
    }
    let total = conn.query_row(&sql, rusqlite::params_from_iter(vals.iter().cloned()), |row| {
        row.get::<_, f64>(0)
    })?;
    Ok(total)
}

// ── Settings (key/value store) ──

pub fn get_setting(key: &str) -> Result<Option<String>> {
    let conn = open()?;
    let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
    let mut rows = stmt.query_map(params![key], |row| row.get::<_, String>(0))?;
    Ok(rows.next().transpose()?)
}

pub fn set_setting(key: &str, value: &str) -> Result<()> {
    let conn = open()?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

// ── Semantic-cache calibration ──

/// Silent, per-request log used to (a) monitor the similarity distribution and
/// (b) accumulate labeled samples for threshold self-tuning. `label` is None for
/// passive observations; the inline question sets it (1=correct, 0=incorrect).
pub fn insert_cache_calibration(
    sim: f64,
    decision: &str,
    model: &str,
    label: Option<bool>,
    source: &str,
) -> Result<()> {
    let conn = open()?;
    conn.execute(
        "INSERT INTO cache_calibration (sim, decision, model, label, source)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            sim,
            decision,
            model,
            label.map(|b| if b { 1i64 } else { 0i64 }),
            source
        ],
    )?;
    Ok(())
}

/// All labeled (sim, correct) samples collected so far.
pub fn calibration_labeled_samples() -> Result<Vec<(f64, bool)>> {
    let conn = open()?;
    let mut stmt = conn.prepare("SELECT sim, label FROM cache_calibration WHERE label IS NOT NULL")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, f64>(0)?, row.get::<_, i64>(1)? == 1))
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Choose a threshold by Youden's J over the labeled samples, hard-capped so the
/// false-positive rate stays <= `max_fpr`. Returns None when there are too few
/// samples to tune reliably. Result is clamped to [0.70, 0.92].
pub fn optimal_threshold(samples: &[(f64, bool)], max_fpr: f64) -> Option<f64> {
    if samples.len() < 10 {
        return None;
    }
    let lo = 0.70_f64;
    let hi = 0.92_f64;
    let steps = 120;
    let mut best: Option<(f64, f64)> = None;
    for i in 0..=steps {
        let t = lo + (hi - lo) * (i as f64 / steps as f64);
        let (mut tp, mut fp, mut tn, mut fn_) = (0i64, 0i64, 0i64, 0i64);
        for (s, correct) in samples {
            let pred_hit = *s >= t;
            match (pred_hit, *correct) {
                (true, true) => tp += 1,
                (true, false) => fp += 1,
                (false, true) => fn_ += 1,
                (false, false) => tn += 1,
            }
        }
        let tpr = if (tp + fn_) > 0 {
            tp as f64 / (tp + fn_) as f64
        } else {
            0.0
        };
        let fpr = if (fp + tn) > 0 {
            fp as f64 / (fp + tn) as f64
        } else {
            0.0
        };
        if fpr > max_fpr {
            continue;
        }
        let youden = tpr - fpr;
        if best.map(|(_, by)| youden <= by).unwrap_or(true) {
            best = Some((t, youden));
        }
    }
    best.map(|(t, _)| t)
}

// ── Budget ──

pub fn upsert_budget(scope: &str, scope_id: &str, max_budget: f64, duration: &str) -> Result<()> {
    let conn = open()?;
    conn.execute(
        "INSERT INTO budgets (scope, scope_id, max_budget, duration) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(scope, scope_id) DO UPDATE SET max_budget = excluded.max_budget, duration = excluded.duration",
        params![scope, scope_id, max_budget, duration],
    )?;
    Ok(())
}

pub fn get_budget(scope: &str, scope_id: &str) -> Result<Option<Budget>> {
    let conn = open()?;
    let mut stmt = conn.prepare("SELECT * FROM budgets WHERE scope = ?1 AND scope_id = ?2")?;
    let mut rows = stmt.query(params![scope, scope_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(Budget {
            id: row.get("id")?,
            scope: row.get("scope")?,
            scope_id: row.get("scope_id")?,
            max_budget: row.get("max_budget")?,
            duration: row.get("duration")?,
        })),
        None => Ok(None),
    }
}

pub fn delete_budget(scope: &str, scope_id: &str) -> Result<bool> {
    let conn = open()?;
    let n = conn.execute(
        "DELETE FROM budgets WHERE scope = ?1 AND scope_id = ?2",
        params![scope, scope_id],
    )?;
    Ok(n > 0)
}

pub fn list_budgets() -> Result<Vec<Budget>> {
    let conn = open()?;
    let mut stmt = conn.prepare("SELECT * FROM budgets ORDER BY scope, scope_id")?;
    let rows = stmt.query_map([], |row| {
        Ok(Budget {
            id: row.get("id")?,
            scope: row.get("scope")?,
            scope_id: row.get("scope_id")?,
            max_budget: row.get("max_budget")?,
            duration: row.get("duration")?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

// ── helpers ──

fn json_to_sqlite(v: &serde_json::Value) -> rusqlite::types::Value {
    match v {
        serde_json::Value::Null => rusqlite::types::Value::Null,
        serde_json::Value::Bool(b) => rusqlite::types::Value::Integer(if *b { 1 } else { 0 }),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                rusqlite::types::Value::Integer(i)
            } else {
                rusqlite::types::Value::Real(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => rusqlite::types::Value::Text(s.clone()),
        _ => rusqlite::types::Value::Text(v.to_string()),
    }
}
