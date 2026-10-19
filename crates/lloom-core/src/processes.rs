//! Sub-process management for Ollama.

use crate::config;
use crate::error::{AppError, Result};
use std::process::{Child, Command, Stdio};

/// Enhanced PATH so bundled/standard binaries are found in all layouts.
/// Splits/joins on the platform separator (`:` on Unix, `;` on Windows) so the
/// child environment is never a single bogus path entry.
fn enhanced_path() -> String {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let extra: &[&str] = if cfg!(windows) {
        // Standard user install roots (Ollama installer defaults to
        // %LOCALAPPDATA%\Programs\Ollama; expand below when spawning).
        &[]
    } else {
        &[
            "/usr/local/bin",
            "/opt/homebrew/bin",
            "/usr/bin",
            "/bin",
            "/usr/sbin",
            "/sbin",
        ]
    };
    let mut parts: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
    let current = std::env::var("PATH").unwrap_or_default();
    for p in current.split(sep).filter(|s| !s.is_empty()) {
        if !parts.iter().any(|q| q == p) {
            parts.push(p.to_string());
        }
    }
    parts.join(&sep.to_string())
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

// ── Health helpers ──

/// Open a URL in the user's default browser (cross-platform; used by
/// `--open` on startup and the `/api/system/open-web` endpoint). `webbrowser`
/// wraps the platform launcher (open / xdg-open / cmd start) with proper
/// argument quoting. Fire-and-forget: the browser is not our child process.
pub fn open_browser(url: &str) -> bool {
    webbrowser::open(url).is_ok()
}

/// Reveal a directory in the platform file manager. `webbrowser` converts
/// local paths to `file://` URLs and hands them to the platform handler
/// (Finder / Explorer / xdg-open), so one code path serves all platforms.
pub fn reveal_in_file_manager(dir: &str) -> bool {
    webbrowser::open(dir).is_ok()
}

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
    // PATH lookup via the platform's `where`/`command -v`, plus standard
    // install roots a minimal PATH (IDE/sandbox launch) can miss.
    if cfg!(windows) {
        if Command::new("where")
            .arg("ollama")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return true;
        }
        let mut roots = vec![std::path::PathBuf::from(
            r"C:\Program Files\Ollama\ollama.exe",
        )];
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            // Ollama installer default per-user location.
            roots.push(
                std::path::PathBuf::from(local)
                    .join("Programs")
                    .join("Ollama")
                    .join("ollama.exe"),
            );
        }
        return roots.iter().any(|r| r.exists());
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The enhanced PATH must stay a well-formed entry list on the host
    /// platform: exactly the platform separator, no empty entries, no dups.
    #[test]
    fn enhanced_path_is_well_formed() {
        let sep = if cfg!(windows) { ';' } else { ':' };
        let path = enhanced_path();
        let entries: Vec<&str> = path.split(sep).collect();
        assert!(!entries.is_empty());
        assert!(
            entries.iter().all(|e| !e.is_empty()),
            "no empty entries: {path}"
        );
        let mut deduped = entries.clone();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(entries.len(), deduped.len(), "no dup entries: {path}");
    }

    /// Unix extras must survive on Unix; Windows never injects Unix roots.
    #[test]
    fn enhanced_path_platform_extras() {
        let sep = if cfg!(windows) { ';' } else { ':' };
        let path = enhanced_path();
        let has = |root: &str| path.split(sep).any(|e| e == root);
        if cfg!(windows) {
            assert!(!has("/usr/local/bin"));
        } else {
            assert!(has("/usr/local/bin") && has("/opt/homebrew/bin"));
        }
    }
}
