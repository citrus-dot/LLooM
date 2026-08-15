//! LLooM headless server.
//!
//! Starts the Python AI micro-service, Ollama, and the axum REST server
//! (`:7861`). WebUI is served at `/`; all UIs (browser, CLI, TUI) share the
//! same REST contract via `lloom-core`.

use lloom_core::config;
use lloom_core::db;
use lloom_core::server::{AppState, build_router};

fn main() {
    let install_dir = config::resolve_install_dir();
    std::env::set_var("LLOOM_INSTALL_DIR", install_dir.to_string_lossy().to_string());

    if let Err(e) = db::init_db() {
        eprintln!("[core] db init failed: {e}");
        std::process::exit(1);
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
            axum::serve(listener, router).await.expect("server error");
        });
    });

    println!("[server] LLooM running");
    println!("[server] Web UI: http://localhost:{web_port}/");
    println!("[server] Press Ctrl+C to stop");

    std::thread::park();
}
