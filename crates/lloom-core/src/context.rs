//! Provider-independent context budgeting and cache-scope helpers.
//!
//! This is the Rust owner of logic that used to live in `api/ai_service.py`.
//! Token counting deliberately keeps the CJK-aware estimate that the former
//! Python fallback used; provider-native tokenizers can be plugged in later
//! without changing callers.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

pub const DEFAULT_CONTEXT_BUDGET: usize = 24_000;
pub const DEFAULT_SUMMARY_BLOCK: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextStats {
    pub budget: usize,
    pub query_tokens: usize,
    pub history_total: usize,
    pub kept: usize,
    pub dropped: usize,
    pub summary_used: bool,
    pub needs_summary: bool,
    pub uncovered: usize,
}

#[derive(Debug, Clone)]
pub struct ContextWindow {
    pub messages: Vec<Value>,
    pub summary: String,
    pub summary_upto: usize,
    pub stats: ContextStats,
}

pub fn context_budget() -> usize {
    std::env::var("LLOOM_CONTEXT_BUDGET")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_CONTEXT_BUDGET)
}

pub fn summary_block() -> usize {
    std::env::var("LLOOM_SUMMARY_BLOCK")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_SUMMARY_BLOCK)
}

/// CJK-aware deterministic estimate: CJK ~= 1 token, other Unicode chars ~= 3.5/token.
pub fn count_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let mut cjk = 0usize;
    let mut other = 0usize;
    for ch in text.chars() {
        if ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    cjk + ((other as f64) / 3.5) as usize
}

pub fn build_context(
    query: &str,
    history: &[Value],
    summary: &str,
    summary_upto: usize,
    system_prompt: &str,
    budget: usize,
) -> ContextWindow {
    let query_tokens = count_tokens(query);
    let summary_tokens = count_tokens(summary);
    let available =
        budget.saturating_sub(query_tokens + count_tokens(system_prompt) + summary_tokens);

    let mut kept_rev = Vec::new();
    let mut used = 0usize;
    for message in history.iter().rev() {
        let tokens = count_tokens(message.get("content").and_then(Value::as_str).unwrap_or(""));
        if used + tokens > available {
            break;
        }
        kept_rev.push(message.clone());
        used += tokens;
    }
    kept_rev.reverse();
    let kept = kept_rev.len();
    let dropped = history.len().saturating_sub(kept);
    let upto = summary_upto.min(history.len());
    let uncovered = dropped.saturating_sub(upto);
    let needs_summary = uncovered > 0 && (summary.is_empty() || uncovered >= summary_block());

    let mut messages = vec![json!({"role": "system", "content": system_prompt})];
    if !summary.is_empty() {
        messages.push(json!({
            "role": "system",
            "content": format!("以下是本对话早前内容的摘要，供你参考：\n{summary}")
        }));
    }
    messages.extend(kept_rev);
    messages.push(json!({"role": "user", "content": query}));

    ContextWindow {
        messages,
        summary: summary.to_string(),
        summary_upto: upto,
        stats: ContextStats {
            budget,
            query_tokens,
            history_total: history.len(),
            kept,
            dropped,
            summary_used: !summary.is_empty(),
            needs_summary,
            uncovered,
        },
    }
}

fn anaphora() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"它|他|她|它们|他们|她们|这个|那个|这些|那些|上面|上文|刚才|之前|继续|再说|再讲|另外|那么|前面|上述|以上|接下来|这样|那样").unwrap())
}

fn time_sensitive() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"今天|现在|当前|最新|最近|(?i:today|now|latest)").unwrap())
}

pub fn is_context_free(query: &str, history: &[Value]) -> bool {
    history.is_empty() || (!anaphora().is_match(query) && query.chars().count() >= 12)
}

pub fn cacheable(query: &str, temperature: f64) -> bool {
    temperature <= 0.7 && !time_sensitive().is_match(query)
}

pub fn normalize_query(query: &str) -> String {
    query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub fn fingerprint(conversation_id: &str, history: &[Value]) -> String {
    let tail: String = history
        .iter()
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|m| {
            m.get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .chars()
                .take(80)
                .collect::<String>()
        })
        .collect();
    sha256_prefix(&format!("{conversation_id}:{tail}"), 16)
}

pub fn system_id(messages: &[Value]) -> String {
    messages
        .iter()
        .find(|m| m.get("role").and_then(Value::as_str) == Some("system"))
        .and_then(|m| m.get("content").and_then(Value::as_str))
        .map(|s| sha256_prefix(s, 16))
        .unwrap_or_else(|| "none".into())
}

pub fn exact_key(model: &str, system_id: &str, fingerprint: &str, query: &str) -> String {
    sha256_prefix(
        &format!(
            "{model}|{system_id}|{fingerprint}|{}",
            normalize_query(query)
        ),
        64,
    )
}

fn sha256_prefix(text: &str, chars: usize) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let hex = format!("{digest:x}");
    hex[..chars.min(hex.len())].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_keeps_newest_messages_within_budget() {
        let history = vec![
            json!({"role":"user", "content":"甲乙丙丁"}),
            json!({"role":"assistant", "content":"一二三四"}),
            json!({"role":"user", "content":"最新历史"}),
        ];
        let out = build_context("问题", &history, "", 0, "系统", 10);
        assert_eq!(out.stats.kept, 1);
        assert_eq!(out.stats.dropped, 2);
        assert_eq!(out.messages[1]["content"], "最新历史");
        assert!(out.stats.needs_summary);
    }

    #[test]
    fn cache_scope_is_conservative_for_followups() {
        let history = vec![json!({"role":"user", "content":"Rust 是什么"})];
        assert!(!is_context_free("继续", &history));
        assert!(is_context_free("请介绍量子色动力学的基本概念", &history));
        assert!(!cacheable("今天的新闻", 0.0));
        assert!(!cacheable("稳定问题", 0.8));
    }

    #[test]
    fn key_is_stable_under_whitespace_and_case() {
        let a = exact_key("m", "s", "", " Hello   WORLD ");
        let b = exact_key("m", "s", "", "hello world");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }
}
