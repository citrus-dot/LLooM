//! Model API DTO 层：HTTP 边界与领域层 `Model` 之间的显式转换。
//!
//! - `ModelCreate`：POST /api/models 的入参（kind: local|cloud），`TryFrom` 校验后落领域层
//! - `ModelPatch`：PUT /api/models/{name} 的 typed 部分更新，`resolve_against` 整体校验
//! - `ModelDto`：GET 响应，api_key 以 `****tail` 掩码输出
//!
//! 非法组合（本地带 key、云端缺 provider、错 litellm 前缀）在此层被拒收，
//! 不再流向领域/持久层。

use crate::error::{AppError, Result};
use crate::models::{ApiKeyRef, Backend, LocalCompat, Model, Provider};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 本地服务的缺省调用地址。
fn default_local_base(compat: &LocalCompat) -> &'static str {
    match compat {
        LocalCompat::Ollama => "http://localhost:11434",
        LocalCompat::OpenAiCompat => "http://localhost:1234/v1",
    }
}

/// 密钥掩码：`****` + 末 4 位（与 get_config 的 env 掩码同一约定）。
pub(crate) fn mask_secret(v: &str) -> String {
    let tail: String = v.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    if v.len() <= 4 {
        "****".to_string()
    } else {
        format!("****{tail}")
    }
}

// ── POST /api/models ──

#[derive(Debug, Clone, Deserialize)]
pub struct ModelCreate {
    pub name: String,
    /// "local" | "cloud"（必填）
    pub kind: String,
    /// 本地兼容协议："ollama"（默认）| "openai"
    #[serde(default)]
    pub compat: Option<String>,
    /// 云端运营商：dashscope / openai / anthropic / custom
    #[serde(default)]
    pub provider: Option<String>,
    /// 调用地址；本地缺省按 compat，云端可空（走供应商默认/env 解析）
    #[serde(default)]
    pub api_base: Option<String>,
    /// env 名或 `sk-` 字面密钥；本地必须为空
    #[serde(default)]
    pub api_key: Option<String>,
    /// LiteLLM 模型串；缺省自动按 `{前缀}/{name}` 拼接，错前缀拒收
    #[serde(default)]
    pub litellm_model: Option<String>,
    #[serde(default)]
    pub task_type: Option<String>,
    #[serde(default)]
    pub input_cost_per_token: Option<f64>,
    #[serde(default)]
    pub output_cost_per_token: Option<f64>,
    #[serde(default)]
    pub rpm: Option<i64>,
    #[serde(default)]
    pub capability_tier: Option<i64>,
}

impl TryFrom<ModelCreate> for Model {
    type Error = AppError;

    fn try_from(c: ModelCreate) -> Result<Self> {
        if c.name.trim().is_empty() {
            return Err(AppError::InvalidRequest("模型名不能为空".into()));
        }
        let backend = match c.kind.as_str() {
            "local" => {
                if ApiKeyRef::parse(c.api_key.as_deref().unwrap_or_default()).is_some() {
                    return Err(AppError::InvalidRequest("本地模型不配置 API Key".into()));
                }
                let compat =
                    c.compat.as_deref().map(LocalCompat::parse).unwrap_or(LocalCompat::Ollama);
                let api_base = match c.api_base.as_deref().filter(|s| !s.is_empty()) {
                    Some(b) => b.to_string(),
                    None => default_local_base(&compat).to_string(),
                };
                Backend::Local { compat, api_base }
            }
            "cloud" => {
                let provider = c
                    .provider
                    .as_deref()
                    .map(Provider::parse)
                    .ok_or_else(|| AppError::InvalidRequest("云端模型必须指定 provider".into()))?;
                Backend::Cloud {
                    provider,
                    api_base: c.api_base.clone().filter(|s| !s.is_empty()),
                    api_key: ApiKeyRef::parse(c.api_key.as_deref().unwrap_or_default()),
                }
            }
            other => {
                return Err(AppError::InvalidRequest(format!(
                    "kind 必须是 local|cloud，收到 '{other}'"
                )))
            }
        };
        let litellm_model = match c.litellm_model.as_deref().unwrap_or_default() {
            "" => format!("{}/{}", backend.litellm_prefix(), c.name),
            s if s.starts_with(&format!("{}/", backend.litellm_prefix())) => s.to_string(),
            s => {
                return Err(AppError::InvalidRequest(format!(
                    "litellm_model '{s}' 前缀应为 '{}/'",
                    backend.litellm_prefix()
                )))
            }
        };
        Ok(Model {
            id: 0,
            name: c.name,
            litellm_model,
            backend,
            task_type: c.task_type.unwrap_or_else(|| "general".into()),
            input_cost_per_token: c.input_cost_per_token.unwrap_or(0.0),
            output_cost_per_token: c.output_cost_per_token.unwrap_or(0.0),
            rpm: c.rpm.unwrap_or(60),
            is_active: 1,
            capability_tier: c.capability_tier.unwrap_or(2),
            quality_score: 0.6,
            context_window: 32768,
            supports_tools: 0,
            supports_vision: 0,
            supports_stream: 0,
            priority: 0,
            health_state: "unknown".into(),
            needs_calibration: 1,
        })
    }
}

// ── PUT /api/models/{name} ──

/// typed 部分更新。`api_key` 为 `****` 开头的掩码哨兵时表示「保持原值」。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ModelPatch {
    pub kind: Option<String>,
    pub compat: Option<String>,
    pub provider: Option<String>,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    pub litellm_model: Option<String>,
    pub task_type: Option<String>,
    pub input_cost_per_token: Option<f64>,
    pub output_cost_per_token: Option<f64>,
    pub rpm: Option<i64>,
    pub is_active: Option<i64>,
    pub capability_tier: Option<i64>,
    pub quality_score: Option<f64>,
    pub context_window: Option<i64>,
    pub supports_tools: Option<i64>,
    pub supports_vision: Option<i64>,
    pub supports_stream: Option<i64>,
    pub priority: Option<i64>,
}

/// 掩码哨兵：前端把未修改的掩码值原样传回，此处不当作新值。
fn is_mask_sentinel(k: &str) -> bool {
    k.starts_with("****")
}

impl ModelPatch {
    /// 应用到现有模型并整体校验，返回更新后的领域模型。
    pub fn resolve_against(&self, existing: &Model) -> Result<Model> {
        let mut m = existing.clone();
        if let Some(kind) = self.kind.as_deref() {
            match kind {
                "local" => {
                    if let Some(k) =
                        self.api_key.as_deref().filter(|s| !s.is_empty() && !is_mask_sentinel(s))
                    {
                        let _ = k;
                        return Err(AppError::InvalidRequest("本地模型不配置 API Key".into()));
                    }
                    let compat = self
                        .compat
                        .as_deref()
                        .map(LocalCompat::parse)
                        .unwrap_or(match &m.backend {
                            Backend::Local { compat, .. } => compat.clone(),
                            _ => LocalCompat::Ollama,
                        });
                    let api_base = match self.api_base.as_deref().filter(|s| !s.is_empty()) {
                        Some(b) => b.to_string(),
                        None => default_local_base(&compat).to_string(),
                    };
                    m.backend = Backend::Local { compat, api_base };
                }
                "cloud" => {
                    let provider = match self.provider.as_deref() {
                        Some(p) => Provider::parse(p),
                        None => match &m.backend {
                            Backend::Cloud { provider, .. } => provider.clone(),
                            _ => Provider::Custom("custom".into()),
                        },
                    };
                    let api_base = match self.api_base.as_deref() {
                        Some(b) if !b.is_empty() => Some(b.to_string()),
                        Some(_) => None, // 显式清空
                        None => Some(m.api_base()).filter(|s| !s.is_empty()).map(String::from),
                    };
                    let api_key = match self.api_key.as_deref() {
                        Some(k) if is_mask_sentinel(k) => ApiKeyRef::parse(m.api_key_env()),
                        Some(k) if !k.is_empty() => ApiKeyRef::parse(k),
                        Some(_) => None, // 显式清空
                        None => ApiKeyRef::parse(m.api_key_env()),
                    };
                    m.backend = Backend::Cloud { provider, api_base, api_key };
                }
                other => {
                    return Err(AppError::InvalidRequest(format!(
                        "kind 必须是 local|cloud，收到 '{other}'"
                    )))
                }
            }
        } else {
            match &mut m.backend {
                Backend::Local { compat, api_base } => {
                    if let Some(c) = self.compat.as_deref() {
                        *compat = LocalCompat::parse(c);
                    }
                    if let Some(b) = self.api_base.as_deref().filter(|s| !s.is_empty()) {
                        *api_base = b.to_string();
                    }
                    if let Some(k) =
                        self.api_key.as_deref().filter(|s| !s.is_empty() && !is_mask_sentinel(s))
                    {
                        let _ = k;
                        return Err(AppError::InvalidRequest("本地模型不配置 API Key".into()));
                    }
                }
                Backend::Cloud { provider, api_base, api_key } => {
                    if let Some(p) = self.provider.as_deref() {
                        *provider = Provider::parse(p);
                    }
                    if let Some(b) = self.api_base.as_deref() {
                        *api_base = if b.is_empty() { None } else { Some(b.to_string()) };
                    }
                    if let Some(k) = self.api_key.as_deref() {
                        if !is_mask_sentinel(k) {
                            *api_key = ApiKeyRef::parse(k);
                        }
                    }
                }
            }
        }
        if let Some(lm) = self.litellm_model.as_deref().filter(|s| !s.is_empty()) {
            let prefix = m.backend.litellm_prefix();
            if !lm.starts_with(&format!("{prefix}/")) {
                return Err(AppError::InvalidRequest(format!(
                    "litellm_model '{lm}' 前缀应为 '{prefix}/'"
                )));
            }
            m.litellm_model = lm.to_string();
        }
        if let Some(v) = &self.task_type {
            m.task_type = v.clone();
        }
        if let Some(v) = self.input_cost_per_token {
            m.input_cost_per_token = v;
        }
        if let Some(v) = self.output_cost_per_token {
            m.output_cost_per_token = v;
        }
        if let Some(v) = self.rpm {
            m.rpm = v;
        }
        if let Some(v) = self.is_active {
            m.is_active = v;
        }
        if let Some(v) = self.capability_tier {
            m.capability_tier = v;
        }
        if let Some(v) = self.quality_score {
            m.quality_score = v.clamp(0.0, 1.0);
        }
        if let Some(v) = self.context_window {
            m.context_window = v;
        }
        if let Some(v) = self.supports_tools {
            m.supports_tools = v;
        }
        if let Some(v) = self.supports_vision {
            m.supports_vision = v;
        }
        if let Some(v) = self.supports_stream {
            m.supports_stream = v;
        }
        if let Some(v) = self.priority {
            m.priority = v;
        }
        Ok(m)
    }

    /// 前后行差异 → `db::update_model` 的列更新映射（白名单校验仍在 db 层）。
    pub fn diff_updates(before: &Model, after: &Model) -> serde_json::Map<String, Value> {
        let rb = crate::db::ModelRow::from(before);
        let ra = crate::db::ModelRow::from(after);
        let mut map = serde_json::Map::new();
        let mut put = |k: &str, changed: bool, v: Value| {
            if changed {
                map.insert(k.to_string(), v);
            }
        };
        put("provider", rb.provider != ra.provider, json!(ra.provider));
        put(
            "litellm_model",
            rb.litellm_model != ra.litellm_model,
            json!(ra.litellm_model),
        );
        put("api_base", rb.api_base != ra.api_base, json!(ra.api_base));
        put(
            "api_key_env",
            rb.api_key_env != ra.api_key_env,
            json!(ra.api_key_env),
        );
        put("task_type", rb.task_type != ra.task_type, json!(ra.task_type));
        put(
            "input_cost_per_token",
            rb.input_cost_per_token != ra.input_cost_per_token,
            json!(ra.input_cost_per_token),
        );
        put(
            "output_cost_per_token",
            rb.output_cost_per_token != ra.output_cost_per_token,
            json!(ra.output_cost_per_token),
        );
        put("rpm", rb.rpm != ra.rpm, json!(ra.rpm));
        put("is_active", rb.is_active != ra.is_active, json!(ra.is_active));
        put(
            "capability_tier",
            rb.capability_tier != ra.capability_tier,
            json!(ra.capability_tier),
        );
        put(
            "quality_score",
            rb.quality_score != ra.quality_score,
            json!(ra.quality_score),
        );
        put(
            "context_window",
            rb.context_window != ra.context_window,
            json!(ra.context_window),
        );
        put(
            "supports_tools",
            rb.supports_tools != ra.supports_tools,
            json!(ra.supports_tools),
        );
        put(
            "supports_vision",
            rb.supports_vision != ra.supports_vision,
            json!(ra.supports_vision),
        );
        put(
            "supports_stream",
            rb.supports_stream != ra.supports_stream,
            json!(ra.supports_stream),
        );
        put("is_local", rb.is_local != ra.is_local, json!(ra.is_local));
        put("priority", rb.priority != ra.priority, json!(ra.priority));
        map
    }
}

// ── GET 响应 ──

#[derive(Debug, Clone, Serialize)]
pub struct ModelDto {
    pub id: i64,
    pub name: String,
    /// "local" | "cloud"
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compat: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub litellm_model: String,
    pub api_base: String,
    /// 掩码输出（`****tail`）；未配置时为空串
    pub api_key: String,
    pub task_type: String,
    pub input_cost_per_token: f64,
    pub output_cost_per_token: f64,
    pub rpm: i64,
    pub is_active: i64,
    pub capability_tier: i64,
    pub quality_score: f64,
    pub context_window: i64,
    pub supports_tools: i64,
    pub supports_vision: i64,
    pub supports_stream: i64,
    pub is_local: bool,
    pub priority: i64,
    pub health_state: String,
    pub needs_calibration: i64,
}

impl From<&Model> for ModelDto {
    fn from(m: &Model) -> Self {
        let (kind, compat, provider) = match &m.backend {
            Backend::Cloud { provider, .. } => {
                ("cloud", None, Some(provider.as_str().to_string()))
            }
            Backend::Local { compat, .. } => {
                ("local", Some(compat.as_str().to_string()), None)
            }
        };
        let raw_key = m.api_key_env();
        ModelDto {
            id: m.id,
            name: m.name.clone(),
            kind: kind.to_string(),
            compat,
            provider,
            litellm_model: m.litellm_model.clone(),
            api_base: m.api_base().to_string(),
            api_key: if raw_key.is_empty() { String::new() } else { mask_secret(raw_key) },
            task_type: m.task_type.clone(),
            input_cost_per_token: m.input_cost_per_token,
            output_cost_per_token: m.output_cost_per_token,
            rpm: m.rpm,
            is_active: m.is_active,
            capability_tier: m.capability_tier,
            quality_score: m.quality_score,
            context_window: m.context_window,
            supports_tools: m.supports_tools,
            supports_vision: m.supports_vision,
            supports_stream: m.supports_stream,
            is_local: m.is_local(),
            priority: m.priority,
            health_state: m.health_state.clone(),
            needs_calibration: m.needs_calibration,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_local_fills_defaults_and_rejects_key() {
        let c: ModelCreate = serde_json::from_str(
            r#"{"name":"qwen3:8b","kind":"local"}"#,
        )
        .unwrap();
        let m = Model::try_from(c).unwrap();
        assert!(m.is_local());
        assert_eq!(m.api_base(), "http://localhost:11434");
        assert_eq!(m.litellm_model, "ollama/qwen3:8b");
        assert_eq!(m.api_key_env(), "");

        let bad: ModelCreate = serde_json::from_str(
            r#"{"name":"x","kind":"local","api_key":"DASHSCOPE_API_KEY"}"#,
        )
        .unwrap();
        assert!(Model::try_from(bad).is_err(), "本地模型带 key 必须拒收");
    }

    #[test]
    fn create_cloud_requires_provider_and_prefix() {
        let c: ModelCreate =
            serde_json::from_str(r#"{"name":"qwen-plus","kind":"cloud"}"#).unwrap();
        assert!(Model::try_from(c).is_err(), "云端缺 provider 必须拒收");

        let c: ModelCreate = serde_json::from_str(
            r#"{"name":"qwen-plus","kind":"cloud","provider":"dashscope","api_key":"DASHSCOPE_API_KEY"}"#,
        )
        .unwrap();
        let m = Model::try_from(c).unwrap();
        assert!(!m.is_local());
        assert_eq!(m.litellm_model, "dashscope/qwen-plus");
        assert_eq!(m.api_key_env(), "DASHSCOPE_API_KEY");

        let c: ModelCreate = serde_json::from_str(
            r#"{"name":"m1","kind":"cloud","provider":"dashscope","litellm_model":"openai/m1"}"#,
        )
        .unwrap();
        assert!(Model::try_from(c).is_err(), "错 litellm 前缀必须拒收");
    }

    #[test]
    fn local_openai_compat_defaults_and_roundtrip_kind() {
        let c: ModelCreate = serde_json::from_str(
            r#"{"name":"qwen2.5-7b","kind":"local","compat":"openai"}"#,
        )
        .unwrap();
        let m = Model::try_from(c).unwrap();
        assert_eq!(m.api_base(), "http://localhost:1234/v1");
        assert_eq!(m.litellm_model, "openai/qwen2.5-7b");
        assert_eq!(m.provider_name(), "custom");
    }

    #[test]
    fn patch_kind_switch_clears_key_and_zeroes_base() {
        let mut cloud = Model::fixture("qwen-plus", "dashscope");
        if let Backend::Cloud { api_key, .. } = &mut cloud.backend {
            *api_key = Some(ApiKeyRef::parse("DASHSCOPE_API_KEY").unwrap());
        }
        let patch: ModelPatch =
            serde_json::from_str(r#"{"kind":"local"}"#).unwrap();
        let after = patch.resolve_against(&cloud).unwrap();
        assert!(after.is_local());
        assert_eq!(after.api_key_env(), "");
        assert_eq!(after.api_base(), "http://localhost:11434");

        let updates = ModelPatch::diff_updates(&cloud, &after);
        assert_eq!(updates["is_local"], json!(1));
        assert_eq!(updates["api_key_env"], json!(""));
        assert_eq!(updates["provider"], json!("ollama"));
    }

    #[test]
    fn patch_mask_sentinel_keeps_existing_key() {
        let mut cloud = Model::fixture("qwen-plus", "dashscope");
        if let Backend::Cloud { api_key, .. } = &mut cloud.backend {
            *api_key = Some(ApiKeyRef::parse("DASHSCOPE_API_KEY").unwrap());
        }
        let patch: ModelPatch = serde_json::from_str(
            r#"{"api_key":"****KEY_","input_cost_per_token":1e-6}"#,
        )
        .unwrap();
        let after = patch.resolve_against(&cloud).unwrap();
        assert_eq!(after.api_key_env(), "DASHSCOPE_API_KEY", "掩码哨兵保持原 key");
        assert_eq!(after.input_cost_per_token, 1e-6);
    }

    #[test]
    fn patch_local_with_key_is_rejected() {
        let local = Model::local_fixture("qwen3:8b", LocalCompat::Ollama);
        let patch: ModelPatch =
            serde_json::from_str(r#"{"api_key":"sk-abc123"}"#).unwrap();
        assert!(patch.resolve_against(&local).is_err());
    }

    #[test]
    fn dto_masks_key() {
        let mut cloud = Model::fixture("qwen-plus", "dashscope");
        if let Backend::Cloud { api_key, .. } = &mut cloud.backend {
            *api_key = Some(ApiKeyRef::parse("sk-abcdefgh1234").unwrap());
        }
        let dto = ModelDto::from(&cloud);
        assert_eq!(dto.api_key, "****1234");
        assert_eq!(dto.kind, "cloud");
        assert_eq!(dto.provider.as_deref(), Some("dashscope"));

        let local = Model::local_fixture("qwen3:8b", LocalCompat::Ollama);
        let dto = ModelDto::from(&local);
        assert_eq!(dto.kind, "local");
        assert_eq!(dto.compat.as_deref(), Some("ollama"));
        assert!(dto.provider.is_none());
        assert_eq!(dto.api_key, "");
    }
}
