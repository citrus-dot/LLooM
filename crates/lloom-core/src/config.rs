//! Runtime configuration — install dir, data dir, ports, path resolution.
//!
//! 配置优先级链（高 → 低）：
//! **CLI 参数**（`CliOverrides`，main 解析后 `init_cli_overrides` 注入一次）
//! → **环境变量**（含启动时从 `.env` 注入的部分）→ **代码默认值**。
//! 动态运行时调参（健康阈值/信号权重等）走 SQLite settings KV，不在此层。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const DEFAULT_AI_PORT: u16 = 7862;
pub const DEFAULT_WEB_PORT: u16 = 7861;

/// CLI 覆盖层：仅承载启动参数能显式给定的项；`None` 表示未指定、回落 env 链。
#[derive(Debug, Default, Clone)]
pub struct CliOverrides {
    pub web_port: Option<u16>,
    pub ai_port: Option<u16>,
    pub bind: Option<String>,
}

static CLI_OVERRIDES: OnceLock<CliOverrides> = OnceLock::new();

/// main() 在解析启动参数后调用一次（进程生命周期内不可重复设置）。
pub fn init_cli_overrides(o: CliOverrides) {
    let _ = CLI_OVERRIDES.set(o);
}

fn overrides() -> &'static CliOverrides {
    CLI_OVERRIDES.get_or_init(CliOverrides::default)
}

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

/// Current semantic-cache similarity threshold. Source of truth is the `settings`
/// kv table (so the auto-tuner can update it at runtime); falls back to 0.80.
pub fn cache_threshold() -> f64 {
    crate::db::get_setting("cache_threshold")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.80)
}

/// Persist the semantic-cache similarity threshold (called by the tuner).
pub fn set_cache_threshold(t: f64) -> std::result::Result<(), String> {
    let clamped = t.max(0.70).min(0.92);
    crate::db::set_setting("cache_threshold", &format!("{clamped:.4}"))
        .map_err(|e| e.to_string())
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

/// P1.d 影子采样率（0..1，默认 0.10）：命中时对请求做「路由选择 × 强基线」双跑。
/// 存 settings `routing.shadow_ratio`（可零成本关闭）。
pub fn shadow_ratio() -> f64 {
    crate::db::get_setting("routing.shadow_ratio")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.10)
        .clamp(0.0, 1.0)
}

// ── P3 健康感知（ROUTING-PLAN §P3）──
// 阈值全部走 settings KV，可在设置页运行时调整；默认值对应「滑窗 5 内 ≥2 失败降级、
// 连续 3 失败宕机、连续 5 失败熔断」的自适应口径。

/// 滑窗大小（保留最近 n 次 outcomes）。默认 5。
pub fn health_fail_window() -> i64 {
    crate::db::get_setting("health.fail_window")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(5)
        .clamp(1, 1000)
}

/// 滑窗内达到该失败数 → degraded。默认 2。
pub fn health_degraded_fails() -> u32 {
    crate::db::get_setting("health.degraded_fails")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(2)
        .clamp(1, 1000)
}

/// 连续失败达该数 → down（状态机转移）。默认 3。
pub fn health_down_consecutive() -> u32 {
    crate::db::get_setting("health.down_consecutive")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(3)
        .clamp(1, 10000)
}

/// 熔断阈值：连续失败达该数强制 down（安全网，通常 ≥ down_consecutive）。默认 5。
pub fn health_circuit_threshold() -> u32 {
    crate::db::get_setting("health.circuit_threshold")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(5)
        .clamp(1, 10000)
}

/// 主动探测间隔（秒）：对 `down`/`degraded` 模型发最小请求试探。默认 60。
pub fn health_probe_sec() -> u64 {
    crate::db::get_setting("health.probe_sec")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(60)
        .clamp(5, 86400)
}

pub fn env_file_path() -> PathBuf {
    install_dir().join(".env")
}

/// AI 微服务端口链：CLI `--ai-port` > `LLOOM_AI_PORT` > `LLOOM_AI_SERVICE_URL`
/// 里抠出的端口 > 默认 7862。（前两者生效时 spawn 与调用同源，见 [`ai_service_url`]）
pub fn ai_port() -> u16 {
    ai_port_with(overrides(), std::env::var("LLOOM_AI_PORT").ok().as_deref(), std::env::var("LLOOM_AI_SERVICE_URL").ok().as_deref())
}

fn ai_port_with(over: &CliOverrides, ai_port_env: Option<&str>, ai_url_env: Option<&str>) -> u16 {
    over.ai_port
        .or_else(|| ai_port_env.and_then(|p| p.parse().ok()))
        .or_else(|| {
            ai_url_env
                .as_deref()
                .and_then(extract_port_from_url)
        })
        .unwrap_or(DEFAULT_AI_PORT)
}

fn extract_port_from_url(url: &str) -> Option<u16> {
    url.split(':').last().and_then(|p| p.trim_end_matches('/').parse().ok())
}

/// Rust → Python AI 微服务的调用 URL（与 [`ai_port`] 同源）：
/// 显式设置了完整 `LLOOM_AI_SERVICE_URL`（且无 CLI/`LLOOM_AI_PORT` 覆盖）时按原样使用，
/// 保留 base path 自定义能力；否则按 ai_port() 构造。
pub fn ai_service_url() -> String {
    ai_service_url_with(overrides(), std::env::var("LLOOM_AI_PORT").ok().as_deref(), std::env::var("LLOOM_AI_SERVICE_URL").ok().as_deref())
}

fn ai_service_url_with(over: &CliOverrides, ai_port_env: Option<&str>, ai_url_env: Option<&str>) -> String {
    let has_port_override = over.ai_port.is_some() || ai_port_env.is_some_and(|p| p.parse::<u16>().is_ok());
    if !has_port_override {
        if let Some(url) = ai_url_env.filter(|u| !u.trim().is_empty()) {
            return url.to_string();
        }
    }
    format!("http://localhost:{}", ai_port_with(over, ai_port_env, ai_url_env))
}

pub fn web_port() -> u16 {
    web_port_with(overrides(), std::env::var("LLOOM_WEB_PORT").ok().as_deref())
}

fn web_port_with(over: &CliOverrides, web_port_env: Option<&str>) -> u16 {
    over.web_port
        .or_else(|| web_port_env.and_then(|p| p.parse().ok()))
        .unwrap_or(DEFAULT_WEB_PORT)
}

/// N1/O2 收尾：REST 服务器默认只绑环回（本地工具，默认关闭局域网暴露）。
/// 需要局域网访问时显式给 `--bind 0.0.0.0` 或设 `LLOOM_BIND=0.0.0.0`（建议同时配置 LLOOM_PROXY_TOKEN）。
pub fn bind_addr() -> String {
    bind_addr_with(overrides(), std::env::var("LLOOM_BIND").ok().as_deref())
}

fn bind_addr_with(over: &CliOverrides, bind_env: Option<&str>) -> String {
    over.bind
        .clone()
        .or_else(|| bind_env.map(str::to_string).filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

/// Locate the built frontend (React `dist/` or legacy single `index.html`).
pub fn ui_dir() -> Option<PathBuf> {
    let candidates = [
        install_dir().join("resources/webui/dist"),
        install_dir().join("resources/ui"),
        install_dir().join("../../webui/dist"),
        install_dir().join("../../webui"),
        PathBuf::from("webui/dist"),
        PathBuf::from("webui"),
        PathBuf::from("ui"),
    ];
    for c in candidates.iter() {
        if c.join("index.html").exists() {
            return Some(c.clone());
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
                "../../../../api/ai_service.py",
                "../../../api/ai_service.py",
                "../../api/ai_service.py",
                "../api/ai_service.py",
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
        if cwd.join("api/ai_service.py").exists() {
            return canonical(cwd);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join("resources/ai-service/ai-service").exists() {
                return canonical(dir.to_path_buf());
            }
        }
    }
    let deb = PathBuf::from("/usr/lib/LLooM");
    if deb.join("resources/ai-service/ai-service").exists() {
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

/// Load `.env` into the current process environment so that subprocesses
/// (Python AI service, Ollama) inherit the variables. Existing env vars take precedence.
pub fn load_env() {
    for (k, v) in read_env() {
        if std::env::var(&k).is_err() {
            std::env::set_var(k, v);
        }
    }
}

/// Resolve an API key for a model: value of `api_key_env` var, or the literal
/// value if it looks like a key (not an env var name).
pub fn api_key_for(api_key_env: &str) -> String {
    if api_key_env.is_empty() {
        return String::new();
    }
    // Treat the stored value as a literal key only when it clearly is one
    // (`sk-...` with no underscore). Anything else is an env-var *name*
    // (e.g. `DASHSCOPE_API_KEY`) and is read from the process environment.
    let is_literal_key = api_key_env.starts_with("sk-") && !api_key_env.contains('_');
    if is_literal_key {
        return api_key_env.to_string();
    }
    std::env::var(api_key_env).unwrap_or_default()
}

/// Resolve a value that may be either a literal (e.g. an URL) or an env var name.
/// Used for `api_base` stored in the DB as `DASHSCOPE_API_BASE` etc.
pub fn resolve_env_or_literal(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    // Treat as env var name if it looks like one: all uppercase, contains underscore, no spaces/slashes/colons.
    let looks_like_env = value.chars().all(|c| c.is_ascii_uppercase() || c == '_')
        && value.contains('_')
        && !value.chars().any(|c| c.is_ascii_whitespace());
    if looks_like_env {
        std::env::var(value).unwrap_or_default()
    } else {
        value.to_string()
    }
}

/// A value that can be a literal or a path. No-op; kept for API symmetry.
pub fn is_absolute(p: &str) -> bool {
    Path::new(p).is_absolute()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_port_precedence() {
        // 默认值
        assert_eq!(web_port_with(&CliOverrides::default(), None), DEFAULT_WEB_PORT);
        // env > 默认
        assert_eq!(web_port_with(&CliOverrides::default(), Some("8080")), 8080);
        // CLI > env > 默认
        let over = CliOverrides { web_port: Some(9090), ..Default::default() };
        assert_eq!(web_port_with(&over, Some("8080")), 9090);
        // 非法 env 回落默认
        assert_eq!(web_port_with(&CliOverrides::default(), Some("not-a-port")), DEFAULT_WEB_PORT);
    }

    #[test]
    fn ai_port_precedence() {
        // 默认值
        assert_eq!(ai_port_with(&CliOverrides::default(), None, None), DEFAULT_AI_PORT);
        // 从 LLOOM_AI_SERVICE_URL 抠端口（旧行为保留）
        assert_eq!(
            ai_port_with(&CliOverrides::default(), None, Some("http://127.0.0.1:17962/")),
            17962
        );
        // LLOOM_AI_PORT > URL 抠取
        assert_eq!(
            ai_port_with(&CliOverrides::default(), Some("17970"), Some("http://127.0.0.1:17962/")),
            17970
        );
        // CLI > 一切
        let over = CliOverrides { ai_port: Some(17980), ..Default::default() };
        assert_eq!(ai_port_with(&over, Some("17970"), Some("http://127.0.0.1:17962/")), 17980);
    }

    #[test]
    fn ai_service_url_coherent_with_port() {
        // 旧行为：显式完整 URL 原样使用（保留 base path 自定义）
        assert_eq!(
            ai_service_url_with(&CliOverrides::default(), None, Some("http://10.0.0.5:8000/v1")),
            "http://10.0.0.5:8000/v1"
        );
        // 显式端口 env 生效时，URL 必须与 ai_port 同源（防 spawn/调用断链）
        assert_eq!(
            ai_service_url_with(&CliOverrides::default(), Some("17970"), Some("http://127.0.0.1:17962/")),
            "http://localhost:17970"
        );
        // 非法端口 env → ai_port 回落默认，URL 同源
        assert_eq!(
            ai_service_url_with(&CliOverrides::default(), Some("not-a-port"), None),
            format!("http://localhost:{DEFAULT_AI_PORT}")
        );
        assert_eq!(
            ai_service_url_with(&CliOverrides::default(), Some("7862"), None),
            "http://localhost:7862"
        );
        // CLI 覆盖压过 URL env
        let over = CliOverrides { ai_port: Some(17980), ..Default::default() };
        assert_eq!(
            ai_service_url_with(&over, None, Some("http://127.0.0.1:17962/")),
            "http://localhost:17980"
        );
        // 未配置任何项 → 按默认端口构造
        assert_eq!(
            ai_service_url_with(&CliOverrides::default(), None, None),
            format!("http://localhost:{DEFAULT_AI_PORT}")
        );
    }

    #[test]
    fn bind_addr_precedence() {
        assert_eq!(bind_addr_with(&CliOverrides::default(), None), "127.0.0.1");
        assert_eq!(bind_addr_with(&CliOverrides::default(), Some("0.0.0.0")), "0.0.0.0");
        let over = CliOverrides { bind: Some("192.168.1.10".into()), ..Default::default() };
        assert_eq!(bind_addr_with(&over, Some("0.0.0.0")), "192.168.1.10");
        // 空 env 视为未设置
        assert_eq!(bind_addr_with(&CliOverrides::default(), Some("  ")), "127.0.0.1");
    }
}
