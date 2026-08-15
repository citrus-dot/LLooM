//! Async REST client for the LLooM server (:7861).
//! All data flows through the REST API — no direct lloom-core calls.

use reqwest::Client;
use serde_json::Value;

const BASE: &str = "http://localhost:7861";

pub fn client() -> Client {
    Client::new()
}

pub async fn get(path: &str) -> Result<Value, String> {
    let res = client()
        .get(format!("{BASE}{path}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()));
    }
    res.json().await.map_err(|e| e.to_string())
}

pub async fn post(path: &str, body: Value) -> Result<Value, String> {
    let res = client()
        .post(format!("{BASE}{path}"))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()));
    }
    res.json().await.map_err(|e| e.to_string())
}

pub async fn put(path: &str, body: Value) -> Result<Value, String> {
    let res = client()
        .put(format!("{BASE}{path}"))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()));
    }
    res.json().await.map_err(|e| e.to_string())
}

pub async fn delete(path: &str) -> Result<Value, String> {
    let res = client()
        .delete(format!("{BASE}{path}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()));
    }
    res.json().await.map_err(|e| e.to_string())
}

/// Fetch the SSE body of an endpoint (chat / orchestrate streams).
pub async fn sse_text(path: &str, body: Value) -> Result<String, String> {
    let res = client()
        .post(format!("{BASE}{path}"))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()));
    }
    res.text().await.map_err(|e| e.to_string())
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/' {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}
