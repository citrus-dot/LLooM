<p align="center">
  <img src="assets/logo.png" width="100" height="100" alt="LLooM logo" />
</p>

<h1 align="center">LLooM</h1>

<p align="center">
  Self-hosted LLM routing gateway: one Rust process for model routing, cost tracking, caching and provider access.<br/>
  Local-first, no Docker and no external database required.
</p>

<p align="center">
  <a href="https://github.com/citrus-dot/LLooM/actions/workflows/ci.yml"><img src="https://github.com/citrus-dot/LLooM/actions/workflows/ci.yml/badge.svg?branch=v2" alt="CI" /></a>
  <a href="https://github.com/citrus-dot/LLooM/releases"><img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-blue" alt="Supported platforms" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-green" alt="MIT license" /></a>
</p>

<p align="center">
  <strong>English</strong> · <a href="README-ZH.md">简体中文</a> · <a href="https://github.com/citrus-dot/LLooM/releases">Releases</a> · <a href="CONTRIBUTING.md">Contributing</a> · <a href="SECURITY.md">Security</a>
</p>

LLooM sits between your applications and LLM providers. Register local or cloud models in its Web UI, then chat through the built-in interface or use the OpenAI-compatible `/v1` proxy. Routing, budgets and usage accounting share one process; application data and the two cache layers use separate local SQLite databases.

## Features

- **Routing and failover**: rule/LLM task classification, capability and health gates, cost/quality scoring, fallback candidates and optional model pinning.
- **Task orchestration**: decomposition, dependency-aware waves and result aggregation over SSE. The Web UI can stop generation and save the partial answer.
- **Usage and budgets**: per-model costs, budget tiers, monthly probe budget, offline routing review and DashScope bill reconciliation.
- **Exact and semantic cache**: L1 exact matching and L2 FastEmbed semantic similarity. Both scope by model, system prompt, request context and temperature (L2 matches it exactly); L2 falls back to the provider when unavailable.
- **Providers**: OpenAI-compatible endpoints and Anthropic Messages. OpenAI, DashScope and Anthropic have default API base URLs; local Ollama, LM Studio and vLLM endpoints are supported.
- **Interfaces**: Web UI, Rust CLI and OpenTUI/SolidJS terminal UI share the same REST API.
- **Local-first security boundary**: management endpoints and metrics are loopback-only. `/v1` supports hashed API keys, quotas, RPM limits, expiry and model allowlists.

## Quick start

### Download a release

1. Download the appropriate archive from [Releases](https://github.com/citrus-dot/LLooM/releases).
2. Extract it and run `start.sh` on macOS/Linux or `start.bat` on Windows. The Web UI opens at <http://localhost:7861>.
3. Add a model on **Models** and configure its provider key. Start chatting or configure a client to use `http://localhost:7861/v1`.

Run `stop.sh` or `stop.bat` to stop. On Windows, allow the unsigned binary through SmartScreen if necessary. On macOS, run `xattr -cr LLooM` once on the extracted folder.

**Intel Mac:** choose `darwin-x86_64`. L2 additionally needs `brew install onnxruntime`; without it, L2 is bypassed while L1 and routing still work. Ollama is optional and is not bundled. See [Troubleshooting](TEST-GUIDE.md#troubleshooting).

### Build from source

Prerequisites: stable Rust, Node.js 20+ and npm. Install Bun only if you want to run the TUI.

```bash
git clone -b v2 https://github.com/citrus-dot/LLooM.git
cd LLooM
cp .env.example .env

npm --prefix webui ci
npm --prefix webui run build
cargo run -p lloom-server
```

Open <http://localhost:7861> and add a model. You can set its key in the Web UI or reference an environment variable from `.env`; the process starts without keys but model calls need at least one configured model.

For the TUI, keep the server running and run `cd tui && bun install && bun run dev` in another terminal. `lloom-cli chat "Explain quicksort"` uses the same server. Development and contribution commands are in [CONTRIBUTING.md](CONTRIBUTING.md).

## Use the APIs

The built-in chat endpoint takes a `messages` array and returns SSE frames:

```bash
curl -N http://127.0.0.1:7861/api/chat/stream \
  -H 'Content-Type: application/json' \
  -d '{"messages":[{"role":"user","content":"Explain quicksort briefly"}]}'
```

The OpenAI-compatible proxy supports `GET /v1/models` and `POST /v1/chat/completions`. On loopback, it works without a key until you create the first API key. After that, send `Authorization: Bearer <your-api-key>`. LAN clients must always use a key. Only `model`, `messages`, `temperature`, `max_tokens` and `stream` are currently accepted in chat requests.

See [ARCHITECTURE.md](ARCHITECTURE.md#http-api) for endpoint groups, [docs/SSE-CONTRACT.md](docs/SSE-CONTRACT.md) for the streaming contract and [TEST-GUIDE.md](TEST-GUIDE.md) for copyable acceptance checks.

## Configuration

Copy `.env.example` to `.env`. The most common settings are:

| Variable | Default | Purpose |
|---|---|---|
| `DASHSCOPE_API_KEY` | empty | Optional fallback key for a DashScope model |
| `OPENAI_API_KEY` | empty | Optional fallback key for an OpenAI model |
| `ANTHROPIC_API_KEY` | empty | Optional fallback key for an Anthropic model |
| `LLOOM_WEB_PORT` | `7861` | Web UI and API port |
| `LLOOM_BIND` | `127.0.0.1` | Listen address; a non-loopback bind requires an API key at startup |
| `LLOOM_DATA_DIR` | `./data` | SQLite databases, conversations and downloaded cache model |
| `LLOOM_SEMANTIC_THRESHOLD` | `0.88` | L2 cosine similarity threshold |
| `LLOOM_CACHE_MAX_ENTRIES` | `5000` | Cache capacity |
| `LLOOM_LOG` | `info` | Log level: `error`, `warn` or `info` |
| `LLOOM_SERVER_URL` | loopback server | Server URL for CLI/TUI clients |

`OLLAMA_API_BASE` defaults to `http://localhost:11434`. Provider key variables can be referenced by name from a model's key field; `.env` is not committed. Health intervals, shadow sampling and security blocking switches are editable in System Settings. Port, bind address and data directory require a restart.

## Documentation

- **[ARCHITECTURE.md](ARCHITECTURE.md)** — runtime, modules, HTTP routes, cache scope, storage and error semantics.
- **[CONTRIBUTING.md](CONTRIBUTING.md)** — development setup, quality gates, change boundaries and release process.
- **[TEST-GUIDE.md](TEST-GUIDE.md)** — isolated manual acceptance tests and troubleshooting.
- **[LLooMprogress.md](LLooMprogress.md)** — current baseline, completed work and roadmap.
- **[benchmarks/HANDOVER.md](benchmarks/HANDOVER.md)** and **[docs/handover/00-总览与手记索引.md](docs/handover/00-总览与手记索引.md)** — research/benchmark handover, not user documentation.

The latest published release is **v3.0.2**. The `v2` working tree may contain reviewed-but-unreleased fixes; build the version you intend to use. Archived design plans are historical references, not the current architecture.

## License

[MIT](LICENSE)
