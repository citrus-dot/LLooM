//! Async HTTP client for the Python AI micro-service.
//! All LLM calls (litellm) are delegated to this stateless service.

use crate::config;
use crate::error::{AppError, Result};
use crate::models::Model;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpec {
    pub name: String,
    pub litellm_model: String,
    #[serde(default)]
    pub api_base: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub input_cost_per_token: f64,
    #[serde(default)]
    pub output_cost_per_token: f64,
}

impl From<&Model> for ModelSpec {
    fn from(m: &Model) -> Self {
        Self {
            name: m.name.clone(),
            litellm_model: m.litellm_model.clone(),
            api_base: config::resolve_env_or_literal(&m.api_base),
            api_key: config::api_key_for(&m.api_key_env),
            input_cost_per_token: m.input_cost_per_token,
            output_cost_per_token: m.output_cost_per_token,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResult {
    pub content: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost: f64,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyResult {
    pub task_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SseEvent {
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub data: Value,
}

fn base_url() -> String {
    std::env::var("LLOOM_AI_SERVICE_URL").unwrap_or_else(|_| {
        format!("http://localhost:{}", config::ai_port())
    })
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// AI service health: reachable + whether any LLM backend is ready.
#[derive(Debug, Clone, Deserialize)]
pub struct AiHealth {
    pub status: String,
    #[serde(default)]
    pub ready: bool,
}

pub async fn health() -> AiHealth {
    let url = format!("{}/v1/health", base_url());
    let Ok(resp) = client()
        .get(&url)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
    else {
        return AiHealth { status: "down".to_string(), ready: false };
    };
    resp.json::<AiHealth>().await.unwrap_or(AiHealth {
        status: "down".to_string(),
        ready: false,
    })
}

/// Non-streaming chat completion.
pub async fn chat(spec: &ModelSpec, messages: &[Value], max_tokens: i64, temperature: f64) -> Result<ChatResult> {
    let url = format!("{}/v1/chat", base_url());
    let body = json!({
        "model": spec,
        "messages": messages,
        "max_tokens": max_tokens,
        "temperature": temperature,
    });
    let resp = client()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::AiService(format!("AI service unreachable: {e}")))?;
    resp.json::<ChatResult>()
        .await
        .map_err(|e| AppError::AiService(format!("AI service bad response: {e}")))
}

/// LLM-based task classification (fallback layer). Never fails hard.
pub async fn classify(text: &str, classifier: &ModelSpec, valid_types: &[&str]) -> String {
    let url = format!("{}/v1/classify", base_url());
    let body = json!({
        "text": text,
        "classifier": classifier,
        "valid_types": valid_types,
    });
    match client().post(&url).json(&body).send().await {
        Ok(resp) => resp
            .json::<ClassifyResult>()
            .await
            .map(|r| r.task_type)
            .unwrap_or_else(|_| "general".to_string()),
        Err(_) => "general".to_string(),
    }
}

/// Full orchestration stream. Returns materialized SSE events.
pub async fn orchestrate_stream(
    query: &str,
    history: &[Value],
    sr_domain: &str,
    models: &[ModelSpec],
    cache_dir: &str,
) -> Result<Vec<SseEvent>> {
    let url = format!("{}/v1/orchestrate/stream", base_url());
    let body = json!({
        "query": query,
        "history": history,
        "sr_domain": sr_domain,
        "models": models,
        "cache_dir": cache_dir,
    });
    let resp = client()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::AiService(format!("AI service unreachable: {e}")))?;
    let text = resp
        .text()
        .await
        .map_err(|e| AppError::AiService(format!("AI service read error: {e}")))?;
    Ok(parse_sse(&text))
}

/// Parse an SSE body into a Vec of {event, data} events.
pub fn parse_sse(text: &str) -> Vec<SseEvent> {
    let mut events: Vec<SseEvent> = Vec::new();
    let mut current: Option<(String, String)> = None;

    for line in text.lines() {
        if let Some(ev) = line.strip_prefix("event:") {
            if let Some((name, data)) = current.take() {
                events.push(mk_event(name, &data));
            }
            current = Some((ev.trim().to_string(), String::new()));
        } else if let Some(data) = line.strip_prefix("data: ") {
            if let Some((name, existing)) = current.as_mut() {
                if existing.is_empty() {
                    *existing = data.to_string();
                } else {
                    events.push(mk_event(name.clone(), existing));
                    *existing = data.to_string();
                }
            } else {
                current = Some(("message".to_string(), data.to_string()));
            }
        }
    }
    if let Some((name, data)) = current.take() {
        events.push(mk_event(name, &data));
    }
    events
}

// ── Semantic-cache management (proxied to the AI service) ──

/// Start the embedding-model pre-initialization on the AI service. Returns the
/// AI service's JSON response.
pub async fn cache_init() -> Result<Value> {
    let url = format!("{}/v1/cache/init", base_url());
    let resp = client()
        .post(&url)
        .send()
        .await
        .map_err(|e| AppError::AiService(format!("AI service unreachable: {e}")))?;
    resp.json::<Value>()
        .await
        .map_err(|e| AppError::AiService(format!("AI service bad response: {e}")))
}

/// Poll the cache-init progress.
pub async fn cache_status() -> Result<Value> {
    let url = format!("{}/v1/cache/status", base_url());
    let resp = client()
        .get(&url)
        .send()
        .await
        .map_err(|e| AppError::AiService(format!("AI service unreachable: {e}")))?;
    resp.json::<Value>()
        .await
        .map_err(|e| AppError::AiService(format!("AI service bad response: {e}")))
}

/// Reset cache init state and remove partial chroma data.
pub async fn cache_cleanup() -> Result<Value> {
    let url = format!("{}/v1/cache/cleanup", base_url());
    let resp = client()
        .post(&url)
        .send()
        .await
        .map_err(|e| AppError::AiService(format!("AI service unreachable: {e}")))?;
    resp.json::<Value>()
        .await
        .map_err(|e| AppError::AiService(format!("AI service bad response: {e}")))
}

fn mk_event(name: String, data: &str) -> SseEvent {
    let parsed = serde_json::from_str::<Value>(data).unwrap_or(Value::String(data.to_string()));
    SseEvent { event: name, data: parsed }
}
