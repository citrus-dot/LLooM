//! Strongly-typed domain models for LLooM.
//!
//! `Model` is the domain layer: it owns a `Backend` (local vs cloud) plus the
//! routing metadata shared by both kinds. Persistence goes through
//! `db::ModelRow` (flat projection of the SQLite table) via explicit `From`
//! conversions; the HTTP layer uses dedicated DTOs in `server.rs`.

use serde::de::{self, Deserializer};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ── Backend（本地/云端判别）──

/// 云端运营商。未知字符串一律落入 `Custom`，保证对存量数据与新增供应商开放。
#[derive(Debug, Clone, PartialEq)]
pub enum Provider {
    DashScope,
    OpenAI,
    Anthropic,
    Custom(String),
}

impl Provider {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "dashscope" => Provider::DashScope,
            "openai" => Provider::OpenAI,
            "anthropic" => Provider::Anthropic,
            other => Provider::Custom(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Provider::DashScope => "dashscope",
            Provider::OpenAI => "openai",
            Provider::Anthropic => "anthropic",
            Provider::Custom(s) => s,
        }
    }
}

impl Serialize for Provider {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Provider {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(Provider::parse(&s))
    }
}

/// 本地服务的兼容协议：Ollama 原生（litellm 前缀 `ollama/`）或 OpenAI 兼容端点
/// （LM Studio / vLLM 等，litellm 前缀 `openai/`）。
#[derive(Debug, Clone, PartialEq)]
pub enum LocalCompat {
    Ollama,
    OpenAiCompat,
}

impl LocalCompat {
    pub fn parse(s: &str) -> Self {
        if s.eq_ignore_ascii_case("ollama") {
            LocalCompat::Ollama
        } else {
            LocalCompat::OpenAiCompat
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            LocalCompat::Ollama => "ollama",
            LocalCompat::OpenAiCompat => "openai",
        }
    }

    pub fn litellm_prefix(&self) -> &'static str {
        match self {
            LocalCompat::Ollama => "ollama",
            LocalCompat::OpenAiCompat => "openai",
        }
    }
}

/// 云端 API key 的两种来源：环境变量名或 `sk-` 开头的字面密钥。
/// 判别规则与 `config::api_key_for` 一致（`sk-` 开头且不含下划线 → 字面值）。
#[derive(Debug, Clone, PartialEq)]
pub enum ApiKeyRef {
    EnvName(String),
    Literal(String),
}

impl ApiKeyRef {
    /// 空串 → None；其余按 `sk-` 启发式判别。
    pub fn parse(s: &str) -> Option<Self> {
        if s.is_empty() {
            return None;
        }
        let is_literal = s.starts_with("sk-") && !s.contains('_');
        Some(if is_literal {
            ApiKeyRef::Literal(s.to_string())
        } else {
            ApiKeyRef::EnvName(s.to_string())
        })
    }

    /// 原始字符串（env 名或字面密钥），落库/展示用。
    pub fn raw(&self) -> &str {
        match self {
            ApiKeyRef::EnvName(s) | ApiKeyRef::Literal(s) => s,
        }
    }

    /// 运行时解析为真实密钥（env 名 → 环境变量值；字面 → 原值）。
    pub fn resolve(&self) -> String {
        crate::config::api_key_for(self.raw())
    }
}

impl Serialize for ApiKeyRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.raw())
    }
}

impl<'de> Deserialize<'de> for ApiKeyRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        ApiKeyRef::parse(&s).ok_or_else(|| de::Error::custom("api_key must not be empty"))
    }
}

/// 模型的调用后端：本地（无 key）或云端（运营商 + 可选地址 + 可选 key）。
///
/// serde 采用内部 tag（`{"kind":"cloud","provider":"dashscope",...}` /
/// `{"kind":"local","compat":"ollama",...}`），None/空串统一序列化为 `""`。
#[derive(Debug, Clone, PartialEq)]
pub enum Backend {
    Cloud {
        provider: Provider,
        api_base: Option<String>,
        api_key: Option<ApiKeyRef>,
    },
    Local {
        compat: LocalCompat,
        api_base: String,
    },
}

impl Backend {
    /// litellm 模型字符串前缀；`Custom` 供应商按 OpenAI 兼容协议调用。
    pub fn litellm_prefix(&self) -> &str {
        match self {
            Backend::Cloud { provider, .. } => match provider {
                Provider::DashScope => "dashscope",
                Provider::OpenAI => "openai",
                Provider::Anthropic => "anthropic",
                Provider::Custom(_) => "openai",
            },
            Backend::Local { compat, .. } => compat.litellm_prefix(),
        }
    }
}

impl Serialize for Backend {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let v = match self {
            Backend::Cloud { provider, api_base, api_key } => json!({
                "kind": "cloud",
                "provider": provider.as_str(),
                "api_base": api_base.clone().unwrap_or_default(),
                "api_key": api_key.as_ref().map(|k| k.raw()).unwrap_or_default(),
            }),
            Backend::Local { compat, api_base } => json!({
                "kind": "local",
                "compat": compat.as_str(),
                "api_base": api_base,
            }),
        };
        v.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Backend {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(deserializer)?;
        let obj = v
            .as_object()
            .ok_or_else(|| de::Error::custom("backend must be an object"))?;
        let get_str = |key: &str| obj.get(key).and_then(|x| x.as_str()).unwrap_or_default();
        match obj.get("kind").and_then(|k| k.as_str()) {
            Some("local") => Ok(Backend::Local {
                compat: LocalCompat::parse(get_str("compat")),
                api_base: get_str("api_base").to_string(),
            }),
            Some("cloud") => Ok(Backend::Cloud {
                provider: Provider::parse(get_str("provider")),
                api_base: Some(get_str("api_base")).filter(|s| !s.is_empty()).map(String::from),
                api_key: ApiKeyRef::parse(get_str("api_key")),
            }),
            Some(other) => Err(de::Error::custom(format!("unknown backend kind '{other}'"))),
            // 缺 kind 时按云端宽容解析（存量 DTO 兼容）
            None => Ok(Backend::Cloud {
                provider: Provider::parse(get_str("provider")),
                api_base: Some(get_str("api_base")).filter(|s| !s.is_empty()).map(String::from),
                api_key: ApiKeyRef::parse(get_str("api_key")),
            }),
        }
    }
}

// ── Model ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model {
    #[serde(default)]
    pub id: i64,
    pub name: String,
    pub litellm_model: String,
    pub backend: Backend,
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

impl Model {
    pub fn is_local(&self) -> bool {
        matches!(self.backend, Backend::Local { .. })
    }

    /// 供应商标识（价格键 / overlay 键 / 归属展示用）。本地 OpenAI 兼容端点
    /// 归入 `custom`，与 DB provider 列的编码保持一致。
    pub fn provider_name(&self) -> &str {
        match &self.backend {
            Backend::Cloud { provider, .. } => provider.as_str(),
            Backend::Local { compat: LocalCompat::Ollama, .. } => "ollama",
            Backend::Local { compat: LocalCompat::OpenAiCompat, .. } => "custom",
        }
    }

    /// 原始调用地址（未做 env 解析；解析只发生在出进程边界的 ModelSpec）。
    pub fn api_base(&self) -> &str {
        match &self.backend {
            Backend::Cloud { api_base, .. } => api_base.as_deref().unwrap_or_default(),
            Backend::Local { api_base, .. } => api_base,
        }
    }

    /// key 引用原文（env 名或字面密钥）；本地模型恒为空。
    pub fn api_key_env(&self) -> &str {
        match &self.backend {
            Backend::Cloud { api_key: Some(k), .. } => k.raw(),
            _ => "",
        }
    }

    /// The ModelSpec payload sent to the Python AI service.
    pub fn to_ai_spec(&self, api_key: &str) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "litellm_model": self.litellm_model,
            "api_base": self.api_base(),
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

#[cfg(test)]
impl Model {
    /// 测试 fixture：云端模型。
    pub fn fixture(name: &str, provider: &str) -> Model {
        Model {
            id: 0,
            name: name.to_string(),
            litellm_model: format!("{provider}/{name}"),
            backend: Backend::Cloud {
                provider: Provider::parse(provider),
                api_base: None,
                api_key: None,
            },
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
            priority: 0,
            health_state: "unknown".to_string(),
            needs_calibration: 0,
        }
    }

    /// 测试 fixture：本地模型。
    pub fn local_fixture(name: &str, compat: LocalCompat) -> Model {
        Model {
            backend: Backend::Local {
                compat,
                api_base: "http://localhost:11434".to_string(),
            },
            ..Model::fixture(name, "ollama")
        }
    }
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
