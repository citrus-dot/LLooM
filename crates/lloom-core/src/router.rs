//! Smart routing — 信号投影 band + 注册表驱动评分路由（ROUTING-PLAN P0.d）。
//!
//! 分类层（正则 + LLM 兜底）保留；模型选择不再用任何硬编码映射表，
//! 由 `plan()` 基于 models 注册表（capability_tier / context_window / …）、
//! routing_policy 权重、price_specs 真源成本统一评分决策。
//! Python 侧纯执行，Rust 单一决策。

use crate::ai_client::ModelSpec;
use crate::db;
use crate::models::{Model, RoutingDecision, RoutingPolicy};
use crate::pricing::{PriceSpec, ZoneResolver};
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

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

// ── Complexity detection (band projection) ──

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

/// 难度带投影（P0.d 简版：task_type 基线 + 复杂度正则提档；
/// P0.g 信号层落地后由 prefix_stability 等信号精化）。
pub fn band_for(task_type: &str, query: &str) -> &'static str {
    let base = match task_type {
        "simple_qa" => "easy",
        "complex_reasoning" => "hard",
        _ => "medium",
    };
    if base != "hard" && is_complex(query) {
        return if base == "easy" { "medium" } else { "hard" };
    }
    base
}

fn band_tier(band: &str) -> i64 {
    match band {
        "easy" => 1,
        "medium" => 2,
        _ => 3,
    }
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
/// 返回 (task_type, method)；模型选择交给 `plan()`，不再内嵌默认映射。
pub async fn classify(text: &str, classifier: Option<&ModelSpec>) -> (String, String) {
    if let Some(task) = rule_classify(text) {
        return (task, "rule".to_string());
    }
    let task = match classifier {
        Some(spec) => crate::ai_client::classify(text, spec, &VALID_TASK_TYPES).await,
        None => "general".to_string(),
    };
    (task, "llm".to_string())
}

// ── plan(): 注册表驱动评分路由（ROUTING-PLAN §4.6） ──

/// plan() 的纯函数输入（依赖注入，测试不碰 DB）。
pub struct PlanInput<'a> {
    pub task_type: &'a str,
    pub band: &'a str,
    pub policy: &'a RoutingPolicy,
    pub models: &'a [Model],
    /// (provider, model_name) → PriceSpec；本地/未登记模型缺省（成本 0）
    pub price_specs: &'a HashMap<(String, String), PriceSpec>,
    pub zones: &'a ZoneResolver,
    pub t_epoch_secs: i64,
    pub est_in_tokens: i64,
    pub est_out_tokens: i64,
    /// model → 成效分（model_task_score.ewma_quality，sample≥5 才收录；
    /// 无记录回落 m.quality_score）
    pub quality_override: &'a HashMap<String, f64>,
    /// P5 预算档预留，本阶段恒 "normal"
    pub budget_tier: &'a str,
}

#[derive(Debug)]
pub struct Candidate {
    pub name: String,
    pub score: f64,
    pub est_cost: f64,
    pub quality: f64,
    pub capability_tier: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    /// 无候选：携带每个被淘汰模型的原因，供调用方诊断（不再伪造空 spec）
    #[error("无可用候选 (task={task_type} band={band}): {report}")]
    NoCandidates {
        task_type: String,
        band: String,
        report: String,
    },
}

#[derive(Debug)]
pub struct PlanOutcome {
    pub primary: String,
    pub fallback_chain: Vec<String>,
    pub candidates: Vec<Candidate>,
}

/// 门槛（gate）+ 评分（score）。全量候选淘汰时返回带诊断的 NoCandidates。
pub fn plan(input: &PlanInput) -> Result<PlanOutcome, PlanError> {
    let tier_req = band_tier(input.band).max(input.policy.min_capability_tier);
    let cap = input.policy.max_cost_per_request;

    let mut rejected: Vec<String> = Vec::new();
    let mut gated: Vec<&Model> = Vec::new();

    for m in input.models.iter().filter(|m| m.is_active != 0) {
        if m.health_state == "down" {
            rejected.push(format!("{}: health down", m.name));
            continue;
        }
        if m.capability_tier < tier_req {
            rejected.push(format!("{}: tier{} < 需求{tier_req}", m.name, m.capability_tier));
            continue;
        }
        if m.context_window > 0 && input.est_in_tokens + input.est_out_tokens > m.context_window {
            rejected.push(format!(
                "{}: ctx {} < est {}+{}",
                m.name, m.context_window, input.est_in_tokens, input.est_out_tokens
            ));
            continue;
        }
        if let (Some(cap), Some(spec)) = (
            cap,
            input.price_specs.get(&(m.provider.clone(), m.name.clone())),
        ) {
            let ec = spec.est_cost(0.0, input.est_in_tokens, input.est_out_tokens, input.t_epoch_secs, input.zones);
            if ec > cap {
                rejected.push(format!("{}: est ${ec:.4} > cap ${cap}", m.name));
                continue;
            }
        }
        gated.push(m);
    }

    if gated.is_empty() {
        return Err(PlanError::NoCandidates {
            task_type: input.task_type.to_string(),
            band: input.band.to_string(),
            report: if rejected.is_empty() {
                "注册表为空".to_string()
            } else {
                rejected.join("; ")
            },
        });
    }

    // pinned：策略钦定（须在门槛内，健康可用）
    if let Some(pinned) = input.policy.pinned_model.as_deref() {
        if let Some(m) = gated.iter().find(|m| m.name == pinned) {
            let rest: Vec<String> = score_all(input, &gated)
                .into_iter()
                .filter(|c| c.name != pinned)
                .map(|c| c.name)
                .take(input.policy.fallback_depth.max(0) as usize)
                .collect();
            return Ok(PlanOutcome {
                primary: pinned.to_string(),
                fallback_chain: rest,
                candidates: vec![Candidate {
                    name: pinned.to_string(),
                    score: f64::INFINITY,
                    est_cost: 0.0,
                    quality: quality_of(input, m),
                    capability_tier: m.capability_tier,
                }],
            });
        }
    }

    let mut candidates = score_all(input, &gated);
    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    let primary = candidates[0].name.clone();
    let chain = candidates[1..]
        .iter()
        .take(input.policy.fallback_depth.max(0) as usize)
        .map(|c| c.name.clone())
        .collect();
    Ok(PlanOutcome {
        primary,
        fallback_chain: chain,
        candidates,
    })
}

fn quality_of(input: &PlanInput, m: &Model) -> f64 {
    input
        .quality_override
        .get(&m.name)
        .copied()
        .unwrap_or(m.quality_score)
        .clamp(0.0, 1.0)
}

/// s = qw·quality − cw·norm_cost − lw·norm_latency + 0.05·priority − 保守期罚分
fn score_all(input: &PlanInput, gated: &[&Model]) -> Vec<Candidate> {
    let ecs: Vec<f64> = gated
        .iter()
        .map(|m| {
            input
                .price_specs
                .get(&(m.provider.clone(), m.name.clone()))
                .map(|s| s.est_cost(0.0, input.est_in_tokens, input.est_out_tokens, input.t_epoch_secs, input.zones))
                .unwrap_or(0.0)
        })
        .collect();
    let mut sorted_ecs = ecs.clone();
    sorted_ecs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let med_ec = sorted_ecs.get(sorted_ecs.len() / 2).copied().unwrap_or(0.0);
    let tier_req = band_tier(input.band).max(input.policy.min_capability_tier);

    gated
        .iter()
        .zip(ecs.iter())
        .map(|(m, &ec)| {
            let q = quality_of(input, m);
            let norm_cost = if ec + med_ec > 0.0 { ec / (ec + med_ec) } else { 0.0 };
            let norm_latency = 0.0; // P1.a latency 落库后接入
            let mut s = input.policy.quality_weight * q
                - input.policy.cost_weight * norm_cost
                - input.policy.latency_weight * norm_latency
                + 0.05 * m.priority as f64;
            if m.needs_calibration != 0 && tier_req > 1 {
                s -= 0.3;
            }
            Candidate {
                name: m.name.clone(),
                score: s,
                est_cost: ec,
                quality: q,
                capability_tier: m.capability_tier,
            }
        })
        .collect()
}

// ── route(): 生产入口（读 DB 组装 PlanInput → plan()） ──

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Build a routing decision for a request.
///
/// `model` 为 "auto"（分类 + plan() 评分）或显式模型名（direct）。
/// direct 未注册模型由调用方（server）报错——本函数不伪造 spec。
pub async fn route(model: &str, user_text: &str, classifier: Option<&ModelSpec>) -> RoutingDecision {
    let models = db::list_models(true).unwrap_or_default();
    if model != "auto" && model != "auto-route" {
        let stream = models
            .iter()
            .find(|m| m.name == model)
            .map(|m| m.supports_stream != 0)
            .unwrap_or(false);
        return RoutingDecision {
            model: model.to_string(),
            task_type: "direct".to_string(),
            method: "direct".to_string(),
            stream,
            band: String::new(),
            fallback_chain: Vec::new(),
        };
    }

    let (task_type, method) = classify(user_text, classifier).await;
    let band = band_for(&task_type, user_text);

    let policy = db::get_routing_policy(&task_type)
        .ok()
        .flatten()
        .unwrap_or_default();

    let mut spec_map: HashMap<(String, String), PriceSpec> = HashMap::new();
    for spec in db::list_price_specs().unwrap_or_default() {
        spec_map.insert((spec.provider.clone(), spec.model.clone()), spec);
    }
    let zr = ZoneResolver::new();
    zr.load(db::list_provider_zones().unwrap_or_default());

    let mut quality_override = HashMap::new();
    for m in &models {
        if let Some(sc) = db::get_model_task_score(&m.name, &task_type).ok().flatten() {
            if sc.sample_count >= 5 {
                quality_override.insert(m.name.clone(), sc.ewma_quality.clamp(0.0, 1.0));
            }
        }
    }

    // est_in 粗估：中英混合 ~0.6 token/字符（P5.c tiktoken 预算器落地前）
    let est_in = (user_text.chars().count() as f64 * 0.6) as i64;
    let est_out = 500i64;

    let input = PlanInput {
        task_type: &task_type,
        band,
        policy: &policy,
        models: &models,
        price_specs: &spec_map,
        zones: &zr,
        t_epoch_secs: now_epoch(),
        est_in_tokens: est_in,
        est_out_tokens: est_out,
        quality_override: &quality_override,
        budget_tier: "normal",
    };

    match plan(&input) {
        Ok(outcome) => {
            let stream = models
                .iter()
                .find(|m| m.name == outcome.primary)
                .map(|m| m.supports_stream != 0)
                .unwrap_or(false);
            RoutingDecision {
                model: outcome.primary,
                task_type,
                method,
                stream,
                band: band.to_string(),
                fallback_chain: outcome.fallback_chain,
            }
        }
        Err(e) => RoutingDecision {
            model: String::new(),
            task_type: task_type.clone(),
            method: format!("plan_error:{e}"),
            stream: false,
            band: band.to_string(),
            fallback_chain: Vec::new(),
        },
    }
}

// ── plan_decision(): 编排角色决策（P0.f Rust 统一决策） ──
//
// 与 `route()` 共用同一套 plan() 评分逻辑，但跳过「分类」步骤：
// 编排角色（general/decompose/aggregate）的任务类型是固定的，
// 只按注册表 + 策略直接评分选主模型。结果以 assignments 下发给 Python，
// Python 侧删除了 TASK_MODEL_PREFERENCE/DECOMPOSER_PREFERENCE 等硬编码真源，
// 优先用本决策，仅在全池兜底时回落 models[0]。

/// 按固定编排角色做一次 plan() 决策，返回主模型名所在 PlanOutcome。
/// 失败时由调用方兜底（回落 models 首模型），不抛业务中断。
pub fn plan_decision(task_type: &str, models: &[Model]) -> Result<PlanOutcome, PlanError> {
    plan_for_task(task_type, models, 500, 1000, "normal")
}

/// P4：子任务级/可变预算档 plan——按调用方给的预估 token 与预算档评分路由。
/// 供 `POST /api/routing/plan-subtask` 使用（Python 每个子任务按其 task_type 独立 plan）；
/// 无状态，仅为 plan() 的参数化封装。
pub fn plan_for_task(
    task_type: &str,
    models: &[Model],
    est_in_tokens: i64,
    est_out_tokens: i64,
    budget_tier: &str,
) -> Result<PlanOutcome, PlanError> {
    let policy = db::get_routing_policy(task_type)
        .ok()
        .flatten()
        .unwrap_or_default();

    let mut spec_map: HashMap<(String, String), PriceSpec> = HashMap::new();
    for spec in db::list_price_specs().unwrap_or_default() {
        spec_map.insert((spec.provider.clone(), spec.model.clone()), spec);
    }
    let zr = ZoneResolver::new();
    zr.load(db::list_provider_zones().unwrap_or_default());

    let mut quality_override = HashMap::new();
    for m in models {
        if let Some(sc) = db::get_model_task_score(&m.name, task_type).ok().flatten() {
            if sc.sample_count >= 5 {
                quality_override.insert(m.name.clone(), sc.ewma_quality.clamp(0.0, 1.0));
            }
        }
    }

    let band = match task_type {
        "simple_qa" => "easy",
        "complex_reasoning" => "hard",
        _ => "medium",
    };

    let input = PlanInput {
        task_type,
        band,
        policy: &policy,
        models,
        price_specs: &spec_map,
        zones: &zr,
        t_epoch_secs: now_epoch(),
        est_in_tokens,
        est_out_tokens,
        quality_override: &quality_override,
        budget_tier,
    };
    plan(&input)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn model(name: &str, provider: &str, tier: i64, ctx: i64) -> Model {
        Model {
            id: 0,
            name: name.to_string(),
            provider: provider.to_string(),
            litellm_model: name.to_string(),
            api_base: String::new(),
            api_key_env: String::new(),
            task_type: String::new(),
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
            rpm: 60,
            is_active: 1,
            capability_tier: tier,
            quality_score: match tier {
                3 => 0.85,
                2 => 0.70,
                _ => 0.45,
            },
            context_window: ctx,
            supports_tools: 0,
            supports_vision: 0,
            supports_stream: 0,
            is_local: if provider == "ollama" { 1 } else { 0 },
            priority: 0,
            health_state: "unknown".to_string(),
            needs_calibration: 0,
        }
    }

    fn spec(provider: &str, m: &str, in_c: f64, out_c: f64) -> PriceSpec {
        PriceSpec {
            provider: provider.to_string(),
            model: m.to_string(),
            input_cost: in_c,
            output_cost: out_c,
            cache_read_cost: None,
            cache_write_cost: Some(0.0),
            reasoning_cost: None,
            tiered: None,
            zone_ref: None,
            batch_multiplier: 0.5,
            price_source: "manual".to_string(),
            price_stale: false,
            effective_from: None,
        }
    }

    /// 测试上下文（owned，避免借用临时值）
    struct Ctx {
        zones: ZoneResolver,
        quality: HashMap<String, f64>,
    }

    impl Ctx {
        fn new() -> Self {
            Self {
                zones: ZoneResolver::new(),
                quality: HashMap::new(),
            }
        }
    }

    fn base_input<'a>(
        models: &'a [Model],
        specs: &'a HashMap<(String, String), PriceSpec>,
        task_type: &'a str,
        band: &'a str,
        policy: &'a RoutingPolicy,
        est_in: i64,
        ctx: &'a Ctx,
    ) -> PlanInput<'a> {
        PlanInput {
            task_type,
            band,
            policy,
            models,
            price_specs: specs,
            zones: &ctx.zones,
            t_epoch_secs: 0,
            est_in_tokens: est_in,
            est_out_tokens: 500,
            quality_override: &ctx.quality,
            budget_tier: "normal",
        }
    }

    fn registry() -> Vec<Model> {
        vec![
            model("qwen2.5-local", "ollama", 1, 32768),
            model("qwen3.6-flash", "dashscope", 1, 262144),
            model("qwen-plus", "dashscope", 2, 131072),
            model("deepseek-v3", "dashscope", 3, 65536),
            model("qwen3.6-plus", "dashscope", 3, 1048576),
            model("qwen3-max", "dashscope", 3, 262144),
            model("gpt-4o", "openai", 2, 128000),
        ]
    }

    fn prices() -> HashMap<(String, String), PriceSpec> {
        let mut m = HashMap::new();
        m.insert(("dashscope".into(), "qwen3.6-flash".into()), spec("dashscope", "qwen3.6-flash", 1.11e-7, 9e-7));
        m.insert(("dashscope".into(), "qwen-plus".into()), spec("dashscope", "qwen-plus", 1.11e-7, 4.4e-7));
        m.insert(("dashscope".into(), "deepseek-v3".into()), spec("dashscope", "deepseek-v3", 2.0e-7, 1.1e-6));
        m.insert(("dashscope".into(), "qwen3.6-plus".into()), spec("dashscope", "qwen3.6-plus", 6.9e-7, 2.8e-6));
        m.insert(("dashscope".into(), "qwen3-max".into()), spec("dashscope", "qwen3-max", 1.6e-6, 6.4e-6));
        m.insert(("openai".into(), "gpt-4o".into()), spec("openai", "gpt-4o", 2.5e-6, 1.0e-5));
        m
    }

    #[test]
    fn empty_registry_is_explicit_error() {
        let empty: Vec<Model> = vec![];
        let specs = HashMap::new();
        let policy = RoutingPolicy::default();
        let ctx = Ctx::new();
        let r = plan(&base_input(&empty, &specs, "general", "medium", &policy, 100, &ctx));
        assert!(matches!(r, Err(PlanError::NoCandidates { .. })));
    }

    #[test]
    fn all_gated_out_reports_reasons() {
        let models = registry();
        let specs = prices();
        // coding 策略 min_tier=3，但全部 tier3 标 down
        let mut down = models.clone();
        for m in down.iter_mut().filter(|m| m.capability_tier >= 3) {
            m.health_state = "down".to_string();
        }
        let policy = RoutingPolicy {
            task_type: "coding".into(),
            min_capability_tier: 3,
            ..Default::default()
        };
        let ctx = Ctx::new();
        let r = plan(&base_input(&down, &specs, "coding", "hard", &policy, 100, &ctx));
        match r {
            Err(PlanError::NoCandidates { report, .. }) => assert!(report.contains("health down")),
            other => panic!("expected NoCandidates, got {other:?}"),
        }
    }

    #[test]
    fn easy_task_prefers_tier1_low_cost() {
        let models = registry();
        let specs = prices();
        let policy = RoutingPolicy {
            task_type: "simple_qa".into(),
            min_capability_tier: 1,
            cost_weight: 0.7,
            quality_weight: 0.2,
            latency_weight: 0.1,
            ..Default::default()
        };
        let ctx = Ctx::new();
        let out = plan(&base_input(&models, &specs, "simple_qa", "easy", &policy, 100, &ctx)).unwrap();
        // 本地零成本模型胜出（质量 0.45，成本 0）
        assert_eq!(out.primary, "qwen2.5-local");
        assert!(out.fallback_chain.len() <= 2);
    }

    #[test]
    fn hard_task_requires_tier3_and_pays_quality() {
        let models = registry();
        let specs = prices();
        let policy = RoutingPolicy {
            task_type: "complex_reasoning".into(),
            min_capability_tier: 3,
            cost_weight: 0.2,
            quality_weight: 0.7,
            latency_weight: 0.1,
            ..Default::default()
        };
        let ctx = Ctx::new();
        let out = plan(&base_input(&models, &specs, "complex_reasoning", "hard", &policy, 100, &ctx)).unwrap();
        // 三个 tier3（deepseek-v3/qwen3.6-plus/qwen3-max）都过门槛；高质量+低成本的 deepseek-v3 应领先
        assert!(["deepseek-v3", "qwen3.6-plus", "qwen3-max"].contains(&out.primary.as_str()));
        assert!(out.candidates.iter().all(|c| c.capability_tier >= 3));
    }

    #[test]
    fn deleting_primary_reselects_from_registry() {
        // 旧硬编码的致命 bug：删模型后路由名找不到 → 伪造空 spec。
        // 新行为：primary 永远来自注册表；删掉后自动改选下一个。
        let mut models = registry();
        let specs = prices();
        let policy = RoutingPolicy {
            task_type: "general".into(),
            min_capability_tier: 2,
            ..Default::default()
        };
        let ctx = Ctx::new();
        let first = plan(&base_input(&models, &specs, "general", "medium", &policy, 100, &ctx)).unwrap();
        models.retain(|m| m.name != first.primary);
        let second = plan(&base_input(&models, &specs, "general", "medium", &policy, 100, &ctx)).unwrap();
        assert_ne!(first.primary, second.primary);
        assert!(
            models.iter().any(|m| m.name == second.primary),
            "re-selected model must exist in registry"
        );
    }

    #[test]
    fn context_window_gate_fails_gracefully() {
        let mut models = registry();
        let specs = prices();
        let policy = RoutingPolicy::default();
        // 100K 输入：32K 本地被门槛淘汰，但 1M 窗口的 qwen3.6-plus 可接
        let ctx = Ctx::new();
        let out = plan(&base_input(&models, &specs, "general", "medium", &policy, 100_000, &ctx)).unwrap();
        assert_ne!(out.primary, "qwen2.5-local");
        // 900K：只剩 qwen3.6-plus 过窗
        models.retain(|m| m.name != "qwen3.6-plus");
        let r = plan(&base_input(&models, &specs, "general", "medium", &policy, 900_000, &ctx));
        assert!(matches!(r, Err(PlanError::NoCandidates { .. })));
    }

    #[test]
    fn max_cost_cap_rejects_expensive() {
        let models = registry();
        let specs = prices();
        let policy = RoutingPolicy {
            task_type: "general".into(),
            min_capability_tier: 2,
            max_cost_per_request: Some(0.01),
            ..Default::default()
        };
        // 10K 输入：gpt-4o ≈$0.03、qwen3-max ≈$0.019 超 cap 被拒；平价模型仍在
        let ctx = Ctx::new();
        let out = plan(&base_input(&models, &specs, "general", "medium", &policy, 10_000, &ctx)).unwrap();
        let names: Vec<&str> = out.candidates.iter().map(|c| c.name.as_str()).collect();
        assert!(!names.contains(&"gpt-4o"), "gpt-4o must be gated by cap: {names:?}");
        assert!(!names.contains(&"qwen3-max"), "qwen3-max must be gated by cap: {names:?}");
        assert!(!names.contains(&"qwen2.5-local"), "tier1 gated by policy");
    }

    #[test]
    fn calibration_penalty_deprioritizes_new_model_on_hard_tasks() {
        let mut models = registry();
        let specs = prices();
        let policy = RoutingPolicy {
            task_type: "complex_reasoning".into(),
            min_capability_tier: 3,
            cost_weight: 0.2,
            quality_weight: 0.7,
            latency_weight: 0.1,
            ..Default::default()
        };
        // qwen3.6-plus 未标定（保守期），deepseek-v3 已标定
        for m in models.iter_mut() {
            m.needs_calibration = if m.name == "qwen3.6-plus" { 1 } else { 0 };
        }
        let ctx = Ctx::new();
        let out = plan(&base_input(&models, &specs, "complex_reasoning", "hard", &policy, 100, &ctx)).unwrap();
        assert_ne!(out.primary, "qwen3.6-plus");
    }

    #[test]
    fn pinned_model_wins_when_healthy() {
        let models = registry();
        let specs = prices();
        let policy = RoutingPolicy {
            task_type: "general".into(),
            pinned_model: Some("qwen-plus".into()),
            ..Default::default()
        };
        let ctx = Ctx::new();
        let out = plan(&base_input(&models, &specs, "general", "medium", &policy, 100, &ctx)).unwrap();
        assert_eq!(out.primary, "qwen-plus");
        // pinned down → 回落评分链
        let mut down = models.clone();
        down.iter_mut().find(|m| m.name == "qwen-plus").unwrap().health_state = "down".to_string();
        let out2 = plan(&base_input(&down, &specs, "general", "medium", &policy, 100, &ctx)).unwrap();
        assert_ne!(out2.primary, "qwen-plus");
    }

    #[test]
    fn quality_override_beats_cold_quality() {
        let models = registry();
        let specs = prices();
        let policy = RoutingPolicy {
            task_type: "complex_reasoning".into(),
            min_capability_tier: 3,
            cost_weight: 0.0,
            quality_weight: 1.0,
            latency_weight: 0.0,
            ..Default::default()
        };
        let mut ctx = Ctx::new();
        ctx.quality.insert("deepseek-v3".to_string(), 0.1); // 实测质量崩了
        let input = base_input(&models, &specs, "complex_reasoning", "hard", &policy, 100, &ctx);
        let out = plan(&input).unwrap();
        assert_ne!(out.primary, "deepseek-v3");
    }

    #[test]
    fn band_projection() {
        assert_eq!(band_for("simple_qa", "你好"), "easy");
        assert_eq!(band_for("complex_reasoning", "x"), "hard");
        assert_eq!(band_for("general", "帮我写代码"), "medium");
        // 多步骤复杂查询提档
        assert_eq!(
            band_for("general", "先分析A和B的优缺点，然后对比方案，最后给出建议"),
            "hard"
        );
    }
}
