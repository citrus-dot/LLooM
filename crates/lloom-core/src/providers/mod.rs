//! Native provider adapters：OpenAI-compatible 与 Anthropic Messages 直连，
//! 完全接替已移除的 Python/LiteLLM 传输层。
//!
//! 协议分发的单一真源是 [`Protocol`]（L-4）：provider 字符串 → 协议的映射只在此处
//! 出现一次，supports_native / chat / stream 都委托它，新增 provider 只需扩一个映射。

pub mod anthropic;
pub mod openai_compat;
mod retry;
mod sse_lines;

use crate::ai_client::{ChatResult, ModelSpec};
use crate::error::Result;
use serde_json::Value;

pub use openai_compat::{ProviderStream, StreamEvent};

/// 传输协议。`custom` 兼容端点按 OpenAI 协议处理（显式约定，非探测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// OpenAI-compatible chat completions（OpenAI/DashScope/DeepSeek/OpenRouter/Ollama/LM Studio/vLLM/custom）
    OpenAiCompat,
    /// Anthropic Messages API
    AnthropicMessages,
}

impl Protocol {
    /// provider 标识（`Model::provider_name()` 的产物）→ 协议。
    /// 未知 provider 返回 None（不支持原生直连）。
    pub fn of(provider: &str) -> Option<Protocol> {
        match provider {
            "openai" | "dashscope" | "deepseek" | "openrouter" | "groq" | "ollama"
            | "lmstudio" | "vllm" | "custom" => Some(Protocol::OpenAiCompat),
            "anthropic" => Some(Protocol::AnthropicMessages),
            _ => None,
        }
    }

    /// 供应商默认 API Base（注册时留空走这里；custom 必须显式配置）。
    /// 这是 WebUI「API Base 可选，留空走供应商默认」承诺的唯一真源。
    pub fn default_base(provider: &str) -> Option<&'static str> {
        match provider {
            "openai" => Some("https://api.openai.com/v1"),
            "dashscope" => Some("https://dashscope.aliyuncs.com/compatible-mode/v1"),
            "anthropic" => Some("https://api.anthropic.com"),
            _ => None,
        }
    }
}

pub fn supports_native(spec: &ModelSpec) -> bool {
    Protocol::of(spec.provider.as_str()).is_some() && !spec.api_base.trim().is_empty()
}

pub async fn chat(
    spec: &ModelSpec,
    messages: &[Value],
    max_tokens: i64,
    temperature: f64,
) -> Result<ChatResult> {
    match Protocol::of(spec.provider.as_str()) {
        Some(Protocol::OpenAiCompat) => {
            openai_compat::chat(spec, messages, max_tokens, temperature).await
        }
        Some(Protocol::AnthropicMessages) => {
            anthropic::chat(spec, messages, max_tokens, temperature).await
        }
        None => Err(crate::error::AppError::AiService(format!(
            "native provider adapter unavailable: {}",
            spec.provider
        ))),
    }
}

pub async fn stream(
    spec: &ModelSpec,
    messages: &[Value],
    max_tokens: i64,
    temperature: f64,
) -> Result<ProviderStream> {
    match Protocol::of(spec.provider.as_str()) {
        Some(Protocol::OpenAiCompat) => {
            openai_compat::stream(spec, messages, max_tokens, temperature).await
        }
        Some(Protocol::AnthropicMessages) => {
            anthropic::stream(spec, messages, max_tokens, temperature).await
        }
        None => Err(crate::error::AppError::AiService(format!(
            "native provider stream adapter unavailable: {}",
            spec.provider
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::Protocol;

    #[test]
    fn protocol_mapping_covers_knowable_providers() {
        for known in [
            "openai",
            "dashscope",
            "deepseek",
            "openrouter",
            "groq",
            "ollama",
            "lmstudio",
            "vllm",
            "custom",
        ] {
            assert_eq!(Protocol::of(known), Some(Protocol::OpenAiCompat), "{known}");
        }
        assert_eq!(Protocol::of("anthropic"), Some(Protocol::AnthropicMessages));
        assert_eq!(Protocol::of("bedrock"), None);
        assert_eq!(Protocol::of(""), None);
    }
}
