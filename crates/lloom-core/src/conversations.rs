//! Conversation storage — SQLite-backed (A-b decision of CONTEXT-PLAN).
//!
//! Layout in `lloom.db`:
//!   conversations(id, title, created_at, updated_at, summary, summary_upto)
//!   messages(id, conv_id, seq, role, content, meta JSON, created_at)
//!
//! SQLite transactions give atomicity (no more torn JSON writes). The legacy
//! `data/conversations/{id}.json` files are imported once on startup and left
//! in place as a rollback backup — they are never written again.
//!
//! `summary` / `summary_upto` persist the L2 rolling summary: the summary text
//! plus the number of leading messages it covers (seq < summary_upto).

use crate::config;
use crate::db;
use crate::error::{AppError, Result};
use crate::models::ConversationMeta;
use rusqlite::params;
use serde_json::{json, Value};

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

fn new_id() -> String {
    // Time-ordered unique-enough id: (ms timestamp << 20) | (pid & 0xfffff).
    // Cheaper than a uuid dep and monotonically sortable by creation time.
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u128;
    let pid = std::process::id() as u128;
    format!("{:016x}", (ms << 20) | (pid & 0xf_ffff))
}

// ── Migration from legacy JSON files ──

/// Import any `data/conversations/*.json` file that is not yet in the DB.
/// Idempotent: already-imported ids are skipped, files are never modified or
/// deleted (they remain as a rollback backup).
pub fn migrate_json_dir() -> Result<usize> {
    let dir = config::conversations_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Ok(0),
    };
    let mut imported = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e != "json").unwrap_or(true) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        let Ok(data) = serde_json::from_str::<Value>(&content) else { continue };
        let Some(id) = data["id"].as_str() else { continue };
        if id.is_empty() || validate_id(id).is_err() {
            continue;
        }
        let conn = match db::open_fk() {
            Ok(c) => c,
            Err(_) => continue,
        };
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = ?1)",
                params![id],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n == 1)
            .unwrap_or(false);
        if exists {
            continue;
        }
        let title = data["title"].as_str().unwrap_or("新对话").to_string();
        let created_at = data["created_at"].as_str().unwrap_or("").to_string();
        let updated_at = data["updated_at"].as_str().unwrap_or("").to_string();
        let now = now_iso();
        if import_conversation(
            &conn,
            id,
            &title,
            if created_at.is_empty() { &now } else { &created_at },
            if updated_at.is_empty() { &now } else { &updated_at },
            data["messages"].as_array(),
        )
        .is_ok()
        {
            imported += 1;
        }
    }
    if imported > 0 {
        println!("[conversations] migrated {imported} JSON conversation(s) into SQLite");
    }
    Ok(imported)
}

fn import_conversation(
    conn: &rusqlite::Connection,
    id: &str,
    title: &str,
    created_at: &str,
    updated_at: &str,
    messages: Option<&Vec<Value>>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO conversations (id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, title, created_at, updated_at],
    )?;
    if let Some(msgs) = messages {
        for (i, m) in msgs.iter().enumerate() {
            let role = m["role"].as_str().unwrap_or("user");
            let content = m["content"].as_str().unwrap_or("");
            let meta = m.get("meta").filter(|v| !v.is_null()).map(|v| v.to_string());
            conn.execute(
                "INSERT INTO messages (conv_id, seq, role, content, meta, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, (i + 1) as i64, role, content, meta, updated_at],
            )?;
        }
    }
    Ok(())
}

// ── Listing / loading ──

pub fn list() -> Result<Vec<ConversationMeta>> {
    let conn = db::open()?;
    let mut stmt = conn.prepare(
        "SELECT c.id, c.title, c.updated_at,
                (SELECT COUNT(*) FROM messages m WHERE m.conv_id = c.id) AS msg_count
         FROM conversations c
         ORDER BY c.updated_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        let count: i64 = row.get("msg_count")?;
        Ok(ConversationMeta {
            id: row.get("id")?,
            title: row.get("title")?,
            updated_at: row.get("updated_at")?,
            message_count: count as usize,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Load one conversation as the frontend-facing JSON document. Any trailing
/// assistant message stuck in `generating` (service crashed mid-answer) is
/// marked `interrupted` — persisted and returned — so the UI can show a
/// retry affordance instead of a silent hole.
pub fn load(id: &str) -> Result<Value> {
    validate_id(id)?;
    let conn = db::open()?;
    let (title, created_at, updated_at, summary, summary_upto): (String, String, String, Option<String>, i64) = conn
        .query_row(
            "SELECT title, created_at, updated_at, summary, summary_upto FROM conversations WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .map_err(|_| AppError::NotFound(format!("conversation '{id}'")))?;

    let mut stmt = conn.prepare(
        "SELECT seq, role, content, meta FROM messages WHERE conv_id = ?1 ORDER BY seq ASC",
    )?;
    let rows = stmt.query_map(params![id], |r| {
        Ok((
            r.get::<_, i64>("seq")?,
            r.get::<_, String>("role")?,
            r.get::<_, String>("content")?,
            r.get::<_, Option<String>>("meta")?,
        ))
    })?;
    let mut messages: Vec<Value> = Vec::new();
    for row in rows {
        let (seq, role, content, meta) = row?;
        let meta_val: Value = match meta {
            Some(s) => serde_json::from_str(&s).unwrap_or(Value::Null),
            None => Value::Null,
        };
        messages.push(json!({
            "seq": seq,
            "role": role,
            "content": content,
            "meta": meta_val,
        }));
    }

    // Crash recovery: trailing assistant message(s) still marked `generating`
    // → flip to `interrupted` (both in DB and in the returned doc).
    for m in messages.iter_mut().rev() {
        let is_assistant = m["role"].as_str() == Some("assistant");
        let status = m["meta"]["status"].as_str().unwrap_or("");
        if is_assistant && status == "generating" {
            let seq = m["seq"].as_i64().unwrap_or_default();
            let mut meta = m["meta"].clone();
            meta["status"] = json!("interrupted");
            let _ = conn.execute(
                "UPDATE messages SET meta = ?1 WHERE conv_id = ?2 AND seq = ?3",
                params![meta.to_string(), id, seq],
            );
            m["meta"] = meta;
        } else {
            break; // only a contiguous generating tail counts as interrupted
        }
    }

    let mut doc = json!({
        "id": id,
        "title": title,
        "messages": messages,
        "created_at": created_at,
        "updated_at": updated_at,
    });
    if let Some(s) = summary {
        doc["summary"] = json!(s);
        doc["summary_upto"] = json!(summary_upto);
    }
    Ok(doc)
}

pub fn delete(id: &str) -> Result<()> {
    validate_id(id)?;
    let conn = db::open_fk()?;
    let n = conn.execute("DELETE FROM conversations WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(AppError::NotFound(format!("conversation '{id}'")));
    }
    Ok(())
}

/// Rename a conversation. Bumps `updated_at` so the list re-sorts to the top.
pub fn rename(id: &str, title: &str) -> Result<()> {
    validate_id(id)?;
    let conn = db::open()?;
    let n = conn.execute(
        "UPDATE conversations SET title = ?1, updated_at = ?2 WHERE id = ?3",
        params![title, now_iso(), id],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("conversation '{id}'")));
    }
    Ok(())
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

/// Legacy full-save semantics (POST /api/conversations): creates the
/// conversation if needed and replaces its messages atomically. Returns the id.
pub fn save_or_create(req_id: &str, title: &str, messages: &[Value]) -> Result<String> {
    validate_id(req_id)?;
    let mut conn = db::open_fk()?;
    let now = now_iso();
    let existing: Option<(String, String)> = conn
        .query_row(
            "SELECT title, created_at FROM conversations WHERE id = ?1",
            params![req_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();

    let id = if req_id.is_empty() { new_id() } else { req_id.to_string() };

    // Title policy (unchanged from the JSON era): explicit title wins; on
    // overwrite with empty title keep the existing title; new conversations
    // auto-title from the first user message.
    let (final_title, created_at) = match &existing {
        Some((t, c)) => (
            if title.is_empty() { t.clone() } else { title.to_string() },
            c.clone(),
        ),
        None => (
            if title.is_empty() { auto_title(messages) } else { title.to_string() },
            now.clone(),
        ),
    };

    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO conversations (id, title, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET title = excluded.title, updated_at = excluded.updated_at",
        params![id, final_title, created_at, now],
    )?;
    if existing.is_some() {
        tx.execute("DELETE FROM messages WHERE conv_id = ?1", params![id])?;
    }
    for (i, m) in messages.iter().enumerate() {
        let role = m["role"].as_str().unwrap_or("user");
        let content = m["content"].as_str().unwrap_or("");
        let meta = m.get("meta").filter(|v| !v.is_null()).map(|v| v.to_string());
        tx.execute(
            "INSERT INTO messages (conv_id, seq, role, content, meta, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, (i + 1) as i64, role, content, meta, now],
        )?;
    }
    tx.commit()?;
    Ok(id)
}

// ── Append / update (two-phase persistence) ──

/// Append one message to a conversation. Returns its seq. Creates the
/// conversation row if it does not exist yet (title auto-derived).
pub fn append_message(
    conv_id: &str,
    role: &str,
    content: &str,
    meta: Option<&Value>,
) -> Result<(String, i64)> {
    validate_id(conv_id)?;
    if content.len() > 512 * 1024 {
        return Err(AppError::InvalidRequest("message too large (>512KB)".to_string()));
    }
    let conn = db::open_fk()?;
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = ?1)",
            params![conv_id],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n == 1)
        .unwrap_or(false);
    let now = now_iso();
    if !exists {
        let title = if role == "user" {
            content.chars().take(20).collect()
        } else {
            "新对话".to_string()
        };
        conn.execute(
            "INSERT INTO conversations (id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![conv_id, title, now, now],
        )?;
    }
    let next_seq: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE conv_id = ?1",
            params![conv_id],
            |r| r.get(0),
        )
        .unwrap_or(1);
    conn.execute(
        "INSERT INTO messages (conv_id, seq, role, content, meta, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            conv_id,
            next_seq,
            role,
            content,
            meta.map(|m| m.to_string()),
            now
        ],
    )?;
    conn.execute(
        "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
        params![now, conv_id],
    )?;
    Ok((conv_id.to_string(), next_seq))
}

/// Update an existing message's content and/or meta (phase 2 of the
/// two-phase persistence: fill in the assistant reply after the stream ends).
pub fn update_message(
    conv_id: &str,
    seq: i64,
    content: Option<&str>,
    meta: Option<&Value>,
) -> Result<()> {
    validate_id(conv_id)?;
    let conn = db::open()?;
    if content.is_some() {
        let n = conn.execute(
            "UPDATE messages SET content = ?1 WHERE conv_id = ?2 AND seq = ?3",
            params![content.unwrap_or(""), conv_id, seq],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("message {conv_id}/{seq}")));
        }
    }
    if let Some(m) = meta {
        let n = conn.execute(
            "UPDATE messages SET meta = ?1 WHERE conv_id = ?2 AND seq = ?3",
            params![m.to_string(), conv_id, seq],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("message {conv_id}/{seq}")));
        }
    }
    conn.execute(
        "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
        params![now_iso(), conv_id],
    )?;
    Ok(())
}

// ── Orchestration support: server-side history + summary persistence ──

/// History for the orchestrator: all completed messages minus the trailing
/// current-turn pair (user query + assistant placeholder appended by the
/// frontend before the call). Also marks any stale `generating` tail as
/// `interrupted` (crash recovery).
pub fn load_history_for_orchestrate(conv_id: &str, query: &str) -> Result<Vec<Value>> {
    validate_id(conv_id)?;
    let doc = load(conv_id)?; // also performs the interrupted-marking
    let Some(arr) = doc["messages"].as_array() else {
        return Ok(Vec::new());
    };
    let mut msgs: Vec<Value> = arr.clone();
    // Drop the trailing assistant placeholder(s) still marked generating or
    // interrupted (crash leftovers) — they carry no usable content.
    let mut drop_tail = 0usize;
    for m in arr.iter().rev() {
        if m["role"].as_str() == Some("assistant")
            && matches!(
                m["meta"]["status"].as_str(),
                Some("generating") | Some("interrupted")
            )
        {
            drop_tail += 1;
        } else {
            break;
        }
    }
    if drop_tail > 0 {
        msgs.truncate(msgs.len() - drop_tail);
    }
    // Drop the trailing user message when it is the current query.
    if let Some(last) = msgs.last() {
        if last["role"].as_str() == Some("user") && last["content"].as_str() == Some(query) {
            msgs.pop();
        }
    }
    // Strip meta/seq — the AI service only understands role/content.
    for m in msgs.iter_mut() {
        if let Some(obj) = m.as_object_mut() {
            obj.remove("seq");
            obj.remove("meta");
        }
    }
    Ok(msgs)
}

/// Current rolling summary + the number of leading messages it covers.
pub fn get_summary(conv_id: &str) -> Result<(Option<String>, i64)> {
    validate_id(conv_id)?;
    let conn = db::open()?;
    conn.query_row(
        "SELECT summary, summary_upto FROM conversations WHERE id = ?1",
        params![conv_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .map_err(|_| AppError::NotFound(format!("conversation '{conv_id}'")))
}

/// Persist a (re)computed rolling summary emitted by the AI service.
pub fn set_summary(conv_id: &str, text: &str, upto: i64) -> Result<()> {
    validate_id(conv_id)?;
    let conn = db::open()?;
    conn.execute(
        "UPDATE conversations SET summary = ?1, summary_upto = ?2, updated_at = ?3 WHERE id = ?4",
        params![text, upto, now_iso(), conv_id],
    )?;
    Ok(())
}

/// Generate a fresh conversation id (used by the frontend before the first
/// append when it needs to pre-allocate the id).
pub fn generate_id() -> String {
    new_id()
}
