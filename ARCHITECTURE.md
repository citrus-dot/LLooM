# LLooM 架构与 HTTP 契约

> 适用范围：`v2` 当前工作区。已发布安装包以对应 tag 的源码为准。
> 读者：需要理解运行链路、修改服务或接入 API 的开发者。新用户请先看 [README-ZH.md](README-ZH.md)。

## 1. 运行时与分层

```text
WebUI / CLI / TUI / OpenAI-compatible 客户端
                    │ HTTP + SSE
                    ▼
          lloom-server（Rust + axum，:7861）
                    │
        ┌───────────┼────────────┐
        ▼           ▼            ▼
   路由与编排     缓存与安全     SQLite（WAL）
        │           │       模型/对话/价格/API Key/用量
        ▼           ▼
   Provider 适配器（OpenAI-compatible / Anthropic Messages）
                    │ reqwest + SSE
                    ▼
   云端 Provider / Ollama / LM Studio / vLLM
```

运行时只有一个 LLooM 进程；Ollama 是可选的外部模型服务。没有 Python、FastAPI、LiteLLM、ChromaDB、Docker 或外部数据库运行时依赖。SQLite 与 FastEmbed 语义缓存所需文件默认位于 `data/`。

**职责边界：**

- Rust 是路由、编排、预算、计价与存储的唯一决策源。
- REST/SSE 是 WebUI、CLI、TUI 的共同契约；前端不复制业务规则。
- 上游 Provider Key 属于模型配置；下游 `/v1` API Key 属于客户端授权，二者分离。
- 缓存可降级：L2 的模型下载或初始化失败不应阻断 Provider 调用。
- `docs/archive/` 是历史设计材料，不是当前实现的权威来源。

## 2. Workspace 与关键模块

| 路径 | 职责 |
|---|---|
| `crates/lloom-server/src/main.rs` | 进程入口、启动校验、监听、优雅关停 |
| `crates/lloom-core/src/server.rs` | 内部 API、SSE、访问边界、用量与后台任务 |
| `crates/lloom-core/src/router.rs` | 分类、领域增强、门槛、评分、fallback |
| `crates/lloom-core/src/orchestrator.rs` | 分解结果解析、依赖波次 |
| `crates/lloom-core/src/ai_client.rs` | 调用编排、缓存装配、Provider 规格转换 |
| `crates/lloom-core/src/providers/mod.rs` | `Protocol`：provider 到协议与默认 API Base 的单一映射 |
| `crates/lloom-core/src/providers/openai_compat.rs` | OpenAI-compatible 请求、usage 合并、SSE |
| `crates/lloom-core/src/providers/anthropic.rs` | Anthropic Messages 请求、usage 合并、SSE |
| `crates/lloom-core/src/providers/retry.rs` | 可重试错误、退避与 `Retry-After` |
| `crates/lloom-core/src/providers/sse_lines.rs` | 字节级 SSE 行解码 |
| `crates/lloom-core/src/error.rs` | 应用错误与结构化 Provider 错误 |
| `crates/lloom-core/src/context.rs` | 上下文裁剪、token 估算、缓存键 |
| `crates/lloom-core/src/exact_cache.rs` | L1 SQLite 缓存 |
| `crates/lloom-core/src/semantic_cache.rs` | L2 embedding、相似检索与淘汰 |
| `crates/lloom-core/src/conversations.rs` | 对话持久化、滚动摘要、两阶段写入 |
| `crates/lloom-core/src/db.rs` | Schema、幂等列迁移、索引、查询与额度事务 |
| `crates/lloom-core/src/pricing.rs` | 分项、阶梯、缓存与时段计价 |
| `crates/lloom-core/src/openai_compat.rs` | `/v1` 代理、下游 Key、预扣与结算 |
| `crates/lloom-cli/src/main.rs` | Rust CLI |
| `webui/` | React + Vite + Ant Design；`npm run build` 生成静态文件 |
| `tui/` | OpenTUI + SolidJS，使用 Bun 运行 |

`Model` 在领域层保存后端信息（Cloud/Local）与原始 Key 引用；`ModelSpec` 在出进程边界解析环境变量和默认 API Base。API Key 掩码 `****` 表示保持原值，不是可使用的真实 Key。

## 3. 请求链路

普通聊天：

```text
messages 校验
  → 安全检查
  → 分类与领域增强
  → 门槛与成本/质量评分（plan）
  → L1 精确缓存 → L2 语义缓存
  → Provider 重试与调用
  → usage 合并、计价、持久化
  → SSE / JSON 响应
```

流式失败或客户端断开也会尽量保存部分用量，标记 `field_missing`/partial；只有没有产出且调用失败时才按失败更新模型健康。路由 fallback 不等同于同模型重试：瞬时错误先按 Provider 重试策略处理，再决定是否切换候选。

编排请求额外经过：上下文裁剪/滚动摘要 → 分解 → 依赖波次 → 子任务 plan → 聚合。每个子任务有独立路由与容错。客户端直接提交 `history` 时上限 200 条；携带 `conversation_id` 时以服务端会话与摘要为准。

## 4. Provider、重试与错误语义

支持的协议：

- **OpenAI-compatible**：OpenAI、DashScope 及兼容端点。本地 Ollama、LM Studio、vLLM 通过该协议接入。
- **Anthropic Messages**：Anthropic 原生 Messages API。

云端 API Base 可留空：`Protocol::default_base` 为 OpenAI、DashScope、Anthropic 提供默认地址。`custom` 不提供默认地址，需要在模型配置中显式填写，调用时不会凭空猜测服务地址。`Protocol::of` 是协议分发的唯一入口。

重试仅针对连接/超时错误与 HTTP 408、409、429、5xx；最多 2 次，带退避与 jitter，尊重不超过 120 秒的 `Retry-After`。请求体无法克隆或流已开始消费后不重试，避免重复输出。

Provider 错误通过 `ProviderError` 保留类别，网关转发规则：

| 上游情况 | 网关状态 |
|---|---|
| 429 | 429，客户端可按 `Retry-After`/退避重试 |
| 400 等其他请求错误 | 400 |
| 401、403、5xx、连接故障 | 502 |
| 超时 | 504 |

`AppError` 的业务错误使用 400/404/409/500 等状态；普通错误响应是 JSON。OpenAI `/v1` 错误体为 `{"error":{"message","type","code"}}`。

## 5. 缓存与上下文

### L1：精确缓存

- 文件：`data/cache_exact.sqlite3`。
- 键：模型 + system prompt hash + 会话上下文指纹 + 规范化 query + 温度的位表示。
- 指纹覆盖末两条历史消息的完整内容，避免短前缀碰撞。
- 默认 TTL 24h、容量 5000；当前容量淘汰按创建时间（FIFO），不是按命中刷新的 LRU。

### L2：语义缓存

- 文件：`data/cache_semantic.sqlite3`。
- FastEmbed 量化 all-MiniLM-L6-v2，默认模型目录 `data/models/fastembed`。
- 默认阈值 `0.88`（`LLOOM_SEMANTIC_THRESHOLD`）；查询、模型、system hash 与 temperature 共同参与作用域判断，其中 temperature 在 SQL 查询中精确匹配（不是容差匹配）。
- 只对缓存适用的请求启用；高温采样等请求不会无条件写入。
- 当前向量检索为 SQLite 中的线性余弦计算，适合当前缓存规模；大规模升级 HNSW 仍属于按需方向，不是已交付能力。
- Intel Mac 使用运行期 ONNX Runtime；不可用时跳过 L2，不影响 L1 与 Provider 调用。

**缓存边界：**命中缓存不代表 Provider 返回了新答案，也不是实时的语义正确性证明。客户端应保留 `cache_hit` 等元数据用于诊断；探针、影子评测与代理请求不与普通会话共用缓存命名空间。

## 6. HTTP API

所有服务默认共享 `http://127.0.0.1:7861`。内部接口与 OpenAI 兼容接口见下表；请求字段以对应 handler/DTO 为准，不把旧文档中不存在的参数视为契约。

### 6.1 系统、设置与状态

| 方法 | 路径 | 用途 |
|---|---|---|
| GET | `/api/health` | 基础健康检查 |
| GET | `/api/services/status` | Core、Ollama、Native Providers 状态 |
| GET, PUT | `/api/settings/runtime` | 健康、影子采样、安全等运行时设置 |
| GET | `/metrics` | Prometheus 文本指标（本机） |
| POST | `/api/shutdown` | 优雅关停，仅环回 peer |
| POST | `/api/system/open-folder` | 打开本地目录 |
| POST | `/api/system/open-web` | 打开允许的 http/https URL |

### 6.2 模型、对话与聊天

| 方法 | 路径 | 用途 |
|---|---|---|
| GET, POST | `/api/models` | 模型列表/注册 |
| GET, PUT, DELETE | `/api/models/{name}` | 模型详情、更新、停用 |
| POST | `/api/models/{name}/probe` | 主动测活（云模型可能产生少量费用） |
| GET, POST | `/api/conversations` | 会话列表/保存 |
| GET, PUT, DELETE | `/api/conversations/{id}` | 读取、改名、删除 |
| POST | `/api/conversations/{id}/messages` | 追加消息 |
| PATCH | `/api/conversations/{id}/messages/{seq}` | 更新消息与 meta |
| POST | `/api/chat/stream` | 内置聊天 SSE |
| POST | `/api/orchestrate/stream` | 编排 SSE |

内部聊天 JSON 最小示例：

```json
{"messages":[{"role":"user","content":"简要解释快速排序"}]}
```

可选字段：`model`、`conversation_id`、`sr_domain`。编排请求使用 `query`，可带 `conversation_id`、钉选 `model` 或 legacy `history`。

### 6.3 OpenAI-compatible 代理

| 方法 | 路径 | 用途 |
|---|---|---|
| GET | `/v1/models` | 可用模型与 `auto` |
| POST | `/v1/chat/completions` | 普通/流式聊天 |
| GET | `/api/proxy/config` | 接入信息 |
| POST | `/api/proxy/selftest` | 本机代理自测 |
| GET, POST | `/api/api-keys` | 下游 Key 列表/创建 |
| PUT, DELETE | `/api/api-keys/{id}` | 更新/删除下游 Key |

聊天请求支持 `model`、`messages`、`temperature`、`max_tokens`、`stream`。不声明 tools、多模态、response_format 等未实现字段的兼容性。

`/v1` 鉴权规则：

- 尚未创建任何 Key 时，仅环回访问可免鉴权。
- 创建首个 Key 后，请求必须带 `Authorization: Bearer <key>`。
- 非环回 `/v1` 访问必须已有 Key。
- Key 只在创建时显示一次，数据库存哈希与掩码前缀；支持启停、额度、RPM、有效期和模型白名单。
- 额度采用请求前预留、请求后按实际成本结算；后台回收过期 RPM 记录和预留。
- `/api/*` 管理接口与 `/metrics` 仅允许本机访问；环回浏览器请求还需 Host/Origin 校验。
- 非环回绑定且尚无 API Key 时，服务拒绝启动。不要把管理 API 直接暴露到公网。

### 6.4 用量、预算与定价

| 方法 | 路径 | 用途 |
|---|---|---|
| GET | `/api/usage` | 用量统计 |
| GET | `/api/usage/reconcile` | 最近一次账单对账结果 |
| GET, POST, DELETE | `/api/budgets` | 预算管理 |
| GET | `/api/budgets/check` | 预算检查 |
| GET | `/api/pricing/specs` | 价格列表 |
| PUT | `/api/pricing/specs/{provider}/{model}` | 修改价格 |
| POST | `/api/pricing/specs/{provider}/{model}/accept` | 接受为人工锚定价 |
| POST | `/api/pricing/refresh` | 刷新远端价格 |
| GET, POST | `/api/pricing/reference`, `/api/pricing/reference/refresh` | 参考价读取/刷新 |
| GET | `/api/pricing/calibration` | 价格校准数据 |
| GET | `/api/probe/stats` | 探针状态 |
| PUT | `/api/probe/budget` | 探针预算设置 |
| GET | `/api/stats` | 概览统计 |

### 6.5 路由评估

| 方法 | 路径 | 用途 |
|---|---|---|
| GET, POST | `/api/routing/shadow` | 影子评测状态/采集 |
| POST | `/api/routing/plan-subtask` | 子任务路由计划 |
| GET | `/api/routing/overhead` | 路由开销报告 |
| GET | `/api/routing/review` | 最新路由体检 |
| POST | `/api/routing/review/refresh` | 手动生成体检 |
| POST | `/api/routing/review/adopt` | 人工采纳权重建议 |

## 7. SSE 契约

`docs/SSE-CONTRACT.md` 是字段与解析要求的权威契约。核心约定：

- HTTP chunk 不是 SSE frame，也不是 UTF-8 字符边界；客户端必须保留未完成的字节/行。
- `data:` 帧可能没有 `event:`；按 SSE 规范，默认事件类型是 `message`，不能直接丢弃。
- 多条 `data:` 行以换行拼接；`:` 开头是注释/心跳。
- `/api/chat/stream` 使用未命名帧：`routing`、`chunk`、`done` 或 `error`。`done.content` 是完整回答，不是增量。
- `/api/orchestrate/stream` 使用命名事件；可能混有未命名错误帧。
- 客户端应先检查 HTTP 状态，处理 `error` 帧，且不能把缺少预期终帧的流当作成功空回答。

## 8. 存储、配置与端口

默认 `data/` 内容：`lloom.db`（模型、对话、价格、路由、Key、用量）、`cache_exact.sqlite3`、`cache_semantic.sqlite3`、`models/fastembed`。这是用户数据目录，升级安装包时保留；不要将它纳入提交或公开分享。

常用配置：

- `LLOOM_WEB_PORT`：默认 `7861`。
- `LLOOM_BIND`：默认 `127.0.0.1`。
- `LLOOM_DATA_DIR`：默认 `./data`。
- `OLLAMA_API_BASE`：默认 `http://localhost:11434`。
- `LLOOM_SERVER_URL`：CLI/TUI 的服务地址。
- `LLOOM_LOG`：`error|warn|info`。
- Provider Key、缓存阈值与容量等见 `.env.example`。

**配置现状注意：**`/v1` 的下游 Bearer token 由 WebUI 的 API Keys 页创建与维护，`.env.example` 不提供独立的代理 Token 配置项。`/api/health` 的 `version` 字段当前固定为 `2.0.0`，版本判断应使用 `/metrics` 的 build_info 或构建时的 `git describe`，不能依赖该字段。

端口只有服务 `7861` 与可选 Ollama `11434`；历史上的 `7860/7862` 不是当前端口。

## 9. 构建、测试与发布

开发及质量门见 [CONTRIBUTING.md](CONTRIBUTING.md)。核心命令：

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm --prefix webui ci
npm --prefix webui run build
npm --prefix webui test
cd tui && npx tsc --noEmit
```

发布由 tag `v*` 触发 `.github/workflows/release.yml`，构建四个平台的便携包并上传 GitHub Release；产物名来自 `git describe`。发布包包含 Rust 二进制、WebUI 静态文件与启动脚本，不包含真实 `data/` 或 `.env`。

## 10. 已知边界

- 当前是单进程、SQLite、本地优先的部署模型；不是多租户高可用网关。
- 不提供 Gemini/Bedrock/Azure 专用协议、通用 tools/function calling 或完整多模态兼容层；这些在 [LLooMprogress.md](LLooMprogress.md#后续方向) 中按需推进。
- 语义缓存可能误命中相似但不等价的问题；system/温度隔离不能替代业务正确性验证。
- PII 与越狱检测是应用层辅助防线，不等同企业 DLP、完整安全审计或数据库静态加密。
- 生产对外代理仍需自行处理 TLS、反向代理、访问控制、备份、日志保留与供应商合规。默认管理面不提供公网访问能力。
