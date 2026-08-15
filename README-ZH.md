# LLooM — 智能大模型路由平台

[English](README.md) | **中文**

一个自包含的 LLM 路由平台。Rust 核心服务器负责模型管理、按任务类型路由、Token 用量与成本追踪、安全过滤；仅剩一个薄薄的 Python 服务，用于 Rust 无法替代的 LLM 调用。

## 架构

LLooM 分层设计，**REST API 是 UI 与业务核心之间的唯一契约**。四个前端 —— WebUI、CLI、TUI、以及 headless REST API 本身 —— 都接入同一个核心。

```
UI 层（WebUI / CLI / TUI）               ← 任意前端，与业务无关
        │  HTTP REST（全部类型化 JSON 对象）
Rust 核心 + axum REST 服务器（:7861）    ← 主服务器，承载全部业务逻辑
        │  直接函数调用
Rust 核心模块（db / router / security / processes / conversations）
        │  异步 HTTP
Python AI 微服务（:7862）                ← 无状态 litellm 封装
        │
LLM 提供商（DashScope / Ollama / OpenAI / Anthropic）
```

要点：
- **Rust axum 服务器**（`:7861`）是主服务器，承载 SQLite、任务路由、安全过滤、进程管理，并内置 WebUI
- **Python 瘦身为无状态 AI 微服务**（`:7862`），仅封装 litellm —— 这是 Rust 无法替代的部分（100+ 提供商覆盖）
- **所有前端拿到的是类型化 JSON 对象，绝不套字符串** —— 任何前端都无需手动解析
- **诚实的状态报告**：`GET /api/services/status` 反映真实状态（子进程存活 + 端口响应 + AI 就绪），能区分 Down / 端口冲突 / 未配置模型 —— 绝不伪装 healthy

详见 [ARCHITECTURE.md](ARCHITECTURE.md)（分层详解、REST API 参考、端口、数据流）。

## 功能特性

### 模型管理
- 注册云端模型（通义千问/DashScope、OpenAI、Anthropic）和本地模型（Ollama）
- 实时追踪每个模型的 Token 用量和成本
- 设置预算及可配置周期（日/周/月）
- 基于注册的定价自动计算成本

### 智能路由
- **两层分类**：正则规则（零成本）优先，LLM 兜底其次
- **回退链**：5 级故障转移（qwen3-max → plus → qwen-plus → flash → 本地）
- **推理模型支持**：自动为推理模型启用流式输出
- **领域增强**：STEM → 数学逻辑，计算机/工程 → 编程
- **成本感知选择**：挑选能处理任务的最低成本模型

### 任务编排
- **复杂度检测**：6 条正则规则 + 长度/句子数启发式
- **任务分解**：基于 LLM 的子任务拆分及依赖追踪
- **顺序执行**：子任务按序执行并注入上下文
- **结果聚合**：LLM 将子任务输出综合为连贯回答
- **SSE 流式**：实时事件流（分解 → 任务开始 → 任务完成 → 结果）

### 安全层
- **PII 检测**（7 类）：邮箱、电话、身份证号、信用卡、IP、身份证、银行账号
- **越狱拦截**（5 类）：DAN、指令覆盖、角色操纵、安全绕过、提示注入
- **领域分类**：14 个 MMLU 类别，关键词预过滤 + LLM 兜底

### 语义缓存
- ChromaDB 向量相似度搜索（余弦相似度 0.95，24 小时 TTL）
- 对重复的简单问答返回缓存响应（零成本）
- 缓存命中会被标记（`cache_hit`）并在各界面显示"来自缓存"，因此服务 down 时仍能回复也一目了然
- 嵌入模型不可用时优雅降级

### 界面
- **WebUI** — 浏览器访问 `http://localhost:7861/`（服务状态、聊天、模型、用量、设置）
- **CLI** — `lloom-cli`，脚本与快速操作
- **TUI** — OpenTUI + SolidJS 终端仪表盘（`tui/`）
- **诚实的服务管理** — 启动/停止/重启 Ollama 和 AI 服务，真实状态报告（WebUI 按钮、TUI 右键菜单、CLI 命令），并可查看各服务日志

## 快速开始

### 方式 A：下载应用

1. 从 [GitHub Releases](https://github.com/citrus-dot/LLooM/releases) 下载最新版本
2. 启动（或安装 `.deb`/`.rpm` 包）
3. 在设置 → API 密钥中配置密钥
4. 开始聊天

### 方式 B：开发模式

```bash
git clone -b v2 https://github.com/citrus-dot/LLooM.git
cd LLooM

# 安装 Python 依赖（Python AI 微服务）
pip install -e ".[dev]"

# 复制并编辑环境配置
cp .env.example .env
# 在 .env 中填入你的 API 密钥

# 启动 Rust 服务器（WebUI 在 :7861）
cargo run -p lloom-server
```

Rust 服务器（`:7861`）是唯一入口，会自动拉起 Python AI 微服务（`:7862`）和 Ollama（`:11434`）。

### 方式 C：构建发布包

```bash
# 完整构建（Rust release + AI 微服务 PyInstaller + Ollama）
bash scripts/build.sh

# 或分步：
bash scripts/build.sh --skip-ai       # 跳过 AI 微服务打包
bash scripts/build.sh --skip-ollama   # 跳过 Ollama 下载
```

构建产物：
- `dist/ai-service/ai-service` — 独立 AI 微服务可执行（约 26MB，封装 litellm）
- `target/release/lloom-server` — 主服务器（REST + WebUI）
- `target/release/lloom-cli` — 命令行界面
- `dist/ollama/ollama` — 内置 Ollama 二进制

TUI 是独立的 Node/SolidJS 应用（`tui/`，见下文），不属于 Rust 构建。

Rust 二进制是主体；AI 微服务以独立可执行打进应用 resources，目标机器无需安装 Python。

### 冒烟测试

```bash
bash scripts/smoke_test.sh
```

覆盖 19 项检查：健康检查、服务状态、AI 自检、模型注册、聊天、编排、用量、对话 CRUD、预算、服务重启。

## 配置

所有配置通过 `.env` 环境变量文件完成：

| 键 | 默认值 | 说明 |
|-----|---------|------|
| `DASHSCOPE_API_KEY` | （空） | 阿里云百炼 DashScope API 密钥 |
| `DASHSCOPE_API_BASE` | `https://dashscope.aliyuncs.com/compatible-mode/v1` | DashScope 端点 |
| `OPENAI_API_KEY` | （空） | OpenAI API 密钥 |
| `OPENAI_BASE_URL` | （空） | OpenAI 基础 URL 覆盖 |
| `ANTHROPIC_API_KEY` | （空） | Anthropic API 密钥 |
| `LLOOM_WEB_PORT` | `7861` | Rust 服务器 + WebUI 端口 |
| `LLOOM_AI_SERVICE_URL` | `http://localhost:7862` | Python AI 微服务 URL |
| `LLOOM_DATA_DIR` | `./data` | 数据目录（SQLite、对话） |
| `OLLAMA_API_BASE` | `http://localhost:11434` | Ollama 端点 |

## REST API

| 方法 | 路径 | 说明 |
|--------|------|------|
| GET | `/api/health` | 健康检查 |
| GET | `/api/models` | 列出所有模型 |
| POST | `/api/models` | 注册新模型 |
| GET/PUT/DELETE | `/api/models/{name}` | 查询/更新/删除模型 |
| GET | `/api/usage` | 用量统计 |
| GET | `/api/budgets` | 列出预算 |
| POST | `/api/budgets` | 创建/更新预算 |
| GET | `/api/budgets/check` | 检查预算状态 |
| GET/POST | `/api/config` | 读写 .env 配置 |
| GET | `/api/stats` | 仪表盘统计 |
| POST | `/api/chat/stream` | 聊天（SSE 流式） |
| POST | `/api/orchestrate/stream` | 任务编排（SSE 流式） |
| GET/POST/DELETE | `/api/conversations` | 对话 CRUD |
| GET | `/api/services/status` | 诚实的服务状态 |
| POST | `/api/services/{name}/start` | 启动服务（ollama/ai） |
| POST | `/api/services/{name}/stop` | 停止服务 |
| POST | `/api/services/{name}/restart` | 重启服务 |
| GET | `/api/services/{name}/logs` | 服务日志 |
| POST | `/api/services/smart-restart` | 配置变更后重启 AI 服务 |
| POST | `/api/system/open-folder` | 打开目录 |
| POST | `/api/system/open-web` | 打开网页 |
| POST | `/api/system/cli` | 运行 CLI |

## 技术栈

| 组件 | 技术 | 用途 |
|-----------|-----------|---------|
| API 服务器 | **Rust + axum 0.8** | 主 REST + SSE 服务器，全部业务逻辑 |
| 异步运行时 | tokio | 事件循环、异步 HTTP |
| 数据库 | SQLite（WAL 模式，rusqlite） | 模型注册、用量追踪、预算 |
| LLM API | litellm SDK（Python） | 所有 LLM 供应商的统一接口 |
| AI 微服务 | FastAPI + Uvicorn | litellm 的无状态封装 |
| 向量缓存 | ChromaDB（PersistentClient） | 问答语义缓存 |
| HTTP 客户端 | reqwest 0.13 | 异步调用 AI 服务 / 健康探测 |
| 正则 | fancy-regex 0.19 | PII/越狱/领域模式（支持 lookaround） |
| CLI | clap | 命令行界面（lloom-cli） |
| TUI | OpenTUI + SolidJS（bun） | 终端仪表盘（tui/） |
| 本地 LLM | Ollama | 零成本兜底模型运行时 |

## CLI 与 TUI

LLooM 附带命令行界面和终端界面，两者都直接链接 `lloom-core`（本地操作离线可用，无需运行中的服务器）。

### CLI（`lloom-cli`）

```bash
# 构建
cargo build -p lloom-cli
# 或直接用 target/debug/lloom-cli

# 初始化数据库
lloom-cli init

# 模型
lloom-cli models list
lloom-cli models add qwen2.5-local --provider ollama --model ollama/qwen2.5:latest \
  --api-base http://localhost:11434 --input-cost 0.000001 --output-cost 0.000002
lloom-cli models remove <名称>

# 预算
lloom-cli budgets set user default 10 --duration 30d
lloom-cli budgets list
lloom-cli budgets check user default

# 用量与状态
lloom-cli usage
lloom-cli status

# 服务管理
lloom-cli service status
lloom-cli service start ollama
lloom-cli service stop ollama
lloom-cli service restart ai
lloom-cli service logs ollama

# 聊天（需 AI 服务运行：cargo run -- --headless）
lloom-cli chat "2+2 等于几？"
```

### TUI（`tui/`）

OpenTUI + SolidJS 终端仪表盘（用 bun 运行，通过 REST 连接正在运行的服务器）。

```bash
cd tui
bun install
bun run src/index.tsx
```

五个标签页：**首页**（Logo + 提示词）、**聊天**（会话列表 + 流式聊天）、**模型**（已注册模型）、**用量**（成本）、**设置**（API 密钥 + 服务管理）。`Tab` 切换，`Ctrl+C` 退出。

- `Enter` 发送，`Shift+Enter` 换行
- 右键会话项弹出菜单（打开 / 删除）
- 在设置页右键服务名弹出菜单（日志 / 重启 / 停止 / 启动）
- 右键密钥行弹出编辑弹框

## 项目结构

```
LLooM/
├── Cargo.toml                    # Rust workspace 根
├── crates/lloom-core/            # 业务核心 lib（UI 无关）
│   └── src/                      # server.rs, db.rs, router.rs, security.rs,
│                                 # ai_client.rs, processes.rs, conversations.rs,
│                                 # models.rs, config.rs, error.rs
├── crates/lloom-server/          # 主服务器（REST + WebUI）
├── crates/lloom-cli/             # CLI（clap，链接 lloom-core）
├── tui/                          # TUI（OpenTUI + SolidJS，bun）
│   ├── src/                      # app.tsx, index.tsx, routes/, ui/
│   └── package.json
├── webui/index.html              # WebUI 前端（SPA，独立）
├── api/ai_service.py             # Python AI 微服务（litellm 封装）
├── scripts/
│   ├── build.sh                  # 跨平台构建（含系统依赖检测）
│   ├── download_ollama.sh        # 跨平台 Ollama 下载
│   └── smoke_test.sh             # 19 项冒烟测试
├── ai_service.spec               # PyInstaller spec（AI 微服务）
├── ARCHITECTURE.md               # 分层详解 + REST 参考
├── pyproject.toml                # Python 项目配置（AI 服务）
└── .env.example                  # 环境模板
```

## 许可证

MIT
