<p align="center">
  <img src="assets/logo.png" width="100" height="100" alt="LLooM Logo" />
</p>

<h1 align="center">LLooM</h1>

<p align="center">
  自托管的 LLM 路由网关：一个 Rust 进程完成模型路由、成本核算、缓存与 Provider 接入。<br/>
  本地优先，不需要 Docker 或外部数据库。
</p>

<p align="center">
  <a href="https://github.com/citrus-dot/LLooM/actions/workflows/ci.yml"><img src="https://github.com/citrus-dot/LLooM/actions/workflows/ci.yml/badge.svg?branch=v2" alt="CI" /></a>
  <a href="https://github.com/citrus-dot/LLooM/releases"><img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-blue" alt="支持平台" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-green" alt="MIT 许可证" /></a>
</p>

<p align="center">
  <a href="README.md">English</a> · <strong>简体中文</strong> · <a href="https://github.com/citrus-dot/LLooM/releases">发布包</a> · <a href="CONTRIBUTING.md">参与开发</a> · <a href="SECURITY.md">安全策略</a>
</p>

LLooM 位于你的应用与 LLM Provider 之间。通过 WebUI 注册本地或云模型，直接聊天，或让现有客户端使用 OpenAI 兼容的 `/v1` 代理。路由、预算和用量在一个进程内完成；应用数据与两级缓存分别使用独立的本地 SQLite 数据库。

## 功能

- **智能路由与容灾**：规则/LLM 两层任务分类，能力与健康门槛，成本/质量评分，回退候选与可选钉选模型。
- **任务编排**：任务拆解、依赖波次与结果聚合，SSE 全程流式；WebUI 可停止生成并保存已产出内容。
- **成本与预算**：按模型统计用量与费用、预算档联动、探针月度预算、离线路由体检与百炼账单对账。
- **两级缓存**：L1 精确匹配 + L2 FastEmbed 语义检索；两级均按模型、system prompt、请求上下文和温度隔离，L2 要求温度精确一致。L2 不可用时自动直连 Provider。
- **Provider**：OpenAI-compatible 与 Anthropic Messages；OpenAI、DashScope、Anthropic 支持默认 API Base；支持 Ollama、LM Studio、vLLM 等本地端点。
- **多前端**：WebUI、Rust CLI、OpenTUI/SolidJS TUI 共用同一 REST 契约。
- **本地访问边界**：管理接口与 `/metrics` 仅允许本机访问；`/v1` 支持哈希存储 API Key、额度、RPM、有效期和模型白名单。

## 快速开始

### 方式 A：下载发布包

1. 从 [Releases](https://github.com/citrus-dot/LLooM/releases) 下载对应平台的压缩包。
2. 解压，macOS/Linux 运行 `start.sh`，Windows 运行 `start.bat`；浏览器会打开 <http://localhost:7861>。
3. 在「模型管理」页添加模型并配置 API Key，即可聊天，或让客户端接入 `http://localhost:7861/v1`。

停止：运行 `stop.sh` 或 `stop.bat`。Windows 二进制未签名，SmartScreen 提示时点「更多信息 → 仍要运行」。macOS 首次启动前对解压目录执行一次 `xattr -cr LLooM`。

**Intel Mac**：请选择 `darwin-x86_64` 包。L2 另需 `brew install onnxruntime`；不安装则仅绕过 L2，L1 与路由不受影响。Ollama 为可选组件，不随包提供。常见问题见[故障排查](TEST-GUIDE.md#故障排查)。

### 方式 B：源码运行

依赖：stable Rust、Node.js 20+ 和 npm。只有运行 TUI 时才需要 Bun。

```bash
git clone -b v2 https://github.com/citrus-dot/LLooM.git
cd LLooM
cp .env.example .env

npm --prefix webui ci
npm --prefix webui run build
cargo run -p lloom-server
```

打开 <http://localhost:7861> 添加模型。Key 可直接在模型页填写，也可在 `.env` 中配置后在模型页引用变量名。没有 Key 时服务仍可启动，但模型调用需要至少一个已配置的模型。

TUI 需保持服务端运行，另开终端执行 `cd tui && bun install && bun run dev`。CLI 也连接同一服务：`lloom-cli chat "解释一下快速排序"`。开发与协作命令见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## API 用法

内置聊天端点接受 `messages` 数组，返回 SSE 帧：

```bash
curl -N http://127.0.0.1:7861/api/chat/stream \
  -H 'Content-Type: application/json' \
  -d '{"messages":[{"role":"user","content":"简要解释快速排序"}]}'
```

OpenAI 兼容代理提供 `GET /v1/models` 与 `POST /v1/chat/completions`。本机访问时，未创建任何 API Key 可以免鉴权；创建首个 Key 后，需要发送 `Authorization: Bearer <你的 API Key>`。局域网客户端始终需要 Key。聊天请求目前支持 `model`、`messages`、`temperature`、`max_tokens`、`stream` 字段。

端点分组见 [ARCHITECTURE.md](ARCHITECTURE.md#http-api)，流式契约见 [docs/SSE-CONTRACT.md](docs/SSE-CONTRACT.md)，可复制验收命令见 [TEST-GUIDE.md](TEST-GUIDE.md)。

## 配置

将 `.env.example` 复制为 `.env`。常用项如下：

| 变量 | 默认值 | 用途 |
|---|---|---|
| `DASHSCOPE_API_KEY` | 空 | DashScope 模型的可选后备 Key |
| `OPENAI_API_KEY` | 空 | OpenAI 模型的可选后备 Key |
| `ANTHROPIC_API_KEY` | 空 | Anthropic 模型的可选后备 Key |
| `LLOOM_WEB_PORT` | `7861` | WebUI 与 API 端口 |
| `LLOOM_BIND` | `127.0.0.1` | 监听地址；非环回绑定要求启动时已配置 API Key |
| `LLOOM_DATA_DIR` | `./data` | SQLite、对话与缓存模型文件目录 |
| `LLOOM_SEMANTIC_THRESHOLD` | `0.88` | L2 余弦相似度阈值 |
| `LLOOM_CACHE_MAX_ENTRIES` | `5000` | 缓存容量 |
| `LLOOM_LOG` | `info` | 日志级别：`error`、`warn`、`info` |
| `LLOOM_SERVER_URL` | 本机服务 | CLI/TUI 连接的服务地址 |

`OLLAMA_API_BASE` 默认 `http://localhost:11434`。模型 Key 字段可以引用环境变量名；`.env` 不提交。健康探测间隔、影子采样比例、安全拦截开关可在系统设置中修改；端口、监听地址和数据目录需要重启。

## 文档

- **[ARCHITECTURE.md](ARCHITECTURE.md)**：运行时、模块、HTTP 路由、缓存作用域、存储与错误语义。
- **[CONTRIBUTING.md](CONTRIBUTING.md)**：开发环境、质量门、修改边界与发布流程。
- **[TEST-GUIDE.md](TEST-GUIDE.md)**：隔离数据的手工验收与故障排查。
- **[LLooMprogress.md](LLooMprogress.md)**：当前基线、已完成工作与后续方向。
- **[benchmarks/HANDOVER.md](benchmarks/HANDOVER.md)** 与 **[docs/handover/00-总览与手记索引.md](docs/handover/00-总览与手记索引.md)**：研究/Benchmark 交接，不属于用户快速上手指南。

最新已发布版本为 **v3.0.2**；`v2` 工作区可能包含已复核但尚未发布的修复，请按实际要使用的版本构建。`docs/archive/` 中的设计计划仅作历史参考，不代表当前实现。

## 许可证

[MIT](LICENSE)
