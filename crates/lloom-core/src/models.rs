//! Strongly-typed domain models for LLooM.
//!
//! These mirror the SQLite schema in `db.rs` and the wire formats shared with
//! the frontend and the Python AI service.

use serde::{Deserialize, Serialize};

// ── Model ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    #[serde(default)]
    pub id: i64,
    pub name: String,
    pub provider: String,
    pub litellm_model: String,
    #[serde(default)]
    pub api_base: String,
    #[serde(default)]
    pub api_key_env: String,
    #[serde(default)]
    pub task_type: String,
    #[serde(default)]
    pub input_cost_per_token: f64,
    #[serde(default)]
    pub output_cost_per_token: f64,
    #[serde(default = "default_rpm")]
    pub rpm: i64,
    #[serde(default = "default_active")]
    pub is_active: i64,
    // ── P0.b 路由元数据（成本真源在 price_specs，models 不再加价格列）──
    /// 能力档 1/2/3（light/general/flagship），plan() 门槛用
    #[serde(default = "default_tier")]
    pub capability_tier: i64,
    /// 冷启动质量分 0..1（P1.c 的 ewma_quality 接线前的兜底）
    #[serde(default = "default_quality")]
    pub quality_score: f64,
    /// 上下文窗口（token），门槛过滤用
    #[serde(default = "default_ctx")]
    pub context_window: i64,
    #[serde(default)]
    pub supports_tools: i64,
    #[serde(default)]
    pub supports_vision: i64,
    /// 须流式调用（推理系模型，非流式易超时）；0=非流式可用
    #[serde(default)]
    pub supports_stream: i64,
    /// 本地模型（Ollama 等，零成本兜底）
    #[serde(default)]
    pub is_local: i64,
    /// 人工偏好加权（评分 +0.05/级）
    #[serde(default)]
    pub priority: i64,
    /// unknown/up/degraded/down（P3 健康状态机维护，系统写）
    #[serde(default = "default_health")]
    pub health_state: String,
    /// 保守期标记：sample_count<20 的模型在复杂任务上扣分
    #[serde(default = "default_needs_cal")]
    pub needs_calibration: i64,
}

fn default_rpm() -> i64 {
    60
}

fn default_active() -> i64 {
    1
}

fn default_tier() -> i64 {
    2
}

fn default_quality() -> f64 {
    0.6
}

fn default_ctx() -> i64 {
    32768
}

fn default_health() -> String {
    "unknown".to_string()
}

fn default_needs_cal() -> i64 {
    1
}

impl Model {
    /// The ModelSpec payload sent to the Python AI service.
    pub fn to_ai_spec(&self, api_key: &str) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "litellm_model": self.litellm_model,
            "api_base": self.api_base,
            "api_key": api_key,
            "input_cost_per_token": self.input_cost_per_token,
            "output_cost_per_token": self.output_cost_per_token,
        })
    }

    pub fn calculate_cost(&self, input_tokens: i64, output_tokens: i64) -> f64 {
        input_tokens as f64 * self.input_cost_per_token
            + output_tokens as f64 * self.output_cost_per_token
    }
}

// ── Usage ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStats {
    pub model_name: String,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_cost: f64,
    pub request_count: i64,
    pub cache_hits: i64,
    /// P2.b 语义缓存命中累计省下的金额（USD）。
    #[serde(default)]
    pub cache_saved: f64,
}

// ── Budget ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budget {
    #[serde(default)]
    pub id: i64,
    pub scope: String,
    pub scope_id: String,
    pub max_budget: f64,
    pub duration: String,
    // P5.b 预算模型扩展（可空）
    #[serde(default)]
    pub scope_task_type: Option<String>,
    #[serde(default)]
    pub soft_limit_ratio: Option<f64>,
    #[serde(default)]
    pub action_on_exceed: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetCheck {
    pub within_budget: bool,
    pub budget: Option<Budget>,
    pub spent: f64,
}

// ── Conversation ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMeta {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub message_count: usize,
}

// ── Security report ──

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SecurityReport {
    pub blocked: bool,
    pub block_reason: String,
    #[serde(default)]
    pub pii: serde_json::Value,
    #[serde(default)]
    pub jailbreak: serde_json::Value,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub domain_method: String,
    #[serde(default)]
    pub processed_text: String,
}

// ── Service status ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub name: String,
    pub status: String,
    pub healthy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteStatus {
    pub services: Vec<ServiceStatus>,
    pub total: usize,
    pub healthy: usize,
    pub running: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartRestartResult {
    pub ok: bool,
    pub restarted: Vec<String>,
    pub errors: Vec<String>,
}

// ── Routing ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    pub model: String,
    pub task_type: String,
    pub method: String,
    pub stream: bool,
    /// 难度带 easy/medium/hard（投影层输出，P0.d 简版来自 task_type 映射）
    #[serde(default)]
    pub band: String,
    /// N2.a：本次决策时的预算档（normal/throttle/tight/protect），落 signals_json 供体检分布
    #[serde(default)]
    pub budget_tier: String,
    /// 候补链（P3 故障转移按序重试；本阶段仅审计透传）
    #[serde(default)]
    pub fallback_chain: Vec<String>,
}

/// P0.c 任务级路由策略（routing_policy 表行）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingPolicy {
    pub task_type: String,
    pub min_capability_tier: i64,
    pub cost_weight: f64,
    pub quality_weight: f64,
    pub latency_weight: f64,
    #[serde(default)]
    pub max_cost_per_request: Option<f64>,
    #[serde(default)]
    pub pinned_model: Option<String>,
    pub fallback_depth: i64,
    pub escalation_enabled: i64,
}

impl Default for RoutingPolicy {
    fn default() -> Self {
        Self {
            task_type: "general".to_string(),
            min_capability_tier: 2,
            cost_weight: 0.5,
            quality_weight: 0.4,
            latency_weight: 0.1,
            max_cost_per_request: None,
            pinned_model: None,
            fallback_depth: 2,
            escalation_enabled: 0,
        }
    }
}

/// N2.a 路由体检报告（policy_review 表行）。三线成本/质量来自 aiq_replay.py 重放，
/// suggestions_json 为 N2.b 网格搜索产出的权重建议（供 adopt 端点人工采纳）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyReview {
    pub id: i64,
    pub created_at: String,
    pub samples: i64,
    pub weak_cost: f64,
    pub weak_quality: f64,
    pub cur_cost: f64,
    pub cur_quality: f64,
    pub strong_cost: f64,
    pub strong_quality: f64,
    pub aiq: f64,
    pub saved_pct: f64,
    pub conclusion: String,
    #[serde(default)]
    pub budget_tiers_json: String,
    #[serde(default)]
    pub suggestions_json: String,
}

/// P1.d 影子评测样本（routing_calibration 表行），N2.b 网格搜索重放的输入。
#[derive(Debug, Clone)]
pub struct CalibrationRow {
    pub task_type: String,
    pub query_hash: String,
    pub routed_model: String,
    pub baseline_model: String,
    pub routed_cost: f64,
    pub baseline_cost: f64,
    pub routed_quality: Option<f64>,
    pub baseline_quality: Option<f64>,
}

/// P0.c 模型×任务成效分（model_task_score 表行），ewma_quality 由信号回填
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelTaskScore {
    pub model_name: String,
    pub task_type: String,
    pub success_count: i64,
    pub fail_count: i64,
    pub escalation_count: i64,
    pub avg_cost: f64,
    pub avg_latency_ms: f64,
    pub ewma_quality: f64,
    pub sample_count: i64,
    // P5.c：该 (model, task_type) 真实 output_tokens 的 EWMA 滚动均值（默认 500=历史固定 est_out）。
    #[serde(default)]
    pub avg_out_tokens: f64,
}

/// P1.c 成效信号：枚举 → σ 值（EWMA 输入），并决定 success/fail/escalation 计数器自增方向。
///
/// σ 值约定（ROUTING-PLAN §P1.c）：
/// 正常完成 +0.7、子任务失败 −0.5、cascade 升级 −0.4、重生成/切模型 −0.6、
/// reask 隐式不满 −0.4、点赞 +1.0、点踩 −1.0、结构化解析失败 −0.3。
/// 负信号会把 ewma_quality 向下拉，最终结果被 clamp 到 [0,1]（读侧合法性），
/// 输入 σ 本身**不做** clamp——否则负反馈会被误丢弃。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualitySignalKind {
    /// 正常完成（chat 成功 / task_done 无 error）：+0.7
    Success,
    /// 子任务失败（task_done.error 非空）：−0.5
    SubtaskFail,
    /// cascade 升级（回退链重试命中）：−0.4
    Escalation,
    /// 重生成 / 切模型重问（同对话短间隔新请求 + 不同模型）：−0.6
    ModelRegen,
    /// reask 隐式不满（同对话相似度>阈值且间隔短）：−0.4
    Reask,
    /// 点赞（cache_feedback correct=true）：+1.0
    Like,
    /// 点踩（cache_feedback correct=false）：−1.0
    Dislike,
    /// 结构化解析失败（JSON schema 校验失败）：−0.3
    ParseFail,
}

impl QualitySignalKind {
    /// 该信号在 EWMA 公式 `ewma ← (1-α)·ewma + α·σ` 中的 σ 值。
    pub fn value(self) -> f64 {
        match self {
            Self::Success => 0.7,
            Self::SubtaskFail => -0.5,
            Self::Escalation => -0.4,
            Self::ModelRegen => -0.6,
            Self::Reask => -0.4,
            Self::Like => 1.0,
            Self::Dislike => -1.0,
            Self::ParseFail => -0.3,
        }
    }
}

// ── Env / Config ──

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnvConfig {
    #[serde(flatten)]
    pub values: std::collections::HashMap<String, String>,
}
