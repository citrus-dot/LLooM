//! bench.rs — RouterBench v0.1 冻结矩阵回放引擎（Public Track）。
//!
//! 架构红线（11 号 §四 / 10 号 §九、§七）：
//! - **决策单一真源**：本模块只组装 [`router::PlanInput`] 并调用 production [`router::plan`]，
//!   不复刻任何评分公式；`hit_rate` 恒全零 map（§三十六：semantic cache 不算 routing 质量）；
//! - **防泄漏**：`quality_override`/Best Single/等效单价只来自 **calibration split** 统计
//!   （Best Single 样本 ≥5 才收录，对照 review.rs 生产惯例）；est 口径为固定 profile
//!   （est_in = chars×0.6 在线口径、est_out = 750 冷启动），**禁止读取 test outcome 的 tokens**；
//! - **零成本**：replay 全程无网络、无 LLM 调用、不触碰 production 表；
//!   bench_* 表落独立 `benchmarks/routerbench/bench.db`（决策 P1a，生产库零污染）。
//!
//! cost 语义：Public 池 `cost_basis=source`（源矩阵逐题实测 USD）。plan() 评分所需的
//! PriceSpec 由 calibration 统计导出**等效综合单价** p_m = mean_cost_m / mean(est_in+750)，
//! input/output 同价；推导只读 calibration 侧，公式记入 bench_runs.parameter_json。
//!
//! 与生产组装的同构性（parity，10 号 §二十四）：PlanInput 字段填法对照生产两条先例——
//! review.rs 离线重放（sticky=None、budget="normal"、deferrable=false）与
//! router.rs 在线 est 口径（chars×0.6 / 冷启动 750）；band 由 production `band_for` 现算。

use crate::error::{AppError, Result};
use crate::models::{Backend, Model, RoutingPolicy};
use crate::pricing::{PriceSpec, ZoneResolver};
use crate::router::{self, PlanInput};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

/// model_id slug：与导入侧 screening.py 的 slugify 同规则（非字母数字折叠为 _）。
pub fn slugify(name: &str) -> String {
    let mut s = String::with_capacity(name.len());
    let mut last_us = false;
    for c in name.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c);
            last_us = false;
        } else if !last_us {
            s.push('_');
            last_us = true;
        }
    }
    s.trim_matches('_').to_string()
}

/// 冷启动 est_out 固定口径（router.rs 在线路径的历史缺省；calibration 无统计时的固定 profile）。
pub const EST_OUT_COLD_START: i64 = 750;
/// est_in 粗估：中英混合 ~0.6 token/字符（与 router.rs 在线口径一致）。
pub const EST_IN_CHARS_PER_TOKEN: f64 = 0.6;
/// quality_override 收录门槛：calibration 样本数（对照 review.rs 生产惯例 ≥5）。
pub const CALIBRATION_MIN_SAMPLES: usize = 5;
/// Random baseline 固定 seed（10 号 §12.3）。
pub const RANDOM_SEED: u64 = 3407;

// ── 冻结矩阵数据面 ──

/// 冻结矩阵：`(sample_id, model_id) → (score, cost)`。加载即全内存，replay 只查不写。
pub struct FrozenMatrix {
    outcomes: HashMap<(String, String), (f64, f64)>,
}

/// Core 回放实例（来自 instances.jsonl，经 core_selection 名单过滤）。
pub struct BenchInstance {
    pub dataset_id: String,
    pub sample_id: String,
    pub task_type: String,
    pub prompt: String,
}

/// 显式模型池（weak/mid/strong，model_id 为冻结矩阵内 slug）。
#[derive(Debug, Clone)]
pub struct BenchPool {
    pub weak: String,
    pub mid: String,
    pub strong: String,
}

impl BenchPool {
    pub fn model_ids(&self) -> [&str; 3] {
        [&self.weak, &self.mid, &self.strong]
    }
}

/// load normalized/instances.jsonl + outcomes.jsonl；返回 (test 实例, calibration 实例, 矩阵)。
///
/// - `core_selection.jsonl` 名单只过滤 **test** 侧（Day 3 冻结名单）；名单缺失时退化为全 test
///   （诊断用；正式 replay 必须走名单）；calibration 侧全量返回（统计样本越大越稳）；
/// - test 实例 split 双重校验：非 test 即拒（泄漏防护自检）。
pub fn load_frozen(
    norm_dir: &Path,
    selection_path: Option<&Path>,
) -> Result<(Vec<BenchInstance>, Vec<BenchInstance>, FrozenMatrix)> {
    let inst_path = norm_dir.join("instances.jsonl");
    let out_path = norm_dir.join("outcomes.jsonl");
    let selected: Option<std::collections::HashSet<String>> = match selection_path {
        Some(p) => {
            let text = std::fs::read_to_string(p)
                .map_err(|e| AppError::InvalidRequest(format!("core_selection 读取失败 {p:?}: {e}")))?;
            let mut set = std::collections::HashSet::new();
            for l in text.lines().filter(|l| !l.trim().is_empty()) {
                let v: Value = serde_json::from_str(l)
                    .map_err(|e| AppError::InvalidRequest(format!("core_selection 行解析失败: {e}")))?;
                set.insert(v["sample_id"].as_str().unwrap_or_default().to_string());
            }
            Some(set)
        }
        None => None,
    };

    let mut test_instances = Vec::new();
    let mut calib_instances = Vec::new();
    for line in std::fs::read_to_string(&inst_path)
        .map_err(|e| AppError::InvalidRequest(format!("instances.jsonl 读取失败: {e}")))?
        .lines()
    {
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(line)
            .map_err(|e| AppError::InvalidRequest(format!("instances 行解析失败: {e}")))?;
        let sample_id = v["sample_id"].as_str().unwrap_or_default().to_string();
        let inst = BenchInstance {
            dataset_id: v["dataset_id"].as_str().unwrap_or_default().to_string(),
            sample_id: sample_id.clone(),
            task_type: v["task_type"].as_str().unwrap_or("general").to_string(),
            prompt: v["prompt"].as_str().unwrap_or_default().to_string(),
        };
        match v["split"].as_str() {
            Some("test") => {
                let in_core = selected.as_ref().map(|s| s.contains(&sample_id)).unwrap_or(true);
                if in_core {
                    test_instances.push(inst);
                }
            }
            Some("calibration") => calib_instances.push(inst),
            _ => {} // 其他 split 值一律忽略（泄漏防护自检：只认两种合法值）
        }
    }
    if test_instances.is_empty() {
        return Err(AppError::InvalidRequest(
            "冻结矩阵加载后 Core 实例为空：检查 core_selection 名单与 instances.jsonl 是否同代（重切会换 benchmark_id）".into(),
        ));
    }

    let mut outcomes = HashMap::new();
    for line in std::fs::read_to_string(&out_path)
        .map_err(|e| AppError::InvalidRequest(format!("outcomes.jsonl 读取失败: {e}")))?
        .lines()
    {
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(line)
            .map_err(|e| AppError::InvalidRequest(format!("outcomes 行解析失败: {e}")))?;
        let key = (
            v["sample_id"].as_str().unwrap_or_default().to_string(),
            v["model_id"].as_str().unwrap_or_default().to_string(),
        );
        outcomes.insert(
            key,
            (
                v["score"].as_f64().unwrap_or_default(),
                v["cost"].as_f64().unwrap_or_default(),
            ),
        );
    }
    Ok((test_instances, calib_instances, FrozenMatrix { outcomes }))
}

impl FrozenMatrix {
    pub fn lookup(&self, sample_id: &str, model_id: &str) -> Option<(f64, f64)> {
        self.outcomes.get(&(sample_id.to_string(), model_id.to_string())).copied()
    }

    /// calibration 统计：(dataset_id, model_id) → (mean_score, n)。只读 calibration split——
    /// 但矩阵只存了 outcome 键，切分信息在 instances 侧：调用方把 calibration 实例键集传入。
    pub fn mean_score(
        &self,
        calib_keys: &[(String, Vec<String>)], // (dataset_id, calibration sample_ids)
        model_id: &str,
    ) -> HashMap<String, (f64, usize)> {
        let mut acc: HashMap<String, (f64, usize)> = HashMap::new();
        for (dataset_id, sids) in calib_keys {
            let mut sum = 0.0;
            let mut n = 0;
            for sid in sids {
                if let Some((score, _)) = self.lookup(sid, model_id) {
                    sum += score;
                    n += 1;
                }
            }
            if n > 0 {
                acc.insert(dataset_id.clone(), (sum / n as f64, n));
            }
        }
        acc
    }
}

// ── 池构造与 policy ──

/// bench policy：显式 `min_capability_tier=1` 允许全池三层（RoutingPolicy::default 的 2 会把
/// weak 永久挡掉，routing headroom 将不可见）；权重沿用生产 default（cost .5/quality .4/latency .1）。
/// 此设定记入 bench_runs.parameter_json，是 B0 Plan Benchmark 的被测配置之一（Day 5 λ sweep 扫权重）。
pub fn bench_policy(task_type: &str) -> RoutingPolicy {
    RoutingPolicy {
        task_type: task_type.to_string(),
        min_capability_tier: 1,
        ..RoutingPolicy::default()
    }
}

/// calibration 统计包：等效单价 p_m + per (dataset, model) 校准均分 + (model, task_type) 质量先验。
pub struct CalibrationStats {
    /// model_id → 等效综合单价（USD/token，input=output 同价）
    pub unit_price: HashMap<String, f64>,
    /// (dataset_id, model_id) → calibration 均分（Best Single 用）
    pub dataset_mean: HashMap<(String, String), f64>,
    /// (model_id, task_type) → (ewma 均分, n)（quality_override 用，n≥5 才有效）
    pub task_quality: HashMap<(String, String), (f64, usize)>,
}

/// 从 calibration split 统计导出（**只读 calibration 侧**，泄漏红线）。
pub fn calibration_stats(
    matrix: &FrozenMatrix,
    pool: &BenchPool,
    calib_instances: &[BenchInstance],
) -> CalibrationStats {
    let mut dataset_mean = HashMap::new();
    for m in pool.model_ids() {
        let mut per_ds: HashMap<&str, (f64, usize)> = HashMap::new();
        for inst in calib_instances {
            if let Some((score, _)) = matrix.lookup(&inst.sample_id, m) {
                let e = per_ds.entry(inst.dataset_id.as_str()).or_insert((0.0, 0));
                e.0 += score;
                e.1 += 1;
            }
        }
        for (ds, (sum, n)) in per_ds {
            if n > 0 {
                dataset_mean.insert((ds.to_string(), m.to_string()), sum / n as f64);
            }
        }
    }

    // (model, task_type) 质量先验（production model_task_score 的 bench 等价物）
    let mut task_quality = HashMap::new();
    for m in pool.model_ids() {
        let mut per_tt: HashMap<&str, (f64, usize)> = HashMap::new();
        for inst in calib_instances {
            if let Some((score, _)) = matrix.lookup(&inst.sample_id, m) {
                let e = per_tt.entry(inst.task_type.as_str()).or_insert((0.0, 0));
                e.0 += score;
                e.1 += 1;
            }
        }
        for (tt, (sum, n)) in per_tt {
            task_quality.insert((m.to_string(), tt.to_string()), (sum / n as f64, n));
        }
    }

    // 等效综合单价：p_m = mean_cost_m / mean(est_in_q + 750)，est_in = chars×0.6（固定 profile）。
    let mut unit_price = HashMap::new();
    for m in pool.model_ids() {
        let mut cost_sum = 0.0;
        let mut token_sum = 0.0;
        let mut n = 0;
        for inst in calib_instances {
            if let Some((_, cost)) = matrix.lookup(&inst.sample_id, m) {
                cost_sum += cost;
                token_sum += est_in_tokens(&inst.prompt) as f64 + EST_OUT_COLD_START as f64;
                n += 1;
            }
        }
        if n > 0 && token_sum > 0.0 {
            unit_price.insert(m.to_string(), cost_sum / token_sum);
        }
    }
    CalibrationStats { unit_price, dataset_mean, task_quality }
}

pub fn est_in_tokens(prompt: &str) -> i64 {
    (prompt.chars().count() as f64 * EST_IN_CHARS_PER_TOKEN) as i64
}

/// 池 → 内存 Model 三元 + PriceSpec 表（等效单价，input=output 同价；price_source=bench_calibration）。
pub fn build_pool_models(pool: &BenchPool, stats: &CalibrationStats) -> (Vec<Model>, HashMap<(String, String), PriceSpec>) {
    let tiers = [(&pool.weak, 1), (&pool.mid, 2), (&pool.strong, 3)];
    let mut models = Vec::new();
    let mut specs = HashMap::new();
    for (id, tier) in tiers {
        let p = *stats.unit_price.get(id).unwrap_or(&0.0);
        models.push(Model {
            id: 0,
            name: id.to_string(),
            provider_model: id.to_string(),
            backend: Backend::Cloud {
                provider: crate::models::Provider::parse("custom"),
                api_base: None,
                api_key: None,
            },
            task_type: String::new(),
            input_cost_per_token: p,
            output_cost_per_token: p,
            rpm: 0,
            is_active: 1,
            capability_tier: tier,
            quality_score: 0.5,
            context_window: 128_000,
            supports_tools: 0,
            supports_vision: 0,
            supports_stream: 0,
            priority: 0,
            health_state: "healthy".to_string(),
            needs_calibration: 0,
        });
        specs.insert(
            ("custom".to_string(), id.to_string()),
            PriceSpec {
                provider: "custom".to_string(),
                model: id.to_string(),
                input_cost: p,
                output_cost: p,
                price_source: "bench_calibration".to_string(),
                ..PriceSpec::default()
            },
        );
    }
    (models, specs)
}

// ── PlanInput 组装（parity 基准：与生产填法同构）──

/// 一次 replay 的共享上下文（纯内存，零 DB/网络）。
pub struct ReplayCtx {
    pub pool: BenchPool,
    pub models: Vec<Model>,
    pub specs: HashMap<(String, String), PriceSpec>,
    pub zones: ZoneResolver,
    pub stats: CalibrationStats,
    /// quality_override 开关（E5 实验：w/o=false；w/=true 时用 calibration 先验，n≥5）
    pub use_calibration: bool,
}

impl ReplayCtx {
    pub fn new(pool: BenchPool, models: Vec<Model>, specs: HashMap<(String, String), PriceSpec>, stats: CalibrationStats) -> Self {
        Self { pool, models, specs, zones: ZoneResolver::new(), stats, use_calibration: false }
    }

    fn quality_override(&self, task_type: &str) -> HashMap<String, f64> {
        let mut m = HashMap::new();
        if self.use_calibration {
            for model_id in self.pool.model_ids() {
                if let Some((q, n)) = self.stats.task_quality.get(&(model_id.to_string(), task_type.to_string())) {
                    if *n >= CALIBRATION_MIN_SAMPLES {
                        m.insert(model_id.to_string(), q.clamp(0.0, 1.0));
                    }
                }
            }
        }
        m
    }

    /// 组装 PlanInput：填法与生产同构（band_for 现算 / est_in=chars×0.6 / est_out=750 冷启动 /
    /// hit_rate 全零 / sticky=None / budget=normal / deferrable=false / t_epoch=0 确定性）。
    pub fn build_plan_input<'a>(
        &'a self,
        inst: &'a BenchInstance,
        policy: &'a RoutingPolicy,
        quality_override: &'a HashMap<String, f64>,
        hit_rate: &'a HashMap<String, f64>,
    ) -> PlanInput<'a> {
        PlanInput {
            task_type: &inst.task_type,
            band: router::band_for(&inst.task_type, &inst.prompt),
            policy,
            models: &self.models,
            price_specs: &self.specs,
            zones: &self.zones,
            t_epoch_secs: 0, // 确定性回放：无时段价漂移（公共池 PriceSpec 无 zone/tiered）
            est_in_tokens: est_in_tokens(&inst.prompt),
            est_out_tokens: EST_OUT_COLD_START,
            quality_override,
            budget_tier: "normal",
            hit_rate,
            last_model_conv: None,
            conv_sticky: None,
            deferrable: false,
        }
    }
}

// ── 策略 ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    Lloom,
    AlwaysWeak,
    AlwaysMid,
    AlwaysStrong,
    Cheapest,
    Random,
    BestSingle,
    Oracle,
}

impl Strategy {
    pub const ALL: [Strategy; 8] = [
        Strategy::AlwaysWeak,
        Strategy::AlwaysMid,
        Strategy::AlwaysStrong,
        Strategy::Cheapest,
        Strategy::Random,
        Strategy::BestSingle,
        Strategy::Oracle,
        Strategy::Lloom,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Strategy::Lloom => "lloom",
            Strategy::AlwaysWeak => "always_weak",
            Strategy::AlwaysMid => "always_mid",
            Strategy::AlwaysStrong => "always_strong",
            Strategy::Cheapest => "cheapest",
            Strategy::Random => "random",
            Strategy::BestSingle => "best_single",
            Strategy::Oracle => "oracle",
        }
    }

    pub fn parse(s: &str) -> Option<Strategy> {
        Strategy::ALL.iter().copied().find(|st| st.name() == s)
    }
}

/// xorshift64*：确定性伪随机（无 rand 依赖新路径；seed 固定 3407，10 号 §12.3）。
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 单策略回放：Core 实例 → 每题选模 → 冻结矩阵查 (score, cost) → RunItem 列表。
/// Oracle 是 hindsight 上界（读 test 侧 argmax，仅供 regret/上界对照，永不进 quality_override）。
pub fn replay(
    ctx: &ReplayCtx,
    matrix: &FrozenMatrix,
    instances: &[BenchInstance],
    strategy: Strategy,
) -> Result<Vec<RunItem>> {
    let mut items = Vec::with_capacity(instances.len());
    // Best Single：per dataset 由 calibration 均分 argmax（§12.1）；缺失时回落 strong。
    let mut best_single_cache: HashMap<String, String> = HashMap::new();
    let mut rng = Lcg(RANDOM_SEED);
    let empty_hit: HashMap<String, f64> = HashMap::new(); // 红线：hit_rate 恒全零

    for inst in instances {
        let (selected, decision) = match strategy {
            Strategy::AlwaysWeak | Strategy::AlwaysMid | Strategy::AlwaysStrong => {
                let m = match strategy {
                    Strategy::AlwaysWeak => ctx.pool.weak.clone(),
                    Strategy::AlwaysMid => ctx.pool.mid.clone(),
                    _ => ctx.pool.strong.clone(),
                };
                (m, json!({ "strategy": strategy.name(), "rule": "constant" }))
            }
            Strategy::Cheapest => {
                // §12.2：成本只来自 calibration 导出单价，不看 test score
                let m = ctx
                    .pool
                    .model_ids()
                    .iter()
                    .min_by(|a, b| {
                        ctx.stats
                            .unit_price
                            .get(**a)
                            .unwrap_or(&f64::INFINITY)
                            .total_cmp(ctx.stats.unit_price.get(**b).unwrap_or(&f64::INFINITY))
                    })
                    .map(|s| s.to_string())
                    .ok_or_else(|| AppError::InvalidRequest("空池".into()))?;
                (m, json!({ "strategy": strategy.name(), "rule": "argmin unit_price(calibration)" }))
            }
            Strategy::Random => {
                let ids = ctx.pool.model_ids();
                let m = ids[rng.below(3) as usize].to_string();
                (m, json!({ "strategy": strategy.name(), "seed": RANDOM_SEED, "rule": "uniform" }))
            }
            Strategy::BestSingle => {
                let m = best_single_cache
                    .entry(inst.dataset_id.clone())
                    .or_insert_with(|| {
                        ctx.pool
                            .model_ids()
                            .iter()
                            .copied()
                            .max_by(|a, b| {
                                let va = ctx.stats.dataset_mean.get(&(inst.dataset_id.clone(), a.to_string())).copied().unwrap_or(f64::NEG_INFINITY);
                                let vb = ctx.stats.dataset_mean.get(&(inst.dataset_id.clone(), b.to_string())).copied().unwrap_or(f64::NEG_INFINITY);
                                va.total_cmp(&vb)
                            })
                            .unwrap_or(&ctx.pool.strong)
                            .to_string()
                    })
                    .clone();
                (m, json!({ "strategy": strategy.name(), "rule": "argmax calibration mean per dataset" }))
            }
            Strategy::Oracle => {
                let ids = ctx.pool.model_ids();
                let best = ids
                    .iter()
                    .enumerate()
                    .filter_map(|(i, m)| matrix.lookup(&inst.sample_id, m).map(|(s, _)| (s, i, m)))
                    .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1))) // 并列取列表序前者（确定性）
                    .ok_or_else(|| {
                        AppError::InvalidRequest(format!("oracle lookup 失败（矩阵覆盖缺口）: {}", inst.sample_id))
                    })?;
                (
                    best.2.to_string(),
                    json!({ "strategy": strategy.name(), "rule": "argmax test score (hindsight upper bound)" }),
                )
            }
            Strategy::Lloom => {
                let policy = bench_policy(&inst.task_type);
                let q = ctx.quality_override(&inst.task_type);
                let input = ctx.build_plan_input(inst, &policy, &q, &empty_hit);
                let outcome = router::plan(&input).map_err(|e| {
                    AppError::InvalidRequest(format!("plan() 失败 @ {}: {e}", inst.sample_id))
                })?;
                let decision = json!({
                    "strategy": strategy.name(),
                    "task_type": inst.task_type,
                    "band": router::band_for(&inst.task_type, &inst.prompt),
                    "est_in_tokens": est_in_tokens(&inst.prompt),
                    "est_out_tokens": EST_OUT_COLD_START,
                    "quality_override": ctx.use_calibration,
                    "candidates": outcome.candidates.iter().map(|c| json!({
                        "model": c.name, "score": c.score, "est_cost": c.est_cost,
                        "quality": c.quality, "capability_tier": c.capability_tier,
                    })).collect::<Vec<_>>(),
                    "selected_model": outcome.primary,
                });
                (decision["selected_model"].as_str().unwrap_or_default().to_string(), decision)
            }
        };

        let (score, cost) = matrix
            .lookup(&inst.sample_id, &selected)
            .ok_or_else(|| {
                AppError::InvalidRequest(format!(
                    "冻结矩阵缺 outcome：({}/{})——Gate 1 coverage 缺口，拒绝进入 headline",
                    inst.sample_id, selected
                ))
            })?;
        // oracle 参照（regret 计算；lloom/全部策略统一带）
        let ids = ctx.pool.model_ids();
        let (oracle_model, oracle_score) = ids
            .iter()
            .enumerate()
            .filter_map(|(i, m)| matrix.lookup(&inst.sample_id, m).map(|(s, _)| (s, i, m)))
            .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)))
            .map(|(s, _, m)| (m.to_string(), s))
            .unwrap_or((selected.clone(), score));
        items.push(RunItem {
            dataset_id: inst.dataset_id.clone(),
            sample_id: inst.sample_id.clone(),
            selected_model: selected,
            score,
            cost,
            oracle_model,
            oracle_score,
            regret: oracle_score - score,
            decision_json: serde_json::to_string(&decision)
                .unwrap_or_else(|_| "{}".to_string()),
        });
    }
    Ok(items)
}

/// 单条回放结果（bench_run_items 行）。
pub struct RunItem {
    pub dataset_id: String,
    pub sample_id: String,
    pub selected_model: String,
    pub score: f64,
    pub cost: f64,
    pub oracle_model: String,
    pub oracle_score: f64,
    pub regret: f64,
    pub decision_json: String,
}

// ── 指标汇总（10 号 §13；bootstrap CI 留 report 层）──

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 * p).ceil() as usize).saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}

/// 聚合一组 items（单 dataset 或全量）：Q/C/regret P50/P95/选择分布。
/// saving/retention/gap_recovery 需要 Always-Strong/Always-Weak 基准行，在对比层（CLI/report）计算。
pub fn summarize(items: &[RunItem]) -> Value {
    let n = items.len() as f64;
    let q = items.iter().map(|i| i.score).sum::<f64>() / n.max(1.0);
    let c = items.iter().map(|i| i.cost).sum::<f64>() / n.max(1.0);
    let mut regrets: Vec<f64> = items.iter().map(|i| i.regret).collect();
    regrets.sort_by(f64::total_cmp);
    let mut dist: HashMap<String, u64> = HashMap::new();
    for i in items {
        *dist.entry(i.selected_model.clone()).or_default() += 1;
    }
    json!({
        "n": items.len(),
        "quality_mean": q,
        "cost_mean": c,
        "regret_mean": regrets.iter().sum::<f64>() / n.max(1.0),
        "regret_p50": percentile(&regrets, 0.50),
        "regret_p95": percentile(&regrets, 0.95),
        "selection_dist": dist,
    })
}

// ── bench.db（决策 P1a：独立库，生产零接触）──

pub fn open_bench_db(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(path)
        .map_err(|e| AppError::InvalidRequest(format!("bench.db 打开失败 {path:?}: {e}")))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS bench_runs (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           benchmark_id TEXT NOT NULL,
           manifest_hash TEXT NOT NULL,
           pool_id TEXT NOT NULL,
           strategy TEXT NOT NULL,
           parameter_json TEXT NOT NULL,
           split TEXT NOT NULL,
           created_at TEXT DEFAULT (datetime('now')),
           summary_json TEXT
         );
         CREATE TABLE IF NOT EXISTS bench_run_items (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           run_id INTEGER NOT NULL REFERENCES bench_runs(id),
           dataset_id TEXT NOT NULL,
           sample_id TEXT NOT NULL,
           selected_model TEXT NOT NULL,
           score REAL NOT NULL,
           cost REAL NOT NULL,
           oracle_model TEXT,
           oracle_score REAL,
           regret REAL,
           decision_json TEXT,
           UNIQUE(run_id, dataset_id, sample_id)
         );
         CREATE INDEX IF NOT EXISTS idx_bri_run ON bench_run_items(run_id);",
    )
    .map_err(|e| AppError::InvalidRequest(format!("bench.db 建表失败: {e}")))?;
    Ok(conn)
}

/// 一次 run 的元数据（save_run 参数聚合）。
pub struct RunMeta<'a> {
    pub benchmark_id: &'a str,
    pub manifest_hash: &'a str,
    pub pool_id: &'a str,
    pub strategy: &'a str,
    pub parameter_json: &'a str,
    pub split: &'a str,
    pub summary_json: &'a str,
}

/// 落库一次 run（items 事务批量插入），返回 run_id。
pub fn save_run(conn: &Connection, meta: &RunMeta, items: &[RunItem]) -> Result<i64> {
    conn.execute(
        "INSERT INTO bench_runs (benchmark_id, manifest_hash, pool_id, strategy, parameter_json, split, summary_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            meta.benchmark_id,
            meta.manifest_hash,
            meta.pool_id,
            meta.strategy,
            meta.parameter_json,
            meta.split,
            meta.summary_json
        ],
    )
    .map_err(|e| AppError::InvalidRequest(format!("bench_runs 插入失败: {e}")))?;
    let run_id = conn.last_insert_rowid();
    conn.execute_batch("BEGIN")
        .map_err(|e| AppError::InvalidRequest(format!("事务开启失败: {e}")))?;
    for i in items {
        conn.execute(
            "INSERT INTO bench_run_items
             (run_id, dataset_id, sample_id, selected_model, score, cost, oracle_model, oracle_score, regret, decision_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params![
                run_id, i.dataset_id, i.sample_id, i.selected_model, i.score, i.cost,
                i.oracle_model, i.oracle_score, i.regret, i.decision_json
            ],
        )
        .map_err(|e| AppError::InvalidRequest(format!("bench_run_items 插入失败: {e}")))?;
    }
    conn.execute_batch("COMMIT")
        .map_err(|e| AppError::InvalidRequest(format!("事务提交失败: {e}")))?;
    Ok(run_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool() -> BenchPool {
        BenchPool {
            weak: "weak_m".into(),
            mid: "mid_m".into(),
            strong: "strong_m".into(),
        }
    }

    fn matrix_fixture() -> FrozenMatrix {
        // 3 实例 × 3 模型手工矩阵（覆盖全池）
        let rows = [
            ("s1", "weak_m", 0.0, 0.0001), ("s1", "mid_m", 1.0, 0.0005), ("s1", "strong_m", 1.0, 0.003),
            ("s2", "weak_m", 1.0, 0.0001), ("s2", "mid_m", 0.0, 0.0004), ("s2", "strong_m", 1.0, 0.004),
            ("s3", "weak_m", 0.0, 0.0001), ("s3", "mid_m", 1.0, 0.0006), ("s3", "strong_m", 0.0, 0.002),
        ];
        let mut outcomes = HashMap::new();
        for (s, m, sc, c) in rows {
            outcomes.insert((s.to_string(), m.to_string()), (sc, c));
        }
        FrozenMatrix { outcomes }
    }

    fn instance(sid: &str, task: &str) -> BenchInstance {
        BenchInstance {
            dataset_id: "d1".into(),
            sample_id: sid.into(),
            task_type: task.into(),
            prompt: "计算 1+1 并解释".into(),
        }
    }

    fn ctx() -> ReplayCtx {
        let m = matrix_fixture();
        let calib = vec![instance("s1", "math_logic"), instance("s2", "math_logic"), instance("s3", "math_logic")];
        let stats = calibration_stats(&m, &pool(), &calib);
        let (models, specs) = build_pool_models(&pool(), &stats);
        ReplayCtx::new(pool(), models, specs, stats)
    }

    /// Gate 1：Core 实例 × 全池 outcome 覆盖率必须 100%（缺口即拒绝 replay）。
    #[test]
    fn coverage_full_or_reject() {
        let m = matrix_fixture();
        let ctx = ctx();
        let insts = vec![instance("s1", "general"), instance("s2", "general"), instance("s3", "general")];
        for st in Strategy::ALL {
            let items = replay(&ctx, &m, &insts, st).unwrap_or_else(|e| panic!("{st:?}: {e}"));
            assert_eq!(items.len(), 3);
        }
    }

    /// 红线自检：hit_rate 恒全零、est_out=750、band 由 band_for 现算、sticky=None。
    #[test]
    fn plan_input_freeze_semantics() {
        let c = ctx();
        let inst = instance("s1", "math_logic");
        let policy = bench_policy(&inst.task_type);
        let q = HashMap::new();
        let h: HashMap<String, f64> = HashMap::new();
        let input = c.build_plan_input(&inst, &policy, &q, &h);
        assert_eq!(input.est_out_tokens, EST_OUT_COLD_START);
        assert_eq!(input.est_in_tokens, est_in_tokens(&inst.prompt));
        assert!(input.hit_rate.is_empty());
        assert!(input.last_model_conv.is_none() && input.conv_sticky.is_none());
        assert_eq!(input.budget_tier, "normal");
        assert!(!input.deferrable);
        assert_eq!(input.t_epoch_secs, 0);
        assert_eq!(input.band, router::band_for("math_logic", &inst.prompt));
        // policy：min_tier=1 全池放行（default 2 会挡 weak）
        assert_eq!(policy.min_capability_tier, 1);
    }

    /// parity：bench 组装的 PlanInput 直接走 production plan()，
    /// 且与按生产语义手工组装的输入产生相同决策（10 号 §二十四）。
    #[test]
    fn parity_bench_assembly_matches_production() {
        let c = ctx();
        let inst = instance("s1", "math_logic");
        let policy = bench_policy(&inst.task_type);
        let q = HashMap::new();
        let h: HashMap<String, f64> = HashMap::new();
        let bench_input = c.build_plan_input(&inst, &policy, &q, &h);
        let via_bench = router::plan(&bench_input).unwrap();

        // 生产语义手工组装（review.rs/server.rs 填法逐字段对照）
        let prod_input = PlanInput {
            task_type: "math_logic",
            band: router::band_for("math_logic", "计算 1+1 并解释"),
            policy: &policy,
            models: &c.models,
            price_specs: &c.specs,
            zones: &c.zones,
            t_epoch_secs: 0,
            est_in_tokens: (inst.prompt.chars().count() as f64 * 0.6) as i64,
            est_out_tokens: 750,
            quality_override: &q,
            budget_tier: "normal",
            hit_rate: &h,
            last_model_conv: None,
            conv_sticky: None,
            deferrable: false,
        };
        let via_prod = router::plan(&prod_input).unwrap();
        assert_eq!(via_bench.primary, via_prod.primary);
        assert_eq!(via_bench.candidates.len(), via_prod.candidates.len());
        for (b, p) in via_bench.candidates.iter().zip(&via_prod.candidates) {
            assert_eq!(b.name, p.name);
            assert!((b.score - p.score).abs() < 1e-12);
            assert!((b.est_cost - p.est_cost).abs() < 1e-12);
        }
    }

    /// Oracle=hindsight 上界；Cheapest 不看分数（仅 calibration 单价）；Random 确定性。
    #[test]
    fn strategy_semantics() {
        let c = ctx();
        let m = matrix_fixture();
        let insts = vec![instance("s1", "general"), instance("s2", "general"), instance("s3", "general")];

        let oracle = replay(&c, &m, &insts, Strategy::Oracle).unwrap();
        assert_eq!(oracle[0].selected_model, "mid_m"); // s1: mid/strong 并列 1.0，取列表序前者（mid）
        assert_eq!(oracle[1].selected_model, "weak_m"); // s2: weak/strong 并列 1.0 → weak
        assert_eq!(oracle[2].selected_model, "mid_m"); // s3: 仅 mid=1.0

        let cheapest = replay(&c, &m, &insts, Strategy::Cheapest).unwrap();
        assert!(cheapest.iter().all(|i| i.selected_model == "weak_m")); // weak 单价最低

        let a = replay(&c, &m, &insts, Strategy::Random).unwrap();
        let b = replay(&c, &m, &insts, Strategy::Random).unwrap();
        assert_eq!(a.iter().map(|i| &i.selected_model).collect::<Vec<_>>(),
                   b.iter().map(|i| &i.selected_model).collect::<Vec<_>>());

        // lloom 策略 decision_json 含全候选评分（candidates 序列化，10 号 §二十三）。
        // simple_qa → band=easy → tier_req=1 全池放行；general（medium）会按生产门槛淘汰 weak，
        // 恰是 B0 要测量的行为，故全池断言用 simple_qa。
        let simple = vec![instance("s1", "simple_qa"), instance("s2", "simple_qa"), instance("s3", "simple_qa")];
        let lloom = replay(&c, &m, &simple, Strategy::Lloom).unwrap();
        let d: Value = serde_json::from_str(&lloom[0].decision_json).unwrap();
        assert_eq!(d["candidates"].as_array().unwrap().len(), 3);
        assert!(d["band"].is_string());
    }

    /// 等效单价只来自 calibration（构造 cost 全等时单价并列，cheapest 取列表序前者=weak）。
    #[test]
    fn calibration_price_uses_only_calibration() {
        let m = matrix_fixture();
        let calib = vec![instance("s1", "math_logic")];
        let stats = calibration_stats(&m, &pool(), &calib);
        assert!(stats.unit_price.values().all(|p| *p > 0.0));
        // p_weak = 0.0001/(est_in+750) < p_mid < p_strong（fixture cost 单调）
        let pw = stats.unit_price["weak_m"];
        let ps = stats.unit_price["strong_m"];
        assert!(pw < ps);
    }
}
