//! N3.b：Prometheus 文本格式指标导出（`GET /metrics`）。
//!
//! 不引第三方 metrics 库——纯文本渲染 DB 聚合，零新依赖。
//! 数据真源全部复用既有表：usage_records / routing_decisions / models.health_state。
//! 格式：Prometheus text exposition 0.0.4（`text/plain; version=0.0.4`）。

use crate::db::Db;

/// Prometheus label 值转义（`\` `"` `\n`）。
fn escape_label(v: &str) -> String {
    v.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn metric(name: &str, labels: &[(&str, &str)], value: f64) -> String {
    let val = if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    };
    if labels.is_empty() {
        format!("{name} {val}\n")
    } else {
        let joined = labels
            .iter()
            .map(|(k, v)| format!("{}=\"{}\"", k, escape_label(v)))
            .collect::<Vec<_>>()
            .join(",");
        format!("{name}{{{joined}}} {val}\n")
    }
}

/// models.health_state → 数值（HELP 中注明映射）。
fn health_value(state: &str) -> f64 {
    match state {
        "up" => 3.0,
        "degraded" => 2.0,
        "down" => 1.0,
        _ => 0.0, // unknown / 其它
    }
}

/// 渲染全部指标文本。数据缺失（空表）时输出 0 值或省略标签组，格式始终合法。
pub fn render(db: &Db) -> std::result::Result<String, crate::error::AppError> {
    let mut out = String::with_capacity(4096);

    // ── 生命周期 ──
    out.push_str("# HELP lloom_up Whether the LLooM server is up.\n");
    out.push_str("# TYPE lloom_up gauge\n");
    out.push_str(&metric("lloom_up", &[], 1.0));
    out.push_str("# HELP lloom_build_info Build version info.\n");
    out.push_str("# TYPE lloom_build_info gauge\n");
    out.push_str(&metric(
        "lloom_build_info",
        &[("version", env!("CARGO_PKG_VERSION"))],
        1.0,
    ));

    // ── 用量：请求数（model × task_type × api_source） ──
    out.push_str("# HELP lloom_usage_requests_total Total LLM requests served.\n");
    out.push_str("# TYPE lloom_usage_requests_total counter\n");
    for (model, task_type, source, n) in db.metrics_usage_by_source()? {
        out.push_str(&metric(
            "lloom_usage_requests_total",
            &[
                ("model", &model),
                ("task_type", task_type.as_str()),
                ("api_source", source.as_str()),
            ],
            n as f64,
        ));
    }

    // ── 用量：token / 成本 / 缓存（per model） ──
    out.push_str(
        "# HELP lloom_usage_tokens_total Tokens consumed by direction (input/output).\n",
    );
    out.push_str("# TYPE lloom_usage_tokens_total counter\n");
    out.push_str("# HELP lloom_usage_cost_total Accumulated actual cost in USD.\n");
    out.push_str("# TYPE lloom_usage_cost_total counter\n");
    out.push_str("# HELP lloom_cache_hits_total Requests served from cache.\n");
    out.push_str("# TYPE lloom_cache_hits_total counter\n");
    out.push_str("# HELP lloom_cache_saved_cost_total USD saved by cache hits.\n");
    out.push_str("# TYPE lloom_cache_saved_cost_total counter\n");
    for s in db.get_usage_stats(None, None, None)? {
        out.push_str(&metric(
            "lloom_usage_tokens_total",
            &[("model", &s.model_name), ("direction", "input")],
            s.total_input_tokens as f64,
        ));
        out.push_str(&metric(
            "lloom_usage_tokens_total",
            &[("model", &s.model_name), ("direction", "output")],
            s.total_output_tokens as f64,
        ));
        out.push_str(&metric("lloom_usage_cost_total", &[("model", &s.model_name)], s.total_cost));
        out.push_str(&metric("lloom_cache_hits_total", &[("model", &s.model_name)], s.cache_hits as f64));
        out.push_str(&metric(
            "lloom_cache_saved_cost_total",
            &[("model", &s.model_name)],
            s.cache_saved,
        ));
    }

    // ── 路由：决策结果（task_type × outcome） ──
    out.push_str("# HELP lloom_routing_decisions_total Routing decisions by task_type and outcome.\n");
    out.push_str("# TYPE lloom_routing_decisions_total counter\n");
    for (task_type, outcome, n) in db.metrics_routing_by_outcome()? {
        out.push_str(&metric(
            "lloom_routing_decisions_total",
            &[("task_type", task_type.as_str()), ("outcome", outcome.as_str())],
            n as f64,
        ));
    }

    // ── 路由：故障转移（fallback / escalation 升档）事件 ──
    out.push_str(
        "# HELP lloom_failover_total Successful requests where the serving model differed from the primary selection (fallback or escalation).\n",
    );
    out.push_str("# TYPE lloom_failover_total counter\n");
    out.push_str(&metric(
        "lloom_failover_total",
        &[],
        db.metrics_failover_count()? as f64,
    ));

    // ── 路由：开销（routing_decisions.routing_ms） ──
    let (count, avg, p95, max, slow) = db.routing_overhead_report(0)?;
    out.push_str("# HELP lloom_routing_overhead_count Routing decisions measured.\n");
    out.push_str("# TYPE lloom_routing_overhead_count gauge\n");
    out.push_str(&metric("lloom_routing_overhead_count", &[], count as f64));
    out.push_str("# HELP lloom_routing_overhead_ms Routing overhead in ms by stat (avg/p95/max).\n");
    out.push_str("# TYPE lloom_routing_overhead_ms gauge\n");
    for (stat, v) in [("avg", avg), ("p95", p95), ("max", max)] {
        out.push_str(&metric("lloom_routing_overhead_ms", &[("stat", stat)], v));
    }
    out.push_str("# HELP lloom_routing_slow_total Routing decisions slower than 100ms.\n");
    out.push_str("# TYPE lloom_routing_slow_total gauge\n");
    out.push_str(&metric("lloom_routing_slow_total", &[], slow as f64));

    // ── 预算档分布（signals_json.budget_tier） ──
    out.push_str("# HELP lloom_budget_tier_total Routing decisions by budget tier.\n");
    out.push_str("# TYPE lloom_budget_tier_total counter\n");
    for (tier, n) in db.budget_tier_distribution(0)? {
        out.push_str(&metric("lloom_budget_tier_total", &[("tier", &tier)], n as f64));
    }

    // ── 模型健康（3=up 2=degraded 1=down 0=unknown） ──
    out.push_str("# HELP lloom_model_health Model health state (3=up 2=degraded 1=down 0=unknown).\n");
    out.push_str("# TYPE lloom_model_health gauge\n");
    for m in db.list_models(false)? {
        out.push_str(&metric(
            "lloom_model_health",
            &[("model", &m.name)],
            health_value(&m.health_state),
        ));
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::UsageRecord;

    /// 每个 test 用独立临时库，避免并行共享相互干扰（项目测试惯例）。
    fn mem_db(tag: &str) -> Db {
        let dir = std::env::temp_dir().join(format!("lloom_metrics_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        Db::new(dir.join(format!("{tag}.db"))).expect("test db")
    }

    #[test]
    fn test_escape_label() {
        assert_eq!(escape_label("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
        assert_eq!(escape_label("plain"), "plain");
    }

    #[test]
    fn test_metric_format() {
        assert_eq!(metric("m", &[], 3.0), "m 3\n");
        assert_eq!(metric("m", &[("k", "v")], 1.5), "m{k=\"v\"} 1.5\n");
        assert_eq!(
            metric("m", &[("k", "a\"b")], 2.0),
            "m{k=\"a\\\"b\"} 2\n"
        );
    }

    #[test]
    fn test_render_empty_db_is_valid() {
        let db = mem_db("empty");
        let body = render(&db).expect("render");
        // 空库也应输出生命周期与开销等基线指标
        assert!(body.contains("lloom_up 1\n"));
        assert!(body.contains("lloom_routing_overhead_count 0\n"));
        assert!(body.contains("lloom_failover_total 0\n"));
        // 每行非注释行均为 name 或 name{labels} value 形态
        for line in body.lines().filter(|l| !l.starts_with('#')) {
            let mut it = line.split_whitespace();
            let name = it.next().expect("metric name");
            assert!(it.next().is_some(), "missing value: {line}");
            assert!(
                name.chars().next().unwrap().is_ascii_alphabetic(),
                "bad metric name: {name}"
            );
        }
    }

    #[test]
    fn test_render_with_data() {
        let db = mem_db("data");
        // 两条 usage：同 request_id，实际模型 ≠ 主选 → failover 计 1
        db.insert_usage(&UsageRecord {
            model_name: "fallback-model",
            user_id: "default",
            input_tokens: 100,
            output_tokens: 50,
            cost: 0.001,
            task_type: Some("coding"),
            cache_hit: false,
            latency_ms: Some(120.0),
            request_id: Some("chat-1"),
            extra: Some(crate::db::UsageExtra {
                api_source: Some("proxy".to_string()),
                ..Default::default()
            }),
        })
        .unwrap();
        db.insert_usage(&UsageRecord {
            model_name: "fallback-model",
            user_id: "default",
            input_tokens: 10,
            output_tokens: 5,
            cost: 0.0001,
            task_type: Some("coding"),
            cache_hit: false,
            latency_ms: Some(80.0),
            request_id: Some("chat-2"),
            extra: None,
        })
        .unwrap();
        db.insert_routing_decision(&crate::db::RoutingDecisionRecord {
            request_id: "chat-1",
            task_type: "coding",
            band: "medium",
            signals_json: "{\"budget_tier\":\"normal\"}",
            candidates_json: "[\"fallback-model\"]",
            selected: "primary-model",
            fallback_chain: "fallback-model",
            routing_ms: 5.0,
        })
        .unwrap();
        db.update_routing_decision_outcome(1, "success").unwrap();

        let body = render(&db).expect("render");
        assert!(body.contains(
            "lloom_usage_requests_total{model=\"fallback-model\",task_type=\"coding\",api_source=\"proxy\"} 1\n"
        ));
        assert!(body.contains(
            "lloom_usage_tokens_total{model=\"fallback-model\",direction=\"input\"} 110\n"
        ));
        assert!(body.contains("lloom_usage_cost_total{model=\"fallback-model\"} 0.0011"));
        assert!(body.contains(
            "lloom_routing_decisions_total{task_type=\"coding\",outcome=\"success\"} 1\n"
        ));
        assert!(body.contains("lloom_failover_total 1\n"));
        assert!(body.contains("lloom_budget_tier_total{tier=\"normal\"} 1\n"));
    }
}
