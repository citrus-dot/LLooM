//! 模型元数据自动打标（ROUTING-PLAN P0.e）。
//!
//! `resolve_and_fill` 按「五级兜底」给新增 `Model` 回填路由元数据
//! （capability_tier / context_window / is_local / needs_calibration / 成本）：
//! 已由更高优先级来源（overlay / 用户显式给出）的字段不被 heuristic 覆盖，
//! 保证「命即填，后级不覆盖高级」。
//!
//! 五级来源（离线 box 只启用 4/5，前三级留接线点）：
//!   1. litellm 打包表（运行时 import litellm.model_cost）——需 Python 运行时，暂不启用
//!   2. litellm 远端刷新结果（P2 job 落库后读 price_source=='litellm_remote' 值）——P2 接线
//!   3. models.dev api.json（镜像拉取；实测无 dashscope）——需网络，离线跳过
//!   4. overlay：`data/model_catalog.json` 里的 `{provider}/{name}` 显式条目
//!   5. 启发式（名字关键词 + 本地端点），最后兜底

use crate::config;
use crate::models::Model;
use serde_json::Value;
use std::path::Path;

/// 目测 <某个数字>+"b"（如 7b / 4b / 1.5b / 0.5b / 72b）→ 开放权重的轻量小模型。
fn looks_small_parameter(n: &str) -> bool {
    let b = n.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let mut j = i;
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.') {
                j += 1;
            }
            if j < b.len() && (b[j] == b'b' || b[j] == b'B') {
                return true;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    false
}

/// name → capability_tier 的启发式映射。
/// 轻量系（flash/mini/turbo/small/lite/air/<参数规模>）→ 1，
/// 旗舰/推理系（max/opus/ultra/pro/reasoner/thinking/r1/o1/o3）→ 3，其余 2。
pub fn tier_for_name(name: &str) -> i64 {
    let n = name.to_lowercase();
    const LIGHT: &[&str] = &["flash", "mini", "turbo", "small", "lite", "air"];
    const FLAGSHIP: &[&str] = &[
        "max", "opus", "ultra", "pro", "reasoner", "thinking", "r1", "o1", "o3",
    ];
    if looks_small_parameter(&n) {
        return 1;
    }
    for kw in LIGHT {
        if n.contains(kw) {
            return 1;
        }
    }
    for kw in FLAGSHIP {
        if n.contains(kw) {
            return 3;
        }
    }
    2
}

/// 本地端点检测（硬事实，无条件应用）。
pub fn is_local_endpoint(m: &Model) -> bool {
    m.provider.eq_ignore_ascii_case("ollama")
        || m.api_base.contains("127.0.0.1")
        || m.api_base.to_lowercase().contains("localhost")
}

/// overlay 命中了哪些字段，供 heuristic 判断「不覆盖高级来源」。
#[derive(Default)]
struct OverlayHits {
    tier: bool,
    ctx: bool,
    cost: bool,
}

fn overlay_key(m: &Model) -> String {
    format!("{}/{}", m.provider, m.name)
}

/// 第 4 级：读 `model_catalog.json` 的 `{provider}/{name}` 条目回填。
/// 生产路径用 `config::data_dir()`；测试可注入任意目录以避开全局 env 竞态。
fn fill_from_overlay(m: &mut Model) -> OverlayHits {
    fill_from_overlay_in(m, &config::data_dir())
}

/// 第 4 级：读 `model_catalog.json` 的 `{provider}/{name}` 条目回填。
/// 文件缺失/无此条目/无法解析 → 无贡献。
fn fill_from_overlay_in(m: &mut Model, data_dir: &Path) -> OverlayHits {
    let mut hits = OverlayHits::default();
    let path = data_dir.join("model_catalog.json");
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return hits,
    };
    let root: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return hits,
    };
    let Some(entry) = root.get(&overlay_key(m)) else {
        return hits;
    };
    if let Some(v) = entry.get("capability_tier").and_then(|x| x.as_i64()) {
        m.capability_tier = v;
        hits.tier = true;
    }
    if let Some(v) = entry.get("context_window").and_then(|x| x.as_i64()) {
        m.context_window = v;
        hits.ctx = true;
    }
    if let Some(v) = entry.get("quality_score").and_then(|x| x.as_f64()) {
        m.quality_score = v.clamp(0.0, 1.0);
    }
    if let Some(v) = entry.get("supports_stream").and_then(|x| x.as_i64()) {
        m.supports_stream = v;
    }
    if let Some(v) = entry.get("is_local").and_then(|x| x.as_i64()) {
        m.is_local = v;
    }
    let in_c = entry.get("input_cost_per_token").and_then(|x| x.as_f64());
    let out_c = entry.get("output_cost_per_token").and_then(|x| x.as_f64());
    match (in_c, out_c) {
        (Some(i), Some(o)) => {
            m.input_cost_per_token = i;
            m.output_cost_per_token = o;
            hits.cost = true;
        }
        (Some(i), None) if i >= 0.0 => m.input_cost_per_token = i,
        (None, Some(o)) if o >= 0.0 => m.output_cost_per_token = o,
        _ => {}
    }
    hits
}

/// 第 5 级启发式兜底：只填未被 overlay/用户显式给出的字段。
fn fill_heuristic(m: &mut Model, tier_filled: bool, ctx_filled: bool, cost_filled: bool) {
    // 仅当能力档仍是默认值 2（用户未显式设 1/3）时才启发式定档——最低优先级，
    // 不得覆盖用户显式给出的档位。
    if !tier_filled && m.capability_tier == 2 {
        m.capability_tier = tier_for_name(&m.name);
    }
    if !ctx_filled && m.context_window <= 0 {
        m.context_window = 32768;
    }
    if is_local_endpoint(m) {
        m.is_local = 1;
        // 本地模型无外部计费：未显式给价 → 置 0（避免误计入账）
        if !cost_filled {
            m.input_cost_per_token = 0.0;
            m.output_cost_per_token = 0.0;
        }
    }
    // 新 / 仅启发式的模型一律进入保守期，等 run 结果回填 ewma_quality 后解除
    m.needs_calibration = 1;
}

/// P0.e 入口：给新增模型回填路由元数据（就地 mutate）。
/// 调用点：`db::insert_model`。
pub fn resolve_and_fill(m: &mut Model) -> FillReport {
    let hits = fill_from_overlay(m);
    fill_heuristic(m, hits.tier, hits.ctx, hits.cost);
    FillReport {
        source: if hits.tier || hits.ctx || hits.cost {
            "overlay+heuristic"
        } else {
            "heuristic"
        },
        backfilled: hits.tier || hits.ctx || hits.cost,
    }
}

/// 本次打标来源报告（供日志/审计）。
#[derive(Debug, Clone)]
pub struct FillReport {
    pub source: &'static str,
    pub backfilled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(name: &str, provider: &str, api_base: &str) -> Model {
        Model {
            id: 0,
            name: name.to_string(),
            provider: provider.to_string(),
            litellm_model: name.to_string(),
            api_base: api_base.to_string(),
            api_key_env: String::new(),
            task_type: String::new(),
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
            rpm: 60,
            is_active: 1,
            capability_tier: 2,
            quality_score: 0.6,
            context_window: 32768,
            supports_tools: 0,
            supports_vision: 0,
            supports_stream: 0,
            is_local: 0,
            priority: 0,
            health_state: "unknown".to_string(),
            needs_calibration: 0,
        }
    }

    #[test]
    fn tier_flash_is_light() {
        assert_eq!(tier_for_name("qwen3.6-flash"), 1);
        assert_eq!(tier_for_name("gpt-4o-mini"), 1);
        assert_eq!(tier_for_name("qwen2.5-7b"), 1);
    }

    #[test]
    fn tier_max_is_flagship() {
        assert_eq!(tier_for_name("qwen3-max"), 3);
        assert_eq!(tier_for_name("deepseek-r1"), 3);
        assert_eq!(tier_for_name("opengpt-4o-pro"), 3);
    }

    #[test]
    fn tier_default_is_mid() {
        assert_eq!(tier_for_name("qwen-plus"), 2);
        assert_eq!(tier_for_name("gpt-4o"), 2);
        assert_eq!(tier_for_name("deepseek-v3"), 2);
    }

    #[test]
    fn local_detection_sets_flag_and_zero_cost() {
        let mut m = model("qwen2.5-local", "ollama", "http://host.docker.internal:11434");
        resolve_and_fill(&mut m);
        assert_eq!(m.is_local, 1, "ollama 应被标本地");
        assert_eq!(m.input_cost_per_token, 0.0);
        assert_eq!(m.output_cost_per_token, 0.0);
        assert_eq!(m.needs_calibration, 1, "启发式模型须进入保守期");
    }

    #[test]
    fn localhost_api_base_is_local() {
        let m = model("my-model", "openai", "http://localhost:11434/v1");
        assert!(is_local_endpoint(&m));
    }

    #[test]
    fn resolve_fills_tier_and_calibration() {
        let mut m = model("new-flash-x", "dashscope", "");
        resolve_and_fill(&mut m);
        assert_eq!(m.capability_tier, 1);
        assert_eq!(m.needs_calibration, 1);
        assert_eq!(m.is_local, 0, "云端默认非本地");
    }

    #[test]
    fn overlay_entry_wins_over_heuristic() {
        // 用可注入目录而非全局 env，避免和 db 迁移测试（共享 LLOOM_DATA_DIR）竞态
        let dir = std::env::temp_dir().join(format!("lloom_metadata_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("model_catalog.json"),
            r#"{"dashscope/qwen-flash-x": {"capability_tier": 2, "input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6}}"#,
        )
        .unwrap();
        let mut m = model("qwen-flash-x", "dashscope", "");
        let hits = fill_from_overlay_in(&mut m, &dir);
        assert_eq!(m.capability_tier, 2, "overlay 显式档应覆盖启发式(flash→1)");
        assert_eq!(m.input_cost_per_token, 1e-6);
        assert_eq!(m.output_cost_per_token, 2e-6);
        assert!(hits.tier && hits.cost);
    }
}