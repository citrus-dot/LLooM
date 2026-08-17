//! Conversation CRUD — JSON files under the data dir.
//!
//! Ported from the Rust conversation handlers; keeps the same on-disk layout
//! (`data/conversations/{id}.json`) shared with the frontend.

use crate::config;
use crate::error::{AppError, Result};
use crate::models::ConversationMeta;
use serde_json::{json, Value};
use std::path::PathBuf;

fn conv_path(id: &str) -> PathBuf {
    config::conversations_dir().join(format!("{id}.json"))
}

/// Validate a conversation id. Only `[A-Za-z0-9_-]` are allowed — this blocks
/// path traversal (`../`) and other filesystem-unsafe input that comes straight
/// from the URL path `/api/conversations/{id}`. Returns an error on violation.
fn validate_id(id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(AppError::InvalidRequest("conversation id is empty".to_string()));
    }
    if id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        Ok(())
    } else {
        Err(AppError::InvalidRequest(format!(
            "invalid conversation id '{id}': only letters, digits, '_' and '-' are allowed"
        )))
    }
}

pub fn list() -> Result<Vec<ConversationMeta>> {
    let dir = config::conversations_dir();
    let mut convs = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Ok(convs),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e != "json").unwrap_or(true) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        let Ok(data) = serde_json::from_str::<Value>(&content) else { continue };
        convs.push(ConversationMeta {
            id: data["id"].as_str().unwrap_or("").to_string(),
            title: data["title"].as_str().unwrap_or("").to_string(),
            updated_at: data["updated_at"].as_str().unwrap_or("").to_string(),
            message_count: data["messages"].as_array().map(|a| a.len()).unwrap_or(0),
        });
    }
    convs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(convs)
}

pub fn load(id: &str) -> Result<Value> {
    validate_id(id)?;
    let path = conv_path(id);
    let content = std::fs::read_to_string(&path)
        .map_err(|_| AppError::NotFound(format!("conversation '{id}'")))?;
    serde_json::from_str(&content).map_err(AppError::Json)
}

pub fn save(id: &str, data: &Value) -> Result<()> {
    validate_id(id)?;
    let path = conv_path(id);
    std::fs::write(&path, data.to_string())
        .map_err(|e| AppError::Io(e))
}

pub fn delete(id: &str) -> Result<()> {
    validate_id(id)?;
    let path = conv_path(id);
    if !path.exists() {
        return Err(AppError::NotFound(format!("conversation '{id}'")));
    }
    std::fs::remove_file(&path).map_err(AppError::Io)
}

/// Rename a conversation. Loads the existing JSON, updates only `title` (and
/// bumps `updated_at` so the list re-sorts to the top), and writes it back.
/// Messages are preserved — unlike `save_or_create`, this never clears them.
pub fn rename(id: &str, title: &str) -> Result<()> {
    validate_id(id)?;
    let mut data = load(id)?;
    data["title"] = Value::String(title.to_string());
    data["updated_at"] = Value::String(now_iso());
    let path = conv_path(id);
    std::fs::write(&path, data.to_string()).map_err(AppError::Io)
}

/// Auto-title a conversation from its first user message.
pub fn auto_title(messages: &[Value]) -> String {
    for msg in messages {
        if msg.get("role").and_then(|r| r.as_str()) == Some("user") {
            if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
                let trimmed: String = content.chars().take(20).collect();
                return trimmed;
            }
        }
    }
    "新对话".to_string()
}

/// Save with the same semantics as the old API: generate id if empty, merge
/// created_at on overwrite. Returns the final conversation id.
pub fn save_or_create(req_id: &str, title: &str, messages: &[Value]) -> Result<String> {
    let id = if req_id.is_empty() {
        format!("{:012x}", rand_hex())
    } else {
        req_id.to_string()
    };
    let now = now_iso();
    let path = conv_path(&id);

    // 标题策略：
    // - 新建对话（req_id 为空）且未提供 title：从首条用户消息自动生成。
    // - 已存在对话且 title 为空：保留原 title，避免用户编辑过的标题被后续消息覆盖。
    // - 任何显式传入的 title：直接使用（首次保存或重命名）。
    let title = if !title.is_empty() {
        title.to_string()
    } else if path.exists() {
        if let Ok(existing) = std::fs::read_to_string(&path) {
            if let Ok(v) = serde_json::from_str::<Value>(&existing) {
                v["title"].as_str().unwrap_or(&auto_title(messages)).to_string()
            } else {
                auto_title(messages)
            }
        } else {
            auto_title(messages)
        }
    } else {
        auto_title(messages)
    };

    let created_at = if path.exists() {
        if let Ok(existing) = std::fs::read_to_string(&path) {
            if let Ok(v) = serde_json::from_str::<Value>(&existing) {
                v["created_at"].as_str().unwrap_or(&now).to_string()
            } else {
                now.clone()
            }
        } else {
            now.clone()
        }
    } else {
        now.clone()
    };

    let data = json!({
        "id": id,
        "title": title,
        "messages": messages,
        "created_at": created_at,
        "updated_at": now,
    });
    std::fs::write(&path, data.to_string()).map_err(AppError::Io)?;
    Ok(id)
}

fn rand_hex() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let pid = std::process::id() as u128;
    (nanos ^ (pid << 32)) & 0xffff_ffff_ffff
}

fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    format_iso(secs)
}

fn format_iso(secs: i64) -> String {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}
