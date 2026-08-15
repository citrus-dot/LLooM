//! Security layer — PII detection, jailbreak interception, domain classification.
//!
//! Pure-regex (zero-cost) tier ported from `core/security.py`. The LLM fallback
//! for domain classification is delegated to the AI service.

use crate::models::SecurityReport;
use fancy_regex::Regex;
use std::sync::OnceLock;

// ── PII patterns (7 types) ──

pub const PII_TYPES: [&str; 7] = [
    "EMAIL_ADDRESS",
    "PHONE_NUMBER",
    "US_SSN",
    "CREDIT_CARD",
    "IP_ADDRESS",
    "ID_CARD",
    "BANK_ACCOUNT",
];

const PII_PATTERNS: [(&str, &str); 7] = [
    ("EMAIL_ADDRESS", r"(?<![A-Za-z0-9._%+-])[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}(?![A-Za-z])"),
    ("PHONE_NUMBER", r"(?<!\d)(?:1[3-9]\d{9})(?!\d)|(?<!\d)(?:\+?1\s*(?:[.-]\s*)?(?:\(?\d{3}\)?[\s.-]?\d{3}[\s.-]?\d{4}))(?!\d)"),
    ("US_SSN", r"(?<!\d)\d{3}-\d{2}-\d{4}(?!\d)"),
    ("CREDIT_CARD", r"(?<!\d)(?:\d[ -]*?){13,16}(?!\d)"),
    ("IP_ADDRESS", r"(?<!\d)(?:(?:25[0-5]|2[0-4]\d|[01]?\d\d?)\.){3}(?:25[0-5]|2[0-4]\d|[01]?\d\d?)(?!\d)"),
    ("ID_CARD", r"(?<!\d)\d{17}[\dXx](?!\d)"),
    ("BANK_ACCOUNT", r"(?<![A-Z])[A-Z]{2}\d{2}[A-Z0-9]{10,30}(?![A-Z0-9])"),
];

const PII_MASKS: [(&str, &str); 7] = [
    ("EMAIL_ADDRESS", "[EMAIL]"),
    ("PHONE_NUMBER", "[PHONE]"),
    ("US_SSN", "[SSN]"),
    ("CREDIT_CARD", "[CARD]"),
    ("IP_ADDRESS", "[IP]"),
    ("ID_CARD", "[ID]"),
    ("BANK_ACCOUNT", "[BANK]"),
];

fn pii_regex() -> &'static [(Regex, &'static str, &'static str)] {
    static PII: OnceLock<Vec<(Regex, &'static str, &'static str)>> = OnceLock::new();
    PII.get_or_init(|| {
        PII_PATTERNS
            .iter()
            .map(|(name, pat)| {
                (
                    Regex::new(pat).expect("valid pii regex"),
                    *name,
                    PII_MASKS.iter().find(|(n, _)| n == name).map(|(_, m)| *m).unwrap_or("[REDACTED]"),
                )
            })
            .collect()
    })
}

// ── Jailbreak patterns ──

fn jailbreak_regex() -> &'static Vec<(Regex, &'static str)> {
    static JB: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    JB.get_or_init(|| {
        [
            (r"you\s+are\s+(now\s+)?(?:DAN|do\s+anything\s+now)", "DAN_ATTACK"),
            (r"ignore\s+(?:(?:previous|prior|above|all)\s+)+(?:instructions?|prompts?|rules?|guidelines?)", "INSTRUCTION_OVERRIDE"),
            (r"disregard\s+(?:(?:previous|prior|above|all)\s+)+(?:instructions?|prompts?)", "INSTRUCTION_OVERRIDE"),
            (r"forget\s+(?:everything|all|previous)", "INSTRUCTION_OVERRIDE"),
            (r"(?:pretend|act\s+as|roleplay)\s+(?:you\s+are\s+)?(?:a|an)?\s*(?:different|unrestricted|unfiltered|unlimited|free|evil|hacker)", "ROLE_MANIPULATION"),
            (r"you\s+are\s+now\s+(?:in\s+)?(?:developer|root|admin|god|unlimited)\s+mode", "ROLE_MANIPULATION"),
            (r"(?:no|without|remove|disable)\s+(?:safety|restrictions?|guidelines?|rules?|limits?|filters?|guardrails?)", "SAFETY_BYPASS"),
            (r"(?:bypass|circumvent|override)\s+(?:safety|security|content\s+filter)", "SAFETY_BYPASS"),
            (r"(?:system|admin|developer)\s+prompt\s*(?:is|:|=)", "PROMPT_INJECTION"),
            (r"reveal|show|print|output\s+(?:your|the)\s+(?:system\s+)?prompt", "PROMPT_INJECTION"),
            (r"(?<!\w)jailbreak(?!\w)", "JAILBREAK_KEYWORD"),
        ]
        .iter()
        .map(|(pat, t)| (Regex::new(pat).expect("valid jailbreak regex"), *t))
        .collect()
    })
}

// ── Domain keyword classification ──

fn domain_keywords() -> &'static Vec<(&'static str, &'static [&'static str])> {
    static DK: OnceLock<Vec<(&'static str, &'static [&'static str])>> = OnceLock::new();
    DK.get_or_init(|| {
        vec![
            ("math", &["calculate", "equation", "solve", "derivative", "integral", "algebra", "geometry", "calculus", "probability", "statistics", "theorem", "matrix", "数学", "计算", "方程", "微积分", "概率", "统计", "矩阵", "几何"][..]),
            ("physics", &["physics", "mechanics", "thermodynamics", "electromagnetism", "quantum", "relativity", "force", "energy", "velocity", "acceleration", "wavelength", "物理", "力学", "热力学", "电磁", "量子", "相对论", "速度", "加速度"][..]),
            ("chemistry", &["chemistry", "molecule", "reaction", "compound", "element", "bond", "organic", "inorganic", "acid", "base", "oxidation", "catalyst", "化学", "分子", "反应", "化合物", "元素", "催化剂", "氧化"][..]),
            ("biology", &["biology", "cell", "organism", "evolution", "genetics", "DNA", "RNA", "protein", "enzyme", "photosynthesis", "mitosis", "ecosystem", "生物", "细胞", "进化", "遗传", "蛋白质", "酶", "光合作用"][..]),
            ("computer_science", &["programming", "code", "algorithm", "data structure", "software", "database", "network", "compiler", "operating system", "API", "编程", "代码", "算法", "数据结构", "软件", "数据库", "网络"][..]),
            ("engineering", &["engineering", "circuit", "mechanical", "structural", "control system", "signal processing", "embedded", "CAD", "manufacturing", "工程", "电路", "机械", "结构", "制造", "嵌入式"][..]),
            ("business", &["business", "management", "marketing", "strategy", "finance", "investment", "revenue", "profit", "startup", "entrepreneur", "商业", "管理", "营销", "战略", "金融", "投资", "利润", "创业"][..]),
            ("law", &["law", "legal", "court", "contract", "regulation", "compliance", "statute", "liability", "intellectual property", "copyright", "法律", "法院", "合同", "法规", "合规", "知识产权", "版权"][..]),
            ("economics", &["economics", "market", "supply", "demand", "GDP", "inflation", "trade", "fiscal", "monetary", "macroeconomics", "microeconomics", "经济", "市场", "供给", "需求", "通胀", "贸易", "财政"][..]),
            ("health", &["health", "medical", "disease", "symptom", "treatment", "medicine", "patient", "diagnosis", "therapy", "drug", "vaccine", "健康", "医疗", "疾病", "症状", "治疗", "药物", "疫苗", "诊断"][..]),
            ("psychology", &["psychology", "behavior", "cognitive", "emotion", "mental", "therapy", "personality", "consciousness", "perception", "心理", "行为", "认知", "情绪", "意识", "感知"][..]),
            ("philosophy", &["philosophy", "ethics", "metaphysics", "existence", "consciousness", "morality", "nihilism", "existentialism", "rationalism", "哲学", "伦理", "道德", "存在", "形而上学"][..]),
            ("history", &["history", "historical", "ancient", "medieval", "modern", "century", "revolution", "empire", "dynasty", "war", "civilization", "历史", "古代", "中世纪", "现代", "革命", "帝国", "朝代"][..]),
        ]
    })
}

// ── Public API ──

pub fn detect_pii(text: &str) -> std::collections::HashMap<String, Vec<String>> {
    let mut findings = std::collections::HashMap::new();
    for (re, name, _mask) in pii_regex() {
        if let Ok(caps) = re.captures_iter(text).collect::<Result<Vec<_>, _>>() {
            for cap in caps {
                if let Some(m) = cap.get(0) {
                    let matched = m.as_str().to_string();
                    findings
                        .entry((*name).to_string())
                        .or_insert_with(Vec::new)
                        .push(matched);
                }
            }
        }
    }
    findings
}

pub fn mask_pii(text: &str, findings: &std::collections::HashMap<String, Vec<String>>) -> String {
    let mut out = text.to_string();
    for (name, matches) in findings {
        let mask = PII_MASKS.iter().find(|(n, _)| n == name).map(|(_, m)| *m).unwrap_or("[REDACTED]");
        for m in matches {
            out = out.replace(m, mask);
        }
    }
    out
}

pub fn detect_jailbreak(text: &str) -> Vec<(String, i64)> {
    let mut detections: Vec<(String, i64)> = Vec::new();
    for (re, attack_type) in jailbreak_regex() {
        let count = match re.find_iter(text).collect::<Result<Vec<_>, _>>() {
            Ok(matches) => matches.len() as i64,
            Err(_) => 0,
        };
        if count > 0 {
            detections.push((attack_type.to_string(), count));
        }
    }
    detections
}

/// Keyword-based domain classification (zero cost).
pub fn keyword_domain(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let mut scores: Vec<(&str, usize)> = Vec::new();
    for (domain, keywords) in domain_keywords() {
        let score = keywords.iter().filter(|kw| lower.contains(&kw.to_lowercase())).count();
        if score > 0 {
            scores.push((domain, score));
        }
    }
    scores.sort_by(|a, b| b.1.cmp(&a.1));
    scores.first().map(|(d, _)| (*d).to_string())
}

/// Extract the last user message text.
pub fn extract_user_text(messages: &[serde_json::Value]) -> String {
    for msg in messages.iter().rev() {
        if msg.get("role").and_then(|r| r.as_str()) == Some("user") {
            let content = msg.get("content").cloned().unwrap_or(serde_json::Value::Null);
            if let Some(s) = content.as_str() {
                return s.to_string();
            }
            if let Some(parts) = content.as_array() {
                return parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join(" ");
            }
        }
    }
    String::new()
}

/// Full security check: PII masking/block + jailbreak block + domain classify.
///
/// `block_pii` / `block_jailbreak` mirror the config actions. Returns a
/// SecurityReport; if `blocked` is true the caller must reject the request.
pub fn check(text: &str, block_pii: bool, block_jailbreak: bool) -> SecurityReport {
    let mut report = SecurityReport {
        processed_text: text.to_string(),
        ..Default::default()
    };

    // PII
    let pii = detect_pii(text);
    if !pii.is_empty() {
        let summary: serde_json::Value = pii
            .iter()
            .map(|(k, v)| serde_json::json!({ "type": k, "count": v.len() }))
            .collect();
        report.pii = serde_json::json!({ "detected": summary });
        if block_pii {
            report.blocked = true;
            report.block_reason = "PII_DETECTED".to_string();
            return report;
        }
        report.processed_text = mask_pii(text, &pii);
    }

    // Jailbreak
    let jb = detect_jailbreak(text);
    if !jb.is_empty() {
        report.jailbreak = serde_json::json!({
            "detected": jb.iter().map(|(t, c)| serde_json::json!({"type": t, "count": c})).collect::<Vec<_>>()
        });
        if block_jailbreak {
            report.blocked = true;
            report.block_reason = "JAILBREAK".to_string();
            return report;
        }
    }

    // Domain (keyword tier; LLM fallback happens in the caller)
    if let Some(domain) = keyword_domain(text) {
        report.domain = domain;
        report.domain_method = "keyword".to_string();
    }

    report
}
