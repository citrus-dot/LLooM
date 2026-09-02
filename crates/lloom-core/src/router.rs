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
    /// PR-5 §5.1：model → 缓存命中率（0..1），喂 `effective_input_cost` 期望单价。
    /// 缺省 0 = 不认为有缓存收益（不偏袒）。
    pub hit_rate: &'a HashMap<String, f64>,
    /// PR-5 §5.2 会话亲和：本会话上一轮所用模型（sticky）。None = 不粘。
    pub last_model_conv: Option<&'a str>,
    /// PR-8：deferrable=1 时按「预计谷时执行时刻」估成本（若 2h 内进谷则用谷价+该候选机会成本下降）。
    /// 实时 chat/编排默认 false（实时路径零延迟变化）。
    pub deferrable: bool,
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

/// P5.a：剩余预算比 r → 预算档（ROUTING-PLAN §P5.a）。纯函数，可单测。
/// r>50% normal；20<r≤50% throttle；5<r≤20% tight；r≤5% protect。
pub fn budget_tier_from_ratio(r: f64) -> &'static str {
    if r > 0.5 {
        "normal"
    } else if r > 0.2 {
        "throttle"
    } else if r > 0.05 {
        "tight"
    } else {
        "protect"
    }
}

/// P5.a：预算档对 cost_weight 的放大系数（normal=×1，throttle=×1.5，tight=×2.5）。
/// protect 不再依赖 cost 评分（仅本地候选，见 plan() 门槛）。
fn tier_cost_multiplier(tier: &str) -> f64 {
    match tier {
        "throttle" => 1.5,
        "tight" => 2.5,
        _ => 1.0,
    }
}

/// 门槛（gate）+ 评分（score）。全量候选淘汰时返回带诊断的 NoCandidates。
pub fn plan(input: &PlanInput) -> Result<PlanOutcome, PlanError> {
    // P5.a tight：复杂任务降一档（只降需求能力档，band 报告仍保留原值）。
    let req_band = if input.budget_tier == "tight" && input.band == "hard" {
        "medium"
    } else {
        input.band
    };
    let tier_req = band_tier(req_band).max(input.policy.min_capability_tier);
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
        let ec = input
            .price_specs
            .get(&(m.provider.clone(), m.name.clone()))
            .map(|s| s.est_cost(hit_of(input, m), input.est_in_tokens, input.est_out_tokens, cost_epoch(input), input.zones))
            .unwrap_or(0.0);
        // P5.a protect：仅本地免费或零成本模型（预算耗尽推本地 Ollama 的最后一档）。
        if input.budget_tier == "protect" && m.is_local != 1 && ec > 0.0 {
            rejected.push(format!("{}: protect 仅本地/零成本", m.name));
            continue;
        }
        if let Some(cap) = cap {
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

/// PR-5 §5.1：model → 缓存命中率（缺省 0，不偏袒）。
fn hit_of(input: &PlanInput, m: &Model) -> f64 {
    input.hit_rate.get(&m.name).copied().unwrap_or(0.0).clamp(0.0, 1.0)
}

/// PR-5 §5.2：会话亲和加分——仅缓存敏感通道（spec 有 cache_read 区分）且命中本会话末模型时 +0.05。
fn sticky_bonus(input: &PlanInput, m: &Model) -> f64 {
    if input.last_model_conv != Some(m.name.as_str()) {
        return 0.0;
    }
    let cache_sensitive = input
        .price_specs
        .get(&(m.provider.clone(), m.name.clone()))
        .map(|s| s.cache_read_cost.is_some())
        .unwrap_or(false);
    if cache_sensitive { 0.05 } else { 0.0 }
}

/// PR-8 谷时调度视野：当前高峰（multiplier≥1）且 2 小时内有谷时窗口的渠道，其最早谷时起始时刻。
/// None = 无需延迟（非高峰、已谷时、或 2h 内无谷时）。供 plan() 估谷价 + probe/server 对齐执行。
pub const VALLEY_HORIZON_SECS: i64 = 7200;

/// 扫描已加载分时渠道，取「当前高峰且 2h 内有谷时」的最早谷时起始时刻（严格未来）。None=无需延迟。
pub fn next_valley_epoch(zr: &ZoneResolver, now: i64) -> Option<i64> {
    let mut best: Option<i64> = None;
    for zone in zr.zones() {
        // 当前已是谷价（multiplier<1）→ 无需延迟；无分时渠道 multiplier 恒 1 → 无谷时窗口，自然跳过。
        if zone.multiplier_at(now) >= 1.0 {
            if let Some(v) = zone.first_valley_epoch(now, VALLEY_HORIZON_SECS) {
                if best.map_or(true, |b| v < b) {
                    best = Some(v);
                }
            }
        }
    }
    best
}

/// 估算成本所用时刻：deferrable 且 2h 内进谷 → 用谷时起始时刻（谷价）；否则用当前时刻（实时价）。
fn cost_epoch(input: &PlanInput) -> i64 {
    if input.deferrable {
        next_valley_epoch(input.zones, input.t_epoch_secs).unwrap_or(input.t_epoch_secs)
    } else {
        input.t_epoch_secs
    }
}

/// s = qw·quality − cw·norm_cost − lw·norm_latency + 0.05·priority + sticky − 保守期罚分
fn score_all(input: &PlanInput, gated: &[&Model]) -> Vec<Candidate> {
    let ecs: Vec<f64> = gated
        .iter()
        .map(|m| {
            input
                .price_specs
                .get(&(m.provider.clone(), m.name.clone()))
                .map(|s| s.est_cost(hit_of(input, m), input.est_in_tokens, input.est_out_tokens, cost_epoch(input), input.zones))
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
                - input.policy.cost_weight * tier_cost_multiplier(input.budget_tier) * norm_cost
                - input.policy.latency_weight * norm_latency
                + 0.05 * m.priority as f64
                + sticky_bonus(input, m); // PR-5 §5.2 会话亲和
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
pub async fn route(
    model: &str,
    user_text: &str,
    classifier: Option<&ModelSpec>,
    last_model: Option<&str>,
) -> RoutingDecision {
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
            budget_tier: String::new(),
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
    let mut hit_rate = HashMap::new();
    for m in &models {
        if let Some(sc) = db::get_model_task_score(&m.name, &task_type).ok().flatten() {
            if sc.sample_count >= 5 {
                quality_override.insert(m.name.clone(), sc.ewma_quality.clamp(0.0, 1.0));
            }
        }
    }
    // PR-5 §5.1：真实缓存命中率喂 effective_input_cost（缺省 0 = 不偏袒）
    for (k, v) in db::model_cache_hit_rate(&task_type) {
        hit_rate.insert(k, v);
    }

    // est_in 粗估：中英混合 ~0.6 token/字符（编排路径已由 Python 侧 count_tokens 精确传 plan-subtask）。
    // est_out：P5.c 用该 task_type 历史 avg_out_tokens（冷启动 750），替换固定 500。
    let est_in = (user_text.chars().count() as f64 * 0.6) as i64;
    let est_out = db::task_avg_out_tokens(&task_type).round() as i64;
    // P5.a：预算档由全局预算剩余比自动注入（无/未设全局预算 → normal）。
    let budget_tier = db::global_budget_ratio()
        .map(budget_tier_from_ratio)
        .unwrap_or("normal");

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
        budget_tier,
        hit_rate: &hit_rate,
        last_model_conv: last_model,
        deferrable: false, // 实时 chat 永不延迟（PR-8）
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
                budget_tier: budget_tier.to_string(),
                fallback_chain: outcome.fallback_chain,
            }
        }
        Err(e) => RoutingDecision {
            model: String::new(),
            task_type: task_type.clone(),
            method: format!("plan_error:{e}"),
            stream: false,
            band: band.to_string(),
            budget_tier: budget_tier.to_string(),
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
    plan_for_task(task_type, models, 500, 1000, "normal", false)
}

/// P4：子任务级/可变预算档 plan——按调用方给的预估 token 与预算档评分路由。
/// 供 `POST /api/routing/plan-subtask` 使用（Python 每个子任务按其 task_type 独立 plan）；
/// 无状态，仅为 plan() 的参数化封装。`deferrable` 为 PR-8：true 时按谷价估成本（B 端批/评测接入）。
pub fn plan_for_task(
    task_type: &str,
    models: &[Model],
    est_in_tokens: i64,
    est_out_tokens: i64,
    budget_tier: &str,
    deferrable: bool,
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
    let mut hit_rate = HashMap::new();
    for m in models {
        if let Some(sc) = db::get_model_task_score(&m.name, task_type).ok().flatten() {
            if sc.sample_count >= 5 {
                quality_override.insert(m.name.clone(), sc.ewma_quality.clamp(0.0, 1.0));
            }
        }
    }
    // PR-5 §5.1：真实缓存命中率喂 effective_input_cost
    for (k, v) in db::model_cache_hit_rate(task_type) {
        hit_rate.insert(k, v);
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
        hit_rate: &hit_rate,
        last_model_conv: None,
        deferrable,
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
            stale_reason: None,
            effective_from: None,
        }
    }

    /// 测试上下文（owned，避免借用临时值）
    struct Ctx {
        zones: ZoneResolver,
        quality: HashMap<String, f64>,
        hit: HashMap<String, f64>,
        sticky: Option<String>,
    }

    impl Ctx {
        fn new() -> Self {
            Self {
                zones: ZoneResolver::new(),
                quality: HashMap::new(),
                hit: HashMap::new(),
                sticky: None,
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
            hit_rate: &ctx.hit,
            last_model_conv: ctx.sticky.as_deref(),
            deferrable: false,
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

    // ── P5 预算档 ──

    #[test]
    fn budget_tier_thresholds() {
        assert_eq!(budget_tier_from_ratio(0.6), "normal");
        assert_eq!(budget_tier_from_ratio(0.5), "throttle"); // 边界 r>0.5 才 normal
        assert_eq!(budget_tier_from_ratio(0.3), "throttle");
        assert_eq!(budget_tier_from_ratio(0.2), "tight"); // 边界 20<r≤50 结束
        assert_eq!(budget_tier_from_ratio(0.1), "tight");
        assert_eq!(budget_tier_from_ratio(0.05), "protect"); // 边界 r≤5%
        assert_eq!(budget_tier_from_ratio(0.0), "protect");
    }

    #[test]
    fn cost_multiplier_by_tier() {
        assert_eq!(tier_cost_multiplier("normal"), 1.0);
        assert_eq!(tier_cost_multiplier("protect"), 1.0);
        assert_eq!(tier_cost_multiplier("throttle"), 1.5);
        assert_eq!(tier_cost_multiplier("tight"), 2.5);
    }

    #[test]
    fn protect_tier_forces_local_only() {
        let models = registry();
        let specs = prices();
        let policy = RoutingPolicy {
            task_type: "simple_qa".into(),
            min_capability_tier: 1,
            ..Default::default()
        };
        let ctx = Ctx::new();
        let mut inp = base_input(&models, &specs, "simple_qa", "easy", &policy, 100, &ctx);
        inp.budget_tier = "protect";
        let out = plan(&inp).unwrap();
        // protect：仅本地或零成本模型保留，其余被 reject → 只剩 qwen2.5-local
        assert_eq!(out.primary, "qwen2.5-local");
        assert!(out.candidates.iter().all(|c| c.est_cost == 0.0));
    }

    #[test]
    fn throttle_shifts_to_cheaper_model() {
        let specs = prices();
        let policy = RoutingPolicy {
            task_type: "general".into(),
            min_capability_tier: 2,
            cost_weight: 0.5,
            quality_weight: 0.5,
            latency_weight: 0.0,
            ..Default::default()
        };
        // 两个 tier2 候选：qwen-plus（便宜、质量差）与 gpt-4o（贵、质量高）。
        // 正常：质量主导 → gpt-4o；throttle 放大成本权重（×1.5）→ 翻转为便宜的 qwen-plus。
        let models: Vec<Model> = registry()
            .into_iter()
            .filter(|m| m.name == "qwen-plus" || m.name == "gpt-4o")
            .collect();
        let mut ctx = Ctx::new();
        ctx.quality.insert("qwen-plus".into(), 0.3);
        ctx.quality.insert("gpt-4o".into(), 0.95);

        let normal = {
            let mut i = base_input(&models, &specs, "general", "medium", &policy, 100, &ctx);
            i.budget_tier = "normal";
            plan(&i).unwrap()
        };
        let throttle = {
            let mut i = base_input(&models, &specs, "general", "medium", &policy, 100, &ctx);
            i.budget_tier = "throttle";
            plan(&i).unwrap()
        };
        assert_eq!(normal.primary, "gpt-4o", "normal 应选高质量昂贵模型");
        assert_eq!(throttle.primary, "qwen-plus", "throttle 应翻转为便宜模型");
    }

    #[test]
    fn tight_reduces_hard_band_to_medium() {
        let models = registry();
        let specs = prices();
        let policy = RoutingPolicy {
            task_type: "complex_reasoning".into(),
            min_capability_tier: 2,
            cost_weight: 0.5,
            quality_weight: 0.5,
            latency_weight: 0.0,
            ..Default::default()
        };
        let ctx = Ctx::new();
        // hard 带：normal 要求 tier≥3，tight 降为 tier≥2 → 便宜的 qwen-plus(tier2) 可入候选
        let normal = {
            let mut i = base_input(&models, &specs, "complex_reasoning", "hard", &policy, 100, &ctx);
            i.budget_tier = "normal";
            plan(&i).unwrap()
        };
        let tight = {
            let mut i = base_input(&models, &specs, "complex_reasoning", "hard", &policy, 100, &ctx);
            i.budget_tier = "tight";
            plan(&i).unwrap()
        };
        assert!(normal.candidates.iter().all(|c| c.capability_tier >= 3));
        assert!(tight.candidates.iter().any(|c| c.capability_tier == 2));
    }

    // ── PR-5 路由衔接：命中率期望单价 + 会话亲和 ──

    #[test]
    fn sticky_bonus_only_for_cache_sensitive_match() {
        let models = registry();
        let mut specs = prices();
        specs.insert(
            ("dashscope".into(), "qwen-plus".into()),
            PriceSpec { cache_read_cost: Some(2.22e-8), ..spec("dashscope", "qwen-plus", 1.11e-7, 4.4e-7) },
        );
        let policy = RoutingPolicy::default();
        let ctx = Ctx::new();
        // 粘 qwen-plus + spec 缓存敏感 → +0.05
        let m = models.iter().find(|x| x.name == "qwen-plus").unwrap();
        let mut inp = base_input(&models, &specs, "general", "medium", &policy, 100, &ctx);
        inp.last_model_conv = Some("qwen-plus");
        assert_eq!(sticky_bonus(&inp, m), 0.05);
        // 粘其他模型 → 0
        let mut inp2 = base_input(&models, &specs, "general", "medium", &policy, 100, &ctx);
        inp2.last_model_conv = Some("gpt-4o");
        assert_eq!(sticky_bonus(&inp2, m), 0.0);
        // 非缓存敏感（gpt spec cache_read None）→ 即使命中也 0
        let g = models.iter().find(|x| x.name == "gpt-4o").unwrap();
        let mut inp3 = base_input(&models, &specs, "general", "medium", &policy, 100, &ctx);
        inp3.last_model_conv = Some("gpt-4o");
        assert_eq!(sticky_bonus(&inp3, g), 0.0);
    }

    #[test]
    fn sticky_bonus_flips_near_tie() {
        let models: Vec<Model> = registry()
            .into_iter()
            .filter(|m| m.name == "qwen-plus" || m.name == "gpt-4o")
            .collect();
        let mut specs = prices();
        specs.insert(
            ("dashscope".into(), "qwen-plus".into()),
            PriceSpec { cache_read_cost: Some(2.22e-8), ..spec("dashscope", "qwen-plus", 1.11e-7, 4.4e-7) },
        );
        let policy = RoutingPolicy {
            task_type: "general".into(),
            min_capability_tier: 2,
            cost_weight: 0.0,
            quality_weight: 1.0,
            latency_weight: 0.0,
            ..Default::default()
        };
        let mut ctx = Ctx::new();
        ctx.quality.insert("qwen-plus".into(), 0.56);
        ctx.quality.insert("gpt-4o".into(), 0.60);
        // 无粘性 → 质量稍高的 gpt-4o
        let no_sticky = plan(&base_input(&models, &specs, "general", "medium", &policy, 100, &ctx)).unwrap();
        assert_eq!(no_sticky.primary, "gpt-4o");
        // 粘上一轮所用 qwen-plus → +0.05 翻转为它（缓存敏感才粘）
        ctx.sticky = Some("qwen-plus".into());
        let sticky = plan(&base_input(&models, &specs, "general", "medium", &policy, 100, &ctx)).unwrap();
        assert_eq!(sticky.primary, "qwen-plus");
    }

    #[test]
    fn hit_rate_lowers_estimated_cost_in_candidates() {
        let models: Vec<Model> = registry().into_iter().filter(|m| m.name == "qwen-plus").collect();
        let mut specs = prices();
        specs.insert(
            ("dashscope".into(), "qwen-plus".into()),
            PriceSpec { cache_read_cost: Some(2.22e-8), ..spec("dashscope", "qwen-plus", 1.11e-7, 4.4e-7) },
        );
        let policy = RoutingPolicy { task_type: "general".into(), min_capability_tier: 2, ..Default::default() };
        let mut ctx = Ctx::new();
        let no_hit = plan(&base_input(&models, &specs, "general", "medium", &policy, 1000, &ctx)).unwrap();
        ctx.hit.insert("qwen-plus".into(), 0.9);
        let hit = plan(&base_input(&models, &specs, "general", "medium", &policy, 1000, &ctx)).unwrap();
        assert!(hit.candidates[0].est_cost < no_hit.candidates[0].est_cost);
    }

    // ── PR-8 峰谷调度：deferrable 任务按谷价估成本，翻转候选偏好 ──

    fn deepseek_test_zone() -> crate::pricing::Zone {
        crate::pricing::Zone::from_db(
            "deepseek",
            r#"[
              {"holidays":true,"hours":"*","multiplier":0.5},
              {"days":["sat","sun"],"hours":"*","multiplier":0.5},
              {"days":["mon","tue","wed","thu","fri"],"hours":"9-12,14-18","multiplier":1.0},
              {"days":["mon","tue","wed","thu","fri"],"hours":"*","multiplier":0.5}
            ]"#,
            "Asia/Shanghai",
            "[]",
        )
    }

    #[test]
    fn deferrable_shifts_to_valley_price_and_flips_primary() {
        let ctx = Ctx::new();
        ctx.zones.load(vec![deepseek_test_zone()]);

        // 两个 tier3 候选：qwen（不分时，恒定价） vs deepseek（分时，高峰价更贵）
        let models = vec![
            model("qwen3-max", "dashscope", 3, 262144),
            model("deepseek-v4-pro", "deepseek-official", 3, 131072),
        ];
        let mut specs = HashMap::new();
        specs.insert(("dashscope".into(), "qwen3-max".into()), spec("dashscope", "qwen3-max", 1.5e-6, 4.0e-6));
        let mut ds = spec("deepseek-official", "deepseek-v4-pro", 2.0e-6, 5.0e-6);
        ds.zone_ref = Some("deepseek".into());
        specs.insert(("deepseek-official".into(), "deepseek-v4-pro".into()), ds.clone());

        let policy = RoutingPolicy {
            task_type: "general".into(),
            min_capability_tier: 2,
            cost_weight: 0.8,
            quality_weight: 0.1,
            latency_weight: 0.1,
            ..Default::default()
        };
        // 2026-08-24 周一 10:00 高峰（deepseek 原价 1.0×；2h 内 12:00 进谷）
        let peak = crate::pricing::beijing_epoch(2026, 8, 24, 10, 0, 8);

        // 实时路径：deepseek 按峰价更贵 → qwen 胜
        let mut rt = base_input(&models, &specs, "general", "medium", &policy, 1000, &ctx);
        rt.t_epoch_secs = peak;
        rt.deferrable = false;
        assert_eq!(plan(&rt).unwrap().primary, "qwen3-max");

        // 可延迟路径：本轮估 2h 内谷价（deepseek 半价）→ 翻转胜出
        let mut d = base_input(&models, &specs, "general", "medium", &policy, 1000, &ctx);
        d.t_epoch_secs = peak;
        d.deferrable = true;
        assert_eq!(plan(&d).unwrap().primary, "deepseek-v4-pro");
    }

    #[test]
    fn deferrable_in_valley_does_not_shift() {
        // 已在谷时（23:00 周一 0.5×）：deferrable 与实时一致，无谷时窗口可挪，候选不变
        let ctx = Ctx::new();
        ctx.zones.load(vec![deepseek_test_zone()]);
        let models = vec![
            model("qwen3-max", "dashscope", 3, 262144),
            model("deepseek-v4-pro", "deepseek-official", 3, 131072),
        ];
        let mut specs = HashMap::new();
        specs.insert(("dashscope".into(), "qwen3-max".into()), spec("dashscope", "qwen3-max", 2.5e-6, 6.0e-6));
        let mut ds = spec("deepseek-official", "deepseek-v4-pro", 1.0e-6, 2.0e-6);
        ds.zone_ref = Some("deepseek".into());
        specs.insert(("deepseek-official".into(), "deepseek-v4-pro".into()), ds.clone());
        let policy = RoutingPolicy {
            task_type: "general".into(),
            min_capability_tier: 2,
            cost_weight: 0.8,
            quality_weight: 0.1,
            latency_weight: 0.1,
            ..Default::default()
        };
        let valley = crate::pricing::beijing_epoch(2026, 8, 24, 23, 0, 8);
        let mut rt = base_input(&models, &specs, "general", "medium", &policy, 1000, &ctx);
        rt.t_epoch_secs = valley;
        assert_eq!(plan(&rt).unwrap().primary, "deepseek-v4-pro");
        let mut d = base_input(&models, &specs, "general", "medium", &policy, 1000, &ctx);
        d.t_epoch_secs = valley;
        d.deferrable = true;
        assert_eq!(plan(&d).unwrap().primary, "deepseek-v4-pro");
    }
}
