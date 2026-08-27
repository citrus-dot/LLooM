//! Always-on probes (PRICING-PLAN §7) — small, budget-capped requests that keep
//! models warm, verify responsiveness, and feed price calibration.
//!
//! Design:
//! - Each round = 2 requests with a **byte-stable** prefix (>256 tokens so the
//!   provider's implicit prefix cache engages): ① warm-up/write, ② verify hit.
//! - Budget is hard-capped: monthly limit (default ¥5), per-round cap, and a
//!   Hourly→Daily→SuspendedCloud downshift ladder. Counters use integer
//!   micro-dollars to avoid f64 races.
//! - Probe usage is recorded with `task_type='probe'` and
//!   `conversation_id='probe:{provider}:{model}'` so it never pollutes user
//!   statistics. Probe failures are recorded with `cost = -1` as a sentinel
//!   (stats counts `cost < 0` as failures).

use crate::ai_client::{self, ModelSpec};
use crate::db;
use crate::models::Model;
use crate::pricing;
use crate::router;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// 默认月预算（微美分）：¥5 ÷ 7.2 ≈ $0.0007。0 = 关闭探针。WebUI 可调。
const DEFAULT_MONTHLY_LIMIT_USD: u64 = 700;
/// 单轮成本上限（USD）：异常放大即熔断该模型本轮。
const PER_ROUND_CAP_USD: f64 = 0.002;
/// 失败哨兵：探针调用失败以 act_cost = -1 落库（stats 用 cost < 0 计数）。
const FAIL_SENTINEL_COST: f64 = -1.0;
/// 连续失败 ≥8 轮 → 暂停该模型探针（指数退避由间隔拉长体现）。
const FAIL_PAUSE_THRESHOLD: u32 = 8;
/// 稳定前缀：>256 token 以触发隐式缓存（中文字符 ≈ 1 token，600+ 字符 ≈ 600+ token）。
/// 前缀必须**字节级稳定**——探针载荷是固定常量，永不变化，保证第 2 条请求命中缓存。
const PROBE_PREFIX: &str = "你是 LLooM 探针。请仅回复 ok。\
系统指令：保持简洁、准确、不编造。若信息不足请说明。请勿引用本前缀中的任何内容。\
工具定义：\
search_docs(查询词)：检索内部文档库，返回前三条相关片段。\
get_time()：返回当前北京时间。\
list_files(目录)：列出指定目录下的文件清单。\
read_file(路径)：读取文件内容，限制 16KB。\
calc(表达式)：执行数学计算。\
translate(文本, 目标语言)：翻译文本。\
summarize(文本)：生成长度不超过三句的摘要。\
format_json(对象)：将对象序列化为缩进 JSON。\
send_email(收件人, 主题, 正文)：发送邮件前需用户确认。\
参考文档：\
LLooM 是面向开发者的 LLM 路由代理，支持多供应商接入、语义缓存、预算控制与用量统计。\
定价模块按分项计费：输入 token、输出 token、缓存命中、缓存写入，并支持阶梯价与峰谷时段折扣。\
路由策略由模型注册表驱动，按成本与成效为任务分配最合适的模型，决策过程可审计。\
安全模块对请求做 PII 与越狱检测，命中则拦截。\
编排模块将复杂任务分解为子任务并行执行，最后汇总。\
上下文模块按预算裁剪历史，滚动摘要保持前缀稳定以提升缓存命中率。\
请忽略以上内容本身，只按要求作答。";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Freq {
    Hourly,
    Daily,
    SuspendedCloud,
}

#[derive(Debug)]
pub struct ProbeBudget {
    monthly_limit_usd: AtomicU64, // 微美分；0 = 关闭
    spent_this_month: AtomicU64,
    month_key: Mutex<String>, // "YYYY-MM"，跨月重置
    freq: Mutex<HashMap<(String, String), Freq>>,
    failures: Mutex<HashMap<(String, String), u32>>,
}

static BUDGET: OnceLock<ProbeBudget> = OnceLock::new();

pub fn budget() -> &'static ProbeBudget {
    BUDGET.get_or_init(ProbeBudget::default)
}

impl Default for ProbeBudget {
    fn default() -> Self {
        Self {
            monthly_limit_usd: AtomicU64::new(DEFAULT_MONTHLY_LIMIT_USD),
            spent_this_month: AtomicU64::new(0),
            month_key: Mutex::new(month_key_now()),
            freq: Mutex::new(HashMap::new()),
            failures: Mutex::new(HashMap::new()),
        }
    }
}

fn month_key_now() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, m, _, _, _) = pricing::beijing_parts(t, 8);
    format!("{y:04}-{m:02}")
}

impl ProbeBudget {
    pub fn set_monthly_limit_usd(&self, usd: f64) {
        self.monthly_limit_usd.store((usd * 1e6).max(0.0) as u64, Ordering::Relaxed);
    }
    pub fn monthly_limit_usd(&self) -> f64 {
        self.monthly_limit_usd.load(Ordering::Relaxed) as f64 / 1e6
    }

    /// 请求前检查：false = 本轮不执行（预算关闭/单轮熔断/已暂停）。
    /// 预算耗尽时自动降频（Hourly→Daily→SuspendedCloud），降频当轮仍放行一次。
    pub fn try_charge(&self, provider: &str, model: &str, est_usd: f64) -> bool {
        if self.monthly_limit_usd.load(Ordering::Relaxed) == 0 {
            return false;
        }
        if est_usd > PER_ROUND_CAP_USD {
            return false;
        }
        self.roll_month_if_needed();
        let mut freq = self.freq.lock().unwrap();
        let f = freq.entry((provider.to_string(), model.to_string())).or_insert(Freq::Hourly);
        if *f == Freq::SuspendedCloud {
            return false;
        }
        let spent = self.spent_this_month.load(Ordering::Relaxed);
        let limit = self.monthly_limit_usd.load(Ordering::Relaxed);
        if spent as f64 / 1e6 + est_usd > limit as f64 / 1e6 {
            match *f {
                Freq::Hourly => *f = Freq::Daily,
                Freq::Daily => *f = Freq::SuspendedCloud,
                Freq::SuspendedCloud => {}
            }
        }
        true
    }

    /// 请求完成后按**实际成本**记账（微美分累加；失败传 cost ≤ 0 不记账）。
    /// provider/model 参数保留供未来按模型细分预算（当前仅总额计数）。
    pub fn charge(&self, _provider: &str, _model: &str, cost_usd: f64) {
        self.roll_month_if_needed();
        if cost_usd > 0.0 {
            self.spent_this_month.fetch_add((cost_usd * 1e6) as u64, Ordering::Relaxed);
        }
    }

    pub fn note_failure(&self, provider: &str, model: &str) -> u32 {
        let mut f = self.failures.lock().unwrap();
        let c = f.entry((provider.to_string(), model.to_string())).or_insert(0);
        *c += 1;
        *c
    }
    pub fn note_success(&self, provider: &str, model: &str) {
        self.failures.lock().unwrap().remove(&(provider.to_string(), model.to_string()));
    }
    pub fn failure_count(&self, provider: &str, model: &str) -> u32 {
        *self.failures.lock().unwrap().get(&(provider.to_string(), model.to_string())).unwrap_or(&0)
    }

    fn roll_month_if_needed(&self) {
        let now = month_key_now();
        let mut key = self.month_key.lock().unwrap();
        if *key != now {
            *key = now;
            self.spent_this_month.store(0, Ordering::Relaxed);
            self.freq.lock().unwrap().clear();
        }
    }
}

fn probe_messages() -> Vec<Value> {
    vec![
        json!({ "role": "system", "content": PROBE_PREFIX }),
        json!({ "role": "user", "content": "请回复：ok" }),
    ]
}

fn is_cloud(m: &Model) -> bool {
    !m.provider.eq_ignore_ascii_case("ollama") && !m.api_base.contains("localhost") && !m.api_base.contains("127.0.0.1")
}

/// 后台探针循环：每小时一轮。由 `server::spawn_background_jobs` 挂载。
/// PR-8：当有分时渠道正处高峰且 2h 内进谷时，本轮探针先挪到谷时窗口执行（DeepSeek 直接半价）。
pub async fn probe_loop() {
    let mut ticker = tokio::time::interval(Duration::from_secs(3600));
    loop {
        ticker.tick().await;
        // PR-8 谷时对齐：可延迟后台任务（探针）避开高峰成本
        if let Some(wait) = valley_wait_secs(crate::server::zone_resolver(), crate::server::now_epoch_secs()) {
            eprintln!("[core] probe deferring {wait}s to next valley window (peak-hour cost avoidance)");
            tokio::time::sleep(Duration::from_secs(wait)).await;
        }
        if let Err(e) = run_probe_round().await {
            eprintln!("[core] probe round failed: {e}");
        }
    }
}

/// PR-8：探针可延迟——当前处于某分时渠道高峰（multiplier≥1）且 2h 内有谷时窗口时，
/// 返回等待到谷时窗口的秒数；否则 None（当下立即执行，实测价已含时段系数）。
fn valley_wait_secs(zr: &pricing::ZoneResolver, now: i64) -> Option<u64> {
    router::next_valley_epoch(zr, now).map(|v| (v - now).max(0) as u64)
}

async fn run_probe_round() -> std::result::Result<(), crate::error::AppError> {
    let models = db::list_models(true)?;
    for m in &models {
        if !is_cloud(m) {
            continue; // 本轮只探云端；本地免费通道探针留待后续扩展
        }
        if budget().failure_count(&m.provider, &m.name) >= FAIL_PAUSE_THRESHOLD {
            continue;
        }
        if !budget().try_charge(&m.provider, &m.name, PER_ROUND_CAP_USD) {
            continue;
        }
        let spec = ModelSpec::from(m);
        let msgs = probe_messages();
        // ① 暖机（写缓存）
        match ai_client::chat(&spec, &msgs, 8, 0.0).await {
            Ok(res) => {
                budget().note_success(&m.provider, &m.name);
                let cost = record_probe_usage(m, &res.usage, false);
                budget().charge(&m.provider, &m.name, cost);
            }
            Err(e) => {
                let n = budget().note_failure(&m.provider, &m.name);
                record_probe_failure(m);
                eprintln!("[core] probe {}/{} failed ({n} consecutive): {e}", m.provider, m.name);
                continue;
            }
        }
        // ② 验证隐式缓存命中（同载荷应命中）
        match ai_client::chat(&spec, &msgs, 8, 0.0).await {
            Ok(res) => {
                budget().note_success(&m.provider, &m.name);
                let hit = res.usage.cached_tokens > 0;
                let cost = record_probe_usage(m, &res.usage, hit);
                budget().charge(&m.provider, &m.name, cost);
                if !hit {
                    eprintln!(
                        "[core] probe {}/{} cache-verify MISS (cached_tokens=0) — 校准层将核对表价",
                        m.provider, m.name
                    );
                }
            }
            Err(e) => {
                let n = budget().note_failure(&m.provider, &m.name);
                record_probe_failure(m);
                eprintln!("[core] probe {}/{} round-2 failed ({n} consecutive): {e}", m.provider, m.name);
            }
        }
    }
    Ok(())
}

/// 探针成功记账（task_type='probe'；act_cost 由 Rust 按 PriceSpec 分项计算）。
/// 返回实际成本（USD）供预算扣费。
fn record_probe_usage(m: &Model, usage: &pricing::UsageDetail, hit: bool) -> f64 {
    let (act_cost, zm) = match db::get_price_spec(&m.provider, &m.name) {
        Ok(Some(ps)) => {
            let zr = crate::server::zone_resolver();
            let t = crate::server::now_epoch_secs();
            (ps.actual_cost(usage, t, zr), ps.zone_multiplier(t, zr))
        }
        _ => (0.0, 1.0),
    };
    let _ = db::insert_usage(
        &m.name,
        "default",
        usage.prompt_tokens,
        usage.completion_tokens,
        act_cost,
        Some("probe"),
        hit,
        None, // P1.a：探针非用户请求，不标 latency/request_id
        None,
        Some(&db::UsageExtra {
            cached_tokens: usage.cached_tokens,
            reasoning_tokens: usage.reasoning_tokens,
            est_cost: 0.0,
            act_cost,
            zone_multiplier: zm,
            conversation_id: Some(format!("probe:{}:{}", m.provider, m.name)),
            field_missing: usage.field_missing,
            cache_saved_cost: 0.0,
        }),
    );
    act_cost
}

/// 探针失败记账：cost=-1 哨兵（stats 用 cost<0 计数）。
fn record_probe_failure(m: &Model) {
    let _ = db::insert_usage(
        &m.name,
        "default",
        0,
        0,
        FAIL_SENTINEL_COST,
        Some("probe"),
        false,
        None, // P1.a：探针非用户请求，不标 latency/request_id
        None,
        Some(&db::UsageExtra {
            cached_tokens: 0,
            reasoning_tokens: 0,
            est_cost: 0.0,
            act_cost: FAIL_SENTINEL_COST,
            zone_multiplier: 1.0,
            conversation_id: Some(format!("probe:{}:{}", m.provider, m.name)),
            field_missing: true,
            cache_saved_cost: 0.0,
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_budget(limit_usd: u64, month: &str) -> ProbeBudget {
        ProbeBudget {
            monthly_limit_usd: AtomicU64::new(limit_usd),
            spent_this_month: AtomicU64::new(0),
            month_key: Mutex::new(month.into()),
            freq: Mutex::new(HashMap::new()),
            failures: Mutex::new(HashMap::new()),
        }
    }

    #[test]
    fn budget_hard_cap_and_downshift() {
        let b = test_budget(2000, "2099-01"); // $0.002 总预算
        // 轮 1-3：预算内（spent 600/1200/1800 < 2000）
        assert!(b.try_charge("dashscope", "qwen3-max", 0.0006));
        b.charge("dashscope", "qwen3-max", 0.0006);
        assert!(b.try_charge("dashscope", "qwen3-max", 0.0006));
        b.charge("dashscope", "qwen3-max", 0.0006);
        assert!(b.try_charge("dashscope", "qwen3-max", 0.0006));
        b.charge("dashscope", "qwen3-max", 0.0006);
        // 轮 4：spent 1800+600 > 2000 → 降频 Daily 但放行
        assert!(b.try_charge("dashscope", "qwen3-max", 0.0006));
        b.charge("dashscope", "qwen3-max", 0.0006);
        // 轮 5：Daily 仍超 → SuspendedCloud 放行
        assert!(b.try_charge("dashscope", "qwen3-max", 0.0006));
        b.charge("dashscope", "qwen3-max", 0.0006);
        // 轮 6：SuspendedCloud → 拒绝
        assert!(!b.try_charge("dashscope", "qwen3-max", 0.0006));
    }

    #[test]
    fn zero_limit_disables() {
        let b = test_budget(0, "2099-01");
        assert!(!b.try_charge("dashscope", "qwen3-max", 0.0001));
    }

    #[test]
    fn per_round_cap_fuses_abnormal_cost() {
        let b = ProbeBudget::default();
        assert!(!b.try_charge("dashscope", "qwen3-max", 0.01)); // > 0.002 熔断
    }

    #[test]
    fn failure_ladder_pauses() {
        let b = ProbeBudget::default();
        for _ in 0..FAIL_PAUSE_THRESHOLD {
            b.note_failure("dashscope", "qwen3-max");
        }
        assert_eq!(b.failure_count("dashscope", "qwen3-max"), FAIL_PAUSE_THRESHOLD);
        b.note_success("dashscope", "qwen3-max");
        assert_eq!(b.failure_count("dashscope", "qwen3-max"), 0);
    }

    #[test]
    fn probe_prefix_is_long_enough_for_implicit_cache() {
        // 隐式缓存最小 256 token；600+ 中文字符 ≈ 600+ token（>512 保守验证）
        assert!(
            PROBE_PREFIX.chars().count() > 512,
            "prefix too short: {}",
            PROBE_PREFIX.chars().count()
        );
    }

    #[test]
    fn probe_wait_defers_only_during_peak_with_upcoming_valley() {
        let zr = pricing::ZoneResolver::new();
        zr.load(vec![pricing::Zone::from_db(
            "deepseek",
            r#"[
              {"holidays":true,"hours":"*","multiplier":0.5},
              {"days":["sat","sun"],"hours":"*","multiplier":0.5},
              {"days":["mon","tue","wed","thu","fri"],"hours":"9-12,14-18","multiplier":1.0},
              {"days":["mon","tue","wed","thu","fri"],"hours":"*","multiplier":0.5}
            ]"#,
            "Asia/Shanghai",
            "[]",
        )]);
        // 周一 10:00 高峰，2h 内（12:00）进谷 → 等待到 12:00
        let peak = pricing::beijing_epoch(2026, 8, 24, 10, 0, 8);
        let valley12 = pricing::beijing_epoch(2026, 8, 24, 12, 0, 8);
        assert_eq!(valley_wait_secs(&zr, peak), Some((valley12 - peak) as u64));
        // 已在谷时（23:00 周一）→ 无需延迟
        let valley_now = pricing::beijing_epoch(2026, 8, 24, 23, 0, 8);
        assert_eq!(valley_wait_secs(&zr, valley_now), None);
        // 空渠道（无分时）→ 恒 1.0，永不延迟
        let empty = pricing::ZoneResolver::new();
        assert_eq!(valley_wait_secs(&empty, peak), None);
    }
}
