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
#[command(
    name = "lloom-cli",
    version,
    about = "LLooM — intelligent LLM routing platform CLI"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show service status
    Status,
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
    /// OpenAI-compatible proxy: show access info / manage API key
    #[command(subcommand)]
    Proxy(ProxyCmd),
    /// RouterBench 冻结矩阵回放（本地离线命令，不需要 lloom-server）
    #[command(subcommand)]
    Bench(BenchCmd),
}

#[derive(Subcommand)]
enum BenchCmd {
    /// 回放：Core 名单 test 实例 × 冻结矩阵 × 真实 plan()，零 LLM 成本（bench.rs）
    Replay {
        /// 策略：all | lloom | always_weak | always_mid | always_strong | cheapest | random | best_single | oracle
        #[arg(long, default_value = "all")]
        strategy: String,
        /// 临时池三元 model_id（weak mid strong；冻结值见 manifests/model_pool_public.yaml）
        #[arg(long, num_args = 3)]
        models: Vec<String>,
        /// 冻结矩阵目录（instances/outcomes jsonl）
        #[arg(long, default_value = "benchmarks/routerbench/normalized")]
        normalized: std::path::PathBuf,
        /// Core 抽样名单（Day 3 冻结）
        #[arg(long, default_value = "benchmarks/routerbench/manifests/core_selection.jsonl")]
        selection: std::path::PathBuf,
        /// frozen manifest（benchmark_id/manifest_hash 溯源）
        #[arg(long, default_value = "benchmarks/routerbench/manifests/frozen_manifest.json")]
        manifest: std::path::PathBuf,
        /// bench.db 落点（决策 P1a：独立库，生产零接触）
        #[arg(long, default_value = "benchmarks/routerbench/bench.db")]
        bench_db: std::path::PathBuf,
        /// E5 实验：LLooM 启用 calibration 质量先验（w/ 版；缺省 w/o）
        #[arg(long, default_value_t = false)]
        calibration: bool,
    },
    /// λ sweep：LLooM cost 权重 0→1 步进 0.1，产出 Pareto 点集（10 号 §十四）
    Sweep {
        #[arg(long, num_args = 3)]
        models: Vec<String>,
        #[arg(long, default_value = "benchmarks/routerbench/normalized")]
        normalized: std::path::PathBuf,
        #[arg(long, default_value = "benchmarks/routerbench/manifests/core_selection.jsonl")]
        selection: std::path::PathBuf,
        #[arg(long, default_value = "benchmarks/routerbench/manifests/frozen_manifest.json")]
        manifest: std::path::PathBuf,
        #[arg(long, default_value = "benchmarks/routerbench/bench.db")]
        bench_db: std::path::PathBuf,
        /// Pareto 点集输出（json）
        #[arg(long, default_value = "benchmarks/routerbench/reports/pareto_points.json")]
        output: std::path::PathBuf,
    },
}

#[derive(Subcommand)]
enum ProxyCmd {
    /// Show proxy Base URL, auth status and model-field notes
    Show,
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
enum ModelsCmd {
    /// List registered models
    List,
    /// Register a new model
    Add {
        /// Model name (e.g. qwen2.5-local)
        name: String,
        /// Model kind: local / cloud. Default: inferred (compat set or provider=ollama -> local)
        #[arg(long)]
        kind: Option<String>,
        /// Local compat protocol: ollama (default) / openai (LM Studio / vLLM)
        #[arg(long)]
        compat: Option<String>,
        /// Cloud provider: dashscope / openai / anthropic / custom
        #[arg(long)]
        provider: Option<String>,
        /// Provider 真实模型 ID（provider_model，如 qwen2.5:latest / gpt-4o）
        #[arg(long)]
        model: Option<String>,
        /// API base URL (default: local 11434 / 1234/v1; cloud optional)
        #[arg(long)]
        api_base: Option<String>,
        /// API key (cloud only): sk-... literal, or env var name like DASHSCOPE_API_KEY
        #[arg(long)]
        api_key: Option<String>,
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
        /// Switch model kind: local / cloud
        #[arg(long)]
        kind: Option<String>,
        /// Local compat protocol: ollama / openai
        #[arg(long)]
        compat: Option<String>,
        /// Cloud provider: dashscope / openai / anthropic / custom
        #[arg(long)]
        provider: Option<String>,
        /// New input cost per token
        #[arg(long)]
        input_cost: Option<f64>,
        /// New output cost per token
        #[arg(long)]
        output_cost: Option<f64>,
        /// New API base URL
        #[arg(long)]
        api_base: Option<String>,
        /// New API key (cloud only): sk-... literal, or env var name
        #[arg(long)]
        api_key: Option<String>,
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
        Command::Models(c) => cmd_models(&client, c).await?,
        Command::Budgets(c) => cmd_budgets(&client, c).await?,
        Command::Usage => cmd_usage(&client).await?,
        Command::Chat {
            query,
            session,
            interactive,
        } => cmd_chat(&client, &query, session.as_deref(), interactive).await?,
        Command::Orchestrate { query } => cmd_orchestrate(&client, &query).await?,
        Command::Conversation(c) => cmd_conversation(&client, c).await?,
        Command::Proxy(c) => cmd_proxy(&client, c).await?,
        Command::Bench(c) => cmd_bench(c).await?,
    }
    Ok(())
}

// ── bench（本地离线命令：读冻结矩阵 + 独立 bench.db，不触碰 lloom-server 与生产库）──

use lloom_core::bench::{self, BenchPool, Strategy};

async fn cmd_bench(cmd: BenchCmd) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        BenchCmd::Replay { .. } => cmd_bench_replay(cmd).await,
        BenchCmd::Sweep { .. } => cmd_bench_sweep(cmd).await,
    }
}

/// Replay/Sweep 共用：读 manifest + 冻结矩阵 + 构造 ReplayCtx。
async fn bench_ctx(
    models: Vec<String>,
    normalized: &std::path::Path,
    selection: &std::path::Path,
    manifest: &std::path::Path,
    calibration: bool,
) -> Result<(String, String, bench::BenchPool, bench::ReplayCtx, Vec<bench::BenchInstance>, bench::FrozenMatrix), Box<dyn std::error::Error>> {
    let frozen: Value = serde_json::from_str(&std::fs::read_to_string(manifest)?)?;
    let benchmark_id = frozen["benchmark_id"].as_str().unwrap_or_default().to_string();
    let manifest_hash = frozen["manifest_hash"].as_str().unwrap_or_default().to_string();

    if models.len() != 3 {
        eprintln!("✗ --models 需恰好 3 个 model_id（weak mid strong），顺序敏感");
        std::process::exit(1);
    }
    let pool = BenchPool {
        weak: bench::slugify(&models[0]),
        mid: bench::slugify(&models[1]),
        strong: bench::slugify(&models[2]),
    };

    let (test, calib, matrix) = bench::load_frozen(normalized, Some(selection))?;
    let declared = frozen["core"]["total"].as_u64().unwrap_or(0) as usize;
    if declared > 0 && test.len() != declared {
        eprintln!("✗ Core 数量不符：名单声明 {declared}，实载 {}（Gate 1 前置失败）", test.len());
        std::process::exit(1);
    }

    let stats = bench::calibration_stats(&matrix, &pool, &calib);
    let (pool_models, specs) = bench::build_pool_models(&pool, &stats);
    let mut ctx = bench::ReplayCtx::new(pool.clone(), pool_models, specs, stats);
    ctx.use_calibration = calibration;
    Ok((benchmark_id, manifest_hash, pool, ctx, test, matrix))
}

async fn cmd_bench_replay(cmd: BenchCmd) -> Result<(), Box<dyn std::error::Error>> {
    let BenchCmd::Replay {
        strategy,
        models,
        normalized,
        selection,
        manifest,
        bench_db,
        calibration,
    } = cmd
    else {
        unreachable!()
    };
    let (benchmark_id, manifest_hash, pool, ctx, test, matrix) =
        bench_ctx(models, &normalized, &selection, &manifest, calibration).await?;

    let strategies: Vec<Strategy> = if strategy == "all" {
        Strategy::ALL.to_vec()
    } else {
        vec![Strategy::parse(&strategy).unwrap_or_else(|| {
            eprintln!("✗ 未知策略 '{strategy}'（可选 all/lloom/always_*/cheapest/random/best_single/oracle）");
            std::process::exit(1);
        })]
    };

    let conn = bench::open_bench_db(&bench_db)?;
    println!(
        "bench replay: {} ({}) | n={} test | pool={}/{}/{} | calibration={}",
        benchmark_id,
        &manifest_hash[..12.min(manifest_hash.len())],
        test.len(),
        pool.weak,
        pool.mid,
        pool.strong,
        if calibration { "on" } else { "off" }
    );

    let mut per_strategy = serde_json::Map::new();
    let mut strong_ref: Option<(f64, f64)> = None; // (Q, C) Always-Strong
    let mut weak_ref: Option<f64> = None; // Q Always-Weak
    for st in strategies {
        let items = bench::replay(&ctx, &matrix, &test, st)?;
        let summary = bench::summarize(&items);
        if st == Strategy::AlwaysStrong {
            strong_ref = Some((
                summary["quality_mean"].as_f64().unwrap_or(0.0),
                summary["cost_mean"].as_f64().unwrap_or(0.0),
            ));
        }
        if st == Strategy::AlwaysWeak {
            weak_ref = Some(summary["quality_mean"].as_f64().unwrap_or(0.0));
        }
        let parameter_json = serde_json::json!({
            "pool": { "weak": pool.weak, "mid": pool.mid, "strong": pool.strong },
            "est_profile": { "est_in": "chars*0.6", "est_out_cold_start": bench::EST_OUT_COLD_START },
            "hit_rate": "all-zero (红线)",
            "quality_override": calibration,
            "quality_score_source": "calibration global mean (注册冷启动先验)",
            "policy": { "min_capability_tier": 1, "weights": "cost .5 / quality .4 / latency .1 (default)" },
            "cost_basis": "source (等效单价由 calibration 导出，公式见 bench.rs)"
        })
        .to_string();
        let run_id = bench::save_run(
            &conn,
            &bench::RunMeta {
                benchmark_id: &benchmark_id,
                manifest_hash: &manifest_hash,
                pool_id: "public_matrix_top3_v1",
                strategy: st.name(),
                parameter_json: &parameter_json,
                split: "test",
                summary_json: summary.to_string().as_str(),
            },
            &items,
        )?;
        per_strategy.insert(st.name().to_string(), summary);
        let s = &per_strategy[st.name()];
        let dist_str = format!("{:?}", s["selection_dist"]);
        println!(
            "  {:<14} run={:<4} Q={:.4} C=${:.6}/q regret_p50={:.2} dist={}",
            st.name(),
            run_id,
            s["quality_mean"].as_f64().unwrap_or(0.0),
            s["cost_mean"].as_f64().unwrap_or(0.0),
            s["regret_p50"].as_f64().unwrap_or(0.0),
            dist_str,
        );
    }

    // 对比指标（10 号 §13.3-13.5）：以 Always-Strong/Always-Weak 基准行计算 LLooM 三指标
    if let (Some((qs, cs)), Some(qw)) = (strong_ref, weak_ref) {
        if let Some(lloom) = per_strategy.get("lloom") {
            let ql = lloom["quality_mean"].as_f64().unwrap_or(0.0);
            let cl = lloom["cost_mean"].as_f64().unwrap_or(0.0);
            let saving = if cs > 0.0 { 1.0 - cl / cs } else { 0.0 };
            let retention = if qs > 0.0 { ql / qs } else { 0.0 };
            let gap = if qs - qw > 1e-12 { (ql - qw) / (qs - qw) } else { 0.0 };
            println!(
                "  ── LLooM vs 基准（{} calibration）──\n  saving={:.1}% retention={:.1}% gap_recovery={:.1}%",
                if calibration { "w/" } else { "w/o" },
                saving * 100.0,
                retention * 100.0,
                gap * 100.0
            );
        }
    }
    Ok(())
}

async fn cmd_bench_sweep(cmd: BenchCmd) -> Result<(), Box<dyn std::error::Error>> {
    let BenchCmd::Sweep {
        models,
        normalized,
        selection,
        manifest,
        bench_db,
        output,
    } = cmd
    else {
        unreachable!()
    };
    let (benchmark_id, manifest_hash, pool, mut ctx, test, matrix) =
        bench_ctx(models, &normalized, &selection, &manifest, false).await?;

    let conn = bench::open_bench_db(&bench_db)?;
    println!("bench sweep: {} | n={} test | λ∈{{0,0.1,…,0.9}}（cost_weight；quality_weight=0.9−λ，latency 恒 0.1）", benchmark_id, test.len());

    let mut points = Vec::new();
    for step in 0..=9u32 {
        let cw = step as f64 / 10.0;
        let qw = 0.9 - cw;
        ctx.weights = Some((cw, qw));
        let items = bench::replay(&ctx, &matrix, &test, Strategy::Lloom)?;
        let summary = bench::summarize(&items);
        let parameter_json = serde_json::json!({
            "pool": { "weak": pool.weak, "mid": pool.mid, "strong": pool.strong },
            "est_profile": { "est_in": "chars*0.6", "est_out_cold_start": bench::EST_OUT_COLD_START },
            "hit_rate": "all-zero (红线)",
            "quality_override": false,
            "quality_score_source": "calibration global mean (注册冷启动先验)",
            "policy": { "min_capability_tier": 1, "cost_weight": cw, "quality_weight": qw, "latency_weight": 0.1 },
            "cost_basis": "source (等效单价由 calibration 导出)"
        })
        .to_string();
        let run_id = bench::save_run(
            &conn,
            &bench::RunMeta {
                benchmark_id: &benchmark_id,
                manifest_hash: &manifest_hash,
                pool_id: "public_matrix_top3_v1",
                strategy: "lloom",
                parameter_json: &parameter_json,
                split: "test",
                summary_json: summary.to_string().as_str(),
            },
            &items,
        )?;
        let q = summary["quality_mean"].as_f64().unwrap_or(0.0);
        let c = summary["cost_mean"].as_f64().unwrap_or(0.0);
        println!("  λ={cw:.1} run={run_id:<4} Q={q:.4} C=${c:.6}/q");
        points.push(serde_json::json!({
            "run_id": run_id, "cost_weight": cw, "quality_weight": qw,
            "quality_mean": q, "cost_mean": c,
            "selection_dist": summary["selection_dist"],
        }));
    }

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let report = serde_json::json!({
        "benchmark_id": benchmark_id,
        "manifest_hash": manifest_hash,
        "pool": { "weak": pool.weak, "mid": pool.mid, "strong": pool.strong },
        "split": "test",
        "note": "λ sweep Pareto 点集（10 号 §十四）；latency 权重恒 0.1，cost+quality=0.9 与生产 default 形态一致",
        "points": points,
    });
    std::fs::write(&output, serde_json::to_string_pretty(&report)? + "\n")?;
    println!("✓ pareto points → {}", output.display());
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

async fn post(
    client: &Client,
    path: &str,
    body: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let res = client
        .post(format!("{BASE}{path}"))
        .json(&body)
        .send()
        .await?;
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
                    let kind = m["kind"].as_str().unwrap_or("cloud");
                    let backend = if kind == "local" {
                        format!("本地:{}", m["compat"].as_str().unwrap_or("ollama"))
                    } else {
                        format!("云端:{}", m["provider"].as_str().unwrap_or("?"))
                    };
                    println!(
                        "  {:<18} {:<16} {:<40} in=${:.6}/tok out=${:.6}/tok {}",
                        m["name"].as_str().unwrap_or(""),
                        backend,
                        m["provider_model"].as_str().unwrap_or(""),
                        m["input_cost_per_token"].as_f64().unwrap_or(0.0),
                        m["output_cost_per_token"].as_f64().unwrap_or(0.0),
                        if m["task_type"].as_str().unwrap_or("").is_empty() {
                            ""
                        } else {
                            m["task_type"].as_str().unwrap_or("")
                        },
                    );
                }
            }
            println!("共 {} 个模型", models.len());
        }
        ModelsCmd::Add {
            name,
            kind,
            compat,
            provider,
            model,
            api_base,
            api_key,
            input_cost,
            output_cost,
            task_type,
        } => {
            let is_local = match kind.as_deref() {
                Some("local") => true,
                Some("cloud") => false,
                Some(other) => {
                    eprintln!("✗ kind 必须是 local|cloud，收到 '{other}'");
                    exit(1);
                }
                None => compat.is_some() || provider.as_deref() == Some("ollama"),
            };
            if is_local && api_key.is_some() {
                eprintln!("✗ 本地模型不配置 API Key");
                exit(1);
            }
            let mut body = serde_json::json!({
                "name": name,
                "kind": if is_local { "local" } else { "cloud" },
                "task_type": task_type.unwrap_or_else(|| "general".into()),
                "input_cost_per_token": input_cost.unwrap_or(0.0),
                "output_cost_per_token": output_cost.unwrap_or(0.0),
                "rpm": 60,
            });
            if let Some(v) = compat {
                body["compat"] = serde_json::json!(v);
            }
            if let Some(v) = provider {
                if !is_local {
                    body["provider"] = serde_json::json!(v);
                }
            }
            if let Some(v) = api_base {
                body["api_base"] = serde_json::json!(v);
            }
            if let Some(v) = api_key {
                if !is_local {
                    body["api_key"] = serde_json::json!(v);
                }
            }
            if let Some(v) = model {
                body["provider_model"] = serde_json::json!(v);
            }
            let r = post(client, "/api/models", body).await?;
            println!("✓ 模型已注册 (id={}, name={})", r["id"], r["name"]);
        }
        ModelsCmd::Update {
            name,
            kind,
            compat,
            provider,
            input_cost,
            output_cost,
            api_base,
            api_key,
            task_type,
        } => {
            let mut updates = serde_json::Map::new();
            if let Some(v) = kind {
                updates.insert("kind".into(), serde_json::json!(v));
            }
            if let Some(v) = compat {
                updates.insert("compat".into(), serde_json::json!(v));
            }
            if let Some(v) = provider {
                updates.insert("provider".into(), serde_json::json!(v));
            }
            if let Some(v) = input_cost {
                updates.insert("input_cost_per_token".into(), serde_json::json!(v));
            }
            if let Some(v) = output_cost {
                updates.insert("output_cost_per_token".into(), serde_json::json!(v));
            }
            if let Some(v) = api_base {
                updates.insert("api_base".into(), serde_json::json!(v));
            }
            if let Some(v) = api_key {
                updates.insert("api_key".into(), serde_json::json!(v));
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
        BudgetsCmd::Set {
            scope,
            scope_id,
            max_budget,
            duration,
        } => {
            let r = post(client, "/api/budgets", serde_json::json!({
                "scope": scope, "scope_id": scope_id, "max_budget": max_budget, "duration": duration,
            })).await?;
            if r["set"].as_bool().unwrap_or(false) {
                println!("✓ 预算已设置: {scope}/{scope_id} = ${max_budget:.2} / {duration}");
            }
        }
        BudgetsCmd::Check { scope, scope_id } => {
            let r: Value = get(
                client,
                &format!(
                    "/api/budgets/check?scope={}&scope_id={}",
                    urlencode(&scope),
                    urlencode(&scope_id)
                ),
            )
            .await?;
            let spent = r["spent"].as_f64().unwrap_or(0.0);
            let max = r["budget"]["max_budget"].as_f64();
            match max {
                Some(m) => {
                    let within = r["within_budget"].as_bool().unwrap_or(false);
                    println!("  预算: ${:.2} / ${:.2} (已用 ${:.2})", spent, m, spent);
                    println!(
                        "  状态: {}",
                        if within {
                            "✓ 在预算内"
                        } else {
                            "✗ 超出预算"
                        }
                    );
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
    println!(
        "累计花费: ${:.6}",
        stats["total_spend"].as_f64().unwrap_or(0.0)
    );
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
async fn resolve_session_id(
    client: &Client,
    input: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // Fast path: assume it's an ID and see if the conversation exists.
    if let Ok(conv) = get(client, &format!("/api/conversations/{input}")).await {
        if conv.get("id").is_some() || conv.get("messages").is_some() {
            return Ok(input.to_string());
        }
    }
    // Title match against the conversation list.
    let data: Value = get(client, "/api/conversations").await?;
    let convs = data["conversations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let lower = input.to_lowercase();
    for c in &convs {
        let title = c["title"].as_str().unwrap_or("").to_lowercase();
        if title.contains(&lower) {
            return Ok(c["id"].as_str().unwrap_or("").to_string());
        }
    }
    Err(format!("找不到会话: {input}（先用 lloom-cli conversation list 查看）").into())
}

async fn cmd_chat(
    client: &Client,
    query: &str,
    session: Option<&str>,
    interactive: bool,
) -> Result<(), Box<dyn std::error::Error>> {
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
        if input.is_empty()
            || input.eq_ignore_ascii_case("exit")
            || input.eq_ignore_ascii_case("quit")
        {
            return Ok(());
        }
        history.push(serde_json::json!({ "role": "user", "content": input }));
    }
}

/// POST /api/chat/stream, printing tokens as they arrive; returns the full reply.
async fn stream_chat(
    client: &Client,
    messages: &[Value],
) -> Result<String, Box<dyn std::error::Error>> {
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

async fn cmd_conversation(
    client: &Client,
    cmd: ConversationCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ConversationCmd::List => {
            let data: Value = get(client, "/api/conversations").await?;
            let convs = data["conversations"]
                .as_array()
                .cloned()
                .unwrap_or_default();
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
            println!(
                "{}",
                if r["deleted"].as_bool().unwrap_or(false) {
                    "已删除"
                } else {
                    "删除失败"
                }
            );
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
            println!(
                "{}",
                if r["renamed"].as_bool().unwrap_or(false) {
                    "已重命名"
                } else {
                    "重命名失败"
                }
            );
        }
        ConversationCmd::New => {
            let r = post(
                client,
                "/api/conversations",
                serde_json::json!({ "messages": [] }),
            )
            .await?;
            println!("新建会话: {}", r["id"].as_str().unwrap_or(""));
        }
    }
    Ok(())
}

// ── OpenAI 兼容代理（N1 接入向导） ──

async fn cmd_proxy(client: &Client, cmd: ProxyCmd) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ProxyCmd::Show => print_proxy_info(&get(client, "/api/proxy/config").await?),
    }
    Ok(())
}

fn print_proxy_info(cfg: &Value) {
    let base = cfg["base_url"].as_str().unwrap_or("");
    let bind = cfg["bind"].as_str().unwrap_or("");
    let auth = cfg["auth_enabled"].as_bool().unwrap_or(false);
    let key_count = cfg["key_count"].as_i64().unwrap_or(0);
    println!("OpenAI 兼容代理接入");
    println!("  Base URL : {base}");
    if bind != "127.0.0.1" && bind != "localhost" {
        println!("  绑定     : {bind}（局域网可达——远程接入把 127.0.0.1 换成本机 IP）");
    }
    println!(
        "  鉴权     : {}",
        if auth {
            format!("启用（{key_count} 个 API Key）")
        } else {
            "未鉴权（任何能访问该端口的程序都可调用；仅环回绑定时可接受）".to_string()
        }
    );
    println!();
    println!("  接入示例（任意 OpenAI 客户端）:");
    println!("    Base URL = {base}");
    println!("    API Key  = 上述鉴权 token（未鉴权时填任意非空串）");
    println!();
    println!("  model 字段:");
    println!("    auto        智能路由（按任务/成本/质量/预算自动选模型，推荐）");
    println!("    注册模型名  直连该模型，跳过路由（GET /v1/models 可列出）");
    println!("    未知名      自动回落 auto，不报错");
    println!();
    println!("  限制: 流式为整段下发；不支持 tools/多模态；编排分解不在本通道。");
    println!("  管理: 打开 WebUI 的 API Keys 页面。");
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

// ── helpers ──

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
