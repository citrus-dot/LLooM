//! SQLite database layer — schema, CRUD, and query helpers.
//! Strongly-typed port of `core/database.py`.

use crate::error::{AppError, Result};
use crate::models::{Budget, Model, UsageStats};
use crate::pricing::{PriceSpec, TierBand, Zone};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashMap;

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
    scope_task_type TEXT,
    soft_limit_ratio REAL DEFAULT 0.8,
    action_on_exceed TEXT DEFAULT 'degrade',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(scope, scope_id)
);

CREATE INDEX IF NOT EXISTS idx_usage_model ON usage_records(model_name);
CREATE INDEX IF NOT EXISTS idx_usage_created ON usage_records(created_at);
CREATE INDEX IF NOT EXISTS idx_usage_user ON usage_records(user_id);
-- idx_usage_req 移到 migrate_db（P1.a）：旧库 usage_records 缺 request_id 时，
-- 此处建索引会在 migrate 的 ALTER 补列之前失败，必须等 ALTER 后幂等创建。

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
/// 6. routing_policy 种子策略（INSERT OR IGNORE 幂等，用户改过后不覆盖）+ P1.b 推荐主选回填
pub fn migrate_db(conn: &Connection) -> Result<()> {
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
        // P2.b：语义缓存命中省下的金额（命中时 act_cost 置 0，本列存「若未命中本应花费」）
        ("cache_saved_cost", "ALTER TABLE usage_records ADD COLUMN cache_saved_cost REAL DEFAULT 0"),
    ];
    for (col, ddl) in add_cols {
        if !cols.iter().any(|c| c == col) {
            conn.execute_batch(ddl)?;
        }
    }
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_usage_req ON usage_records(request_id)")?;

    // P5.b：预算模型扩展——scope 扩 user/model/task_type/global；新增紧凑对象列。
    //     UNIQUE 仍为 (scope, scope_id)（改约束需重建表，现有库不迁移）；scope_task_type
    //     作为附加维度保留（自动降档只用全局 scope=global）。
    {
        let budget_table_cols = table_columns(&conn, "budgets")?;
        for (col, ddl) in [
            ("scope_task_type", "ALTER TABLE budgets ADD COLUMN scope_task_type TEXT"),
            (
                "soft_limit_ratio",
                "ALTER TABLE budgets ADD COLUMN soft_limit_ratio REAL DEFAULT 0.8",
            ),
            (
                "action_on_exceed",
                "ALTER TABLE budgets ADD COLUMN action_on_exceed TEXT DEFAULT 'degrade'",
            ),
        ] {
            if !budget_table_cols.iter().any(|c| c == col) {
                conn.execute_batch(ddl)?;
            }
        }
    }

    // P5.c：model_task_score 加 avg_out_tokens（真实 usage 滚动 EWMA，见 roll_avg_out_tokens）。
    //     列默认 500 与历史固定 est_out 一致；冷启动由读侧判别（sample_count<20 → ×1.5）。
    {
        let score_table_cols = table_columns(&conn, "model_task_score")?;
        if !score_table_cols.iter().any(|c| c == "avg_out_tokens") {
            conn.execute_batch("ALTER TABLE model_task_score ADD COLUMN avg_out_tokens REAL DEFAULT 500")?;
        }
    }

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
    //    + P1.b：按 §P1.b 表为各任务预置「推荐主选」 pinned_model（新库在 VALUES 里带）
    conn.execute_batch(
        "INSERT OR IGNORE INTO routing_policy
            (task_type, min_capability_tier, cost_weight, quality_weight, latency_weight,
             max_cost_per_request, pinned_model, fallback_depth, escalation_enabled)
         VALUES
            ('simple_qa',         1, 0.7, 0.2, 0.1, NULL, 'qwen2.5-local', 2, 1),
            ('general',           2, 0.5, 0.4, 0.1, NULL, 'qwen-plus', 2, 0),
            ('coding',            3, 0.3, 0.6, 0.1, NULL, 'deepseek-v3', 2, 0),
            ('math_logic',        3, 0.3, 0.6, 0.1, NULL, 'deepseek-v3', 2, 0),
            ('complex_reasoning', 3, 0.2, 0.7, 0.1, NULL, 'qwen3-max', 2, 0),
            ('decompose',         1, 0.6, 0.3, 0.1, NULL, 'qwen-plus', 1, 1),
            ('aggregate',         2, 0.4, 0.5, 0.1, NULL, NULL, 2, 0)",
    )?;

    // 6b. P1.b 一次性：既有部署的 routing_policy seed 是 NULL pinned，不回填则推荐主选不生效。
    //     只填「当前 pinned_model 为 NULL」的推荐行——绝不覆盖用户已主动钦定的模型；
    //     新库经上面的 VALUES 已带值 → 此处 UPDATE 落空，天然幂等。
    let p1b_seeded: i64 = conn.query_row(
        "SELECT COUNT(*) FROM settings WHERE key = 'migration_policy_v1_p1b'",
        [],
        |r| r.get(0),
    )?;
    if p1b_seeded == 0 {
        let recs: &[(&str, &str)] = &[
            ("simple_qa", "qwen2.5-local"),
            ("general", "qwen-plus"),
            ("decompose", "qwen-plus"),
            ("coding", "deepseek-v3"),
            ("math_logic", "deepseek-v3"),
            ("complex_reasoning", "qwen3-max"),
        ];
        for (tt, m) in recs {
            conn.execute(
                "UPDATE routing_policy SET pinned_model = ?2, updated_at = CURRENT_TIMESTAMP
                 WHERE task_type = ?1 AND pinned_model IS NULL",
                params![tt, m],
            )?;
        }
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('migration_policy_v1_p1b', 'done')",
            [],
        )?;
    }

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
    // WAL 只允许单写者；不加 busy_timeout 时并发写会立刻报 "database is locked"
    // （路由回调 / 用量打点 / 信号回填常并发触发）。等待而非失败，避免瞬时锁竞争。
    conn.busy_timeout(std::time::Duration::from_millis(3000))?;
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
        Ok(_) => {
            let id = conn.last_insert_rowid();
            // P1.c：注册即有冷启动成效分——按 overlay quality_by_task / quality_score 预置各任务分。
            // INSERT OR IGNORE 幂等；失败不阻塞注册（成效分由后续 EWMA 慢慢补齐）。
            let _ = seed_cold_start_scores(&conn, &filled.name);
            Ok(id)
        }
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
    /// P2.b 语义缓存命中省下的金额（≈ 未命中时应花的 act_cost）。非命中恒 0。
    pub cache_saved_cost: f64,
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
                     "zone_multiplier", "conversation_id", "field_missing", "cache_saved_cost"]);
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
        vals.push(rusqlite::types::Value::Real(e.cache_saved_cost));
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
    // P5.c：真实生成（非缓存命中）且 task_type 已知时，把实际输出 token 滚入该角色 avg_out_tokens。
    //     缓存命中/探针（无 task_type）不入样本，避免拉低输出均值。
    if !cache_hit && output_tokens > 0 {
        if let Some(tt) = task_type {
            let _ = roll_avg_out_tokens(model_name, tt, output_tokens);
        }
    }
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
                SUM(cache_hit) as cache_hits,
                SUM(cache_saved_cost) as cache_saved
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
            cache_saved: row.get("cache_saved").unwrap_or(0.0),
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

/// P2.a 刷新更新：仅覆盖 `price_source != 'manual'` 的行（manual 为人工锚定，永不覆盖），
/// 覆盖后 source 标 `litellm_remote`、`price_stale=0`。cache_read 为 None 时保持原值（COALESCE）。
/// 返回是否命中更新（false = 行不存在或属 manual）。
pub fn refresh_price_spec(
    provider: &str,
    model: &str,
    input_cost: f64,
    output_cost: f64,
    cache_read_cost: Option<f64>,
) -> Result<bool> {
    let conn = open()?;
    let n = conn.execute(
        "UPDATE price_specs SET
            input_cost      = COALESCE(?3, input_cost),
            output_cost     = COALESCE(?4, output_cost),
            cache_read_cost = COALESCE(?5, cache_read_cost),
            price_source    = 'litellm_remote',
            price_updated_at = CURRENT_TIMESTAMP,
            price_stale     = 0,
            stale_reason    = NULL,
            effective_from  = COALESCE(effective_from, CURRENT_DATE)
         WHERE provider = ?1 AND model = ?2 AND price_source != 'manual'",
        params![provider, model, input_cost, output_cost, cache_read_cost],
    )?;
    Ok(n > 0)
}

/// P2.a 采纳刷新价：把指定行强制转正为 `manual`（人工确认远端价可信，此后不被刷新覆盖），
/// 价格保持现值不变。返回是否命中（行不存在则 false）。
pub fn accept_price_spec(provider: &str, model: &str) -> Result<bool> {
    let conn = open()?;
    let n = conn.execute(
        "UPDATE price_specs SET
            price_source    = 'manual',
            price_updated_at = CURRENT_TIMESTAMP,
            price_stale     = 0,
            stale_reason    = NULL
         WHERE provider = ?1 AND model = ?2",
        params![provider, model],
    )?;
    Ok(n > 0)
}

// ── Routing policy / score / audit (ROUTING-PLAN P0.c) ──

use crate::models::{ModelTaskScore, QualitySignalKind, RoutingPolicy};

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

/// P3：写入模型健康状态与检查时刻。仅状态变化由 `health.rs` 触发，非热路径。
pub fn set_model_health(name: &str, state: &str) -> Result<()> {
    let conn = open()?;
    conn.execute(
        "UPDATE models SET health_state = ?2, health_checked_at = CURRENT_TIMESTAMP WHERE name = ?1",
        params![name, state],
    )?;
    Ok(())
}

/// P3：routing overhead 聚合报告（routing_decisions.routing_ms）。
/// days=0 表示全部；返回 (条数, 均值ms, P95ms, maxms, 慢决策条数)。
/// （快路径 >10ms / 全路径 >100ms 视为实现 bug 上报；阈值由调用方解释。）
pub fn routing_overhead_report(days: i64) -> Result<(i64, f64, f64, f64, i64)> {
    let conn = open()?;
    let where_clause = if days > 0 {
        format!("WHERE created_at >= datetime('now', '-{days} days') AND routing_ms IS NOT NULL")
    } else {
        "WHERE routing_ms IS NOT NULL".to_string()
    };
    let (count, avg, max): (i64, f64, f64) = conn.query_row(
        &format!(
            "SELECT COUNT(*), COALESCE(AVG(routing_ms),0), COALESCE(MAX(routing_ms),0)
             FROM routing_decisions {where_clause}"
        ),
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    // P95（近似）：取降序第 ceil(0.95*count) 个值
    let p95: f64 = if count > 0 {
        let nth = ((count as f64) * 0.95).ceil().max(1.0) as i64;
        let idx = (nth - 1).max(0);
        conn.query_row(
            &format!(
                "SELECT routing_ms FROM routing_decisions {where_clause}
                 ORDER BY routing_ms DESC LIMIT 1 OFFSET ?1"
            ),
            params![idx],
            |r| r.get::<_, f64>(0),
        )
        .unwrap_or(0.0)
    } else {
        0.0
    };
    // 慢决策（>100ms）条数 —— 快路径超限的实现在调用方以断言/告警呈现
    let slow: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM routing_decisions {where_clause} AND routing_ms > 100.0"),
        [],
        |r| r.get(0),
    )?;
    Ok((count, avg, p95, max, slow))
}

/// P1.d 影子评测记录：一条「路由选择 × 强模型基线」双跑结果，供离线 AIQ 重放。
/// quality 两列留给裁判/离线脚本回填（开放式生成无结构化信号时不回填，判 NULL）。
pub fn insert_routing_calibration(
    task_type: &str,
    query_hash: &str,
    routed_model: &str,
    baseline_model: &str,
    routed_cost: f64,
    baseline_cost: f64,
    source: &str,
) -> Result<i64> {
    let conn = open()?;
    conn.execute(
        "INSERT INTO routing_calibration (task_type, query_hash, routed_model, baseline_model,
                                          routed_cost, baseline_cost, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            task_type,
            query_hash,
            routed_model,
            baseline_model,
            routed_cost,
            baseline_cost,
            source,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// P1.d：同一 (task_type, query_hash) 是否已有样本——重放去重，避免相同 query 重复膨胀样本数。
pub fn routing_calibration_exists(task_type: &str, query_hash: &str) -> Result<bool> {
    let conn = open()?;
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM routing_calibration WHERE task_type = ?1 AND query_hash = ?2",
        params![task_type, query_hash],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// P1.d：已采集的影子样本数（AIQ 重放/判定需要足够样本才有统计意义）。
pub fn count_routing_calibration() -> Result<i64> {
    let conn = open()?;
    let c = conn.query_row("SELECT COUNT(*) FROM routing_calibration", [], |r| r.get::<_, i64>(0))?;
    Ok(c)
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
            avg_out_tokens: row.get("avg_out_tokens").unwrap_or(500.0),
        })),
        None => Ok(None),
    }
}

/// P5.c：真实 output_tokens 滚入 (model, task_type) 的 avg_out_tokens（EWMA，α 同 signal.ewma_alpha）。
/// 仅由 insert_usage 在非缓存命中且 task_type 已知时调用——这是唯一能拿到真实输出 token 的入口。
/// 行不存在则首样本直接写入；存在则 `avg_out ← (1-α)·avg_out + α·actual`，不触碰质量 sample_count。
pub fn roll_avg_out_tokens(model_name: &str, task_type: &str, actual_out: i64) -> Result<()> {
    if actual_out <= 0 {
        return Ok(());
    }
    let alpha = get_setting("signal.ewma_alpha")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.15)
        .clamp(0.01, 0.5);
    let conn = open()?;
    conn.execute(
        "INSERT INTO model_task_score (model_name, task_type, avg_out_tokens, sample_count, updated_at)
         VALUES (?1, ?2, ?3, 0, CURRENT_TIMESTAMP)
         ON CONFLICT(model_name, task_type) DO UPDATE SET
            avg_out_tokens = (?3) * ?4 + avg_out_tokens * ?5,
            updated_at = CURRENT_TIMESTAMP",
        params![model_name, task_type, actual_out as f64, alpha, 1.0 - alpha],
    )?;
    Ok(())
}

/// P5.c：某 task_type 的保守输出 token 预估——将有充分样本（sample_count≥20）的模型的
/// avg_out_tokens 取平均作「真实均值」；无充分样本（冷启动）返回 500×1.5=750。
/// 门槛是硬上限语义：估低只少拦、不误拦；冷启动走高估系数更安全。
pub fn task_avg_out_tokens(task_type: &str) -> f64 {
    let conn = match open() {
        Ok(c) => c,
        Err(_) => return 750.0,
    };
    let mut stmt = match conn.prepare(
        "SELECT COALESCE(AVG(avg_out_tokens), -1.0) FROM model_task_score
         WHERE task_type = ?1 AND sample_count >= 20 AND avg_out_tokens > 0",
    ) {
        Ok(s) => s,
        Err(_) => return 750.0,
    };
    match stmt.query_row(params![task_type], |r| r.get::<_, f64>(0)) {
        Ok(v) if v > 0.0 => v,
        _ => 750.0, // 冷启动
    }
}

/// PR-5 §5.1：某 task_type 下各模型的缓存命中率（usage_records 真实 cached/prompt 平均，0..1）。
/// 无样本的模型不回填——`plan()` 缺省 0，不偏袒任何候选。
pub fn model_cache_hit_rate(task_type: &str) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    let conn = match open() {
        Ok(c) => c,
        Err(_) => return out,
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT model, AVG(CAST(COALESCE(cached_tokens,0) AS REAL) / MAX(prompt_tokens,1))
         FROM usage_records WHERE task_type = ?1 AND prompt_tokens > 0 GROUP BY model",
    ) else {
        return out;
    };
    let Ok(rows) = stmt.query_map(params![task_type], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
    }) else {
        return out;
    };
    for r in rows.flatten() {
        out.insert(r.0, r.1.clamp(0.0, 1.0));
    }
    out
}

/// PR-5 §5.2 会话亲和：某会话最近一次落库所用模型（usage_records 最新行）。None = 无记录。
pub fn recent_conversation_model(conversation_id: &str) -> Result<Option<String>> {
    let conn = open()?;
    let mut stmt = conn.prepare(
        "SELECT model FROM usage_records
         WHERE conversation_id = ?1 AND model IS NOT NULL AND model <> ''
         ORDER BY rowid DESC LIMIT 1",
    )?;
    let mut rows = stmt.query(params![conversation_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

/// P5.b/P5.a：全局预算剩余比 r=(max-spent)/max（clamp [0,1]）；无全局预算返回 None（→ normal）。
pub fn global_budget_ratio() -> Option<f64> {
    let budget = get_budget("global", "global").ok().flatten()?;
    if budget.max_budget <= 0.0 {
        return None;
    }
    let spent = get_total_spend(None, None, None).unwrap_or(0.0);
    Some(((budget.max_budget - spent) / budget.max_budget).clamp(0.0, 1.0))
}

/// P1.c 信号回填：`ewma_quality ← (1-α)·ewma + α·σ`，α 读 settings `signal.ewma_alpha`（默认 0.15）。
///
/// 副作用：按信号自增 success/fail/escalation 计数器；`sample_count>=20` 时解除模型保守期
/// （`needs_calibration=0`，系统写，绕开 update_model 白名单）。
///
/// 注意：σ 可能为负（失败/点踩），**只在写入后 clamp 结果**到 [0,1]（读侧合法性），
/// 不能 clamp 输入信号——否则负反馈会被误当作 0 丢弃，模型永远学不坏。
pub fn upsert_model_task_score_signal(
    model_name: &str,
    task_type: &str,
    kind: QualitySignalKind,
) -> Result<()> {
    let alpha = get_setting("signal.ewma_alpha")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.15)
        .clamp(0.01, 0.5);
    let sigma = kind.value();
    let (success, fail, escalation) = match kind {
        QualitySignalKind::Success => (1, 0, 0),
        QualitySignalKind::Escalation => (0, 0, 1),
        QualitySignalKind::SubtaskFail
        | QualitySignalKind::ModelRegen
        | QualitySignalKind::Reask
        | QualitySignalKind::Dislike
        | QualitySignalKind::ParseFail => (0, 1, 0),
        // 点赞不改变成败计数（与成败正交的独立信号）
        QualitySignalKind::Like => (0, 0, 0),
    };

    let conn = open()?;
    // 首样本直接写 σ（clamp 到 [0,1] 保证读侧合法）；后续走 EWMA。
    // `ewma_quality` 读写均 clamp：MIN/MAX 在此作为标量函数（非聚合）。
    conn.execute(
        "INSERT INTO model_task_score (model_name, task_type, success_count, fail_count,
            escalation_count, ewma_quality, sample_count, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, CURRENT_TIMESTAMP)
         ON CONFLICT(model_name, task_type) DO UPDATE SET
            ewma_quality    = MIN(1.0, MAX(0.0, ewma_quality * ?7 + ?8 * ?9)),
            success_count   = success_count + ?3,
            fail_count      = fail_count + ?4,
            escalation_count = escalation_count + ?5,
            sample_count    = sample_count + 1,
            updated_at      = CURRENT_TIMESTAMP",
        params![
            model_name,
            task_type,
            success,
            fail,
            escalation,
            sigma.clamp(0.0, 1.0),
            1.0 - alpha,
            sigma,
            alpha,
        ],
    )?;

    // 保守期解除：sample>=20 的模型在复杂任务上不再扣分（P0.d 的 needs_calibration 罚分）。
    let sample: i64 = conn.query_row(
        "SELECT sample_count FROM model_task_score WHERE model_name = ?1 AND task_type = ?2",
        params![model_name, task_type],
        |r| r.get(0),
    )?;
    if sample >= 20 {
        conn.execute(
            "UPDATE models SET needs_calibration = 0 WHERE name = ?1 AND needs_calibration = 1",
            params![model_name],
        )?;
    }
    Ok(())
}

/// P1.c 冷启动：为模型在各任务类型预置 `model_task_score` 行，`ewma_quality` 用 overlay
/// `quality_by_task` 的榜单折算分（按任务分别给分，如 coding 0.8 ≠ math 0.5），缺省回落
/// 模型 `quality_score`。`INSERT OR IGNORE` 幂等——只补缺，不覆盖任何已在线学习的成效分。
fn seed_cold_start_scores(conn: &Connection, model_name: &str) -> Result<usize> {
    let m = get_model(model_name)?;
    let tasks: Vec<String> = conn
        .prepare("SELECT task_type FROM routing_policy WHERE task_type IS NOT NULL")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<_, _>>()?;
    let mut seeded = 0usize;
    for t in tasks {
        let cold = crate::metadata::cold_start_quality(&m, &t).unwrap_or(m.quality_score);
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO model_task_score
                (model_name, task_type, ewma_quality, sample_count, updated_at)
             VALUES (?1, ?2, ?3, 0, CURRENT_TIMESTAMP)",
            params![model_name, t, cold.clamp(0.0, 1.0)],
        )?;
        seeded += inserted as usize;
    }
    Ok(seeded)
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

pub fn upsert_budget(
    scope: &str,
    scope_id: &str,
    max_budget: f64,
    duration: &str,
    scope_task_type: Option<&str>,
    soft_limit_ratio: Option<f64>,
    action_on_exceed: Option<&str>,
) -> Result<()> {
    let conn = open()?;
    conn.execute(
        "INSERT INTO budgets (scope, scope_id, max_budget, duration, scope_task_type, soft_limit_ratio, action_on_exceed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(scope, scope_id) DO UPDATE SET
            max_budget = excluded.max_budget,
            duration = excluded.duration,
            scope_task_type = COALESCE(excluded.scope_task_type, budgets.scope_task_type),
            soft_limit_ratio = COALESCE(excluded.soft_limit_ratio, budgets.soft_limit_ratio),
            action_on_exceed = COALESCE(excluded.action_on_exceed, budgets.action_on_exceed)",
        params![
            scope,
            scope_id,
            max_budget,
            duration,
            scope_task_type,
            soft_limit_ratio.map(|v| v as f64),
            action_on_exceed,
        ],
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
            scope_task_type: row.get("scope_task_type").ok().flatten(),
            soft_limit_ratio: row.get("soft_limit_ratio").ok().flatten(),
            action_on_exceed: row.get("action_on_exceed").ok().flatten(),
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
            scope_task_type: row.get("scope_task_type").ok().flatten(),
            soft_limit_ratio: row.get("soft_limit_ratio").ok().flatten(),
            action_on_exceed: row.get("action_on_exceed").ok().flatten(),
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
    use std::sync::Mutex;

    // DB 集成测试各自使用独立专属临时目录 + 用毕还原全局 env，避免进程级
    // LLOOM_DATA_DIR 泄漏与并行测试竞态；SQLite `PRAGMA journal_mode=WAL` 不走
    // busy handler，并发写者会立刻 SQLITE_BUSY，故仍以 DB_LOCK 串行化写库测试。
    static DB_LOCK: Mutex<()> = Mutex::new(());

    /// init_db 在并行测试下可能与并发读连接竞态：`PRAGMA journal_mode=WAL` 需要排它锁，
    /// 而并行测试若经 `db::get_setting`/`extract` 等路径在读迁移目录的共享锁，会瞬时
    /// SQLITE_BUSY。串行化写测试 + 用毕还原 env 已消除大部分竞争，这里再对瞬时 busy
    /// 短重试，保证 `cargo test` 稳定全绿。
    fn init_db_retry() {
        for _ in 0..30 {
            if init_db().is_ok() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        init_db().expect("init_db keeps failing with transient lock contention");
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
    /// 独立专属临时目录 + 用毕还原 env，避免共享 LLOOM_DATA_DIR 与并行测试竞态。
    /// 验证：量纲 ÷10、7 新列、投影、deepseek zone 预置、幂等（不二次除）、
    /// 投影后的 PriceSpec 参与实际成本计算。
    #[test]
    fn migration_is_idempotent_and_fixes_scale() {
        let _guard = DB_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("lloom_migration_scale_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let prev = std::env::var("LLOOM_DATA_DIR").ok();
        std::env::set_var("LLOOM_DATA_DIR", &dir);
        // 旧库：SCHEMA 建表，不设 migration 标记
        let conn = open().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        insert_dashscope_model(&conn);
        drop(conn);

        // 升级路径：init_db → migrate
        init_db_retry();
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
        init_db_retry();
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
                cache_saved_cost: 0.0,
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

        // P2.b 冒烟：cache_saved_cost 列存在 + 命中行的节省可被聚合读出
        let p2_cols = table_columns(&open().unwrap(), "usage_records").unwrap();
        assert!(p2_cols.contains(&"cache_saved_cost".to_string()), "missing P2.b cache_saved_cost column");
        insert_usage(
            "qwen-plus-test", "default", 200, 40, 0.0,
            Some("coding"), true, None, Some("chat-cache-smoke"),
            Some(&UsageExtra {
                cached_tokens: 0, reasoning_tokens: 0, est_cost: 0.0, act_cost: 0.0,
                zone_multiplier: 1.0, conversation_id: None, field_missing: false,
                cache_saved_cost: 3.33e-05,
            }),
        )
        .unwrap();
        let saved_sum: f64 = get_usage_stats(None, None, None)
            .unwrap()
            .iter()
            .map(|s| s.cache_saved)
            .sum();
        assert!(
            saved_sum >= 3.33e-05,
            "cache_saved_cost 应被 SUM 聚合读出，got {saved_sum}"
        );

        // P1.b 冒烟：推荐主选已 seed；聚合不钦定（按评分择优）；用户钦定不被一次性回填覆盖
        let pin = |tt: &str| -> Option<String> {
            open().unwrap()
                .query_row(
                    "SELECT pinned_model FROM routing_policy WHERE task_type = ?1",
                    [tt],
                    |r| r.get(0),
                )
                .unwrap()
        };
        assert_eq!(pin("coding").as_deref(), Some("deepseek-v3"), "coding 推荐主选");
        assert_eq!(pin("complex_reasoning").as_deref(), Some("qwen3-max"), "复杂推理 ≤32K 推荐主选");
        assert_eq!(pin("general").as_deref(), Some("qwen-plus"));
        assert!(pin("aggregate").is_none(), "聚合应按评分择优，不钦定");
        // 用户钦定 general --> 自定义模型，重新 init_db 不得覆盖
        open().unwrap()
            .execute(
                "UPDATE routing_policy SET pinned_model = 'custom-llm' WHERE task_type = 'general'",
                [],
            )
            .unwrap();
        init_db_retry();
        assert_eq!(pin("general").as_deref(), Some("custom-llm"), "用户钦定应保留");

        // 用毕还原全局 env，避免泄漏破坏其他并行测试；再清理临时目录
        match prev {
            Some(v) => std::env::set_var("LLOOM_DATA_DIR", v),
            None => std::env::remove_var("LLOOM_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// P1.c：upsert_model_task_score_signal 的 EWMA 行为。
    /// 验证：①首样本直接写 σ；②负信号被采用（非 clamp 成 0），按公式拉低；
    /// ③success/fail/escalation 计数器各自自增；④sample≥20 系统写解除保守期；⑤α 从 settings 可调。
    #[test]
    fn ewma_signal_accumulates_and_releases_calibration() {
        // 本用例自己建独立 data dir（不共享 migration 测试的目录）：migration 用例需要
        // "纯净旧库" 才测得出 ÷10 路径，共享会让两个断言互相污染。DB_LOCK 已把两个
        // 写库测试串行化，故在此安全地覆盖全局 env、用毕还原。
        let _guard = DB_LOCK.lock().unwrap();
        let dir_text = format!("lloom_ewma_test_{}", std::process::id());
        let dir = std::env::temp_dir().join(&dir_text);
        let _ = std::fs::remove_dir_all(&dir);
        let prev = std::env::var("LLOOM_DATA_DIR").ok();
        std::env::set_var("LLOOM_DATA_DIR", &dir);

        // 确保 schema 存在（settings/routing_policy/model_task_score），init_db 幂等
        init_db_retry();
        // 固定默认 α，避免其它用例残留的自定义 α 污染本用例
        set_setting("signal.ewma_alpha", "0.15").unwrap();
        // 唯一模型名 + 前置清理，避免与历史数据纠缠（用完立即 drop，防 SQLite 写锁）
        let model_name = format!("p1c-ewma-{}", std::process::id());
        let conn = open().unwrap();
        conn.execute("DELETE FROM model_task_score WHERE model_name = ?1", params![model_name]).unwrap();
        conn.execute("DELETE FROM models WHERE name = ?1", params![model_name]).unwrap();
        conn.execute(
            "INSERT INTO models
                (name, provider, litellm_model, capability_tier, needs_calibration, is_active)
             VALUES (?1, 'test', ?1, 2, 1, 1)",
            params![model_name],
        )
        .unwrap();
        drop(conn);

        // ① 首样本 Success → ewma 直接写 σ=0.7，success_count=1
        upsert_model_task_score_signal(&model_name, "general", QualitySignalKind::Success).unwrap();
        let row = get_model_task_score(&model_name, "general").unwrap().unwrap();
        assert_eq!(row.sample_count, 1);
        assert!((row.ewma_quality - 0.7).abs() < 1e-9, "首样本直接写 σ，得 {}", row.ewma_quality);
        assert_eq!(row.success_count, 1);
        assert_eq!(row.fail_count, 0);

        // ② SubtaskFail(−0.5)：ewma = 0.7·0.85 + (−0.5)·0.15 = 0.52；负信号被采用
        upsert_model_task_score_signal(&model_name, "general", QualitySignalKind::SubtaskFail).unwrap();
        let row = get_model_task_score(&model_name, "general").unwrap().unwrap();
        let expect = 0.7 * 0.85 + (-0.5) * 0.15;
        assert!(
            (row.ewma_quality - expect).abs() < 1e-9,
            "负信号应参与 EWMA：{} != {expect}（若被误 clamp 则为 0.595）",
            row.ewma_quality
        );
        assert_eq!(row.sample_count, 2);
        assert_eq!(row.fail_count, 1);
        assert_eq!(row.escalation_count, 0);

        // ③ Escalation 只加 escalation_count，不扰动成败计数
        upsert_model_task_score_signal(&model_name, "general", QualitySignalKind::Escalation).unwrap();
        let row = get_model_task_score(&model_name, "general").unwrap().unwrap();
        assert_eq!(row.escalation_count, 1);
        assert_eq!(row.success_count, 1);
        assert_eq!(row.fail_count, 1);
        assert_eq!(row.sample_count, 3);

        // ④ 补足样本到 ≥20 → 解除保守期（needs_calibration 由系统写 0）
        for _ in 0..17 {
            upsert_model_task_score_signal(&model_name, "general", QualitySignalKind::Success).unwrap();
        }
        let row = get_model_task_score(&model_name, "general").unwrap().unwrap();
        assert!(row.sample_count >= 20, "样本数 {}", row.sample_count);
        assert_eq!(get_model(&model_name).unwrap().needs_calibration, 0, "sample>=20 应解除保守期");

        // ⑤ α 可调：设 α=0.5，一次 Success 后新值 = 0.5·旧值 + 0.5·0.7
        set_setting("signal.ewma_alpha", "0.5").unwrap();
        let before = get_model_task_score(&model_name, "general").unwrap().unwrap().ewma_quality;
        upsert_model_task_score_signal(&model_name, "general", QualitySignalKind::Success).unwrap();
        let after = get_model_task_score(&model_name, "general").unwrap().unwrap().ewma_quality;
        assert!(
            (after - (before * 0.5 + 0.7 * 0.5)).abs() < 1e-9,
            "α=0.5 公式：{after} != {}", before * 0.5 + 0.7 * 0.5
        );

        // P1.d：影子评测记录往返（插入 + 计数）——复用同一独立库，规避额外写库测试
        let cal_id = insert_routing_calibration(
            "coding",
            "abc123",
            "deepseek-v3",
            "qwen3-max",
            1.7e-5,
            2.2e-5,
            "shadow",
        )
        .unwrap();
        assert!(cal_id > 0, "影子记录插入应返回自增 id");
        assert_eq!(count_routing_calibration().unwrap(), 1, "影子样本数应为 1");

        // 还原全局 env，并按序清理锁（guard 兜底解锁）
        match prev {
            Some(v) => std::env::set_var("LLOOM_DATA_DIR", v),
            None => std::env::remove_var("LLOOM_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// P3：routing_overhead_report 聚合（count/avg/p95/max/slow），独立数据目录。
    #[test]
    fn routing_overhead_report_aggregates() {
        let _guard = DB_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("lloom_oh_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let prev = std::env::var("LLOOM_DATA_DIR").ok();
        std::env::set_var("LLOOM_DATA_DIR", &dir);
        init_db_retry(); // 幂等建全部 schema，含 routing_decisions

        // 唯一请求号 + 前置清空，避免历史纠缠
        let conn = open().unwrap();
        conn.execute("DELETE FROM routing_decisions", []).unwrap();
        for (i, ms) in [2.0f64, 3.0, 4.0, 5.0, 6.0, 150.0].iter().enumerate() {
            insert_routing_decision(
                &format!("oh-req-{}", i), "chat", "medium", "", "", "m", "", *ms,
            )
            .unwrap_or_else(|e| panic!("insert failed: {e}"));
        }
        drop(conn);

        let (count, avg, p95, max, slow) = routing_overhead_report(0).unwrap();
        assert_eq!(count, 6, "条数=6");
        let expected_avg = (2.0 + 3.0 + 4.0 + 5.0 + 6.0 + 150.0) / 6.0;
        assert!((avg - expected_avg).abs() < 1e-9, "avg={avg}");
        assert!((max - 150.0).abs() < 1e-9, "max={max}");
        assert_eq!(slow, 1, "仅 150>100 记慢");
        // P95：降序 [150,6,5,4,3,2]，第 ceil(0.95*6)=6 个 = 2.0
        assert!((p95 - 2.0).abs() < 1e-9, "p95={p95}");

        match prev {
            Some(v) => std::env::set_var("LLOOM_DATA_DIR", v),
            None => std::env::remove_var("LLOOM_DATA_DIR"),
        }
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
