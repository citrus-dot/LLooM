//! Unified LLM client：原生 Rust provider 适配器为唯一调用路径
//! （OpenAI-compatible / Anthropic Messages 直连，Python 传输层已移除）。

use crate::config;
use crate::error::{AppError, Result};
use crate::models::Model;
use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::pin::Pin;

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
        Self {
            name: m.name.clone(),
            provider: m.provider_name().to_string(),
            provider_model: m.provider_model.clone(),
            api_base: config::resolve_env_or_literal(m.api_base()),
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

pub async fn chat_stream(
    spec: &ModelSpec,
    messages: &[Value],
    max_tokens: i64,
    temperature: f64,
) -> Result<ChatStream> {
    crate::providers::stream(spec, messages, max_tokens, temperature).await
}

/// chat 主路径的两层缓存装配（opt-in）。probe/shadow 必须传 None——
/// 缓存命中会把成本样本污染成 0，校准数据全部失真。
pub struct ChatCacheCtx<'a> {
    pub conversation_id: &'a str,
    pub cache_dir: &'a str,
}

fn cache_key(spec: &ModelSpec, messages: &[Value], conversation_id: &str) -> Option<String> {
    let query = messages
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(Value::as_str) == Some("user"))?
        .get("content")?
        .as_str()?;
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
    Some(crate::context::exact_key(
        &spec.name,
        &crate::context::system_id(messages),
        &fingerprint,
        query,
    ))
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
            Some(c) => crate::exact_cache::ExactCache::open(
                crate::config::data_dir().join("cache_exact.sqlite3"),
                86_400,
            )
            .ok()
            .map(|store| (store, c)),
            None => None,
        };
        let key = exact
            .as_ref()
            .and_then(|(_, c)| cache_key(spec, messages, c.conversation_id));
        if let (Some((store, _)), Some(key)) = (&exact, &key) {
            if let Ok(Some(content)) = store.lookup(key) {
                let prompt_tokens = messages
                    .iter()
                    .map(|m| {
                        crate::context::count_tokens(
                            m.get("content").and_then(Value::as_str).unwrap_or(""),
                        ) as i64
                    })
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
        let semantic = if exact.is_some() && context_free && !query.is_empty() {
            let threshold = std::env::var("LLOOM_SEMANTIC_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.88);
            match crate::semantic_cache::SemanticCache::open(
                crate::config::data_dir().join("cache_semantic.sqlite3"),
                86_400,
                threshold,
            )
            .ok()
            {
                Some(cache) => crate::semantic_cache::SemanticCache::embed(query)
                    .ok()
                    .map(|vector| (cache, vector)),
                None => None,
            }
        } else {
            None
        };
        if let Some((cache, vector)) = &semantic {
            if let Ok(Some((content, _similarity))) = cache.lookup(&spec.name, vector) {
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
                    &spec.name,
                    &result.content,
                    (!c.conversation_id.is_empty()).then_some(c.conversation_id),
                );
                let max = std::env::var("LLOOM_CACHE_MAX_ENTRIES")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(5000);
                let _ = store.sweep(max);
            }
        }
        if crate::context::cacheable(query, temperature) {
            if let Some((cache, vector)) = &semantic {
                let _ = cache.store(&spec.name, query, &result.content, vector);
                let max = std::env::var("LLOOM_CACHE_MAX_ENTRIES")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(5000);
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

/// LLM-based task classification (fallback layer). Never fails hard.
pub async fn classify(text: &str, classifier: &ModelSpec, valid_types: &[&str]) -> String {
    const PROMPT: &str = "将用户请求分类，只返回一个类别名称，不要解释。";
    let valid = valid_types.join(" / ");
    match chat(
        classifier,
        &[
            json!({"role":"system","content":format!("{PROMPT} 可选类别：{valid}")}),
            json!({"role":"user","content":text.chars().take(500).collect::<String>()}),
        ],
        20,
        0.0,
        None,
    )
    .await
    {
        Ok(result) => {
            let lower = result.content.to_lowercase();
            valid_types
                .iter()
                .find(|t| lower.contains(**t))
                .copied()
                .unwrap_or("general")
                .to_string()
        }
        Err(_) => "general".to_string(),
    }
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
        let mut provider_stream = chat_stream(spec, prepared_messages, 2000, 0.3).await?;
        let spec_name = spec.name.clone();
        tokio::spawn(async move {
            use futures::StreamExt;
            let started = Instant::now();
            let mut content = String::new();
            let mut reasoning = String::new();
            let mut usage = crate::pricing::UsageDetail::default();
            while let Some(item) = provider_stream.next().await {
                match item {
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
            tx.send(event("result",json!({"response":content,"model":spec_name,"cost":0.0,"input_tokens":usage.prompt_tokens,"output_tokens":usage.completion_tokens,"saved_cost":0.0,"total_duration":duration,"models_used":[spec_name],"cache_hit":false,"reasoning":if reasoning.is_empty(){None}else{Some(reasoning)}}))).await.ok();
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
    let mut aggregate_stream = chat_stream(aggregate, &aggregation_messages, 4096, 0.3).await?;
    let aggregate_name = aggregate.name.clone();
    tokio::spawn(async move {
        use futures::StreamExt;
        let mut final_text = String::new();
        let mut reasoning = String::new();
        let mut agg_usage = crate::pricing::UsageDetail::default();
        while let Some(item) = aggregate_stream.next().await {
            match item {
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
        tx.send(event("result",json!({"response":final_text,"model":aggregate_name,"cost":0.0,"input_tokens":total_in,"output_tokens":total_out,"saved_cost":0.0,"total_duration":total_duration,"models_used":used.into_iter().collect::<Vec<_>>(),"cache_hit":false,"reasoning":if reasoning.is_empty(){None}else{Some(reasoning)}}))).await.ok();
    });
    Ok(Box::pin(futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|e| (e, rx))
    })))
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

fn mk_event(name: String, data: &str) -> SseEvent {
    let parsed = serde_json::from_str::<Value>(data).unwrap_or(Value::String(data.to_string()));
    SseEvent {
        event: name,
        data: parsed,
    }
}
