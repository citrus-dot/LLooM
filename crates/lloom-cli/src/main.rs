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
    /// Chat with the default model
    Chat { query: String },
    /// Orchestrate a complex task
    Orchestrate { query: String },
    /// Read/write .env configuration
    #[command(subcommand)]
    Config(ConfigCmd),
}

#[derive(Subcommand)]
enum ServiceCmd {
    /// Show service status
    Status,
    /// Start a service (ai / ollama)
    Start { name: String },
    /// Stop a service (ai / ollama)
    Stop { name: String },
    /// Restart a service (ai / ollama)
    Restart { name: String },
}

#[derive(Subcommand)]
enum ModelsCmd {
    /// List registered models
    List,
    /// Register a new model
    Add {
        name: String,
        #[arg(long)]
        provider: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        api_base: Option<String>,
        #[arg(long)]
        input_cost: Option<f64>,
        #[arg(long)]
        output_cost: Option<f64>,
        #[arg(long)]
        task_type: Option<String>,
    },
    /// Update a model's fields
    Update {
        name: String,
        #[arg(long)]
        input_cost: Option<f64>,
        #[arg(long)]
        output_cost: Option<f64>,
        #[arg(long)]
        api_base: Option<String>,
        #[arg(long)]
        task_type: Option<String>,
    },
    /// Remove a model (soft delete)
    Remove { name: String },
}

#[derive(Subcommand)]
enum BudgetsCmd {
    /// List budgets
    List,
    /// Set a budget
    Set {
        scope: String,
        scope_id: String,
        max_budget: f64,
        #[arg(long, default_value = "30d")]
        duration: String,
    },
    /// Check a budget
    Check { scope: String, scope_id: String },
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
        Command::Chat { query } => cmd_chat(&client, &query).await?,
        Command::Orchestrate { query } => cmd_orchestrate(&client, &query).await?,
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

fn svc_id(name: &str) -> &str {
    if name.to_lowercase().contains("ollama") {
        "ollama"
    } else {
        "ai"
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
    }
    Ok(())
}

async fn cmd_service(client: &Client, cmd: ServiceCmd) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ServiceCmd::Status => cmd_status(client).await?,
        ServiceCmd::Start { name } => {
            let r = post(client, &format!("/api/services/{}/start", svc_id(&name)), Value::Null).await?;
            println!("{}", r["message"].as_str().unwrap_or("ok"));
        }
        ServiceCmd::Stop { name } => {
            let r = post(client, &format!("/api/services/{}/stop", svc_id(&name)), Value::Null).await?;
            println!("{}", r["message"].as_str().unwrap_or("ok"));
        }
        ServiceCmd::Restart { name } => {
            let r = post(client, &format!("/api/services/{}/restart", svc_id(&name)), Value::Null).await?;
            println!("{}", r["message"].as_str().unwrap_or("ok"));
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

async fn cmd_chat(client: &Client, query: &str) -> Result<(), Box<dyn std::error::Error>> {
    let res = client
        .post(format!("{BASE}/api/chat/stream"))
        .json(&serde_json::json!({ "messages": [{"role": "user", "content": query}] }))
        .send()
        .await?;
    let text = res.text().await?;
    // chat/stream emits data-only SSE frames
    for line in text.lines() {
        if let Some(data) = line.strip_prefix("data: ") {
            if let Ok(v) = serde_json::from_str::<Value>(data) {
                if let Some(content) = v["content"].as_str() {
                    println!("{content}");
                } else if let Some(err) = v["error"].as_bool() {
                    if err {
                        eprintln!("✗ 请求失败: {}", v["detail"].as_str().unwrap_or(""));
                    }
                }
            }
        }
    }
    Ok(())
}

async fn cmd_orchestrate(client: &Client, query: &str) -> Result<(), Box<dyn std::error::Error>> {
    let res = client
        .post(format!("{BASE}/api/orchestrate/stream"))
        .json(&serde_json::json!({ "query": query, "history": [] }))
        .send()
        .await?;
    let text = res.text().await?;
    let mut current_event = String::new();
    for line in text.lines() {
        if let Some(ev) = line.strip_prefix("event:") {
            current_event = ev.trim().to_string();
        } else if let Some(data) = line.strip_prefix("data: ") {
            match current_event.as_str() {
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
                "task_done" => {
                    if let Ok(v) = serde_json::from_str::<Value>(data) {
                        let id = v["id"].as_i64().unwrap_or(0);
                        let dur = v["duration"].as_f64().unwrap_or(0.0);
                        println!("    ✓ 子任务 {id} 完成 ({dur:.1}s)");
                    }
                }
                "result" => {
                    if let Ok(v) = serde_json::from_str::<Value>(data) {
                        if let Some(r) = v["response"].as_str() {
                            println!("\n{r}");
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
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
