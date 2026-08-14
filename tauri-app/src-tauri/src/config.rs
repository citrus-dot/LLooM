//! Runtime configuration — install dir, data dir, ports, path resolution.

use std::path::{Path, PathBuf};

pub const DEFAULT_API_PORT: u16 = 7860;
pub const DEFAULT_AI_PORT: u16 = 7862;
pub const DEFAULT_WEB_PORT: u16 = 7861;

/// Root install dir. Defaults to `LLOOM_INSTALL_DIR` or `.`.
pub fn install_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("LLOOM_INSTALL_DIR").unwrap_or_else(|_| ".".to_string()),
    )
}

/// Data dir for SQLite / conversations / logs. Defaults to `{install_dir}/data`.
pub fn data_dir() -> PathBuf {
    let dir = match std::env::var("LLOOM_DATA_DIR") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => install_dir().join("data"),
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("[config] failed to create data dir {dir:?}: {e}");
    }
    dir
}

pub fn db_path() -> PathBuf {
    data_dir().join("lloom.db")
}

pub fn conversations_dir() -> PathBuf {
    let dir = data_dir().join("conversations");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("[config] failed to create conversations dir {dir:?}: {e}");
    }
    dir
}

pub fn log_dir() -> PathBuf {
    data_dir().join("logs")
}

pub fn env_file_path() -> PathBuf {
    install_dir().join(".env")
}

pub fn api_port() -> u16 {
    std::env::var("LLOOM_API_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_API_PORT)
}

pub fn ai_port() -> u16 {
    std::env::var("LLOOM_AI_SERVICE_URL")
        .ok()
        .and_then(|u| u.split(':').last().and_then(|p| p.trim_end_matches('/').parse().ok()))
        .unwrap_or(DEFAULT_AI_PORT)
}

pub fn web_port() -> u16 {
    std::env::var("LLOOM_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_WEB_PORT)
}

/// Locate `ui/index.html` in any of the known layouts (deb / portable / repo).
pub fn ui_dir() -> Option<PathBuf> {
    let candidates = [
        install_dir().join("resources/ui"),
        install_dir().join("ui"),
        install_dir().join("../../../src-tauri/ui"),
        PathBuf::from("tauri-app/src-tauri/ui"),
        PathBuf::from("ui"),
    ];
    for c in candidates.iter() {
        if c.join("index.html").exists() {
            return Some(c.clone());
        }
    }
    None
}

/// Locate the PyInstaller lloom-server binary (production bundle).
pub fn api_binary_path() -> Option<PathBuf> {
    for sub in &["resources/lloom-server", "lloom-server"] {
        let bin = install_dir().join(sub).join("lloom-server");
        if bin.exists() && bin.is_file() {
            return Some(bin);
        }
    }
    None
}

/// Locate the bundled Ollama binary, falling back to PATH.
pub fn ollama_binary_path() -> String {
    for sub in &["resources", ""] {
        let bin = install_dir().join(sub).join("ollama");
        if bin.exists() && bin.is_file() {
            return bin.to_string_lossy().to_string();
        }
    }
    "ollama".to_string()
}

/// Resolve install dir across dev / portable / deb layouts.
pub fn resolve_install_dir() -> PathBuf {
    if let Ok(d) = std::env::var("LLOOM_INSTALL_DIR") {
        if !d.is_empty() {
            return canonical(PathBuf::from(d));
        }
    }
    // Dev builds: find the repo root (has api/ and .venv/) by walking up from
    // the executable. Checked before the portable layout because target/debug
    // may contain a copied resources/ dir that would otherwise win.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for cand in [
                "../../../../api/server.py",
                "../../../api/server.py",
                "../../api/server.py",
                "../api/server.py",
            ] {
                let p = dir.join(cand);
                if p.exists() {
                    // p = <root>/api/server.py → root is p's grandparent
                    if let Some(root) = p.parent().and_then(|a| a.parent()) {
                        return canonical(root.to_path_buf());
                    }
                }
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        if cwd.join("api/server.py").exists() {
            return canonical(cwd);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join("resources/lloom-server/lloom-server").exists() {
                return canonical(dir.to_path_buf());
            }
        }
    }
    let deb = PathBuf::from("/usr/lib/LLooM");
    if deb.join("resources/lloom-server/lloom-server").exists() {
        return deb;
    }
    canonical(PathBuf::from("."))
}

/// Normalize a path by resolving `..` and symlinks; falls back to the input.
fn canonical(p: PathBuf) -> PathBuf {
    std::fs::canonicalize(&p).unwrap_or(p)
}

/// Read a `.env` file into a map.
pub fn read_env() -> std::collections::HashMap<String, String> {
    let mut result = std::collections::HashMap::new();
    if let Ok(content) = std::fs::read_to_string(env_file_path()) {
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

/// Resolve an API key for a model: value of `api_key_env` var, or the literal
/// value if it looks like a key (not an env var name).
pub fn api_key_for(api_key_env: &str) -> String {
    if api_key_env.is_empty() {
        return String::new();
    }
    // If the stored value is a real key (contains no whitespace and is short),
    // treat it as the key itself. Otherwise read the named env var.
    let looks_like_key = api_key_env.contains("sk-")
        || api_key_env.contains("-")
        && !api_key_env.chars().any(|c| c.is_ascii_whitespace());
    if looks_like_key && !api_key_env.contains('_') {
        return api_key_env.to_string();
    }
    std::env::var(api_key_env).unwrap_or_default()
}

/// A value that can be a literal or a path. No-op; kept for API symmetry.
pub fn is_absolute(p: &str) -> bool {
    Path::new(p).is_absolute()
}
