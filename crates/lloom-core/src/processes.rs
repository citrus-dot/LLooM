//! Sub-process management — start/stop/restart the Python API server, Ollama,
//! and the Python AI service.

use crate::config;
use crate::error::{AppError, Result};
use std::process::{Child, Command, Stdio};

/// Enhanced PATH so bundled/standard binaries are found in all layouts.
fn enhanced_path() -> String {
    let current = std::env::var("PATH").unwrap_or_default();
    let extra = [
        "/usr/local/bin",
        "/opt/homebrew/bin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
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

fn log_file(name: &str) -> std::path::PathBuf {
    config::log_dir().join(name)
}

fn attach_log(c: &mut Command, log_name: &str) {
    let path = log_file(log_name);
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
    match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(f) => {
            match f.try_clone() {
                Ok(fe) => {
                    c.stdout(Stdio::from(f));
                    c.stderr(Stdio::from(fe));
                }
                Err(_) => {
                    c.stdout(Stdio::from(f));
                    c.stderr(Stdio::null());
                }
            }
        }
        Err(_) => {
            c.stdout(Stdio::null());
            c.stderr(Stdio::null());
        }
    }
}

/// Spawn a child process, detaching stdout to a log file. Returns the Child.
fn spawn(binary: &str, args: &[&str], log: &str, cwd: Option<&str>) -> Result<Child> {
    let mut c = cmd(binary);
    c.args(args);
    if let Some(dir) = cwd {
        c.current_dir(dir);
    }
    attach_log(&mut c, log);
    c.spawn().map_err(|e| AppError::Process(format!("failed to spawn {binary}: {e}")))
}

// ── Python AI micro-service ──

/// Resolve the Python interpreter for the AI service: prefer a local venv
/// (dev), then `python3` on PATH (production bundles Python).
fn python_interp() -> String {
    let install_dir = config::install_dir();
    for cand in [
        install_dir.join(".venv/bin/python"),
        install_dir.join("venv/bin/python"),
        std::path::PathBuf::from(".venv/bin/python"),
        std::path::PathBuf::from("venv/bin/python"),
    ] {
        if cand.exists() && cand.is_file() {
            return cand.to_string_lossy().to_string();
        }
    }
    "python3".to_string()
}

/// The Python AI micro-service is the only required Python process.
///
/// Resolution order:
///   1. PyInstaller bundle: `resources/ai-service/ai-service` (production)
///   2. Source file: `resources/ai_service.py` (installed source)
///   3. Dev: `api/ai_service.py` via `python3 -m uvicorn`
///
/// Returns `Ok(None)` if a healthy instance already answers on the port
/// (prevents duplicate spawns and "address already in use" errors).
pub async fn start_ai() -> Result<Option<Child>> {
    // Fast path: reuse an already-healthy instance.
    if check_ai_health().await.status == "ok" {
        return Ok(None);
    }
    let install_dir = config::install_dir();
    let port = config::ai_port().to_string();

    // 1. PyInstaller bundle
    let bundled = install_dir.join("resources/ai-service/ai-service");
    if bundled.exists() && bundled.is_file() {
        let child = spawn(
            bundled.to_string_lossy().as_ref(),
            &["--port", &port],
            "ai.log",
            Some(install_dir.to_string_lossy().as_ref()),
        )?;
        return Ok(Some(child));
    }

    // 2. Installed source file
    let script = install_dir.join("resources/ai_service.py");
    if script.exists() {
        let interp = python_interp();
        let child = spawn(
            &interp,
            &[script.to_string_lossy().as_ref(), "--port", &port],
            "ai.log",
            Some(install_dir.to_string_lossy().as_ref()),
        )?;
        return Ok(Some(child));
    }

    // 3. Dev mode
    let interp = python_interp();
    let child = spawn(
        &interp,
        &["-m", "uvicorn", "api.ai_service:app", "--port", &port],
        "ai.log",
        Some(install_dir.to_string_lossy().as_ref()),
    )?;
    Ok(Some(child))
}

pub async fn start_ollama() -> Result<Option<Child>> {
    // Fast path: reuse an already-running Ollama (its port is authoritative).
    let bin = config::ollama_binary_path();
    if check_ollama_health().await {
        return Ok(None);
    }
    let child = spawn(&bin, &["serve"], "ollama.log", None)?;
    Ok(Some(child))
}

/// Stop a running Ollama regardless of who started it. If this process spawned
/// it (child handle held), the caller kills the handle; otherwise find and
/// terminate the `ollama serve` process (external / system-managed instance).
pub fn stop_ollama() -> String {
    match Command::new("pkill").args(["-f", "ollama serve"]).status() {
        Ok(s) if s.success() => "Ollama stopped".to_string(),
        Ok(_) => "Ollama not running".to_string(),
        Err(e) => format!("Failed to stop Ollama: {e}"),
    }
}

/// Stop a running AI service regardless of who started it. Matches the dev
/// (`uvicorn api.ai_service`) and installed-script invocation patterns.
pub fn stop_ai() -> String {
    let pats = [
        "uvicorn api.ai_service:app",
        "ai_service.py --port",
        "ai-service/ai-service",
    ];
    let mut stopped = false;
    for pat in pats {
        if let Ok(s) = Command::new("pkill").args(["-f", pat]).status() {
            if s.success() {
                stopped = true;
            }
        }
    }
    if stopped {
        "AI service stopped".to_string()
    } else {
        "AI service not running".to_string()
    }
}

// ── Health helpers ──

/// Async HTTP GET, returning the body. Used for health probes.
async fn http_get(url: &str, timeout_secs: u64) -> String {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build();
    let Ok(client) = client else { return String::new() };
    let Ok(resp) = client.get(url).send().await else { return String::new() };
    resp.text().await.unwrap_or_default()
}

pub async fn check_ai_health() -> crate::ai_client::AiHealth {
    crate::ai_client::health().await
}

pub async fn check_ollama_health() -> bool {
    let out = http_get("http://localhost:11434/api/tags", 3).await;
    out.contains("\"models\"") || out.contains("name")
}
