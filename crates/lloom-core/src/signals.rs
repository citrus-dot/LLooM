//! Signal layer — named heuristics feeding the routing pipeline
//! (ROUTING-PLAN §4.3 / PRICING-PLAN §5.3).
//!
//! Signals only answer "what do we observe"; they never decide. Each signal is
//! a pure function over request context, so it can be unit-tested in isolation
//! and toggled by config later.

use crate::db;
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

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

// ── P0.g 信号集：困难度信号 + 难度带投影 + reask/LLM 判定（可配置、可单测） ──
//
// SignalSet 只回答「观测到了什么」，不决策。`extract` 读 settings KV 去重阈值，
// 纯计算路径 `compute` 不触 DB，便于单测。本阶段不追求信号齐备，
// 只求命名、可配置、有单测（含 band 边界与 reask 判定）。

/// 单个请求观测到的信号集。
#[derive(Debug, Clone)]
pub struct SignalSet {
    /// 启发式任务类型（仅正则快路径，不触发 LLM；None 表示需 LLM 分类的灰区）
    pub task_type: Option<String>,
    /// 综合困难度 0..1（structure/complexity/context 加权）
    pub difficulty: f64,
    /// 难度带 easy/medium/hard（由 band_easy/band_medium 阈值投影）
    pub band: String,
    pub needs_tools: bool,
    pub needs_vision: bool,
    /// 输入 token 粗估（近似 0.6 token/字符，P5.c 精确预算器落地前）
    pub context_tokens: u32,
    /// 预算档预留，本阶段恒 "normal"（P5 接线）
    pub budget_tier: &'static str,
    /// (signal_name, contribution) 溯源，供审计/调参
    pub reasons: Vec<(&'static str, f64)>,
}

/// 困难度各分量的权重（settings 可覆盖，默认见计划 §P0.g）。
pub struct DifficultyWeights {
    pub structure: f64,
    pub complexity: f64,
    pub context: f64,
    pub embedding: f64,
}

impl Default for DifficultyWeights {
    fn default() -> Self {
        Self { structure: 0.3, complexity: 0.3, context: 0.2, embedding: 0.2 }
    }
}

/// 难度带投影（纯函数，band 边界可单测）
pub fn band_from(difficulty: f64, band_easy: f64, band_medium: f64) -> &'static str {
    if difficulty < band_easy {
        "easy"
    } else if difficulty < band_medium {
        "medium"
    } else {
        "hard"
    }
}

/// 语义缓存命中是否「不自信到需要 reask」：`sim < sim_threshold` → 真。
/// 高相似（≥阈值）信任缓存不放行 reask；低相似则宁可信其不准。
pub fn reask_decision(sim: f64, sim_threshold: f64) -> bool {
    sim < sim_threshold
}

/// 启发式 task_type 为 None 且难度落在灰区 → 需要 LLM 分类兜底
/// （灰区 = 不是显然简单，也不是显然困难）。
pub fn llm_classify_needed(signal: &SignalSet, confidence_floor: f64) -> bool {
    signal.task_type.is_none()
        && signal.difficulty >= confidence_floor
        && signal.difficulty <= 1.0 - confidence_floor
}

fn structure_rx() -> &'static [Regex] {
    static RX: OnceLock<Vec<Regex>> = OnceLock::new();
    RX.get_or_init(|| {
        [
            r"(然后|接着|再|之后|最后).{2,}",
            r"(第[一二三四五1-5]步|Step\s?\d)",
            r"(同时|并且|此外|另外)",
            r"(对比|比较|分析|评估).+(和|与|跟|vs)",
            r"(写|实现|开发).+(并|然后|接着).*(测试|验证|部署)",
        ]
        .iter()
        .map(|p| Regex::new(p).unwrap())
        .collect()
    })
}

fn complexity_rx() -> &'static [Regex] {
    static RX: OnceLock<Vec<Regex>> = OnceLock::new();
    RX.get_or_init(|| {
        [
            r"(权衡|优缺点|利弊|方案).*(对比|比较|选择|分析)",
            r"(分别|各自|逐一).{2,}(说明|分析|列出|给出|介绍|总结|处理)",
            r"(分析|analyze|对比|compare|评估|evaluate|综述|research|论文|paper)",
            r"(推理|reason|证明|prove|逻辑|logic)",
            r"(多步|多重|多个|多项).{1,}(任务|步骤|方面|模块)",
        ]
        .iter()
        .map(|p| Regex::new(p).unwrap())
        .collect()
    })
}

fn tools_rx() -> &'static [Regex] {
    static RX: OnceLock<Vec<Regex>> = OnceLock::new();
    RX.get_or_init(|| {
        [
            // CJK 项不加 \b（\b 是 ASCII 词边界，两个 CJK 字符之间不生效）
            r"(工具|调用函数|函数调用|运行代码|执行脚本|执行命令|终端|命令行)",
            r"\b(terminal|shell|tool|call|run|execute)\b",
        ]
        .iter()
        .map(|p| Regex::new(p).unwrap())
        .collect()
    })
}

fn vision_rx() -> &'static Regex {
    static RX: OnceLock<Regex> = OnceLock::new();
    RX.get_or_init(|| Regex::new(r"(图片|图像|看图|截图|照片|视觉|画面|image|picture|screenshot|photo|ocr)").unwrap())
}

fn compare_rx() -> &'static Regex {
    static RX: OnceLock<Regex> = OnceLock::new();
    RX.get_or_init(|| Regex::new(r"(比较|对比|对照|区别|差异|异同|优缺点|利弊|vs|VS|Vs)").unwrap())
}

fn entity_sep() -> &'static Regex {
    static RX: OnceLock<Regex> = OnceLock::new();
    RX.get_or_init(|| Regex::new(r"[、，,；;／/\s]+|(?:和|与|跟|vs|VS|Vs)").unwrap())
}

/// 结构复杂度：句子数 / 编号项 / 多对象比较 → 0..1
fn structure_score(text: &str) -> f64 {
    let mut score = 0.0;
    let sentences = text
        .split(['。', '！', '？', '.', '!', '?', '\n'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .count();
    if sentences > 1 {
        score += 0.3 * (sentences.min(5) as f64 / 5.0);
    }
    let numbered = text
        .lines()
        .filter(|l| Regex::new(r"^\s*\d+[\.、]").unwrap().is_match(l))
        .count();
    if numbered >= 2 {
        score += 0.4;
    }
    // 多对象比较：比较关键词 + ≥2 个并列实体
    if compare_rx().is_match(text) {
        let ents: Vec<&str> = entity_sep()
            .split(text)
            .map(|e| e.trim())
            .filter(|e| e.chars().count() >= 2)
            .collect();
        if ents.len() >= 2 {
            score += 0.4;
        }
    }
    score.clamp(0.0, 1.0)
}

/// 语义复杂度：分解/推理关键词 + 长度 + 句子数 → 0..1
fn complexity_score(text: &str) -> f64 {
    let mut score: f64 = 0.0;
    if complexity_rx().iter().any(|r| r.is_match(text)) {
        score += 0.6;
    }
    let chars = text.chars().count();
    if chars > 100 {
        score += 0.3;
    } else if chars > 40 {
        score += 0.1;
    }
    let sentences = text
        .split(['。', '！', '？', '.', '!', '?'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .count();
    if sentences > 2 {
        score += 0.2;
    }
    score.clamp(0.0, 1.0)
}

/// 上下文复杂度：长度相对 32K 窗口 → 0..1
fn context_score(text: &str) -> f64 {
    let tokens = est_tokens(text);
    (tokens as f64 / 32768.0).clamp(0.0, 1.0)
}

/// 输入 token 粗估（中英混合 ~0.6 token/字符）
pub fn est_tokens(text: &str) -> u32 {
    (text.chars().count() as f32 * 0.6) as u32
}

/// 启发式任务类型（正则快路径，purity：不触 DB / 不触发 LLM 分类）
fn heuristic_task_type(text: &str) -> Option<String> {
    let s = structure_rx();
    let low = text.to_lowercase();
    // 复杂>编码>数学>简单 的优先级顺序，与 router::rule_classify 一致取首命中
    for (name, pats) in [
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
            &[r"(你好|hi|hello|在吗)", r"(天气|时间|日期)", r"(翻译|translate)"][..],
        ),
    ] {
        for p in pats {
            if s.iter().any(|_| false) {
                break;
            }
            if Regex::new(p).unwrap().is_match(&low) {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// 纯计算路径（不触 DB）：给定权重与阈值 → 信号集。单测入口。
pub fn compute(
    user_text: &str,
    w: &DifficultyWeights,
    band_easy: f64,
    band_medium: f64,
) -> SignalSet {
    let structure = structure_score(user_text);
    let complexity = complexity_score(user_text);
    let context = context_score(user_text);
    let embedding = 0.0; // 语义缓存未初始化时贡献 0，不阻塞快路径
    let difficulty = (w.structure * structure
        + w.complexity * complexity
        + w.context * context
        + w.embedding * embedding)
        .clamp(0.0, 1.0);
    SignalSet {
        task_type: heuristic_task_type(user_text),
        band: band_from(difficulty, band_easy, band_medium).to_string(),
        needs_tools: tools_rx().iter().any(|r| r.is_match(user_text)),
        needs_vision: vision_rx().is_match(user_text),
        context_tokens: est_tokens(user_text),
        budget_tier: "normal",
        reasons: vec![
            ("structure", structure),
            ("complexity", complexity),
            ("context", context),
            ("embedding", embedding),
        ],
        difficulty,
    }
}

fn cfg_f64(key: &str, default: f64) -> f64 {
    db::get_setting(key)
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// 生产入口：读 settings 阈值后走纯计算。history/conv_id 预留（P5 信号用）。
pub fn extract(user_text: &str, _history: &[Value], _conv_id: Option<&str>) -> SignalSet {
    let w = DifficultyWeights {
        structure: cfg_f64("signal.difficulty.weights.structure", 0.3),
        complexity: cfg_f64("signal.difficulty.weights.complexity", 0.3),
        context: cfg_f64("signal.difficulty.weights.context", 0.2),
        embedding: cfg_f64("signal.difficulty.weights.embedding", 0.2),
    };
    let easy = cfg_f64("signal.band.easy", 0.33);
    let medium = cfg_f64("signal.band.medium", 0.66);
    compute(user_text, &w, easy, medium)
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

    // ── P0.g 单测：难度带投影 / reask / LLM 分类判定 / compute ──

    #[test]
    fn band_from_boundaries() {
        let (e, m) = (0.33, 0.66);
        assert_eq!(band_from(0.00, e, m), "easy");
        assert_eq!(band_from(0.32, e, m), "easy");
        assert_eq!(band_from(0.33, e, m), "medium", "下边界归 medium（easy 是 < 阈值）");
        assert_eq!(band_from(0.65, e, m), "medium");
        assert_eq!(band_from(0.66, e, m), "hard", "下边界归 hard（medium 是 < 阈值）");
        assert_eq!(band_from(1.00, e, m), "hard");
    }

    #[test]
    fn reask_decision_uses_threshold() {
        assert!(reask_decision(0.70, 0.80), "低相似 → 不自信 → reask");
        assert!(!reask_decision(0.90, 0.80), "高相似 → 信任缓存 → 不 reask");
        assert!(!reask_decision(0.80, 0.80), "恰好相等不 reask（< 阈值）");
    }

    #[test]
    fn llm_classify_needed_only_in_gray_zone() {
        let gray = SignalSet {
            task_type: None,
            difficulty: 0.5,
            band: "medium".into(),
            needs_tools: false,
            needs_vision: false,
            context_tokens: 0,
            budget_tier: "normal",
            reasons: vec![],
        };
        assert!(llm_classify_needed(&gray, 0.2), "灰区 → 需 LLM 分类");

        let mut certain = gray.clone();
        certain.difficulty = 0.95;
        assert!(!llm_classify_needed(&certain, 0.2), "显然困难 → 不需 LLM 分类");

        let mut h = gray.clone();
        h.task_type = Some("coding".into());
        assert!(!llm_classify_needed(&h, 0.2), "启发式已识别 → 不需 LLM 分类");
    }

    #[test]
    fn compute_complex_medium_band() {
        let w = DifficultyWeights::default();
        // 含「分析+比较优缺点」→ 命中 complex_reasoning 优先；难度落在 medium，而非 easy
        let s = compute("帮我写一个 Rust 函数，实现 SQL 查询分析，并比较两种做法的优缺点，然后给出推荐。", &w, 0.33, 0.66);
        assert_eq!(s.task_type.as_deref(), Some("complex_reasoning"));
        assert_eq!(s.band, "medium", "比较类任务应为 medium 而非 easy");
        assert!(s.difficulty >= 0.30);
        assert!(s.context_tokens > 0);
    }

    #[test]
    fn compute_easy_band() {
        let w = DifficultyWeights::default();
        let s = compute("你好", &w, 0.33, 0.66);
        assert_eq!(s.task_type.as_deref(), Some("simple_qa"));
        assert_eq!(s.band, "easy");
    }

    #[test]
    fn compute_vision_and_tools_flags() {
        let w = DifficultyWeights::default();
        let s = compute("分析这张截图里的图表，并调用工具算出总和", &w, 0.33, 0.66);
        assert!(s.needs_vision);
        assert!(s.needs_tools);
    }
}
