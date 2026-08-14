//! LLooM v2 — entry point.
//!
//! Two run modes:
//!   `lloom`                → GUI (Tauri desktop window + tray)
//!   `lloom --headless`     → headless axum server (browser UI)
//!
//! In both modes the Rust axum server (`:7861`) is the single REST contract.
//! The GUI frontend talks to the same REST API as the WebUI — the Tauri shell
//! only provides the desktop window and system tray.

use lloom::config;
use lloom::db;
use lloom::server::{AppState, build_router};
use tauri::Manager;

fn is_headless() -> bool {
    std::env::args().any(|a| a == "--headless" || a == "-H")
}

fn main() {
    if is_headless() {
        run_headless();
    } else {
        run_gui();
    }
}

/// Start the Python AI micro-service, Ollama, and the axum REST server.
/// The server runs in a background tokio runtime; returns the AppState so the
/// caller can manage child processes.
fn start_core() -> AppState {
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
            match lloom::processes::start_ai().await {
                Ok(child) => state_for_spawn.children.lock().unwrap().ai = child,
                Err(e) => eprintln!("[core] ⚠ AI service start failed: {e}"),
            }
            println!("[core] starting Ollama...");
            match lloom::processes::start_ollama().await {
                Ok(child) => state_for_spawn.children.lock().unwrap().ollama = child,
                Err(e) => eprintln!("[core] ⚠ Ollama start failed: {e}"),
            }

            for _ in 0..30 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if lloom::processes::check_ai_health().await.status == "ok" {
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

    state
}

fn run_headless() {
    println!("[headless] LLooM v2 running without GUI");
    start_core();
    println!("[headless] Web UI: http://localhost:{}/", config::web_port());
    println!("[headless] Press Ctrl+C to stop");

    // Park the main thread; child processes and the server run in background.
    std::thread::park();
}

fn run_gui() {
    let state = start_core();

    tauri::Builder::default()
        .manage(state.clone())
        .setup(|app| {
            create_tray(app.handle());
            Ok(())
        })
        .on_window_event(move |_window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let children = state.children.clone();
                let mut guard = children.lock().unwrap();
                if let Some(child) = guard.ai.as_mut() {
                    let _ = child.kill();
                }
                if let Some(child) = guard.ollama.as_mut() {
                    let _ = child.kill();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running LLooM v2");
}

// ── System tray ──

fn create_tray(app: &tauri::AppHandle) {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)
        .expect("failed to create show menu item");
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)
        .expect("failed to create quit menu item");
    let menu = Menu::with_items(app, &[&show, &quit]);

    if let Ok(menu) = menu {
        let _ = TrayIconBuilder::with_id("main")
            .icon(app.default_window_icon().unwrap().clone())
            .menu(&menu)
            .on_menu_event(|app, event| {
                match event.id().as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                }
            })
            .build(app);
    }
}