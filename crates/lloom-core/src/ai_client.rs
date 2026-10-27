//! Unified LLM client：原生 Rust provider 适配器为唯一调用路径
//! （OpenAI-compatible / Anthropic Messages 直连，Python 传输层已移除）。

use crate::config;
use crate::error::{AppError, Result};
use crate::models::Model;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};

fn exact_cache(path: &std::path::Path) -> Option<Arc<crate::exact_cache::ExactCache>> {
    static CACHES: OnceLock<
        Mutex<std::collections::HashMap<std::path::PathBuf, Arc<crate::exact_cache::ExactCache>>>,
    > = OnceLock::new();
    let mut caches = CACHES
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(store) = caches.get(path) {
        return Some(store.clone());
    }
    match crate::exact_cache::ExactCache::open(path, 86_400) {
        Ok(store) => {
            let store = Arc::new(store);
            caches.insert(path.to_path_buf(), store.clone());
            Some(store)
        }
        Err(error) => {
            crate::logging::warn("cache", &format!("打开精确缓存失败: {error}"));
            None
        }
    }
}

fn semantic_cache(
    path: &std::path::Path,
    threshold: f32,
) -> Option<Arc<crate::semantic_cache::SemanticCache>> {
    type CacheMap = std::collections::HashMap<
        (std::path::PathBuf, u32),
        Arc<crate::semantic_cache::SemanticCache>,
    >;
    static CACHES: OnceLock<Mutex<CacheMap>> = OnceLock::new();
    let key = (path.to_path_buf(), threshold.to_bits());
    let mut caches = CACHES
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(store) = caches.get(&key) {
        return Some(store.clone());
    }
    match crate::semantic_cache::SemanticCache::open(path, 86_400, threshold) {
        Ok(store) => {
            let store = Arc::new(store);
            caches.insert(key, store.clone());
            Some(store)
        }
        Err(error) => {
            crate::logging::warn("cache", &format!("打开语义缓存失败: {error}"));
            None
        }
    }
}

fn cache_max_entries() -> usize {
    static MAX: OnceLock<usize> = OnceLock::new();
    *MAX.get_or_init(|| {
        std::env::var("LLOOM_CACHE_MAX_ENTRIES")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|v| *v > 0)
            .unwrap_or(5000)
    })
}

fn semantic_threshold() -> f32 {
    static THRESHOLD: OnceLock<f32> = OnceLock::new();
    *THRESHOLD.get_or_init(|| {
        std::env::var("LLOOM_SEMANTIC_THRESHOLD")
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|value| value.is_finite() && (0.5..=1.0).contains(value))
            .unwrap_or(0.88)
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpec {
    pub name: String,
    #[serde(default)]
    pub provider: String,
    pub provider_model: String,
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
        // api_base/api_key 的 env 解析只发生在出进程边界（这里）；domain 层存原始值。
        let api_key = match &m.backend {
            crate::models::Backend::Cloud {
                api_key: Some(k), ..
            } => k.resolve(),
            _ => String::new(),
        };
        // 云端留空 api_base → 供应商默认（WebUI「留空走供应商默认」承诺的落点）；
        // custom 无默认，保持空值由 supports_native 拒绝并给出明确错误。
        let mut api_base = config::resolve_env_or_literal(m.api_base());
        if api_base.trim().is_empty() {
            if let Some(default) = crate::providers::Protocol::default_base(m.provider_name()) {
                api_base = default.to_string();
            }
        }
        Self {
            name: m.name.clone(),
            provider: m.provider_name().to_string(),
            provider_model: m.provider_model.clone(),
            api_base,
            api_key,
            input_cost_per_token: m.input_cost_per_token,
            output_cost_per_token: m.output_cost_per_token,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResult {
    pub content: String,
    #[serde(default)]
    pub usage: crate::pricing::UsageDetail,
    #[serde(default)]
    pub model: String,
    /// C1：推理模型的思考链（模型返回 reasoning_content 时非空）
    #[serde(default)]
    pub reasoning: Option<String>,
    /// CONTEXT-PLAN Phase 4：两层缓存命中（L1 精确 / L2 语义）
    #[serde(default)]
    pub cache_hit: bool,
}

pub type ChatStream = Pin<Box<dyn Stream<Item = Result<crate::providers::StreamEvent>> + Send>>;

fn cache_model_id(spec: &ModelSpec) -> String {
    format!("{}|{}|{}", spec.name, spec.provider_model, spec.api_base)
}

pub async fn chat_stream(
    spec: &ModelSpec,
    messages: &[Value],
    max_tokens: i64,
    temperature: f64,
    cache: Option<&ChatCacheCtx<'_>>,
) -> Result<ChatStream> {
    let Some(cache) = cache else {
        return crate::providers::stream(spec, messages, max_tokens, temperature).await;
    };
    let query = messages
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(Value::as_str) == Some("user"))
        .and_then(|m| m.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let Some(key) = cache_key(spec, messages, cache.conversation_id, temperature) else {
        return crate::providers::stream(spec, messages, max_tokens, temperature).await;
    };
    let exact = exact_cache(&cache.cache_dir.join("cache_exact.sqlite3"));
    if let Some(store) = &exact {
        if let Ok(Some(content)) = store.lookup(&key) {
            return Ok(cached_stream(content, messages));
        }
    }

    let history: Vec<Value> = messages
        .iter()
        .filter(|m| {
            matches!(
                m.get("role").and_then(Value::as_str),
                Some("user" | "assistant")
            )
        })
        .cloned()
        .collect();
    let system_hash = crate::context::system_id(messages);
    let semantic = if exact.is_some()
        && !query.is_empty()
        && crate::context::is_context_free(query, &history[..history.len().saturating_sub(1)])
    {
        match semantic_cache(
            &cache.cache_dir.join("cache_semantic.sqlite3"),
            semantic_threshold(),
        ) {
            Some(store) => embed_query(query.to_string())
                .await
                .map(|vector| (store, vector)),
            None => None,
        }
    } else {
        None
    };
    if let Some((store, vector)) = &semantic {
        if let Ok(Some((content, _))) =
            store.lookup(&cache_model_id(spec), &system_hash, temperature, vector)
        {
            return Ok(cached_stream(content, messages));
        }
    }

    let upstream = crate::providers::stream(spec, messages, max_tokens, temperature).await?;
    let name = cache_model_id(spec);
    let query = query.to_string();
    let conversation_id = cache.conversation_id.to_string();
    Ok(Box::pin(futures::stream::unfold(
        (
            upstream,
            exact,
            semantic,
            key,
            name,
            query,
            system_hash,
            temperature,
            conversation_id,
            String::new(),
        ),
        |(
            mut upstream,
            exact,
            semantic,
            key,
            name,
            query,
            system_hash,
            temperature,
            conversation_id,
            mut content,
        )| async move {
            let event = upstream.next().await?;
            match &event {
                Ok(crate::providers::StreamEvent::Content(delta)) => content.push_str(delta),
                Ok(crate::providers::StreamEvent::Done) if !content.is_empty() => {
                    if let Some(store) = &exact {
                        if let Err(e) = store.store(
                            &key,
                            &name,
                            &content,
                            (!conversation_id.is_empty()).then_some(conversation_id.as_str()),
                        ) {
                            crate::logging::error("cache", &format!("保存精确缓存失败: {e}"));
                        }
                    }
                    if let Some((store, vector)) = &semantic {
                        if let Err(e) =
                            store.store(&name, &system_hash, temperature, &query, &content, vector)
                        {
                            crate::logging::error("cache", &format!("保存语义缓存失败: {e}"));
                        }
                    }
                }
                _ => {}
            }
            Some((
                event,
                (
                    upstream,
                    exact,
                    semantic,
                    key,
                    name,
                    query,
                    system_hash,
                    temperature,
                    conversation_id,
                    content,
                ),
            ))
        },
    )))
}

fn cached_stream(content: String, messages: &[Value]) -> ChatStream {
    let prompt_tokens = messages
        .iter()
        .map(|m| crate::context::message_tokens(m) as i64)
        .sum();
    let output_tokens = crate::context::count_tokens(&content) as i64;
    Box::pin(futures::stream::iter(vec![
        Ok(crate::providers::StreamEvent::CacheHit),
        Ok(crate::providers::StreamEvent::Content(content)),
        Ok(crate::providers::StreamEvent::Usage(
            crate::pricing::UsageDetail {
                prompt_tokens,
                completion_tokens: output_tokens,
                field_missing: true,
                ..Default::default()
            },
        )),
        Ok(crate::providers::StreamEvent::Done),
    ]))
}

async fn embed_query(query: String) -> Option<Vec<f32>> {
    match tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::task::spawn_blocking(move || crate::semantic_cache::SemanticCache::embed(&query)),
    )
    .await
    {
        Ok(Ok(Ok(vector))) => Some(vector),
        Ok(Ok(Err(error))) => {
            crate::logging::warn(
                "cache",
                &format!("L2 向量生成失败，继续无缓存调用: {error}"),
            );
            None
        }
        Ok(Err(error)) => {
            crate::logging::error("cache", &format!("L2 后台任务失败: {error}"));
            None
        }
        Err(_) => {
            crate::logging::warn("cache", "L2 向量生成超过 2 秒，继续无缓存调用");
            None
        }
    }
}

#[cfg(test)]
mod spec_default_base_tests {
    use super::*;

    #[test]
    fn cloud_model_without_api_base_gets_provider_default() {
        // 「API Base 留空走供应商默认」承诺的回归：修复前 supports_native 直接拒绝空 base。
        for (provider, expected) in [
            ("openai", "https://api.openai.com/v1"),
            (
                "dashscope",
                "https://dashscope.aliyuncs.com/compatible-mode/v1",
            ),
            ("anthropic", "https://api.anthropic.com"),
        ] {
            let m = crate::models::Model::fixture("t", provider);
            let spec = ModelSpec::from(&m);
            assert_eq!(spec.api_base, expected, "provider {provider}");
            assert!(crate::providers::supports_native(&spec));
        }
        // custom 无默认：保持空并由 supports_native 拒绝（显式配置仍是硬要求）。
        let custom = crate::models::Model::fixture("t", "custom");
        let spec = ModelSpec::from(&custom);
        assert!(spec.api_base.is_empty());
        assert!(!crate::providers::supports_native(&spec));
    }
}

#[cfg(test)]
mod stream_cache_tests {
    use super::*;
    use axum::{http::header, routing::post, Router};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[tokio::test]
    async fn second_stream_is_served_from_exact_cache_without_provider_call() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let app = Router::new().route(
            "/v1/chat/completions",
            post(move || {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    ([(header::CONTENT_TYPE, "text/event-stream")],
                 "data: {\"choices\":[{\"delta\":{\"content\":\"缓存回答\"}}]}\n\ndata: [DONE]\n\n")
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let dir = std::env::temp_dir().join(format!(
            "lloom_stream_cache_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let spec = ModelSpec {
            name: "fixture".into(),
            provider: "openai".into(),
            provider_model: "fixture".into(),
            api_base: format!("http://127.0.0.1:{port}/v1"),
            api_key: String::new(),
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
        };
        let messages = [
            json!({"role":"user","content":"上文内容"}),
            json!({"role":"assistant","content":"好的"}),
            json!({"role":"user","content":"继续之前的问题"}),
        ];
        let context = ChatCacheCtx {
            conversation_id: "test-conv",
            cache_dir: &dir,
        };
        for expected_hit in [false, true] {
            let mut stream = chat_stream(&spec, &messages, 16, 0.3, Some(&context))
                .await
                .unwrap();
            let mut hit = false;
            let mut content = String::new();
            while let Some(item) = stream.next().await {
                match item.unwrap() {
                    crate::providers::StreamEvent::CacheHit => hit = true,
                    crate::providers::StreamEvent::Content(delta) => content.push_str(&delta),
                    crate::providers::StreamEvent::Done => break,
                    _ => {}
                }
            }
            assert_eq!(hit, expected_hit);
            assert_eq!(content, "缓存回答");
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        server.abort();
        let _ = std::fs::remove_file(dir.join("cache_exact.sqlite3"));
        let _ = std::fs::remove_dir(dir);
    }
}

/// chat 主路径的两层缓存装配（opt-in）。probe/shadow 必须传 None——
/// 缓存命中会把成本样本污染成 0，校准数据全部失真。
pub struct ChatCacheCtx<'a> {
    pub conversation_id: &'a str,
    pub cache_dir: &'a std::path::Path,
}

fn cache_key(
    spec: &ModelSpec,
    messages: &[Value],
    conversation_id: &str,
    temperature: f64,
) -> Option<String> {
    let query = messages
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(Value::as_str) == Some("user"))?
        .get("content")?
        .as_str()?;
    if !crate::context::cacheable(query, temperature) {
        return None;
    }
    let history: Vec<Value> = messages
        .iter()
        .filter(|m| {
            matches!(
                m.get("role").and_then(Value::as_str),
                Some("user") | Some("assistant")
            )
        })
        .cloned()
        .collect();
    let prior = history.len().saturating_sub(1);
    let context_free = crate::context::is_context_free(query, &history[..prior]);
    let fingerprint = if context_free {
        String::new()
    } else {
        crate::context::fingerprint(conversation_id, &history)
    };
    Some(format!(
        "{}:{:016x}",
        crate::context::exact_key(
            &cache_model_id(spec),
            &crate::context::system_id(messages),
            &fingerprint,
            query,
        ),
        temperature.to_bits()
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SseEvent {
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub data: Value,
}

fn event(name: &str, data: Value) -> SseEvent {
    SseEvent {
        event: name.to_string(),
        data,
    }
}

/// Non-streaming chat completion.
pub async fn chat(
    spec: &ModelSpec,
    messages: &[Value],
    max_tokens: i64,
    temperature: f64,
    cache: Option<&ChatCacheCtx<'_>>,
) -> Result<ChatResult> {
    if crate::providers::supports_native(spec) {
        let exact = match cache {
            Some(c) => {
                exact_cache(&c.cache_dir.join("cache_exact.sqlite3")).map(|store| (store, c))
            }
            None => None,
        };
        let key = exact
            .as_ref()
            .and_then(|(_, c)| cache_key(spec, messages, c.conversation_id, temperature));
        if let (Some((store, _)), Some(key)) = (&exact, &key) {
            if let Ok(Some(content)) = store.lookup(key) {
                let prompt_tokens = messages
                    .iter()
                    .map(|m| crate::context::message_tokens(m) as i64)
                    .sum();
                return Ok(ChatResult {
                    content,
                    usage: crate::pricing::UsageDetail {
                        prompt_tokens,
                        completion_tokens: 0,
                        field_missing: true,
                        ..Default::default()
                    },
                    model: spec.name.clone(),
                    reasoning: None,
                    cache_hit: true,
                });
            }
        }
        let query = messages
            .iter()
            .rev()
            .find_map(|m| {
                (m.get("role").and_then(Value::as_str) == Some("user"))
                    .then(|| m.get("content").and_then(Value::as_str))
                    .flatten()
            })
            .unwrap_or("");
        let history: Vec<Value> = messages
            .iter()
            .filter(|m| {
                matches!(
                    m.get("role").and_then(Value::as_str),
                    Some("user") | Some("assistant")
                )
            })
            .cloned()
            .collect();
        let context_free =
            crate::context::is_context_free(query, &history[..history.len().saturating_sub(1)]);
        let semantic = if exact.is_some()
            && context_free
            && !query.is_empty()
            && crate::context::cacheable(query, temperature)
        {
            match semantic_cache(
                &cache
                    .expect("exact cache requires context")
                    .cache_dir
                    .join("cache_semantic.sqlite3"),
                semantic_threshold(),
            ) {
                Some(cache) => embed_query(query.to_string())
                    .await
                    .map(|vector| (cache, vector)),
                None => None,
            }
        } else {
            None
        };
        if let Some((cache, vector)) = &semantic {
            if let Ok(Some((content, _similarity))) = cache.lookup(
                &cache_model_id(spec),
                &crate::context::system_id(messages),
                temperature,
                vector,
            ) {
                return Ok(ChatResult {
                    content,
                    usage: crate::pricing::UsageDetail {
                        field_missing: true,
                        ..Default::default()
                    },
                    model: spec.name.clone(),
                    reasoning: None,
                    cache_hit: true,
                });
            }
        }
        let result = crate::providers::chat(spec, messages, max_tokens, temperature).await?;
        if let (Some((store, c)), Some(key)) = (&exact, &key) {
            let query = messages
                .iter()
                .rev()
                .find_map(|m| {
                    (m.get("role").and_then(Value::as_str) == Some("user"))
                        .then(|| m.get("content").and_then(Value::as_str))
                        .flatten()
                })
                .unwrap_or("");
            if crate::context::cacheable(query, temperature) {
                let _ = store.store(
                    key,
                    &cache_model_id(spec),
                    &result.content,
                    (!c.conversation_id.is_empty()).then_some(c.conversation_id),
                );
                let max = cache_max_entries();
                let _ = store.sweep(max);
            }
        }
        if crate::context::cacheable(query, temperature) {
            if let Some((cache, vector)) = &semantic {
                let _ = cache.store(
                    &cache_model_id(spec),
                    &crate::context::system_id(messages),
                    temperature,
                    query,
                    &result.content,
                    vector,
                );
                let max = cache_max_entries();
                let _ = cache.sweep(max);
            }
        }
        return Ok(result);
    }
    Err(AppError::AiService(format!(
        "不支持的 Provider '{}'；当前原生支持 openai/dashscope/ollama/custom/anthropic",
        spec.provider
    )))
}

/// LLM-based task classification. The caller chooses an observable fallback.
pub async fn classify(text: &str, classifier: &ModelSpec, valid_types: &[&str]) -> Result<String> {
    const PROMPT: &str = "将用户请求分类，只返回一个类别名称，不要解释。";
    let valid = valid_types.join(" / ");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        chat(
            classifier,
            &[
                json!({"role":"system","content":format!("{PROMPT} 可选类别：{valid}")}),
                json!({"role":"user","content":text.chars().take(500).collect::<String>()}),
            ],
            20,
            0.0,
            None,
        ),
    )
    .await
    .map_err(|_| AppError::AiService("分类模型调用超时（8 秒）".into()))??;
    let lower = result.content.to_lowercase();
    valid_types
        .iter()
        .find(|t| lower.contains(**t))
        .map(|task| (*task).to_string())
        .ok_or_else(|| AppError::AiService("分类模型输出不在允许的类别中".into()))
}

/// Generate/extend a rolling conversation summary. The policy and persistence
/// live in Rust; the assigned model is called through the native provider adapters.
pub async fn summarize(spec: &ModelSpec, previous: &str, uncovered: &[Value]) -> Result<String> {
    let transcript = uncovered
        .iter()
        .map(|message| {
            let role = if message.get("role").and_then(Value::as_str) == Some("user") {
                "用户"
            } else {
                "助手"
            };
            let content: String = message
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .chars()
                .take(400)
                .collect();
            format!("{role}: {content}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let prefix = if previous.is_empty() {
        String::new()
    } else {
        format!("已有摘要：\n{previous}\n\n")
    };
    let prompt = format!(
        "{prefix}请把以下对话片段压缩成一段简洁的事实摘要（≤300字），保留关键事实、用户意图与已做决定，忽略寒暄：\n{transcript}"
    );
    let result = chat(
        spec,
        &[json!({"role": "user", "content": prompt})],
        400,
        0.0,
        None,
    )
    .await?;
    Ok(result.content.trim().to_string())
}

/// Ask the assigned model for a structured decomposition. Parsing and fallback
/// are Rust responsibilities; the model call goes through the native provider adapters.
pub async fn decompose(spec: &ModelSpec, query: &str) -> Result<Vec<crate::orchestrator::SubTask>> {
    const PROMPT: &str = "你是一个任务分解专家。将用户的复杂任务分解为2-5个子任务。\n规则：\n1. 每个子任务应该是独立的、可执行的\n2. 标注子任务之间的依赖关系（depends_on）\n3. 类型只能是 simple_qa / general / coding / math_logic / complex_reasoning\n4. 估算每个子任务的输出 token 数\n只输出JSON数组，字段为 id, description, task_type, depends_on, estimated_output_tokens。";
    let result = chat(
        spec,
        &[
            json!({"role": "system", "content": PROMPT}),
            json!({"role": "user", "content": query}),
        ],
        800,
        0.0,
        None,
    )
    .await?;
    Ok(crate::orchestrator::parse_decomposition(&result.content))
}

/// Rust-owned orchestration executor. It preserves the existing SSE contract;
/// provider calls use the native adapters through `chat`.
#[allow(clippy::too_many_arguments)]
pub async fn orchestrate_native(
    db: &crate::db::Db,
    query: &str,
    _history: &[Value],
    models: &[ModelSpec],
    _conversation_id: &str,
    assignments: &Value,
    prepared_messages: &[Value],
    context_stats: &Value,
    is_complex: bool,
    planned_tasks: &[crate::orchestrator::SubTask],
    waves: &[Vec<usize>],
) -> Result<Pin<Box<dyn Stream<Item = SseEvent> + Send>>> {
    use std::collections::{HashMap, HashSet};
    use std::time::Instant;

    let assigned = |role: &str| -> Option<&ModelSpec> {
        assignments
            .get(role)
            .and_then(Value::as_str)
            .and_then(|name| models.iter().find(|m| m.name == name))
            .or_else(|| models.first())
    };
    let (tx, rx) = tokio::sync::mpsc::channel::<SseEvent>(64);
    if tx
        .send(event("context", context_stats.clone()))
        .await
        .is_err()
    {
        return Err(AppError::Internal("orchestration receiver closed".into()));
    }
    if !is_complex {
        let spec = assigned("general").ok_or_else(|| AppError::AiService("无可用模型".into()))?;
        if tx.send(event("decompose", json!({
            "sub_tasks": [{"id":1,"description":query,"task_type":"general","selected_model":spec.name,"cost":0.0}],
            "total_cost": 0.0
        }))).await.is_err() { return Err(AppError::Internal("orchestration receiver closed".into())); }
        if tx
            .send(event(
                "task_start",
                json!({"id":1,"description":query,"model":spec.name}),
            ))
            .await
            .is_err()
        {
            return Err(AppError::Internal("orchestration receiver closed".into()));
        }
        let mut provider_stream = chat_stream(spec, prepared_messages, 2000, 0.3, None).await?;
        let spec_name = spec.name.clone();
        tokio::spawn(async move {
            use futures::StreamExt;
            let started = Instant::now();
            let mut content = String::new();
            let mut reasoning = String::new();
            let mut usage = crate::pricing::UsageDetail::default();
            while let Some(item) = provider_stream.next().await {
                match item {
                    Ok(crate::providers::StreamEvent::CacheHit) => {}
                    Ok(crate::providers::StreamEvent::Content(delta)) => {
                        content.push_str(&delta);
                        if tx
                            .send(event(
                                "token",
                                json!({"id":1,"model":spec_name,"delta":delta}),
                            ))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    Ok(crate::providers::StreamEvent::Reasoning(delta)) => {
                        reasoning.push_str(&delta)
                    }
                    Ok(crate::providers::StreamEvent::Usage(value)) => usage = value,
                    Ok(crate::providers::StreamEvent::Done) => break,
                    Err(error) => {
                        tx.send(event("error", json!({"message":error.to_string()})))
                            .await
                            .ok();
                        return;
                    }
                }
            }
            let duration = started.elapsed().as_secs_f64();
            if tx.send(event("task_done",json!({"id":1,"model":spec_name,"task_type":"general","duration":duration,"cost":0.0,"input_tokens":usage.prompt_tokens,"output_tokens":usage.completion_tokens,"saved_cost":0.0,"cache_hit":false}))).await.is_err(){return;}
            if tx.send(event("result",json!({"response":content,"model":spec_name,"cost":0.0,"input_tokens":usage.prompt_tokens,"output_tokens":usage.completion_tokens,"saved_cost":0.0,"total_duration":duration,"models_used":[spec_name],"cache_hit":false,"reasoning":if reasoning.is_empty(){None}else{Some(reasoning)}}))).await.is_err() {}
        });
        return Ok(Box::pin(futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|e| (e, rx))
        })));
    }

    let general =
        assigned("general").ok_or_else(|| AppError::AiService("无可用执行模型".into()))?;
    let aggregate =
        assigned("aggregate").ok_or_else(|| AppError::AiService("无可用汇总模型".into()))?;
    if tx
        .send(event(
            "decompose",
            json!({
                "sub_tasks": planned_tasks.iter().map(|t| json!({
                    "id":t.id,"description":t.description,"task_type":t.task_type,
                    "depends_on":t.depends_on,"estimated_output_tokens":t.estimated_output_tokens,
                    "selected_model":general.name,"cost":0.0
                })).collect::<Vec<_>>(), "total_cost":0.0
            }),
        ))
        .await
        .is_err()
    {
        return Err(AppError::Internal("orchestration receiver closed".into()));
    }

    let tasks: HashMap<usize, &crate::orchestrator::SubTask> =
        planned_tasks.iter().map(|t| (t.id, t)).collect();
    let routable_models: Vec<crate::models::Model> = db
        .list_models(true)
        .unwrap_or_default()
        .into_iter()
        .filter(|m| {
            !matches!(m.backend, crate::models::Backend::Cloud { .. })
                || !crate::config::api_key_for(m.api_key_env()).is_empty()
        })
        .collect();
    let mut completed: HashMap<usize, String> = HashMap::new();
    let mut used = HashSet::new();
    let mut total_in = 0i64;
    let mut total_out = 0i64;
    let mut total_duration = 0.0;
    for wave in waves {
        for id in wave {
            if let Some(task) = tasks.get(id) {
                if tx
                    .send(event(
                        "task_start",
                        json!({"id":task.id,"description":task.description,"model":general.name}),
                    ))
                    .await
                    .is_err()
                {
                    return Err(AppError::Internal("orchestration receiver closed".into()));
                }
            }
        }
        // Calls are kept deterministic here; provider concurrency is added once
        // native rate-limit coordination moves into this module.
        for id in wave {
            let Some(task) = tasks.get(id) else { continue };
            let dependency_context = task
                .depends_on
                .iter()
                .filter_map(|dep| {
                    completed
                        .get(dep)
                        .map(|value| format!("[子任务{dep}] {value}"))
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            let user = if dependency_context.is_empty() {
                task.description.clone()
            } else {
                format!(
                    "前置任务结果：\n{dependency_context}\n\n当前任务：{}",
                    task.description
                )
            };
            let route = crate::router::plan_decision(db, &task.task_type, &routable_models).ok();
            let candidates = route
                .as_ref()
                .map(|p| {
                    std::iter::once(p.primary.as_str())
                        .chain(p.fallback_chain.iter().map(String::as_str))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| vec![general.name.as_str()]);
            let started = Instant::now();
            let mut outcome = None;
            let mut error = None;
            let mut used_spec = general;
            for name in candidates {
                let Some(spec) = models.iter().find(|m| m.name == name) else {
                    continue;
                };
                match chat(spec, &[json!({"role":"system","content":"你是一个专业的AI助手。请认真完成以下任务。"}), json!({"role":"user","content":user})], task.estimated_output_tokens as i64, 0.3, None).await {
                    Ok(result) => { used_spec = spec; outcome = Some(result); break; }
                    Err(e) => error.get_or_insert_with(|| e.to_string()),
                };
            }
            let duration = started.elapsed().as_secs_f64();
            total_duration += duration;
            if let Some(result) = outcome {
                total_in += result.usage.prompt_tokens;
                total_out += result.usage.completion_tokens;
                used.insert(used_spec.name.clone());
                completed.insert(task.id, result.content);
                if tx.send(event("task_done", json!({
                    "id":task.id,"model":used_spec.name,"task_type":task.task_type,"duration":duration,
                    "cost":0.0,"input_tokens":result.usage.prompt_tokens,"output_tokens":result.usage.completion_tokens,
                    "saved_cost":0.0,"cache_hit":result.cache_hit
                }))).await.is_err(){return Err(AppError::Internal("orchestration receiver closed".into()));}
            } else {
                let detail = error.unwrap_or_else(|| "所有候选模型均失败".into());
                completed.insert(task.id, format!("执行失败: {detail}"));
                if tx.send(event("task_done", json!({"id":task.id,"model":general.name,"task_type":task.task_type,"duration":duration,"error":detail}))).await.is_err(){return Err(AppError::Internal("orchestration receiver closed".into()));}
            }
        }
    }
    let summary = planned_tasks
        .iter()
        .map(|t| {
            format!(
                "## 子任务 {}: {}\n\n{}",
                t.id,
                t.description,
                completed
                    .get(&t.id)
                    .map(String::as_str)
                    .unwrap_or("执行失败")
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    if tx
        .send(event(
            "task_start",
            json!({"id":0,"description":"汇总最终回答","model":aggregate.name}),
        ))
        .await
        .is_err()
    {
        return Err(AppError::Internal("orchestration receiver closed".into()));
    }
    let aggregation_messages = vec![
        json!({"role":"system","content":"将子任务结果汇总成连贯、完整的最终回答；如实保留失败信息，不得编造。"}),
        json!({"role":"user","content":format!("原始任务：{query}\n\n子任务执行结果：\n{summary}")}),
    ];
    let mut aggregate_stream =
        chat_stream(aggregate, &aggregation_messages, 4096, 0.3, None).await?;
    let aggregate_name = aggregate.name.clone();
    tokio::spawn(async move {
        use futures::StreamExt;
        let mut final_text = String::new();
        let mut reasoning = String::new();
        let mut agg_usage = crate::pricing::UsageDetail::default();
        while let Some(item) = aggregate_stream.next().await {
            match item {
                Ok(crate::providers::StreamEvent::CacheHit) => {}
                Ok(crate::providers::StreamEvent::Content(delta)) => {
                    final_text.push_str(&delta);
                    if tx
                        .send(event(
                            "token",
                            json!({"id":0,"model":aggregate_name,"delta":delta}),
                        ))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(crate::providers::StreamEvent::Reasoning(delta)) => reasoning.push_str(&delta),
                Ok(crate::providers::StreamEvent::Usage(v)) => agg_usage = v,
                Ok(crate::providers::StreamEvent::Done) => break,
                Err(_) => {
                    final_text = summary.clone();
                    break;
                }
            }
        }
        total_in += agg_usage.prompt_tokens;
        total_out += agg_usage.completion_tokens;
        used.insert(aggregate_name.clone());
        if tx.send(event("task_done",json!({"id":0,"model":aggregate_name,"task_type":"aggregate","duration":0.0,"cost":0.0,"input_tokens":agg_usage.prompt_tokens,"output_tokens":agg_usage.completion_tokens,"cache_hit":false}))).await.is_err(){return;}
        if tx.send(event("result",json!({"response":final_text,"model":aggregate_name,"cost":0.0,"input_tokens":total_in,"output_tokens":total_out,"saved_cost":0.0,"total_duration":total_duration,"models_used":used.into_iter().collect::<Vec<_>>(),"cache_hit":false,"reasoning":if reasoning.is_empty(){None}else{Some(reasoning)}}))).await.is_err() {}
    });
    Ok(Box::pin(futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|e| (e, rx))
    })))
}
