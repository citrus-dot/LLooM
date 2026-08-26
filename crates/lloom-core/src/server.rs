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
    Ok(Json(json!({ "usage": stats, "total_spend": total })))
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
    let routed_model: Option<&Model> = models.iter().find(|m| m.name == routing.model);
    let (provider, spec): (&str, ModelSpec) = match routed_model {
        Some(m) => (m.provider.as_str(), ModelSpec::from(m)),
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

    // Direct async AI call (reqwest is async; safe on the tokio executor)
    let tail = match ai_client::chat(&spec, &processed_messages, 500, 0.3).await {
        Ok(res) => {
            if decision_id > 0 {
                let _ = db::update_routing_decision_outcome(decision_id, "success");
            }
            // PRICING-PLAN §4.2/§6.1：Rust 单一计价真源，按真实 usage 分项计算并落库。
            // （PR-5 落地前 est_cost 传 0；task_type 用路由分类结果）
            let (act_cost, zm) = priced_usage(provider, &res.model, &res.usage);
            let _ = db::insert_usage(
                &res.model,
                "default",
                res.usage.prompt_tokens,
                res.usage.completion_tokens,
                act_cost,
                Some(&routing_task_type),
                false,
                Some(&db::UsageExtra {
                    cached_tokens: res.usage.cached_tokens,
                    reasoning_tokens: res.usage.reasoning_tokens,
                    est_cost: 0.0,
                    act_cost,
                    zone_multiplier: zm,
                    conversation_id: None,
                    field_missing: res.usage.field_missing,
                }),
            );
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
            // Real usage accounting (PRICING-PLAN PR-1): unconditional on the
            // final `result` event — no longer gated behind cache_sim, which
            // previously dropped every non-cached orchestration.
            if ev.event == "result" {
                let model = obj
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default")
                    .to_string();
                let usage_v = obj.get("usage").cloned().unwrap_or(json!({}));
                let in_tok = usage_v
                    .get("prompt_tokens")
                    .and_then(|v| v.as_i64())
                    .or_else(|| obj.get("input_tokens").and_then(|v| v.as_i64()))
                    .unwrap_or(0);
                let out_tok = usage_v
                    .get("completion_tokens")
                    .and_then(|v| v.as_i64())
                    .or_else(|| obj.get("output_tokens").and_then(|v| v.as_i64()))
                    .unwrap_or(0);
                let cached = usage_v.get("cached_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                let reasoning = usage_v.get("reasoning_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                let field_missing = usage_v
                    .get("field_missing")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let is_hit = obj.get("cache_hit").and_then(|v| v.as_bool()).unwrap_or(false);
                let provider = models
                    .iter()
                    .find(|m| m.name == model)
                    .map(|m| m.provider.as_str())
                    .unwrap_or("unknown");
                let usage_detail = pricing::UsageDetail {
                    prompt_tokens: in_tok,
                    completion_tokens: out_tok,
                    cached_tokens: cached,
                    reasoning_tokens: reasoning,
                    cache_creation_tokens: 0,
                    field_missing,
                };
                let (mut act_cost, zm) = priced_usage(provider, &model, &usage_detail);
                if is_hit {
                    // 语义缓存命中：未真正调用供应商，费用为 0
                    act_cost = 0.0;
                }
                let _ = db::insert_usage(
                    &model,
                    "default",
                    in_tok,
                    out_tok,
                    act_cost,
                    Some("orchestrate"),
                    is_hit,
                    Some(&db::UsageExtra {
                        cached_tokens: cached,
                        reasoning_tokens: reasoning,
                        est_cost: 0.0,
                        act_cost,
                        zone_multiplier: zm,
                        conversation_id: conv_for_events.clone(),
                        field_missing,
                    }),
                );
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

/// 挂载后台任务：日级校准 job + 探针循环。在 main 启动 axum::serve 前调用。
pub fn spawn_background_jobs() -> Vec<tokio::task::JoinHandle<()>> {
    vec![
        tokio::spawn(calibration_job()),
        tokio::spawn(crate::probe::probe_loop()),
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
