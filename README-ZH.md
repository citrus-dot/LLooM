<p align="center">
  <img src="assets/logo.png" width="120" height="120" alt="LLooM Logo" />
</p>

<h1 align="center">LLooM</h1>

<p align="center">
  <strong>自包含的智能 LLM 路由平台</strong> — 一个 Rust 服务器搞定模型路由、成本核算、语义缓存与安全过滤，零 Docker 依赖
</p>

<p align="center">
  <img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT" />
  <img src="https://img.shields.io/badge/Platform-macOS%20%7C%20Linux-blue" alt="Platform" />
  <img src="https://img.shields.io/badge/Rust-axum-CE422B?logo=rust&logoColor=white" alt="Rust" />
  <img src="https://img.shields.io/badge/Python-3.11+-3776AB?logo=python&logoColor=white" alt="Python" />
  <img src="https://img.shields.io/badge/LiteLLM-100%2B%20providers-red" alt="LiteLLM" />
  <img src="https://img.shields.io/badge/SQLite-WAL-003B57?logo=sqlite&logoColor=white" alt="SQLite" />
</p>

<p align="center">
  <a href="README.md">English</a> | <strong>中文</strong>
</p>

---

## 为什么选择 LLooM?

| 痛点 | LLooM 方案 |
|------|-----------|
| 多模型切换繁琐，手动选模型靠感觉 | **智能路由** — 两层分类（正则规则 → LLM 兜底）+ 评分选模，按任务类型自动挑性价比最优的模型 |
| API 账单不可控 | **预算档联动路由** — normal/throttle/tight/protect 四档预算自动降级，protect 强制零成本本地兜底 |
| 价格信息不透明 | **多源定价体系** — manual > overlay > litellm_remote > litellm_packaged > heuristic 优先级链，OpenRouter 参考价交叉核对（偏差 ≥20% 预警） |
| 重复问题反复付费 | **语义缓存** — 向量相似度匹配，命中直接返回缓存，零成本回复，命中率可监控 |
| 复杂任务单模型难以胜任 | **任务编排** — 自动拆解子任务、按子任务分派模型、按序执行并汇总结果 |
| LLM 调用缺乏安全防护 | **安全层** — PII 脱敏（7 类）+ 越狱拦截（5 类）+ MMLU 14 域分类 |
| 服务状态是黑盒，"healthy" 未必健康 | **诚实状态报告** — 子进程存活 + 端口响应 + AI 就绪三重探测，能区分 Down / 端口冲突 / 未配置模型 |
| 传统方案要堆 10 个 Docker 容器 | **单二进制 + SQLite** — 无 Docker、无外部数据库，克隆即跑 |

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
- 基于注册的定价自动计算成本，支持人工改价与远端刷新采纳

### 智能路由
- **两层分类**：正则规则（零成本）优先，LLM 兜底其次
- **评分路由（`plan()`）**：注册表门槛（能力档/上下文/健康/成本上限/钉选）+ 成本质量加权评分；成本走 `pricing.rs est_cost`，质量走 EWMA 冷启动分。已彻底取代全部硬编码模型表
- **钉选软优先**：钉选模型默认作为 +0.3 加分的软优先（仍受健康/预算门槛约束）；设 `LLOOM_PINNED_MODE=hard` 可恢复旧的强制指定行为
- **回退链 + 升档**：5 级故障转移（qwen3-max → plus → qwen-plus → flash → 本地），健康感知自动升档
- **影子评测 + AIQ**：自动采样流量校准成本—质量，可离线重放（`scripts/aiq_replay.py`）
- **健康感知容灾**：滑窗健康状态机、小时级主动探测（月度预算封顶，可调）、按请求回退
- **预算联动**：预算档（normal/throttle/tight/protect）注入路由；tight 复杂任务降档，protect 强制本地/零成本
- **推理模型支持**：自动为推理模型启用流式输出
- **领域增强**：STEM → 数学逻辑，计算机/工程 → 编程

### 定价与成本核算
- **多源优先级链**：manual > overlay > litellm_remote > litellm_packaged > heuristic，来源可在定价页逐一核实
- **OpenRouter 第三方参考价**：与本地图价联表展示输入/输出偏差百分比，≥20% 橙色预警，仅作交叉核对、不覆盖本地价
- **探针校准**：小时级 warm-up + cache-verify 探测，失败 sentinel 不污染用量统计，月度预算可调（`/api/probe/budget`）

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
- 缓存生命周期可通过 `/api/cache/*` 管理（预初始化 / 状态 / 清理 / 反馈 / 阈值自调）

### 界面
- **WebUI** — 浏览器访问 `http://localhost:7861/`（服务状态、聊天、模型、用量、定价、设置）
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
# 推荐 uv（按入库的 uv.lock 冻结安装，构建可复现）：
uv sync --extra dev --extra build
# 受限网络：uv 不读 pip.conf，需显式指定镜像：
#   export UV_DEFAULT_INDEX=https://pypi.tuna.tsinghua.edu.cn/simple
# 无 uv 时回落 pip（仓库自带 pip.conf 清华镜像）：
#   export PIP_CONFIG_FILE="$PWD/pip.conf" && pip install -e ".[dev]"

# 复制并编辑环境配置
cp .env.example .env
# 在 .env 中填入你的 API 密钥

# 构建 WebUI（lloom-server 从 webui/dist 提供界面）
cd webui && npm install && npm run build && cd ..

# 启动 Rust 服务器（WebUI 在 :7861）
cargo run -p lloom-server
```

Rust 服务器（`:7861`）是唯一入口，会自动拉起 Python AI 微服务（`:7862`）和 Ollama（`:11434`）。

### 方式 C：构建发布包

```bash
# 完整构建（Rust release + AI 微服务打包）
bash scripts/build.sh

# 或分步：
bash scripts/build.sh --skip-ai       # 跳过 AI 微服务打包
```

**不捆绑 Ollama**。服务器使用 PATH 或 `localhost:11434` 上的系统 Ollama；若缺失，CLI / WebUI / TUI 会在用到本地模型时给出安装提示。安装方式：`curl -fsSL https://ollama.com/install.sh | sh`。

构建产物：
- `target/release/lloom-server` — 主服务器（REST + WebUI）
- `target/release/lloom-cli` — 命令行界面
- `dist/ai-service/ai-service` — 独立 AI 微服务可执行（约 26MB，封装 litellm）
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
| `LLOOM_PINNED_MODE` | `soft` | 钉选模式：`soft` 软优先（+0.3 加分），`hard` 强制指定 |

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
| POST | `/api/conversations/{id}/messages` | 追加单条消息（原子写）|
| PATCH | `/api/conversations/{id}/messages/{seq}` | 回填消息内容/元数据 |
| GET | `/api/services/status` | 诚实的服务状态 |
| POST | `/api/services/{name}/start` | 启动服务（ollama/ai） |
| POST | `/api/services/{name}/stop` | 停止服务 |
| POST | `/api/services/{name}/restart` | 重启服务 |
| GET | `/api/services/{name}/logs` | 服务日志 |
| POST | `/api/services/smart-restart` | 配置变更后重启 AI 服务 |
| POST | `/api/system/open-folder` | 打开目录 |
| POST | `/api/system/open-web` | 打开网页 |
| POST | `/api/system/cli` | 运行 CLI |
| GET | `/api/pricing/specs` | 列出所有 PriceSpec |
| PUT | `/api/pricing/specs/{provider}/{model}` | 手工改价 |
| POST | `/api/pricing/specs/{provider}/{model}/accept` | 采纳刷新价（转正 manual） |
| POST | `/api/pricing/refresh` | 触发远端定价刷新 job |
| GET | `/api/pricing/reference` | OpenRouter 参考价 × 本地图价联表（含偏差 %） |
| POST | `/api/pricing/reference/refresh` | 手动刷新参考价 |
| GET | `/api/pricing/calibration` | 校准曲线 |
| GET | `/api/probe/stats` | 探针消耗/预算 |
| PUT | `/api/probe/budget` | 调整探针月预算 |
| POST | `/api/routing/plan-subtask` | 子任务级路由规划（primary + fallback + escalation） |
| POST,GET | `/api/routing/shadow` | 影子评测采样（AIQ 重放） |
| GET | `/api/routing/overhead` | 路由开销报告（count/avg/P95/max/slow） |
| POST | `/api/shutdown` | 优雅关停（等价 SIGINT） |
| POST | `/api/cache/init` | 语义缓存预初始化（触发 chroma 模型下载） |
| GET | `/api/cache/status` | 缓存状态（就绪 / 下载进度） |
| POST | `/api/cache/cleanup` | 清理缓存 |
| POST | `/api/cache/feedback` | 命中反馈（灰区采样） |
| GET,POST | `/api/cache/threshold` | 缓存阈值查询 / 自调 |

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

# 模型
lloom-cli models list
lloom-cli models add qwen2.5-local --provider ollama --model ollama/qwen2.5:latest \
  --api-base http://localhost:11434 --input-cost 0.000001 --output-cost 0.000002
lloom-cli models update <名称> --input-cost 0.000001 --output-cost 0.000002
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
lloom-cli service apply DASHSCOPE_API_KEY     # 智能重启受影响服务

# 会话管理
lloom-cli conversation list
lloom-cli conversation show <id>
lloom-cli conversation delete <id>
lloom-cli conversation new

# 聊天 —— 单次 / 续接会话 / 交互式多轮
lloom-cli chat "2+2 等于几？"
lloom-cli chat "继续说" --session <id>
lloom-cli chat "你好" --interactive

# --session 与 conversation show/delete 既接受 ID 也接受标题（前缀）匹配；
# 运行 `lloom-cli conversation list` 查看现有会话。
```

### TUI（`tui/`）

OpenTUI + SolidJS 终端仪表盘（用 bun 运行，通过 REST 连接正在运行的服务器）。

```bash
cd tui
bun install
bun run src/index.tsx
```

 五个标签页：**首页**（Logo + 提示词 + 花费统计）、**聊天**（会话列表 + 流式聊天）、**模型**（已注册模型 + 添加表单）、**用量**（成本、模型定价）、**设置**（API 密钥 + 服务管理）。`Tab` 切换，`Ctrl+C` 退出。

- `Enter` 发送，`Shift+Enter` 换行
- 聊天侧栏顶部有 `[+] 新建对话` 项（默认选中）
- 会话携带完整多轮历史进入编排；缓存回复会标注"来自缓存"
- 模型页可直接通过内置表单添加模型（名称 / 提供商 / LiteLLM 模型 / API Base / 任务路由）
- 右键会话项弹出菜单（打开 / 删除），右键服务名弹出菜单（日志 / 重启 / 停止 / 启动），右键密钥行弹出编辑弹框
- 删除模型 / 会话前有确认弹框
- 首页 / 用量页每 30 秒自动刷新

## 项目结构

```
LLooM/
├── Cargo.toml                    # Rust workspace 根
├── crates/lloom-core/            # 业务核心 lib（UI 无关）
│   └── src/                      # server.rs, db.rs, router.rs, security.rs,
│                                 # ai_client.rs, processes.rs, conversations.rs,
│                                 # pricing.rs, probe.rs, signals.rs,
│                                 # metadata.rs, health.rs,
│                                 # models.rs, config.rs, error.rs
├── crates/lloom-server/          # 主服务器（REST + WebUI）
├── crates/lloom-cli/             # CLI（clap，链接 lloom-core）
├── webui/                        # WebUI 前端（React + Vite + Ant Design）
│   ├── src/                      # pages: Overview/Usage/Chat/Models/Pricing/Settings
│   └── dist/                     # 构建产物（由 lloom-server 提供服务）
├── tui/                          # TUI（OpenTUI + SolidJS，bun）
│   ├── src/                      # app.tsx, index.tsx, routes/, ui/
│   └── package.json
├── api/ai_service.py             # Python AI 微服务（litellm 封装）
├── assets/                       # README 等文档图片素材
├── scripts/
│   ├── build.sh                  # 跨平台构建（含系统依赖检测）
│   ├── download_ollama.sh        # 跨平台 Ollama 下载
│   ├── aiq_replay.py             # AIQ 离线重放
│   └── smoke_test.sh             # 19 项冒烟测试
├── ai_service.spec               # PyInstaller spec（AI 微服务）
├── ARCHITECTURE.md               # 分层详解 + REST 参考
├── pyproject.toml                # Python 项目配置（AI 服务）
└── .env.example                  # 环境模板
```

## 路线图

### 近期

- [ ] **OpenAI 兼容代理** — `POST /v1/chat/completions` + `GET /v1/models`，任意 OpenAI 客户端（ChatBox / Open WebUI / 沉浸式翻译 / Agent 框架）零改造接入评分路由、缓存与预算档
- [ ] **子任务并行执行** — 无依赖子任务 `asyncio.gather` 并行，降低编排端到端延迟
- [ ] **Prometheus 指标导出** — `GET /metrics`：按模型/任务类型/预算档计数、缓存命中、fallback 事件、路由开销

### 中期

- [ ] **路由权重闭环建议** — 离线重放网格搜索最优 (cost, quality, latency) 权重，人工审查后一键采纳
- [ ] **账单对账** — 云厂商账单导出 × 实际记账对账，报告偏差
- [ ] **多租户 / MCP 网关** — 视决策门（G1/G2）展开

## 文档

- [ARCHITECTURE.md](ARCHITECTURE.md) — 分层架构详解 + REST API 参考
- [TEST-GUIDE.md](TEST-GUIDE.md) — 功能测试指南
- [ROUTING-PLAN.md](ROUTING-PLAN.md) / [PRICING-PLAN.md](PRICING-PLAN.md) / [CONTEXT-PLAN.md](CONTEXT-PLAN.md) — 路由 / 定价 / 上下文设计文档
- [LLooMprogress.md](LLooMprogress.md) — 项目进展台账

## 许可证

MIT
