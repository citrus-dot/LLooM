//! Native provider adapters. LiteLLM remains a compatibility fallback while
//! these adapters replace its transport role protocol by protocol.

pub mod anthropic;
pub mod openai_compat;

use crate::ai_client::{ChatResult, ModelSpec};
use crate::error::Result;
use serde_json::Value;

pub use openai_compat::{ProviderStream, StreamEvent};

pub fn supports_native(spec: &ModelSpec) -> bool {
    matches!(
        spec.provider.as_str(),
        "openai" | "dashscope" | "ollama" | "custom" | "anthropic"
    ) && !spec.api_base.trim().is_empty()
}

pub async fn chat(
    spec: &ModelSpec,
    messages: &[Value],
    max_tokens: i64,
    temperature: f64,
) -> Result<ChatResult> {
    match spec.provider.as_str() {
        "openai" | "dashscope" | "ollama" | "custom" => {
            openai_compat::chat(spec, messages, max_tokens, temperature).await
        }
        "anthropic" => anthropic::chat(spec, messages, max_tokens, temperature).await,
        provider => Err(crate::error::AppError::AiService(format!(
            "native provider adapter unavailable: {provider}"
        ))),
    }
}

pub async fn stream(
    spec: &ModelSpec,
    messages: &[Value],
    max_tokens: i64,
    temperature: f64,
) -> Result<ProviderStream> {
    match spec.provider.as_str() {
        "openai" | "dashscope" | "ollama" | "custom" => {
            openai_compat::stream(spec, messages, max_tokens, temperature).await
        }
        "anthropic" => anthropic::stream(spec, messages, max_tokens, temperature).await,
        provider => Err(crate::error::AppError::AiService(format!(
            "native provider stream adapter unavailable: {provider}"
        ))),
    }
}
