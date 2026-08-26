//! Signal layer — named heuristics feeding the routing pipeline
//! (ROUTING-PLAN §4.3 / PRICING-PLAN §5.3).
//!
//! Signals only answer "what do we observe"; they never decide. Each signal is
//! a pure function over request context, so it can be unit-tested in isolation
//! and toggled by config later.

use serde_json::Value;

/// 前缀稳定度：检测同会话相邻请求的 prompt 前缀是否漂移。
///
/// 背景：百炼/DeepSeek 的隐式缓存按「字节级稳定的前缀」命中；如果每轮
/// 重排工具列表或在 system prompt 里插入时间戳，前缀漂移 → 缓存全废。
///
/// 返回 `(drift, Some(prefix_hash))`：
/// - `drift`：0.0 = 与前一轮前缀完全一致（稳定）；1.0 = 发生漂移。
///   首轮（无参照）返回 0.0 且带出当前哈希供下一轮比对。
/// - `Some(prefix_hash)`：当前前缀的 fnv1a 哈希，供下一轮传入。
pub fn prefix_stability(
    messages: &[Value],
    prev_prefix_hash: Option<u64>,
) -> (f64, Option<u64>) {
    let prefix = normalize_prefix(messages, 512);
    let curr_hash = fnv1a(prefix.as_bytes());
    let drift = match prev_prefix_hash {
        Some(h) if h == curr_hash => 0.0,
        Some(_) => 1.0,
        None => 0.0, // 首轮无参照，不算漂移
    };
    (drift, Some(curr_hash))
}

/// 提取稳定前缀文本：只拼 `role == "system"` 的消息（工具定义/系统指令通常在此），
/// 最多 `approx_tokens`（近似 4 字符/token）。用户/助手消息是尾部变化内容，不参与
/// 前缀判定——前缀漂移检测只关心"本应稳定的部分"是否被改动。
pub fn normalize_prefix(messages: &[Value], approx_tokens: usize) -> String {
    let mut out = String::new();
    let char_budget = approx_tokens.saturating_mul(4);
    let mut used = 0usize;
    for m in messages {
        if used >= char_budget {
            break;
        }
        if m.get("role").and_then(|v| v.as_str()) != Some("system") {
            continue;
        }
        let content = match m.get("content") {
            Some(Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
            None => String::new(),
        };
        let take = content.chars().take(char_budget - used);
        let taken: String = take.collect();
        out.push_str(&taken);
        used += taken.chars().count();
        out.push('\n');
    }
    out
}

/// fnv1a-64 哈希（零依赖，稳定跨进程）
pub fn fnv1a(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut h = OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn msg(role: &str, content: &str) -> Value {
        json!({ "role": role, "content": content })
    }

    #[test]
    fn identical_prefix_no_drift() {
        let m1 = vec![msg("system", "你是助手。"), msg("user", "你好")];
        let m2 = vec![msg("system", "你是助手。"), msg("user", "你好，再讲一遍")];
        let (_, h1) = prefix_stability(&m1, None);
        let (drift, _) = prefix_stability(&m2, Some(h1.unwrap()));
        assert_eq!(drift, 0.0, "相同前缀不应视为漂移");
    }

    #[test]
    fn changed_prefix_drifts() {
        let m1 = vec![msg("system", "你是助手。"), msg("user", "你好")];
        let m2 = vec![msg("system", "你是不同的助手。"), msg("user", "你好")];
        let (_, h1) = prefix_stability(&m1, None);
        let (drift, _) = prefix_stability(&m2, Some(h1.unwrap()));
        assert_eq!(drift, 1.0, "system 前缀变化应标记漂移");
    }

    #[test]
    fn tail_change_does_not_drift() {
        let m1 = vec![msg("system", "你是助手。"), msg("user", "今天天气？")];
        let m2 = vec![msg("system", "你是助手。"), msg("user", "明天天气？")];
        let (_, h1) = prefix_stability(&m1, None);
        let (drift, _) = prefix_stability(&m2, Some(h1.unwrap()));
        assert_eq!(drift, 0.0, "仅尾部用户消息变化（512-token 前缀外）不应漂移");
    }

    #[test]
    fn first_call_no_reference() {
        let m = vec![msg("user", "hi")];
        let (drift, h) = prefix_stability(&m, None);
        assert_eq!(drift, 0.0);
        assert!(h.is_some());
    }

    #[test]
    fn fnv1a_stable_and_distinct() {
        assert_eq!(fnv1a(b"abc"), fnv1a(b"abc"));
        assert_ne!(fnv1a(b"abc"), fnv1a(b"abd"));
        // 已知值回归（fnv1a-64("abc") 手工验证值）
        assert_eq!(fnv1a(b"abc"), 0xe71fa2190541574b);
    }
}
