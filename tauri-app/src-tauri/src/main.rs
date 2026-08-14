// LLooM v2 — Tauri backend: process management + API proxy + conversation CRUD
//
// v2 changes from v1:
// - Python API server replaces Docker Compose (spawn as child process)
// - Port 7860 replaces port 3002
// - Direct API calls replace CLI JSON commands for model management
// - No Docker dependency

use std::collections::HashMap;
use std::env;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use tauri::{Manager, State};

// ── State ──

struct AppState {
    api_child: Mutex<Option<Child>>,
    ollama_child: Mutex<Option<Child>>,
}

// ── Helpers ──

fn get_install_dir() -> String {
    env::var("LLOOM_INSTALL_DIR")
        .unwrap_or_else(|_| ".".to_string())
}

fn get_api_binary_path() -> Option<String> {
    let install_dir = get_install_dir();
    // Production: resources are under {install_dir}/resources/lloom-server/
    for sub in &["resources/lloom-server", "lloom-server"] {
        let bin = std::path::Path::new(&install_dir)
            .join(sub)
            .join("lloom-server");
        if bin.exists() && bin.is_file() {
            return Some(bin.to_string_lossy().to_string());
        }
    }
    None
}

fn get_ollama_binary_path() -> String {
    let install_dir = get_install_dir();
    for sub in &["resources", ""] {
        let bin = std::path::Path::new(&install_dir).join(sub).join("ollama");
        if bin.exists() && bin.is_file() {
            return bin.to_string_lossy().to_string();
        }
    }
    "ollama".to_string()
}

fn enhanced_path() -> String {
    let current = env::var("PATH").unwrap_or_default();
    let extra = [
        "/usr/local/bin",
        "/opt/homebrew/bin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
        "/Library/Frameworks/Python.framework/Versions/3.11/bin",
        "/Library/Frameworks/Python.framework/Versions/3.10/bin",
    ];
    let mut parts: Vec<&str> = extra.to_vec();
    for p in current.split(':') {
        if !parts.contains(&p) {
            parts.push(p);
        }
    }
    parts.join(":")
}

fn cmd(binary: &str) -> Command {
    let mut c = Command::new(binary);
    c.env("PATH", enhanced_path());
    c.env_remove("PYTHONHOME");
    c.env_remove("PYTHONPATH");
    c
}

fn curl_get(url: &str) -> String {
    let output = cmd("curl")
        .arg("-s")
        .arg("--connect-timeout")
        .arg("3")
        .arg("-m")
        .arg("10")
        .arg(url)
        .output();
    match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(e) => format!("{{\"error\": \"curl failed: {e}\"}}"),
    }
}

fn curl_post(url: &str, body: &str) -> String {
    let output = cmd("curl")
        .arg("-s")
        .arg("--connect-timeout")
        .arg("3")
        .arg("-m")
        .arg("30")
        .arg("-X")
        .arg("POST")
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("-d")
        .arg(body)
        .arg(url)
        .output();
    match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(e) => format!("{{\"error\": \"curl failed: {e}\"}}"),
    }
}

fn curl_delete(url: &str) -> String {
    let output = cmd("curl")
        .arg("-s")
        .arg("--connect-timeout")
        .arg("3")
        .arg("-m")
        .arg("10")
        .arg("-X")
        .arg("DELETE")
        .arg(url)
        .output();
    match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(e) => format!("{{\"error\": \"curl failed: {e}\"}}"),
    }
}

fn curl_post_sse(url: &str, body: &str) -> String {
    let output = cmd("curl")
        .arg("-s")
        .arg("-N")
        .arg("--connect-timeout")
        .arg("5")
        .arg("-m")
        .arg("120")
        .arg("-X")
        .arg("POST")
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("-d")
        .arg(body)
        .arg(url)
        .output();
    match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(e) => format!("{{\"error\": \"curl SSE failed: {e}\"}}"),
    }
}

// ── Data structures ──

#[derive(serde::Serialize)]
struct ServiceStatus {
    name: String,
    status: String,
    healthy: bool,
}

#[derive(serde::Serialize)]
struct SuiteStatus {
    services: Vec<ServiceStatus>,
    total: usize,
    healthy: usize,
    running: usize,
}

#[derive(serde::Serialize)]
struct ConversationMeta {
    id: String,
    title: String,
    updated_at: String,
    message_count: usize,
}

#[derive(serde::Serialize)]
struct SmartRestartResult {
    ok: bool,
    restarted: Vec<String>,
    errors: Vec<String>,
}

// ── Tauri Commands ──

#[tauri::command]
fn get_services_status() -> SuiteStatus {
    let mut services: Vec<ServiceStatus> = Vec::new();

    // API server
    let api_health = curl_get("http://localhost:7860/api/health");
    let api_ok = api_health.contains("\"ok\"");
    services.push(ServiceStatus {
        name: "API Server".to_string(),
        status: if api_ok { "Up (healthy)".to_string() } else { "Down".to_string() },
        healthy: api_ok,
    });

    // Ollama
    let ollama_health = curl_get("http://localhost:11434/api/tags");
    let ollama_ok = ollama_health.contains("\"models\"") || ollama_health.contains("name");
    services.push(ServiceStatus {
        name: "Ollama".to_string(),
        status: if ollama_ok { "Up (healthy)".to_string() } else { "Down".to_string() },
        healthy: ollama_ok,
    });

    let total = services.len();
    let healthy = services.iter().filter(|s| s.healthy).count();
    let running = services.iter().filter(|s| s.status.starts_with("Up")).count();

    SuiteStatus { services, total, healthy, running }
}

#[tauri::command]
fn start_api(state: State<AppState>) -> String {
    let install_dir = get_install_dir();
    let mut c = if let Some(bin) = get_api_binary_path() {
        let mut c = cmd(&bin);
        // Set working dir to the binary's parent (where _internal/ is)
        let bin_dir = std::path::Path::new(&bin)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(install_dir.clone());
        c.current_dir(&bin_dir);
        c
    } else {
        let mut c = cmd("python3");
        c.arg("api/server.py").current_dir(&install_dir);
        c
    };
    c.stdout(Stdio::null()).stderr(Stdio::null());

    match c.spawn() {
        Ok(child) => {
            *state.api_child.lock().unwrap() = Some(child);
            "API server started".to_string()
        }
        Err(e) => format!("Failed to start API: {e}"),
    }
}

#[tauri::command]
fn stop_api(state: State<AppState>) -> String {
    let mut guard = state.api_child.lock().unwrap();
    if let Some(child) = guard.as_mut() {
        let _ = child.kill();
        *guard = None;
        "API server stopped".to_string()
    } else {
        "API server not running".to_string()
    }
}

#[tauri::command]
fn start_ollama(state: State<AppState>) -> String {
    let ollama_bin = get_ollama_binary_path();
    let mut c = cmd(&ollama_bin);
    c.arg("serve")
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    match c.spawn() {
        Ok(child) => {
            *state.ollama_child.lock().unwrap() = Some(child);
            "Ollama started".to_string()
        }
        Err(e) => format!("Failed to start Ollama: {e}"),
    }
}

#[tauri::command]
fn check_ollama() -> bool {
    let result = curl_get("http://localhost:11434/api/tags");
    result.contains("\"models\"") || result.contains("name")
}

#[tauri::command]
fn check_api() -> bool {
    let result = curl_get("http://localhost:7860/api/health");
    result.contains("\"ok\"")
}

// ── .env Config ──

#[tauri::command]
fn read_env() -> HashMap<String, String> {
    let install_dir = get_install_dir();
    let env_path = std::path::Path::new(&install_dir).join(".env");
    let mut result = HashMap::new();
    if let Ok(content) = std::fs::read_to_string(&env_path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                result.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
    }
    result
}

#[tauri::command]
fn write_env(key: String, value: String) -> bool {
    let install_dir = get_install_dir();
    let env_path = std::path::Path::new(&install_dir).join(".env");
    let mut lines = Vec::new();
    let mut found = false;

    if let Ok(content) = std::fs::read_to_string(&env_path) {
        for line in content.lines() {
            if line.trim().starts_with(&format!("{key}=")) || line.trim() == key {
                lines.push(format!("{key}={value}"));
                found = true;
            } else {
                lines.push(line.to_string());
            }
        }
    }
    if !found {
        lines.push(format!("{key}={value}"));
    }
    std::fs::write(&env_path, lines.join("\n") + "\n").is_ok()
}

#[tauri::command]
fn write_env_batch(updates: HashMap<String, String>) -> bool {
    let install_dir = get_install_dir();
    let env_path = std::path::Path::new(&install_dir).join(".env");
    let mut lines = Vec::new();
    let mut existing: HashMap<String, String> = HashMap::new();

    if let Ok(content) = std::fs::read_to_string(&env_path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                lines.push(line.to_string());
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                existing.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }

    existing.extend(updates);
    let mut out: Vec<String> = existing.iter().map(|(k, v)| format!("{k}={v}")).collect();
    out.sort();
    std::fs::write(&env_path, out.join("\n") + "\n").is_ok()
}

// ── API Proxy ──

#[tauri::command]
fn get_usage_stats() -> String {
    curl_get("http://localhost:7860/api/stats")
}

#[tauri::command]
fn get_quota() -> String {
    curl_get("http://localhost:7860/api/budgets")
}

#[tauri::command]
fn get_trends() -> String {
    curl_get("http://localhost:7860/api/usage")
}

#[tauri::command]
fn chat_request(query: String, history: Option<String>) -> String {
    let mut body = format!("{{\"model\":\"auto\",\"messages\":[");
    if let Some(h) = history {
        if let Ok(hist) = serde_json::from_str::<serde_json::Value>(&h) {
            if let Some(arr) = hist.as_array() {
                for (i, msg) in arr.iter().enumerate() {
                    if i > 0 {
                        body.push(',');
                    }
                    body.push_str(&msg.to_string());
                }
            }
        }
    }
    body.push_str(&format!(",{{\"role\":\"user\",\"content\":{}}}]}}",
        serde_json::Value::String(query)));

    let raw = curl_post_sse("http://localhost:7860/api/chat/stream", &body);

    // Parse SSE data lines into JSON array
    let mut events: Vec<serde_json::Value> = Vec::new();
    for line in raw.lines() {
        if line.starts_with("data: ") {
            let json_str = &line[6..];
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                events.push(val);
            }
        }
    }
    serde_json::to_string(&events).unwrap_or_else(|_| "[]".to_string())
}

#[tauri::command]
fn orchestrate_request(query: String, history: Option<String>) -> String {
    let body = if let Some(h) = history {
        format!("{{\"query\":{},\"history\":{}}}",
            serde_json::Value::String(query), h)
    } else {
        format!("{{\"query\":{}}}", serde_json::Value::String(query))
    };

    let raw = curl_post_sse("http://localhost:7860/api/orchestrate/stream", &body);

    let mut events: Vec<serde_json::Value> = Vec::new();
    for line in raw.lines() {
        if line.starts_with("event:") {
            let ev_type = line[7..].trim().to_string();
            events.push(serde_json::json!({"event": ev_type}));
        }
        if line.starts_with("data: ") {
            let json_str = &line[6..];
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                if let Some(last) = events.last_mut() {
                    if last.get("data").is_none() {
                        last["data"] = val;
                        continue;
                    }
                }
                events.push(serde_json::json!({"data": val}));
            }
        }
    }
    serde_json::to_string(&events).unwrap_or_else(|_| "[]".to_string())
}

// ── Model Management (via API) ──

#[tauri::command]
fn get_models() -> String {
    curl_get("http://localhost:7860/api/models")
}

#[tauri::command]
fn add_model(config: String) -> String {
    curl_post("http://localhost:7860/api/models", &config)
}

#[tauri::command]
fn remove_model(model_name: String) -> String {
    let url = format!("http://localhost:7860/api/models/{}", model_name);
    curl_delete(&url)
}

// ── Conversations CRUD ──

fn conversations_dir() -> std::path::PathBuf {
    let install_dir = get_install_dir();
    let dir = std::path::Path::new(&install_dir).join("data").join("conversations");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[tauri::command]
fn list_conversations() -> Vec<ConversationMeta> {
    let dir = conversations_dir();
    let mut convs: Vec<ConversationMeta> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e != "json").unwrap_or(true) {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                    let id = data["id"].as_str().unwrap_or("").to_string();
                    let title = data["title"].as_str().unwrap_or("").to_string();
                    let updated_at = data["updated_at"].as_str().unwrap_or("").to_string();
                    let message_count = data["messages"].as_array().map(|a| a.len()).unwrap_or(0);
                    convs.push(ConversationMeta { id, title, updated_at, message_count });
                }
            }
        }
    }
    convs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    convs
}

#[tauri::command]
fn load_conversation(id: String) -> String {
    let path = conversations_dir().join(format!("{id}.json"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
}

#[tauri::command]
fn save_conversation(conversation_json: String) -> String {
    let data = serde_json::from_str::<serde_json::Value>(&conversation_json);
    match data {
        Ok(val) => {
            let id = val["id"].as_str().unwrap_or("default").to_string();
            let path = conversations_dir().join(format!("{id}.json"));
            match std::fs::write(&path, &conversation_json) {
                Ok(_) => format!("{{\"id\": \"{id}\", \"saved\": true}}"),
                Err(e) => format!("{{\"error\": \"{e}\"}}"),
            }
        }
        Err(e) => format!("{{\"error\": \"invalid JSON: {e}\"}}"),
    }
}

#[tauri::command]
fn delete_conversation(id: String) -> bool {
    let path = conversations_dir().join(format!("{id}.json"));
    std::fs::remove_file(&path).is_ok()
}

// ── Smart Restart ──

#[tauri::command]
fn smart_restart(changed_keys: Vec<String>, state: State<AppState>) -> SmartRestartResult {
    let mut restarted = Vec::new();
    let mut errors = Vec::new();

    // Any key change requires API restart (env_file is read at startup)
    if !changed_keys.is_empty() {
        // Stop existing API
        {
            let mut guard = state.api_child.lock().unwrap();
            if let Some(child) = guard.as_mut() {
                let _ = child.kill();
                *guard = None;
            }
        }

        // Wait briefly
        std::thread::sleep(std::time::Duration::from_secs(1));

        // Restart API
        let install_dir = get_install_dir();
        let mut c = if let Some(bin) = get_api_binary_path() {
            let mut c = cmd(&bin);
            let bin_dir = std::path::Path::new(&bin)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or(install_dir.clone());
            c.current_dir(&bin_dir);
            c
        } else {
            let mut c = cmd("python3");
            c.arg("api/server.py").current_dir(&install_dir);
            c
        };
        c.stdout(Stdio::null()).stderr(Stdio::null());

        match c.spawn() {
            Ok(child) => {
                *state.api_child.lock().unwrap() = Some(child);
                restarted.push("API Server".to_string());
            }
            Err(e) => errors.push(format!("API restart failed: {e}")),
        }

        // Health check (poll up to 30s)
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let health = curl_get("http://localhost:7860/api/health");
            if health.contains("\"ok\"") {
                break;
            }
        }
    }

    SmartRestartResult {
        ok: errors.is_empty(),
        restarted,
        errors,
    }
}

// ── System ──

#[tauri::command]
fn open_web_interface(url: String) {
    let _ = cmd("open").arg(&url).spawn();
}

#[tauri::command]
fn run_cli(args: Vec<String>) -> String {
    let install_dir = get_install_dir();
    let output = cmd("python3")
        .arg("cli/lloom.py")
        .args(&args)
        .current_dir(&install_dir)
        .output();
    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            if !stderr.is_empty() {
                format!("{stdout}\n{stderr}")
            } else {
                stdout
            }
        }
        Err(e) => format!("CLI error: {e}"),
    }
}

// ── First-Run Setup ──

#[tauri::command]
fn first_run_setup() -> String {
    let install_dir = get_install_dir();
    // Check resources/ subdirectory (production) then root (dev)
    let script_path = std::path::Path::new(&install_dir)
        .join("resources")
        .join("first_run_setup.py");
    let (script, work_dir) = if script_path.exists() {
        (script_path.to_string_lossy().to_string(), install_dir.clone())
    } else {
        let dev_path = std::path::Path::new(&install_dir).join("first_run_setup.py");
        if dev_path.exists() {
            (dev_path.to_string_lossy().to_string(), install_dir.clone())
        } else {
            ("scripts/first_run_setup.py".to_string(), install_dir.clone())
        }
    };
    let output = cmd("python3")
        .arg(&script)
        .current_dir(&work_dir)
        .output();
    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            if !stderr.is_empty() {
                format!("{stdout}\n{stderr}")
            } else {
                stdout
            }
        }
        Err(e) => format!("First-run setup error: {e}"),
    }
}

// ── System Tray ──

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

// ── Main ──

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            api_child: Mutex::new(None),
            ollama_child: Mutex::new(None),
        })
        .setup(|app| {
            // In production, set install dir to resource directory
            if let Ok(resource_dir) = app.path().resource_dir() {
                env::set_var("LLOOM_INSTALL_DIR", resource_dir.to_string_lossy().to_string());
            }
            create_tray(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // Kill child processes on close
                let state: State<AppState> = window.app_handle().state();
                let mut api_guard = state.api_child.lock().unwrap();
                if let Some(child) = api_guard.as_mut() {
                    let _ = child.kill();
                }
                let mut ollama_guard = state.ollama_child.lock().unwrap();
                if let Some(child) = ollama_guard.as_mut() {
                    let _ = child.kill();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_services_status,
            start_api,
            stop_api,
            start_ollama,
            check_ollama,
            check_api,
            read_env,
            write_env,
            write_env_batch,
            get_usage_stats,
            get_quota,
            get_trends,
            chat_request,
            orchestrate_request,
            get_models,
            add_model,
            remove_model,
            list_conversations,
            load_conversation,
            save_conversation,
            delete_conversation,
            smart_restart,
            open_web_interface,
            run_cli,
            first_run_setup,
        ])
        .run(tauri::generate_context!())
        .expect("error while running LLooM v2");
}
