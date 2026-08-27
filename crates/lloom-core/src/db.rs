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
    capability_tier INTEGER DEFAULT 2,
    quality_score REAL DEFAULT 0.6,
    context_window INTEGER DEFAULT 32768,
    supports_tools INTEGER DEFAULT 0,
    supports_vision INTEGER DEFAULT 0,
    supports_stream INTEGER DEFAULT 0,
    is_local INTEGER DEFAULT 0,
    priority INTEGER DEFAULT 0,
    health_state TEXT DEFAULT 'unknown',
    health_checked_at TIMESTAMP,
    needs_calibration INTEGER DEFAULT 1,
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
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    latency_ms REAL,
    request_id TEXT
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
CREATE INDEX IF NOT EXISTS idx_usage_req ON usage_records(request_id);

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

CREATE TABLE IF NOT EXISTS routing_policy (
    task_type            TEXT PRIMARY KEY,
    min_capability_tier  INTEGER DEFAULT 1,
    cost_weight          REAL    DEFAULT 0.4,
    quality_weight       REAL    DEFAULT 0.5,
    latency_weight       REAL    DEFAULT 0.1,
    max_cost_per_request REAL,
    pinned_model         TEXT,
    fallback_depth       INTEGER DEFAULT 2,
    escalation_enabled   INTEGER DEFAULT 0,
    updated_at           TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS model_task_score (
    model_name TEXT, task_type TEXT,
    success_count INTEGER DEFAULT 0, fail_count INTEGER DEFAULT 0,
    escalation_count INTEGER DEFAULT 0,
    avg_cost REAL DEFAULT 0, avg_latency_ms REAL DEFAULT 0,
    ewma_quality REAL DEFAULT 0.6, sample_count INTEGER DEFAULT 0,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (model_name, task_type)
);

CREATE TABLE IF NOT EXISTS routing_decisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id TEXT, created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    task_type TEXT, band TEXT,
    signals_json TEXT, candidates_json TEXT,
    selected TEXT, fallback_chain TEXT,
    routing_ms REAL, outcome TEXT
);
CREATE INDEX IF NOT EXISTS idx_rd_created ON routing_decisions(created_at);
CREATE INDEX IF NOT EXISTS idx_rd_task ON routing_decisions(task_type);

CREATE TABLE IF NOT EXISTS routing_calibration (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    task_type TEXT, query_hash TEXT,
    routed_model TEXT, baseline_model TEXT,
    routed_cost REAL, baseline_cost REAL,
    routed_quality REAL, baseline_quality REAL, label INTEGER, source TEXT
);
"#;

pub fn init_db() -> Result<()> {
    let conn = open()?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch(SCHEMA)?;
    migrate_db(&conn)?;
    Ok(())
}

/// 增量迁移（PRICING-PLAN §九 + ROUTING-PLAN P0.b/c，幂等可重跑）：
/// 1. usage_records 追加 7 列（PRAGMA 检查防重复 ALTER）
/// 2. DashScope 系单价除以 10 修正量纲（一次性，settings 标记）
/// 3. models → price_specs 投影（INSERT OR IGNORE）
/// 4. 预置 deepseek 峰谷规则与 2026 节假日表（仅当缺失）
/// 5. models 追加路由元数据列（P0.b）+ 一次性名称启发式回填（settings 标记）
/// 6. routing_policy 种子策略（INSERT OR IGNORE 幂等，用户改过后不覆盖）
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
        // P1.a：延迟与请求号，串联 chat/orchestrate 的用量行
        ("latency_ms", "ALTER TABLE usage_records ADD COLUMN latency_ms REAL"),
        ("request_id", "ALTER TABLE usage_records ADD COLUMN request_id TEXT"),
    ];
    for (col, ddl) in add_cols {
        if !cols.iter().any(|c| c == col) {
            conn.execute_batch(ddl)?;
        }
    }
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_usage_req ON usage_records(request_id)")?;

    // 7. P1.a：清理遗留脏数据（早期编排展示性占位行：model='default'、cost=0）。
    //    一次性标记防重复跑；只清 cost=0 的占位，不动真实（可能 cost=0 的本地/缓存）成功账。
    let usage_cleaned: i64 = conn.query_row(
        "SELECT COUNT(*) FROM settings WHERE key = 'migration_usage_v1_p1a'",
        [],
        |r| r.get(0),
    )?;
    if usage_cleaned == 0 {
        conn.execute("DELETE FROM usage_records WHERE model_name = 'default' AND cost = 0", [])?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('migration_usage_v1_p1a', 'done')",
            [],
        )?;
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

    // 5. P0.b：models 追加路由元数据列（旧库；新库 SCHEMA 已含）
    let mcols = table_columns(conn, "models")?;
    let model_cols: &[(&str, &str)] = &[
        ("capability_tier", "ALTER TABLE models ADD COLUMN capability_tier INTEGER DEFAULT 2"),
        ("quality_score", "ALTER TABLE models ADD COLUMN quality_score REAL DEFAULT 0.6"),
        ("context_window", "ALTER TABLE models ADD COLUMN context_window INTEGER DEFAULT 32768"),
        ("supports_tools", "ALTER TABLE models ADD COLUMN supports_tools INTEGER DEFAULT 0"),
        ("supports_vision", "ALTER TABLE models ADD COLUMN supports_vision INTEGER DEFAULT 0"),
        ("supports_stream", "ALTER TABLE models ADD COLUMN supports_stream INTEGER DEFAULT 0"),
        ("is_local", "ALTER TABLE models ADD COLUMN is_local INTEGER DEFAULT 0"),
        ("priority", "ALTER TABLE models ADD COLUMN priority INTEGER DEFAULT 0"),
        ("health_state", "ALTER TABLE models ADD COLUMN health_state TEXT DEFAULT 'unknown'"),
        ("health_checked_at", "ALTER TABLE models ADD COLUMN health_checked_at TIMESTAMP"),
        ("needs_calibration", "ALTER TABLE models ADD COLUMN needs_calibration INTEGER DEFAULT 1"),
    ];
    for (col, ddl) in model_cols {
        if !mcols.iter().any(|c| c == col) {
            conn.execute_batch(ddl)?;
        }
    }

    // 5b. 一次性名称启发式回填（P0.e fill_heuristic 简版；settings 标记防重跑，
    //     用户后续可经 update_model 白名单改 capability_tier 等）
    let meta_migrated: i64 = conn.query_row(
        "SELECT COUNT(*) FROM settings WHERE key = 'migration_routing_meta_v1'",
        [],
        |r| r.get(0),
    )?;
    if meta_migrated == 0 {
        conn.execute_batch(
            // 能力档：现有 7 模型精确映射优先，未见名称走 LIKE 启发式
            "UPDATE models SET capability_tier = CASE
                WHEN name IN ('qwen3.6-plus','qwen3-max','deepseek-v3') THEN 3
                WHEN name IN ('qwen-plus','gpt-4o') THEN 2
                WHEN name IN ('qwen2.5-local','qwen3.6-flash') THEN 1
                WHEN name LIKE '%flash%' OR name LIKE '%mini%' OR name LIKE '%turbo%'
                  OR name LIKE '%small%' OR name LIKE '%lite%'  OR name LIKE '%air%'
                  OR name LIKE '%local%' OR name LIKE '%8b%'   OR name LIKE '%4b%' THEN 1
                WHEN name LIKE '%max%'   OR name LIKE '%opus%'  OR name LIKE '%ultra%'
                  OR name LIKE '%pro%'   OR name LIKE '%reasoner%' OR name LIKE '%thinking%'
                  OR name LIKE '%r1%'    OR name LIKE '%o1%'    OR name LIKE '%o3%' THEN 3
                ELSE 2 END",
        )?;
        // 上下文窗口：已知模型精确值，其余保持默认 32768
        conn.execute_batch(
            "UPDATE models SET context_window = CASE
                WHEN name = 'qwen3.6-plus' THEN 1048576
                WHEN name = 'qwen3-max'    THEN 262144
                WHEN name = 'qwen3.6-flash' THEN 262144
                WHEN name = 'qwen-plus'    THEN 131072
                WHEN name = 'gpt-4o'       THEN 128000
                WHEN name = 'deepseek-v3'  THEN 65536
                WHEN name = 'qwen2.5-local' THEN 32768
                ELSE context_window END",
        )?;
        // 本地模型判定（Ollama 或本机端点，零成本）
        conn.execute_batch(
            "UPDATE models SET is_local = CASE
                WHEN provider = 'ollama' OR api_base LIKE '%127.0.0.1%' OR api_base LIKE '%localhost%'
                THEN 1 ELSE 0 END",
        )?;
        // 须流式调用：推理系（沿用旧 INFERENCE_MODELS 名单 + 思考系名称启发式）
        conn.execute_batch(
            "UPDATE models SET supports_stream = CASE
                WHEN name IN ('qwen3.6-flash','qwen3.6-plus','qwen3-max','deepseek-v3') THEN 1
                WHEN name LIKE '%thinking%' OR name LIKE '%reasoner%' OR name LIKE '%r1%' THEN 1
                ELSE 0 END",
        )?;
        // 冷启动质量分：按能力档粗分（P1.c ewma 接线前的兜底）
        conn.execute_batch(
            "UPDATE models SET quality_score = CASE
                WHEN capability_tier = 3 THEN 0.85
                WHEN capability_tier = 2 THEN 0.70
                WHEN is_local = 1 THEN 0.45
                ELSE 0.50 END",
        )?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('migration_routing_meta_v1', 'done')",
            [],
        )?;
    }

    // 6. P0.c 种子路由策略（INSERT OR IGNORE：仅补缺，不覆盖用户改动）
    conn.execute_batch(
        "INSERT OR IGNORE INTO routing_policy
            (task_type, min_capability_tier, cost_weight, quality_weight, latency_weight,
             max_cost_per_request, pinned_model, fallback_depth, escalation_enabled)
         VALUES
            ('simple_qa',       1, 0.7, 0.2, 0.1, NULL, NULL, 2, 0),
            ('general',         2, 0.5, 0.4, 0.1, NULL, NULL, 2, 0),
            ('coding',          3, 0.3, 0.6, 0.1, NULL, NULL, 2, 0),
            ('math_logic',      3, 0.3, 0.6, 0.1, NULL, NULL, 2, 0),
            ('complex_reasoning', 3, 0.2, 0.7, 0.1, NULL, NULL, 2, 0),
            ('decompose',       1, 0.6, 0.3, 0.1, NULL, NULL, 1, 1),
            ('aggregate',       2, 0.4, 0.5, 0.1, NULL, NULL, 2, 0)",
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

/// P0.a 量纲写入断言：单价必须为 0（本地/未知价）或落在 [1e-9, 1e-3] USD/token。
/// 防「官方元/M × 汇率」忘除量纲的 10× 错误复发（见 ROUTING-PLAN P0-4）。
fn validate_cost(in_cost: f64, out_cost: f64) -> Result<()> {
    const LO: f64 = 1e-9;
    const HI: f64 = 1e-3;
    let ok = |v: f64| v == 0.0 || (LO..=HI).contains(&v);
    if !ok(in_cost) || !ok(out_cost) {
        return Err(AppError::InvalidRequest(format!(
            "单价越界 [1e-9, 1e-3] USD/token: in={in_cost} out={out_cost}; 疑似量纲错误（百炼价是否忘了 /10 或汇率）"
        )));
    }
    Ok(())
}

pub fn insert_model(m: &Model) -> Result<i64> {
    // P0.e: 新增模型自动打标（overlay 显式 > 启发式兜底；未显式档位按名字定档，
    // 本地端点置零成本，标 needs_calibration 进入保守期）。
    let mut filled = m.clone();
    let _report = crate::metadata::resolve_and_fill(&mut filled);
    validate_cost(filled.input_cost_per_token, filled.output_cost_per_token)?;
    let conn = open()?;
    let res = conn.execute(
        "INSERT INTO models (name, provider, litellm_model, api_base, api_key_env, task_type,
                             input_cost_per_token, output_cost_per_token, rpm, is_active,
                             capability_tier, quality_score, context_window,
                             supports_tools, supports_vision, supports_stream,
                             is_local, priority, needs_calibration)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                 ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
        params![
            filled.name,
            filled.provider,
            filled.litellm_model,
            filled.api_base,
            filled.api_key_env,
            filled.task_type,
            filled.input_cost_per_token,
            filled.output_cost_per_token,
            filled.rpm,
            filled.is_active,
            filled.capability_tier,
            filled.quality_score,
            filled.context_window,
            filled.supports_tools,
            filled.supports_vision,
            filled.supports_stream,
            filled.is_local,
            filled.priority,
            filled.needs_calibration,
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
        // P0.b 路由元数据（用户可在 UI 调档）
        "capability_tier",
        "quality_score",
        "context_window",
        "supports_tools",
        "supports_vision",
        "supports_stream",
        "is_local",
        "priority",
        // health_state / needs_calibration 由系统写，不放白名单（防手改破坏自适应）
    ];
    for k in updates.keys() {
        if !ALLOWED.contains(&k.as_str()) {
            return Err(AppError::InvalidRequest(format!(
                "unknown column '{k}'; allowed: {}",
                ALLOWED.join(", ")
            )));
        }
    }
    // P0.a：单价更新走同一断言（未提供的分量按 0 只验证出现的键）
    if let Some(in_v) = updates.get("input_cost_per_token").and_then(|v| v.as_f64()) {
        let out_v = updates
            .get("output_cost_per_token")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        validate_cost(in_v, out_v)?;
    } else if let Some(out_v) = updates.get("output_cost_per_token").and_then(|v| v.as_f64()) {
        validate_cost(0.0, out_v)?;
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
        capability_tier: row.get("capability_tier")?,
        quality_score: row.get("quality_score")?,
        context_window: row.get("context_window")?,
        supports_tools: row.get("supports_tools")?,
        supports_vision: row.get("supports_vision")?,
        supports_stream: row.get("supports_stream")?,
        is_local: row.get("is_local")?,
        priority: row.get("priority")?,
        health_state: row
            .get::<_, Option<String>>("health_state")?
            .unwrap_or_else(|| "unknown".to_string()),
        needs_calibration: row.get("needs_calibration")?,
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

/// Insert one usage_records row. `latency_ms`/`request_id` (P1.a) are optional
/// for backward compatibility — old callers pass None.
#[allow(clippy::too_many_arguments)]
pub fn insert_usage(
    model_name: &str,
    user_id: &str,
    input_tokens: i64,
    output_tokens: i64,
    cost: f64,
    task_type: Option<&str>,
    cache_hit: bool,
    latency_ms: Option<f64>,
    request_id: Option<&str>,
    extra: Option<&UsageExtra>,
) -> Result<i64> {
    let mut cols: Vec<&str> = vec![
        "model_name", "user_id", "input_tokens", "output_tokens", "cost", "task_type", "cache_hit",
    ];
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
    if let Some(e) = extra {
        cols.extend(["cached_tokens", "reasoning_tokens", "est_cost", "act_cost",
                     "zone_multiplier", "conversation_id", "field_missing"]);
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
    cols.extend(["latency_ms", "request_id"]);
    vals.push(match latency_ms {
        Some(l) => rusqlite::types::Value::Real(l),
        None => rusqlite::types::Value::Null,
    });
    vals.push(match request_id {
        Some(r) => rusqlite::types::Value::Text(r.to_string()),
        None => rusqlite::types::Value::Null,
    });
    let placeholders: Vec<String> = (1..=vals.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "INSERT INTO usage_records ({}) VALUES ({})",
        cols.join(", "),
        placeholders.join(", ")
    );
    let conn = open()?;
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

// ── Routing policy / score / audit (ROUTING-PLAN P0.c) ──

use crate::models::{ModelTaskScore, RoutingPolicy};

fn routing_policy_from_row(row: &rusqlite::Row) -> rusqlite::Result<RoutingPolicy> {
    Ok(RoutingPolicy {
        task_type: row.get("task_type")?,
        min_capability_tier: row.get("min_capability_tier")?,
        cost_weight: row.get("cost_weight")?,
        quality_weight: row.get("quality_weight")?,
        latency_weight: row.get("latency_weight")?,
        max_cost_per_request: row.get("max_cost_per_request")?,
        pinned_model: row.get("pinned_model")?,
        fallback_depth: row.get("fallback_depth")?,
        escalation_enabled: row.get("escalation_enabled")?,
    })
}

pub fn get_routing_policy(task_type: &str) -> Result<Option<RoutingPolicy>> {
    let conn = open()?;
    let mut stmt =
        conn.prepare("SELECT * FROM routing_policy WHERE task_type = ?1")?;
    let mut rows = stmt.query(params![task_type])?;
    match rows.next()? {
        Some(row) => Ok(Some(routing_policy_from_row(row)?)),
        None => Ok(None),
    }
}

pub fn list_routing_policy() -> Result<Vec<RoutingPolicy>> {
    let conn = open()?;
    let mut stmt = conn.prepare("SELECT * FROM routing_policy ORDER BY task_type")?;
    let rows = stmt.query_map([], routing_policy_from_row)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn upsert_routing_policy(p: &RoutingPolicy) -> Result<()> {
    let conn = open()?;
    conn.execute(
        "INSERT INTO routing_policy (task_type, min_capability_tier, cost_weight, quality_weight,
                                     latency_weight, max_cost_per_request, pinned_model,
                                     fallback_depth, escalation_enabled, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, CURRENT_TIMESTAMP)
         ON CONFLICT(task_type) DO UPDATE SET
            min_capability_tier = excluded.min_capability_tier,
            cost_weight = excluded.cost_weight,
            quality_weight = excluded.quality_weight,
            latency_weight = excluded.latency_weight,
            max_cost_per_request = excluded.max_cost_per_request,
            pinned_model = excluded.pinned_model,
            fallback_depth = excluded.fallback_depth,
            escalation_enabled = excluded.escalation_enabled,
            updated_at = CURRENT_TIMESTAMP",
        params![
            p.task_type,
            p.min_capability_tier,
            p.cost_weight,
            p.quality_weight,
            p.latency_weight,
            p.max_cost_per_request,
            p.pinned_model,
            p.fallback_depth,
            p.escalation_enabled,
        ],
    )?;
    Ok(())
}

/// 审计落库：plan() 的决策快照。返回行 id 供调用方回填 outcome。
#[allow(clippy::too_many_arguments)]
pub fn insert_routing_decision(
    request_id: &str,
    task_type: &str,
    band: &str,
    signals_json: &str,
    candidates_json: &str,
    selected: &str,
    fallback_chain: &str,
    routing_ms: f64,
) -> Result<i64> {
    let conn = open()?;
    conn.execute(
        "INSERT INTO routing_decisions (request_id, task_type, band, signals_json,
                                        candidates_json, selected, fallback_chain, routing_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            request_id,
            task_type,
            band,
            signals_json,
            candidates_json,
            selected,
            fallback_chain,
            routing_ms,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_routing_decision_outcome(id: i64, outcome: &str) -> Result<()> {
    let conn = open()?;
    conn.execute(
        "UPDATE routing_decisions SET outcome = ?2 WHERE id = ?1",
        params![id, outcome],
    )?;
    Ok(())
}

pub fn get_model_task_score(model_name: &str, task_type: &str) -> Result<Option<ModelTaskScore>> {
    let conn = open()?;
    let mut stmt = conn.prepare(
        "SELECT * FROM model_task_score WHERE model_name = ?1 AND task_type = ?2",
    )?;
    let mut rows = stmt.query(params![model_name, task_type])?;
    match rows.next()? {
        Some(row) => Ok(Some(ModelTaskScore {
            model_name: row.get("model_name")?,
            task_type: row.get("task_type")?,
            success_count: row.get("success_count")?,
            fail_count: row.get("fail_count")?,
            escalation_count: row.get("escalation_count")?,
            avg_cost: row.get("avg_cost")?,
            avg_latency_ms: row.get("avg_latency_ms")?,
            ewma_quality: row.get("ewma_quality")?,
            sample_count: row.get("sample_count")?,
        })),
        None => Ok(None),
    }
}

/// 信号回填：ewma_quality ← (1-α)·ewma + α·σ，α=0.15；首样本直接写 σ。
/// 供 P1.c 信号层接线（成功/失败/升级）调用，P0.d 阶段不接线。
pub fn upsert_model_task_score_signal(
    model_name: &str,
    task_type: &str,
    signal: f64,
) -> Result<()> {
    let conn = open()?;
    conn.execute(
        "INSERT INTO model_task_score (model_name, task_type, ewma_quality, sample_count, updated_at)
         VALUES (?1, ?2, ?3, 1, CURRENT_TIMESTAMP)
         ON CONFLICT(model_name, task_type) DO UPDATE SET
            ewma_quality = ewma_quality * 0.85 + ?3 * 0.15,
            sample_count = sample_count + 1,
            updated_at = CURRENT_TIMESTAMP",
        params![model_name, task_type, signal.clamp(0.0, 1.0)],
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

        // P0.e 自动打标冒烟：insert_model 走 resolve_and_fill，注册的新模型按名字+本地端点
        // 回填能力档/上下文/is_local/needs_calibration，并落库验证。
        let smoke = crate::models::Model {
            id: 0,
            name: "smoke-flash-1b".into(),
            provider: "dashscope".into(),
            litellm_model: "dashscope/smoke-flash-1b".into(),
            api_base: String::new(),
            api_key_env: String::new(),
            task_type: "general".into(),
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
            rpm: 60,
            is_active: 1,
            capability_tier: 2,      // 默认档 → 应按名字启发式降档为轻量
            quality_score: 0.6,
            context_window: 0,       // 0 → 启发式回填默认 32K
            supports_tools: 0,
            supports_vision: 0,
            supports_stream: 0,
            is_local: 0,
            priority: 0,
            health_state: "unknown".into(),
            needs_calibration: 0,
        };
        insert_model(&smoke).unwrap();
        let conn3 = open().unwrap();
        let (tier, ctx, calib): (i64, i64, i64) = conn3
            .query_row(
                "SELECT capability_tier, context_window, needs_calibration
                 FROM models WHERE name = 'smoke-flash-1b'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(tier, 1, "flash/1b 应按名字归轻量档");
        assert_eq!(ctx, 32768, "未显式上下文应回填 32K");
        assert_eq!(calib, 1, "新模型应进入保守期");
        drop(conn3);

        // P1.a 冒烟：新列存在 + insert_usage 落 latency_ms/request_id/task_type，
        // 且幂等 init_db 不重复清脏、不重复加列。
        let p1_cols = table_columns(&open().unwrap(), "usage_records").unwrap();
        for c in ["latency_ms", "request_id"] {
            assert!(p1_cols.contains(&c.to_string()), "missing P1.a column {c}");
        }
        insert_usage(
            "qwen-plus-test", "default", 100, 20, 1.11e-5,
            Some("coding"), true, Some(812.5), Some("chat-smoke-1"),
            Some(&UsageExtra {
                cached_tokens: 0, reasoning_tokens: 0, est_cost: 0.0, act_cost: 1.11e-5,
                zone_multiplier: 1.0, conversation_id: None, field_missing: false,
            }),
        )
        .unwrap();
        let conn4 = open().unwrap();
        let (lat, rid, tt): (Option<f64>, Option<String>, Option<String>) = conn4
            .query_row(
                "SELECT latency_ms, request_id, task_type FROM usage_records
                 WHERE request_id = 'chat-smoke-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(lat, Some(812.5));
        assert_eq!(rid.as_deref(), Some("chat-smoke-1"));
        assert_eq!(tt.as_deref(), Some("coding"));
        drop(conn4);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod cost_assert_tests {
    use super::*;

    #[test]
    fn validate_cost_bounds() {
        assert!(validate_cost(0.0, 0.0).is_ok()); // 本地/未知价
        assert!(validate_cost(1.11e-7, 2.78e-7).is_ok()); // 正常 USD/token
        assert!(validate_cost(1e-9, 1e-3).is_ok()); // 边界含端点
        // 10× 内部错误（1e-6 级）值域仍在界内，断言拦不住——那是迁移+校准层的职责；
        // 断言拦的是「忘除 1e6」级（元/M 原值直接写入）与负数/超上限
        assert!(validate_cost(1.39e-6, 1.111e-5).is_ok()); // 迁移前原始 10× 值仍在界内
        assert!(validate_cost(1.11e-5, 2.78e-5).is_ok()); // 100× 仍 < 1e-3
        assert!(validate_cost(2.5e-6, 1.0e-5).is_ok()); // gpt-4o 正确值（USD）不受影响
        assert!(validate_cost(0.8, 2.0).is_err()); // 元/M 原值（忘除量纲）被拒
        assert!(validate_cost(-1e-6, 0.0).is_err()); // 负数被拒
        assert!(validate_cost(0.0, 1e-2).is_err()); // 超上限
    }
}
