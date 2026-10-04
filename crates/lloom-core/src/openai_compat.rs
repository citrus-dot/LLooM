//! N1：OpenAI 兼容代理（NEXT-PLAN §二）。
//!
//! 任意 OpenAI 客户端（ChatBox / Open WebUI / 沉浸式翻译 / Agent 框架）把
//! `http://127.0.0.1:7861/v1` 当上游即可零改造获得 LLooM 的：
//! - `router::route()` 评分路由（注册名直连 / `auto` 智能路由）
//! - `chat_with_failover` 健康容灾（P3）
//! - `priced_usage` 计价真源 + `insert_usage` 落库（api_source='proxy'，C3）
//! - `security::check` 安全检查（与 WebUI chat 路径同规则）
//!
//! 端点：
//! - `POST /v1/chat/completions`（流/非流）
//! - `GET /v1/models`
//! - 鉴权：SQLite 多 API Key，明文仅创建时返回，服务端只保存 SHA-256。
//!
//! 管理端点（本模块）：`GET /api/proxy/config`（接入信息+掩码回显）、
//! `PUT /api/proxy/token`（设置/清除，`****` 掩码哨兵=保持原值）、
//! `POST /api/proxy/selftest`（服务端环回自测 /v1/models）。

use crate::db;
use crate::models::Model;
use crate::router;
use crate::security;
use crate::server::{chat_with_failover, pick_classifier, priced_usage, AppState, FailoverRequest};
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

// ── Request / response shapes ──

/// OpenAI `POST /v1/chat/completions` 请求体（兼容子集：
/// model / messages / temperature / max_tokens / stream）。
#[derive(Debug, Deserialize)]
pub struct OpenAiChatRequest {
    pub model: String,
    pub messages: Vec<Value>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<i64>,
    #[serde(default)]
    pub stream: Option<bool>,
}

// ── Pure helpers（可单测，不触 DB/网络）──

/// model 参数语义（NEXT-PLAN 契约）：注册表内激活名 → 原样直连；
/// `"auto"`、空串或未知名 → 智能路由。
pub(crate) fn resolve_model_param(requested: &str, registered: &[&str]) -> String {
    if registered.contains(&requested) {
        requested.to_string()
    } else {
        "auto".to_string()
    }
}

/// OpenAI 风格错误体。
fn error_body(message: &str, typ: &str, code: &str) -> Value {
    json!({ "error": { "message": message, "type": typ, "code": code } })
}

fn error_response(status: StatusCode, message: &str, typ: &str, code: &str) -> Response {
    (status, Json(error_body(message, typ, code))).into_response()
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn new_completion_id() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("chatcmpl-{ms:x}")
}

/// 非流式响应体：标准 `choices` + `usage`（取自 ChatResult 真实 usage）。
pub(crate) fn completion_json(
    id: &str,
    created: i64,
    model: &str,
    content: &str,
    prompt_tokens: i64,
    completion_tokens: i64,
) -> Value {
    json!({
        "id": id,
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": content },
            "finish_reason": "stop",
        }],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": prompt_tokens + completion_tokens,
        },
    })
}

/// 流式 SSE 帧序列：`role delta → content delta → finish_reason → [DONE]`。
/// 底层调用是非流式 `ai_client::chat`，content 以单帧整段下发（帧契约完整，
/// 客户端零改造兼容；token 级真流式留待后续按需开）。
#[cfg(test)]
pub(crate) fn sse_frames(id: &str, created: i64, model: &str, content: &str) -> String {
    let chunk = |delta: Value, finish: Value| {
        json!({
            "id": id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{ "index": 0, "delta": delta, "finish_reason": finish }],
        })
    };
    let role = chunk(json!({ "role": "assistant" }), Value::Null);
    let content_frame = chunk(json!({ "content": content }), Value::Null);
    let finish = chunk(json!({}), json!("stop"));
    format!("data: {role}\n\ndata: {content_frame}\n\ndata: {finish}\n\ndata: [DONE]\n\n")
}

fn sse_response(body: String) -> Response {
    Response::builder()
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream; charset=utf-8"),
        )
        .body(Body::from(body))
        .unwrap()
}

fn openai_chunk(id: &str, created: i64, model: &str, delta: Value, finish: Value) -> String {
    format!(
        "data: {}\n\n",
        json!({
            "id":id,"object":"chat.completion.chunk","created":created,"model":model,
            "choices":[{"index":0,"delta":delta,"finish_reason":finish}]
        })
    )
}

async fn send_frame(tx: &tokio::sync::mpsc::Sender<String>, frame: String) -> bool {
    tx.send(frame).await.is_ok()
}

// ── Handlers ──

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .trim()
        .strip_prefix("Bearer ")
}

fn key_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

fn authenticate(db: &db::Db, headers: &HeaderMap, model: &str) -> Option<i64> {
    let keys = db.list_api_keys().ok()?;
    if keys.is_empty() {
        return Some(0);
    }
    let token = bearer_token(headers)?;
    db.authenticate_api_key(&key_hash(token), model)
        .ok()
        .flatten()
}

/// `POST /v1/chat/completions`
pub async fn chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<OpenAiChatRequest>,
) -> Response {
    let Some(api_key_id) = authenticate(&state.db, &headers, &req.model) else {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "Invalid API key",
            "invalid_request_error",
            "invalid_api_key",
        );
    };

    // 安全检查与 WebUI chat 路径同规则（security::check 只查当前请求文本）
    // 边界校验（习惯③）：畸形 messages 止步于代理边界，不流入 AI 服务
    if let Err(msg) = crate::server::validate_messages(&req.messages) {
        return error_response(
            StatusCode::BAD_REQUEST,
            &msg,
            "invalid_request_error",
            "invalid_messages",
        );
    }
    let user_text = security::extract_user_text(&req.messages);
    let sec = security::check(&user_text, true, true);
    if sec.blocked {
        return error_response(
            StatusCode::BAD_REQUEST,
            "请求被安全策略拦截",
            "invalid_request_error",
            "content_blocked",
        );
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

    let models = match state.db.list_models(true) {
        Ok(m) if !m.is_empty() => m,
        Ok(_) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "注册表为空：请先在模型页添加可用模型",
                "invalid_request_error",
                "no_models",
            )
        }
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("读取模型注册表失败：{e}"),
                "server_error",
                "internal",
            )
        }
    };

    let classifier = pick_classifier(&models);
    let registered: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
    let route_param = resolve_model_param(&req.model, &registered);
    let routing = router::route(
        &state.db,
        &route_param,
        &user_text,
        classifier.as_ref(),
        None,
        None, // 代理请求无 lloom 会话上下文（B15 证据仅在 WebUI chat 路径注入）
    )
    .await;

    let Some(primary) = models.iter().find(|m| m.name == routing.model) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            &format!("模型 '{}' 未注册或未启用，请先在模型页添加", routing.model),
            "invalid_request_error",
            "model_not_found",
        );
    };
    let primary_provider = primary.provider_name().to_string();

    // 审计落库（与 chat_stream 同款：决策快照 + 耗时，outcome 调用后回填）
    let routing_task_type = routing.task_type.clone();
    // C2：plan 路径主选的输入侧事前估算（direct 为 0）
    let routing_est_input_cost = routing.est_input_cost;
    let request_id = format!(
        "proxy-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let decision_id = if routing.method != "direct" {
        let signals_json = serde_json::to_string(&json!({
            "method": routing.method,
            "api": "openai_compat",
            "budget_tier": routing.budget_tier,
        }))
        .unwrap_or_default();
        let candidates_json = serde_json::to_string(&routing.fallback_chain).unwrap_or_default();
        state
            .db
            .insert_routing_decision(&db::RoutingDecisionRecord {
                request_id: &request_id,
                task_type: &routing.task_type,
                band: &routing.band,
                signals_json: &signals_json,
                candidates_json: &candidates_json,
                selected: &routing.model,
                fallback_chain: &routing.fallback_chain.join(","),
                routing_ms: 0.0,
            })
            .unwrap_or(0)
    } else {
        0
    };

    // 客户端未指定时的缺省：max_tokens 给足（代理场景常见长输出，WebUI 内部
    // 路径仍为 500）；temperature 沿用 OpenAI 语义缺省 1.0（客户端显式值优先）
    let max_tokens = req.max_tokens.unwrap_or(4096);
    let temperature = req.temperature.unwrap_or(1.0);

    if req.stream.unwrap_or(false) {
        let mut names = vec![routing.model.clone()];
        names.extend(routing.fallback_chain.clone());
        names.dedup();
        let mut selected = None;
        let mut first_error = None;
        for name in names {
            let Some(model) = models.iter().find(|m| m.name == name) else {
                continue;
            };
            let spec = crate::ai_client::ModelSpec::from(model);
            match crate::providers::stream(&spec, &processed_messages, max_tokens, temperature)
                .await
            {
                Ok(stream) => {
                    selected = Some((model.clone(), stream));
                    break;
                }
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error.to_string());
                    }
                }
            }
        }
        let Some((selected_model, mut stream)) = selected else {
            let detail = first_error.unwrap_or_else(|| "all providers failed".into());
            return sse_response(format!(
                "data: {}\n\ndata: [DONE]\n\n",
                error_body(&detail, "server_error", "upstream_failure")
            ));
        };
        let model_name = selected_model.name.clone();
        let provider = selected_model.provider_name().to_string();
        let db = state.db.clone();
        let id = new_completion_id();
        let created = now_secs();
        let (tx, rx) = tokio::sync::mpsc::channel::<String>(64);
        if !send_frame(
            &tx,
            openai_chunk(
                &id,
                created,
                &model_name,
                json!({"role":"assistant"}),
                Value::Null,
            ),
        )
        .await
        {
            return sse_response(String::new());
        }
        tokio::spawn(async move {
            use futures::StreamExt;
            let mut usage = crate::pricing::UsageDetail::default();
            while let Some(item) = stream.next().await {
                match item {
                    Ok(crate::providers::StreamEvent::Content(delta)) => {
                        if !send_frame(
                            &tx,
                            openai_chunk(
                                &id,
                                created,
                                &model_name,
                                json!({"content":delta}),
                                Value::Null,
                            ),
                        )
                        .await
                        {
                            return;
                        }
                    }
                    Ok(crate::providers::StreamEvent::Usage(value)) => usage = value,
                    Ok(crate::providers::StreamEvent::Done) => break,
                    Ok(crate::providers::StreamEvent::Reasoning(_)) => {}
                    Err(error) => {
                        send_frame(
                            &tx,
                            format!(
                                "data: {}\n\n",
                                error_body(&error.to_string(), "server_error", "upstream_failure")
                            ),
                        )
                        .await;
                        return;
                    }
                }
            }
            let (cost, _, _) = priced_usage(&db, &provider, &model_name, &usage);
            if api_key_id > 0 {
                if let Err(error) = db.add_api_key_usage(api_key_id, cost) {
                    eprintln!("[proxy] key usage update failed: {error}");
                }
            }
            let final_frame = format!(
                "{}data: [DONE]\n\n",
                openai_chunk(&id, created, &model_name, json!({}), json!("stop"))
            );
            send_frame(&tx, final_frame).await;
        });
        let body = Body::from_stream(futures::stream::unfold(rx, |mut rx| async move {
            rx.recv()
                .await
                .map(|frame| (Ok::<_, std::io::Error>(bytes::Bytes::from(frame)), rx))
        }));
        return Response::builder()
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/event-stream; charset=utf-8"),
            )
            .body(body)
            .unwrap();
    }

    let chat_start = std::time::Instant::now();
    let result = chat_with_failover(
        &state.db,
        FailoverRequest {
            models: &models,
            task_type: &routing_task_type,
            primary: &routing.model,
            fallback_chain: &routing.fallback_chain,
            messages: &processed_messages,
            max_tokens,
            temperature,
            cache: None, // 代理请求无 lloom 会话/缓存命名空间
        },
    )
    .await;

    match result {
        Ok((res, used_model)) => {
            if decision_id > 0 {
                let _ = state
                    .db
                    .update_routing_decision_outcome(decision_id, "success");
            }
            let provider = models
                .iter()
                .find(|m| m.name == used_model)
                .map(|m| m.provider_name())
                .unwrap_or(primary_provider.as_str());
            let latency_ms = chat_start.elapsed().as_secs_f64() * 1000.0;
            let (act_cost, act_input_cost, zm) =
                priced_usage(&state.db, provider, &used_model, &res.usage);
            let _ = state.db.insert_usage(&db::UsageRecord {
                model_name: &used_model,
                user_id: "default",
                input_tokens: res.usage.prompt_tokens,
                output_tokens: res.usage.completion_tokens,
                cost: act_cost,
                task_type: Some(&routing_task_type),
                cache_hit: false,
                latency_ms: Some(latency_ms),
                request_id: Some(&request_id),
                extra: Some(db::UsageExtra {
                    cached_tokens: res.usage.cached_tokens,
                    reasoning_tokens: res.usage.reasoning_tokens,
                    est_cost: 0.0,
                    act_cost,
                    zone_multiplier: zm,
                    conversation_id: None,
                    field_missing: res.usage.field_missing,
                    cache_saved_cost: 0.0,
                    api_source: Some("proxy".to_string()),
                    est_input_cost: routing_est_input_cost,
                    act_input_cost,
                }),
            });
            if api_key_id > 0 {
                let _ = state.db.add_api_key_usage(api_key_id, act_cost);
            }
            state
                .db
                .upsert_model_task_score_signal(
                    &used_model,
                    &routing_task_type,
                    crate::models::QualitySignalKind::Success,
                )
                .ok();

            let id = new_completion_id();
            let created = now_secs();
            Json(completion_json(
                &id,
                created,
                &used_model,
                &res.content,
                res.usage.prompt_tokens,
                res.usage.completion_tokens,
            ))
            .into_response()
        }
        Err(e) => {
            if decision_id > 0 {
                let _ = state
                    .db
                    .update_routing_decision_outcome(decision_id, "failed");
            }
            let msg = e.to_string();
            error_response(
                StatusCode::BAD_GATEWAY,
                &msg,
                "server_error",
                "upstream_failure",
            )
        }
    }
}

/// `GET /v1/models`：激活模型列表（`auto` 恒在首位，方便客户端直接选用）。
pub async fn models_list(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if authenticate(&state.db, &headers, "auto").is_none() {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "Invalid API key",
            "invalid_request_error",
            "invalid_api_key",
        );
    }
    let models: Vec<Model> = match state.db.list_models(true) {
        Ok(m) => m,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("读取模型注册表失败：{e}"),
                "server_error",
                "internal",
            )
        }
    };
    let created = now_secs();
    let mut data = vec![json!({
        "id": "auto",
        "object": "model",
        "created": created,
        "owned_by": "lloom",
    })];
    data.extend(models.iter().map(|m| {
        json!({
            "id": m.name,
            "object": "model",
            "created": created,
            "owned_by": m.provider_name(),
        })
    }));
    Json(json!({ "object": "list", "data": data })).into_response()
}

// ── API Key management ──

/// 代理接入信息投影（base_url 恒用 127.0.0.1——0.0.0.0 下的真实局域网 IP
/// 服务端不可知，前端按 `bind` 字段提示替换）。
fn proxy_config_json(state: &AppState) -> Value {
    let bind = crate::config::bind_addr();
    let port = crate::config::web_port();
    let count = state.db.list_api_keys().map(|v| v.len()).unwrap_or(0);
    json!({
        "base_url": format!("http://127.0.0.1:{port}/v1"),
        "bind": bind,
        "web_port": port,
        "auth_enabled": count > 0,
        "key_count": count,
    })
}

/// `GET /api/proxy/config`：接入信息（Base URL / 绑定 / 鉴权状态 / 掩码 token）。
pub async fn proxy_config(State(state): State<AppState>) -> Response {
    Json(proxy_config_json(&state)).into_response()
}

#[derive(Debug, Deserialize)]
pub struct ApiKeyCreate {
    pub name: String,
    pub quota_usd: Option<f64>,
    pub rpm: Option<i64>,
    #[serde(default)]
    pub allowed_models: Vec<String>,
    pub expires_at: Option<String>,
}
#[derive(Debug, Deserialize)]
pub struct ApiKeyPatch {
    pub name: String,
    pub status: String,
    pub quota_usd: Option<f64>,
    pub rpm: i64,
    #[serde(default)]
    pub allowed_models: Vec<String>,
    pub expires_at: Option<String>,
}

pub async fn api_keys_list(State(state): State<AppState>) -> Response {
    match state.db.list_api_keys() {
        Ok(keys) => Json(json!({"keys": keys.into_iter().map(|k| json!({
            "id":k.id,"name":k.name,"key_prefix":k.key_prefix,"status":k.status,
            "quota_usd":k.quota_usd,"used_usd":k.used_usd,"rpm":k.rpm,
            "allowed_models":serde_json::from_str::<Value>(&k.allowed_models).unwrap_or(json!([])),
            "expires_at":k.expires_at,"created_at":k.created_at,"last_used_at":k.last_used_at
        })).collect::<Vec<_>>() }))
        .into_response(),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &e.to_string(),
            "server_error",
            "internal",
        ),
    }
}

pub async fn api_key_create(
    State(state): State<AppState>,
    Json(body): Json<ApiKeyCreate>,
) -> Response {
    use rand::RngCore;
    if body.name.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "名称不能为空",
            "invalid_request_error",
            "invalid_name",
        );
    }
    let mut random = [0u8; 24];
    rand::rng().fill_bytes(&mut random);
    let secret = format!(
        "llm-{}",
        random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let prefix = format!("{}…{}", &secret[..10], &secret[secret.len() - 4..]);
    let models = serde_json::to_string(&body.allowed_models).unwrap_or_else(|_| "[]".into());
    let write = db::ApiKeyWrite {
        name: body.name.trim(),
        status: "active",
        quota_usd: body.quota_usd,
        rpm: body.rpm.unwrap_or(60).clamp(1, 10000),
        allowed_models: &models,
        expires_at: body.expires_at.as_deref(),
    };
    match state.db.insert_api_key(&key_hash(&secret), &prefix, &write) {
        Ok(id) => Json(json!({"id":id,"key":secret,"key_prefix":prefix})).into_response(),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &e.to_string(),
            "server_error",
            "internal",
        ),
    }
}

pub async fn api_key_update(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<i64>,
    Json(body): Json<ApiKeyPatch>,
) -> Response {
    let models = serde_json::to_string(&body.allowed_models).unwrap_or_else(|_| "[]".into());
    let write = db::ApiKeyWrite {
        name: &body.name,
        status: &body.status,
        quota_usd: body.quota_usd,
        rpm: body.rpm.clamp(1, 10000),
        allowed_models: &models,
        expires_at: body.expires_at.as_deref(),
    };
    match state.db.update_api_key(id, &write) {
        Ok(_) => Json(json!({"updated":true})).into_response(),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &e.to_string(),
            "server_error",
            "internal",
        ),
    }
}

pub async fn api_key_delete(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Response {
    match state.db.delete_api_key(id) {
        Ok(_) => Json(json!({"deleted":true})).into_response(),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &e.to_string(),
            "server_error",
            "internal",
        ),
    }
}

/// `POST /api/proxy/selftest`：服务端环回自测——带解析出的 token 请求自身
/// `/v1/models`，验证「端点可达 + 鉴权链正确」一步到位。
pub async fn proxy_selftest(State(_state): State<AppState>) -> Response {
    let port = crate::config::web_port();
    let url = format!("http://127.0.0.1:{port}/v1/models");
    let started = std::time::Instant::now();
    let req = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(5));
    match req.send().await {
        Ok(res) => {
            let http = res.status().as_u16();
            let latency_ms = started.elapsed().as_secs_f64() * 1000.0;
            let models = res
                .json::<Value>()
                .await
                .ok()
                .and_then(|v| v.get("data").and_then(|d| d.as_array()).cloned())
                .map(|a| a.len());
            let ok = http == 200 && models.is_some();
            Json(json!({
                "ok": ok,
                "http": http,
                "models": models,
                "latency_ms": (latency_ms * 100.0).round() / 100.0,
                "detail": if ok { "代理端点可达，鉴权链正确".to_string() }
                          else { format!("HTTP {http}——检查 token 或服务状态") },
            }))
            .into_response()
        }
        Err(e) => Json(json!({
            "ok": false,
            "detail": format!("请求失败：{e}"),
        }))
        .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_model_registered_name_direct_else_auto() {
        let reg = ["qwen-plus", "deepseek-v3"];
        assert_eq!(resolve_model_param("qwen-plus", &reg), "qwen-plus");
        assert_eq!(resolve_model_param("auto", &reg), "auto");
        assert_eq!(resolve_model_param("", &reg), "auto");
        // 未注册名 → 智能路由（NEXT-PLAN 契约），不按 direct 失败
        assert_eq!(resolve_model_param("gpt-99", &reg), "auto");
    }

    #[test]
    fn completion_json_matches_openai_shape() {
        let v = completion_json("chatcmpl-1", 123, "qwen-plus", "你好", 10, 5);
        assert_eq!(v["object"], "chat.completion");
        assert_eq!(v["choices"][0]["message"]["role"], "assistant");
        assert_eq!(v["choices"][0]["message"]["content"], "你好");
        assert_eq!(v["choices"][0]["finish_reason"], "stop");
        assert_eq!(v["usage"]["prompt_tokens"], 10);
        assert_eq!(v["usage"]["completion_tokens"], 5);
        assert_eq!(v["usage"]["total_tokens"], 15);
    }

    #[test]
    fn sse_frames_full_sequence_ends_with_done() {
        let s = sse_frames("chatcmpl-1", 123, "qwen-plus", "答案内容");
        let frames: Vec<&str> = s.split("data: ").filter(|f| !f.is_empty()).collect();
        assert_eq!(frames.len(), 4, "role + content + finish + [DONE]");
        assert!(s.ends_with("data: [DONE]\n\n"));
        // 帧 1：role delta
        let f1: Value = serde_json::from_str(frames[0].trim()).unwrap();
        assert_eq!(f1["choices"][0]["delta"]["role"], "assistant");
        assert!(f1["choices"][0]["finish_reason"].is_null());
        // 帧 2：content delta
        let f2: Value = serde_json::from_str(frames[1].trim()).unwrap();
        assert_eq!(f2["choices"][0]["delta"]["content"], "答案内容");
        assert_eq!(f2["object"], "chat.completion.chunk");
        // 帧 3：finish_reason stop、delta 为空
        let f3: Value = serde_json::from_str(frames[2].trim()).unwrap();
        assert_eq!(f3["choices"][0]["finish_reason"], "stop");
        assert_eq!(f3["choices"][0]["delta"].as_object().unwrap().len(), 0);
        // 帧 4：[DONE]
        assert_eq!(frames[3].trim(), "[DONE]");
    }
}
