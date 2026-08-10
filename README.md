<p align="center">
  <img src="tauri-app/app-icon.jpg" width="120" height="120" alt="LLooM Logo" />
</p>

<h1 align="center">LLooM</h1>

<p align="center">
  智能 LLM 路由代理平台 — 多模型管理 · 智能路由 · 安全检测 · 任务编排 · 可视化管理
</p>

<p align="center">
  <img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT" />
  <img src="https://img.shields.io/badge/Platform-macOS%20ARM64-blue" alt="Platform" />
  <img src="https://img.shields.io/badge/Docker-Required-2496ED?logo=docker&logoColor=white" alt="Docker" />
  <img src="https://img.shields.io/badge/Python-3.11+-3776AB?logo=python&logoColor=white" alt="Python" />
  <img src="https://img.shields.io/badge/Rust-Tauri_v2-CE422B?logo=rust&logoColor=white" alt="Rust" />
  <img src="https://img.shields.io/badge/LiteLLM-v1.82.3-red" alt="LiteLLM" />
</p>

---

## 项目简介

LLooM 是一套基于 [LiteLLM](https://github.com/BerriAI/litellm) 的智能 LLM 路由代理平台，通过**控制面/数据面分离架构**统一管理阿里云百炼、OpenAI、Anthropic、Ollama 等多个供应商的模型。平台根据用户问题类型自动选择最优模型，结合安全检测、语义缓存、配额追踪和复杂任务编排，实现**成本最优、安全可控、开箱即用**的 LLM 基础设施。

### 为什么选择 LLooM?

| 痛点 | 解决方案 |
|------|----------|
| 多模型切换繁琐，手动选模型 | **智能路由** — 规则 + LLM 两层分类，自动选择最优模型 |
| API 调用成本不可控 | **配额追踪** — 按 Key/User/Model 维度追踪花费，预算上限保护 |
| 重复问题浪费 Token | **语义缓存** — Qdrant 向量相似度匹配，24h TTL，命中率可监控 |
| 复杂任务单模型难以胜任 | **任务编排** — 自动拆解为子任务，分配合适模型，汇总结果 |
| LLM 调用缺乏安全防护 | **Semantic Router** — PII 脱敏、越狱拦截、MMLU 14 域分类 |
| 命令行操作门槛高 | **Tauri 桌面应用 + Web 管理端** — 图形化管理全部功能 |
| 云端模型不可用时服务中断 | **三层 Fallback** — 云端高质量 → 云端降级 → 本地零成本兜底 |

---

## 核心特性

| 特性 | 说明 |
|------|------|
| **智能任务路由** | 两层混合分类（正则规则 → LLM 兜底），5 种任务类型自动映射最优模型 |
| **复杂任务编排** | 自动检测复杂度 → 拆解子任务 → 成本规划 → 多模型执行 → 结果汇总 |
| **Semantic Router 安全代理** | PII 脱敏（7 类）+ 越狱拦截（5 种攻击）+ MMLU 14 域分类 |
| **语义缓存** | Qdrant 向量库 + DashScope embedding，相似度 ≥ 0.95 命中缓存 |
| **配额与预算管理** | 按 Key/User/Model 三维追踪，默认 $10/30d，上限 $1000/365d |
| **三层 Fallback 容灾** | 云端高质量 → 云端降级 → Ollama 本地零成本兜底 |
| **Tauri 桌面应用** | 5 页面 GUI（总览/用量/对话/模型管理/设置），.env 可视化编辑 + 智能重启 |
| **Web 管理端** | 仪表盘 + SSE 流式智能对话 + 配置向导 + 模型管理 |
| **可观测性** | Prometheus 双端采集 + Grafana 预配仪表盘 + 自定义路由/配额指标 |
| **Open WebUI 集成** | 开箱即用的聊天前端，所有请求经 SR 安全检测 |

---

## 系统架构

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           用户访问层                                      │
├──────────────┬──────────────┬──────────────┬────────────────────────────┤
│  Tauri App   │  Open WebUI  │  Web 管理端   │     CLI 命令行             │
│  (桌面应用)   │  (:3001)     │  (:3002)     │  (litellm_cli.py)         │
└──────┬───────┴──────┬───────┴──────┬───────┴───────────┬────────────────┘
       │              │              │                    │
       ▼              ▼              ▼                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                    Semantic Router (:8888)                               │
│     PII 脱敏(7类) → 越狱拦截(5种) → MMLU 14 域分类 → (幻觉检测)          │
└─────────────────────────────┬───────────────────────────────────────────┘
                               ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                LiteLLM Worker (:4001) — 数据面                           │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────────────────┐    │
│  │ 智能任务路由  │  │ 语义缓存      │  │ 配额追踪                    │    │
│  │ task_router  │  │ Qdrant ≥0.95 │  │ quota_tracker              │    │
│  │ 规则→LLM分类  │  │ TTL=24h     │  │ Key/User/Model             │    │
│  └──────┬───────┘  └──────────────┘  └────────────────────────────┘    │
│  ┌──────▼──────────────────────────────────────────────────────────┐   │
│  │        LiteLLM Router (usage-based-routing-v2)                  │   │
│  │   fallbacks: qwen3.6-plus → qwen-plus → ... → qwen2.5-local    │   │
│  └──────┬──────────────────────────────────────────────────────────┘   │
└─────────┼───────────────────────────────────────────────────────────────┘
          ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                          模型供应商层                                     │
├────────────────┬────────────────┬────────────────┬─────────────────────┤
│  阿里云百炼     │  OpenAI        │  Anthropic     │  Ollama (本地)       │
│  qwen-plus     │  gpt-4o        │  claude-3.5    │  qwen2.5:latest      │
│  qwen3.6-flash │                │  sonnet        │  (零成本兜底)         │
│  qwen3.6-plus  │                │                │                     │
│  deepseek-v3   │                │                │                     │
└────────────────┴────────────────┴────────────────┴─────────────────────┘

┌─────────────────────────────────────────────────────────────────────────┐
│                           支撑服务层                                      │
├──────────────┬──────────────┬──────────────┬────────────────────────────┤
│ PostgreSQL   │ Redis 7      │ Qdrant       │ Prometheus + Grafana       │
│ (:5432)      │ (:6379)      │ (:6333)      │ (:9090)    (:3000)         │
│ 模型/密钥/   │ 路由状态/     │ 语义缓存     │ 指标采集 + 可视化           │
│ 花费记录     │ 限流计数器    │ 向量存储     │                            │
└──────────────┴──────────────┴──────────────┴────────────────────────────┘

┌──────────────────────────────────────────┐  ┌──────────────────────────┐
│  LiteLLM Admin (:4000) — 控制面          │  │ Orchestrator (:3002)     │
│  管理 UI + 配置 API + 密钥管理            │  │ 仪表盘+SSE对话+配置+模型  │
└──────────────────────────────────────────┘  └──────────────────────────┘
```

### 请求流转路径

```
标准请求:  用户 → Open WebUI → Semantic Router(PII/越狱/域分类) → Worker(智能路由) → 模型供应商

编排请求:  用户 → Tauri/Web → SR /check(安全预检) → 编排引擎(分解→规划→执行→汇总) → Worker → 模型供应商
```

---

## 技术栈

| 层级 | 技术 | 版本 | 用途 |
|------|------|------|------|
| LLM 代理 | LiteLLM | v1.82.3 | 多模型路由代理（控制面 + 数据面分离） |
| 向量缓存 | Qdrant | v1.16.3 | 语义缓存向量数据库 |
| 数据库 | PostgreSQL | 16.4-alpine | 模型/密钥/花费记录 |
| 缓存/限流 | Redis | 7-alpine | 路由状态 + 限流计数器 |
| 监控 | Prometheus | v3.8.1 | 指标采集（Admin + Worker 双端） |
| 可视化 | Grafana | 11.5.2 | 预配仪表盘 |
| 聊天前端 | Open WebUI | v0.9.2 | 开箱即用聊天界面 |
| 安全代理 | Flask | Python 3.11 | Semantic Router（自研） |
| 编排引擎 | Python | 3.11 | 复杂任务编排（自研） |
| 桌面应用 | Tauri | v2 | Rust 后端 + HTML/JS 前端 |
| 本地模型 | Ollama | — | qwen2.5:latest（零成本兜底层） |

---

## 快速开始

### 前置条件

- **macOS**（Apple Silicon ARM64；Tauri 桌面应用目前仅支持 macOS，Web 管理端和 CLI 可跨平台使用）
- **Docker Desktop**（含 Docker Compose）
- **Python 3**（宿主机仅需标准库运行 CLI；Web 管理端和 SR 的 Python 依赖在 Docker 容器内安装）
- **Ollama**（可选，用于本地零成本模型层）

### 一键安装

```bash
# 1. 克隆仓库
git clone https://github.com/citrus-dot/LLooM.git
cd LLooM

# 2. 复制环境变量模板
cp .env.example .env

# 3. 编辑 .env，填入 API Key（至少填 DASHSCOPE_API_KEY）
#    也可在安装向导中交互式填写
#    使用任意编辑器：vim .env / nano .env / code .env
vim .env

# 4. 一键安装（交互式向导，自动检测环境、收集密钥、启动服务）
./install.sh

# 或直接运行 CLI 初始化
python3 litellm_cli.py init
```

安装向导会自动完成：环境检测 → 收集 API Key → 适配路由策略 → 启动 10 个 Docker 服务 → 健康检查。

### 验证安装

```bash
# 健康检查（确认所有服务正常运行）
python3 litellm_cli.py health

# 运行状态（路由统计、花费、模型可用性）
python3 litellm_cli.py status
```

> 如果 `health` 检查失败，请先确认 Docker Desktop 已启动，然后查看日志排查：`python3 litellm_cli.py logs <服务名>`。常见问题排障请参考[项目文档第 13 节](项目文档.md#13-常见问题与排障)。

### 离线部署

```bash
# 在有网络的机器上导出镜像
./save-images.sh

# 拷贝镜像文件 + 项目目录到目标机器

# 导入镜像并安装
./load-images.sh
./offline-install.sh
```

---

## 服务端口一览

| 服务 | 端口 | 用途 |
|------|------|------|
| Admin UI | http://localhost:4000 | 管理界面 + 配置 API + 密钥管理 |
| Worker API | http://localhost:4001 | 推理 API（OpenAI 兼容格式） |
| Open WebUI | http://localhost:3001 | 聊天前端（开箱即用） |
| Orchestrator | http://localhost:3002 | Web 管理端（仪表盘+对话+配置+模型） |
| Semantic Router | http://localhost:8888 | 安全检测代理（PII/越狱/域分类） |
| Grafana | http://localhost:3000 | 可视化仪表盘（admin/admin） |
| Prometheus | http://localhost:9090 | 指标采集 |
| PostgreSQL | localhost:5432 | 数据库 |
| Redis | localhost:6379 | 路由状态 + 限流 |
| Qdrant | localhost:6333 | 语义缓存向量库 |

---

## 功能详解

### 智能任务路由

请求中指定 `model="auto"` 即可触发智能路由。平台采用两层混合分类策略：

| 分类层 | 机制 | 成本 | 延迟 |
|--------|------|------|------|
| 第一层 | 正则规则匹配 | 零 | 零 |
| 第二层 | LLM 意图识别 | 低 | ~1s |

**任务类型 → 模型映射**:

| 任务类型 | 推荐模型 | 场景 |
|----------|----------|------|
| `simple_qa` | qwen2.5-local (Ollama) | 简单问答，零 API 费用 |
| `general` | qwen-plus (百炼) | 通用对话，性价比最优 |
| `coding` | deepseek-v3 (百炼) | 编程任务 |
| `math_logic` | deepseek-v3 (百炼) | 数学推理 |
| `complex_reasoning` | qwen3.6-plus (百炼) | 复杂综合推理 |

> 推理模型（qwen3.6-flash/plus、qwen3-max、deepseek-v3）自动启用流式响应，避免 reasoning tokens 导致 HTTP 超时。

### 复杂任务编排

```
用户输入 "写一个 Python 爬虫并测试它"
    │
    ├─ 1. 复杂度检测 ✓ (多步骤关键词命中)
    ├─ 2. 任务分解 → [子任务1: 写爬虫(coding)] [子任务2: 测试爬虫(coding)]
    ├─ 3. 成本规划 → deepseek-v3 × 2，预估 $0.003
    ├─ 4. 按序执行 → 子任务1结果作为子任务2上下文
    └─ 5. 结果汇总 → LLM 合并为连贯回答
```

编排引擎支持对话上下文（最近 10 轮历史），并对 simple_qa/general 类型启用语义缓存。

### Semantic Router 安全代理

所有请求经过 4 层插件链检测：

| 插件 | 检测内容 | 动作 |
|------|----------|------|
| **PII 检测** | 邮箱、手机号、SSN、信用卡、IP、身份证、银行账号（7 类） | 脱敏替换 |
| **越狱检测** | DAN、指令覆盖、角色操纵、安全绕过、提示注入（5 种攻击） | 拦截阻断 |
| **域分类** | MMLU 14 学科领域（关键词预筛 + LLM 兜底） | 路由增强 |
| **幻觉检测** | LLM-based 事实核查（默认关闭） | 加头/加警告/拦截 |

SR 检测结果通过 `X-SR-Domain`、`X-SR-PII-Types`、`X-SR-Jailbreak-Types` 响应头传递给客户端和 task_router，实现路由增强（如 STEM 域 → 数学/逻辑模型）。

### 语义缓存

| 参数 | 值 | 说明 |
|------|------|------|
| 向量数据库 | Qdrant | Cosine 余弦相似度 |
| Embedding | DashScope text-embedding-v3 | 1024 维 |
| 相似度阈值 | 0.95 | 低于此值不命中（防误命中） |
| TTL | 86400s (24h) | 自动过期 |
| 量化 | binary | 32 倍压缩，节省存储 |

> 编排引擎对 simple_qa/general 类型启用缓存，复杂子任务禁用缓存（结果不可复用）。

### 配额追踪与预算管理

- **追踪维度**: Virtual Key + User ID + Model
- **预算默认**: 新密钥 max_budget=$10 / 30d
- **安全上限**: max_budget=$1000 / 365d
- **分类请求排除**: task_router 内部分类调用不计入配额（避免双重计数）
- **Prometheus 指标**: 按 Key/User/Model 维度的花费和请求数 Counter

### 三层 Fallback 容灾

```
云端高质量                  云端降级                    本地兜底
qwen3.6-plus ──失败──→ qwen-plus ──失败──→ qwen3.6-flash ──失败──→ qwen2.5-local (Ollama, 零成本零依赖)
qwen3-max    ──失败──→ qwen3.6-plus
deepseek-v3  ──失败──→ qwen3.6-plus
```

### Tauri 桌面应用

| 页面 | 功能 |
|------|------|
| **总览** | Docker 服务状态、SR 安全状态、快捷入口 |
| **用量** | 缓存命中、累计花费、模型花费分布、路由统计、定价表 |
| **对话** | SSE 智能聊天、任务分解可视化、对话历史持久化、Markdown 渲染 |
| **模型管理** | 可视化添加/删除模型（替代 CLI 交互命令）、初始化向导 |
| **设置** | .env 配置编辑（分组可折叠）、单字段/批量保存、智能重启 |

**智能重启**: 修改 .env 配置后，自动根据变更的键映射到受影响的 Docker 服务，仅重启必要服务（如 API Key 变更 → 重启 worker+admin），无需全量重启。

---

## CLI 命令

```bash
# 初始化配置（交互式向导：环境检测 → 收集密钥 → 启动服务）
python3 litellm_cli.py init

# 模型管理
python3 litellm_cli.py add-model                          # 交互式添加模型
python3 litellm_cli.py list-models                        # 列出已配置模型
python3 litellm_cli.py remove-model --name <模型名>        # 删除模型

# 运维
python3 litellm_cli.py health                             # 健康检查
python3 litellm_cli.py status                             # 运行时状态（容器/路由/配额/模型）
python3 litellm_cli.py logs [服务名] --lines 100           # 查看服务日志

# 任务编排
python3 litellm_cli.py orchestrate "写一个Python爬虫并测试它"
```

---

## API 调用示例

> 以下示例中的 `sk-1234` 为 `.env` 中 `LITELLM_MASTER_KEY` 的默认值，生产环境请务必修改。如需为不同用户创建独立密钥，请在 Admin UI（http://localhost:4000）中创建 Virtual Key。

### 智能路由（自动选择模型）

```bash
curl http://localhost:4001/v1/chat/completions \
  -H "Authorization: Bearer sk-1234" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "auto",
    "messages": [{"role": "user", "content": "解释快速排序算法"}],
    "max_tokens": 500
  }'
```

### 指定模型调用

```bash
curl http://localhost:4001/v1/chat/completions \
  -H "Authorization: Bearer sk-1234" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "deepseek-v3",
    "messages": [{"role": "user", "content": "写一个二分查找的Python实现"}],
    "max_tokens": 500
  }'
```

### 复杂任务编排（SSE 流式）

```bash
# CLI 编排
python3 litellm_cli.py orchestrate "写一个Python爬虫并测试它"

# Web SSE 流式（实时查看分解/执行/汇总过程）
curl -N -X POST "http://localhost:3002/api/chat/stream" \
  -H "Content-Type: application/json" \
  -d '{"q": "写一个Python爬虫并测试它"}'
```

---

## 项目结构

```
LLooM/
├── docker-compose.yml            # Docker 编排（10 个服务）
├── config_admin.yaml             # 控制面配置
├── config_worker.yaml            # 数据面配置（模型/路由/缓存/回调）
├── .env.example                  # 环境变量模板
├── redis.conf                    # Redis 安全与性能配置
├── prometheus.yml                # Prometheus 采集配置
│
├── custom_callbacks.py           # 智能路由 + 配额追踪回调
├── task_orchestrator.py          # 复杂任务编排引擎
├── litellm_cli.py                # CLI 管理工具
├── install.sh                    # 一键安装脚本
│
├── semantic_router/              # Semantic Router 安全代理
│   ├── app.py                    # Flask 代理 + PII/越狱/域分类/幻觉检测
│   └── config.yaml               # SR 配置
│
├── webapp/                       # Web 管理端
│   ├── app.py                    # Flask 应用（仪表盘/对话/配置/模型/API）
│   └── requirements.txt
│
├── tauri-app/                    # Tauri 桌面应用
│   └── src-tauri/
│       ├── src/main.rs           # Rust 后端（Docker/配置/对话/模型管理）
│       ├── ui/index.html         # 前端 UI（5 页面）
│       └── tauri.conf.json       # Tauri 配置
│
├── grafana/                      # Grafana 仪表盘
│   ├── dashboards/               # 仪表盘 JSON
│   └── provisioning/             # 自动加载配置
│
├── conversations/                # 对话历史存储（JSON）
├── save-images.sh                # Docker 镜像导出（离线部署）
├── load-images.sh                # Docker 镜像导入
└── offline-install.sh            # 离线安装脚本
```

---

## Docker 服务

| # | 服务 | 端口 | 镜像 | 职责 |
|---|------|------|------|------|
| 1 | litellm-admin | 4000 | litellm:v1.82.3 | 控制面：管理 UI + 配置 API |
| 2 | litellm-worker | 4001 | litellm:v1.82.3 | 数据面：LLM 请求 + 路由 + 缓存 |
| 3 | db | 5432 | postgres:16.4 | 数据库：模型/密钥/花费 |
| 4 | redis | 6379 | redis:7 | 路由状态 + 限流 |
| 5 | qdrant | 6333 | qdrant:v1.16.3 | 语义缓存向量库 |
| 6 | prometheus | 9090 | prometheus:v3.8.1 | 指标采集 |
| 7 | grafana | 3000 | grafana:11.5.2 | 可视化面板 |
| 8 | open-webui | 3001 | open-webui:v0.9.2 | 聊天前端 |
| 9 | semantic-router | 8888 | python:3.11-slim | 安全检测代理 |
| 10 | orchestrator-web | 3002 | python:3.11-slim | 编排平台 + 管理 API |

所有镜像使用 ARM64 原生版本，适配 Apple Silicon。

---

## 监控与可观测性

### Grafana 仪表盘

| 仪表盘 | 直达链接 | UID |
|--------|----------|-----|
| LiteLLM 总览 | http://localhost:3000/d/litellm-overview | litellm-overview |
| 配额与智能路由 | http://localhost:3000/d/litellm-quota | litellm-quota |

### 关键 Prometheus 指标

| 指标 | 用途 |
|------|------|
| `litellm_task_router_classification_total` | 路由分类统计（按 task_type） |
| `litellm_quota_key_spend_total` | 按 Key 累计花费 |
| `litellm_quota_user_spend_total` | 按 User 累计花费 |
| `litellm_quota_key_requests_total` | 按 Key 累计请求数 |
| `litellm_cache_hits_metric_total` | 缓存命中次数 |

---

## 路线图

### 短期

- [ ] 幻觉检测启用测试（延迟影响评估）
- [ ] 缓存命中率 Grafana 仪表盘
- [ ] SR 统计持久化（内存 → 数据库）
- [ ] 对话导出（Markdown / JSON）
- [ ] 模型可用性实时检测

### 中期

- [ ] 多用户支持（Open WebUI 认证 + 用户隔离）
- [ ] API 调用审计日志
- [ ] 模型 A/B 测试
- [ ] 自定义 SR 插件系统
- [ ] 缓存预热 + 成本告警

### 长期

- [ ] Kubernetes 部署（水平扩展）
- [ ] 多区域容灾
- [ ] RAG 知识库集成
- [ ] 多模态支持（图片/音频/视频）
- [ ] Windows / Linux 跨平台支持

---

## 技术文档

详细的技术实现文档请参考：

- [项目文档](项目文档.md) — 完整项目文档（含代码级实现注释、技术决策说明、踩坑记录）
- [项目进展](progress.md) — 各阶段进展、优化记录、经验总结

### 关键技术参考

- [LiteLLM Callbacks](https://docs.litellm.ai/docs/proxy/callbacks) — 自定义回调机制
- [LiteLLM Caching](https://docs.litellm.ai/docs/proxy/caching) — 语义缓存配置
- [LiteLLM Reliability](https://docs.litellm.ai/docs/proxy/reliability) — Fallback 容灾
- [LiteLLM Routing](https://docs.litellm.ai/docs/routing) — 路由策略
- [Tauri v2 CSP](https://v2.tauri.app/security/csp/) — 内容安全策略
- [Qdrant Quantization](https://qdrant.tech/documentation/manage-data/quantization/) — 向量量化

---

## 许可证

本项目基于 [MIT License](LICENSE) 开源。
