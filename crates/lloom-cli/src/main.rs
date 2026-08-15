//! LLooM CLI — command-line interface.
//!
//! Links `lloom-core` directly for local operations (models, budgets, usage)
//! and calls the running AI service for chat.

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
    /// Usage statistics
    Usage,
    /// Service status
    Status,
    /// Chat with the default model
    Chat { query: String },
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
        Command::Status => status().await?,
        Command::Chat { query } => chat(&query).await?,
    }
    Ok(())
}

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
        } => {
            let m = Model {
                id: 0,
                name,
                provider,
                litellm_model: model,
                api_base: api_base.unwrap_or_default(),
                api_key_env: String::new(),
                task_type: "general".to_string(),
                input_cost_per_token: input_cost.unwrap_or(0.0),
                output_cost_per_token: output_cost.unwrap_or(0.0),
                rpm: 60,
                is_active: 1,
            };
            let id = db::insert_model(&m)?;
            println!("✓ 模型已注册 (id={id}, name={})", m.name);
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

async fn status() -> Result<(), Box<dyn std::error::Error>> {
    let ai = lloom_core::processes::check_ai_health().await;
    let ollama = lloom_core::processes::check_ollama_health().await;
    println!("  {:<14} {}", "Core Server", "Up (this process)");
    println!("  {:<14} {}", "Ollama", if ollama { "Up" } else { "Down" });
    println!(
        "  {:<14} {} ({})",
        "AI Service",
        if ai.status == "ok" { "Up" } else { "Down" },
        if ai.ready { "ready" } else { "not ready" }
    );
    Ok(())
}

async fn chat(query: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Find the default model (first active one), or use qwen2.5-local naming.
    let models = db::list_models(true)?;
    let spec = match models.first() {
        Some(m) => lloom_core::ai_client::ModelSpec::from(m),
        None => lloom_core::ai_client::ModelSpec {
            name: "qwen2.5-local".to_string(),
            litellm_model: "ollama/qwen2.5:latest".to_string(),
            api_base: "http://localhost:11434".to_string(),
            api_key: String::new(),
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
        },
    };
    let messages = vec![serde_json::json!({ "role": "user", "content": query })];
    match lloom_core::ai_client::chat(&spec, &messages, 500, 0.3).await {
        Ok(res) => {
            println!("{}", res.content);
            println!("\n[模型={} 输入={}tok 输出={}tok 花费=${:.6}]", res.model, res.input_tokens, res.output_tokens, res.cost);
        }
        Err(e) => {
            eprintln!("✗ 聊天失败: {e}");
            eprintln!("  提示: 确保 AI 服务已启动（cargo run -- --headless）");
            exit(1);
        }
    }
    Ok(())
}
