//! LLooM CLI — command-line interface.
//!
//! All operations go through the REST API exposed by `lloom-server` (:7861).
//! This keeps CLI/WebUI/TUI in one consistent state. Requires the server
//! running: `target/release/lloom-server`.

use clap::{Parser, Subcommand};
use reqwest::Client;
use serde_json::Value;
use std::process::exit;

#[derive(Parser)]
#[command(name = "lloom-cli", version, about = "LLooM — intelligent LLM routing platform CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show service status
    Status,
    /// Service management
    #[command(subcommand)]
    Service(ServiceCmd),
    /// Model management
    #[command(subcommand)]
    Models(ModelsCmd),
    /// Budget management
    #[command(subcommand)]
    Budgets(BudgetsCmd),
    /// Usage statistics
    Usage,
    /// Chat with the default model (use --interactive for multi-turn)
    Chat {
        /// Question or message to send (e.g. `lloom-cli chat "What is 2+2?"`)
        query: String,
        /// Resume a conversation by ID — run `lloom-cli conversation list` to
        /// find session IDs
        #[arg(long)]
        session: Option<String>,
        /// Interactive multi-turn session (prompt repeatedly until EOF)
        #[arg(long)]
        interactive: bool,
    },
    /// Orchestrate a complex task
    Orchestrate {
        /// Task description to decompose and run
        query: String,
    },
    /// Conversation management
    #[command(subcommand)]
    Conversation(ConversationCmd),
    /// Read/write .env configuration
    #[command(subcommand)]
    Config(ConfigCmd),
}

#[derive(Subcommand)]
enum ConversationCmd {
    /// List conversations (run this to find session IDs)
    List,
    /// Show one conversation's messages
    Show {
        /// Conversation/session ID (see `lloom-cli conversation list`)
        id: String,
    },
    /// Delete a conversation
    Delete {
        /// Conversation/session ID (see `lloom-cli conversation list`)
        id: String,
    },
    /// Rename a conversation
    Rename {
        /// Conversation/session ID (see `lloom-cli conversation list`)
        id: String,
        /// New title
        title: String,
    },
    /// Start a fresh conversation
    New,
}

#[derive(Subcommand)]
enum ServiceCmd {
    /// Show service status
    Status,
    /// Start a service (ai / ollama)
    Start {
        /// Service name: ai or ollama
        name: String,
    },
    /// Stop a service (ai / ollama)
    Stop {
        /// Service name: ai or ollama
        name: String,
    },
    /// Restart a service (ai / ollama)
    Restart {
        /// Service name: ai or ollama
        name: String,
    },
    /// Show recent logs for a service (ai / ollama)
    Logs {
        /// Service name: ai or ollama
        name: String,
    },
    /// Apply config changes: smart-restart services affected by changed keys
    Apply {
        /// Env keys that changed (e.g. DASHSCOPE_API_KEY)
        keys: Vec<String>,
    },
    /// Shut down all services (AI + Ollama + core server)
    Shutdown,
}

#[derive(Subcommand)]
enum ModelsCmd {
    /// List registered models
    List,
    /// Register a new model
    Add {
        /// Model name (e.g. qwen2.5-local)
        name: String,
        /// Provider: dashscope / openai / anthropic / ollama / custom
        #[arg(long)]
        provider: String,
        /// LiteLLM model string (e.g. ollama/qwen2.5:latest)
        #[arg(long)]
        model: String,
        /// API base URL (e.g. http://localhost:11434)
        #[arg(long)]
        api_base: Option<String>,
        /// Input cost per token (e.g. 0.000001)
        #[arg(long)]
        input_cost: Option<f64>,
        /// Output cost per token (e.g. 0.000002)
        #[arg(long)]
        output_cost: Option<f64>,
        /// Task type: simple_qa / general / coding / math_logic / complex_reasoning
        #[arg(long)]
        task_type: Option<String>,
    },
    /// Update a model's fields
    Update {
        /// Model name to update
        name: String,
        /// New input cost per token
        #[arg(long)]
        input_cost: Option<f64>,
        /// New output cost per token
        #[arg(long)]
        output_cost: Option<f64>,
        /// New API base URL
        #[arg(long)]
        api_base: Option<String>,
        /// New task type
        #[arg(long)]
        task_type: Option<String>,
    },
    /// Remove a model (soft delete)
    Remove {
        /// Model name to remove
        name: String,
    },
}

#[derive(Subcommand)]
enum BudgetsCmd {
    /// List budgets
    List,
    /// Set a budget
    Set {
        /// Budget scope: user / model
        scope: String,
        /// Scope ID (user name or model name)
        scope_id: String,
        /// Max budget in USD (e.g. 10)
        max_budget: f64,
        /// Duration: 30d / 7d / 1d
        #[arg(long, default_value = "30d")]
        duration: String,
    },
    /// Check a budget
    Check {
        /// Budget scope: user / model
        scope: String,
        /// Scope ID (user name or model name)
        scope_id: String,
    },
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// List all env keys (values masked)
    List,
    /// Set an env key
    Set { key: String, value: String },
}

const BASE: &str = "http://localhost:7861";

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        eprintln!("错误: {e}");
        eprintln!("  提示: 确保 lloom-server 已启动（target/release/lloom-server）");
        exit(1);
    }
}

async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();
    match cli.command {
        Command::Status => cmd_status(&client).await?,
        Command::Service(c) => cmd_service(&client, c).await?,
        Command::Models(c) => cmd_models(&client, c).await?,
        Command::Budgets(c) => cmd_budgets(&client, c).await?,
        Command::Usage => cmd_usage(&client).await?,
        Command::Chat { query, session, interactive } => cmd_chat(&client, &query, session.as_deref(), interactive).await?,
        Command::Orchestrate { query } => cmd_orchestrate(&client, &query).await?,
        Command::Conversation(c) => cmd_conversation(&client, c).await?,
        Command::Config(c) => cmd_config(&client, c).await?,
    }
    Ok(())
}

// ── helpers ──

async fn get(client: &Client, path: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let res = client.get(format!("{BASE}{path}")).send().await?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()).into());
    }
    Ok(res.json().await?)
}

async fn post(client: &Client, path: &str, body: Value) -> Result<Value, Box<dyn std::error::Error>> {
    let res = client.post(format!("{BASE}{path}")).json(&body).send().await?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()).into());
    }
    Ok(res.json().await?)
}

async fn del(client: &Client, path: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let res = client.delete(format!("{BASE}{path}")).send().await?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()).into());
    }
    Ok(res.json().await?)
}

fn svc_id(name: &str) -> Result<&str, &'static str> {
    let n = name.to_lowercase();
    if n == "ai" || n.contains("ai service") {
        Ok("ai")
    } else if n.contains("ollama") {
        Ok("ollama")
    } else if n.contains("core") {
        Err("Core Server 是宿主进程，不能通过 CLI 管理；请用 ai / ollama")
    } else {
        Ok("ai")
    }
}

// ── Status / Service ──

async fn cmd_status(client: &Client) -> Result<(), Box<dyn std::error::Error>> {
    let status: Value = get(client, "/api/services/status").await?;
    let services = status["services"].as_array().cloned().unwrap_or_default();
    for s in &services {
        let name = s["name"].as_str().unwrap_or("");
        let st = s["status"].as_str().unwrap_or("");
        let healthy = s["healthy"].as_bool().unwrap_or(false);
        let mark = if healthy { "✓" } else { "✗" };
        println!("  {mark} {:<14} {}", name, st);
        if let Some(d) = s["detail"].as_str() {
            if !d.is_empty() {
                println!("        {d}");
            }
        }
    }
    Ok(())
}

async fn cmd_service(client: &Client, cmd: ServiceCmd) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ServiceCmd::Status => cmd_status(client).await?,
        ServiceCmd::Start { name } => {
            let id = svc_id(&name)?;
            let r = post(client, &format!("/api/services/{id}/start"), Value::Null).await?;
            println!("{}", r["message"].as_str().unwrap_or("ok"));
        }
        ServiceCmd::Stop { name } => {
            let id = svc_id(&name)?;
            let r = post(client, &format!("/api/services/{id}/stop"), Value::Null).await?;
            println!("{}", r["message"].as_str().unwrap_or("ok"));
        }
        ServiceCmd::Restart { name } => {
            let id = svc_id(&name)?;
            let r = post(client, &format!("/api/services/{id}/restart"), Value::Null).await?;
            println!("{}", r["message"].as_str().unwrap_or("ok"));
        }
        ServiceCmd::Logs { name } => {
            let id = svc_id(&name)?;
            let r = get(client, &format!("/api/services/{id}/logs")).await?;
            let logs = r["logs"].as_str().unwrap_or("");
            if logs.is_empty() {
                println!("(暂无日志)");
            } else {
                print!("{logs}");
            }
        }
        ServiceCmd::Apply { keys } => {
            let r = post(client, "/api/services/smart-restart", serde_json::json!({ "changed_keys": keys })).await?;
            if r["ok"].as_bool().unwrap_or(false) {
                let restarted = r["restarted"].as_array().cloned().unwrap_or_default();
                println!("✓ 配置已生效，已重启: {}", restarted.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "));
            } else {
                let errors = r["errors"].as_array().cloned().unwrap_or_default();
                eprintln!("✗ 重启失败: {}", errors.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join("; "));
            }
        }
        ServiceCmd::Shutdown => {
            let r = post(client, "/api/shutdown", Value::Null).await?;
            println!("{}", if r["shutting_down"].as_bool().unwrap_or(false) { "正在关闭全部服务..." } else { "关闭请求已发送" });
        }
    }
    Ok(())
}

// ── Models ──

async fn cmd_models(client: &Client, cmd: ModelsCmd) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ModelsCmd::List => {
            let data: Value = get(client, "/api/models").await?;
            let models = data["models"].as_array().cloned().unwrap_or_default();
            if models.is_empty() {
                println!("(无模型)");
            } else {
                for m in &models {
                    println!(
                        "  {:<18} {:<12} {:<40} in=${:.6}/tok out=${:.6}/tok {}",
                        m["name"].as_str().unwrap_or(""),
                        m["provider"].as_str().unwrap_or(""),
                        m["litellm_model"].as_str().unwrap_or(""),
                        m["input_cost_per_token"].as_f64().unwrap_or(0.0),
                        m["output_cost_per_token"].as_f64().unwrap_or(0.0),
                        if m["task_type"].as_str().unwrap_or("").is_empty() { "" } else { m["task_type"].as_str().unwrap_or("") },
                    );
                }
            }
            println!("共 {} 个模型", models.len());
        }
        ModelsCmd::Add { name, provider, model, api_base, input_cost, output_cost, task_type } => {
            let body = serde_json::json!({
                "name": name,
                "provider": provider,
                "litellm_model": model,
                "api_base": api_base.unwrap_or_default(),
                "api_key_env": "",
                "task_type": task_type.unwrap_or_else(|| "general".into()),
                "input_cost_per_token": input_cost.unwrap_or(0.0),
                "output_cost_per_token": output_cost.unwrap_or(0.0),
                "rpm": 60,
            });
            let r = post(client, "/api/models", body).await?;
            println!("✓ 模型已注册 (id={}, name={})", r["id"], r["name"]);
        }
        ModelsCmd::Update { name, input_cost, output_cost, api_base, task_type } => {
            let mut updates = serde_json::Map::new();
            if let Some(v) = input_cost {
                updates.insert("input_cost_per_token".into(), serde_json::json!(v));
            }
            if let Some(v) = output_cost {
                updates.insert("output_cost_per_token".into(), serde_json::json!(v));
            }
            if let Some(v) = api_base {
                updates.insert("api_base".into(), serde_json::json!(v));
            }
            if let Some(v) = task_type {
                updates.insert("task_type".into(), serde_json::json!(v));
            }
            if updates.is_empty() {
                println!("未指定要更新的字段");
                return Ok(());
            }
            let res = client
                .put(format!("{BASE}/api/models/{}", urlencode(&name)))
                .json(&Value::Object(updates))
                .send()
                .await?;
            if res.status().is_success() {
                println!("✓ 模型已更新: {name}");
            } else {
                println!("✗ 更新失败 (HTTP {})", res.status());
            }
        }
        ModelsCmd::Remove { name } => {
            let r = del(client, &format!("/api/models/{}", urlencode(&name))).await?;
            if r["deleted"].as_bool().unwrap_or(false) {
                println!("✓ 模型已删除: {name}");
            } else {
                println!("✗ 模型不存在: {name}");
            }
        }
    }
    Ok(())
}

// ── Budgets ──

async fn cmd_budgets(client: &Client, cmd: BudgetsCmd) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        BudgetsCmd::List => {
            let data: Value = get(client, "/api/budgets").await?;
            let budgets = data["budgets"].as_array().cloned().unwrap_or_default();
            if budgets.is_empty() {
                println!("(无预算)");
            } else {
                for b in &budgets {
                    println!(
                        "  {} {}  max=${:.2}  duration={}",
                        b["scope"].as_str().unwrap_or(""),
                        b["scope_id"].as_str().unwrap_or(""),
                        b["max_budget"].as_f64().unwrap_or(0.0),
                        b["duration"].as_str().unwrap_or(""),
                    );
                }
            }
        }
        BudgetsCmd::Set { scope, scope_id, max_budget, duration } => {
            let r = post(client, "/api/budgets", serde_json::json!({
                "scope": scope, "scope_id": scope_id, "max_budget": max_budget, "duration": duration,
            })).await?;
            if r["set"].as_bool().unwrap_or(false) {
                println!("✓ 预算已设置: {scope}/{scope_id} = ${max_budget:.2} / {duration}");
            }
        }
        BudgetsCmd::Check { scope, scope_id } => {
            let r: Value = get(client, &format!(
                "/api/budgets/check?scope={}&scope_id={}", urlencode(&scope), urlencode(&scope_id)
            )).await?;
            let spent = r["spent"].as_f64().unwrap_or(0.0);
            let max = r["budget"]["max_budget"].as_f64();
            match max {
                Some(m) => {
                    let within = r["within_budget"].as_bool().unwrap_or(false);
                    println!("  预算: ${:.2} / ${:.2} (已用 ${:.2})", spent, m, spent);
                    println!("  状态: {}", if within { "✓ 在预算内" } else { "✗ 超出预算" });
                }
                None => println!("  未设置预算: {scope}/{scope_id}"),
            }
        }
    }
    Ok(())
}

// ── Usage ──

async fn cmd_usage(client: &Client) -> Result<(), Box<dyn std::error::Error>> {
    let stats: Value = get(client, "/api/stats").await?;
    let usage: Value = get(client, "/api/usage").await?;
    println!("累计花费: ${:.6}", stats["total_spend"].as_f64().unwrap_or(0.0));
    let rows = usage["usage"].as_array().cloned().unwrap_or_default();
    if rows.is_empty() {
        println!("(无用量记录)");
    } else {
        for s in &rows {
            println!(
                "  {:<18} 输入={} 输出={} 请求={} 缓存命中={} 花费=${:.6}",
                s["model_name"].as_str().unwrap_or(""),
                s["total_input_tokens"].as_i64().unwrap_or(0),
                s["total_output_tokens"].as_i64().unwrap_or(0),
                s["request_count"].as_i64().unwrap_or(0),
                s["cache_hits"].as_i64().unwrap_or(0),
                s["total_cost"].as_f64().unwrap_or(0.0),
            );
        }
    }
    Ok(())
}

// ── Chat / Orchestrate (SSE via server) ──

/// Resolve a --session argument: if it's already a valid ID, use as-is;
/// otherwise treat it as a conversation title (or prefix) and pick the first
/// match. Errors if nothing matches.
async fn resolve_session_id(client: &Client, input: &str) -> Result<String, Box<dyn std::error::Error>> {
    // Fast path: assume it's an ID and see if the conversation exists.
    if let Ok(conv) = get(client, &format!("/api/conversations/{input}")).await {
        if conv.get("id").is_some() || conv.get("messages").is_some() {
            return Ok(input.to_string());
        }
    }
    // Title match against the conversation list.
    let data: Value = get(client, "/api/conversations").await?;
    let convs = data["conversations"].as_array().cloned().unwrap_or_default();
    let lower = input.to_lowercase();
    for c in &convs {
        let title = c["title"].as_str().unwrap_or("").to_lowercase();
        if title.contains(&lower) {
            return Ok(c["id"].as_str().unwrap_or("").to_string());
        }
    }
    Err(format!("找不到会话: {input}（先用 lloom-cli conversation list 查看）").into())
}

async fn cmd_chat(client: &Client, query: &str, session: Option<&str>, interactive: bool) -> Result<(), Box<dyn std::error::Error>> {
    // history holds the conversation so far (role/content pairs).
    let mut history: Vec<Value> = Vec::new();
    if let Some(id) = session {
        // If the argument isn't a valid session ID, try matching it as a title
        // (or title prefix) from the conversation list.
        let resolved = resolve_session_id(client, id).await?;
        let conv: Value = get(client, &format!("/api/conversations/{resolved}")).await?;
        for m in conv["messages"].as_array().cloned().unwrap_or_default() {
            let role = m["role"].as_str().unwrap_or("");
            if role == "user" || role == "assistant" {
                history.push(serde_json::json!({ "role": role, "content": m["content"] }));
            }
        }
    }

    // Single-shot: send query once, stream the reply, done.
    if !interactive {
        let mut messages = history.clone();
        messages.push(serde_json::json!({ "role": "user", "content": query }));
        stream_chat(client, &messages).await?;
        println!();
        return Ok(());
    }

    // Interactive: keep history across turns, prompt for each new input.
    use std::io::{self, Write};
    history.push(serde_json::json!({ "role": "user", "content": query }));
    loop {
        let reply = stream_chat(client, &history).await?;
        println!();
        if !reply.is_empty() {
            history.push(serde_json::json!({ "role": "assistant", "content": reply }));
        }
        print!("你> ");
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            return Ok(());
        }
        let input = line.trim().to_string();
        if input.is_empty() || input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit") {
            return Ok(());
        }
        history.push(serde_json::json!({ "role": "user", "content": input }));
    }
}

/// POST /api/chat/stream, printing tokens as they arrive; returns the full reply.
async fn stream_chat(client: &Client, messages: &[Value]) -> Result<String, Box<dyn std::error::Error>> {
    use futures_util::StreamExt;
    use std::io::Write;
    let res = client
        .post(format!("{BASE}/api/chat/stream"))
        .json(&serde_json::json!({ "messages": messages }))
        .send()
        .await?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()).into());
    }
    let mut stream = res.bytes_stream();
    let mut buf = String::new();
    let mut reply = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line: String = buf.drain(..=pos).collect();
            let line = line.trim_end_matches(['\r', '\n']);
            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(v) = serde_json::from_str::<Value>(data) {
                    if let Some(content) = v["content"].as_str() {
                        print!("{content}");
                        std::io::stdout().flush()?;
                        reply.push_str(content);
                    } else if v["error"].as_bool().unwrap_or(false) {
                        eprintln!("\n✗ 请求失败: {}", v["detail"].as_str().unwrap_or(""));
                    }
                }
            }
        }
    }
    Ok(reply)
}

async fn cmd_conversation(client: &Client, cmd: ConversationCmd) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ConversationCmd::List => {
            let data: Value = get(client, "/api/conversations").await?;
            let convs = data["conversations"].as_array().cloned().unwrap_or_default();
            if convs.is_empty() {
                println!("(无会话)");
            }
            for c in convs {
                let id = c["id"].as_str().unwrap_or("");
                let title = c["title"].as_str().unwrap_or("");
                let n = c["message_count"].as_i64().unwrap_or(0);
                println!("  {id}  {title}  ({n} 条)");
            }
        }
        ConversationCmd::Show { id } => {
            let resolved = resolve_session_id(client, &id).await?;
            let conv: Value = get(client, &format!("/api/conversations/{resolved}")).await?;
            for m in conv["messages"].as_array().cloned().unwrap_or_default() {
                let role = m["role"].as_str().unwrap_or("");
                let content = m["content"].as_str().unwrap_or("");
                let mark = if role == "user" { "你" } else { "AI" };
                println!("[{mark}] {content}");
            }
        }
        ConversationCmd::Delete { id } => {
            let resolved = resolve_session_id(client, &id).await?;
            let r = del(client, &format!("/api/conversations/{resolved}")).await?;
            println!("{}", if r["deleted"].as_bool().unwrap_or(false) { "已删除" } else { "删除失败" });
        }
        ConversationCmd::Rename { id, title } => {
            let resolved = resolve_session_id(client, &id).await?;
            let r = client
                .put(format!("{BASE}/api/conversations/{resolved}"))
                .json(&serde_json::json!({ "title": title }))
                .send()
                .await?
                .json::<Value>()
                .await?;
            println!("{}", if r["renamed"].as_bool().unwrap_or(false) { "已重命名" } else { "重命名失败" });
        }
        ConversationCmd::New => {
            let r = post(client, "/api/conversations", serde_json::json!({ "messages": [] })).await?;
            println!("新建会话: {}", r["id"].as_str().unwrap_or(""));
        }
    }
    Ok(())
}

async fn cmd_orchestrate(client: &Client, query: &str) -> Result<(), Box<dyn std::error::Error>> {
    use futures_util::StreamExt;
    use std::io::Write;

    let res = client
        .post(format!("{BASE}/api/orchestrate/stream"))
        .json(&serde_json::json!({ "query": query, "history": [] }))
        .send()
        .await?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()).into());
    }

    // True streaming: consume the SSE byte stream line by line, printing token
    // deltas as they arrive (instead of buffering the whole response first).
    let mut stream = res.bytes_stream();
    let mut buf = String::new();
    let mut current_event = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line: String = buf.drain(..=pos).collect();
            let line = line.trim_end_matches(['\r', '\n']);
            if let Some(ev) = line.strip_prefix("event:") {
                current_event = ev.trim().to_string();
            } else if let Some(data) = line.strip_prefix("data: ") {
                handle_orchestrate_event(&current_event, data);
            }
        }
    }
    // Flush any trailing line without a newline terminator.
    if let Some(data) = buf.strip_prefix("data: ") {
        handle_orchestrate_event(&current_event, data);
    }
    let _ = std::io::stdout().flush();
    Ok(())
}

/// Handle one SSE `data:` payload from the orchestrate stream.
fn handle_orchestrate_event(ev: &str, data: &str) {
    use std::io::Write;
    match ev {
        "decompose" => {
            if let Ok(v) = serde_json::from_str::<Value>(data) {
                let n = v["sub_tasks"].as_array().map(|a| a.len()).unwrap_or(0);
                println!("📋 任务分解: {} 个子任务", n);
            }
        }
        "task_start" => {
            if let Ok(v) = serde_json::from_str::<Value>(data) {
                let desc = v["description"].as_str().unwrap_or("");
                let model = v["model"].as_str().unwrap_or("");
                println!("  ▶ 执行: {desc}  [{model}]");
            }
        }
        "token" => {
            // Stream tokens to stdout as they arrive — no trailing newline.
            if let Ok(v) = serde_json::from_str::<Value>(data) {
                if let Some(delta) = v["delta"].as_str() {
                    print!("{delta}");
                    let _ = std::io::stdout().flush();
                }
            }
        }
        "task_done" => {
            if let Ok(v) = serde_json::from_str::<Value>(data) {
                let id = v["id"].as_i64().unwrap_or(0);
                let dur = v["duration"].as_f64().unwrap_or(0.0);
                if let Some(err) = v["error"].as_str() {
                    if !err.is_empty() {
                        println!("\n    ✗ 子任务 {id} 失败: {err}");
                    }
                } else {
                    let cached = v["cache_hit"].as_bool().unwrap_or(false);
                    let mark = if cached { "（缓存命中）" } else { "" };
                    println!("\n    ✓ 子任务 {id} 完成 ({dur:.1}s){mark}");
                }
            }
        }
        "result" => {
            if let Ok(v) = serde_json::from_str::<Value>(data) {
                if let Some(r) = v["response"].as_str() {
                    println!("\n\n{r}");
                }
                let models = v["models_used"].as_array().cloned().unwrap_or_default();
                let names: Vec<&str> = models.iter().filter_map(|m| m.as_str()).collect();
                if !names.is_empty() {
                    println!("\n── 调用模型: {}", names.join(" | "));
                }
                if let Some(agg) = v["aggregator"].as_str() {
                    if !agg.is_empty() {
                        println!("   汇总模型: {agg}");
                    }
                }
                if v["cache_hit"].as_bool().unwrap_or(false) {
                    println!("   来自语义缓存");
                }
            }
        }
        _ => {}
    }
}

// ── Config ──

async fn cmd_config(client: &Client, cmd: ConfigCmd) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ConfigCmd::List => {
            let env: Value = get(client, "/api/config").await?;
            let obj = env.as_object().cloned().unwrap_or_default();
            if obj.is_empty() {
                println!("(空)");
            } else {
                let mut keys: Vec<&String> = obj.keys().collect();
                keys.sort();
                for k in keys {
                    let v = obj.get(k).and_then(|x| x.as_str()).unwrap_or("");
                    let masked = if v.trim().is_empty() { "(空)" } else { "***" };
                    println!("  {:<24} {}", k, masked);
                }
            }
        }
        ConfigCmd::Set { key, value } => {
            let r = post(client, "/api/config", serde_json::json!({ "updates": { key.clone(): value } })).await?;
            let updated = r["updated"].as_array().cloned().unwrap_or_default();
            if !updated.is_empty() {
                println!("✓ 已设置 {key}");
            } else {
                println!("✗ 设置失败");
            }
        }
    }
    Ok(())
}

fn urlencode(s: &str) -> String {
    // Simple percent-encoding for path segments (model names, scope ids).
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/' {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}
