//! LLooM core library — modular Rust backend.
//!
//! Module layout (Rust best practices: single responsibility, strong typing,
//! centralized error handling):
//!   - `config`: paths, ports, env
//!   - `db`: SQLite layer
//!   - `ai_client`: Python AI micro-service HTTP client
//!   - `security`: regex security (PII / jailbreak / domain)
//!   - `router`: task classification + model selection
//!   - `processes`: child-process management
//!   - `conversations`: conversation file CRUD
//!   - `server`: axum HTTP server (REST + SSE + RPC bridge)

pub mod ai_client;
pub mod config;
pub mod conversations;
pub mod db;
pub mod error;
pub mod health;
pub mod metadata;
pub mod models;
pub mod pricing;
pub mod probe;
pub mod processes;
pub mod router;
pub mod security;
pub mod server;
pub mod signals;
