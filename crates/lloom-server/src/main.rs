//! LLooM headless server.
//!
//! Starts Ollama and the axum REST server with native Rust provider adapters
//! (`:7861`). WebUI is served at `/`; all UIs (browser, CLI, TUI) share the
//! same REST contract via `lloom-core`.
//!
//! 配置优先级：CLI 参数 > 环境变量（含 `.env` 注入）> 代码默认值。

use clap::Parser;
use lloom_core::config;
use lloom_core::server::{self, build_router, AppState};

#[derive(Parser, Debug)]
#[command(
    name = "lloom-server",
    version,
    about = "LLooM 智能路由网关（REST + WebUI）"
)]
struct Args {
    /// WebUI/REST 监听端口（默认 7861；同 LLOOM_WEB_PORT）
    #[arg(long)]
    web_port: Option<u16>,

    /// 绑定地址（默认 127.0.0.1；同 LLOOM_BIND）
    #[arg(long)]
    bind: Option<String>,

    /// 启动完成后自动在系统默认浏览器打开 WebUI（便携包 start 脚本默认带上）
    #[arg(long)]
    open: bool,
}

fn main() {
    let install_dir = config::resolve_install_dir();
    std::env::set_var(
        "LLOOM_INSTALL_DIR",
        install_dir.to_string_lossy().to_string(),
    );

    // Load `.env` into the process environment so models can resolve API keys/bases
    // and subprocesses inherit them.
    config::load_env();

    // CLI 参数最后解析并注入覆盖层（优先级最高；未给定的项回落 env 链）。
    let args = Args::parse();
    config::init_cli_overrides(config::CliOverrides {
        web_port: args.web_port,
        bind: args.bind,
    });

    // 连接池 + schema 初始化（原 init_db 的职责已并入 Db::new）。
    let state = match AppState::new() {
        Ok(s) => s,
        Err(e) => {
            lloom_core::logging::error("core", &format!("db init failed: {e}"));
            std::process::exit(1);
        }
    };

    // One-time (idempotent) import of legacy JSON conversations into SQLite.
    // Files stay in place as a rollback backup; already-imported ids are skipped.
    if let Err(e) = lloom_core::conversations::migrate_json_dir(&state.db) {
        lloom_core::logging::error(
            "core",
            &format!("conversation migration failed (non-fatal): {e}"),
        );
    }

    let state_for_spawn = state.clone();
    let web_port = config::web_port();
    let open_ui = args.open;
    let router = build_router(state.clone());

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
        rt.block_on(async move {
            lloom_core::logging::info("core", "starting Ollama...");
            match lloom_core::processes::start_ollama().await {
                Ok(child) => state_for_spawn.children.lock().unwrap().ollama = child,
                Err(e) => lloom_core::logging::warn("core", &format!("Ollama start failed: {e}")),
            }

            let bind_addr = config::bind_addr();
            // P0-4 fail-closed：非环回绑定的管理端口要求已配置 API Key，
            // 否则拒绝启动（避免静默暴露无鉴权的管理面）。
            let loopback_bind = bind_addr
                .parse::<std::net::IpAddr>()
                .map(|ip| ip.is_loopback())
                .unwrap_or_else(|_| bind_addr.eq_ignore_ascii_case("localhost"));
            if !loopback_bind && !server::non_loopback_bind_permitted(false, state_for_spawn.db.list_api_keys().map(|k| !k.is_empty()).unwrap_or(false)) {
                lloom_core::logging::error(
                    "core",
                    &format!(
                        "refusing to start: 非环回绑定 {bind_addr} 需要先配置至少一个 API Key（防止管理面无鉴权暴露到局域网）"
                    ),
                );
                server::shutdown_all(&state_for_spawn);
                std::process::exit(1);
            }
            let listener = tokio::net::TcpListener::bind((bind_addr.as_str(), web_port))
                .await
                .unwrap_or_else(|e| {
                    lloom_core::logging::error(
                        "core",
                        &format!("failed to bind {bind_addr}:{web_port}: {e}"),
                    );
                    std::process::exit(1);
                });
            lloom_core::logging::info(
                    "core",
                    &format!("REST server on {bind_addr}:{web_port}"),
                );

            // --open: fire the default browser once the port is live, so the
            // page never races the listener (start.sh / start.bat pass this).
            if open_ui {
                let url = format!("http://localhost:{web_port}/");
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    if !lloom_core::processes::open_browser(&url) {
                        lloom_core::logging::warn("core", &format!("failed to open browser for {url}"));
                    }
                });
            }

            // Background jobs: daily price calibration + always-on probes
            // (PRICING-PLAN §6.2 / §7). Handles are intentionally not held;
            // they die with the process.
            let _jobs = server::spawn_background_jobs(state_for_spawn.db.clone());

            // Graceful shutdown: on SIGINT (Ctrl+C) or SIGTERM (kill/stop-lloom.command),
            // clean up all child processes so no stale AI service / Ollama holds the ports.
            let s = state_for_spawn.clone();
            let shutdown_signal = async move {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {
                        lloom_core::logging::info("core", "SIGINT received, shutting down...");
                    }
                    _ = sigterm() => {
                        lloom_core::logging::info("core", "SIGTERM received, shutting down...");
                    }
                }
                server::shutdown_all(&s);
                lloom_core::logging::info("core", "all services stopped.");
            };

            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(shutdown_signal)
            .await
            .ok();
            // Ensure the whole process exits even if background tasks linger.
            std::process::exit(0);
        });
    });

    lloom_core::logging::info("server", "LLooM running");
    lloom_core::logging::info("server", &format!("Web UI: http://localhost:{web_port}/"));
    lloom_core::logging::info("server", "Press Ctrl+C to stop");

    std::thread::park();
}

/// Wait for SIGTERM on Unix. On non-Unix, this future never resolves (ctrl_c
/// above still handles the common case).
#[cfg(unix)]
async fn sigterm() {
    use tokio::signal::unix::{signal, SignalKind};
    match signal(SignalKind::terminate()) {
        Ok(mut s) => {
            s.recv().await;
        }
        Err(_) => {
            // Fallback: never resolve.
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(not(unix))]
async fn sigterm() {
    std::future::pending::<()>().await;
}
