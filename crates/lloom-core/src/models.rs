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
}

fn default_rpm() -> i64 {
    60
}

fn default_active() -> i64 {
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
}

// ── Env / Config ──

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnvConfig {
    #[serde(flatten)]
    pub values: std::collections::HashMap<String, String>,
}
