//! Pricing engine — PriceSpec (tiered × zone × per-component costs), the single
//! source of truth for cost calculation (PRICING-PLAN §四).
//!
//! Design decisions:
//! - **Zero new dependencies**: Beijing-time conversion is implemented by hand
//!   (fixed +8h offset — Asia/Shanghai has no DST) + a civil-date algorithm, so
//!   the crate builds offline. `chrono` may replace it later if needed.
//! - `actual_cost()` mirrors LiteLLM's `cost_calculator` component formula.
//! - `est_cost()` / `effective_input_cost()` feed router scoring (PR-5, later).
//! - All prices are **USD/token**.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

// ── Usage detail (透传自 Python litellm 响应) ──

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageDetail {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    #[serde(default)]
    pub cached_tokens: i64,          // prompt_tokens_details.cached_tokens
    #[serde(default)]
    pub reasoning_tokens: i64,       // completion_tokens_details.reasoning_tokens
    #[serde(default)]
    pub cache_creation_tokens: i64,  // cache_creation_input_tokens
    #[serde(default)]
    pub field_missing: bool,         // usage 缺 cached_tokens 字段（校准记账，不告警）
}

// ── PriceSpec ──

/// 单档阶梯价（max_input：输入长度 ≤ 该值适用本档；末档必须为 i64::MAX）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TierBand {
    pub max_input: i64,
    #[serde(default)]
    pub input_cost: f64,
    #[serde(default)]
    pub output_cost: f64,
    #[serde(default)]
    pub cache_read_cost: Option<f64>,   // None = 该档无缓存计价区分（命中按原价）
    #[serde(default)]
    pub cache_write_cost: Option<f64>,
    #[serde(default)]
    pub reasoning_cost: Option<f64>,
}

/// 时段规则一条（数组先具体后兜底，首条命中生效）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZoneRule {
    #[serde(default)]
    pub days: Option<Vec<String>>,   // ["mon",...]；None = 不限工作日（holidays 规则用）
    #[serde(default = "default_star")]
    pub hours: String,               // "*" 或 "9-12,14-18"
    #[serde(default = "default_one")]
    pub multiplier: f64,
    #[serde(default)]
    pub holidays: bool,              // true = 仅节假日命中
}

fn default_star() -> String {
    "*".to_string()
}
fn default_one() -> f64 {
    1.0
}

/// 渠道级时段规则（provider_zones 表行）
#[derive(Debug, Clone, Default)]
pub struct Zone {
    pub provider: String,
    pub tz_offset_hours: i32,        // 北京时间 +8（Asia/Shanghai 无夏令时）
    pub rules: Vec<ZoneRule>,
    pub holidays: HashSet<String>,   // "YYYY-MM-DD"
}

/// 主表 price_specs 行（DB 反序列化用）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PriceSpec {
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub input_cost: f64,
    #[serde(default)]
    pub output_cost: f64,
    #[serde(default)]
    pub cache_read_cost: Option<f64>,
    #[serde(default)]
    pub cache_write_cost: Option<f64>,
    #[serde(default)]
    pub reasoning_cost: Option<f64>,
    #[serde(default)]
    pub tiered: Option<Vec<TierBand>>,
    #[serde(default)]
    pub zone_ref: Option<String>,
    #[serde(default = "default_half")]
    pub batch_multiplier: f64,
    #[serde(default)]
    pub price_source: String,
    #[serde(default)]
    pub price_stale: bool,
    #[serde(default)]
    pub effective_from: Option<String>,
}

fn default_half() -> f64 {
    0.5
}

// ── 远端价格刷新（P2.a，纯解析无副作用）──

/// 一次远端刷新取回的单条价（litellm model_prices 文件，主键 (provider, model)）。
#[derive(Debug, Clone, Default)]
pub struct RemotePrice {
    pub input_cost: f64,
    pub output_cost: f64,
    pub cache_read_cost: Option<f64>,
}

/// 解析 litellm 官方 `model_prices_and_context_window.json` 文本 → 远端价表。
/// 仅收录形如 `provider/model` 的 key（裸 model 名无法归真源 provider，跳过）；
/// 单价缺或不正（<=0）跳过。cache_read 缺则 None（刷新用 COALESCE 不破坏原值）。
/// 纯函数，离线单测。
pub fn parse_remote_prices(raw: &str) -> HashMap<(String, String), RemotePrice> {
    let mut out = HashMap::new();
    let Ok(root) = serde_json::from_str::<serde_json::Value>(raw) else {
        return out;
    };
    let Some(obj) = root.as_object() else {
        return out;
    };
    for (key, v) in obj {
        let Some(io) = v.as_object() else {
            continue;
        };
        let Some((prov, model)) = key.split_once('/') else {
            continue;
        };
        if prov.is_empty() || model.is_empty() {
            continue;
        }
        let n = |k: &str| io.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
        let input = n("input_cost_per_token");
        let output = n("output_cost_per_token");
        if input <= 0.0 || output <= 0.0 {
            continue;
        }
        let cache_read = io
            .get("cache_read_input_token_cost")
            .and_then(|x| x.as_f64())
            .filter(|c| *c >= 0.0);
        out.insert(
            (prov.to_string(), model.to_string()),
            RemotePrice {
                input_cost: input,
                output_cost: output,
                cache_read_cost: cache_read,
            },
        );
    }
    out
}

/// 生效档的抽象：有阶梯走 Tier，无阶梯回落 Flat（避免借用 self 的生存期问题）
enum BandRef<'a> {
    Tier(&'a TierBand),
    Flat(&'a PriceSpec),
}

impl<'a> BandRef<'a> {
    fn in_cost(&self) -> f64 {
        match self {
            BandRef::Tier(b) => b.input_cost,
            BandRef::Flat(s) => s.input_cost,
        }
    }
    fn out_cost(&self) -> f64 {
        match self {
            BandRef::Tier(b) => b.output_cost,
            BandRef::Flat(s) => s.output_cost,
        }
    }
    fn cache_read(&self) -> f64 {
        match self {
            BandRef::Tier(b) => b.cache_read_cost.unwrap_or(b.input_cost), // 无区分→命中按原价
            BandRef::Flat(s) => s.cache_read_cost.unwrap_or(s.input_cost),
        }
    }
    fn cache_write(&self) -> f64 {
        match self {
            BandRef::Tier(b) => b.cache_write_cost.unwrap_or(0.0),
            BandRef::Flat(s) => s.cache_write_cost.unwrap_or(0.0),
        }
    }
    fn reasoning(&self) -> f64 {
        match self {
            BandRef::Tier(b) => b.reasoning_cost.unwrap_or(b.output_cost), // 无区分→按普通输出价
            BandRef::Flat(s) => s.reasoning_cost.unwrap_or(s.output_cost),
        }
    }
}

// ── 北京时间换算（纯标准库） ──

/// 公历 → 1970-01-01 起天数（Howard Hinnant days_from_civil，已验证）
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// 天数 → 公历（反向，Sakamoto 星期算法用不到；此函数供单测构造时刻）
#[allow(dead_code)]
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

/// Sakamoto 星期算法：0=Sunday .. 6=Saturday
fn weekday(y: i64, m: u32, d: u32) -> u32 {
    const T: [i64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    ((y + y / 4 - y / 100 + y / 400 + T[m as usize - 1] + d as i64) % 7) as u32
}

/// 北京时刻的组成部分（epoch 秒 → 北京年/月/日/星期/小时）
pub fn beijing_parts(epoch_secs: i64, tz_offset_hours: i32) -> (i64, u32, u32, u32, u32) {
    let shifted = epoch_secs + tz_offset_hours as i64 * 3600;
    let days = shifted.div_euclid(86_400);
    let secs_of_day = shifted.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let hour = (secs_of_day / 3600) as u32;
    (y, m, d, weekday(y, m, d), hour)
}

/// 构造指定北京时刻的 epoch 秒（单测用）
pub fn beijing_epoch(y: i64, mo: u32, d: u32, h: u32, min: u32, tz_offset_hours: i32) -> i64 {
    let days = days_from_civil(y, mo as i64, d as i64);
    days * 86_400 + (h as i64 * 3600 + min as i64 * 60) - tz_offset_hours as i64 * 3600
}

fn hours_match(spec: &str, hh: u32) -> bool {
    if spec == "*" {
        return true;
    }
    for part in spec.split(',') {
        let mut iter = part.split('-');
        let lo: u32 = iter
            .next()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let hi: u32 = iter
            .next()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(lo);
        // 区间为 [lo, hi)：高峰截止时刻整点即进入谷（如 12:00、18:00 已是谷时）。
        if hh >= lo && hh < hi {
            return true;
        }
    }
    false
}

// ── ZoneResolver（provider_zones 内存缓存） ──

#[derive(Debug, Default)]
pub struct ZoneResolver {
    inner: RwLock<HashMap<String, Zone>>,
}

impl ZoneResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load(&self, zones: Vec<Zone>) {
        let mut m = self.inner.write().unwrap();
        for z in zones {
            m.insert(z.provider.clone(), z);
        }
    }

    pub fn get(&self, provider: &str) -> Option<Zone> {
        self.inner.read().unwrap().get(provider).cloned()
    }

    pub fn len(&self) -> usize {
        self.inner.read().unwrap().len()
    }

    /// 已加载分时渠道快照（PR-8 峰谷调度扫描用）。
    pub fn zones(&self) -> Vec<Zone> {
        self.inner.read().unwrap().values().cloned().collect()
    }
}

// ── PriceSpec 计算 ──

impl PriceSpec {
    /// 阶梯档选择：按当次请求总输入长度（含缓存命中部分——供应商如此计价）
    fn band(&self, prompt_tokens: i64) -> BandRef<'_> {
        if let Some(tiers) = &self.tiered {
            for b in tiers {
                if prompt_tokens <= b.max_input {
                    return BandRef::Tier(b);
                }
            }
            if let Some(last) = tiers.last() {
                return BandRef::Tier(last);
            }
        }
        BandRef::Flat(self)
    }

    /// 时段系数。规则缺失/未命中 → 1.0（不优惠、不报错，校准层会暴露）
    pub fn zone_multiplier(&self, t_epoch_secs: i64, zr: &ZoneResolver) -> f64 {
        let Some(zref) = &self.zone_ref else { return 1.0; };
        let Some(zone) = zr.get(zref) else { return 1.0; };
        zone.multiplier_at(t_epoch_secs)
    }

    /// 事后精确账单（对齐 LiteLLM cost_calculator 分项公式）
    pub fn actual_cost(&self, u: &UsageDetail, t_epoch_secs: i64, zr: &ZoneResolver) -> f64 {
        let band = self.band(u.prompt_tokens);
        let z = self.zone_multiplier(t_epoch_secs, zr);
        // 容错：cached_tokens 不应超过 prompt_tokens（供应商偶发数据异常）
        let cached = u.cached_tokens.min(u.prompt_tokens).max(0);
        let non_cached = (u.prompt_tokens - cached).max(0);
        z * (non_cached as f64 * band.in_cost()
            + cached as f64 * band.cache_read()
            + u.cache_creation_tokens as f64 * band.cache_write())
            + z * u.completion_tokens as f64 * band.out_cost()
            + z * u.reasoning_tokens as f64 * band.reasoning()
    }

    /// 有效输入单价（路由评分用）：命中率期望加权
    pub fn effective_input_cost(&self, hit_rate_ewma: f64, t_epoch_secs: i64, zr: &ZoneResolver) -> f64 {
        let z = self.zone_multiplier(t_epoch_secs, zr);
        let p_read = self.cache_read_cost.unwrap_or(self.input_cost); // 无区分=不优惠
        let h = hit_rate_ewma.clamp(0.0, 1.0);
        z * (h * p_read + (1.0 - h) * self.input_cost)
    }

    /// 事前估算成本（喂 plan() 门槛；输出按顶层价，不含 cache_write——保守）。
    /// 注意：`effective_input_cost` 已含时段系数 z，输入侧不得再乘 z（避免 z²）。
    pub fn est_cost(
        &self,
        hit_rate_ewma: f64,
        est_in: i64,
        est_out: i64,
        t_epoch_secs: i64,
        zr: &ZoneResolver,
    ) -> f64 {
        let z = self.zone_multiplier(t_epoch_secs, zr);
        let eff_in = self.effective_input_cost(hit_rate_ewma, t_epoch_secs, zr);
        est_in as f64 * eff_in + z * est_out as f64 * self.output_cost
    }
}

fn holiday_key(t_epoch_secs: i64, tz_offset_hours: i32) -> String {
    let (y, m, d, _, _) = beijing_parts(t_epoch_secs, tz_offset_hours);
    format!("{y:04}-{m:02}-{d:02}")
}

// ── Zone JSON 解析（provider_zones.rule_json / holidays_json） ──

impl Zone {
    /// 从 DB 行构造。rule_json / holidays_json 解析失败 → 空规则（不报错，等价不分时）。
    pub fn from_db(provider: &str, rule_json: &str, tz: &str, holidays_json: &str) -> Zone {
        let tz_offset_hours = if tz == "Asia/Shanghai" || tz.is_empty() {
            8
        } else {
            8 // 本项目仅支持北京时间；其余时区留待未来
        };
        let rules = serde_json::from_str(rule_json).unwrap_or_default();
        let holidays: HashSet<String> =
            serde_json::from_str(holidays_json).unwrap_or_default();
        Zone {
            provider: provider.to_string(),
            tz_offset_hours,
            rules,
            holidays,
        }
    }

    /// 渠道在指定时刻的分时系数（PR-8 抽出，供调度扫描复用）。
    /// 规则缺失/未命中 → 1.0。
    pub fn multiplier_at(&self, t_epoch_secs: i64) -> f64 {
        let (_, _, _, dow, hh) = beijing_parts(t_epoch_secs, self.tz_offset_hours);
        let dow_name = match dow {
            1 => "mon",
            2 => "tue",
            3 => "wed",
            4 => "thu",
            5 => "fri",
            6 => "sat",
            _ => "sun",
        }
        .to_string();
        let is_holiday = self.holidays.contains(&holiday_key(t_epoch_secs, self.tz_offset_hours));
        for rule in &self.rules {
            let day_ok = rule
                .days
                .as_ref()
                .map(|d| d.contains(&dow_name))
                .unwrap_or(true);
            let hit = if rule.holidays {
                is_holiday
            } else {
                day_ok
            };
            if !hit {
                continue;
            }
            if hours_match(&rule.hours, hh) {
                return rule.multiplier;
            }
        }
        1.0
    }

    /// PR-8：在 `horizon_secs` 内（严格未来）首个折扣系数 (<1.0) 的起始时刻。
    /// 30 分钟步进扫描；当前已在谷时（multiplier<1）不返回 now，仅返回未来的谷时窗口。
    /// 无谷时窗口 → None。
    pub fn first_valley_epoch(&self, t_epoch_secs: i64, horizon_secs: i64) -> Option<i64> {
        let step: i64 = 1800; // 30-min 粒度，能对齐半小时/整点边界
        let end = t_epoch_secs.saturating_add(horizon_secs.max(step));
        let mut t = t_epoch_secs.saturating_add(step);
        while t <= end {
            if self.multiplier_at(t) < 1.0 {
                return Some(t);
            }
            t = t.saturating_add(step);
        }
        None
    }
}

// ── 单测 ──

#[cfg(test)]
mod tests {
    use super::*;

    fn test_spec() -> PriceSpec {
        PriceSpec {
            provider: "dashscope".into(),
            model: "qwen3-max".into(),
            input_cost: 3.47e-7,
            output_cost: 1.389e-6,
            cache_read_cost: Some(6.94e-8), // 0.2×
            cache_write_cost: Some(0.0),
            reasoning_cost: None,
            tiered: Some(vec![
                TierBand {
                    max_input: 32768,
                    input_cost: 3.47e-7,
                    output_cost: 1.389e-6,
                    cache_read_cost: Some(6.94e-8),
                    cache_write_cost: Some(0.0),
                    reasoning_cost: None,
                },
                TierBand {
                    max_input: 131072,
                    input_cost: 5.56e-7,
                    output_cost: 2.222e-6,
                    cache_read_cost: Some(1.11e-7),
                    cache_write_cost: Some(0.0),
                    reasoning_cost: None,
                },
            ]),
            zone_ref: None,
            batch_multiplier: 0.5,
            price_source: "overlay".into(),
            price_stale: false,
            effective_from: None,
        }
    }

    fn deepseek_spec() -> PriceSpec {
        let mut s = test_spec();
        s.zone_ref = Some("deepseek".into());
        s
    }

    fn deepseek_zone() -> Zone {
        Zone::from_db(
            "deepseek",
            // 先具体后兜底；holidays 必须最优先（否则工作日高峰规则先命中）
            r#"[
              {"holidays":true,"hours":"*","multiplier":0.5},
              {"days":["sat","sun"],"hours":"*","multiplier":0.5},
              {"days":["mon","tue","wed","thu","fri"],"hours":"9-12,14-18","multiplier":1.0},
              {"days":["mon","tue","wed","thu","fri"],"hours":"*","multiplier":0.5}
            ]"#,
            "Asia/Shanghai",
            r#"["2026-10-01","2026-10-02"]"#,
        )
    }

    #[test]
    fn no_cache_differentiation_falls_back_to_input() {
        let s = PriceSpec {
            input_cost: 1e-6,
            cache_read_cost: None,
            ..Default::default()
        };
        assert_eq!(s.band(100).cache_read(), 1e-6);
    }

    #[test]
    fn implicit_cache_hit_billed_at_20pct() {
        let s = test_spec();
        let u = UsageDetail {
            prompt_tokens: 10_000,
            completion_tokens: 100,
            cached_tokens: 5_000,
            ..Default::default()
        };
        let cost = s.actual_cost(&u, 0, &ZoneResolver::new());
        let expected = 5_000.0 * 3.47e-7 + 5_000.0 * 6.94e-8 + 100.0 * 1.389e-6;
        assert!((cost - expected).abs() < 1e-12, "cost={cost} expected={expected}");
    }

    #[test]
    fn cached_gt_prompt_is_clamped() {
        let s = test_spec();
        let u = UsageDetail {
            prompt_tokens: 10_000,
            completion_tokens: 0,
            cached_tokens: 15_000,
            ..Default::default()
        };
        let cost = s.actual_cost(&u, 0, &ZoneResolver::new());
        let expected = 10_000.0 * 6.94e-8; // 全部按命中价，无未命中部分
        assert!((cost - expected).abs() < 1e-12);
    }

    #[test]
    fn tier_band_switch() {
        let s = test_spec();
        let u = UsageDetail {
            prompt_tokens: 40_000,
            completion_tokens: 0,
            cached_tokens: 0,
            ..Default::default()
        };
        let cost = s.actual_cost(&u, 0, &ZoneResolver::new());
        assert!((cost - 40_000.0 * 5.56e-7).abs() < 1e-12);
    }

    #[test]
    fn tier_band_boundary() {
        let s = test_spec();
        let zr = ZoneResolver::new();
        let u1 = UsageDetail { prompt_tokens: 32768, completion_tokens: 0, ..Default::default() };
        let u2 = UsageDetail { prompt_tokens: 32769, completion_tokens: 0, ..Default::default() };
        let c1 = s.actual_cost(&u1, 0, &zr);
        let c2 = s.actual_cost(&u2, 0, &zr);
        assert!((c1 - 32768.0 * 3.47e-7).abs() < 1e-12);
        assert!((c2 - 32769.0 * 5.56e-7).abs() < 1e-12);
    }

    #[test]
    fn workday_peak_multiplier_1() {
        let zr = ZoneResolver::new();
        zr.load(vec![deepseek_zone()]);
        let s = deepseek_spec();
        // 2026-08-24 是周一；北京 10:00
        let t = beijing_epoch(2026, 8, 24, 10, 0, 8);
        assert_eq!(s.zone_multiplier(t, &zr), 1.0);
    }

    #[test]
    fn workday_off_peak_half() {
        let zr = ZoneResolver::new();
        zr.load(vec![deepseek_zone()]);
        let s = deepseek_spec();
        let t = beijing_epoch(2026, 8, 24, 23, 0, 8);
        assert_eq!(s.zone_multiplier(t, &zr), 0.5);
    }

    #[test]
    fn weekend_always_half() {
        let zr = ZoneResolver::new();
        zr.load(vec![deepseek_zone()]);
        let s = deepseek_spec();
        // 2026-08-29 是周六，北京 10:00（高峰时段但周末→谷价）
        let t = beijing_epoch(2026, 8, 29, 10, 0, 8);
        assert_eq!(s.zone_multiplier(t, &zr), 0.5);
    }

    #[test]
    fn holiday_half_even_on_workday() {
        let zr = ZoneResolver::new();
        zr.load(vec![deepseek_zone()]);
        let s = deepseek_spec();
        // 2026-10-01 是周四（工作日）但节假日 → 0.5
        let t = beijing_epoch(2026, 10, 1, 10, 0, 8);
        assert_eq!(s.zone_multiplier(t, &zr), 0.5);
    }

    #[test]
    fn missing_zone_falls_back_to_1() {
        let zr = ZoneResolver::new(); // 未加载任何 zone
        let s = deepseek_spec();
        let t = beijing_epoch(2026, 8, 24, 10, 0, 8);
        assert_eq!(s.zone_multiplier(t, &zr), 1.0);
    }

    #[test]
    fn no_zone_ref_always_1() {
        let zr = ZoneResolver::new();
        let s = test_spec(); // zone_ref = None
        let t = beijing_epoch(2026, 8, 24, 10, 0, 8);
        assert_eq!(s.zone_multiplier(t, &zr), 1.0);
    }

    #[test]
    fn est_cost_hit_rate_weighted() {
        let s = test_spec();
        let zr = ZoneResolver::new();
        // h=0.6, cache_read=0.2×input → eff_in = 0.6*0.2*in + 0.4*in = 0.52*in
        let eff = s.effective_input_cost(0.6, 0, &zr);
        assert!((eff - 0.52 * 3.47e-7).abs() < 1e-15);
        let ec = s.est_cost(0.6, 1000, 200, 0, &zr);
        let expected = 1000.0 * 0.52 * 3.47e-7 + 200.0 * 1.389e-6;
        assert!((ec - expected).abs() < 1e-12);
    }

    #[test]
    fn est_cost_zone_halved() {
        let zr = ZoneResolver::new();
        zr.load(vec![deepseek_zone()]);
        let s = deepseek_spec();
        let t_peak = beijing_epoch(2026, 8, 24, 10, 0, 8);
        let t_valley = beijing_epoch(2026, 8, 24, 23, 0, 8);
        let ec_peak = s.est_cost(0.0, 1000, 100, t_peak, &zr);
        let ec_valley = s.est_cost(0.0, 1000, 100, t_valley, &zr);
        assert!((ec_valley - ec_peak * 0.5).abs() < 1e-12);
    }

    // ── PR-8 峰谷调度：first_valley_epoch ──

    #[test]
    fn first_valley_epoch_finds_next_window_from_peak() {
        let z = deepseek_zone();
        // 周一 17:00 处于高峰段 14-18 → 下一谷时 18:00
        let t = beijing_epoch(2026, 8, 24, 17, 0, 8);
        assert_eq!(z.first_valley_epoch(t, 7200), Some(beijing_epoch(2026, 8, 24, 18, 0, 8)));
    }

    #[test]
    fn first_valley_epoch_from_morning_peak_hits_noon() {
        let z = deepseek_zone();
        // 周一 10:00 高峰 9-12 → 下一谷时 12:00
        let t = beijing_epoch(2026, 8, 24, 10, 0, 8);
        assert_eq!(z.first_valley_epoch(t, 7200), Some(beijing_epoch(2026, 8, 24, 12, 0, 8)));
    }

    #[test]
    fn first_valley_epoch_none_when_horizon_too_small() {
        let z = deepseek_zone();
        // 17:00 高峰 14-18；horizon 仅 1800s → 只扫到 17:30（仍高峰）→ None
        let t = beijing_epoch(2026, 8, 24, 17, 0, 8);
        assert_eq!(z.first_valley_epoch(t, 1800), None);
    }

    #[test]
    fn hours_match_parser() {
        assert!(hours_match("*", 0));
        assert!(hours_match("9-12,14-18", 10));
        assert!(hours_match("9-12,14-18", 17));
        assert!(!hours_match("9-12,14-18", 13));
        assert!(!hours_match("9-12,14-18", 8));
        // [lo, hi)：截止时刻整点即入谷
        assert!(!hours_match("9-12,14-18", 12));
        assert!(!hours_match("9-12,14-18", 18));
    }

    #[test]
    fn beijing_weekday_is_monday() {
        // 2026-08-24 周一
        let t = beijing_epoch(2026, 8, 24, 12, 0, 8);
        let (_, _, _, dow, hh) = beijing_parts(t, 8);
        assert_eq!(dow, 1); // Sakamoto: 1 = Monday
        assert_eq!(hh, 12);
    }

    #[test]
    fn actual_cost_matches_litellm_formula() {
        // 与 LiteLLM 分项公式逐项对齐（无阶梯、无时段）
        let s = PriceSpec {
            input_cost: 1e-6,
            output_cost: 3e-6,
            cache_read_cost: Some(1e-7),
            cache_write_cost: Some(1.25e-6),
            reasoning_cost: Some(3e-6),
            ..Default::default()
        };
        let u = UsageDetail {
            prompt_tokens: 20_000,
            completion_tokens: 500,
            cached_tokens: 12_000,
            reasoning_tokens: 50,
            cache_creation_tokens: 8_000,
            ..Default::default()
        };
        let zr = ZoneResolver::new();
        let cost = s.actual_cost(&u, 0, &zr);
        let expected = 8_000.0 * 1e-6          // miss
            + 12_000.0 * 1e-7                  // cached
            + 8_000.0 * 1.25e-6                // cache creation (write)
            + 500.0 * 3e-6                     // output
            + 50.0 * 3e-6;                     // reasoning
        assert!((cost - expected).abs() < 1e-9, "cost={cost} expected={expected}");
    }

    // ── P2.a 远端价格解析 ──

    #[test]
    fn parse_remote_provider_model_keys() {
        let raw = r#"{
            "dashscope/qwen-max": {"input_cost_per_token": 0.0000026, "output_cost_per_token": 0.0000086, "cache_read_input_token_cost": 0.00000052},
            "openai/gpt-4o": {"input_cost_per_token": 0.0000025, "output_cost_per_token": 0.00001},
            "gpt-4o-mini": {"input_cost_per_token": 0.00000015}
        }"#;
        let m = parse_remote_prices(raw);
        assert_eq!(m.len(), 2, "裸 model 名应跳过：{m:?}");
        let qw = m.get(&("dashscope".into(), "qwen-max".into())).unwrap();
        assert!((qw.input_cost - 0.0000026).abs() < 1e-12);
        assert_eq!(qw.cache_read_cost, Some(0.00000052));
        let gpt = &m[&("openai".into(), "gpt-4o".into())];
        assert_eq!(gpt.cache_read_cost, None);
    }

    #[test]
    fn parse_remote_skips_bad_values() {
        let raw = r#"{
            "dashscope/qwen": {"input_cost_per_token": -1e-6, "output_cost_per_token": 0.0000026},
            "dashscope/qwen-ok": {"input_cost_per_token": 0.000001, "output_cost_per_token": 0.0},
            "dashscope/qwen-good": {"input_cost_per_token": 0.000001, "output_cost_per_token": 0.000002}
        }"#;
        let m = parse_remote_prices(raw);
        assert_eq!(m.len(), 1);
        assert!(m.contains_key(&("dashscope".into(), "qwen-good".into())));
    }

    #[test]
    fn parse_remote_invalid_json_empty() {
        assert!(parse_remote_prices("not json").is_empty());
        assert!(parse_remote_prices("[1,2]").is_empty());
    }
}
