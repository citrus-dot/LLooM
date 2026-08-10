// LiteLLM Suite — Tauri 桌面应用后端
// 提供 Docker 服务管理、系统托盘、本地配置等功能

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::process::Command;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};

// ==================================================
// 安装目录探测
// ==================================================

fn get_install_dir() -> String {
    if let Ok(dir) = std::env::var("LITELLM_INSTALL_DIR") {
        if !dir.is_empty() && std::path::Path::new(&dir).exists() {
            return dir;
        }
    }
    let default = "/Users/orange/litellm-install".to_string();
    if std::path::Path::new(&default).exists() {
        return default;
    }
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or(default)
}

/// macOS GUI 应用从 Finder/Dock 启动时 PATH 极其精简，
/// 不包含 /usr/local/bin 或 /opt/homebrew/bin，导致找不到 docker/curl/python3。
/// 此函数将常见二进制目录前置到 PATH。
fn enhanced_path() -> String {
    let current = std::env::var("PATH").unwrap_or_default();
    let extra = [
        "/usr/local/bin",
        "/opt/homebrew/bin",
        "/opt/homebrew/sbin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
        "/Applications/Docker.app/Contents/Resources/bin",
        "/Library/Frameworks/Python.framework/Versions/3.12/bin",
        "/Library/Frameworks/Python.framework/Versions/3.11/bin",
    ];
    let mut parts: Vec<&str> = extra.to_vec();
    parts.extend(current.split(':'));
    parts.join(":")
}

/// 创建已配置 PATH 的 Command
/// 同时清除 TRAE 注入的 PYTHONHOME/PYTHONPATH，防止系统 Python 崩溃
fn cmd(binary: &str) -> Command {
    let mut c = Command::new(binary);
    c.env("PATH", enhanced_path());
    c.env_remove("PYTHONHOME");
    c.env_remove("PYTHONPATH");
    c
}

// ==================================================
// 数据结构
// ==================================================

#[derive(Serialize, Deserialize)]
struct ServiceStatus {
    name: String,
    status: String,
    healthy: bool,
}

#[derive(Serialize, Deserialize)]
struct SuiteStatus {
    services: Vec<ServiceStatus>,
    total: usize,
    healthy: usize,
    running: bool,
}

// ==================================================
// Docker 服务管理
// ==================================================

#[tauri::command]
fn get_services_status() -> SuiteStatus {
    let dir = get_install_dir();
    let output = cmd("docker")
        .args(["compose", "ps", "--format", "{{.Name}}\t{{.Status}}"])
        .current_dir(&dir)
        .output();

    let mut services = Vec::new();
    if let Ok(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                services.push(ServiceStatus {
                    name: parts[0].to_string(),
                    status: parts[1].to_string(),
                    healthy: parts[1].contains("healthy"),
                });
            }
        }
    }
    let total = services.len();
    let healthy = services.iter().filter(|s| s.healthy).count();
    SuiteStatus { services, total, healthy, running: total > 0 }
}

#[tauri::command]
fn start_services() -> Result<String, String> {
    let dir = get_install_dir();
    let output = cmd("docker")
        .args(["compose", "up", "-d"])
        .current_dir(&dir)
        .output()
        .map_err(|e| format!("启动失败: {}", e))?;
    if output.status.success() { Ok("服务已启动".to_string()) }
    else { Err(String::from_utf8_lossy(&output.stderr).to_string()) }
}

#[tauri::command]
fn stop_services() -> Result<String, String> {
    let dir = get_install_dir();
    let output = cmd("docker")
        .args(["compose", "down"])
        .current_dir(&dir)
        .output()
        .map_err(|e| format!("停止失败: {}", e))?;
    if output.status.success() { Ok("服务已停止".to_string()) }
    else { Err(String::from_utf8_lossy(&output.stderr).to_string()) }
}

#[tauri::command]
fn restart_service(service_name: String) -> Result<String, String> {
    let dir = get_install_dir();
    let output = cmd("docker")
        .args(["compose", "restart", &service_name])
        .current_dir(&dir)
        .output()
        .map_err(|e| format!("重启失败: {}", e))?;
    if output.status.success() { Ok(format!("{} 已重启", service_name)) }
    else { Err(String::from_utf8_lossy(&output.stderr).to_string()) }
}

// ==================================================
// .env 配置管理
// ==================================================

#[tauri::command]
fn read_env() -> Result<HashMap<String, String>, String> {
    let dir = get_install_dir();
    let env_path = format!("{}/.env", dir);
    let content = fs::read_to_string(&env_path)
        .map_err(|e| format!("读取 .env 失败: {}", e))?;
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        if let Some(eq) = line.find('=') {
            map.insert(line[..eq].trim().to_string(), line[eq + 1..].trim().to_string());
        }
    }
    Ok(map)
}

#[tauri::command]
fn write_env(key: String, value: String) -> Result<(), String> {
    let dir = get_install_dir();
    let env_path = format!("{}/.env", dir);
    let content = fs::read_to_string(&env_path)
        .map_err(|e| format!("读取 .env 失败: {}", e))?;
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let prefix = format!("{}=", key);
    let mut found = false;
    for line in &mut lines {
        if line.starts_with(&prefix) {
            *line = format!("{}={}", key, value);
            found = true;
            break;
        }
    }
    if !found { lines.push(format!("{}={}", key, value)); }
    fs::write(&env_path, lines.join("\n") + "\n")
        .map_err(|e| format!("写入 .env 失败: {}", e))?;
    Ok(())
}

#[tauri::command]
fn write_env_batch(updates: HashMap<String, String>) -> Result<(), String> {
    let dir = get_install_dir();
    let env_path = format!("{}/.env", dir);
    let content = fs::read_to_string(&env_path)
        .map_err(|e| format!("读取 .env 失败: {}", e))?;
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    for (key, value) in &updates {
        let prefix = format!("{}=", key);
        let mut found = false;
        for line in &mut lines {
            if line.starts_with(&prefix) {
                *line = format!("{}={}", key, value);
                found = true;
                break;
            }
        }
        if !found { lines.push(format!("{}={}", key, value)); }
    }
    fs::write(&env_path, lines.join("\n") + "\n")
        .map_err(|e| format!("写入 .env 失败: {}", e))?;
    Ok(())
}

// ==================================================
// 系统检查 & 工具
// ==================================================

#[tauri::command]
fn get_usage_stats() -> Result<String, String> {
    let output = cmd("curl")
        .args(["-sf", "--max-time", "10", "http://localhost:3002/api/stats"])
        .output()
        .map_err(|e| format!("请求失败: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err("无法获取用量数据，请确保编排平台服务正在运行".to_string())
    }
}

#[tauri::command]
fn get_quota() -> Result<String, String> {
    let output = cmd("curl")
        .args(["-sf", "--max-time", "10", "http://localhost:3002/api/quota"])
        .output()
        .map_err(|e| format!("请求失败: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err("无法获取配额数据".to_string())
    }
}

#[tauri::command]
fn get_trends() -> Result<String, String> {
    let output = cmd("curl")
        .args(["-sf", "--max-time", "10", "http://localhost:3002/api/trends"])
        .output()
        .map_err(|e| format!("请求失败: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err("无法获取趋势数据".to_string())
    }
}

#[tauri::command]
fn get_service_logs(service_name: String) -> Result<String, String> {
    let output = cmd("curl")
        .args(["-sf", "--max-time", "15", &format!("http://localhost:3002/api/service-logs/{}", service_name)])
        .output()
        .map_err(|e| format!("请求失败: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err("无法获取服务日志".to_string())
    }
}

#[tauri::command]
fn update_user_quota(max_budget: f64) -> Result<String, String> {
    let body = format!(r#"{{"max_budget":{}}}"#, max_budget);
    let output = cmd("curl")
        .args(["-sf", "-X", "POST", "-H", "Content-Type: application/json",
               "-d", &body, "http://localhost:3002/api/quota/user"])
        .output()
        .map_err(|e| format!("请求失败: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err("更新用户配额失败".to_string())
    }
}

#[tauri::command]
fn update_key_quota(key_alias: String, max_budget: f64) -> Result<String, String> {
    let body = format!(r#"{{"key_alias":"{}","max_budget":{}}}"#, key_alias, max_budget);
    let output = cmd("curl")
        .args(["-sf", "-X", "POST", "-H", "Content-Type: application/json",
               "-d", &body, "http://localhost:3002/api/quota/key"])
        .output()
        .map_err(|e| format!("请求失败: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err("更新密钥配额失败".to_string())
    }
}

#[tauri::command]
fn open_web_interface(url: String) -> Result<(), String> {
    cmd("open").arg(&url).spawn()
        .map_err(|e| format!("打开失败: {}", e))?;
    Ok(())
}

#[tauri::command]
fn run_cli(args: Vec<String>) -> Result<String, String> {
    let dir = get_install_dir();
    let mut c = cmd("python3");
    c.arg("litellm_cli.py").current_dir(&dir);
    for arg in &args { c.arg(arg); }
    let output = c.output().map_err(|e| format!("命令执行失败: {}", e))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if output.status.success() { Ok(stdout) }
    else { Err(format!("{}\n{}", stdout, stderr)) }
}

#[tauri::command]
fn check_docker() -> bool {
    cmd("docker").arg("--version").output()
        .map(|o| o.status.success()).unwrap_or(false)
}

#[tauri::command]
fn check_ollama() -> bool {
    cmd("curl").args(["-sf", "http://localhost:11434/api/tags"]).output()
        .map(|o| o.status.success()).unwrap_or(false)
}

/// 代理 SSE 聊天请求 — Tauri webview 无法直接连接 http://localhost:3002（混合内容拦截），
/// 通过 Rust 后端发起请求，解析 SSE 事件，返回结构化结果。
/// 支持 POST 方式发送对话历史。
#[tauri::command]
fn chat_request(query: String, history: Option<Vec<HashMap<String, String>>>) -> Result<String, String> {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;

    let url = "http://localhost:3002/api/chat/stream";

    // 构建 POST body（包含 query 和 history）
    let history_arr: Vec<serde_json::Value> = history.unwrap_or_default().iter().map(|m| {
        serde_json::json!({
            "role": m.get("role").unwrap_or(&"user".to_string()),
            "content": m.get("content").unwrap_or(&"".to_string()),
        })
    }).collect();

    let body = serde_json::json!({
        "q": query,
        "history": history_arr,
    }).to_string();

    // 写入临时文件供 curl --data-binary @- 读取
    let mut curl = cmd("curl")
        .args(["-sf", "-N", "--max-time", "120", "-X", "POST",
               "-H", "Content-Type: application/json",
               "-d", &body,
               url])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("请求失败: {}", e))?;

    let stdout = curl.stdout.take().ok_or("无法读取响应")?;
    let reader = BufReader::new(stdout);

    let mut current_event = String::new();
    let mut decompose_data = String::new();
    let mut tasks: Vec<serde_json::Value> = Vec::new();
    let mut result_data = String::new();

    for line in reader.lines() {
        let line = line.map_err(|e| format!("读取失败: {}", e))?;
        if line.starts_with("event: ") {
            current_event = line[7..].to_string();
        } else if line.starts_with("data: ") {
            let data = &line[6..];
            match current_event.as_str() {
                "decompose" => decompose_data = data.to_string(),
                "task_start" => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                        tasks.push(v);
                    }
                }
                "task_done" => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(id) = v.get("id").and_then(|i| i.as_u64()) {
                            for t in tasks.iter_mut() {
                                if t.get("id").and_then(|i| i.as_u64()) == Some(id) {
                                    if let Ok(done) = serde_json::from_str::<serde_json::Value>(data) {
                                        *t = done;
                                    }
                                }
                            }
                        }
                    }
                }
                "result" => result_data = data.to_string(),
                _ => {}
            }
        }
    }

    let _ = curl.wait();

    if result_data.is_empty() {
        return Err("未收到响应结果，请检查编排服务是否正常".to_string());
    }

    let result: serde_json::Value = serde_json::from_str(&result_data)
        .map_err(|e| format!("解析结果失败: {}", e))?;

    let decompose: Option<serde_json::Value> = if !decompose_data.is_empty() {
        serde_json::from_str(&decompose_data).ok()
    } else {
        None
    };

    let response = serde_json::json!({
        "decompose": decompose,
        "tasks": tasks,
        "result": result,
    });

    serde_json::to_string(&response).map_err(|e| format!("序列化失败: {}", e))
}

// ==================================================
// 对话历史管理
// ==================================================

#[derive(Serialize, Deserialize)]
struct ConversationMeta {
    id: String,
    title: String,
    updated_at: f64,
    message_count: usize,
}

fn conversations_dir() -> String {
    format!("{}/conversations", get_install_dir())
}

#[tauri::command]
fn list_conversations() -> Result<Vec<ConversationMeta>, String> {
    let dir = conversations_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {}", e))?;

    let mut conversations = Vec::new();
    let entries = fs::read_dir(&dir).map_err(|e| format!("读取目录失败: {}", e))?;
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else { continue };
        let Ok(conv) = serde_json::from_str::<serde_json::Value>(&content) else { continue };
        conversations.push(ConversationMeta {
            id: conv.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            title: conv.get("title").and_then(|v| v.as_str()).unwrap_or("无标题").to_string(),
            updated_at: conv.get("updated_at").and_then(|v| v.as_f64()).unwrap_or(0.0),
            message_count: conv.get("messages").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
        });
    }
    conversations.sort_by(|a, b| b.updated_at.partial_cmp(&a.updated_at).unwrap_or(std::cmp::Ordering::Equal));
    Ok(conversations)
}

#[tauri::command]
fn load_conversation(id: String) -> Result<String, String> {
    let path = format!("{}/{}.json", conversations_dir(), id);
    let content = fs::read_to_string(&path).map_err(|e| format!("读取对话失败: {}", e))?;
    Ok(content)
}

#[tauri::command]
fn save_conversation(conversation_json: String) -> Result<(), String> {
    let conv: serde_json::Value = serde_json::from_str(&conversation_json)
        .map_err(|e| format!("解析对话JSON失败: {}", e))?;
    let id = conv.get("id").and_then(|v| v.as_str()).ok_or("缺少对话ID")?;

    let dir = conversations_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {}", e))?;
    let path = format!("{}/{}.json", dir, id);
    fs::write(&path, conversation_json).map_err(|e| format!("保存对话失败: {}", e))?;
    Ok(())
}

#[tauri::command]
fn delete_conversation(id: String) -> Result<(), String> {
    let path = format!("{}/{}.json", conversations_dir(), id);
    fs::remove_file(&path).map_err(|e| format!("删除对话失败: {}", e))?;
    Ok(())
}

// ==================================================
// 配置生效自动化 — 智能重启
// ==================================================

#[derive(Serialize, Deserialize)]
struct SmartRestartResult {
    ok: bool,
    restarted: Vec<String>,
    skipped: Vec<String>,
    errors: Vec<String>,
}

/// 根据变更的配置键映射到需要重启的 Docker 服务
fn services_for_env_keys(keys: &[String]) -> Vec<String> {
    let mut services = Vec::new();
    let mut worker = false;
    let mut admin = false;
    let mut orch = false;
    let mut webui = false;

    for key in keys {
        match key.as_str() {
            "DASHSCOPE_API_KEY" | "DASHSCOPE_API_BASE"
            | "OPENAI_API_KEY" | "OPENAI_BASE_URL"
            | "ANTHROPIC_API_KEY"
            | "OR_API_KEY" | "OR_SITE_URL"
            | "AZURE_API_KEY" | "AZURE_API_BASE"
            | "COHERE_API_KEY"
            | "REPLICATE_API_TOKEN"
            | "NOVITA_API_KEY" => {
                worker = true;
                admin = true;
            }
            "LITELLM_MASTER_KEY" => {
                worker = true;
                admin = true;
                orch = true;
                webui = true;
            }
            "REDIS_PASSWORD" => {
                worker = true;
                admin = true;
            }
            "OLLAMA_API_BASE" => {
                worker = true;
                orch = true;
            }
            "QDRANT_API_KEY" => {
                worker = true;
            }
            _ => {
                worker = true;
                admin = true;
            }
        }
    }

    if worker { services.push("litellm-worker".to_string()); }
    if admin { services.push("litellm-admin".to_string()); }
    if orch { services.push("orchestrator-web".to_string()); }
    if webui { services.push("open-webui".to_string()); }
    services
}

#[tauri::command]
fn smart_restart(changed_keys: Vec<String>) -> Result<SmartRestartResult, String> {
    if changed_keys.is_empty() {
        return Ok(SmartRestartResult {
            ok: true,
            restarted: vec![],
            skipped: vec![],
            errors: vec!["没有变更的配置项".to_string()],
        });
    }

    let dir = get_install_dir();
    let services = services_for_env_keys(&changed_keys);

    if services.is_empty() {
        return Ok(SmartRestartResult {
            ok: true,
            restarted: vec![],
            skipped: vec![],
            errors: vec![],
        });
    }

    // 使用 --force-recreate 重新加载 env_file，--no-deps 避免影响依赖服务
    let mut args = vec!["up", "-d", "--force-recreate", "--no-deps"];
    let svc_refs: Vec<&str> = services.iter().map(|s| s.as_str()).collect();
    args.extend(svc_refs.iter());

    let output = cmd("docker")
        .args(&args)
        .current_dir(&dir)
        .output()
        .map_err(|e| format!("Docker 命令执行失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Ok(SmartRestartResult {
            ok: false,
            restarted: vec![],
            skipped: services.clone(),
            errors: vec![stderr],
        });
    }

    // 等待健康检查（最多 60 秒）
    std::thread::sleep(std::time::Duration::from_secs(5));
    let mut healthy = Vec::new();
    let mut unhealthy = Vec::new();

    for _ in 0..11 {
        let check = cmd("docker")
            .args(["compose", "ps", "--format", "{{.Name}}\t{{.Status}}"])
            .current_dir(&dir)
            .output();
        if let Ok(out) = check {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let mut all_healthy = true;
            for svc in &services {
                let container_name = svc.replace("litellm-", "litellm_").replace("orchestrator-web", "orchestrator");
                let found = stdout.lines().any(|line| {
                    line.contains(&container_name) && line.contains("healthy")
                });
                if found {
                    if !healthy.contains(svc) {
                        healthy.push(svc.clone());
                    }
                } else {
                    all_healthy = false;
                }
            }
            if all_healthy && healthy.len() == services.len() {
                break;
            }
        }
        // 移除不健康的到 unhealthy 列表
        unhealthy = services.iter().filter(|s| !healthy.contains(s)).cloned().collect();
        std::thread::sleep(std::time::Duration::from_secs(5));
    }

    let errors: Vec<String> = unhealthy.iter().map(|s| format!("{} 健康检查超时", s)).collect();

    Ok(SmartRestartResult {
        ok: errors.is_empty(),
        restarted: healthy,
        skipped: unhealthy,
        errors,
    })
}

// ==================================================
// 模型管理（CLI 可视化）
// ==================================================

#[tauri::command]
fn get_models() -> Result<String, String> {
    let dir = get_install_dir();
    let output = cmd("python3")
        .args(["litellm_cli.py", "list-models-json"])
        .current_dir(&dir)
        .output()
        .map_err(|e| format!("执行失败: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(format!("{}", String::from_utf8_lossy(&output.stderr)))
    }
}

#[tauri::command]
fn add_model(json_config: String) -> Result<String, String> {
    let dir = get_install_dir();
    let output = cmd("python3")
        .args(["litellm_cli.py", "add-model-json", &json_config])
        .current_dir(&dir)
        .output()
        .map_err(|e| format!("执行失败: {}", e))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if output.status.success() {
        Ok(stdout)
    } else {
        Err(if stdout.is_empty() { String::from_utf8_lossy(&output.stderr).to_string() } else { stdout })
    }
}

#[tauri::command]
fn remove_model(model_name: String) -> Result<String, String> {
    let dir = get_install_dir();
    let output = cmd("python3")
        .args(["litellm_cli.py", "remove-model-json", "--name", &model_name])
        .current_dir(&dir)
        .output()
        .map_err(|e| format!("执行失败: {}", e))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if output.status.success() {
        Ok(stdout)
    } else {
        Err(if stdout.is_empty() { String::from_utf8_lossy(&output.stderr).to_string() } else { stdout })
    }
}

// ==================================================
// 系统托盘
// ==================================================

fn create_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let _tray = TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .tooltip("LiteLLM Suite")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => { app.exit(0); }
            _ => {}
        })
        .build(app)?;
    Ok(())
}

// ==================================================
// 主入口
// ==================================================

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            get_services_status,
            start_services,
            stop_services,
            restart_service,
            open_web_interface,
            run_cli,
            check_docker,
            check_ollama,
            read_env,
            write_env,
            write_env_batch,
            get_usage_stats,
            get_quota,
            get_trends,
            get_service_logs,
            update_user_quota,
            update_key_quota,
            chat_request,
            list_conversations,
            load_conversation,
            save_conversation,
            delete_conversation,
            smart_restart,
            get_models,
            add_model,
            remove_model,
        ])
        .setup(|app| {
            create_tray(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
