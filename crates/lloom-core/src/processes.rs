//! Sub-process management for Ollama.

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
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(f) => match f.try_clone() {
            Ok(fe) => {
                c.stdout(Stdio::from(f));
                c.stderr(Stdio::from(fe));
            }
            Err(_) => {
                c.stdout(Stdio::from(f));
                c.stderr(Stdio::null());
            }
        },
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
    c.spawn()
        .map_err(|e| AppError::Process(format!("failed to spawn {binary}: {e}")))
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

// ── Health helpers ──

/// Async HTTP GET, returning the body. Used for health probes.
async fn http_get(url: &str, timeout_secs: u64) -> String {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build();
    let Ok(client) = client else {
        return String::new();
    };
    let Ok(resp) = client.get(url).send().await else {
        return String::new();
    };
    resp.text().await.unwrap_or_default()
}

pub async fn check_ollama_health() -> bool {
    let out = http_get("http://localhost:11434/api/tags", 3).await;
    out.contains("\"models\"") || out.contains("name")
}

/// Whether the ollama binary is available (bundled or on PATH). The port may
/// still be down if it isn't running — callers use this to tell "not
/// installed" apart from "installed but stopped".
pub fn ollama_installed() -> bool {
    let bin = crate::config::ollama_binary_path();
    if bin != "ollama" {
        return true; // bundled path found
    }
    if Command::new("sh")
        .args(["-c", "command -v ollama >/dev/null 2>&1"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return true;
    }
    // PATH lookup can miss a user-installed ollama when this process inherits
    // a minimal PATH (e.g. launched from an IDE/sandbox). Fall back to the
    // standard install locations before claiming it's not installed.
    std::path::Path::new("/usr/local/bin/ollama").exists()
        || std::path::Path::new("/opt/homebrew/bin/ollama").exists()
}
