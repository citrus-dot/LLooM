//! Provider-independent orchestration decisions migrated from Python.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubTask {
    pub id: usize,
    pub description: String,
    pub task_type: String,
    pub depends_on: Vec<usize>,
    pub estimated_output_tokens: usize,
}

fn complex_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"(然后|接着|之后|最后).{2,}",
            r"(第[一二三四五1-5]步|(?i:step\s?\d))",
            r"(同时|并且|此外|另外)",
            r"(对比|比较|分析|评估).+(和|与|跟|(?i:vs))",
            r"(首先|其次|然后|再次|最后|第一|第二|第三|第四|第五)",
            r"(?m)^\s*(?:\d+[\.、]|[-*])\s+\S.*(?:\n.*)^\s*(?:\d+[\.、]|[-*])\s+\S",
            r"(分别|各自|逐一).{2,}(说明|分析|列出|给出|介绍|总结|处理)",
            r"(权衡|优缺点|利弊|方案).{2,}(对比|比较|选择)",
        ]
        .into_iter()
        .map(|p| Regex::new(p).unwrap())
        .collect()
    })
}

pub fn is_complex(query: &str) -> bool {
    if complex_patterns().iter().any(|p| p.is_match(query)) || query.chars().count() > 100 {
        return true;
    }
    let sentence_count = query
        .split(|c| "。！？.!?".contains(c))
        .filter(|s| !s.trim().is_empty())
        .count();
    sentence_count > 2 || is_comparison(query)
}

fn is_comparison(query: &str) -> bool {
    static KW: OnceLock<Regex> = OnceLock::new();
    static SEP: OnceLock<Regex> = OnceLock::new();
    let kw = KW
        .get_or_init(|| Regex::new(r"比较|对比|对照|区别|差异|异同|优缺点|利弊|(?i:vs)").unwrap());
    if !kw.is_match(query) {
        return false;
    }
    SEP.get_or_init(|| Regex::new(r"[、，,；;／/\s]+|和|与|跟|(?i:vs)").unwrap())
        .split(query)
        .filter(|s| s.trim().chars().count() >= 2)
        .count()
        >= 2
}

pub fn fallback_decompose(query: &str) -> Vec<SubTask> {
    static ITEM: OnceLock<Regex> = OnceLock::new();
    let item = ITEM.get_or_init(|| {
        Regex::new(r"(?m)^\s*(?:\d+[\.、]|[一二三四五六七八九十]+[、.]|[-*])\s+").unwrap()
    });
    let mut parts: Vec<String> = item
        .split(query)
        .map(|s| {
            s.trim_matches(|c: char| c.is_whitespace() || c == '-' || c == '*')
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() < 2 {
        parts = query
            .split(|c| "。！？.!?".contains(c))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
    }
    parts
        .into_iter()
        .enumerate()
        .map(|(i, description)| SubTask {
            id: i + 1,
            description,
            task_type: "general".into(),
            depends_on: Vec::new(),
            estimated_output_tokens: 300,
        })
        .collect()
}

/// Parse the constrained JSON array returned by the decomposition model.
/// Invalid rows are skipped and dependencies are normalised to positive ids.
pub fn parse_decomposition(text: &str) -> Vec<SubTask> {
    let cleaned = text.replace("```json", "").replace("```", "");
    let Some(start) = cleaned.find('[') else {
        return Vec::new();
    };
    let Some(end) = cleaned.rfind(']') else {
        return Vec::new();
    };
    let Ok(rows) = serde_json::from_str::<Vec<serde_json::Value>>(&cleaned[start..=end]) else {
        return Vec::new();
    };
    rows.into_iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let description = row.get("description")?.as_str()?.trim().to_string();
            if description.is_empty() {
                return None;
            }
            let id = row
                .get("id")
                .and_then(serde_json::Value::as_u64)
                .map(|v| v as usize)
                .unwrap_or(index + 1);
            let depends_on = row
                .get("depends_on")
                .and_then(serde_json::Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(serde_json::Value::as_u64)
                        .filter(|v| *v > 0)
                        .map(|v| v as usize)
                        .collect()
                })
                .unwrap_or_default();
            Some(SubTask {
                id,
                description,
                task_type: row
                    .get("task_type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("general")
                    .to_string(),
                depends_on,
                estimated_output_tokens: row
                    .get("estimated_output_tokens")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(1024) as usize,
            })
        })
        .collect()
}

/// Stable dependency waves; cycles are broken by scheduling the first pending task.
pub fn dependency_waves(tasks: &[SubTask]) -> Vec<Vec<usize>> {
    let mut pending: Vec<&SubTask> = tasks.iter().collect();
    let mut settled = std::collections::HashSet::new();
    let mut waves = Vec::new();
    while !pending.is_empty() {
        let mut ready: Vec<&SubTask> = pending
            .iter()
            .copied()
            .filter(|t| t.depends_on.iter().all(|d| settled.contains(d)))
            .collect();
        if ready.is_empty() {
            ready.push(pending[0]);
        }
        for task in &ready {
            settled.insert(task.id);
        }
        waves.push(ready.iter().map(|t| t.id).collect());
        pending.retain(|t| !ready.iter().any(|r| r.id == t.id));
    }
    waves
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_and_splits_numbered_work() {
        let q = "1. 分析需求\n2. 编写实现\n3. 运行测试";
        assert!(is_complex(q));
        let tasks = fallback_decompose(q);
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[1].description, "编写实现");
    }

    #[test]
    fn builds_dependency_waves() {
        let task = |id, deps| SubTask {
            id,
            description: id.to_string(),
            task_type: "general".into(),
            depends_on: deps,
            estimated_output_tokens: 1,
        };
        let tasks = vec![task(1, vec![]), task(2, vec![]), task(3, vec![1, 2])];
        assert_eq!(dependency_waves(&tasks), vec![vec![1, 2], vec![3]]);
    }

    #[test]
    fn parses_fenced_decomposition_json() {
        let tasks = parse_decomposition(
            "```json\n[{\"id\":1,\"description\":\"调查\",\"task_type\":\"general\",\"depends_on\":[],\"estimated_output_tokens\":200},{\"id\":2,\"description\":\"实现\",\"depends_on\":[1]}]\n```",
        );
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[1].depends_on, vec![1]);
        assert_eq!(tasks[1].estimated_output_tokens, 1024);
    }
}
