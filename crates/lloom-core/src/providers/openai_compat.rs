//! OpenAI-compatible REST adapter used by OpenAI, DashScope, Ollama, LM Studio,
//! vLLM and custom compatible endpoints.

use crate::ai_client::{ChatResult, ModelSpec};
use crate::error::{AppError, Result};
use crate::pricing::UsageDetail;
use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use std::pin::Pin;

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Content(String),
    Reasoning(String),
    Usage(UsageDetail),
    Done,
}

pub type ProviderStream = Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>;

fn endpoint(api_base: &str) -> String {
    let base = api_base.trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        base.to_string()
    } else if base.ends_with("/v1") {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
    }
}

fn provider_model(spec: &ModelSpec) -> &str {
    &spec.provider_model
}

pub async fn chat(
    spec: &ModelSpec,
    messages: &[Value],
    max_tokens: i64,
    temperature: f64,
) -> Result<ChatResult> {
    let mut request = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| AppError::AiService(format!("native client init failed: {e}")))?
        .post(endpoint(&spec.api_base))
        .header(reqwest::header::CONTENT_TYPE, "application/json");
    if !spec.api_key.is_empty() {
        request = request.bearer_auth(&spec.api_key);
    }
    let response = request
        .json(&json!({
            "model": provider_model(spec),
            "messages": messages,
            "max_tokens": max_tokens,
            "temperature": temperature,
            "stream": false,
        }))
        .send()
        .await
        .map_err(|e| classify_transport_error(&e))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| AppError::AiService(format!("native response read failed: {e}")))?;
    if !status.is_success() {
        return Err(classify_status(status.as_u16(), &body));
    }
    let value: Value = serde_json::from_str(&body)
        .map_err(|e| AppError::AiService(format!("native provider bad JSON: {e}")))?;
    parse_response(spec, &value)
}

pub async fn stream(
    spec: &ModelSpec,
    messages: &[Value],
    max_tokens: i64,
    temperature: f64,
) -> Result<ProviderStream> {
    let mut request = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .read_timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| AppError::AiService(format!("native client init failed: {e}")))?
        .post(endpoint(&spec.api_base))
        .header(reqwest::header::CONTENT_TYPE, "application/json");
    if !spec.api_key.is_empty() {
        request = request.bearer_auth(&spec.api_key);
    }
    let response = request
        .json(&json!({
            "model": provider_model(spec), "messages": messages,
            "max_tokens": max_tokens, "temperature": temperature,
            "stream": true, "stream_options": {"include_usage": true},
        }))
        .send()
        .await
        .map_err(|e| classify_transport_error(&e))?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(classify_status(status, &body));
    }
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move {
        let mut bytes = response.bytes_stream();
        let mut buffer = String::new();
        while let Some(chunk) = bytes.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    let _ = tx.send(Err(classify_transport_error(&error))).await;
                    return;
                }
            };
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim_end_matches('\r').to_string();
                buffer.drain(..=pos);
                let Some(data) = line.strip_prefix("data:").map(str::trim) else {
                    continue;
                };
                if data == "[DONE]" {
                    let _ = tx.send(Ok(StreamEvent::Done)).await;
                    return;
                }
                if let Ok(value) = serde_json::from_str::<Value>(data) {
                    for event in parse_stream_chunk(&value) {
                        if tx.send(Ok(event)).await.is_err() {
                            return;
                        }
                    }
                }
            }
        }
        let _ = tx.send(Ok(StreamEvent::Done)).await;
    });
    Ok(Box::pin(futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|event| (event, rx))
    })))
}

fn parse_stream_chunk(value: &Value) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    if let Some(delta) = value.pointer("/choices/0/delta") {
        if let Some(text) = delta
            .get("content")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            events.push(StreamEvent::Content(text.to_string()));
        }
        if let Some(text) = delta
            .get("reasoning_content")
            .or_else(|| delta.get("reasoning"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            events.push(StreamEvent::Reasoning(text.to_string()));
        }
    }
    if let Some(usage) = value.get("usage").filter(|u| !u.is_null()) {
        let cached = usage.pointer("/prompt_tokens_details/cached_tokens");
        events.push(StreamEvent::Usage(UsageDetail {
            prompt_tokens: number(usage.get("prompt_tokens")),
            completion_tokens: number(usage.get("completion_tokens")),
            cached_tokens: number(cached),
            reasoning_tokens: number(usage.pointer("/completion_tokens_details/reasoning_tokens")),
            cache_creation_tokens: number(usage.get("cache_creation_input_tokens")),
            field_missing: cached.is_none(),
        }));
    }
    events
}

fn parse_response(spec: &ModelSpec, value: &Value) -> Result<ChatResult> {
    let message = value.pointer("/choices/0/message").ok_or_else(|| {
        AppError::AiService("native provider response missing choices[0].message".into())
    })?;
    let content = content_text(message.get("content"));
    let reasoning = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let usage = value.get("usage");
    let cached = usage.and_then(|u| u.pointer("/prompt_tokens_details/cached_tokens"));
    Ok(ChatResult {
        content,
        usage: UsageDetail {
            prompt_tokens: number(usage.and_then(|u| u.get("prompt_tokens"))),
            completion_tokens: number(usage.and_then(|u| u.get("completion_tokens"))),
            cached_tokens: number(cached),
            reasoning_tokens: number(
                usage.and_then(|u| u.pointer("/completion_tokens_details/reasoning_tokens")),
            ),
            cache_creation_tokens: number(usage.and_then(|u| u.get("cache_creation_input_tokens"))),
            field_missing: cached.is_none(),
        },
        model: spec.name.clone(),
        reasoning,
        cache_hit: false,
    })
}

fn content_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| part.as_str())
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn number(value: Option<&Value>) -> i64 {
    value.and_then(Value::as_i64).unwrap_or(0)
}

fn classify_transport_error(error: &reqwest::Error) -> AppError {
    let kind = if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "transport"
    } else {
        "request"
    };
    AppError::AiService(format!("native provider {kind}: {error}"))
}

fn classify_status(status: u16, body: &str) -> AppError {
    let kind = match status {
        401 | 403 => "authentication",
        408 => "timeout",
        429 => "rate_limited",
        400..=499 => "invalid_request",
        500..=599 => "server_error",
        _ => "http_error",
    };
    let detail: String = body.chars().take(500).collect();
    AppError::AiService(format!("native provider {kind} ({status}): {detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ModelSpec {
        ModelSpec {
            name: "logical-name".into(),
            provider: "openai".into(),
            provider_model: "gpt-test".into(),
            api_base: "http://localhost:1234/v1".into(),
            api_key: String::new(),
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
        }
    }

    #[test]
    fn normalizes_endpoint_and_model() {
        let s = spec();
        assert_eq!(
            endpoint(&s.api_base),
            "http://localhost:1234/v1/chat/completions"
        );
        assert_eq!(provider_model(&s), "gpt-test");
        assert_eq!(
            endpoint("http://x/chat/completions"),
            "http://x/chat/completions"
        );
    }

    #[test]
    fn parses_usage_reasoning_and_array_content() {
        let value = json!({
            "choices": [{"message": {"content": [{"type":"text", "text":"hello"}], "reasoning_content":"think"}}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 4,
                "prompt_tokens_details": {"cached_tokens": 3},
                "completion_tokens_details": {"reasoning_tokens": 2}}
        });
        let out = parse_response(&spec(), &value).unwrap();
        assert_eq!(out.content, "hello");
        assert_eq!(out.reasoning.as_deref(), Some("think"));
        assert_eq!(out.usage.cached_tokens, 3);
        assert!(!out.usage.field_missing);
    }

    #[test]
    fn parses_stream_deltas_and_usage() {
        let events = parse_stream_chunk(&json!({
            "choices": [{"delta": {"content":"hi", "reasoning_content":"r"}}],
            "usage": {"prompt_tokens":2, "completion_tokens":1}
        }));
        assert!(matches!(&events[0], StreamEvent::Content(v) if v == "hi"));
        assert!(matches!(&events[1], StreamEvent::Reasoning(v) if v == "r"));
        assert!(matches!(&events[2], StreamEvent::Usage(v) if v.prompt_tokens == 2));
    }
}
