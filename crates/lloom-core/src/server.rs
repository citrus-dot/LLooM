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
use crate::router;
use crate::security;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
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

    // Routing: regex tier + domain enhancement
    let models = match db::list_models(true) {
        Ok(m) => m,
        Err(e) => return err_response(e),
    };
    let classifier = pick_classifier(&models);
    let sr_domain = req.sr_domain.clone().unwrap_or_default();
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

    // Resolve the routed model's AI spec
    let spec: ModelSpec = models
        .iter()
        .find(|m| m.name == routing.model)
        .map(ModelSpec::from)
        .unwrap_or_else(|| ModelSpec {
            name: routing.model.clone(),
            litellm_model: routing.model.clone(),
            api_base: String::new(),
            api_key: String::new(),
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
        });

    let head = format!(
        "data: {}\n\n",
        json!({
            "routing": routing,
            "security": { "domain": sec.domain, "domain_method": sec.domain_method, "pii": sec.pii }
        })
    );

    // Direct async AI call (reqwest is async; safe on the tokio executor)
    let tail = match ai_client::chat(&spec, &processed_messages, 500, 0.3).await {
        Ok(res) => format!(
            "data: {}\n\n",
            json!({
                "done": true,
                "content": res.content,
                "model": res.model,
                "cost": res.cost,
                "input_tokens": res.input_tokens,
                "output_tokens": res.output_tokens,
            })
        ),
        Err(e) => format!("data: {}\n\n", json!({ "error": true, "detail": e.to_string() })),
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

    let events = match ai_client::orchestrate_stream(
        &req.query,
        &history,
        req.sr_domain.as_deref().unwrap_or(""),
        &specs,
        &cache_dir,
        conversation_id.as_deref(),
        summary.as_deref(),
        summary_upto,
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
            if let Some(sim) = obj.get("cache_sim").and_then(|v| v.as_f64()) {
                let is_hit = obj.get("cache_hit").and_then(|v| v.as_bool()).unwrap_or(false);
                let decision = if is_hit { "hit" } else { "miss" };
                let model = obj
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default")
                    .to_string();
                let _ = db::insert_cache_calibration(sim, decision, &model, None, "passive");
                if ev.event == "result" {
                    // Real usage accounting: the Python side now reports actual
                    // token counts / cost from litellm (was hard-coded 0).
                    let in_tok = obj.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                    let out_tok = obj.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                    let cost = obj.get("cost").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let _ = db::insert_usage(&model, "default", in_tok, out_tok, cost, None, is_hit);
                }
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

fn pick_classifier(models: &[Model]) -> Option<ModelSpec> {
    for name in ["qwen3.6-flash", "qwen3-max", "qwen-plus"] {
        if let Some(m) = models.iter().find(|m| &m.name == name) {
            return Some(m.into());
        }
    }
    models.first().map(ModelSpec::from)
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
