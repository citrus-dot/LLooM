//! SQLite database layer — schema, CRUD, and query helpers.
//! Strongly-typed port of `core/database.py`.

use crate::error::{AppError, Result};
use crate::models::{Budget, Model, UsageStats};
use crate::pricing::{PriceSpec, TierBand, Zone};
use rusqlite::{params, Connection};
use serde::Serialize;

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

CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL DEFAULT '新对话',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    summary TEXT,
    summary_upto INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    conv_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL DEFAULT '',
    meta TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(conv_id, seq),
    FOREIGN KEY (conv_id) REFERENCES conversations(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_messages_conv ON messages(conv_id, seq);

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

-- ── PRICING-PLAN §3.2：价格规格 / 时段规则 / 校准统计 ──
CREATE TABLE IF NOT EXISTS price_specs (
    provider        TEXT NOT NULL,
    model           TEXT NOT NULL,
    input_cost              REAL NOT NULL,
    output_cost             REAL NOT NULL,
    cache_read_cost         REAL,
    cache_write_cost        REAL DEFAULT 0,
    reasoning_cost          REAL,
    tiered_json     TEXT,
    zone_ref        TEXT,
    batch_multiplier REAL DEFAULT 0.5,
    price_source    TEXT DEFAULT 'unknown',
    price_updated_at TIMESTAMP,
    price_stale     INTEGER DEFAULT 0,
    stale_reason    TEXT,
    effective_from  TEXT,
    cny_list_price_json TEXT,
    PRIMARY KEY (provider, model)
);

CREATE TABLE IF NOT EXISTS provider_zones (
    provider   TEXT NOT NULL,
    rule_json  TEXT NOT NULL,
    tz         TEXT DEFAULT 'Asia/Shanghai',
    holidays_json TEXT,
    PRIMARY KEY (provider)
);

CREATE TABLE IF NOT EXISTS price_calibration (
    provider   TEXT NOT NULL,
    model      TEXT NOT NULL,
    as_of      TEXT NOT NULL,
    calls      INTEGER DEFAULT 0,
    est_cost   REAL DEFAULT 0,
    act_cost   REAL DEFAULT 0,
    input_side_ratio REAL DEFAULT 1.0,
    cache_hit_rate REAL DEFAULT 0.0,
    out_in_ratio REAL DEFAULT 0.0,
    field_missing_count INTEGER DEFAULT 0,
    PRIMARY KEY (provider, model, as_of)
);
"#;

pub fn init_db() -> Result<()> {
    let conn = open()?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch(SCHEMA)?;
    migrate_db(&conn)?;
    Ok(())
}

/// 增量迁移（PRICING-PLAN §九，幂等可重跑）：
/// 1. usage_records 追加 7 列（PRAGMA 检查防重复 ALTER）
/// 2. DashScope 系单价除以 10 修正量纲（一次性，settings 标记）
/// 3. models → price_specs 投影（INSERT OR IGNORE）
/// 4. 预置 deepseek 峰谷规则与 2026 节假日表（仅当缺失）
fn migrate_db(conn: &Connection) -> Result<()> {
    // 1. usage_records 追加列
    let cols = table_columns(conn, "usage_records")?;
    let add_cols: &[(&str, &str)] = &[
        ("cached_tokens", "ALTER TABLE usage_records ADD COLUMN cached_tokens INTEGER DEFAULT 0"),
        ("reasoning_tokens", "ALTER TABLE usage_records ADD COLUMN reasoning_tokens INTEGER DEFAULT 0"),
        ("est_cost", "ALTER TABLE usage_records ADD COLUMN est_cost REAL DEFAULT 0"),
        ("act_cost", "ALTER TABLE usage_records ADD COLUMN act_cost REAL DEFAULT 0"),
        ("zone_multiplier", "ALTER TABLE usage_records ADD COLUMN zone_multiplier REAL DEFAULT 1.0"),
        ("conversation_id", "ALTER TABLE usage_records ADD COLUMN conversation_id TEXT"),
        ("field_missing", "ALTER TABLE usage_records ADD COLUMN field_missing INTEGER DEFAULT 0"),
    ];
    for (col, ddl) in add_cols {
        if !cols.iter().any(|c| c == col) {
            conn.execute_batch(ddl)?;
        }
    }

    // 2. 量纲修正（一次性）
    let migrated: i64 = conn.query_row(
        "SELECT COUNT(*) FROM settings WHERE key = 'migration_pricing_v1'",
        [],
        |r| r.get(0),
    )?;
    if migrated == 0 {
        conn.execute_batch(
            "UPDATE models SET input_cost_per_token  = input_cost_per_token  / 10.0,
                               output_cost_per_token = output_cost_per_token / 10.0
             WHERE provider = 'dashscope'",
        )?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('migration_pricing_v1', 'done')",
            [],
        )?;
    }

    // 3. models → price_specs 投影（只搬有价且活跃的；源标 overlay：值来自早期 overlay
    //    口径录入、未经人工核对，允许刷新覆盖与校准标 stale，而非 manual）
    conn.execute_batch(
        "INSERT OR IGNORE INTO price_specs
            (provider, model, input_cost, output_cost, cache_read_cost, cache_write_cost,
             reasoning_cost, tiered_json, zone_ref, batch_multiplier,
             price_source, price_updated_at, effective_from)
         SELECT provider, name, input_cost_per_token, output_cost_per_token,
            NULL, 0, NULL, NULL, NULL, 0.5,
            'overlay', CURRENT_TIMESTAMP, NULL
         FROM models
         WHERE is_active = 1 AND (input_cost_per_token > 0 OR output_cost_per_token > 0)",
    )?;

    // 4. 预置 deepseek 峰谷规则 + 2026 节假日表（占位，随国务院公布更新）
    conn.execute_batch(
        "INSERT OR IGNORE INTO provider_zones (provider, rule_json, tz, holidays_json) VALUES (
            'deepseek',
            '[{\"holidays\":true,\"hours\":\"*\",\"multiplier\":0.5},
              {\"days\":[\"sat\",\"sun\"],\"hours\":\"*\",\"multiplier\":0.5},
              {\"days\":[\"mon\",\"tue\",\"wed\",\"thu\",\"fri\"],\"hours\":\"9-12,14-18\",\"multiplier\":1.0},
              {\"days\":[\"mon\",\"tue\",\"wed\",\"thu\",\"fri\"],\"hours\":\"*\",\"multiplier\":0.5}]',
            'Asia/Shanghai',
            '[\"2026-01-01\",\"2026-02-16\",\"2026-02-17\",\"2026-02-18\",\"2026-02-19\",\"2026-02-20\",
              \"2026-04-05\",\"2026-05-01\",\"2026-05-02\",\"2026-05-03\",\"2026-06-19\",\"2026-09-25\",
              \"2026-10-01\",\"2026-10-02\",\"2026-10-03\",\"2026-10-04\",\"2026-10-05\",\"2026-10-06\",\"2026-10-07\"]'
        )",
    )?;

    Ok(())
}

fn table_columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
    rows.collect()
}

fn open_impl() -> Result<Connection> {
    let conn = Connection::open(crate::config::db_path())?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    Ok(conn)
}

/// Public connection for domain modules (e.g. the conversations store). WAL
/// mode is enabled for concurrency.
pub fn open() -> Result<Connection> {
    open_impl()
}

/// Open a connection with foreign keys enforced (needed for cascade deletes
/// on the conversations/messages tables).
pub fn open_fk() -> Result<Connection> {
    let conn = open_impl()?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
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

/// 用量落库的扩展字段（PRICING-PLAN §6.1）。全部 Option 化，旧调用点不受影响。
#[derive(Debug, Clone, Default)]
pub struct UsageExtra {
    pub cached_tokens: i64,
    pub reasoning_tokens: i64,
    pub est_cost: f64,
    pub act_cost: f64,
    pub zone_multiplier: f64,
    pub conversation_id: Option<String>,
    pub field_missing: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn insert_usage(
    model_name: &str,
    user_id: &str,
    input_tokens: i64,
    output_tokens: i64,
    cost: f64,
    task_type: Option<&str>,
    cache_hit: bool,
    extra: Option<&UsageExtra>,
) -> Result<i64> {
    let conn = open()?;
    let mut sql = String::from(
        "INSERT INTO usage_records (model_name, user_id, input_tokens, output_tokens, cost, task_type, cache_hit",
    );
    let mut vals: Vec<rusqlite::types::Value> = vec![
        rusqlite::types::Value::Text(model_name.to_string()),
        rusqlite::types::Value::Text(user_id.to_string()),
        rusqlite::types::Value::Integer(input_tokens),
        rusqlite::types::Value::Integer(output_tokens),
        rusqlite::types::Value::Real(cost),
        match task_type {
            Some(t) => rusqlite::types::Value::Text(t.to_string()),
            None => rusqlite::types::Value::Null,
        },
        rusqlite::types::Value::Integer(if cache_hit { 1 } else { 0 }),
    ];
    match extra {
        Some(e) => {
            sql.push_str(", cached_tokens, reasoning_tokens, est_cost, act_cost, zone_multiplier, conversation_id, field_missing) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)");
            vals.push(rusqlite::types::Value::Integer(e.cached_tokens));
            vals.push(rusqlite::types::Value::Integer(e.reasoning_tokens));
            vals.push(rusqlite::types::Value::Real(e.est_cost));
            vals.push(rusqlite::types::Value::Real(e.act_cost));
            vals.push(rusqlite::types::Value::Real(e.zone_multiplier));
            vals.push(match &e.conversation_id {
                Some(c) => rusqlite::types::Value::Text(c.clone()),
                None => rusqlite::types::Value::Null,
            });
            vals.push(rusqlite::types::Value::Integer(if e.field_missing { 1 } else { 0 }));
        }
        None => {
            sql.push_str(") VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)");
        }
    }
    conn.execute(&sql, rusqlite::params_from_iter(vals.iter().cloned()))?;
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

// ── Pricing (PRICING-PLAN §3.2 / §4) ──

fn price_spec_from_row(row: &rusqlite::Row) -> rusqlite::Result<PriceSpec> {
    let tiered_json: Option<String> = row.get("tiered_json")?;
    let tiered = tiered_json
        .as_deref()
        .and_then(|j| serde_json::from_str::<Vec<TierBand>>(j).ok());
    Ok(PriceSpec {
        provider: row.get("provider")?,
        model: row.get("model")?,
        input_cost: row.get("input_cost")?,
        output_cost: row.get("output_cost")?,
        cache_read_cost: row.get("cache_read_cost")?,
        cache_write_cost: row.get::<_, Option<f64>>("cache_write_cost")?.or(Some(0.0)),
        reasoning_cost: row.get("reasoning_cost")?,
        tiered,
        zone_ref: row.get("zone_ref")?,
        batch_multiplier: row.get::<_, Option<f64>>("batch_multiplier")?.unwrap_or(0.5),
        price_source: row.get::<_, Option<String>>("price_source")?.unwrap_or_default(),
        price_stale: row.get::<_, Option<i64>>("price_stale")?.unwrap_or(0) != 0,
        effective_from: row.get("effective_from")?,
    })
}

pub fn get_price_spec(provider: &str, model: &str) -> Result<Option<PriceSpec>> {
    let conn = open()?;
    let mut stmt = conn.prepare(
        "SELECT * FROM price_specs WHERE provider = ?1 AND model = ?2",
    )?;
    let mut rows = stmt.query(params![provider, model])?;
    match rows.next()? {
        Some(row) => Ok(Some(price_spec_from_row(row)?)),
        None => Ok(None),
    }
}

pub fn list_price_specs() -> Result<Vec<PriceSpec>> {
    let conn = open()?;
    let mut stmt = conn.prepare("SELECT * FROM price_specs ORDER BY provider, model")?;
    let rows = stmt.query_map([], price_spec_from_row)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn mark_price_stale(provider: &str, model: &str, stale: bool, reason: &str) -> Result<()> {
    let conn = open()?;
    conn.execute(
        "UPDATE price_specs SET price_stale = ?3, stale_reason = ?4 WHERE provider = ?1 AND model = ?2",
        params![provider, model, if stale { 1 } else { 0 }, reason],
    )?;
    Ok(())
}

pub fn list_provider_zones() -> Result<Vec<Zone>> {
    let conn = open()?;
    let mut stmt = conn.prepare("SELECT provider, rule_json, tz, holidays_json FROM provider_zones")?;
    let rows = stmt.query_map([], |row| {
        Ok(Zone::from_db(
            &row.get::<_, String>("provider")?,
            &row.get::<_, String>("rule_json")?,
            &row.get::<_, Option<String>>("tz")?.unwrap_or_default(),
            &row.get::<_, Option<String>>("holidays_json")?.unwrap_or_default(),
        ))
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 手工更新 PriceSpec（PR-6 WebUI 校对）。全量字段 upsert，强制转正为 manual。
#[allow(clippy::too_many_arguments)]
pub fn upsert_price_spec(
    provider: &str,
    model: &str,
    input_cost: f64,
    output_cost: f64,
    cache_read_cost: Option<f64>,
    cache_write_cost: Option<f64>,
    reasoning_cost: Option<f64>,
    tiered_json: Option<&str>,
    zone_ref: Option<&str>,
    cny_list_price_json: Option<&str>,
) -> Result<()> {
    let conn = open()?;
    conn.execute(
        "INSERT INTO price_specs (provider, model, input_cost, output_cost, cache_read_cost,
                                  cache_write_cost, reasoning_cost, tiered_json, zone_ref,
                                  batch_multiplier, price_source, price_updated_at, price_stale,
                                  stale_reason, effective_from, cny_list_price_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0.5, 'manual', CURRENT_TIMESTAMP, 0, NULL, CURRENT_DATE, ?10)
         ON CONFLICT(provider, model) DO UPDATE SET
            input_cost = excluded.input_cost,
            output_cost = excluded.output_cost,
            cache_read_cost = excluded.cache_read_cost,
            cache_write_cost = excluded.cache_write_cost,
            reasoning_cost = excluded.reasoning_cost,
            tiered_json = excluded.tiered_json,
            zone_ref = excluded.zone_ref,
            price_source = 'manual',
            price_updated_at = CURRENT_TIMESTAMP,
            price_stale = 0,
            stale_reason = NULL,
            effective_from = CURRENT_DATE,
            cny_list_price_json = excluded.cny_list_price_json",
        params![
            provider,
            model,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_write_cost,
            reasoning_cost,
            tiered_json,
            zone_ref,
            cny_list_price_json,
        ],
    )?;
    Ok(())
}

/// 按日聚合用量（PRICING-PLAN §6.2 校准燃料）。排除探针记账（task_type='probe'）。
/// 通过 price_specs join 补出 (provider, model)；无 PriceSpec 的模型（本地/未登记）不计入。
#[derive(Debug, Clone)]
pub struct DailyAggregate {
    pub provider: String,
    pub model: String,
    pub calls: i64,
    pub est_cost: f64,
    pub act_cost: f64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub field_missing: i64,
}

pub fn aggregate_usage_by_model_day(day: &str) -> Result<Vec<DailyAggregate>> {
    let conn = open()?;
    let mut stmt = conn.prepare(
        "SELECT ps.provider, ps.model,
                COUNT(*) as calls,
                COALESCE(SUM(u.est_cost), 0.0) as est_cost,
                COALESCE(SUM(u.act_cost), 0.0) as act_cost,
                COALESCE(SUM(u.input_tokens), 0) as input_tokens,
                COALESCE(SUM(u.output_tokens), 0) as output_tokens,
                COALESCE(SUM(u.cached_tokens), 0) as cached_tokens,
                COALESCE(SUM(u.field_missing), 0) as field_missing
         FROM usage_records u
         JOIN price_specs ps ON ps.model = u.model_name
         WHERE date(u.created_at) = ?1 AND (u.task_type IS NULL OR u.task_type != 'probe')
         GROUP BY ps.provider, ps.model",
    )?;
    let rows = stmt.query_map(params![day], |row| {
        Ok(DailyAggregate {
            provider: row.get("provider")?,
            model: row.get("model")?,
            calls: row.get("calls")?,
            est_cost: row.get("est_cost")?,
            act_cost: row.get("act_cost")?,
            input_tokens: row.get("input_tokens")?,
            output_tokens: row.get("output_tokens")?,
            cached_tokens: row.get("cached_tokens")?,
            field_missing: row.get("field_missing")?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 写入/更新一天的校准统计（UPSERT，按 (provider, model, as_of) 主键）。
#[allow(clippy::too_many_arguments)]
pub fn upsert_price_calibration(
    provider: &str,
    model: &str,
    as_of: &str,
    calls: i64,
    est_cost: f64,
    act_cost: f64,
    input_side_ratio: f64,
    cache_hit_rate: f64,
    out_in_ratio: f64,
    field_missing_count: i64,
) -> Result<()> {
    let conn = open()?;
    conn.execute(
        "INSERT INTO price_calibration (provider, model, as_of, calls, est_cost, act_cost,
                                        input_side_ratio, cache_hit_rate, out_in_ratio, field_missing_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(provider, model, as_of) DO UPDATE SET
            calls = excluded.calls,
            est_cost = excluded.est_cost,
            act_cost = excluded.act_cost,
            input_side_ratio = excluded.input_side_ratio,
            cache_hit_rate = excluded.cache_hit_rate,
            out_in_ratio = excluded.out_in_ratio,
            field_missing_count = excluded.field_missing_count",
        params![
            provider,
            model,
            as_of,
            calls,
            est_cost,
            act_cost,
            input_side_ratio,
            cache_hit_rate,
            out_in_ratio,
            field_missing_count,
        ],
    )?;
    Ok(())
}

/// stale 去抖：最近 `days` 天内对账偏差越界（∉[0.8,1.2]）的天数。
pub fn stale_streak(provider: &str, model: &str, days: i64) -> Result<i64> {
    let conn = open()?;
    let n = conn.query_row(
        "SELECT COUNT(*) FROM price_calibration
         WHERE provider = ?1 AND model = ?2
           AND as_of >= date('now', ?3)
           AND (input_side_ratio > 1.2 OR input_side_ratio < 0.8)",
        params![provider, model, format!("-{days} day")],
        |r| r.get::<_, i64>(0),
    )?;
    Ok(n)
}

/// 近 N 天校准曲线（WebUI 校准视图）。
#[derive(Debug, Clone, Serialize)]
pub struct CalibrationRow {
    pub provider: String,
    pub model: String,
    pub as_of: String,
    pub calls: i64,
    pub est_cost: f64,
    pub act_cost: f64,
    pub input_side_ratio: f64,
    pub cache_hit_rate: f64,
    pub out_in_ratio: f64,
    pub field_missing_count: i64,
}

pub fn list_price_calibration(days: i64) -> Result<Vec<CalibrationRow>> {
    let conn = open()?;
    let mut stmt = conn.prepare(
        "SELECT * FROM price_calibration WHERE as_of >= date('now', ?1) ORDER BY provider, model, as_of",
    )?;
    let rows = stmt.query_map(params![format!("-{days} day")], |row| {
        Ok(CalibrationRow {
            provider: row.get("provider")?,
            model: row.get("model")?,
            as_of: row.get("as_of")?,
            calls: row.get("calls")?,
            est_cost: row.get("est_cost")?,
            act_cost: row.get("act_cost")?,
            input_side_ratio: row.get("input_side_ratio")?,
            cache_hit_rate: row.get("cache_hit_rate")?,
            out_in_ratio: row.get("out_in_ratio")?,
            field_missing_count: row.get("field_missing_count")?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

// ── Probe stats (PRICING-PLAN §7) ──

/// 探针统计：本月轮数/花费/命中验证成功数/失败数（task_type='probe'）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct ProbeStats {
    pub rounds: i64,
    pub spend_usd: f64,
    pub hit_verifications: i64,
    pub hit_failures: i64,
    pub failures: i64, // 调用失败（非命中验证失败）
}

pub fn probe_stats() -> Result<ProbeStats> {
    let conn = open()?;
    let mut stmt = conn.prepare(
        "SELECT COUNT(*) as rounds,
                COALESCE(SUM(cost), 0.0) as spend,
                COALESCE(SUM(CASE WHEN cache_hit = 1 THEN 1 ELSE 0 END), 0) as hits,
                COALESCE(SUM(CASE WHEN cache_hit = 0 AND cost > 0 THEN 1 ELSE 0 END), 0) as hit_fails,
                COALESCE(SUM(CASE WHEN cost < 0 THEN 1 ELSE 0 END), 0) as fails
         FROM usage_records
         WHERE task_type = 'probe' AND created_at >= date('now', 'start of month')",
    )?;
    // cost < 0 作为调用失败的哨兵值（探针失败时插入 act_cost=-1 标记）
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        Ok(ProbeStats {
            rounds: row.get("rounds")?,
            spend_usd: row.get("spend")?,
            hit_verifications: row.get("hits")?,
            hit_failures: row.get("hit_fails")?,
            failures: row.get("fails")?,
        })
    } else {
        Ok(ProbeStats::default())
    }
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

#[cfg(test)]
mod migration_tests {
    use super::*;
    use std::sync::Once;

    static SETUP: Once = Once::new();

    fn test_data_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("lloom_migration_test_{}", std::process::id()))
    }

    fn setup() {
        SETUP.call_once(|| {
            let dir = test_data_dir();
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::env::set_var("LLOOM_DATA_DIR", &dir);
        });
    }

    fn insert_dashscope_model(conn: &Connection) {
        conn.execute(
            "INSERT INTO models (name, provider, litellm_model, input_cost_per_token, output_cost_per_token, is_active)
             VALUES ('qwen-plus-test', 'dashscope', 'qwen-plus', 1.11e-6, 2.78e-6, 1)",
            [],
        )
        .unwrap();
    }

    /// 模拟旧库升级：只建 schema（无迁移标记）+ 预置虚高模型 → init_db 触发迁移。
    /// 单测试串行完成全部断言（两个测试共享 LLOOM_DATA_DIR 环境变量，无法并行）。
    /// 验证：量纲 ÷10、7 新列、投影、deepseek zone 预置、幂等（不二次除）、
    /// 投影后的 PriceSpec 参与实际成本计算。
    #[test]
    fn migration_is_idempotent_and_fixes_scale() {
        setup();
        let dir = test_data_dir();
        // 旧库：SCHEMA 建表，不设 migration 标记
        let conn = open().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        insert_dashscope_model(&conn);
        drop(conn);

        // 升级路径：init_db → migrate
        init_db().unwrap();
        let conn = open().unwrap();
        let in_cost: f64 = conn
            .query_row(
                "SELECT input_cost_per_token FROM models WHERE name = 'qwen-plus-test'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        // 1.11e-6 / 10 = 1.11e-7
        assert!((in_cost - 1.11e-7).abs() < 1e-15, "scale not fixed: {in_cost}");

        // usage_records 新列存在
        let cols = table_columns(&conn, "usage_records").unwrap();
        for c in [
            "cached_tokens",
            "reasoning_tokens",
            "est_cost",
            "act_cost",
            "zone_multiplier",
            "conversation_id",
            "field_missing",
        ] {
            assert!(cols.contains(&c.to_string()), "missing column {c}");
        }

        // price_specs 投影成功，源为 overlay，价格为修正后的值
        let spec = get_price_spec("dashscope", "qwen-plus-test").unwrap().unwrap();
        assert!((spec.input_cost - 1.11e-7).abs() < 1e-15);
        assert_eq!(spec.price_source, "overlay");

        // 投影后的 PriceSpec 参与实际成本计算（cache_read=NULL → 命中按原价）
        let u = crate::pricing::UsageDetail {
            prompt_tokens: 10_000,
            completion_tokens: 100,
            cached_tokens: 5_000,
            ..Default::default()
        };
        let zr = crate::pricing::ZoneResolver::new();
        zr.load(list_provider_zones().unwrap());
        let cost = spec.actual_cost(&u, 0, &zr);
        let expected = 10_000.0 * 1.11e-7 + 100.0 * 2.78e-7;
        assert!((cost - expected).abs() < 1e-15, "cost={cost} expected={expected}");

        // provider_zones 预置 deepseek 规则
        let zones = list_provider_zones().unwrap();
        assert!(zones.iter().any(|z| z.provider == "deepseek" && !z.rules.is_empty()));

        // 幂等性：再次 init_db，量纲不得二次修正、投影不重复
        drop(conn);
        init_db().unwrap();
        let conn2 = open().unwrap();
        let in_cost2: f64 = conn2
            .query_row(
                "SELECT input_cost_per_token FROM models WHERE name = 'qwen-plus-test'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!((in_cost2 - 1.11e-7).abs() < 1e-15, "double-divide: {in_cost2}");
        drop(conn2);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
