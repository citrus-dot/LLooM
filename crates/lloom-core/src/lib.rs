//! LLooM core library — modular Rust backend.
//!
//! Module layout (Rust best practices: single responsibility, strong typing,
//! centralized error handling):
//!   - `config`: paths, ports, env
//!   - `db`: SQLite layer
//!   - `ai_client`: provider 统一调用 + L1/L2 缓存装配 + 编排执行
//!   - `providers`: 原生适配器（OpenAI-compatible / Anthropic Messages）
//!   - `security`: regex security (PII / jailbreak / domain)
//!   - `router`: task classification + model selection
//!   - `processes`: Ollama 子进程管理
//!   - `conversations`: SQLite 对话与滚动摘要
//!   - `server`: axum HTTP server (REST + SSE)

pub mod ai_client;
pub mod config;
pub mod context;
pub mod conversations;
pub mod db;
pub mod error;
pub mod exact_cache;
pub mod health;
pub mod metadata;
pub mod metrics;
pub mod model_dto;
pub mod models;
pub mod openai_compat;
pub mod orchestrator;
pub mod pricing;
pub mod probe;
pub mod processes;
pub mod providers;
pub mod review;
pub mod router;
pub mod security;
pub mod semantic_cache;
pub mod server;
pub mod signals;
