//! OpenAI-compatible REST adapter used by OpenAI, DashScope, Ollama, LM Studio,
//! vLLM and custom compatible endpoints.

use crate::ai_client::{ChatResult, ModelSpec};
use crate::error::{AppError, Result};
use crate::pricing::UsageDetail;
use crate::providers::sse_lines::LineDecoder;
use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use std::pin::Pin;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub enum StreamEvent {
    CacheHit,
    Content(String),
    Reasoning(String),
    Usage(UsageDetail),
    Done,
}

pub type ProviderStream = Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>;

fn client(stream: bool) -> Result<&'static reqwest::Client> {
    static REGULAR: OnceLock<std::result::Result<reqwest::Client, String>> = OnceLock::new();
    static STREAMING: OnceLock<std::result::Result<reqwest::Client, String>> = OnceLock::new();
    let slot = if stream { &STREAMING } else { &REGULAR };
    slot.get_or_init(|| {
        let builder =
            reqwest::Client::builder().connect_timeout(std::time::Duration::from_secs(10));
        let builder = if stream {
            builder.read_timeout(std::time::Duration::from_secs(120))
        } else {
            builder.timeout(std::time::Duration::from_secs(120))
        };
        builder.build().map_err(|e| e.to_string())
    })
    .as_ref()
    .map_err(|e| AppError::AiService(format!("native client init failed: {e}")))
}

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
    let mut request = client(false)?
        .post(endpoint(&spec.api_base))
        .header(reqwest::header::CONTENT_TYPE, "application/json");
    if !spec.api_key.is_empty() {
        request = request.bearer_auth(&spec.api_key);
    }
    let response = super::retry::send(request.json(&json!({
        "model": provider_model(spec),
        "messages": messages,
        "max_tokens": max_tokens,
        "temperature": temperature,
        "stream": false,
    })))
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
    let mut request = client(true)?
        .post(endpoint(&spec.api_base))
        .header(reqwest::header::CONTENT_TYPE, "application/json");
    if !spec.api_key.is_empty() {
        request = request.bearer_auth(&spec.api_key);
    }
    let response = super::retry::send(request.json(&json!({
        "model": provider_model(spec), "messages": messages,
        "max_tokens": max_tokens, "temperature": temperature,
        "stream": true, "stream_options": {"include_usage": true},
    })))
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
        let mut decoder = LineDecoder::default();
        let mut usage_snapshot = UsageDetail {
            field_missing: true,
            ..Default::default()
        };
        while let Some(chunk) = bytes.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    let _ = tx.send(Err(classify_transport_error(&error))).await;
                    return;
                }
            };
            let lines = match decoder.push(&chunk) {
                Ok(lines) => lines,
                Err(error) => {
                    let _ = tx.send(Err(error)).await;
                    return;
                }
            };
            for line in lines {
                if !emit_line(&line, &tx, &mut usage_snapshot).await {
                    return;
                }
            }
        }
        match decoder.finish() {
            Ok(Some(line)) => {
                if !emit_line(&line, &tx, &mut usage_snapshot).await {
                    return;
                }
            }
            Ok(None) => {}
            Err(error) => {
                let _ = tx.send(Err(error)).await;
                return;
            }
        }
        let _ = tx.send(Ok(StreamEvent::Done)).await;
    });
    Ok(Box::pin(futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|event| (event, rx))
    })))
}

async fn emit_line(
    line: &str,
    tx: &tokio::sync::mpsc::Sender<Result<StreamEvent>>,
    usage: &mut UsageDetail,
) -> bool {
    let Some(data) = line.strip_prefix("data:").map(str::trim) else {
        return true;
    };
    if data == "[DONE]" {
        let _ = tx.send(Ok(StreamEvent::Done)).await;
        return false;
    }
    if let Ok(value) = serde_json::from_str::<Value>(data) {
        for event in parse_stream_chunk(&value, usage) {
            if tx.send(Ok(event)).await.is_err() {
                return false;
            }
        }
    }
    true
}

fn parse_stream_chunk(value: &Value, snapshot: &mut UsageDetail) -> Vec<StreamEvent> {
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
        if let Some(v) = usage.get("prompt_tokens").and_then(Value::as_i64) {
            snapshot.prompt_tokens = v;
        }
        if let Some(v) = usage.get("completion_tokens").and_then(Value::as_i64) {
            snapshot.completion_tokens = v;
        }
        if let Some(v) = cached.and_then(Value::as_i64) {
            snapshot.cached_tokens = v;
            snapshot.field_missing = false;
        }
        if let Some(v) = usage
            .pointer("/completion_tokens_details/reasoning_tokens")
            .and_then(Value::as_i64)
        {
            snapshot.reasoning_tokens = v;
        }
        if let Some(v) = usage
            .get("cache_creation_input_tokens")
            .and_then(Value::as_i64)
        {
            snapshot.cache_creation_tokens = v;
        }
        events.push(StreamEvent::Usage(snapshot.clone()));
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
        let mut usage = UsageDetail::default();
        let events = parse_stream_chunk(
            &json!({
                "choices": [{"delta": {"content":"hi", "reasoning_content":"r"}}],
                "usage": {"prompt_tokens":2, "completion_tokens":1}
            }),
            &mut usage,
        );
        assert!(matches!(&events[0], StreamEvent::Content(v) if v == "hi"));
        assert!(matches!(&events[1], StreamEvent::Reasoning(v) if v == "r"));
        assert!(matches!(&events[2], StreamEvent::Usage(v) if v.prompt_tokens == 2));
    }

    #[test]
    fn partial_compatible_usage_frames_preserve_previous_fields() {
        let mut usage = UsageDetail::default();
        parse_stream_chunk(
            &json!({"usage":{"prompt_tokens":100,"prompt_tokens_details":{"cached_tokens":20}}}),
            &mut usage,
        );
        let events = parse_stream_chunk(&json!({"usage":{"completion_tokens":5}}), &mut usage);
        assert!(
            matches!(&events[0], StreamEvent::Usage(v) if v.prompt_tokens == 100 && v.cached_tokens == 20 && v.completion_tokens == 5)
        );
    }

    #[tokio::test]
    async fn stream_line_decodes_split_chinese_and_unnterminated_final_frame() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let mut decoder = LineDecoder::default();
        let mut usage = UsageDetail::default();
        let frames = "data: {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\n\ndata: [DONE]";
        for byte in frames.as_bytes() {
            for line in decoder.push(&[*byte]).unwrap() {
                if !emit_line(&line, &tx, &mut usage).await {
                    break;
                }
            }
        }
        let final_line = decoder.finish().unwrap().unwrap();
        assert!(!emit_line(&final_line, &tx, &mut usage).await);
        assert!(
            matches!(rx.recv().await.unwrap().unwrap(), StreamEvent::Content(text) if text == "你好")
        );
        assert!(matches!(
            rx.recv().await.unwrap().unwrap(),
            StreamEvent::Done
        ));
    }
}
