# LLooM v2 — 智能大模型路由平台

[English](README.md) | **中文**

一个自包含的桌面应用，管理多个大语言模型，根据任务类型智能路由请求，追踪 Token 用量和成本，并提供安全过滤 — 无需任何外部基础设施。

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
- 嵌入模型不可用时优雅降级

### 桌面 GUI（Tauri）
- 5 页界面：概览、用量、聊天、模型、设置
- 进程管理：从 UI 启动/停止 API 服务器和 Ollama
- API 密钥配置与智能重启（配置变更后自动重载服务器）
- 对话历史持久化（本地 JSON 文件）
- 系统托盘快捷访问

## 架构

```
LLooM.app（308MB，自包含）
├── Tauri 二进制（Rust 后端）
│   ├── 进程管理（启动 API + Ollama）
│   ├── API 代理（基于 curl，避免混合内容）
│   ├── 对话 CRUD
│   └── 系统托盘
├── Python 核心（PyInstaller 打包，222MB）
│   ├── FastAPI 服务器（端口 7860，REST + SSE）
│   ├── SmartRouter（分类 + 路由）
│   ├── TaskOrchestrator（分解 + 聚合）
│   ├── Security（PII + 越狱 + 领域）
│   ├── SemanticCache（ChromaDB）
│   ├── ModelManager（CRUD + 用量 + 预算）
│   └── litellm SDK（统一 LLM API）
├── Ollama 二进制（63MB，本地 LLM 运行时）
└── Resources（脚本、.env 模板）
```

**零外部依赖**：SQLite 替代 PostgreSQL，ChromaDB 替代 Qdrant，内嵌 Python 替代 Docker，内置 Ollama 替代系统安装。

## 快速开始

### 方式 A：下载应用

1. 从 [GitHub Releases](https://github.com/citrus-dot/LLooM/releases) 下载 `LLooM.app`
2. 拖入应用程序文件夹
3. 双击启动
4. 在设置 → API 密钥中配置密钥
5. 开始聊天

### 方式 B：从源码构建

#### 前置要求

- Python 3.10+ 及 pip
- Rust 工具链（`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`）
- Node.js 18+ 及 npm
- Xcode 命令行工具（`xcode-select --install`）
- 已安装 [Ollama](https://ollama.com)（用于本地模型支持）

#### 构建步骤

```bash
git clone -b v2 https://github.com/citrus-dot/LLooM.git
cd LLooM

# 安装 Python 依赖
pip install -e ".[dev]"

# 运行完整构建
bash scripts/build.sh

# 或分步构建：
bash scripts/build.sh --skip-ollama      # 跳过 Ollama 下载
bash scripts/build.sh --skip-tauri       # 跳过 Tauri 构建（仅 PyInstaller）
bash scripts/build.sh --skip-pyinstaller # 跳过 Python 打包
```

构建产物：
- `dist/lloom-server/` — PyInstaller 打包（222MB）
- `tauri-app/src-tauri/target/release/bundle/macos/LLooM.app` — 最终应用（308MB）

### 方式 C：开发模式

```bash
git clone -b v2 https://github.com/citrus-dot/LLooM.git
cd LLooM

# 安装依赖
pip install -e ".[dev]"
cd tauri-app && npm install && cd ..

# 复制并编辑环境配置
cp .env.example .env
# 在 .env 中填入你的 API 密钥

# 运行测试
python3 tests/test_phase1.py  # 37 个测试
python3 tests/test_phase2.py  # 64 个测试
python3 tests/test_phase4.py  # 115 个测试
python3 tests/test_phase5.py  # 78 个测试
python3 tests/test_phase6.py  # 55 个测试

# 启动 API 服务器
python3 -m uvicorn api.server:app --port 7860

# 启动 Tauri 开发模式
cd tauri-app && npx tauri dev
```

## 配置

所有配置通过 `.env` 环境变量文件完成：

| 键 | 默认值 | 说明 |
|-----|---------|------|
| `DASHSCOPE_API_KEY` | （空） | 阿里云百炼 DashScope API 密钥 |
| `DASHSCOPE_API_BASE` | `https://dashscope.aliyuncs.com/compatible-mode/v1` | DashScope 端点 |
| `OPENAI_API_KEY` | （空） | OpenAI API 密钥 |
| `OPENAI_BASE_URL` | （空） | OpenAI 基础 URL 覆盖 |
| `ANTHROPIC_API_KEY` | （空） | Anthropic API 密钥 |
| `LLOOM_API_PORT` | `7860` | FastAPI 服务器端口 |
| `LLOOM_DATA_DIR` | `./data` | 数据目录（SQLite、ChromaDB、对话） |
| `OLLAMA_API_BASE` | `http://localhost:11434` | Ollama 端点 |

### 默认预算

- 最大预算：$10
- 周期：30 天
- 上限：$1000 / 365 天

## API 端点

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

## 命令行工具

```bash
# 初始化数据库
python3 cli/lloom.py init

# 列出模型
python3 cli/lloom.py model list

# 添加模型
python3 cli/lloom.py model add --name my-model --provider openai --litellm-model openai/gpt-4o

# 查看状态
python3 cli/lloom.py status

# 聊天
python3 cli/lloom.py chat "2+2 等于几？"

# 编排复杂任务
python3 cli/lloom.py orchestrate "写一个 Python 网页爬虫并解释它的工作原理"

# 启动 API 服务器
python3 cli/lloom.py serve
```

## 技术栈

| 组件 | 技术 | 用途 |
|-----------|-----------|---------|
| LLM API | litellm SDK | 所有 LLM 供应商的统一接口 |
| API 服务器 | FastAPI + Uvicorn | REST + SSE 端点 |
| 数据库 | SQLite（WAL 模式） | 模型注册、用量追踪、预算 |
| 向量缓存 | ChromaDB（PersistentClient） | 问答语义缓存 |
| 命令行 | Click | 开发者友好的命令接口 |
| 桌面端 | Tauri v2（Rust） | 进程管理 + 原生 GUI |
| 打包 | PyInstaller | 打包 Python 运行时及所有依赖 |
| 本地 LLM | Ollama（内置二进制） | 零成本兜底模型运行时 |

## 项目结构

```
LLooM/
├── core/                    # 业务逻辑
│   ├── config.py            # 环境配置
│   ├── database.py          # SQLite CRUD（模型、用量、预算）
│   ├── model_manager.py     # 模型生命周期 + 成本计算
│   ├── smart_router.py      # 两层分类 + 路由
│   ├── orchestrator.py      # 任务分解 + 聚合
│   ├── cache.py             # ChromaDB 语义缓存
│   ├── security.py          # PII + 越狱 + 领域分类
│   ├── callbacks.py         # litellm 用量追踪回调
│   └── seed_models.py       # 默认模型定价数据
├── api/
│   └── server.py            # FastAPI REST + SSE 服务器
├── cli/
│   └── lloom.py             # Click CLI（7 个命令）
├── tauri-app/
│   ├── src-tauri/
│   │   ├── src/main.rs      # Rust 后端（24 个 Tauri 命令）
│   │   ├── ui/index.html    # 5 页 SPA 前端
│   │   ├── tauri.conf.json  # Tauri 打包配置
│   │   └── resources/       # 打包的 PyInstaller + Ollama
│   └── package.json
├── tests/                   # 6 阶段测试套件（401 个测试）
├── scripts/
│   ├── build.sh             # 完整构建流水线
│   ├── download_ollama.sh   # Ollama 二进制下载
│   └── first_run_setup.py   # 首次运行数据库初始化 + 模型拉取
├── lloom_server.py          # PyInstaller 入口
├── lloom.spec               # PyInstaller 配置
├── pyproject.toml           # Python 项目配置
├── .env.example             # 环境模板
└── progress.md              # 开发进度追踪
```

## 许可证

MIT
