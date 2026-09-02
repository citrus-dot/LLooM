//! N2 闭环评估（NEXT-PLAN §三）：路由体检报告 + 权重网格搜索建议。
//!
//! - 报告闭环（N2.a）：周期 job 调 `scripts/aiq_replay.py --json`（三线数字与 CLI
//!   文本同源），连同预算档分布、权重建议一并写 `policy_review` 表；
//!   `GET /api/routing/review` 读最新一份。
//! - 权重建议（N2.b）：**Rust 侧**用 `plan()` 对影子样本任务做无副作用网格重放
//!   （cost × quality 权重网格），找支配当前策略的帕累托点输出建议——不在
//!   Python 复刻评分逻辑（单一真源原则，ROUTING-PLAN P0.f 教训）。
//!   `POST /api/routing/review/adopt` 人工采纳后才生效（不自动改策略）。

use crate::config;
use crate::db;
use crate::error::{AppError, Result};
use crate::models::{Model, RoutingPolicy};
use crate::pricing::{PriceSpec, ZoneResolver};
use crate::router::{self, PlanInput};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// 周期：6 小时一份体检报告（幂等追加；`latest_policy_review` 读最新）。
const REVIEW_INTERVAL_SECS: u64 = 6 * 3600;
/// 网格：cost/quality 权重各 0.2..=0.8（0.1 步进，7×7=49 次内存级重放/任务）。
const GRID: [f64; 7] = [0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
/// 建议打分中的成本相对增幅惩罚系数（质量增益 − λ×成本增幅）。
const COST_PENALTY: f64 = 0.5;
/// 建议物性门槛：得分低于此值视为噪声，不出建议。
const MATERIALITY: f64 = 0.05;

/// 挂载到 `spawn_background_jobs()`：首个 tick 立即触发（启动即出一份报告），
/// 之后每 6h 追加。失败只打日志，下个周期自愈。
pub async fn aiq_report_loop() {
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(REVIEW_INTERVAL_SECS));
    loop {
        ticker.tick().await;
        if let Err(e) = run_review().await {
            eprintln!("[review] aiq report job failed: {e}");
        }
    }
}

/// 跑一轮体检：Python 重放（三线数字）+ Rust 网格建议 + 预算档分布 → 写 policy_review。
/// 无影子样本时不写库，返回 ok:false（启动早期属正常态，不算错误）。
pub async fn run_review() -> Result<Value> {
    let Some(rep) = run_aiq_script().await? else {
        return Ok(json!({
            "ok": false,
            "error": "routing_calibration 无影子样本——先经 POST /api/routing/shadow 或自动采样积累",
        }));
    };
    let suggestions = grid_search_suggestions()?;
    let tiers = db::budget_tier_distribution(7)?;
    let tiers_json = serde_json::to_string(&tiers).unwrap_or_else(|_| "{}".to_string());
    let samples = rep["samples"].as_i64().unwrap_or(0);
    let id = db::insert_policy_review(
        samples,
        rep["weak"]["cost"].as_f64().unwrap_or(0.0),
        rep["weak"]["quality"].as_f64().unwrap_or(0.0),
        rep["current"]["cost"].as_f64().unwrap_or(0.0),
        rep["current"]["quality"].as_f64().unwrap_or(0.0),
        rep["strong"]["cost"].as_f64().unwrap_or(0.0),
        rep["strong"]["quality"].as_f64().unwrap_or(0.0),
        rep["aiq"].as_f64().unwrap_or(0.0),
        rep["saved_pct"].as_f64().unwrap_or(0.0),
        rep["conclusion"].as_str().unwrap_or(""),
        &tiers_json,
        &serde_json::to_string(&suggestions).unwrap_or_else(|_| "[]".to_string()),
    )?;
    Ok(json!({
        "ok": true,
        "id": id,
        "samples": samples,
        "aiq": rep["aiq"],
        "saved_pct": rep["saved_pct"],
        "conclusion": rep["conclusion"],
        "suggestions": suggestions,
    }))
}

/// 调 `scripts/aiq_replay.py --json`（stdlib-only，本地 sqlite，秒级）。
/// 返回 Ok(None) = 无样本/脚本非致命失败（stderr 已打日志）。
async fn run_aiq_script() -> Result<Option<Value>> {
    let script = resolve_script()?;
    let db_arg = config::db_path().to_string_lossy().to_string();
    let out = tokio::task::spawn_blocking(move || {
        std::process::Command::new("python3")
            .arg(&script)
            .arg("--json")
            .arg("--db")
            .arg(&db_arg)
            .output()
    })
    .await
    .map_err(|e| AppError::Internal(format!("join aiq_replay: {e}")))?
    .map_err(|e| AppError::Process(format!("spawn python3 aiq_replay: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        eprintln!("[review] aiq_replay exited {:?}: {}", out.status.code(), stderr.trim());
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let rep: Value = serde_json::from_str(stdout.trim())
        .map_err(|e| AppError::Internal(format!("aiq_replay --json 解析失败: {e}")))?;
    if rep["samples"].as_i64().unwrap_or(0) == 0 {
        return Ok(None);
    }
    Ok(Some(rep))
}

fn resolve_script() -> Result<std::path::PathBuf> {
    let primary = config::install_dir().join("scripts/aiq_replay.py");
    if primary.exists() {
        return Ok(primary);
    }
    let fallback = std::path::PathBuf::from("scripts/aiq_replay.py");
    if fallback.exists() {
        return Ok(fallback);
    }
    Err(AppError::Internal(
        "找不到 scripts/aiq_replay.py（install_dir 与 cwd 均无）".to_string(),
    ))
}

// ── N2.b：权重网格搜索（Rust 单一真源重放） ──

/// 一次 plan() 无副作用重放：返回 (选模, est_cost, quality)。
/// PlanInput 组装口径与 `router::route()` 对齐（est_in 未知 query 文本取 0，
/// 仅影响门槛/成本线性缩放，不影响选模排序）。
fn replay_once(
    task_type: &str,
    policy: &RoutingPolicy,
    models: &[Model],
    spec_map: &HashMap<(String, String), PriceSpec>,
    zr: &ZoneResolver,
) -> Option<(String, f64, f64)> {
    let mut quality_override = HashMap::new();
    for m in models {
        if let Some(sc) = db::get_model_task_score(&m.name, task_type).ok().flatten() {
            if sc.sample_count >= 5 {
                quality_override.insert(m.name.clone(), sc.ewma_quality.clamp(0.0, 1.0));
            }
        }
    }
    let hit_rate = db::model_cache_hit_rate(task_type);
    let est_out = db::task_avg_out_tokens(task_type).round() as i64;
    let band = router::band_for(task_type, "");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let input = PlanInput {
        task_type,
        band,
        policy,
        models,
        price_specs: spec_map,
        zones: zr,
        t_epoch_secs: now,
        est_in_tokens: 0,
        est_out_tokens: est_out,
        quality_override: &quality_override,
        budget_tier: "normal",
        hit_rate: &hit_rate,
        last_model_conv: None,
        deferrable: false,
    };
    let outcome = router::plan(&input).ok()?;
    let top = outcome.candidates.first()?;
    Some((outcome.primary.clone(), top.est_cost, top.quality))
}

/// 网格点双向权衡打分（相对当前策略）：
/// - 省成本方向（更便宜）：相对省额 − 质量损失；
/// - 提质量方向（更准）：质量增益 − 0.5×成本相对增幅；
/// - 两者皆非（同点/更贵且更差）→ −∞。
/// 与 aiq_replay.py 结论逻辑同向：AIQ 高省不足 → 省成本建议；质量掉损 → 提质量建议。
fn point_score(cost: f64, quality: f64, cur_cost: f64, cur_quality: f64) -> f64 {
    if cost < cur_cost - 1e-12 {
        let saving = (cur_cost - cost) / cur_cost.max(1e-12);
        saving - (cur_quality - quality).max(0.0)
    } else if quality > cur_quality + 1e-12 {
        let rel = ((cost - cur_cost) / cur_cost.max(1e-12)).max(0.0);
        (quality - cur_quality) - COST_PENALTY * rel
    } else {
        f64::NEG_INFINITY
    }
}

/// 网格搜索产出建议：对每个有影子样本的 task_type，用 cost×quality 权重网格
/// 重放选模，按 [`point_score`] 双向权衡取最优（得分 > 0.05 才值得建议）；
/// 权重取同选模网格点中距当前权重最近的一个（最小扰动）。无实质改进不出建议。
pub fn grid_search_suggestions() -> Result<Vec<Value>> {
    let cal = db::list_routing_calibration()?;
    if cal.is_empty() {
        return Ok(vec![]);
    }
    let models = db::list_models(true)?;
    if models.is_empty() {
        return Ok(vec![]);
    }
    let mut spec_map: HashMap<(String, String), PriceSpec> = HashMap::new();
    for spec in db::list_price_specs()? {
        spec_map.insert((spec.provider.clone(), spec.model.clone()), spec);
    }
    let zr = ZoneResolver::new();
    zr.load(db::list_provider_zones().unwrap_or_default());

    // task_type → 样本数（建议按流量权重排序展示）
    let mut sample_counts: HashMap<&str, i64> = HashMap::new();
    for r in &cal {
        *sample_counts.entry(r.task_type.as_str()).or_insert(0) += 1;
    }
    let mut task_counts: Vec<(&str, i64)> = sample_counts.into_iter().collect();
    task_counts.sort_by(|a, b| b.1.cmp(&a.1));

    let mut suggestions = Vec::new();
    for (task_type, samples) in task_counts {
        let policy = db::get_routing_policy(task_type)
            .ok()
            .flatten()
            .unwrap_or_default();
        let Some((cur_model, cur_cost, cur_quality)) =
            replay_once(task_type, &policy, &models, &spec_map, &zr)
        else {
            continue;
        };

        // 网格重放：收集每个 (cw, qw) 的 (选模, 成本, 质量)
        let mut points: Vec<(f64, f64, String, f64, f64)> = Vec::new();
        for &cw in GRID.iter() {
            for &qw in GRID.iter() {
                let mut p = policy.clone();
                p.cost_weight = cw;
                p.quality_weight = qw;
                if let Some((m, c, q)) = replay_once(task_type, &p, &models, &spec_map, &zr) {
                    points.push((cw, qw, m, c, q));
                }
            }
        }
        // 双向权衡打分取最优（省成本 / 提质量两个方向都考虑）
        let best = points
            .iter()
            .map(|p| (p, point_score(p.3, p.4, cur_cost, cur_quality)))
            .filter(|(_, s)| *s > MATERIALITY)
            .max_by(|a, b| {
                a.1.partial_cmp(&b.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        let Some((&(_, _, ref bmodel, bcost, bquality), _score)) = best else {
            continue;
        };
        // 最小扰动权重：同选模的网格点中距当前权重最近（曼哈顿距离）
        let (ccw, cqw) = (policy.cost_weight, policy.quality_weight);
        let (scw, sqw) = points
            .iter()
            .filter(|(_, _, m, _, _)| m == bmodel)
            .map(|(cw, qw, _, _, _)| ((cw - ccw).abs() + (qw - cqw).abs(), (*cw, *qw)))
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(_, w)| w)
            .unwrap_or((ccw, cqw));

        suggestions.push(json!({
            "task_type": task_type,
            "samples": samples,
            "current": {
                "model": cur_model,
                "est_cost": cur_cost,
                "quality": cur_quality,
                "cost_weight": ccw,
                "quality_weight": cqw,
            },
            "suggested": {
                "model": bmodel,
                "est_cost": bcost,
                "quality": bquality,
                "cost_weight": scw,
                "quality_weight": sqw,
                "latency_weight": policy.latency_weight, // 网格不动 latency（评分层尚未接入）
            },
        }));
    }
    Ok(suggestions)
}

/// 采纳建议（人工审查点，不自动生效）：从最新 policy_review 读建议，
/// 覆盖对应 task_type 策略的 cost/quality 权重后 upsert。
/// `task_type=None` 采纳全部；指定则只采纳该任务。
/// `get_routing_policy()` 在 route()/plan_for_task() 每请求读库——采纳后下一请求即生效。
pub fn adopt_suggestions(task_type: Option<&str>) -> Result<Value> {
    let latest = db::latest_policy_review()?
        .ok_or_else(|| AppError::NotFound("暂无体检报告（policy_review 为空）".to_string()))?;
    let suggestions: Vec<Value> =
        serde_json::from_str(&latest.suggestions_json).unwrap_or_default();
    let picked: Vec<&Value> = suggestions
        .iter()
        .filter(|s| task_type.map_or(true, |t| s["task_type"].as_str() == Some(t)))
        .collect();
    if picked.is_empty() {
        return Err(AppError::InvalidRequest(format!(
            "报告 #{} 中没有{}的权重建议",
            latest.id,
            task_type.unwrap_or("可采纳")
        )));
    }
    let mut adopted = Vec::new();
    for s in picked {
        let tt = s["task_type"].as_str().unwrap_or_default().to_string();
        let cw = s["suggested"]["cost_weight"].as_f64().unwrap_or(0.5);
        let qw = s["suggested"]["quality_weight"].as_f64().unwrap_or(0.4);
        let mut p = db::get_routing_policy(&tt)
            .ok()
            .flatten()
            .unwrap_or_default();
        p.task_type = tt.clone();
        p.cost_weight = cw;
        p.quality_weight = qw;
        db::upsert_routing_policy(&p)?;
        adopted.push(json!({ "task_type": tt, "cost_weight": cw, "quality_weight": qw }));
    }
    Ok(json!({ "ok": true, "adopted": adopted }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rusqlite::params;

    // 复用 db.rs 约定：跨模块共享 TEST_DB_LOCK 串行化（同一进程内所有写库测试
    // 都切全局 LLOOM_DATA_DIR，必须互斥），独立临时目录 + 用毕还原 env。
    fn setup(tag: &str) -> (std::path::PathBuf, Option<String>) {
        let dir = std::env::temp_dir().join(format!("lloom_review_test_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let prev = std::env::var("LLOOM_DATA_DIR").ok();
        std::env::set_var("LLOOM_DATA_DIR", &dir);
        db::init_db().unwrap();
        (dir, prev)
    }

    fn teardown(dir: std::path::PathBuf, prev: Option<String>) {
        match prev {
            Some(v) => std::env::set_var("LLOOM_DATA_DIR", v),
            None => std::env::remove_var("LLOOM_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn seed_model(name: &str, tier: i64, quality: f64, in_cost: f64, out_cost: f64) {
        let conn = db::open().unwrap();
        conn.execute(
            "INSERT INTO models (name, provider, litellm_model, capability_tier, quality_score,
                                 input_cost_per_token, output_cost_per_token, is_active,
                                 needs_calibration, health_state)
             VALUES (?1, 'test', ?1, ?2, ?3, ?4, ?5, 1, 0, 'up')",
            params![name, tier, quality, in_cost, out_cost],
        )
        .unwrap();
        drop(conn);
        db::upsert_price_spec("test", name, in_cost, out_cost, None, None, None, None, None, None)
            .unwrap();
    }

    /// 当前策略调成质量导向（cw=0.1 qw=0.9）→ 重放选贵强模型；
    /// 网格高 cost_weight 点应给出「换便宜模型」建议（相对省额远大于质量损失）。
    #[test]
    fn grid_search_suggests_cheaper_model() {
        let _g = db::TEST_DB_LOCK.lock().unwrap();
        let (dir, prev) = setup("grid");
        let suffix = std::process::id();
        let strong = format!("rev-strong-{suffix}");
        let cheap = format!("rev-cheap-{suffix}");
        seed_model(&strong, 4, 0.95, 2.4e-6, 9.6e-6);
        seed_model(&cheap, 3, 0.85, 1.1e-7, 4.4e-7);
        db::insert_routing_calibration("general", "h1", &strong, &strong, 1e-4, 1.2e-4, "shadow")
            .unwrap();
        // 质量导向策略（落库，grid_search 从库读）
        let mut p = db::get_routing_policy("general").unwrap().unwrap_or_default();
        p.task_type = "general".to_string();
        p.cost_weight = 0.1;
        p.quality_weight = 0.9;
        db::upsert_routing_policy(&p).unwrap();

        let suggestions = grid_search_suggestions().unwrap();
        assert_eq!(suggestions.len(), 1, "general 应有一条建议: {suggestions:?}");
        let s = &suggestions[0];
        assert_eq!(s["task_type"].as_str(), Some("general"));
        assert_eq!(s["current"]["model"].as_str(), Some(strong.as_str()));
        let sug_cost = s["suggested"]["est_cost"].as_f64().unwrap();
        let cur_cost = s["current"]["est_cost"].as_f64().unwrap();
        assert!(sug_cost < cur_cost, "建议成本应低于当前: {sug_cost} vs {cur_cost}");
        assert_eq!(s["suggested"]["model"].as_str(), Some(cheap.as_str()));
        // 建议权重必须来自网格（0.1 步进）
        let cw = s["suggested"]["cost_weight"].as_f64().unwrap();
        assert!((cw * 10.0).fract().abs() < 1e-9, "建议权重应在网格上: {cw}");

        teardown(dir, prev);
    }

    /// 无改进空间（唯一本地零成本模型）→ 不出建议。
    #[test]
    fn grid_search_no_suggestion_when_no_room() {
        let _g = db::TEST_DB_LOCK.lock().unwrap();
        let (dir, prev) = setup("noroom");
        let suffix = std::process::id();
        let only = format!("rev-only-{suffix}");
        seed_model(&only, 3, 0.8, 0.0, 0.0);
        db::insert_routing_calibration("simple_qa", "h1", &only, &only, 0.0, 0.0, "shadow").unwrap();

        let suggestions = grid_search_suggestions().unwrap();
        assert!(suggestions.is_empty(), "唯一模型无从优化: {suggestions:?}");

        teardown(dir, prev);
    }

    /// 采纳建议 → 策略权重更新 → 下一次重放换选便宜模型（单一真源生效验证）。
    #[test]
    fn adopt_updates_policy_and_changes_selection() {
        let _g = db::TEST_DB_LOCK.lock().unwrap();
        let (dir, prev) = setup("adopt");
        let suffix = std::process::id();
        let strong = format!("ad-strong-{suffix}");
        let cheap = format!("ad-cheap-{suffix}");
        seed_model(&strong, 4, 0.95, 2.4e-6, 9.6e-6);
        seed_model(&cheap, 3, 0.85, 1.1e-7, 4.4e-7);
        db::insert_routing_calibration("general", "h1", &strong, &strong, 1e-4, 1.2e-4, "shadow")
            .unwrap();
        let mut p = db::get_routing_policy("general").unwrap().unwrap_or_default();
        p.task_type = "general".to_string();
        p.cost_weight = 0.1;
        p.quality_weight = 0.9;
        db::upsert_routing_policy(&p).unwrap();

        let suggestions = grid_search_suggestions().unwrap();
        assert_eq!(suggestions.len(), 1, "应有可采纳建议: {suggestions:?}");
        // 建议先落一份报告（adopt 从 policy_review 读）
        db::insert_policy_review(
            1, 0.0, 0.8, 1e-4, 0.95, 1.2e-4, 0.95, 1.0, 16.0, "test", "{}",
            &serde_json::to_string(&suggestions).unwrap(),
        )
        .unwrap();

        let r = adopt_suggestions(None).unwrap();
        assert!(r["ok"].as_bool().unwrap());
        assert_eq!(r["adopted"].as_array().unwrap().len(), 1);

        // 采纳后：读库策略已更新，且重放确实换选便宜模型
        let p = db::get_routing_policy("general").unwrap().unwrap();
        assert_eq!(p.cost_weight, suggestions[0]["suggested"]["cost_weight"].as_f64().unwrap());
        let models = db::list_models(true).unwrap();
        let mut spec_map = HashMap::new();
        for spec in db::list_price_specs().unwrap() {
            spec_map.insert((spec.provider.clone(), spec.model.clone()), spec);
        }
        let zr = ZoneResolver::new();
        let (model, _, _) = replay_once("general", &p, &models, &spec_map, &zr).unwrap();
        assert_eq!(model, cheap, "采纳后重放应换选便宜模型");

        teardown(dir, prev);
    }

    /// policy_review 插入/读取往返 + 预算档分布解析（含旧记录 unknown 兜底）。
    #[test]
    fn policy_review_roundtrip_and_tier_distribution() {
        let _g = db::TEST_DB_LOCK.lock().unwrap();
        let (dir, prev) = setup("roundtrip");
        let id = db::insert_policy_review(
            3, 0.5, 0.6, 1.0, 0.7, 2.0, 0.9, 0.25, 50.0, "结论", r#"{"normal":2}"#, "[]",
        )
        .unwrap();
        let latest = db::latest_policy_review().unwrap().unwrap();
        assert_eq!(latest.id, id);
        assert_eq!(latest.samples, 3);
        assert!((latest.aiq - 0.25).abs() < 1e-9);
        assert_eq!(latest.conclusion, "结论");

        // 两条决策审计：一条带 budget_tier，一条旧格式（unknown 兜底）
        let conn = db::open().unwrap();
        conn.execute(
            "INSERT INTO routing_decisions (request_id, task_type, band, signals_json, candidates_json, selected, fallback_chain, routing_ms)
             VALUES ('r1', 'general', 'medium', '{\"budget_tier\":\"throttle\"}', '[]', 'm', '', 1.0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO routing_decisions (request_id, task_type, band, signals_json, candidates_json, selected, fallback_chain, routing_ms)
             VALUES ('r2', 'general', 'medium', '{\"method\":\"rule\"}', '[]', 'm', '', 1.0)",
            [],
        )
        .unwrap();
        drop(conn);
        let dist = db::budget_tier_distribution(0).unwrap();
        assert_eq!(
            dist,
            vec![("throttle".to_string(), 1), ("unknown".to_string(), 1)]
        );

        teardown(dir, prev);
    }

    /// point_score 纯函数：省成本/提质量/无改进三档。
    #[test]
    fn point_score_directions() {
        // 省成本：省 50%，质量掉 0.1 → 0.5−0.1=0.4
        assert!((point_score(0.5, 0.85, 1.0, 0.95) - 0.4).abs() < 1e-9);
        // 提质量：+0.1 质量，成本翻倍 → 0.1−0.5×1=−0.4
        assert!((point_score(2.0, 1.05, 1.0, 0.95) - (-0.4)).abs() < 1e-9);
        // 无改进（同点）→ −∞
        assert!(point_score(1.0, 0.95, 1.0, 0.95).is_infinite() && point_score(1.0, 0.95, 1.0, 0.95) < 0.0);
        // 零成本当前（本地）：等成本提质量 → 增益全额
        assert!((point_score(0.0, 0.9, 0.0, 0.8) - 0.1).abs() < 1e-9);
    }
}
