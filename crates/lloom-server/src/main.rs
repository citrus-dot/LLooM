//! LLooM headless server.
//!
//! Starts the Python AI micro-service, Ollama, and the axum REST server
//! (`:7861`). WebUI is served at `/`; all UIs (browser, CLI, TUI) share the
//! same REST contract via `lloom-core`.

use lloom_core::config;
use lloom_core::db;
use lloom_core::server::{self, AppState, build_router};

fn main() {
    let install_dir = config::resolve_install_dir();
    std::env::set_var("LLOOM_INSTALL_DIR", install_dir.to_string_lossy().to_string());

    // Load `.env` into the process environment so models can resolve API keys/bases
    // and subprocesses inherit them.
    config::load_env();

    if let Err(e) = db::init_db() {
        eprintln!("[core] db init failed: {e}");
        std::process::exit(1);
    }

    // One-time (idempotent) import of legacy JSON conversations into SQLite.
    // Files stay in place as a rollback backup; already-imported ids are skipped.
    if let Err(e) = lloom_core::conversations::migrate_json_dir() {
        eprintln!("[core] conversation migration failed (non-fatal): {e}");
    }

    let state = AppState::new();
    let state_for_spawn = state.clone();
    let web_port = config::web_port();
    let router = build_router(state.clone());

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("failed to start tokio runtime");
        rt.block_on(async move {
            println!("[core] starting Python AI service...");
            match lloom_core::processes::start_ai().await {
                Ok(child) => state_for_spawn.children.lock().unwrap().ai = child,
                Err(e) => eprintln!("[core] ⚠ AI service start failed: {e}"),
            }
            println!("[core] starting Ollama...");
            match lloom_core::processes::start_ollama().await {
                Ok(child) => state_for_spawn.children.lock().unwrap().ollama = child,
                Err(e) => eprintln!("[core] ⚠ Ollama start failed: {e}"),
            }

            for _ in 0..30 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if lloom_core::processes::check_ai_health().await.status == "ok" {
                    println!("[core] AI service healthy on :{}", config::ai_port());
                    break;
                }
            }
            let listener = tokio::net::TcpListener::bind(("0.0.0.0", web_port))
                .await
                .unwrap_or_else(|e| {
                    eprintln!("[core] failed to bind port {web_port}: {e}");
                    std::process::exit(1);
                });
            println!("[core] REST server on :{web_port}");

            // Background jobs: daily price calibration + always-on probes
            // (PRICING-PLAN §6.2 / §7). Handles are intentionally not held;
            // they die with the process.
            let _jobs = server::spawn_background_jobs();

            // Graceful shutdown: on SIGINT (Ctrl+C) or SIGTERM (kill/stop-lloom.command),
            // clean up all child processes so no stale AI service / Ollama holds the ports.
            let s = state_for_spawn.clone();
            let shutdown_signal = async move {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {
                        println!("[core] SIGINT received, shutting down...");
                    }
                    _ = sigterm() => {
                        println!("[core] SIGTERM received, shutting down...");
                    }
                }
                server::shutdown_all(&s);
                println!("[core] all services stopped.");
            };

            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown_signal)
                .await
                .ok();
            // Ensure the whole process exits even if background tasks linger.
            std::process::exit(0);
        });
    });

    println!("[server] LLooM running");
    println!("[server] Web UI: http://localhost:{web_port}/");
    println!("[server] Press Ctrl+C to stop");

    std::thread::park();
}

/// Wait for SIGTERM on Unix. On non-Unix, this future never resolves (ctrl_c
/// above still handles the common case).
#[cfg(unix)]
async fn sigterm() {
    use tokio::signal::unix::{signal, SignalKind};
    match signal(SignalKind::terminate()) {
        Ok(mut s) => { s.recv().await; }
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
