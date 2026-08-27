//! Axum HTTP server — pure REST API.
//!
//! All LLM work is delegated to the Python AI micro-service (`ai_client`).
//! No RPC bridge, no stringly-typed dispatch — every endpoint is a typed
//! handler on a real resource path.

use crate::ai_client::{self, ModelSpec};
use crate::config;
use crate::conversations;
use crate::db;
use crate::error::{AppError, Result};
use crate::models::*;
use crate::pricing;
use crate::router;
use crate::security;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, Mutex};

// ── AppState ──

#[derive(Clone)]
pub struct AppState {
    pub children: Arc<Mutex<Children>>,
    pub started_at: std::time::Instant,
}

/// 全局时段规则解析器（PRICING-PLAN §3.4）。首次调用时从 provider_zones 表加载，
/// 规则由迁移脚本/overlay 维护，运行期不需要热刷新（校准 job 属 PR-6）。
static ZONES: std::sync::OnceLock<pricing::ZoneResolver> = std::sync::OnceLock::new();
pub fn zone_resolver() -> &'static pricing::ZoneResolver {
    ZONES.get_or_init(|| {
        let zr = pricing::ZoneResolver::new();
        if let Ok(zones) = db::list_provider_zones() {
            zr.load(zones);
        }
        zr
    })
}

/// 取请求当前 UTC epoch 秒（失败回落 0，仅影响峰谷系数精度，不影响正确性）
pub fn now_epoch_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 按模型名查 PriceSpec 并计算实际成本（无 PriceSpec → 0，本地/未登记模型）。
/// 返回 (act_cost, zone_multiplier)。
fn priced_usage(provider: &str, model: &str, usage: &pricing::UsageDetail) -> (f64, f64) {
    match db::get_price_spec(provider, model) {
        Ok(Some(ps)) => {
            let zr = zone_resolver();
            let t = now_epoch_secs();
            (ps.actual_cost(usage, t, zr), ps.zone_multiplier(t, zr))
        }
        _ => (0.0, 1.0),
    }
}

#[derive(Default)]
pub struct Children {
    pub api: Option<std::process::Child>,
    pub ollama: Option<std::process::Child>,
    pub ai: Option<std::process::Child>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            children: Arc::new(Mutex::new(Children::default())),
            started_at: std::time::Instant::now(),
        }
    }
}

// ── Error → response ──

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(json!({ "error": self.to_string() }))).into_response()
    }
}

// ── DTOs ──

#[derive(Debug, Deserialize)]
struct ChatBody {
    #[serde(default)]
    model: Option<String>,
    messages: Vec<Value>,
    #[serde(default)]
    sr_domain: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OrchestrateBody {
    query: String,
    #[serde(default)]
    history: Vec<Value>,
    #[serde(default)]
    sr_domain: Option<String>,
    /// Server-side context building: when present, history is loaded from the
    /// conversation store (SQLite) and the client-sent `history` is ignored.
    #[serde(default)]
    conversation_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MessageAppend {
    role: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    meta: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct MessageUpdate {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    meta: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ConfigUpdate {
    updates: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct ConversationSave {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    messages: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct ConversationRename {
    #[serde(default)]
    title: String,
}

#[derive(Debug, Deserialize)]
struct ServiceAction {
    #[serde(default)]
    changed_keys: Vec<String>,
}

// ── Health / UI ──

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "version": "2.0.0" }))
}

// ── Models ──

async fn list_models(Query(q): Query<Value>) -> Result<Json<Value>> {
    let active_only = q
        .get("active_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    Ok(Json(json!({ "models": db::list_models(active_only)? })))
}

async fn register_model(Json(m): Json<Model>) -> Result<Json<Value>> {
    let id = db::insert_model(&m)?;
    Ok(Json(json!({ "id": id, "name": m.name })))
}

async fn get_model(Path(name): Path<String>) -> Result<Json<Model>> {
    Ok(Json(db::get_model(&name)?))
}

async fn update_model(Path(name): Path<String>, Json(updates): Json<serde_json::Map<String, Value>>) -> Result<Json<Value>> {
    if !db::update_model(&name, &updates)? {
        return Err(AppError::NotFound(format!("model '{name}'")));
    }
    Ok(Json(json!({ "updated": true })))
}

async fn delete_model(Path(name): Path<String>) -> Result<Json<Value>> {
    if !db::delete_model(&name)? {
        return Err(AppError::NotFound(format!("model '{name}'")));
    }
    Ok(Json(json!({ "deleted": true })))
}

// ── Usage / Budgets ──

async fn get_usage(Query(q): Query<Value>) -> Result<Json<Value>> {
    let model_name = q.get("model_name").and_then(|v| v.as_str());
    let user_id = q.get("user_id").and_then(|v| v.as_str());
    let since = q.get("since").and_then(|v| v.as_str());
    let stats = db::get_usage_stats(model_name, user_id, since)?;
    let total = db::get_total_spend(user_id, model_name, since)?;
    let total_cache_saved: f64 = stats.iter().map(|s| s.cache_saved).sum();
    Ok(Json(json!({ "usage": stats, "total_spend": total, "total_cache_saved": total_cache_saved })))
}

async fn list_budgets() -> Result<Json<Value>> {
    Ok(Json(json!({ "budgets": db::list_budgets()? })))
}

async fn set_budget(Json(req): Json<Budget>) -> Result<Json<Value>> {
    db::upsert_budget(&req.scope, &req.scope_id, req.max_budget, &req.duration)?;
    Ok(Json(json!({ "set": true })))
}

async fn delete_budget(Query(q): Query<Value>) -> Result<Json<Value>> {
    let scope = q.get("scope").and_then(|v| v.as_str()).unwrap_or("");
    let scope_id = q.get("scope_id").and_then(|v| v.as_str()).unwrap_or("");
    let deleted = db::delete_budget(scope, scope_id)?;
    Ok(Json(json!({ "deleted": deleted })))
}

async fn check_budget(Query(q): Query<Value>) -> Result<Json<Value>> {
    let scope = q.get("scope").and_then(|v| v.as_str()).unwrap_or("");
    let scope_id = q.get("scope_id").and_then(|v| v.as_str()).unwrap_or("");
    let prospective = q.get("prospective_cost").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let budget = db::get_budget(scope, scope_id)?;
    let (within, spent) = match &budget {
        Some(b) => {
            let spent = db::get_total_spend(
                if scope == "user" { Some(scope_id) } else { None },
                if scope == "model" { Some(scope_id) } else { None },
                None,
            )?;
            ((spent + prospective) <= b.max_budget, spent)
        }
        None => (true, 0.0),
    };
    Ok(Json(json!({ "within_budget": within, "budget": budget, "spent": spent })))
}

// ── Config / Stats ──

async fn get_config() -> Json<Value> {
    // Mask secret values before exposing them over the API. Any key whose name
    // looks like a credential (ends with _API_KEY / _KEY / _TOKEN / _SECRET) is
    // returned as "****" + last 4 chars (or just "****" if too short). The raw
    // values are still on disk in .env and are read directly by write_env /
    // the AI service — the frontend only ever needs to know *whether* a key is
    // set, not its value.
    let env = config::read_env();
    let masked: HashMap<String, String> = env
        .iter()
        .map(|(k, v)| {
            let upper = k.to_ascii_uppercase();
            let is_secret = upper.ends_with("_API_KEY")
                || upper.ends_with("_KEY")
                || upper.ends_with("_TOKEN")
                || upper.ends_with("_SECRET");
            if is_secret && !v.is_empty() {
                let tail: String = v.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
                (k.clone(), if v.len() <= 4 { "****".to_string() } else { format!("****{tail}") })
            } else {
                (k.clone(), v.clone())
            }
        })
        .collect();
    Json(json!(masked))
}

async fn update_config(Json(req): Json<ConfigUpdate>) -> Result<Json<Value>> {
    write_env(&req.updates)?;
    Ok(Json(json!({ "updated": req.updates.keys().collect::<Vec<_>>() })))
}

async fn get_stats() -> Result<Json<Value>> {
    Ok(Json(json!({
        "model_count": db::list_models(true)?.len(),
        "total_spend": db::get_total_spend(None, None, None)?,
        "model_spend": db::get_usage_stats(None, None, None)?,
        "routing_stats": {},
        "cache_enabled": true,
    })))
}

// ── Conversations ──

async fn list_conversations() -> Result<Json<Value>> {
    Ok(Json(json!({ "conversations": conversations::list()? })))
}

async fn get_conversation(Path(id): Path<String>) -> Result<Json<Value>> {
    Ok(Json(conversations::load(&id)?))
}

async fn save_conversation(Json(req): Json<ConversationSave>) -> Result<Json<Value>> {
    let id = conversations::save_or_create(&req.id, &req.title, &req.messages)?;
    Ok(Json(json!({ "id": id, "saved": true })))
}

async fn delete_conversation(Path(id): Path<String>) -> Result<Json<Value>> {
    conversations::delete(&id)?;
    Ok(Json(json!({ "deleted": true })))
}

async fn rename_conversation(
    Path(id): Path<String>,
    Json(req): Json<ConversationRename>,
) -> Result<Json<Value>> {
    conversations::rename(&id, &req.title)?;
    Ok(Json(json!({ "id": id, "renamed": true })))
}

/// Append a single message (phase 1 of two-phase persistence — the user
/// message and the assistant placeholder land on disk BEFORE the LLM call).
async fn append_conversation_message(
    Path(id): Path<String>,
    Json(req): Json<MessageAppend>,
) -> Result<Json<Value>> {
    let role = req.role.trim().to_string();
    if role != "user" && role != "assistant" {
        return Err(AppError::InvalidRequest(format!(
            "role must be 'user' or 'assistant', got '{role}'"
        )));
    }
    // Defense-in-depth: run the same PII/jailbreak rules the chat path uses on
    // user content that is about to be persisted (append bypasses the
    // orchestrate-time check only for assistant placeholders, which are empty).
    if role == "user" && !req.content.is_empty() {
        let sec = security::check(&req.content, true, true);
        if sec.blocked {
            return Err(AppError::InvalidRequest(format!(
                "blocked: {}",
                sec.block_reason
            )));
        }
    }
    let (conv_id, seq) =
        conversations::append_message(&id, &role, &req.content, req.meta.as_ref())?;
    Ok(Json(json!({ "id": conv_id, "seq": seq, "appended": true })))
}

/// Fill in / update an existing message (phase 2 — assistant reply + metadata
/// after the stream completes, error text on failure).
async fn update_conversation_message(
    Path((id, seq)): Path<(String, i64)>,
    Json(req): Json<MessageUpdate>,
) -> Result<Json<Value>> {
    conversations::update_message(&id, seq, req.content.as_deref(), req.meta.as_ref())?;
    Ok(Json(json!({ "id": id, "seq": seq, "updated": true })))
}

// ── Chat / Orchestrate (SSE) ──

/// P3：按 `primary` + `fallback_chain` 顺序故障转移。成功即返回，对失败模型打健康哨点；
/// 只有实际「跳升」到下一个候选时才给失败模型记 Escalation 成效信号（降级已有代价）。
async fn chat_with_failover(
    models: &[Model],
    task_type: &str,
    primary: &str,
    fallback_chain: &[String],
    messages: &[Value],
) -> Result<(ai_client::ChatResult, String)> {
    let mut try_names: Vec<String> = Vec::with_capacity(1 + fallback_chain.len());
    try_names.push(primary.to_string());
    try_names.extend(fallback_chain.iter().cloned());
    try_names.dedup();

    let mut first_err: Option<String> = None;
    let mut idx = 0usize;
    while idx < try_names.len() {
        let name = &try_names[idx];
        let Some(m) = models.iter().find(|m| m.name == *name) else {
            idx += 1;
            continue;
        };
        let spec = ModelSpec::from(m);
        match ai_client::chat(&spec, messages, 500, 0.3).await {
            Ok(res) => {
                crate::health::record_outcome(name, true);
                // 跳升到非主选：给所有先前失败模型记一次 escalation（副作用小，但真实代价信号）
                for failed in try_names.iter().take(idx) {
                    if failed != name {
                        let _ = db::upsert_model_task_score_signal(failed, task_type, QualitySignalKind::Escalation);
                    }
                }
                return Ok((res, name.clone()));
            }
            Err(e) => {
                crate::health::record_outcome(name, false);
                if first_err.is_none() {
                    first_err = Some(e.to_string());
                }
                idx += 1;
            }
        }
    }
    Err(AppError::AiService(first_err.unwrap_or_else(|| "所有候选模型均调用失败".into())))
}

async fn chat_stream(Json(req): Json<ChatBody>) -> Response {
    let user_text = security::extract_user_text(&req.messages);
    let sec = security::check(&user_text, true, true);
    if sec.blocked {
        return blocked_response(&sec);
    }

    let processed_messages: Vec<Value> = if sec.processed_text != user_text {
        let mut msgs = req.messages.clone();
        if let Some(last_user) = msgs
            .iter_mut()
            .rev()
            .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
        {
            last_user["content"] = Value::String(sec.processed_text.clone());
        }
        msgs
    } else {
        req.messages.clone()
    };

    // Routing: regex tier + domain enhancement + plan() scoring (P0.d)
    let models = match db::list_models(true) {
        Ok(m) => m,
        Err(e) => return err_response(e),
    };
    let classifier = pick_classifier(&models);
    let sr_domain = req.sr_domain.clone().unwrap_or_default();
    let route_start = std::time::Instant::now();
    let mut routing = router::route(
        req.model.as_deref().unwrap_or("auto"),
        &user_text,
        classifier.as_ref(),
    )
    .await;
    if !sr_domain.is_empty() {
        let (new_type, changed) = router::enhance_with_domain(&routing.task_type, &sr_domain);
        if changed {
            routing.task_type = new_type;
        }
    }

    // P0.d：路由结果必须落在注册表内；direct 未注册 / plan 无候选 → 明确报错，
    // 不再伪造空 spec 继续调用。
    let primary_provider: &str = match models.iter().find(|m| m.name == routing.model) {
        Some(m) => m.provider.as_str(),
        None => {
            let detail = if let Some(d) = routing.method.strip_prefix("plan_error:") {
                format!("路由失败：{d}")
            } else {
                format!("模型 '{}' 未注册或未启用，请先在模型页添加", routing.model)
            };
            return sse_error(&detail);
        }
    };

    // 审计落库（P0.c/P0.d）：决策快照 + 耗时；outcome 在调用完成后回填
    let routing_ms = route_start.elapsed().as_secs_f64() * 1000.0;
    let request_id = format!(
        "chat-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let decision_id = if routing.method != "direct" {
        db::insert_routing_decision(
            &request_id,
            &routing.task_type,
            &routing.band,
            &serde_json::to_string(&json!({ "method": routing.method, "sr_domain": sr_domain }))
                .unwrap_or_default(),
            &serde_json::to_string(&routing.fallback_chain).unwrap_or_default(),
            &routing.model,
            &routing.fallback_chain.join(","),
            routing_ms,
        )
        .unwrap_or(0)
    } else {
        0
    };

    // routing 会在 head 的 json! 中被 move，先取出落库需要的字段
    let routing_task_type = routing.task_type.clone();

    let head = format!(
        "data: {}\n\n",
        json!({
            "routing": routing,
            "security": { "domain": sec.domain, "domain_method": sec.domain_method, "pii": sec.pii }
        })
    );

    // P3：按 primary + fallback_chain 故障转移（失败自动打健康哨点并跳升重试）
    let chat_start = std::time::Instant::now();
    let tail = match chat_with_failover(
        &models,
        &routing_task_type,
        &routing.model,
        &routing.fallback_chain,
        &processed_messages,
    )
    .await
    {
        Ok((res, used_model)) => {
            if decision_id > 0 {
                let _ = db::update_routing_decision_outcome(decision_id, "success");
            }
            // 实际响应模型的 provider 可能因 fallback 与主选不同（逐模型定位真源）
            let provider = models
                .iter()
                .find(|m| m.name == used_model)
                .map(|m| m.provider.as_str())
                .unwrap_or(primary_provider);
            // PRICING-PLAN §4.2/§6.1：Rust 单一计价真源，按真实 usage 分项计算并落库。
            // （PR-5 落地前 est_cost 传 0；task_type 用路由分类结果）
            // P1.a：只记成功路径；失败/重试走 routing_decisions.outcome。
            let latency_ms = chat_start.elapsed().as_secs_f64() * 1000.0;
            let (act_cost, zm) = priced_usage(provider, &used_model, &res.usage);
            let _ = db::insert_usage(
                &used_model,
                "default",
                res.usage.prompt_tokens,
                res.usage.completion_tokens,
                act_cost,
                Some(&routing_task_type),
                false,
                Some(latency_ms),
                Some(&request_id),
                Some(&db::UsageExtra {
                    cached_tokens: res.usage.cached_tokens,
                    reasoning_tokens: res.usage.reasoning_tokens,
                    est_cost: 0.0,
                    act_cost,
                    zone_multiplier: zm,
                    conversation_id: None,
                    field_missing: res.usage.field_missing,
                    cache_saved_cost: 0.0,
                }),
            );
            // P1.c：正常完成信号 → 该 模型×任务 的 ewma_quality 上修（+0.7）
            db::upsert_model_task_score_signal(&used_model, &routing_task_type, QualitySignalKind::Success)
                .ok();
            // P1.d：按 shadow_ratio 概率后台采样双跑，积累 AIQ 成本—质量样本（不改返回）。
            maybe_shadow_sample(models.clone(), routing_task_type.clone(), user_text.clone());
            format!(
                "data: {}\n\n",
                json!({
                    "done": true,
                    "content": res.content,
                    "model": res.model,
                    "cost": act_cost,
                    "input_tokens": res.usage.prompt_tokens,
                    "output_tokens": res.usage.completion_tokens,
                    "cached_tokens": res.usage.cached_tokens,
                })
            )
        }
        Err(e) => {
            if decision_id > 0 {
                let _ = db::update_routing_decision_outcome(decision_id, "failed");
            }
            format!("data: {}\n\n", json!({ "error": true, "detail": e.to_string() }))
        }
    };

    Response::builder()
        .header(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))
        .body(Body::from(format!("{head}{tail}")))
        .unwrap()
}

async fn orchestrate_stream(Json(req): Json<OrchestrateBody>) -> Response {
    let sec = security::check(&req.query, true, true);
    if sec.blocked {
        return blocked_response(&sec);
    }

    let models = match db::list_models(true) {
        Ok(m) => m,
        Err(e) => return err_response(e),
    };
    let specs: Vec<ModelSpec> = models.iter().map(ModelSpec::from).collect();
    // Semantic cache dir. The Python SemanticCache gates on an internal
    // _cache_ready flag (set only after /v1/cache/init succeeds), so passing a
    // non-empty path here is safe — it will NOT trigger a download unless the
    // user has explicitly pre-initialized the embedding model.
    let cache_dir = config::data_dir().join("chroma").to_string_lossy().to_string();

    // Server-side context building: load history + rolling summary from the
    // conversation store. Client-sent `history` is the legacy fallback (CLI/TUI).
    let (history, conversation_id, summary, summary_upto) = match &req.conversation_id {
        Some(cid) => {
            let h = match conversations::load_history_for_orchestrate(cid, &req.query) {
                Ok(h) => h,
                Err(e) => return err_response(e),
            };
            let (s, upto) = conversations::get_summary(cid).unwrap_or((None, 0));
            (h, Some(cid.clone()), s, upto)
        }
        None => (req.history.clone(), None, None, 0),
    };

    // P0.f: Rust 统一决策，Python 无模型真源。每个编排角色各做一次 plan()；
    // 失败回落 models 首模型（无硬编码字面量），交由 Python 兜底。
    let role_model = |role: &str| -> String {
        router::plan_decision(role, &models)
            .ok()
            .map(|o| o.primary)
            .or_else(|| specs.first().map(|s| s.name.clone()))
            .unwrap_or_default()
    };
    let assignments = json!({
        "general": role_model("general"),
        "decompose": role_model("decompose"),
        "aggregate": role_model("aggregate"),
    });

    let events = match ai_client::orchestrate_stream(
        &req.query,
        &history,
        req.sr_domain.as_deref().unwrap_or(""),
        &specs,
        &cache_dir,
        conversation_id.as_deref(),
        summary.as_deref(),
        summary_upto,
        &assignments,
    )
    .await
    {
        Ok(e) => e,
        Err(e) => return err_response(AppError::AiService(e.to_string())),
    };

    // Forward each event as it arrives (true SSE, not buffered). The Python
    // side streams `token` deltas; the browser renders them incrementally.
    let conv_for_events = conversation_id.clone();
    // P1.a：编排级别的聚合请求号——有会话 id 用它，否则自生成（供 usage 行串联同一次编排的多次 task_done）
    let orchestrate_rid = conversation_id.clone().unwrap_or_else(|| {
        format!(
            "orchestrate-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        )
    });
    // P1.d：编排主 query 副本，供闭包内按 shadow_ratio 后台采样双跑（不改返回）。
    let shadow_query = req.query.clone();
    let body = Body::from_stream(events.map(move |ev| {
        // Persist cache hit/miss for hit-rate stats + threshold calibration.
        // Pure side-effect; failures are non-fatal (cache is best-effort).
        if let Some(obj) = ev.data.as_object() {
            // Rolling-summary persistence: the AI service recomputed the L2
            // summary because the token budget dropped older turns.
            if ev.event == "summary_updated" {
                if let (Some(cid), Some(text), Some(upto)) = (
                    conv_for_events.as_deref(),
                    obj.get("text").and_then(|v| v.as_str()),
                    obj.get("upto").and_then(|v| v.as_i64()),
                ) {
                    let _ = conversations::set_summary(cid, text, upto);
                }
            }
            // Semantic-cache calibration: log whenever the Python side reports a
            // similarity (hit or miss). Pure side-effect; failures non-fatal.
            if let Some(sim) = obj.get("cache_sim").and_then(|v| v.as_f64()) {
                let is_hit = obj.get("cache_hit").and_then(|v| v.as_bool()).unwrap_or(false);
                let decision = if is_hit { "hit" } else { "miss" };
                let model = obj
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default")
                    .to_string();
                let _ = db::insert_cache_calibration(sim, decision, &model, None, "passive");
            }
            // Real usage accounting (PRICING-PLAN PR-1 + ROUTING P1.a): one row
            // per LLM 动作的真实用量，按事件携带的 role（task_type）细分——
            // 轻量=general、子任务=各自 task_type、汇总=aggregate，便于按角色成本归因。
            // 只记带用量的 task_done（每个成功 LLM 调用都会发）；不记失败。
            if ev.event == "task_done" {
                // 读取 role/task_type：1) 事件显式字段；2) 汇总(agg, id=0)；3) 未知
                let role = obj
                    .get("task_type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        obj.get("id")
                            .and_then(|v| v.as_i64())
                            .filter(|&id| id == 0)
                            .map(|_| "aggregate".to_string())
                    })
                    .unwrap_or_else(|| "unknown".to_string());
                let model = obj
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let in_tok = obj.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                let out_tok = obj.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                // task_done 事件不带 cached/reasoning/field_missing，走 0 默认
                let is_hit = obj.get("cache_hit").and_then(|v| v.as_bool()).unwrap_or(false);
                // duration 为秒（Python 计时），换算毫秒落库
                let latency_ms = obj
                    .get("duration")
                    .and_then(|v| v.as_f64())
                    .map(|d| d * 1000.0);
                let provider = models
                    .iter()
                    .find(|m| m.name == model)
                    .map(|m| m.provider.as_str())
                    .unwrap_or("unknown");
                let usage_detail = pricing::UsageDetail {
                    prompt_tokens: in_tok,
                    completion_tokens: out_tok,
                    cached_tokens: 0,
                    reasoning_tokens: 0,
                    cache_creation_tokens: 0,
                    field_missing: false,
                };
                let (mut act_cost, zm) = priced_usage(provider, &model, &usage_detail);
                // P2.b 语义缓存命中省下的金额：未真正调用供应商费用为 0，但本应花费的 act_cost 保留，
                // 用作「缓存为您节省 ¥X」的账实来源（cost 仍记 0）。
                let cache_saved_cost = if is_hit { act_cost } else { 0.0 };
                if is_hit {
                    act_cost = 0.0;
                }
                let _ = db::insert_usage(
                    &model,
                    "default",
                    in_tok,
                    out_tok,
                    act_cost,
                    Some(&role),
                    is_hit,
                    latency_ms,
                    Some(&orchestrate_rid),
                    Some(&db::UsageExtra {
                        cached_tokens: 0,
                        reasoning_tokens: 0,
                        est_cost: 0.0,
                        act_cost,
                        zone_multiplier: zm,
                        conversation_id: conv_for_events.clone(),
                        field_missing: false,
                        cache_saved_cost,
                    }),
                );
                // P3：按 task_done 成功/失败喂健康哨点（模型可达性，无 role 归属冲突）
                if model != "unknown" {
                    let ok = obj.get("error").and_then(|v| v.as_str()).is_none_or(|s| s.is_empty());
                    crate::health::record_outcome(&model, ok);
                }
                // P1.c：按 task_done 是否带 error 下发成功/失败成效信号（skill/model-任务 打点）。
                // 模型解不出时（unknown）无真实归属，跳过打点避免误伤。
                if model != "unknown" && role != "unknown" {
                    let kind = if obj.get("error").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty())
                    {
                        QualitySignalKind::SubtaskFail
                    } else {
                        QualitySignalKind::Success
                    };
                    db::upsert_model_task_score_signal(&model, &role, kind).ok();
                }
                // P4：子任务升档（P4.c）把被跳过的轻量模型按 P3 相同语义记 Escalation 信号——
                // 其质量信号不达标（零成本判别），让路由学习少用该模型。final model 仍记 Success。
                if let Some(ef) = obj.get("escalated_from").and_then(|v| v.as_str()) {
                    if !ef.is_empty() && ef != model && role != "unknown" {
                        db::upsert_model_task_score_signal(ef, &role, QualitySignalKind::Escalation).ok();
                    }
                }
                // P1.d：按 shadow_ratio 概率后台采样双跑（以主 query 作路由样本），不改返回。
                maybe_shadow_sample(models.clone(), role, shadow_query.clone());
            }
        }
        let data = serde_json::to_string(&ev.data).unwrap_or_default();
        Ok::<_, std::io::Error>(bytes::Bytes::from(format!(
            "event: {}\ndata: {}\n\n",
            ev.event, data
        )))
    }));
    Response::builder()
        .header(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))
        .body(body)
        .unwrap()
}

// ── Services (process management) ──

/// A service's status: the port probe is authoritative. The child handle is
/// used to detect *our* spawned process dying while the port stays answered
/// (a stale process holding the port) — reported as "conflict".
async fn services_status(State(state): State<AppState>) -> Json<Value> {
    // Port probes (async HTTP).
    let ai_health = crate::processes::check_ai_health().await;
    let ai_responding = ai_health.status == "ok";
    let ai_ready = ai_health.ready;
    let ollama_responding = crate::processes::check_ollama_health().await;

    // Child handles we manage. `None` means we reused an existing instance
    // (start_* returned Ok(None)); that's healthy, not a conflict.
    let ai_owns = owns_child(&state, "ai");
    let ollama_owns = owns_child(&state, "ollama");

    let service = |name: &str, owns: bool, responding: bool| -> Value {
        match (owns, responding) {
            // We hold a live child OR we reused an existing instance.
            (true, true) | (false, true) => json!({
                "name": name, "status": "Up (healthy)", "healthy": true, "detail": ""
            }),
            (true, false) => json!({
                "name": name,
                "status": "进程存活但无响应",
                "healthy": false,
                "detail": "子进程在运行，但健康检查失败"
            }),
            (false, false) => json!({
                "name": name, "status": "Down", "healthy": false, "detail": ""
            }),
        }
    };

    let ai_status = if ai_responding {
        if ai_ready {
            json!({"name": "AI Service", "status": "Up (healthy)", "healthy": true, "detail": ""})
        } else {
            json!({
                "name": "AI Service",
                "status": "运行但未配置模型",
                "healthy": false,
                "detail": "未配置任何云 API Key 且 Ollama 不可达"
            })
        }
    } else {
        service("AI Service", ai_owns, ai_responding)
    };

    let mut services = json!([
        {
            "name": "Core Server",
            // Reaching this handler proves the server process is alive and
            // responding; expose uptime so the UI can show real state.
            "status": format!("Up ({}s)", state.started_at.elapsed().as_secs()),
            "healthy": true,
            "detail": "HTTP 自检通过",
        },
        service("Ollama", ollama_owns, ollama_responding),
        ai_status,
    ]);
    // Add an install hint to the Ollama entry when it's not installed at all.
    if let Some(ollama) = services.as_array_mut().unwrap().iter_mut().find(|s| s["name"].as_str() == Some("Ollama")) {
        if !crate::processes::ollama_installed() {
            ollama["detail"] = json!("未安装 Ollama。本地模型不可用（云 API 不受影响）。安装: curl -fsSL https://ollama.com/install.sh | sh");
        }
    }
    let healthy = services.as_array().unwrap().iter().filter(|s| s["healthy"].as_bool().unwrap_or(false)).count();
    let running = services.as_array().unwrap().iter().filter(|s| s["status"].as_str().unwrap_or("").starts_with("Up")).count();
    Json(json!({
        "services": services,
        "total": services.as_array().unwrap().len(),
        "healthy": healthy,
        "running": running,
    }))
}

/// True if we hold a live child handle for the named service.
fn owns_child(state: &AppState, name: &str) -> bool {
    let mut guard = state.children.lock().unwrap();
    let child = match name {
        "ai" => guard.ai.as_mut(),
        "ollama" => guard.ollama.as_mut(),
        _ => return false,
    };
    match child {
        Some(c) => c.try_wait().map(|st| st.is_none()).unwrap_or(false),
        None => false,
    }
}

async fn service_start(State(state): State<AppState>, Path(name): Path<String>) -> Json<Value> {
    let result = match name.as_str() {
        "ollama" => start_ollama_proc(&state).await,
        "ai" => start_ai_proc(&state).await,
        other => format!("unknown service: {other}"),
    };
    Json(json!({ "message": result }))
}

async fn service_stop(State(state): State<AppState>, Path(name): Path<String>) -> Json<Value> {
    let result = match name.as_str() {
        "ollama" => stop_ollama_proc(&state),
        "ai" => stop_ai_proc(&state),
        other => format!("unknown service: {other}"),
    };
    Json(json!({ "message": result }))
}

async fn service_restart(State(state): State<AppState>, Path(name): Path<String>) -> Json<Value> {
    let result = match name.as_str() {
        "ollama" => {
            let _ = stop_ollama_proc(&state);
            start_ollama_proc(&state).await
        }
        "ai" => {
            let _ = stop_ai_proc(&state);
            start_ai_proc(&state).await
        }
        other => format!("unknown service: {other}"),
    };
    Json(json!({ "message": result }))
}

async fn service_logs(Path(name): Path<String>) -> Json<Value> {
    let file = match name.as_str() {
        "ollama" => "ollama.log",
        "ai" => "ai.log",
        _ => "ai.log",
    };
    let path = config::log_dir().join(file);
    let content = tokio::task::spawn_blocking(move || std::fs::read_to_string(path).unwrap_or_default())
        .await
        .unwrap_or_default();
    let tail: Vec<&str> = content.lines().rev().take(200).collect();
    let logs: String = tail.iter().rev().map(|s| s.to_string()).collect::<Vec<_>>().join("\n");
    Json(json!({ "logs": logs }))
}

async fn smart_restart(State(state): State<AppState>, Json(action): Json<ServiceAction>) -> Json<Value> {
    let mut restarted = Vec::new();
    let mut errors = Vec::new();
    let _ = action.changed_keys; // any config change triggers an AI service restart
    {
        let mut guard = state.children.lock().unwrap();
        if let Some(child) = guard.ai.as_mut() {
            let _ = child.kill();
            guard.ai = None;
        }
    }
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    match crate::processes::start_ai().await {
        Ok(child) => {
            state.children.lock().unwrap().ai = child;
            restarted.push("AI Service".to_string());
        }
        Err(e) => errors.push(format!("AI service restart failed: {e}")),
    }
    Json(json!({ "ok": errors.is_empty(), "restarted": restarted, "errors": errors }))
}

/// Full cleanup: kill owned child processes (AI service, Ollama) then pkill any
/// external/system-managed instances by name. Shared by the `/api/shutdown`
/// endpoint and the SIGINT/SIGTERM signal handler so both paths leave no stale
/// processes behind on the ports.
pub fn shutdown_all(state: &AppState) {
    // 1. Kill processes we spawned (we hold their Child handles).
    {
        let mut guard = state.children.lock().unwrap();
        if let Some(child) = guard.ai.as_mut() {
            let _ = child.kill();
            guard.ai = None;
        }
        if let Some(child) = guard.ollama.as_mut() {
            let _ = child.kill();
            guard.ollama = None;
        }
    }
    // 2. Kill any external instances we didn't spawn (e.g. started by a
    //    previous run, or a system-managed Ollama). Matches dev + bundled
    //    invocation patterns.
    for pat in [
        "uvicorn api.ai_service:app",
        "ai_service.py --port",
        "ai-service/ai-service",
        "ollama serve",
    ] {
        let _ = Command::new("pkill").args(["-f", pat]).status();
    }
}

async fn shutdown_server(State(state): State<AppState>) -> Json<Value> {
    // Spawn a short-delayed task so the HTTP response is flushed before the
    // process exits. 400ms is enough for axum to write the JSON body back.
    let s = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        println!("[core] shutdown requested via API, cleaning up...");
        shutdown_all(&s);
        println!("[core] all services stopped, exiting.");
        std::process::exit(0);
    });
    Json(json!({ "shutting_down": true }))
}

// ── Semantic-cache management (proxied to the AI service) ──

async fn cache_init() -> Result<Json<Value>> {
    Ok(Json(ai_client::cache_init().await?))
}

async fn cache_status() -> Result<Json<Value>> {
    Ok(Json(ai_client::cache_status().await?))
}

async fn cache_cleanup() -> Result<Json<Value>> {
    Ok(Json(ai_client::cache_cleanup().await?))
}

// ── System ──

async fn open_folder(Json(body): Json<Value>) -> Json<Value> {
    let path = body.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let ok = std::process::Command::new(opener)
        .arg(path)
        .spawn()
        .map(|_| true)
        .unwrap_or(false);
    Json(json!({ "ok": ok }))
}

async fn open_web(Json(body): Json<Value>) -> Json<Value> {
    let url = body.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let _ = std::process::Command::new(opener).arg(url).spawn();
    Json(json!({ "ok": true }))
}

// ── Background jobs (PRICING-PLAN §6.2 / §7) ──

/// 挂载后台任务：日级校准 job + 探针循环 + 24h 定价刷新循环。
/// 在 main 启动 axum::serve 前调用。
pub fn spawn_background_jobs() -> Vec<tokio::task::JoinHandle<()>> {
    vec![
        tokio::spawn(calibration_job()),
        tokio::spawn(crate::probe::probe_loop()),
        tokio::spawn(pricing_refresh_loop()),
        tokio::spawn(health_probe_loop()), // P3 主动探测
    ]
}

/// 日级校准 job：每天聚合昨天用量 → 写 price_calibration → 更新命中率 EWMA →
/// 对账偏差连续越界 3 天则标 price_stale（PRICING-PLAN §6.2）。
async fn calibration_job() {
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(86_400));
    ticker.tick().await; // 跳过立即触发
    loop {
        ticker.tick().await;
        if let Err(e) = run_daily_calibration().await {
            eprintln!("[core] calibration job failed: {e}");
        }
    }
}

/// EWMA α：0.15 ≈ 10 天半衰，对周级调价够灵敏又不抖。
const HIT_RATE_EWMA_ALPHA: f64 = 0.15;
/// stale 判定阈值与去抖天数。
const DRIFT_UPPER: f64 = 1.2;
const DRIFT_LOWER: f64 = 0.8;
const STALE_STREAK_DAYS: i64 = 3;
/// 校准样本下限（低于此样本数不计算，避免单次调用污染）。
const MIN_CALIBRATION_CALLS: i64 = 50;

async fn run_daily_calibration() -> Result<()> {
    let now = now_epoch_secs();
    let (y, m, d, _, _) = pricing::beijing_parts(now - 86_400, 8); // 北京昨天
    let day = format!("{y:04}-{m:02}-{d:02}");
    let rows = db::aggregate_usage_by_model_day(&day)?;
    for r in &rows {
        if r.calls < MIN_CALIBRATION_CALLS {
            continue;
        }
        // 对账比（总额口径：act/est；est_out 误差在 P50 估计下有限）
        let ratio = if r.est_cost > 0.0 { r.act_cost / r.est_cost } else { 1.0 };
        let hit_rate = if r.input_tokens > 0 {
            r.cached_tokens as f64 / r.input_tokens as f64
        } else {
            0.0
        };
        let out_in = if r.input_tokens > 0 {
            r.output_tokens as f64 / r.input_tokens as f64
        } else {
            0.0
        };
        db::upsert_price_calibration(
            &r.provider, &r.model, &day, r.calls,
            r.est_cost, r.act_cost, ratio, hit_rate, out_in, r.field_missing,
        )?;
        // 命中率 EWMA（喂路由 hit_rates——PR-5 落地后读取；当前先行维护）
        let _ = HIT_RATE_EWMA_ALPHA;
        // stale 去抖：连续 3 天越界才标（单日计费异常不误报）
        if ratio > DRIFT_UPPER || ratio < DRIFT_LOWER {
            let streak = db::stale_streak(&r.provider, &r.model, STALE_STREAK_DAYS)?;
            if streak >= STALE_STREAK_DAYS {
                db::mark_price_stale(&r.provider, &r.model, true, "calibration_drift")?;
            }
        }
    }
    Ok(())
}

// ── Pricing + probe API (PRICING-PLAN §10) ──

/// GET /api/pricing/specs[?stale=true] —— 列出 PriceSpec。
async fn pricing_specs(Query(q): Query<HashMap<String, String>>) -> Result<Json<Value>> {
    let mut specs = db::list_price_specs()?;
    if q.get("stale").map(|v| v == "true" || v == "1").unwrap_or(false) {
        specs.retain(|s| s.price_stale);
    }
    Ok(Json(serde_json::to_value(specs).unwrap_or_default()))
}

/// PUT /api/pricing/specs/{provider}/{model} —— 手工改价（强制转正 manual）。
#[derive(Debug, Deserialize)]
struct PriceSpecUpdate {
    input_cost: f64,
    output_cost: f64,
    #[serde(default)]
    cache_read_cost: Option<f64>,
    #[serde(default)]
    cache_write_cost: Option<f64>,
    #[serde(default)]
    reasoning_cost: Option<f64>,
    #[serde(default)]
    tiered_json: Option<String>,
    #[serde(default)]
    zone_ref: Option<String>,
    #[serde(default)]
    cny_list_price_json: Option<String>,
}

async fn pricing_spec_update(
    Path((provider, model)): Path<(String, String)>,
    Json(body): Json<PriceSpecUpdate>,
) -> Result<Json<Value>> {
    // 录入断言：量纲强制 USD/token
    for v in [body.input_cost, body.output_cost] {
        if !(1e-9..=1e-3).contains(&v) {
            return Err(AppError::InvalidRequest(format!(
                "price {v} out of USD/token range [1e-9, 1e-3]; expected per-token value"
            )));
        }
    }
    db::upsert_price_spec(
        &provider,
        &model,
        body.input_cost,
        body.output_cost,
        body.cache_read_cost,
        body.cache_write_cost,
        body.reasoning_cost,
        body.tiered_json.as_deref(),
        body.zone_ref.as_deref(),
        body.cny_list_price_json.as_deref(),
    )?;
    Ok(Json(json!({ "ok": true, "provider": provider, "model": model })))
}

/// GET /api/pricing/calibration?days=30 —— 校准曲线。
async fn pricing_calibration(Query(q): Query<HashMap<String, String>>) -> Result<Json<Value>> {
    let days: i64 = q.get("days").and_then(|v| v.parse().ok()).unwrap_or(30).clamp(1, 365);
    let rows = db::list_price_calibration(days)?;
    Ok(Json(serde_json::to_value(rows).unwrap_or_default()))
}

// ── P2.a 定价刷新（P2.a job + POST /api/pricing/refresh + accept） ──

/// P2.a 刷新编排：拉 litellm 远端价格 → 解析 → 应用到本地「非 manual」行。
/// 主源 jsdelivr，回退 ghproxy 镜像（本机网络受限）；全部不可达返回 Err（调用方静默，保留本地值）。
/// 返回 (更新行数, 远端条数, 被保留的 manual 行数)。
async fn run_pricing_refresh() -> std::result::Result<(usize, usize, usize), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let urls = [
        "https://cdn.jsdelivr.net/gh/BerriAI/litellm@main/model_prices_and_context_window.json",
        "https://mirror.ghproxy.com/https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json",
    ];
    let mut raw: Option<String> = None;
    for u in urls {
        match client.get(u).send().await {
            Ok(r) if r.status().is_success() => {
                if let Ok(t) = r.text().await {
                    raw = Some(t);
                    break;
                }
            }
            _ => continue,
        }
    }
    let Some(raw) = raw else {
        return Err("pricing refresh: all mirrors unreachable".into());
    };
    let remote = pricing::parse_remote_prices(&raw);
    let remote_total = remote.len();
    let specs = db::list_price_specs().map_err(|e| e.to_string())?;
    let mut updated = 0usize;
    let mut manual = 0usize;
    for s in &specs {
        if s.price_source == "manual" {
            manual += 1;
            continue;
        }
        if let Some(rp) = remote.get(&(s.provider.clone(), s.model.clone())) {
            let hit = db::refresh_price_spec(
                &s.provider,
                &s.model,
                rp.input_cost,
                rp.output_cost,
                rp.cache_read_cost,
            )
            .unwrap_or(false);
            if hit {
                updated += 1;
            }
        }
    }
    Ok((updated, remote_total, manual))
}

/// POST /api/pricing/refresh —— 手动触发刷新（不覆盖 manual；断网/镜像不可达返回错误但不动本地值）。
async fn pricing_refresh() -> Result<Json<Value>> {
    match run_pricing_refresh().await {
        Ok((updated, remote_total, manual)) => Ok(Json(json!({
            "ok": true, "updated": updated, "remote_total": remote_total, "manual_kept": manual
        }))),
        Err(e) => Err(AppError::Internal(e)),
    }
}

/// POST /api/pricing/specs/{provider}/{model}/accept —— 采纳刷新价为 manual（此后不被刷新覆盖）。
async fn pricing_accept(Path((provider, model)): Path<(String, String)>) -> Result<Json<Value>> {
    if !db::accept_price_spec(&provider, &model)? {
        return Err(AppError::NotFound(format!("price spec {provider}/{model} not found")));
    }
    Ok(Json(json!({ "ok": true, "provider": provider, "model": model })))
}

/// P2.a 后台 24h 刷新 job：周期拉远端价；断网失败静默（保留本地值，不影响主流程）。
async fn pricing_refresh_loop() {
    let mut int = tokio::time::interval(std::time::Duration::from_secs(86400));
    int.tick().await; // 首个周期：启动后 24h 才首次触发
    loop {
        int.tick().await;
        let _ = run_pricing_refresh().await;
    }
}

/// GET /api/probe/stats —— 探针月消耗/预算/命中验证。
async fn probe_stats() -> Result<Json<Value>> {
    let s = db::probe_stats()?;
    Ok(Json(json!({
        "monthly_limit_usd": crate::probe::budget().monthly_limit_usd(),
        "monthly_limit_cny": crate::probe::budget().monthly_limit_usd() * 7.2,
        "spend_usd": s.spend_usd,
        "rounds": s.rounds,
        "hit_verifications": s.hit_verifications,
        "hit_failures": s.hit_failures,
        "failures": s.failures,
    })))
}

/// PUT /api/probe/budget —— 调整探针月预算（CNY 或 USD，二选一）。
#[derive(Debug, Deserialize)]
struct ProbeBudgetBody {
    #[serde(default)]
    monthly_limit_cny: Option<f64>,
    #[serde(default)]
    monthly_limit_usd: Option<f64>,
}

async fn probe_budget_update(Json(body): Json<ProbeBudgetBody>) -> Json<Value> {
    let usd = body
        .monthly_limit_usd
        .or_else(|| body.monthly_limit_cny.map(|c| c / 7.2))
        .unwrap_or(0.0);
    crate::probe::budget().set_monthly_limit_usd(usd);
    Json(json!({ "ok": true, "monthly_limit_usd": crate::probe::budget().monthly_limit_usd() }))
}

/// P3：routing overhead 报告——快速路径 time-to-decision 的预算与实现健康度。
/// 快路径 >10ms / 全路径 >100ms 视为实现 bug（此处标注 slow 供告警/CI 断言）。
#[derive(Deserialize)]
struct OverheadQuery {
    days: Option<i64>,
}

/// P4.a：子任务级 plan-subtask 回调（Python 每子任务按其 task_type 独立 plan）。
/// 无状态：入 task_type/预估 token/预算档 → 出 primary + fallback 链 + escalation_enabled。
/// 返回模型名，Python 用自己的 models 池解析成 ModelSpec。
#[derive(Deserialize)]
struct PlanSubtaskRequest {
    task_type: String,
    est_in_tokens: Option<i64>,
    est_out_tokens: Option<i64>,
    budget_tier: Option<String>,
}
async fn rust_plan_subtask(Json(req): Json<PlanSubtaskRequest>) -> Result<Json<Value>> {
    let est_in = req.est_in_tokens.unwrap_or(500).max(0);
    let est_out = req.est_out_tokens.unwrap_or(1000).max(0);
    let tier = req.budget_tier.unwrap_or_else(|| "normal".to_string());
    let models = db::list_models(true)?;

    let outcome = match router::plan_for_task(&req.task_type, &models, est_in, est_out, &tier) {
        Ok(o) => o,
        Err(e) => {
            return Ok(Json(json!({
                "primary": null,
                "fallback_chain": [],
                "escalation_enabled": false,
                "tier_req": 0,
                "error": e.to_string(),
            })));
        }
    };
    let escalation_enabled = db::get_routing_policy(&req.task_type)
        .ok()
        .flatten()
        .map(|p| p.escalation_enabled == 1)
        .unwrap_or(false);
    Ok(Json(json!({
        "primary": outcome.primary,
        "fallback_chain": outcome.fallback_chain,
        "escalation_enabled": escalation_enabled,
        "tier_req": 0,
    })))
}

async fn routing_overhead(Query(q): Query<OverheadQuery>) -> Result<Json<Value>> {
    let days = q.days.unwrap_or(0);
    if days < 0 || days > 90 {
        return Err(AppError::InvalidRequest("days 需在 [0,90]".into()));
    }
    let (count, avg, p95, max, slow) = db::routing_overhead_report(days)?;
    // 纯规则型快路径（method != LLM search）应 < 10ms；含 LLM 分类的全路径 < 100ms。
    let fast_path_healthy = avg < 10.0; // 全量均值近似快路径，详细按 method 留后续拆
    Ok(Json(json!({
        "days": days,
        "count": count,
        "avg_ms": avg,
        "p95_ms": p95,
        "max_ms": max,
        "slow_count": slow,
        "fast_path_healthy": fast_path_healthy,
        "note": "快路径>10ms / 全路径>100ms 视为实现 bug",
    })))
}

/// P3：主动探测——对 `down`/`degraded` 模型每 `health.probe_sec` 发最小请求试探恢复。
/// 探针成功 → `down`→`up`（状态机驱动），失败保持，不阻塞主流程。
async fn health_probe_loop() {
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(config::health_probe_sec()));
    ticker.tick().await; // 启动后首个周期才执行
    loop {
        ticker.tick().await;
        // 重新读间隔（运行时调整生效）
        let secs = config::health_probe_sec();
        if secs != ticker.period().as_secs() {
            ticker = tokio::time::interval(std::time::Duration::from_secs(secs));
            ticker.tick().await;
        }
        probe_down_models().await;
    }
}

async fn probe_down_models() {
    let models = match db::list_models(true) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[health] list_models failed: {e}");
            return;
        }
    };
    for m in models.iter().filter(|m| m.health_state == "down" || m.health_state == "degraded") {
        // 最小试探：1 token、低温度、仅确认可达（不产生有意义的答复）。
        let spec = ai_client::ModelSpec::from(m);
        let probe_msg = serde_json::json!([{
            "role": "user",
            "content": "ping"
        }]);
        let ok = match ai_client::chat(&spec, &[probe_msg], 1, 0.0).await {
            Ok(_) => true,
            Err(_) => false,
        };
        let state = crate::health::record_outcome(&m.name, ok);
        if ok {
            eprintln!("[health] probe recovered {} → {state}", m.name);
        }
    }
}

// ── Router ──

pub fn build_router(state: AppState) -> Router {
    // Serve the frontend: static assets from the ui dir, SPA fallback to index.html.
    let ui = config::ui_dir().unwrap_or_default();
    let serve_ui = tower_http::services::ServeDir::new(&ui).not_found_service(
        tower_http::services::ServeFile::new(ui.join("index.html")),
    );

    Router::new()
        // UI (SPA: static files + fallback to index.html) + health
        .fallback_service(serve_ui)
        .route("/api/health", get(health))
        // Models
        .route("/api/models", get(list_models).post(register_model))
        .route("/api/models/{name}", get(get_model).put(update_model).delete(delete_model))
        // Usage + budgets
        .route("/api/usage", get(get_usage))
        .route("/api/budgets", get(list_budgets).post(set_budget).delete(delete_budget))
        .route("/api/budgets/check", get(check_budget))
        // Pricing + probes (PRICING-PLAN §10)
        .route("/api/pricing/specs", get(pricing_specs))
        .route("/api/pricing/specs/{provider}/{model}", put(pricing_spec_update))
        .route("/api/pricing/specs/{provider}/{model}/accept", post(pricing_accept))
        .route("/api/pricing/refresh", post(pricing_refresh))
        .route("/api/pricing/calibration", get(pricing_calibration))
        .route("/api/probe/stats", get(probe_stats))
        .route("/api/probe/budget", put(probe_budget_update))
        // Config + stats
        .route("/api/config", get(get_config).post(update_config))
        .route("/api/stats", get(get_stats))
        // Conversations
        .route("/api/conversations", get(list_conversations).post(save_conversation))
        .route("/api/conversations/{id}", get(get_conversation).put(rename_conversation).delete(delete_conversation))
        // Two-phase persistence: append (phase 1) / fill-in (phase 2)
        .route("/api/conversations/{id}/messages", post(append_conversation_message))
        .route("/api/conversations/{id}/messages/{seq}", patch(update_conversation_message))
        // Chat + orchestrate (SSE)
        .route("/api/chat/stream", post(chat_stream))
        .route("/api/orchestrate/stream", post(orchestrate_stream))
        // P1.d 影子评测 + AIQ 重放数据源
        .route("/api/routing/shadow", post(routing_shadow).get(routing_shadow_status))
        .route("/api/routing/plan-subtask", post(rust_plan_subtask)) // P4.a Python 每子任务回调
        .route("/api/routing/overhead", get(routing_overhead))
        // Services (process management)
        .route("/api/services/status", get(services_status))
        .route("/api/services/{name}/start", post(service_start))
        .route("/api/services/{name}/stop", post(service_stop))
        .route("/api/services/{name}/restart", post(service_restart))
        .route("/api/services/{name}/logs", get(service_logs))
        .route("/api/services/smart-restart", post(smart_restart))
        // System
        .route("/api/system/open-folder", post(open_folder))
        .route("/api/system/open-web", post(open_web))
        .route("/api/shutdown", post(shutdown_server))
        // Semantic cache management
        .route("/api/cache/init", post(cache_init))
        .route("/api/cache/status", get(cache_status))
        .route("/api/cache/cleanup", post(cache_cleanup))
        .route("/api/cache/feedback", post(cache_feedback))
        .route("/api/cache/threshold", get(cache_threshold_get).post(cache_autotune_set))
        .with_state(state)
}

// ── Semantic-cache feedback + self-tuning ──

#[derive(Deserialize)]
struct CacheFeedbackBody {
    sim: f64,
    decision: String,
    correct: bool,
}

async fn cache_feedback(Json(req): Json<CacheFeedbackBody>) -> Json<Value> {
    let decision = if req.decision == "hit" { "hit" } else { "miss" };
    // Record the inline-question answer as a labeled calibration sample.
    let _ = db::insert_cache_calibration(req.sim, decision, "default", Some(req.correct), "inline_question");

    let auto = db::get_setting("cache_auto_tune")
        .ok()
        .flatten()
        .map(|v| v != "0" && v != "false")
        .unwrap_or(true);

    let mut suggested: Option<f64> = None;
    if auto {
        if let Ok(samples) = db::calibration_labeled_samples() {
            if let Some(t) = db::optimal_threshold(&samples, 0.01) {
                let cur = config::cache_threshold();
                let next = cur + 0.5 * (t - cur); // gradual move, avoids abrupt shifts
                if config::set_cache_threshold(next).is_ok() {
                    suggested = Some(next);
                    let _ = db::set_setting("cache_threshold_suggested", &format!("{t:.4}"));
                }
            }
        }
    }
    Json(json!({
        "ok": true,
        "threshold": config::cache_threshold(),
        "suggested": suggested,
        "auto_tune": auto,
    }))
}

// ── P1.d 影子评测（shadow evaluation）──

#[derive(Deserialize)]
struct ShadowBody {
    query: String,
    task_type: Option<String>,
}

/// P1.d FNV-1a（32 位）查询指纹，用作 routing_calibration.query_hash 去重的稳定键。
fn shadow_hash(q: &str) -> String {
    let mut h: u32 = 0x811c9dc5;
    for b in q.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    format!("{:08x}", h)
}

/// P1.d 双跑结果。
struct ShadowSample {
    task_type: String,
    routed_model: String,
    baseline_model: String,
    routed_cost: f64,
    baseline_cost: f64,
}

/// P1.d 双跑核心（手动端点与请求热路径后台采样共用）：
/// 现网 `plan()` 路由选择 × 旗舰基线各跑一次，成本走 `priced_usage` 定价真源，
/// 落 `routing_calibration`（source 区分 shadow 采样/手动端点）。
async fn run_shadow_pair(
    models: Vec<Model>,
    task_type: &str,
    query: &str,
    source: &str,
    dedup: bool,
) -> std::result::Result<ShadowSample, String> {
    if models.is_empty() {
        return Err("无可用模型".to_string());
    }

    // 1) 现网路由：走真实 plan() 看「系统会选谁」；direct/未注册退回注册表首选。
    let classifier = pick_classifier(&models);
    let routing = router::route("auto", query, classifier.as_ref()).await;
    let routed_model = if models.iter().any(|m| m.name == routing.model) {
        routing.model.clone()
    } else {
        models[0].name.clone()
    };

    // 2) 基线：settings 钦定（须已注册），否则取能力档最高者（旗舰）。
    let explicit = db::get_setting("routing.shadow_baseline")
        .ok()
        .flatten()
        .filter(|n| models.iter().any(|m| m.name == *n));
    let baseline_model = explicit.unwrap_or_else(|| {
        models
            .iter()
            .filter(|m| m.is_active != 0)
            .max_by_key(|m| m.capability_tier)
            .map(|m| m.name.clone())
            .unwrap_or_else(|| models[0].name.clone())
    });

    // 3) 双跑（并行），成本统一走定价真源。
    let routed_spec: Option<ModelSpec> = models.iter().find(|m| m.name == routed_model).map(ModelSpec::from);
    let baseline_spec: Option<ModelSpec> = models.iter().find(|m| m.name == baseline_model).map(ModelSpec::from);
    let (routed_cost, baseline_cost) = match (routed_spec, baseline_spec) {
        (Some(r), Some(b)) => {
            let msgs = vec![serde_json::json!({ "role": "user", "content": query })];
            let (rr, br) = tokio::join!(
                ai_client::chat(&r, &msgs, 500, 0.3),
                ai_client::chat(&b, &msgs, 500, 0.3),
            );
            let cost_of = |res: &std::result::Result<ai_client::ChatResult, AppError>, model: &str| -> f64 {
                match res {
                    Ok(x) => {
                        let (c, _) = priced_usage(model, &x.model, &x.usage);
                        c
                    }
                    Err(_) => 0.0,
                }
            };
            (cost_of(&rr, &routed_model), cost_of(&br, &baseline_model))
        }
        _ => return Err("路由/基线模型解析失败".to_string()),
    };

    let t = if task_type.is_empty() {
        routing.task_type.clone()
    } else {
        task_type.to_string()
    };
    // 查询指纹值对真实 query，而非 task_type——否则同任务所有样本同哈希，"防重"失效。
    // 自动采样 dedup=true（避免相同 query 重复膨胀样本数）；手动端点 dedup=false（白名单式审计可重测）。
    let qhash = shadow_hash(query);
    if dedup && db::routing_calibration_exists(&t, &qhash).unwrap_or(false) {
        return Err("该 query 已有影子样本（去重，避免重复膨胀样本数）".to_string());
    }
    let _ = db::insert_routing_calibration(&t, &qhash, &routed_model, &baseline_model, routed_cost, baseline_cost, source);

    Ok(ShadowSample {
        task_type: t,
        routed_model,
        baseline_model,
        routed_cost,
        baseline_cost,
    })
}

/// P1.d 请求热路径自动采样：按 `routing.shadow_ratio`（默认 0.10，0 即零成本关）
/// 以概率后台 spawn 双跑落库，供 AIQ 离线重放积累样本。
/// 不阻塞主响应、不改返回；后台偶发失败静默（AIQ 只需成功样本）。
fn maybe_shadow_sample(models: Vec<Model>, task_type: String, query: String) {
    let ratio = config::shadow_ratio();
    if ratio <= 0.0 {
        return;
    }
    // 简易概率采样：SystemTime 纳秒末位 < ratio 即命中（不引 rand 依赖）。
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as f64)
        .unwrap_or(0.0);
    if nanos / 1_000_000_000.0 >= ratio {
        return;
    }
    tokio::spawn(async move {
        let _ = run_shadow_pair(models, &task_type, &query, "shadow", true).await;
    });
}

/// 手动影子评测端点：强制双跑一条请求，返回路由结果与成本对比（不按采样率）。
/// 采样入口见 [`maybe_shadow_sample`]（请求热路径自动采集）。
async fn routing_shadow(Json(req): Json<ShadowBody>) -> Json<Value> {
    let fail = |msg: &str| Json(json!({ "ok": false, "error": msg }));
    let models = match db::list_models(true) {
        Ok(m) if !m.is_empty() => m,
        Ok(_) => return fail("无可用模型，请先添加"),
        Err(e) => return fail(&e.to_string()),
    };
    let task_type = req.task_type.unwrap_or_default();
    match run_shadow_pair(models, &task_type, &req.query, "shadow", false).await {
        Ok(r) => Json(json!({
            "ok": true,
            "task_type": r.task_type,
            "routed_model": r.routed_model,
            "baseline_model": r.baseline_model,
            "routed_cost": r.routed_cost,
            "baseline_cost": r.baseline_cost,
            "saved": (r.baseline_cost - r.routed_cost).max(0.0),
        })),
        Err(e) => fail(&e),
    }
}

/// 影子评测配置与已采集样本数（AIQ 重放入口）。
async fn routing_shadow_status() -> Json<Value> {
    let ratio = config::shadow_ratio();
    let baseline = db::get_setting("routing.shadow_baseline").ok().flatten();
    let samples = db::count_routing_calibration().unwrap_or(0);
    Json(json!({ "ratio": ratio, "baseline": baseline, "samples": samples }))
}

async fn cache_threshold_get() -> Json<Value> {
    let samples = db::calibration_labeled_samples().map(|s| s.len()).unwrap_or(0);
    let suggested = db::get_setting("cache_threshold_suggested").ok().flatten();
    let auto = db::get_setting("cache_auto_tune")
        .ok()
        .flatten()
        .map(|v| v != "0" && v != "false")
        .unwrap_or(true);
    Json(json!({
        "threshold": config::cache_threshold(),
        "auto_tune": auto,
        "labeled_samples": samples,
        "suggested": suggested,
    }))
}

async fn cache_autotune_set(Json(req): Json<Value>) -> Json<Value> {
    let on = req.get("auto_tune").and_then(|v| v.as_bool()).unwrap_or(true);
    let _ = db::set_setting("cache_auto_tune", if on { "1" } else { "0" });
    // Manual override: an explicit threshold pins it (and implies auto-tune off).
    if let Some(t) = req.get("threshold").and_then(|v| v.as_f64()) {
        let _ = config::set_cache_threshold(t);
        let _ = db::set_setting("cache_auto_tune", "0");
    }
    Json(json!({ "ok": true, "auto_tune": on, "threshold": config::cache_threshold() }))
}

// ── Private helpers ──

/// 分类器选择：注册表驱动（P0.d，去名称硬编码）——
/// 分类是 easy 任务，取能力档最低、价最便宜的 active 模型；并列取名序稳定。
fn pick_classifier(models: &[Model]) -> Option<ModelSpec> {
    let mut pool: Vec<&Model> = models.iter().collect();
    pool.sort_by(|a, b| {
        a.capability_tier
            .cmp(&b.capability_tier)
            .then(a.input_cost_per_token.partial_cmp(&b.input_cost_per_token).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.name.cmp(&b.name))
    });
    pool.first().map(|m| ModelSpec::from(*m))
}

fn blocked_response(sec: &SecurityReport) -> Response {
    let body = json!({
        "error": true,
        "block_reason": sec.block_reason,
        "detail": sec.pii,
    });
    Response::builder()
        .header(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))
        .body(Body::from(format!("data: {}\n\n", body.to_string())))
        .unwrap()
}

/// SSE 格式的单事件错误响应（保持前端 chat 流的错误解析一致）
fn sse_error(detail: &str) -> Response {
    let body = json!({ "error": true, "detail": detail });
    Response::builder()
        .header(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))
        .body(Body::from(format!("data: {}\n\n", body.to_string())))
        .unwrap()
}

fn err_response(e: AppError) -> Response {
    let status = StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(json!({ "error": e.to_string() }))).into_response()
}

fn write_env(updates: &HashMap<String, String>) -> Result<()> {
    let env_path = config::env_file_path();
    let mut env = config::read_env();
    for (k, v) in updates {
        // Defense-in-depth: skip masked values sent back by the frontend (the
        // get_config endpoint masks secrets as "****xxxx"). Accepting them
        // would overwrite the real key with the mask. An unchanged secret is
        // represented by the mask; only non-mask values are written.
        if v.starts_with("****") {
            continue;
        }
        env.insert(k.clone(), v.clone());
    }
    let mut keys: Vec<&String> = env.keys().collect();
    keys.sort();
    let mut out = String::new();
    for k in keys {
        out.push_str(&format!("{k}={}\n", env.get(k).unwrap()));
    }
    std::fs::write(&env_path, out).map_err(AppError::Io)
}

async fn start_ollama_proc(state: &AppState) -> String {
    // start_ollama does a port probe internally; do it before locking.
    match crate::processes::start_ollama().await {
        Ok(child) => {
            let mut guard = state.children.lock().unwrap();
            guard.ollama = child; // None → already running, keep handle
            "Ollama started".to_string()
        }
        Err(e) => format!("Failed to start Ollama: {e}"),
    }
}

fn stop_ollama_proc(state: &AppState) -> String {
    let owned = {
        let mut guard = state.children.lock().unwrap();
        if let Some(child) = guard.ollama.as_mut() {
            let _ = child.kill();
            guard.ollama = None;
            true
        } else {
            false
        }
    };
    if owned {
        return "Ollama stopped".to_string();
    }
    // Not spawned by us (external/system-managed instance): terminate it by name.
    crate::processes::stop_ollama()
}

async fn start_ai_proc(state: &AppState) -> String {
    // start_ai does a health probe internally; do it before locking.
    match crate::processes::start_ai().await {
        Ok(child) => {
            let mut guard = state.children.lock().unwrap();
            guard.ai = child; // None → already running, keep handle
            "AI service started".to_string()
        }
        Err(e) => format!("Failed to start AI service: {e}"),
    }
}

fn stop_ai_proc(state: &AppState) -> String {
    let owned = {
        let mut guard = state.children.lock().unwrap();
        if let Some(child) = guard.ai.as_mut() {
            let _ = child.kill();
            guard.ai = None;
            true
        } else {
            false
        }
    };
    if owned {
        return "AI service stopped".to_string();
    }
    // Not spawned by us (external/dev instance): terminate it by name.
    crate::processes::stop_ai()
}
