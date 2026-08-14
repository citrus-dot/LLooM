# LLooM 架构文档

本文档描述 LLooM 当前的分层架构。核心原则：**REST API 是唯一对外契约，UI 层与业务核心彻底解耦**。任何前端（WebUI、TUI、桌面 GUI）都通过同一套契约接入。

## 分层总览

```
┌─────────────────────────────────────────────────────────┐
│  UI 适配层（任意前端，与业务无关）                       │
│                                                         │
│  ┌───────────┐  ┌───────────┐  ┌───────────────────┐   │
│  │  WebUI    │  │  TUI      │  │  Tauri 桌面 GUI   │   │
│  │ index.html│  │ (未来)    │  │  (窗口+托盘)      │   │
│  └─────┬─────┘  └─────┬─────┘  └────────┬──────────┘   │
│        │  HTTP/REST   │  HTTP/REST      │ Tauri IPC     │
└────────┼──────────────┼────────────────┼───────────────┘
         │              │                │
┌────────▼──────────────▼────────────────▼───────────────┐
│  契约层：axum REST API（唯一对外契约，返回类型化 JSON） │
│  /api/models /api/services /api/chat/stream ...        │
└──────────────────────┬─────────────────────────────────┘
                       │ 直接函数调用
┌──────────────────────▼─────────────────────────────────┐
│  业务核心 core（UI 无关）                              │
│  ┌─────────┬──────────┬─────────┬───────────────┐      │
│  │ db.rs   │ router.rs│security │ ai_client.rs │      │
│  │ SQLite  │ 分类/选择 │ PII/越狱│ AI 微服务客户端│      │
│  └────┬────┴────┬─────┴────┬────┴───────┬───────┘      │
│       │         │          │            │              │
│  ┌────▼────┬────▼─────┬────▼────┬───────▼──────┐      │
│  │processes│convers-  │ models  │  error.rs    │      │
│  │子进程管理│ations CRUD│ 类型定义 │ 统一错误      │      │
│  └─────────┴──────────┴─────────┴──────────────┘      │
└──────────────────┬─────────────────────────────────────┘
                   │ 只调 LLM 的部分（litellm 不可替代）
┌──────────────────▼─────────────────────────────────────┐
│  Python AI 微服务（端口 7862，无状态）                  │
│  ai_service.py — 仅封装 litellm 调用                    │
│  /v1/chat /v1/classify /v1/orchestrate/stream          │
└──────────────────┬─────────────────────────────────────┘
                   │
┌──────────────────▼─────────────────────────────────────┐
│  LLM 提供商（DashScope / Ollama / OpenAI / Anthropic） │
└────────────────────────────────────────────────────────┘
```

## 各层职责

### UI 适配层

| UI | 接入方式 | 说明 |
|---|---|---|
| WebUI | HTTP → `http://localhost:7861/api/*` | 浏览器访问，前端 `restCall()` 映射 REST |
| Tauri GUI | HTTP → `http://localhost:7861/api/*` | GUI 启动时后台起同一 axum 服务器，WebView 前端与 WebUI 完全一致 |
| TUI（未来） | HTTP → `/api/*` 或直连 core | 复用同一契约，零额外工作 |

**关键约定**：
- **REST 是唯一契约**，所有 UI（含桌面 GUI）统一走 `/api/*`。Tauri 壳只提供窗口和系统托盘，没有任何业务命令。
- 所有 UI 拿到的是类型化 JSON **对象**，不是字符串。前端不做任何 `JSON.parse` 包装。
- GUI 模式下 WebView 的页面源是 `tauri://localhost`，因此前端 `restCall` 使用绝对地址 `http://localhost:7861`。

### 契约层（server.rs）

唯一对外暴露的 axum REST 服务器。返回类型化 JSON 对象，支持 SSE 流式。

| 端点 | 方法 | 说明 |
|---|---|---|
| `/api/health` | GET | 健康检查 |
| `/api/models` | GET/POST | 模型列表/注册 |
| `/api/models/:name` | GET/PUT/DELETE | 模型 CRUD（`:name` 为 axum 0.8 动态段） |
| `/api/usage` | GET | 用量统计 |
| `/api/budgets` | GET/POST | 预算列表/设置 |
| `/api/budgets/check` | GET | 预算检查 |
| `/api/config` | GET/POST | 读取/写入 .env |
| `/api/stats` | GET | 仪表盘统计 |
| `/api/conversations` | GET/POST | 对话列表/保存 |
| `/api/conversations/:id` | GET/DELETE | 对话加载/删除 |
| `/api/chat/stream` | POST | 聊天（SSE） |
| `/api/orchestrate/stream` | POST | 任务编排（SSE） |
| `/api/services/status` | GET | 服务健康状态 |
| `/api/services/:name/start` | POST | 启动服务 |
| `/api/services/:name/stop` | POST | 停止服务 |
| `/api/services/:name/restart` | POST | 重启服务 |
| `/api/services/:name/logs` | GET | 服务日志 |
| `/api/services/smart-restart` | POST | 配置变更后智能重启 |
| `/api/system/open-folder` | POST | 打开目录 |
| `/api/system/open-web` | POST | 打开网页 |
| `/api/system/cli` | POST | 运行 CLI |

### 业务核心（core）

- **db.rs** — rusqlite SQLite 层，强类型（`Model`/`Budget`/`UsageStats`）
- **router.rs** — 任务分类（正则层）+ 成本最优模型选择
- **security.rs** — PII 检测 / 越狱拦截 / 领域分类（正则零成本层，fancy-regex 支持 lookaround）
- **ai_client.rs** — Python AI 微服务的 async HTTP 客户端
- **processes.rs** — 子进程管理（API 服务器 / Ollama / AI 服务）
- **conversations.rs** — 对话文件 CRUD（`data/conversations/*.json`）
- **models.rs** — 类型定义
- **error.rs** — thiserror 统一错误 + HTTP 状态映射

### Python AI 微服务（唯一保留的 Python）

`api/ai_service.py` — **无状态**服务，只做一件事：封装 litellm 调用。

- `/v1/chat` — 单次 LLM 调用
- `/v1/chat/stream` — 流式 LLM 调用
- `/v1/classify` — LLM 兜底任务分类
- `/v1/domain` — LLM 兜底领域分类
- `/v1/orchestrate/stream` — 完整编排（分解→执行→聚合）

**为什么保留 Python**：litellm 封装了 100+ LLM 提供商的统一接口，Rust 生态暂无等价物。Rust 通过 `ai_client.rs` 把模型参数（litellm_model/api_base/api_key）显式传入，Python 不碰数据库/配置/业务逻辑。

## 端口分配

| 端口 | 用途 |
|---|---|
| 7860 | （可选）旧版 Python API，兼容阶段保留 |
| 7861 | **Rust axum 主服务器**（REST + WebUI） |
| 7862 | Python AI 微服务 |
| 11434 | Ollama |

## 数据流示例：一次聊天

```
用户输入 → WebUI/窗口 → POST /api/chat/stream
  → Rust: security.rs 检查（PII/越狱）
  → Rust: router.rs 分类（正则优先，LLM 兜底调 /v1/classify）
  → Rust: db.rs 查模型配置 → 构造 ModelSpec
  → Rust: ai_client.rs → Python /v1/chat → litellm → LLM
  → 返回 SSE 流：routing 信息 + 内容分块 + done
  → Rust: db.rs 记录用量/成本
```

## 与旧架构的差异

| | 旧架构 | 当前 |
|---|---|---|
| 主服务器 | Python FastAPI（7860） | **Rust axum（7861）** |
| Rust 角色 | 壳 + curl 代理 | 完整服务器 + 业务核心 |
| Python 角色 | 全部业务 | 仅 litellm 调用（瘦身 ~75%） |
| GUI 前端接入 | Tauri IPC 命令 | **REST（与 WebUI 完全一致）** |
| 前端数据 | JSON 字符串 + 手动 parse | 类型化对象 |
| 通信 | Rust↔Python 双端口转发 | Rust 为主，按需调 AI 服务 |

## 技术栈版本

| crate | 版本 | 用途 |
|---|---|---|
| axum | 0.8 | HTTP 服务器（`{param}` 动态路由） |
| tokio | 1.x | 异步运行时 |
| rusqlite | 0.40 | SQLite（bundled） |
| reqwest | 0.13 | async HTTP 客户端 |
| fancy-regex | 0.19 | lookaround 支持的正则 |
| thiserror | 2.x | 错误类型 |

## 目录结构

```
tauri-app/src-tauri/src/
├── main.rs          # 入口：GUI/headless 分支 + 核心启动 + 系统托盘
├── lib.rs           # 模块声明
├── server.rs        # axum REST 服务器
├── db.rs            # SQLite 层
├── router.rs        # 任务分类 + 模型选择
├── security.rs      # 正则安全层
├── ai_client.rs     # AI 微服务客户端
├── processes.rs     # 子进程管理
├── conversations.rs # 对话 CRUD
├── models.rs        # 类型定义
├── config.rs        # 路径/端口配置
└── error.rs         # 统一错误

api/
└── ai_service.py    # Python AI 微服务（litellm 封装）
```
