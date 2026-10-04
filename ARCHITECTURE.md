# LLooM 当前架构

> 更新时间：2026-10-04。历史 Python/LiteLLM 设计已归档到 `docs/archive/`，不代表当前实现。

## 总览

```text
WebUI / CLI / TUI
        │ HTTP REST / SSE
        ▼
lloom-server（Rust + axum，:7861）
        ├─ 路由与健康：router / signals / health
        ├─ 编排与上下文：orchestrator / context / conversations
        ├─ 计价与预算：pricing / probe / db
        ├─ 安全：security
        ├─ 缓存：ExactCache + FastEmbed SemanticCache
        └─ Provider：OpenAI-compatible / Anthropic Messages
                │ 原生 async HTTP
                ▼
OpenAI / DashScope / DeepSeek / OpenRouter / Ollama / LM Studio / vLLM / Anthropic
```

运行时只有一个 LLooM Rust 进程；Ollama 是可选本地模型服务。没有 Python、FastAPI、LiteLLM、ChromaDB 或 PyInstaller 运行时。

## 核心原则

1. **REST 是唯一 UI 契约**：WebUI、CLI 和 TUI 都连接 `:7861`。
2. **Rust 是单一决策真源**：分类、模型选择、fallback、编排和成本全部在 Rust。
3. **密钥按用途隔离**：上游 Provider Key 属于模型；下游访问 Key 属于 `/v1/*` API Key 管理。
4. **缓存可降级**：L1 精确缓存优先，L2 语义缓存失败时直接访问 Provider。
5. **本地优先**：SQLite、内置向量存储，无 Docker 和外部数据库。

## 模块职责

| 模块 | 职责 |
|---|---|
| `server.rs` | axum 路由、SSE、用量落库、服务状态 |
| `db.rs` | SQLite schema、模型/预算/API Key/用量/路由数据 |
| `router.rs` | 规则 + LLM 分类、能力门槛、成本/质量评分、fallback |
| `ai_client.rs` | Provider 统一调用、L1/L2 缓存装配、Rust 编排执行 |
| `providers/openai_compat.rs` | OpenAI-compatible 请求、响应与 SSE |
| `providers/anthropic.rs` | Anthropic Messages 请求、响应与 SSE |
| `context.rs` | token 估算、窗口裁剪、摘要策略、缓存作用域与键 |
| `exact_cache.rs` | SQLite 精确缓存、TTL 与容量淘汰 |
| `semantic_cache.rs` | FastEmbed 量化 MiniLM、SQLite 向量、余弦检索 |
| `orchestrator.rs` | 复杂度判断、任务分解解析、依赖波次 |
| `pricing.rs` | 分项价格、缓存价、时段价、实际/估算成本 |
| `security.rs` | PII、越狱和领域规则 |
| `conversations.rs` | SQLite 对话与滚动摘要 |
| `openai_compat.rs` | 对外 `/v1` 代理、多 API Key 鉴权、额度与 RPM |

## Provider 模型

模型使用显式后端：

- `Cloud { provider, api_base, api_key }`
- `Local { compat, api_base }`

`provider_model` 保存供应商真实模型 ID，例如 `qwen-plus`、`gpt-4o`、`qwen2.5:7b`，不带适配器前缀。

当前协议：

- **OpenAI-compatible**：OpenAI、DashScope、DeepSeek、OpenRouter、Groq、Ollama、LM Studio、vLLM、自定义兼容端点
- **Anthropic Messages**：Anthropic 原生协议

## 两级缓存

### L1 精确缓存

- 文件：`data/cache_exact.sqlite3`
- 键：`model + system_id + context_fingerprint + normalized_query`
- 默认 TTL：24 小时
- 默认容量：5000

### L2 语义缓存

- 文件：`data/cache_semantic.sqlite3`
- 模型：FastEmbed `all-MiniLM-L6-v2` 量化版
- 模型目录：`data/models/fastembed`
- 默认余弦阈值：`0.88`（`LLOOM_SEMANTIC_THRESHOLD`）
- 仅上下文无关请求进入跨会话语义缓存
- 模型下载或 ONNX 初始化失败时自动绕过

## 对外 API Key

WebUI 的 **API Keys** 页面管理 `/v1/*` 访问凭证：

- 明文只在创建时显示
- SQLite 仅保存 SHA-256 与掩码前缀
- 支持启用/禁用、额度、RPM、模型白名单、有效期
- 每次调用更新累计成本和最后使用时间
- 尚未创建任何 Key 时，环回 `/v1/*` 保持免鉴权；创建首个 Key 后强制 Bearer

## 主要端点

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/health` | 核心健康检查 |
| GET | `/api/services/status` | Core / Ollama / Native Providers 状态 |
| GET,POST | `/api/models` | 模型列表与注册 |
| GET,PUT,DELETE | `/api/models/{name}` | 模型 CRUD |
| POST | `/api/chat/stream` | 内部聊天 SSE |
| POST | `/api/orchestrate/stream` | Rust 编排 SSE |
| GET,POST | `/api/conversations` | 对话列表与保存 |
| GET,PUT,DELETE | `/api/conversations/{id}` | 对话读取、改名、删除 |
| GET | `/api/usage` | 用量与成本 |
| GET,POST,DELETE | `/api/budgets` | 预算管理 |
| GET | `/api/pricing/specs` | 价格规格 |
| POST | `/api/pricing/refresh` | 远端价格刷新 |
| GET,POST | `/api/api-keys` | 下游 API Key 列表与创建 |
| PUT,DELETE | `/api/api-keys/{id}` | 更新与删除 Key |
| GET | `/api/proxy/config` | OpenAI 接入信息 |
| POST | `/api/proxy/selftest` | 本机代理自测 |
| POST | `/v1/chat/completions` | OpenAI-compatible 代理 |
| GET | `/v1/models` | OpenAI-compatible 模型列表 |
| GET | `/metrics` | Prometheus 指标 |
| POST | `/api/shutdown` | 优雅关停 |

## 数据流

```text
请求
 → security 检查
 → router 分类与评分
 → L1 精确缓存
 → L2 语义缓存
 → Provider 原生适配器
 → usage 归一化与 pricing 计价
 → SQLite 落库
 → SSE / OpenAI JSON 返回
```

复杂任务额外经过：

```text
context 窗口与摘要
 → decompose
 → dependency waves
 → 每个子任务重新 plan + fallback
 → aggregate
```

## 端口

| 端口 | 用途 |
|---|---|
| `7861` | LLooM REST、OpenAI API 与 WebUI |
| `11434` | 可选 Ollama |

## 构建与质量门

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run build --prefix webui
cd tui && npx tsc --noEmit
```

发布包只包含 Rust 二进制和 WebUI 静态文件。
