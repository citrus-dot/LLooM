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
//! - 鉴权：`Authorization: Bearer $LLOOM_PROXY_TOKEN`（env 未设则不鉴权；
//!   O2 收尾后默认只绑环回，公网暴露需显式 `LLOOM_BIND` + token 双开）。

use crate::db;
use crate::models::Model;
use crate::router;
use crate::security;
use crate::server::{chat_with_failover, pick_classifier, priced_usage};
use axum::body::Body;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

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

/// Bearer 鉴权：`expected` 为 None（未配置 LLOOM_PROXY_TOKEN）时放行。
pub(crate) fn bearer_ok(headers: &HeaderMap, expected: Option<&str>) -> bool {
    let Some(tok) = expected else { return true };
    let want = format!("Bearer {tok}");
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim() == want)
}

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
    format!(
        "data: {role}\n\ndata: {content_frame}\n\ndata: {finish}\n\ndata: [DONE]\n\n"
    )
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

// ── Handlers ──

fn proxy_token() -> Option<String> {
    std::env::var("LLOOM_PROXY_TOKEN").ok().filter(|s| !s.trim().is_empty())
}

/// `POST /v1/chat/completions`
pub async fn chat_completions(headers: HeaderMap, Json(req): Json<OpenAiChatRequest>) -> Response {
    if !bearer_ok(&headers, proxy_token().as_deref()) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "Invalid API key",
            "invalid_request_error",
            "invalid_api_key",
        );
    }

    // 安全检查与 WebUI chat 路径同规则（security::check 只查当前请求文本）
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

    let models = match db::list_models(true) {
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
    let routing = router::route(&route_param, &user_text, classifier.as_ref(), None).await;

    let Some(primary) = models.iter().find(|m| m.name == routing.model) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            &format!("模型 '{}' 未注册或未启用，请先在模型页添加", routing.model),
            "invalid_request_error",
            "model_not_found",
        );
    };
    let primary_provider = primary.provider.clone();

    // 审计落库（与 chat_stream 同款：决策快照 + 耗时，outcome 调用后回填）
    let routing_task_type = routing.task_type.clone();
    let request_id = format!(
        "proxy-{}",
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
            &serde_json::to_string(&json!({
                "method": routing.method,
                "api": "openai_compat",
                "budget_tier": routing.budget_tier,
            }))
            .unwrap_or_default(),
            &serde_json::to_string(&routing.fallback_chain).unwrap_or_default(),
            &routing.model,
            &routing.fallback_chain.join(","),
            0.0,
        )
        .unwrap_or(0)
    } else {
        0
    };

    // 客户端未指定时的缺省：max_tokens 给足（代理场景常见长输出，WebUI 内部
    // 路径仍为 500）；temperature 沿用 OpenAI 语义缺省 1.0（客户端显式值优先）
    let max_tokens = req.max_tokens.unwrap_or(4096);
    let temperature = req.temperature.unwrap_or(1.0);

    let chat_start = std::time::Instant::now();
    let result = chat_with_failover(
        &models,
        &routing_task_type,
        &routing.model,
        &routing.fallback_chain,
        &processed_messages,
        max_tokens,
        temperature,
    )
    .await;

    match result {
        Ok((res, used_model)) => {
            if decision_id > 0 {
                let _ = db::update_routing_decision_outcome(decision_id, "success");
            }
            let provider = models
                .iter()
                .find(|m| m.name == used_model)
                .map(|m| m.provider.as_str())
                .unwrap_or(primary_provider.as_str());
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
                    api_source: Some("proxy".to_string()),
                }),
            );
            db::upsert_model_task_score_signal(
                &used_model,
                &routing_task_type,
                crate::models::QualitySignalKind::Success,
            )
            .ok();

            let id = new_completion_id();
            let created = now_secs();
            if req.stream.unwrap_or(false) {
                sse_response(sse_frames(&id, created, &used_model, &res.content))
            } else {
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
        }
        Err(e) => {
            if decision_id > 0 {
                let _ = db::update_routing_decision_outcome(decision_id, "failed");
            }
            let msg = e.to_string();
            if req.stream.unwrap_or(false) {
                // OpenAI 流式错误约定：SSE data 帧携带 error 对象后以 [DONE] 收尾
                sse_response(format!(
                    "data: {}\n\ndata: [DONE]\n\n",
                    error_body(&msg, "server_error", "upstream_failure")
                ))
            } else {
                error_response(StatusCode::BAD_GATEWAY, &msg, "server_error", "upstream_failure")
            }
        }
    }
}

/// `GET /v1/models`：激活模型列表（`auto` 恒在首位，方便客户端直接选用）。
pub async fn models_list(headers: HeaderMap) -> Response {
    if !bearer_ok(&headers, proxy_token().as_deref()) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "Invalid API key",
            "invalid_request_error",
            "invalid_api_key",
        );
    }
    let models: Vec<Model> = match db::list_models(true) {
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
            "owned_by": m.provider,
        })
    }));
    Json(json!({ "object": "list", "data": data })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with(auth: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(v) = auth {
            h.insert(header::AUTHORIZATION, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn bearer_no_token_configured_always_ok() {
        assert!(bearer_ok(&headers_with(None), None));
        assert!(bearer_ok(&headers_with(Some("Bearer wrong")), None));
    }

    #[test]
    fn bearer_with_token_requires_exact_match() {
        let h = headers_with(Some("Bearer secret123"));
        assert!(bearer_ok(&h, Some("secret123")));
        assert!(!bearer_ok(&h, Some("other")));
        assert!(!bearer_ok(&headers_with(None), Some("secret123")));
        assert!(!bearer_ok(&headers_with(Some("secret123")), Some("secret123"))); // 缺 Bearer 前缀
    }

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
