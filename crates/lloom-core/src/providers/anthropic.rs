//! Anthropic Messages API adapter.

use crate::ai_client::{ChatResult, ModelSpec};
use crate::error::{AppError, Result};
use crate::pricing::UsageDetail;
use crate::providers::openai_compat::{ProviderStream, StreamEvent};
use futures::StreamExt;
use serde_json::{json, Value};

fn endpoint(api_base: &str) -> String {
    let base = if api_base.trim().is_empty() {
        "https://api.anthropic.com"
    } else {
        api_base.trim_end_matches('/')
    };
    if base.ends_with("/v1/messages") {
        base.to_string()
    } else if base.ends_with("/v1") {
        format!("{base}/messages")
    } else {
        format!("{base}/v1/messages")
    }
}

fn model(spec: &ModelSpec) -> &str {
    &spec.provider_model
}

fn payload(
    spec: &ModelSpec,
    messages: &[Value],
    max_tokens: i64,
    temperature: f64,
    stream: bool,
) -> Value {
    let mut systems = Vec::new();
    let mut converted = Vec::new();
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        let content = message
            .get("content")
            .cloned()
            .unwrap_or(Value::String(String::new()));
        if role == "system" {
            if let Some(text) = content.as_str() {
                systems.push(text.to_string());
            }
        } else {
            let role = if role == "assistant" {
                "assistant"
            } else {
                "user"
            };
            converted.push(json!({"role": role, "content": content}));
        }
    }
    let mut body = json!({
        "model": model(spec), "messages": converted,
        "max_tokens": max_tokens, "temperature": temperature, "stream": stream
    });
    if !systems.is_empty() {
        body["system"] = json!(systems.join("\n\n"));
    }
    body
}

fn request(spec: &ModelSpec) -> Result<reqwest::RequestBuilder> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .read_timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| AppError::AiService(format!("Anthropic client init failed: {e}")))?;
    Ok(client
        .post(endpoint(&spec.api_base))
        .header("x-api-key", &spec.api_key)
        .header("anthropic-version", "2023-06-01")
        .header(reqwest::header::CONTENT_TYPE, "application/json"))
}

pub async fn chat(
    spec: &ModelSpec,
    messages: &[Value],
    max_tokens: i64,
    temperature: f64,
) -> Result<ChatResult> {
    let response = request(spec)?
        .json(&payload(spec, messages, max_tokens, temperature, false))
        .send()
        .await
        .map_err(|e| transport(&e))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| AppError::AiService(format!("Anthropic response read failed: {e}")))?;
    if !status.is_success() {
        return Err(http_error(status.as_u16(), &text));
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| AppError::AiService(format!("Anthropic bad JSON: {e}")))?;
    let mut content = String::new();
    let mut reasoning = String::new();
    for block in value
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                content.push_str(block.get("text").and_then(Value::as_str).unwrap_or(""))
            }
            Some("thinking") => {
                reasoning.push_str(block.get("thinking").and_then(Value::as_str).unwrap_or(""))
            }
            _ => {}
        }
    }
    Ok(ChatResult {
        content,
        usage: parse_usage(value.get("usage")),
        model: spec.name.clone(),
        reasoning: (!reasoning.is_empty()).then_some(reasoning),
        cache_hit: false,
    })
}

pub async fn stream(
    spec: &ModelSpec,
    messages: &[Value],
    max_tokens: i64,
    temperature: f64,
) -> Result<ProviderStream> {
    let response = request(spec)?
        .json(&payload(spec, messages, max_tokens, temperature, true))
        .send()
        .await
        .map_err(|e| transport(&e))?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        return Err(http_error(
            status,
            &response.text().await.unwrap_or_default(),
        ));
    }
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move {
        let mut source = response.bytes_stream();
        let mut buffer = String::new();
        while let Some(chunk) = source.next().await {
            let chunk = match chunk {
                Ok(v) => v,
                Err(e) => {
                    let _ = tx.send(Err(transport(&e))).await;
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
                let Ok(value) = serde_json::from_str::<Value>(data) else {
                    continue;
                };
                for event in parse_event(&value) {
                    if tx.send(Ok(event)).await.is_err() {
                        return;
                    }
                }
                if value.get("type").and_then(Value::as_str) == Some("message_stop") {
                    return;
                }
            }
        }
        let _ = tx.send(Ok(StreamEvent::Done)).await;
    });
    Ok(Box::pin(futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|v| (v, rx))
    })))
}

fn parse_event(value: &Value) -> Vec<StreamEvent> {
    match value.get("type").and_then(Value::as_str) {
        Some("content_block_delta") => {
            let delta = value.get("delta").unwrap_or(&Value::Null);
            match delta.get("type").and_then(Value::as_str) {
                Some("text_delta") => delta
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|s| vec![StreamEvent::Content(s.into())])
                    .unwrap_or_default(),
                Some("thinking_delta") => delta
                    .get("thinking")
                    .and_then(Value::as_str)
                    .map(|s| vec![StreamEvent::Reasoning(s.into())])
                    .unwrap_or_default(),
                _ => Vec::new(),
            }
        }
        Some("message_start") => value
            .pointer("/message/usage")
            .map(|u| vec![StreamEvent::Usage(parse_usage(Some(u)))])
            .unwrap_or_default(),
        Some("message_delta") => value
            .get("usage")
            .map(|u| vec![StreamEvent::Usage(parse_usage(Some(u)))])
            .unwrap_or_default(),
        Some("message_stop") => vec![StreamEvent::Done],
        _ => Vec::new(),
    }
}

fn parse_usage(usage: Option<&Value>) -> UsageDetail {
    let input = usage
        .and_then(|u| u.get("input_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let cache_read = usage
        .and_then(|u| u.get("cache_read_input_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    UsageDetail {
        prompt_tokens: input,
        completion_tokens: usage
            .and_then(|u| u.get("output_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
        cached_tokens: cache_read,
        reasoning_tokens: 0,
        cache_creation_tokens: usage
            .and_then(|u| u.get("cache_creation_input_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
        field_missing: usage
            .and_then(|u| u.get("cache_read_input_tokens"))
            .is_none(),
    }
}

fn transport(error: &reqwest::Error) -> AppError {
    AppError::AiService(format!(
        "Anthropic {}: {error}",
        if error.is_timeout() {
            "timeout"
        } else {
            "transport error"
        }
    ))
}

fn http_error(status: u16, body: &str) -> AppError {
    let kind = match status {
        401 | 403 => "authentication",
        429 => "rate limited",
        400..=499 => "invalid request",
        _ => "server error",
    };
    AppError::AiService(format!(
        "Anthropic {kind} ({status}): {}",
        body.chars().take(500).collect::<String>()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ModelSpec {
        ModelSpec {
            name: "claude".into(),
            provider: "anthropic".into(),
            provider_model: "claude-test".into(),
            api_base: String::new(),
            api_key: "key".into(),
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
        }
    }

    #[test]
    fn converts_system_and_model() {
        let body = payload(
            &spec(),
            &[
                json!({"role":"system","content":"rules"}),
                json!({"role":"user","content":"hi"}),
            ],
            100,
            0.2,
            false,
        );
        assert_eq!(body["system"], "rules");
        assert_eq!(body["model"], "claude-test");
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn parses_anthropic_stream_event() {
        let events = parse_event(
            &json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}),
        );
        assert!(matches!(&events[0], StreamEvent::Content(v) if v == "hi"));
    }
}
