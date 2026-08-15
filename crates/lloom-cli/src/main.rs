//! LLooM CLI — command-line interface, feature-aligned with the WebUI.
//!
//! Local ops (models, budgets, usage, config) link `lloom-core` directly and
//! work offline. Server-dependent ops (service start/stop, chat, orchestrate)
//! talk to the running server via HTTP or the AI service.

use clap::{Parser, Subcommand};
use lloom_core::db;
use lloom_core::models::Model;
use std::process::exit;

#[derive(Parser)]
#[command(name = "lloom-cli", version, about = "LLooM — intelligent LLM routing platform CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize the database
    Init,
    /// Model management
    #[command(subcommand)]
    Models(ModelsCmd),
    /// Budget management
    #[command(subcommand)]
    Budgets(BudgetsCmd),
    /// Usage statistics + routing stats
    Usage,
    /// Service status / start / stop / restart
    #[command(subcommand)]
    Service(ServiceCmd),
    /// Show service status (alias for `service status`)
    Status,
    /// Chat with the default model
    Chat { query: String },
    /// Orchestrate a complex task (decompose + execute + aggregate)
    Orchestrate { query: String },
    /// Read/write .env configuration
    #[command(subcommand)]
    Config(ConfigCmd),
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
enum ConfigCmd {
    /// List all env keys (values masked)
    List,
    /// Set an env key
    Set { key: String, value: String },
}

const WEB_PORT: u16 = 7861;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        eprintln!("错误: {e}");
        exit(1);
    }
}

async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Init => {
            db::init_db()?;
            println!("✓ 数据库已初始化");
        }
        Command::Models(cmd) => models(cmd)?,
        Command::Budgets(cmd) => budgets(cmd)?,
        Command::Usage => usage()?,
        Command::Service(cmd) => service(cmd).await?,
        Command::Status => service(ServiceCmd::Status).await?,
        Command::Chat { query } => chat(&query).await?,
        Command::Orchestrate { query } => orchestrate(&query).await?,
        Command::Config(cmd) => config(cmd)?,
    }
    Ok(())
}

// ── Models ──

fn models(cmd: ModelsCmd) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ModelsCmd::List => {
            let models = db::list_models(true)?;
            if models.is_empty() {
                println!("(无模型)");
            } else {
                for m in &models {
                    println!(
                        "  {:<18} {:<12} {:<40} in=${:.6}/tok out=${:.6}/tok {}",
                        m.name,
                        m.provider,
                        m.litellm_model,
                        m.input_cost_per_token,
                        m.output_cost_per_token,
                        if m.is_active == 1 { "" } else { "[inactive]" }
                    );
                }
            }
            println!("共 {} 个模型", models.len());
        }
        ModelsCmd::Add {
            name,
            provider,
            model,
            api_base,
            input_cost,
            output_cost,
            task_type,
        } => {
            let m = Model {
                id: 0,
                name,
                provider,
                litellm_model: model,
                api_base: api_base.unwrap_or_default(),
                api_key_env: String::new(),
                task_type: task_type.unwrap_or_else(|| "general".into()),
                input_cost_per_token: input_cost.unwrap_or(0.0),
                output_cost_per_token: output_cost.unwrap_or(0.0),
                rpm: 60,
                is_active: 1,
            };
            let id = db::insert_model(&m)?;
            println!("✓ 模型已注册 (id={id}, name={})", m.name);
        }
        ModelsCmd::Update {
            name,
            input_cost,
            output_cost,
            api_base,
            task_type,
        } => {
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
                println!("未指定要更新的字段（--input-cost/--output-cost/--api-base/--task-type）");
                return Ok(());
            }
            if db::update_model(&name, &updates)? {
                println!("✓ 模型已更新: {name}");
            } else {
                println!("✗ 模型不存在: {name}");
            }
        }
        ModelsCmd::Remove { name } => {
            if db::delete_model(&name)? {
                println!("✓ 模型已删除: {name}");
            } else {
                println!("✗ 模型不存在: {name}");
            }
        }
    }
    Ok(())
}

// ── Budgets ──

fn budgets(cmd: BudgetsCmd) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        BudgetsCmd::List => {
            let budgets = db::list_budgets()?;
            if budgets.is_empty() {
                println!("(无预算)");
            } else {
                for b in &budgets {
                    println!("  {} {}  max=${:.2}  duration={}", b.scope, b.scope_id, b.max_budget, b.duration);
                }
            }
        }
        BudgetsCmd::Set {
            scope,
            scope_id,
            max_budget,
            duration,
        } => {
            db::upsert_budget(&scope, &scope_id, max_budget, &duration)?;
            println!("✓ 预算已设置: {scope}/{scope_id} = ${max_budget:.2} / {duration}");
        }
        BudgetsCmd::Check { scope, scope_id } => {
            match db::get_budget(&scope, &scope_id)? {
                Some(b) => {
                    let spent = db::get_total_spend(
                        if scope == "user" { Some(&scope_id) } else { None },
                        if scope == "model" { Some(&scope_id) } else { None },
                        None,
                    )?;
                    let within = (spent + 0.0) <= b.max_budget;
                    println!("  预算: ${:.2} / ${:.2} (已用 ${:.2})", spent, b.max_budget, spent);
                    println!("  状态: {}", if within { "✓ 在预算内" } else { "✗ 超出预算" });
                }
                None => println!("  未设置预算: {scope}/{scope_id}"),
            }
        }
    }
    Ok(())
}

// ── Usage + routing stats ──

fn usage() -> Result<(), Box<dyn std::error::Error>> {
    let stats = db::get_usage_stats(None, None, None)?;
    let total = db::get_total_spend(None, None, None)?;
    println!("累计花费: ${:.6}", total);
    if stats.is_empty() {
        println!("(无用量记录)");
    } else {
        for s in &stats {
            println!(
                "  {:<18} 输入={} 输出={} 请求={} 缓存命中={} 花费=${:.6}",
                s.model_name, s.total_input_tokens, s.total_output_tokens,
                s.request_count, s.cache_hits, s.total_cost
            );
        }
    }
    Ok(())
}

// ── Service management (via REST) ──

async fn service(cmd: ServiceCmd) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ServiceCmd::Status => {
            let ai = lloom_core::processes::check_ai_health().await;
            let ollama = lloom_core::processes::check_ollama_health().await;
            println!("  {:<14} {}", "Core Server", "Up (本进程)");
            println!(
                "  {:<14} {}",
                "Ollama",
                if ollama { "✓ Up" } else { "✗ Down" }
            );
            println!(
                "  {:<14} {} ({})",
                "AI Service",
                if ai.status == "ok" { "✓ Up" } else { "✗ Down" },
                if ai.ready { "ready" } else { "未配置模型" }
            );
        }
        ServiceCmd::Start { name } => {
            let id = svc_id(&name);
            let res = rest_post(&format!("/api/services/{id}/start")).await?;
            println!("{}", res.get("message").and_then(|m| m.as_str()).unwrap_or("ok"));
        }
        ServiceCmd::Stop { name } => {
            let id = svc_id(&name);
            let res = rest_post(&format!("/api/services/{id}/stop")).await?;
            println!("{}", res.get("message").and_then(|m| m.as_str()).unwrap_or("ok"));
        }
        ServiceCmd::Restart { name } => {
            let id = svc_id(&name);
            let res = rest_post(&format!("/api/services/{id}/restart")).await?;
            println!("{}", res.get("message").and_then(|m| m.as_str()).unwrap_or("ok"));
        }
    }
    Ok(())
}

fn svc_id(name: &str) -> &str {
    if name.to_lowercase().contains("ollama") {
        "ollama"
    } else {
        "ai"
    }
}

async fn rest_post(path: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let url = format!("http://localhost:{WEB_PORT}{path}");
    let client = reqwest::Client::new();
    let res = client.post(&url).send().await?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()).into());
    }
    Ok(res.json().await?)
}

// ── Chat ──

async fn chat(query: &str) -> Result<(), Box<dyn std::error::Error>> {
    let models = db::list_models(true)?;
    let spec = default_spec(&models);
    let messages = vec![serde_json::json!({ "role": "user", "content": query })];
    match lloom_core::ai_client::chat(&spec, &messages, 500, 0.3).await {
        Ok(res) => {
            println!("{}", res.content);
            println!("\n[模型={} 输入={}tok 输出={}tok 花费=${:.6}]", res.model, res.input_tokens, res.output_tokens, res.cost);
        }
        Err(e) => {
            eprintln!("✗ 聊天失败: {e}");
            eprintln!("  提示: 确保 AI 服务已启动（target/release/lloom-server）");
            exit(1);
        }
    }
    Ok(())
}

// ── Orchestrate ──

async fn orchestrate(query: &str) -> Result<(), Box<dyn std::error::Error>> {
    let models = db::list_models(true)?;
    let specs: Vec<lloom_core::ai_client::ModelSpec> = models.iter().map(|m| m.into()).collect();

    match lloom_core::ai_client::orchestrate_stream(query, &[], "", &specs, "").await {
        Ok(events) => {
            for ev in &events {
                match ev.event.as_str() {
                    "decompose" => {
                        let n = ev.data.get("sub_tasks").and_then(|s| s.as_array()).map(|a| a.len()).unwrap_or(0);
                        println!("📋 任务分解: {} 个子任务", n);
                    }
                    "task_start" => {
                        let desc = ev.data.get("description").and_then(|d| d.as_str()).unwrap_or("");
                        let model = ev.data.get("model").and_then(|m| m.as_str()).unwrap_or("");
                        println!("  ▶ 执行: {desc}  [{model}]");
                    }
                    "task_done" => {
                        let id = ev.data.get("id").and_then(|i| i.as_i64()).unwrap_or(0);
                        let dur = ev.data.get("duration").and_then(|d| d.as_f64()).unwrap_or(0.0);
                        println!("    ✓ 子任务 {id} 完成 ({dur:.1}s)");
                    }
                    "result" => {
                        if let Some(r) = ev.data.get("response").and_then(|x| x.as_str()) {
                            println!("\n{}", r);
                        }
                    }
                    _ => {}
                }
            }
        }
        Err(e) => {
            eprintln!("✗ 编排失败: {e}");
            eprintln!("  提示: 确保 AI 服务已启动（target/release/lloom-server）");
            exit(1);
        }
    }
    Ok(())
}

// ── Config ──

fn config(cmd: ConfigCmd) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ConfigCmd::List => {
            let env = lloom_core::config::read_env();
            if env.is_empty() {
                println!("(空)");
            } else {
                let mut keys: Vec<&String> = env.keys().collect();
                keys.sort();
                for k in keys {
                    let v = env.get(k).unwrap();
                    let masked = if v.trim().is_empty() { "(空)" } else { "***" };
                    println!("  {:<24} {}", k, masked);
                }
            }
        }
        ConfigCmd::Set { key, value } => {
            let path = lloom_core::config::env_file_path();
            let mut env = lloom_core::config::read_env();
            env.insert(key.clone(), value.clone());
            let mut keys: Vec<&String> = env.keys().collect();
            keys.sort();
            let mut out = String::new();
            for k in keys {
                out.push_str(&format!("{k}={}\n", env.get(k).unwrap()));
            }
            std::fs::write(path, out)?;
            println!("✓ 已设置 {key}");
        }
    }
    Ok(())
}

fn default_spec(models: &[Model]) -> lloom_core::ai_client::ModelSpec {
    match models.first() {
        Some(m) => lloom_core::ai_client::ModelSpec::from(m),
        None => lloom_core::ai_client::ModelSpec {
            name: "qwen2.5-local".to_string(),
            litellm_model: "ollama/qwen2.5:latest".to_string(),
            api_base: "http://localhost:11434".to_string(),
            api_key: String::new(),
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
        },
    }
}
