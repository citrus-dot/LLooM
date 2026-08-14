//! Smart routing — task classification (regex tier) + cost-optimal model selection.
//!
//! Ported from `core/smart_router.py` and `core/orchestrator.py` (the regex and
//! model-preference logic). The LLM fallback classification is delegated to the
//! AI service via `ai_client::classify`.

use crate::ai_client::ModelSpec;
use crate::models::RoutingDecision;
use regex::Regex;
use std::sync::OnceLock;

// ── Task → model mapping ──

pub const TASK_MODEL_MAP: [(&str, &str); 5] = [
    ("simple_qa", "qwen2.5-local"),
    ("general", "qwen-plus"),
    ("coding", "deepseek-v3"),
    ("math_logic", "deepseek-v3"),
    ("complex_reasoning", "qwen3.6-plus"),
];

pub const INFERENCE_MODELS: [&str; 4] = ["qwen3.6-flash", "qwen3.6-plus", "qwen3-max", "deepseek-v3"];

const VALID_TASK_TYPES: [&str; 5] = ["simple_qa", "general", "coding", "math_logic", "complex_reasoning"];

// ── Regex rules (priority: complex > coding > math > simple_qa) ──

fn task_rules() -> &'static Vec<(&'static str, Vec<Regex>)> {
    static RULES: OnceLock<Vec<(&'static str, Vec<Regex>)>> = OnceLock::new();
    RULES.get_or_init(|| {
        let rules: [(&str, &[&str]); 4] = [
            (
                "complex_reasoning",
                &[
                    r"(分析|analyze|对比|compare|评估|evaluate)",
                    r"(方案|plan|策略|strategy|架构|architecture)",
                    r"(论文|paper|研究|research|综述|review)",
                ][..],
            ),
            (
                "coding",
                &[
                    r"(写代码|write code|implement|函数|function|class|bug|debug)",
                    r"(python|java|javascript|go|rust|c\+\+|sql)",
                    r"(api|endpoint|refactor|优化|重构)",
                ][..],
            ),
            (
                "math_logic",
                &[
                    r"(数学|math|计算|calculate|方程|equation)",
                    r"(逻辑|logic|推理|reason|证明|prove)",
                    r"(概率|probability|统计|statistics)",
                ][..],
            ),
            (
                "simple_qa",
                &[
                    r"^(你好|hi|hello|在吗)",
                    r"(天气|时间|日期)",
                    r"(翻译|translate)",
                ][..],
            ),
        ];
        rules
            .iter()
            .map(|(name, pats)| {
                (*name, pats.iter().map(|p| Regex::new(p).expect("valid rule regex")).collect())
            })
            .collect()
    })
}

fn default_model_for_task(task_type: &str) -> String {
    TASK_MODEL_MAP
        .iter()
        .find(|(t, _)| *t == task_type)
        .map(|(_, m)| m.to_string())
        .unwrap_or_else(|| "qwen2.5-local".to_string())
}

// ── Complexity detection (from orchestrator) ──

fn complexity_regex() -> &'static Vec<Regex> {
    static CR: OnceLock<Vec<Regex>> = OnceLock::new();
    CR.get_or_init(|| {
        [
            r"(然后|接着|再|之后|最后).{2,}",
            r"(第[一二三四五1-5]步|Step\s?\d)",
            r"(同时|并且|此外|另外)",
            r"(对比|比较|分析|评估).+(和|与|跟|vs)",
            r"(写|实现|开发).+(并|然后|接着).*(测试|验证|部署)",
            r"(翻译|总结|摘要).+(并|然后).+(分析|评论)",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("valid complexity regex"))
        .collect()
    })
}

pub fn is_complex(query: &str) -> bool {
    for re in complexity_regex() {
        if re.is_match(query) {
            return true;
        }
    }
    if query.chars().count() > 100 {
        return true;
    }
    let sentences: Vec<&str> = query.split(['。', '！', '？', '.', '!', '?'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    sentences.len() > 2
}

// ── Cost-optimal selection ──

pub fn task_model_preference(task_type: &str) -> &'static [&'static str] {
    match task_type {
        "simple_qa" => &["qwen2.5-local", "qwen3.6-flash", "qwen-plus"],
        "general" => &["qwen-plus", "qwen3.6-flash", "qwen2.5-local"],
        "coding" => &["deepseek-v3", "qwen-plus", "qwen2.5-local"],
        "math_logic" => &["deepseek-v3", "qwen-plus", "qwen3.6-plus"],
        "complex_reasoning" => &["qwen3.6-plus", "deepseek-v3", "qwen-plus"],
        _ => &["qwen-plus"],
    }
}

/// Pick the cheapest available model for a task type.
pub fn select_model(task_type: &str, available: &[ModelSpec]) -> String {
    let available_names: Vec<&str> = available.iter().map(|m| m.name.as_str()).collect();
    for model in task_model_preference(task_type) {
        if available_names.is_empty() || available_names.contains(&model) {
            return model.to_string();
        }
    }
    "qwen2.5-local".to_string()
}

// ── Classification ──

/// Regex-tier classification. Returns task type or None.
pub fn rule_classify(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    if lower.trim().is_empty() {
        return None;
    }
    for (task_type, patterns) in task_rules() {
        for re in patterns {
            if re.is_match(&lower) {
                return Some(task_type.to_string());
            }
        }
    }
    None
}

/// Hybrid classification: regex first, LLM fallback second.
///
/// `classifier` is the model spec used for the LLM fallback. Returns
/// (task_type, method, selected_model).
pub async fn classify(text: &str, classifier: Option<&ModelSpec>) -> (String, String, String) {
    if let Some(task) = rule_classify(text) {
        return (task.clone(), "rule".to_string(), default_model_for_task(&task));
    }
    // LLM fallback
    let task = match classifier {
        Some(spec) => crate::ai_client::classify(text, spec, &VALID_TASK_TYPES).await,
        None => "general".to_string(),
    };
    (task.clone(), "llm".to_string(), default_model_for_task(&task))
}

/// Build a routing decision for a request.
///
/// `model` may be "auto" (classify) or an explicit model name (direct).
pub async fn route(model: &str, user_text: &str, classifier: Option<&ModelSpec>) -> RoutingDecision {
    if model == "auto" || model == "auto-route" {
        let (task_type, method, selected) = classify(user_text, classifier).await;
        let is_inference = INFERENCE_MODELS.contains(&selected.as_str());
        RoutingDecision {
            model: selected,
            task_type,
            method,
            stream: is_inference,
        }
    } else {
        RoutingDecision {
            model: model.to_string(),
            task_type: "direct".to_string(),
            method: "direct".to_string(),
            stream: INFERENCE_MODELS.contains(&model),
        }
    }
}

// ── Domain enhancement ──

pub fn enhance_with_domain(task_type: &str, sr_domain: &str) -> (String, bool) {
    if sr_domain.is_empty() {
        return (task_type.to_string(), false);
    }
    match sr_domain {
        "math" | "physics" | "chemistry" | "biology" => {
            if task_type != "math_logic" && task_type != "complex_reasoning" {
                return ("math_logic".to_string(), true);
            }
        }
        "computer_science" | "engineering" => {
            if task_type != "coding" && task_type != "complex_reasoning" {
                return ("coding".to_string(), true);
            }
        }
        _ => {}
    }
    (task_type.to_string(), false)
}
