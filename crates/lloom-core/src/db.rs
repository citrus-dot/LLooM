//! SQLite database layer — schema, CRUD, and query helpers.
//! Strongly-typed port of `core/database.py`.

use crate::error::{AppError, Result};
use crate::models::{
    ApiKeyRef, Backend, Budget, LocalCompat, Model, ModelTaskScore, Provider, QualitySignalKind,
    RoutingPolicy, UsageStats,
};
use crate::pricing::{PriceSpec, TierBand, Zone};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct ModelRow {
    pub id: i64,
    pub name: String,
    pub provider: String,
    pub litellm_model: String,
    pub api_base: String,
    pub api_key_env: String,
    pub task_type: String,
    pub input_cost_per_token: f64,
    pub output_cost_per_token: f64,
    pub rpm: i64,
    pub is_active: i64,
    pub capability_tier: i64,
    pub quality_score: f64,
    pub context_window: i64,
    pub supports_tools: i64,
    pub supports_vision: i64,
    pub supports_stream: i64,
    pub is_local: i64,
    pub priority: i64,
    pub health_state: String,
    pub needs_calibration: i64,
}

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
    cached_tokens INTEGER DEFAULT 0,
    reasoning_tokens INTEGER DEFAULT 0,
    est_cost REAL DEFAULT 0,
    act_cost REAL DEFAULT 0,
    zone_multiplier REAL DEFAULT 1.0,
    conversation_id TEXT,
    field_missing INTEGER DEFAULT 0,
    cache_saved_cost REAL DEFAULT 0,
    api_source TEXT DEFAULT 'webui',
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

-- ── PR-x 第三方参考价（OpenRouter 参考层；独立于 price_specs，永不参与计价） ──
CREATE TABLE IF NOT EXISTS price_reference (
    provider      TEXT NOT NULL,
    model         TEXT NOT NULL,
    ref_source    TEXT NOT NULL DEFAULT 'openrouter',
    ref_model_id  TEXT NOT NULL,
    input_cost    REAL NOT NULL,
    output_cost   REAL NOT NULL,
    fetched_at    TIMESTAMP,
    PRIMARY KEY (provider, model, ref_source)
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
    avg_out_tokens REAL DEFAULT 500,
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
CREATE TABLE IF NOT EXISTS policy_review (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    samples INTEGER,
    weak_cost REAL, weak_quality REAL,
    cur_cost REAL, cur_quality REAL,
    strong_cost REAL, strong_quality REAL,
    aiq REAL, saved_pct REAL,
    conclusion TEXT,
    budget_tiers_json TEXT DEFAULT '{}',
    suggestions_json TEXT DEFAULT '[]'
);
"#;

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
    /// C3 流量来源：`Some("proxy")` 为 OpenAI 兼容代理流量；None 走列默认 `'webui'`。
    pub api_source: Option<String>,
}

// ── PR-x 第三方参考价（OpenRouter 参考层，独立表，不进 price_specs） ──

/// 单条参考价 + 与本地图价的偏差（读取时联算，不落库）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct PriceReferenceView {
    pub provider: String,
    pub model: String,
    pub ref_source: String,
    pub ref_model_id: String,
    pub ref_input_cost: f64,
    pub ref_output_cost: f64,
    pub spec_input_cost: Option<f64>,
    pub spec_output_cost: Option<f64>,
    /// (spec - ref) / ref × 100，None = 本地无价或参考价无效
    pub dev_input_pct: Option<f64>,
    pub dev_output_pct: Option<f64>,
    pub fetched_at: Option<String>,
}

// ── 按日聚合用量（PRICING-PLAN §6.2 校准燃料） ──

/// 排除探针记账（task_type='probe'）。通过 price_specs join 补出 (provider, model)；
/// 无 PriceSpec 的模型（本地/未登记）不计入。
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

// ── 写入参数聚合（把过长的参数表收进结构体，替代 too_many_arguments） ──

/// `Db::insert_usage` 的入参。
#[derive(Debug, Clone)]
pub struct UsageRecord<'a> {
    pub model_name: &'a str,
    pub user_id: &'a str,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost: f64,
    pub task_type: Option<&'a str>,
    pub cache_hit: bool,
    pub latency_ms: Option<f64>,
    pub request_id: Option<&'a str>,
    pub extra: Option<UsageExtra>,
}

/// `Db::upsert_price_spec` 的入参。
#[derive(Debug, Clone)]
pub struct PriceSpecInput<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub input_cost: f64,
    pub output_cost: f64,
    pub cache_read_cost: Option<f64>,
    pub cache_write_cost: Option<f64>,
    pub reasoning_cost: Option<f64>,
    pub tiered_json: Option<&'a str>,
    pub zone_ref: Option<&'a str>,
    pub cny_list_price_json: Option<&'a str>,
}

/// `Db::insert_routing_decision` 的入参。
#[derive(Debug, Clone)]
pub struct RoutingDecisionRecord<'a> {
    pub request_id: &'a str,
    pub task_type: &'a str,
    pub band: &'a str,
    pub signals_json: &'a str,
    pub candidates_json: &'a str,
    pub selected: &'a str,
    pub fallback_chain: &'a str,
    pub routing_ms: f64,
}

/// `Db::insert_routing_calibration` 的入参。
#[derive(Debug, Clone)]
pub struct RoutingCalibrationInput<'a> {
    pub task_type: &'a str,
    pub query_hash: &'a str,
    pub routed_model: &'a str,
    pub baseline_model: &'a str,
    pub routed_cost: f64,
    pub baseline_cost: f64,
    pub source: &'a str,
}

/// `Db::insert_policy_review` 的入参。
#[derive(Debug, Clone)]
pub struct PolicyReviewInput<'a> {
    pub samples: i64,
    pub weak_cost: f64,
    pub weak_quality: f64,
    pub cur_cost: f64,
    pub cur_quality: f64,
    pub strong_cost: f64,
    pub strong_quality: f64,
    pub aiq: f64,
    pub saved_pct: f64,
    pub conclusion: &'a str,
    pub budget_tiers_json: &'a str,
    pub suggestions_json: &'a str,
}

/// `Db::upsert_budget` 的入参。
#[derive(Debug, Clone)]
pub struct BudgetInput<'a> {
    pub scope: &'a str,
    pub scope_id: &'a str,
    pub max_budget: f64,
    pub duration: &'a str,
    pub scope_task_type: Option<&'a str>,
    pub soft_limit_ratio: Option<f64>,
    pub action_on_exceed: Option<&'a str>,
}

#[derive(Clone)]
pub struct Db {
    pool: Pool<SqliteConnectionManager>,
}

impl Db {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let manager = SqliteConnectionManager::file(path).with_init(|c| {
            c.execute_batch("PRAGMA journal_mode=WAL;")?;
            c.execute_batch("PRAGMA foreign_keys=ON;")?;
            c.busy_timeout(std::time::Duration::from_millis(3000))?;
            Ok(())
        });
        let pool = Pool::builder().max_size(8).build(manager)?;
        // schema 只建一次：连接池初始化时执行。
        pool.get()?.execute_batch(SCHEMA)?;
        Ok(Self { pool })
    }

    /// 从连接池取一个连接（Deref 到 `rusqlite::Connection`）。
    pub fn conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>> {
        Ok(self.pool.get()?)
    }

    // ── Model CRUD ──

    pub fn insert_model(&self, m: &Model) -> Result<()> {
        let conn = self.conn()?;
        let mut filled = m.clone();
        let _report = crate::metadata::resolve_and_fill(&mut filled);
        validate_cost(filled.input_cost_per_token, filled.output_cost_per_token)?;
        let row = ModelRow::from(&filled);
        let res = conn.execute(
            "INSERT INTO models (name, provider, litellm_model, api_base, api_key_env, task_type,
                             input_cost_per_token, output_cost_per_token, rpm, is_active,
                             capability_tier, quality_score, context_window,
                             supports_tools, supports_vision, supports_stream,
                             is_local, priority, needs_calibration)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                 ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            params![
                row.name,
                row.provider,
                row.litellm_model,
                row.api_base,
                row.api_key_env,
                row.task_type,
                row.input_cost_per_token,
                row.output_cost_per_token,
                row.rpm,
                row.is_active,
                row.capability_tier,
                row.quality_score,
                row.context_window,
                row.supports_tools,
                row.supports_vision,
                row.supports_stream,
                row.is_local,
                row.priority,
                row.needs_calibration,
            ],
        );
        match res {
            Ok(_) => {
                let _ = self.seed_cold_start_scores(&filled.name);
                Ok(())
            }
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(AppError::Conflict(m.name.clone()))
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn upsert_model(&self, m: &Model) -> Result<()> {
        let conn = self.conn()?;
        let mut filled = m.clone();
        let _report = crate::metadata::resolve_and_fill(&mut filled);
        validate_cost(filled.input_cost_per_token, filled.output_cost_per_token)?;
        let row = ModelRow::from(&filled);
        let res = conn.execute(
            "INSERT INTO models (name, provider, litellm_model, api_base, api_key_env, task_type,
                             input_cost_per_token, output_cost_per_token, rpm, is_active,
                             capability_tier, quality_score, context_window,
                             supports_tools, supports_vision, supports_stream,
                             is_local, priority, needs_calibration)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                 ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
         ON CONFLICT(name) DO UPDATE SET
             provider = excluded.provider,
             litellm_model = excluded.litellm_model,
             api_base = excluded.api_base,
             api_key_env = excluded.api_key_env,
             task_type = excluded.task_type,
             input_cost_per_token = excluded.input_cost_per_token,
             output_cost_per_token = excluded.output_cost_per_token,
             rpm = excluded.rpm,
             is_active = excluded.is_active,
             capability_tier = excluded.capability_tier,
             quality_score = excluded.quality_score,
             context_window = excluded.context_window,
             supports_tools = excluded.supports_tools,
             supports_vision = excluded.supports_vision,
             supports_stream = excluded.supports_stream,
             is_local = excluded.is_local,
             priority = excluded.priority,
             needs_calibration = excluded.needs_calibration",
            params![
                row.name,
                row.provider,
                row.litellm_model,
                row.api_base,
                row.api_key_env,
                row.task_type,
                row.input_cost_per_token,
                row.output_cost_per_token,
                row.rpm,
                row.is_active,
                row.capability_tier,
                row.quality_score,
                row.context_window,
                row.supports_tools,
                row.supports_vision,
                row.supports_stream,
                row.is_local,
                row.priority,
                row.needs_calibration,
            ],
        );
        match res {
            Ok(_) => {
                let _ = self.seed_cold_start_scores(&filled.name);
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn find_model(&self, name: &str, active_only: bool) -> Result<Model> {
        let conn = self.conn()?;
        let sql = if active_only {
            "SELECT * FROM models WHERE name = ?1 AND is_active = 1"
        } else {
            "SELECT * FROM models WHERE name = ?1"
        };
        let mut stmt = conn.prepare(sql)?;
        stmt.query_row(params![name], |r| Ok(Model::from(model_from_row(r)?)))
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => AppError::NotFound(name.to_string()),
                other => other.into(),
            })
    }

    pub fn list_models(&self, active_only: bool) -> Result<Vec<Model>> {
        let conn = self.conn()?;
        let q = if active_only {
            "SELECT * FROM models WHERE is_active = 1 ORDER BY name"
        } else {
            "SELECT * FROM models ORDER BY name"
        };
        let mut stmt = conn.prepare(q)?;
        let rows = stmt.query_map([], |r| model_from_row(r).map(Model::from))?;
        rows.collect::<rusqlite::Result<Vec<Model>>>()
            .map_err(Into::into)
    }

    pub fn deactivate_model(&self, name: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE models SET is_active = 0 WHERE name = ?1",
            params![name],
        )
        .map(|_| ())
        .map_err(|x| x.into())
    }

    // ── Usage ──

    /// Insert one usage_records row. `latency_ms`/`request_id` (P1.a) are optional
    /// for backward compatibility — old callers pass None.
    pub fn insert_usage(&self, rec: &UsageRecord<'_>) -> Result<()> {
        let conn = self.conn()?;
        let mut cols: Vec<&str> = vec![
            "model_name",
            "user_id",
            "input_tokens",
            "output_tokens",
            "cost",
            "task_type",
            "cache_hit",
        ];
        let mut vals: Vec<rusqlite::types::Value> = vec![
            rusqlite::types::Value::Text(rec.model_name.to_string()),
            rusqlite::types::Value::Text(rec.user_id.to_string()),
            rusqlite::types::Value::Integer(rec.input_tokens),
            rusqlite::types::Value::Integer(rec.output_tokens),
            rusqlite::types::Value::Real(rec.cost),
            match rec.task_type {
                Some(t) => rusqlite::types::Value::Text(t.to_string()),
                None => rusqlite::types::Value::Null,
            },
            rusqlite::types::Value::Integer(if rec.cache_hit { 1 } else { 0 }),
        ];
        if let Some(e) = &rec.extra {
            cols.extend([
                "cached_tokens",
                "reasoning_tokens",
                "est_cost",
                "act_cost",
                "zone_multiplier",
                "conversation_id",
                "field_missing",
                "cache_saved_cost",
            ]);
            vals.push(rusqlite::types::Value::Integer(e.cached_tokens));
            vals.push(rusqlite::types::Value::Integer(e.reasoning_tokens));
            vals.push(rusqlite::types::Value::Real(e.est_cost));
            vals.push(rusqlite::types::Value::Real(e.act_cost));
            vals.push(rusqlite::types::Value::Real(e.zone_multiplier));
            vals.push(match &e.conversation_id {
                Some(c) => rusqlite::types::Value::Text(c.clone()),
                None => rusqlite::types::Value::Null,
            });
            vals.push(rusqlite::types::Value::Integer(if e.field_missing {
                1
            } else {
                0
            }));
            vals.push(rusqlite::types::Value::Real(e.cache_saved_cost));
            // C3：仅显式标记来源时写列（None 落库默认 'webui'，旧行为不变）
            if let Some(src) = &e.api_source {
                cols.push("api_source");
                vals.push(rusqlite::types::Value::Text(src.clone()));
            }
        }
        cols.extend(["latency_ms", "request_id"]);
        vals.push(match rec.latency_ms {
            Some(l) => rusqlite::types::Value::Real(l),
            None => rusqlite::types::Value::Null,
        });
        vals.push(match rec.request_id {
            Some(r) => rusqlite::types::Value::Text(r.to_string()),
            None => rusqlite::types::Value::Null,
        });
        let placeholders: Vec<String> = (1..=vals.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "INSERT INTO usage_records ({}) VALUES ({})",
            cols.join(", "),
            placeholders.join(", ")
        );
        conn.execute(&sql, rusqlite::params_from_iter(vals.iter().cloned()))?;
        // P5.c：真实生成（非缓存命中）且 task_type 已知时，把实际输出 token 滚入该角色 avg_out_tokens。
        //     缓存命中/探针（无 task_type）不入样本，避免拉低输出均值。
        if !rec.cache_hit && rec.output_tokens > 0 {
            if let Some(tt) = rec.task_type {
                let _ = self.roll_avg_out_tokens(rec.model_name, tt, rec.output_tokens);
            }
        }
        Ok(())
    }

    pub fn get_usage_stats(
        &self,
        model_name: Option<&str>,
        user_id: Option<&str>,
        since: Option<&str>,
    ) -> Result<Vec<UsageStats>> {
        let conn = self.conn()?;
        let mut sql = String::from(
            "SELECT model_name,
                SUM(input_tokens) as total_input_tokens,
                SUM(output_tokens) as total_output_tokens,
                SUM(cost) as total_cost,
                COUNT(*) as request_count,
                SUM(cache_hit) as cache_hits,
                SUM(cache_saved_cost) as cache_saved
         FROM usage_records WHERE cost >= 0",
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

    pub fn get_total_spend(
        &self,
        user_id: Option<&str>,
        model_name: Option<&str>,
        since: Option<&str>,
    ) -> Result<f64> {
        let conn = self.conn()?;
        let mut sql = String::from(
            "SELECT COALESCE(SUM(cost), 0.0) as total FROM usage_records WHERE cost >= 0",
        );
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
        let total = conn.query_row(
            &sql,
            rusqlite::params_from_iter(vals.iter().cloned()),
            |row| row.get::<_, f64>(0),
        )?;
        Ok(total)
    }

    // ── Settings (key/value store) ──

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query_map(params![key], |row| row.get::<_, String>(0))?;
        Ok(rows.next().transpose()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn()?;
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
        &self,
        sim: f64,
        decision: &str,
        model: &str,
        label: Option<bool>,
        source: &str,
    ) -> Result<()> {
        let conn = self.conn()?;
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
    pub fn calibration_labeled_samples(&self) -> Result<Vec<(f64, bool)>> {
        let conn = self.conn()?;
        let mut stmt =
            conn.prepare("SELECT sim, label FROM cache_calibration WHERE label IS NOT NULL")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, f64>(0)?, row.get::<_, i64>(1)? == 1))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    // ── Pricing (PRICING-PLAN §3.2 / §4) ──

    pub fn get_price_spec(&self, provider: &str, model: &str) -> Result<Option<PriceSpec>> {
        let conn = self.conn()?;
        let mut stmt =
            conn.prepare("SELECT * FROM price_specs WHERE provider = ?1 AND model = ?2")?;
        let mut rows = stmt.query(params![provider, model])?;
        match rows.next()? {
            Some(row) => Ok(Some(price_spec_from_row(row)?)),
            None => Ok(None),
        }
    }

    pub fn list_price_specs(&self) -> Result<Vec<PriceSpec>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT * FROM price_specs ORDER BY provider, model")?;
        let rows = stmt.query_map([], price_spec_from_row)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn mark_price_stale(
        &self,
        provider: &str,
        model: &str,
        stale: bool,
        reason: &str,
    ) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE price_specs SET price_stale = ?3, stale_reason = ?4 WHERE provider = ?1 AND model = ?2",
            params![provider, model, if stale { 1 } else { 0 }, reason],
        )?;
        Ok(())
    }

    pub fn upsert_price_reference(
        &self,
        provider: &str,
        model: &str,
        ref_source: &str,
        ref_model_id: &str,
        input_cost: f64,
        output_cost: f64,
    ) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO price_reference (provider, model, ref_source, ref_model_id, input_cost, output_cost, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, CURRENT_TIMESTAMP)
         ON CONFLICT(provider, model, ref_source) DO UPDATE SET
           ref_model_id = excluded.ref_model_id,
           input_cost = excluded.input_cost,
           output_cost = excluded.output_cost,
           fetched_at = excluded.fetched_at",
            params![provider, model, ref_source, ref_model_id, input_cost, output_cost],
        )?;
        Ok(())
    }

    /// 参考价 × 本地 specs 联表视图（含偏差百分比），定价页直接消费。
    pub fn list_price_references(&self) -> Result<Vec<PriceReferenceView>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT r.provider, r.model, r.ref_source, r.ref_model_id,
                r.input_cost, r.output_cost, r.fetched_at,
                s.input_cost, s.output_cost
         FROM price_reference r
         LEFT JOIN price_specs s ON s.provider = r.provider AND s.model = r.model
         ORDER BY r.provider, r.model",
        )?;
        let rows = stmt.query_map([], |row| {
            let ref_in: f64 = row.get(4)?;
            let ref_out: f64 = row.get(5)?;
            let spec_in: Option<f64> = row.get(7)?;
            let spec_out: Option<f64> = row.get(8)?;
            let dev = |spec: Option<f64>, reference: f64| -> Option<f64> {
                if reference > 0.0 {
                    spec.map(|s| (s - reference) / reference * 100.0)
                } else {
                    None
                }
            };
            Ok(PriceReferenceView {
                provider: row.get(0)?,
                model: row.get(1)?,
                ref_source: row.get(2)?,
                ref_model_id: row.get(3)?,
                ref_input_cost: ref_in,
                ref_output_cost: ref_out,
                spec_input_cost: spec_in,
                spec_output_cost: spec_out,
                dev_input_pct: dev(spec_in, ref_in),
                dev_output_pct: dev(spec_out, ref_out),
                fetched_at: row.get::<_, Option<String>>(6)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn list_provider_zones(&self) -> Result<Vec<Zone>> {
        let conn = self.conn()?;
        let mut stmt =
            conn.prepare("SELECT provider, rule_json, tz, holidays_json FROM provider_zones")?;
        let rows = stmt.query_map([], |row| {
            Ok(Zone::from_db(
                &row.get::<_, String>("provider")?,
                &row.get::<_, String>("rule_json")?,
                &row.get::<_, Option<String>>("tz")?.unwrap_or_default(),
                &row.get::<_, Option<String>>("holidays_json")?
                    .unwrap_or_default(),
            ))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 手工更新 PriceSpec（PR-6 WebUI 校对）。全量字段 upsert，强制转正为 manual。
    pub fn upsert_price_spec(&self, spec: &PriceSpecInput<'_>) -> Result<()> {
        let conn = self.conn()?;
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
                spec.provider,
                spec.model,
                spec.input_cost,
                spec.output_cost,
                spec.cache_read_cost,
                spec.cache_write_cost,
                spec.reasoning_cost,
                spec.tiered_json,
                spec.zone_ref,
                spec.cny_list_price_json,
            ],
        )?;
        Ok(())
    }

    /// P2.a 刷新更新：仅覆盖 `price_source != 'manual'` 的行（manual 为人工锚定，永不覆盖），
    /// 覆盖后 source 标 `litellm_remote`、`price_stale=0`。cache_read 为 None 时保持原值（COALESCE）。
    /// 返回是否命中更新（false = 行不存在或属 manual）。
    pub fn refresh_price_spec(
        &self,
        provider: &str,
        model: &str,
        input_cost: f64,
        output_cost: f64,
        cache_read_cost: Option<f64>,
    ) -> Result<bool> {
        let conn = self.conn()?;
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
    pub fn accept_price_spec(&self, provider: &str, model: &str) -> Result<bool> {
        let conn = self.conn()?;
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

    pub fn get_routing_policy(&self, task_type: &str) -> Result<Option<RoutingPolicy>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT * FROM routing_policy WHERE task_type = ?1")?;
        let mut rows = stmt.query(params![task_type])?;
        match rows.next()? {
            Some(row) => Ok(Some(routing_policy_from_row(row)?)),
            None => Ok(None),
        }
    }

    pub fn list_routing_policy(&self) -> Result<Vec<RoutingPolicy>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT * FROM routing_policy ORDER BY task_type")?;
        let rows = stmt.query_map([], routing_policy_from_row)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn upsert_routing_policy(&self, p: &RoutingPolicy) -> Result<()> {
        let conn = self.conn()?;
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
    pub fn insert_routing_decision(&self, d: &RoutingDecisionRecord<'_>) -> Result<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO routing_decisions (request_id, task_type, band, signals_json,
                                        candidates_json, selected, fallback_chain, routing_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                d.request_id,
                d.task_type,
                d.band,
                d.signals_json,
                d.candidates_json,
                d.selected,
                d.fallback_chain,
                d.routing_ms,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_routing_decision_outcome(&self, id: i64, outcome: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE routing_decisions SET outcome = ?2 WHERE id = ?1",
            params![id, outcome],
        )?;
        Ok(())
    }

    /// P3：写入模型健康状态与检查时刻。仅状态变化由 `health.rs` 触发，非热路径。
    pub fn set_model_health(&self, name: &str, state: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE models SET health_state = ?2, health_checked_at = CURRENT_TIMESTAMP WHERE name = ?1",
            params![name, state],
        )?;
        Ok(())
    }

    /// P3：routing overhead 聚合报告（routing_decisions.routing_ms）。
    /// days=0 表示全部；返回 (条数, 均值ms, P95ms, maxms, 慢决策条数)。
    /// （快路径 >10ms / 全路径 >100ms 视为实现 bug 上报；阈值由调用方解释。）
    pub fn routing_overhead_report(&self, days: i64) -> Result<(i64, f64, f64, f64, i64)> {
        let conn = self.conn()?;
        let where_clause = if days > 0 {
            format!(
                "WHERE created_at >= datetime('now', '-{days} days') AND routing_ms IS NOT NULL"
            )
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
            &format!(
                "SELECT COUNT(*) FROM routing_decisions {where_clause} AND routing_ms > 100.0"
            ),
            [],
            |r| r.get(0),
        )?;
        Ok((count, avg, p95, max, slow))
    }

    /// P1.d 影子评测记录：一条「路由选择 × 强模型基线」双跑结果，供离线 AIQ 重放。
    /// quality 两列留给裁判/离线脚本回填（开放式生成无结构化信号时不回填，判 NULL）。
    pub fn insert_routing_calibration(&self, c: &RoutingCalibrationInput<'_>) -> Result<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO routing_calibration (task_type, query_hash, routed_model, baseline_model,
                                          routed_cost, baseline_cost, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                c.task_type,
                c.query_hash,
                c.routed_model,
                c.baseline_model,
                c.routed_cost,
                c.baseline_cost,
                c.source,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// P1.d：同一 (task_type, query_hash) 是否已有样本——重放去重，避免相同 query 重复膨胀样本数。
    pub fn routing_calibration_exists(&self, task_type: &str, query_hash: &str) -> Result<bool> {
        let conn = self.conn()?;
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM routing_calibration WHERE task_type = ?1 AND query_hash = ?2",
            params![task_type, query_hash],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// P1.d：已采集的影子样本数（AIQ 重放/判定需要足够样本才有统计意义）。
    pub fn count_routing_calibration(&self) -> Result<i64> {
        let conn = self.conn()?;
        let c = conn.query_row("SELECT COUNT(*) FROM routing_calibration", [], |r| {
            r.get::<_, i64>(0)
        })?;
        Ok(c)
    }

    /// N2.b：全量影子样本（网格搜索重放输入）。
    pub fn list_routing_calibration(&self) -> Result<Vec<crate::models::CalibrationRow>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT task_type, query_hash, routed_model, baseline_model,
                routed_cost, baseline_cost, routed_quality, baseline_quality
         FROM routing_calibration",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(crate::models::CalibrationRow {
                    task_type: r.get(0)?,
                    query_hash: r.get(1)?,
                    routed_model: r.get(2)?,
                    baseline_model: r.get(3)?,
                    routed_cost: r.get(4)?,
                    baseline_cost: r.get(5)?,
                    routed_quality: r.get(6)?,
                    baseline_quality: r.get(7)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// N2.a：写入一份路由体检报告（周期 job 幂等追加，latest 读最新）。
    pub fn insert_policy_review(&self, r: &PolicyReviewInput<'_>) -> Result<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO policy_review (samples, weak_cost, weak_quality, cur_cost, cur_quality,
                                    strong_cost, strong_quality, aiq, saved_pct, conclusion,
                                    budget_tiers_json, suggestions_json)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                r.samples,
                r.weak_cost,
                r.weak_quality,
                r.cur_cost,
                r.cur_quality,
                r.strong_cost,
                r.strong_quality,
                r.aiq,
                r.saved_pct,
                r.conclusion,
                r.budget_tiers_json,
                r.suggestions_json,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// N2.a：最近一份路由体检报告（GET /api/routing/review 数据源）。
    pub fn latest_policy_review(&self) -> Result<Option<crate::models::PolicyReview>> {
        let conn = self.conn()?;
        let row = conn
            .query_row(
                "SELECT id, created_at, samples, weak_cost, weak_quality, cur_cost, cur_quality,
                    strong_cost, strong_quality, aiq, saved_pct, conclusion,
                    budget_tiers_json, suggestions_json
             FROM policy_review ORDER BY id DESC LIMIT 1",
                [],
                |r| {
                    Ok(crate::models::PolicyReview {
                        id: r.get(0)?,
                        created_at: r.get(1)?,
                        samples: r.get(2)?,
                        weak_cost: r.get(3)?,
                        weak_quality: r.get(4)?,
                        cur_cost: r.get(5)?,
                        cur_quality: r.get(6)?,
                        strong_cost: r.get(7)?,
                        strong_quality: r.get(8)?,
                        aiq: r.get(9)?,
                        saved_pct: r.get(10)?,
                        conclusion: r.get(11)?,
                        budget_tiers_json: r.get(12)?,
                        suggestions_json: r.get(13)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// N2.a：近 N 天路由决策的预算档触发分布（signals_json.budget_tier 计数）。
    /// 旧记录无 budget_tier 字段 → 计入 "unknown"；无记录 → 空表。
    pub fn budget_tier_distribution(&self, days: i64) -> Result<Vec<(String, i64)>> {
        let conn = self.conn()?;
        let where_clause = if days > 0 {
            format!("WHERE created_at >= datetime('now', '-{days} days')")
        } else {
            String::new()
        };
        let mut stmt = conn.prepare(&format!(
            "SELECT signals_json FROM routing_decisions {where_clause}"
        ))?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut counts: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
        for row in rows {
            let json = row?;
            let tier = serde_json::from_str::<serde_json::Value>(&json)
                .ok()
                .and_then(|v| {
                    v.get("budget_tier")
                        .and_then(|t| t.as_str())
                        .map(String::from)
                })
                .unwrap_or_else(|| "unknown".to_string());
            *counts.entry(tier).or_insert(0) += 1;
        }
        Ok(counts.into_iter().collect())
    }

    pub fn get_model_task_score(
        &self,
        model_name: &str,
        task_type: &str,
    ) -> Result<Option<ModelTaskScore>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT * FROM model_task_score WHERE model_name = ?1 AND task_type = ?2")?;
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
    pub fn roll_avg_out_tokens(
        &self,
        model_name: &str,
        task_type: &str,
        actual_out: i64,
    ) -> Result<()> {
        let conn = self.conn()?;
        if actual_out <= 0 {
            return Ok(());
        }
        let alpha = self
            .get_setting("signal.ewma_alpha")
            .ok()
            .flatten()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.15)
            .clamp(0.01, 0.5);
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
    pub fn task_avg_out_tokens(&self, task_type: &str) -> f64 {
        let Ok(conn) = self.conn() else {
            return 750.0;
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
    pub fn model_cache_hit_rate(&self, task_type: &str) -> HashMap<String, f64> {
        let mut out = HashMap::new();
        let Ok(conn) = self.conn() else {
            return out;
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
    pub fn recent_conversation_model(&self, conversation_id: &str) -> Result<Option<String>> {
        let conn = self.conn()?;
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
    pub fn global_budget_ratio(&self) -> Option<f64> {
        let budget = self.get_budget("global", "global").ok().flatten()?;
        if budget.max_budget <= 0.0 {
            return None;
        }
        let spent = self.get_total_spend(None, None, None).unwrap_or(0.0);
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
        &self,
        model_name: &str,
        task_type: &str,
        kind: QualitySignalKind,
    ) -> Result<()> {
        let conn = self.conn()?;
        let alpha = self
            .get_setting("signal.ewma_alpha")
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
    fn seed_cold_start_scores(&self, model_name: &str) -> Result<usize> {
        let conn = self.conn()?;
        let m = self.find_model(model_name, true)?;
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

    // ── Pricing calibration (PRICING-PLAN §6.2) ──

    pub fn aggregate_usage_by_model_day(&self, day: &str) -> Result<Vec<DailyAggregate>> {
        let conn = self.conn()?;
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
    pub fn upsert_price_calibration(&self, row: &CalibrationRow) -> Result<()> {
        let conn = self.conn()?;
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
                row.provider,
                row.model,
                row.as_of,
                row.calls,
                row.est_cost,
                row.act_cost,
                row.input_side_ratio,
                row.cache_hit_rate,
                row.out_in_ratio,
                row.field_missing_count,
            ],
        )?;
        Ok(())
    }

    /// stale 去抖：最近 `days` 天内对账偏差越界（∉[0.8,1.2]）的天数。
    pub fn stale_streak(&self, provider: &str, model: &str, days: i64) -> Result<i64> {
        let conn = self.conn()?;
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

    pub fn list_price_calibration(&self, days: i64) -> Result<Vec<CalibrationRow>> {
        let conn = self.conn()?;
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

    pub fn probe_stats(&self) -> Result<ProbeStats> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT COUNT(*) as rounds,
                COALESCE(SUM(CASE WHEN cost > 0 THEN cost ELSE 0 END), 0.0) as spend,
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

    pub fn upsert_budget(&self, b: &BudgetInput<'_>) -> Result<()> {
        let conn = self.conn()?;
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
                b.scope,
                b.scope_id,
                b.max_budget,
                b.duration,
                b.scope_task_type,
                b.soft_limit_ratio.map(|v| v as f64),
                b.action_on_exceed,
            ],
        )?;
        Ok(())
    }

    pub fn get_budget(&self, scope: &str, scope_id: &str) -> Result<Option<Budget>> {
        let conn = self.conn()?;
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

    pub fn delete_budget(&self, scope: &str, scope_id: &str) -> Result<bool> {
        let conn = self.conn()?;
        let n = conn.execute(
            "DELETE FROM budgets WHERE scope = ?1 AND scope_id = ?2",
            params![scope, scope_id],
        )?;
        Ok(n > 0)
    }

    pub fn list_budgets(&self) -> Result<Vec<Budget>> {
        let conn = self.conn()?;
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
}

// ── Pure helpers ──

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

fn model_from_row(row: &rusqlite::Row) -> rusqlite::Result<ModelRow> {
    Ok(ModelRow {
        id: row.get("id")?,
        name: row.get("name")?,
        provider: row.get("provider")?,
        litellm_model: row.get("litellm_model")?,
        api_base: row
            .get::<_, Option<String>>("api_base")?
            .unwrap_or_default(),
        api_key_env: row
            .get::<_, Option<String>>("api_key_env")?
            .unwrap_or_default(),
        task_type: row
            .get::<_, Option<String>>("task_type")?
            .unwrap_or_default(),
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

impl From<ModelRow> for Model {
    /// 读侧宽容：尊重存量 `is_local` 判别列，不按 api_base 重分类。
    /// 列编码：Local(Ollama)→provider="ollama"，Local(OpenAiCompat)→provider="custom"。
    fn from(r: ModelRow) -> Self {
        let backend = if r.is_local == 1 {
            let compat = if r.provider.eq_ignore_ascii_case("ollama") {
                LocalCompat::Ollama
            } else {
                LocalCompat::OpenAiCompat
            };
            Backend::Local {
                compat,
                api_base: r.api_base.clone(),
            }
        } else {
            Backend::Cloud {
                provider: Provider::parse(&r.provider),
                api_base: Some(r.api_base.clone()).filter(|s| !s.is_empty()),
                api_key: ApiKeyRef::parse(&r.api_key_env),
            }
        };
        Model {
            id: r.id,
            name: r.name,
            litellm_model: r.litellm_model,
            backend,
            task_type: r.task_type,
            input_cost_per_token: r.input_cost_per_token,
            output_cost_per_token: r.output_cost_per_token,
            rpm: r.rpm,
            is_active: r.is_active,
            capability_tier: r.capability_tier,
            quality_score: r.quality_score,
            context_window: r.context_window,
            supports_tools: r.supports_tools,
            supports_vision: r.supports_vision,
            supports_stream: r.supports_stream,
            priority: r.priority,
            health_state: r.health_state,
            needs_calibration: r.needs_calibration,
        }
    }
}

impl From<&Model> for ModelRow {
    fn from(m: &Model) -> Self {
        ModelRow {
            id: m.id,
            name: m.name.clone(),
            provider: m.provider_name().to_string(),
            litellm_model: m.litellm_model.clone(),
            api_base: m.api_base().to_string(),
            api_key_env: m.api_key_env().to_string(),
            task_type: m.task_type.clone(),
            input_cost_per_token: m.input_cost_per_token,
            output_cost_per_token: m.output_cost_per_token,
            rpm: m.rpm,
            is_active: m.is_active,
            capability_tier: m.capability_tier,
            quality_score: m.quality_score,
            context_window: m.context_window,
            supports_tools: m.supports_tools,
            supports_vision: m.supports_vision,
            supports_stream: m.supports_stream,
            is_local: i64::from(m.is_local()),
            priority: m.priority,
            health_state: m.health_state.clone(),
            needs_calibration: m.needs_calibration,
        }
    }
}

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
        batch_multiplier: row
            .get::<_, Option<f64>>("batch_multiplier")?
            .unwrap_or(0.5),
        price_source: row
            .get::<_, Option<String>>("price_source")?
            .unwrap_or_default(),
        price_stale: row.get::<_, Option<i64>>("price_stale")?.unwrap_or(0) != 0,
        stale_reason: row.get::<_, Option<String>>("stale_reason")?,
        effective_from: row.get("effective_from")?,
    })
}

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

#[cfg(test)]
mod model_row_tests {
    use super::*;

    /// 行层 ↔ 领域层往返恒等：ModelRow → Model → ModelRow 不得丢字段。
    #[test]
    fn row_model_roundtrip_is_identity() {
        let row = ModelRow {
            id: 7,
            name: "m1".into(),
            provider: "dashscope".into(),
            litellm_model: "dashscope/m1".into(),
            api_base: "https://api.example.com".into(),
            api_key_env: "DASHSCOPE_API_KEY".into(),
            task_type: "general".into(),
            input_cost_per_token: 1.11e-7,
            output_cost_per_token: 2.78e-7,
            rpm: 60,
            is_active: 1,
            capability_tier: 2,
            quality_score: 0.6,
            context_window: 32768,
            supports_tools: 1,
            supports_vision: 0,
            supports_stream: 1,
            is_local: 0,
            priority: 1,
            health_state: "up".into(),
            needs_calibration: 0,
        };
        let model = Model::from(row.clone());
        assert_eq!(ModelRow::from(&model), row);
    }

    /// 本地 OpenAI 兼容行：is_local=1 + provider="custom" 编码必须无损往返。
    #[test]
    fn local_openai_compat_row_roundtrip() {
        let row = ModelRow {
            id: 3,
            name: "l1".into(),
            provider: "custom".into(),
            litellm_model: "openai/l1".into(),
            api_base: "http://localhost:1234/v1".into(),
            api_key_env: String::new(),
            task_type: String::new(),
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
            rpm: 60,
            is_active: 1,
            capability_tier: 2,
            quality_score: 0.6,
            context_window: 32768,
            supports_tools: 0,
            supports_vision: 0,
            supports_stream: 0,
            is_local: 1,
            priority: 0,
            health_state: "unknown".into(),
            needs_calibration: 0,
        };
        let model = Model::from(row.clone());
        assert!(model.is_local());
        assert!(matches!(
            model.backend,
            Backend::Local {
                compat: LocalCompat::OpenAiCompat,
                ..
            }
        ));
        assert_eq!(ModelRow::from(&model), row);
    }
}
