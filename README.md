# LLooM — Intelligent LLM Routing Platform

**English** | [中文](README-ZH.md)

A self-contained LLM routing platform. A Rust core server manages models, routes requests by task type, tracks token usage and costs, and filters requests for security — with a thin Python service only for the LLM calls that Rust can't replace.

## Architecture

LLooM is layered with the **REST API as the single contract** between the UI and the business core. Any frontend — the built-in WebUI, the Tauri desktop GUI, or a future TUI — plugs into the same API.

```
UI layer (WebUI / Tauri GUI / future TUI)   ← any frontend, UI-agnostic
        │  HTTP REST (all typed JSON objects)
Rust core + axum REST server (:7861)        ← primary server, all business logic
        │  function calls
Rust core modules (db / router / security / processes / conversations)
        │  async HTTP to the AI service
Python AI micro-service (:7862)             ← stateless litellm wrapper
        │
LLM providers (DashScope / Ollama / OpenAI / Anthropic)
```

Key points:
- **Rust axum server** (`:7861`) is the primary server. It owns SQLite, task routing, security filtering, process management, and the WebUI.
- **Python is reduced to a thin stateless AI micro-service** (`:7862`) that only wraps litellm — the one thing Rust cannot replace (100+ provider coverage).
- **All UIs receive typed JSON objects, never JSON strings** — no manual parsing anywhere.
- **Honest service status**: `GET /api/services/status` reports real state (child process alive + port responding + AI readiness), distinguishing Down / port conflict / misconfigured — never a fake "healthy".

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full layer breakdown, REST API reference, ports, and data flows.

## Features

### Model Management
- Register cloud models (DashScope, OpenAI, Anthropic) and local models (Ollama)
- Track token usage and cost per model in real-time
- Set budgets with configurable duration (daily/weekly/monthly)
- Automatic cost calculation based on registered pricing

### Smart Routing
- **Two-layer classification**: Regex rules (zero cost) first, LLM fallback second
- **Fallback chains**: 5-level failover (qwen3-max → plus → qwen-plus → flash → local)
- **Inference model support**: Auto-enables streaming for inference models
- **Domain enhancement**: STEM → math_logic, CS/engineering → coding
- **Cost-aware selection**: Picks the cheapest model that can handle the task

### Task Orchestration
- **Complexity detection**: 6 regex rules + length/sentence count heuristics
- **Task decomposition**: LLM-based subtask splitting with dependency tracking
- **Sequential execution**: Subtasks run in order with context injection
- **Result aggregation**: LLM synthesizes subtask outputs into a cohesive answer
- **SSE streaming**: Real-time event stream (decompose → task_start → task_done → result)

### Security Layer
- **PII Detection** (7 types): Email, phone, SSN, credit card, IP, ID card, bank account
- **Jailbreak Interception** (5 types): DAN, instruction override, role manipulation, safety bypass, prompt injection
- **Domain Classification**: 14 MMLU categories with keyword pre-filter + LLM fallback

### Semantic Cache
- ChromaDB vector similarity search (cosine, 0.95 threshold, 24h TTL)
- Returns cached responses for repeated simple Q&A (zero cost)
- Graceful degradation when embedding model unavailable

### UIs
- **WebUI** — browser UI at `http://localhost:7861/`
- **Tauri desktop GUI** — window + system tray, uses the same REST API
- **Honest service management** — start/stop Ollama and the AI service with real status reporting

## Quick Start

### Option A: Download the App

1. Download the latest release from [GitHub Releases](https://github.com/citrus-dot/LLooM/releases)
2. Launch it (or the bundled `.deb`/`.rpm`)
3. Configure API keys in Settings → API Keys
4. Start chatting

### Option B: Development Mode

```bash
git clone -b v2 https://github.com/citrus-dot/LLooM.git
cd LLooM

# Install Python dependencies (Python AI micro-service)
pip install -e ".[dev]"

# Install Tauri frontend deps
cd tauri-app && npm install && cd ..

# Copy and edit environment
cp .env.example .env
# Edit .env with your API keys

# Run the Rust server (headless, Web UI on :7861)
cd tauri-app/src-tauri && cargo run -- --headless

# Or start the Tauri GUI (also starts the same Rust server)
cd tauri-app && npx tauri dev
```

The Rust server (`:7861`) is the single entry point. It spawns the Python AI micro-service (`:7862`) and Ollama (`:11434`) automatically.

### Option C: Build the Release Bundle

```bash
# Full build (AI service PyInstaller + Ollama + Tauri bundle)
bash scripts/build.sh

# Or step by step:
bash scripts/build.sh --skip-ai       # skip AI micro-service packaging
bash scripts/build.sh --skip-ollama   # skip Ollama download
bash scripts/build.sh --skip-tauri    # skip Tauri bundling (Rust + AI only)
```

Build outputs:
- `dist/ai-service/ai-service` — standalone AI micro-service (~26MB, wraps litellm)
- `tauri-app/src-tauri/target/release/bundle/` — desktop app (deb/rpm on Linux, app/dmg on macOS)

The Rust binary is the primary artifact; the AI micro-service is bundled into
the app's resources as a standalone executable (no system Python needed).

### Smoke Test

```bash
bash scripts/smoke_test.sh
```

Covers 19 checks: health, service status, AI self-check, model registration, chat, orchestration, usage, conversation CRUD, budgets, and service restart.

## Configuration

All configuration is via environment variables in `.env`:

| Key | Default | Description |
|-----|---------|-------------|
| `DASHSCOPE_API_KEY` | (empty) | Alibaba DashScope API key |
| `DASHSCOPE_API_BASE` | `https://dashscope.aliyuncs.com/compatible-mode/v1` | DashScope endpoint |
| `OPENAI_API_KEY` | (empty) | OpenAI API key |
| `OPENAI_BASE_URL` | (empty) | OpenAI base URL override |
| `ANTHROPIC_API_KEY` | (empty) | Anthropic API key |
| `LLOOM_WEB_PORT` | `7861` | Rust server + Web UI port |
| `LLOOM_AI_SERVICE_URL` | `http://localhost:7862` | Python AI micro-service URL |
| `LLOOM_DATA_DIR` | `./data` | Data directory (SQLite, conversations) |
| `OLLAMA_API_BASE` | `http://localhost:11434` | Ollama endpoint |

## REST API

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/health` | Health check |
| GET | `/api/models` | List all models |
| POST | `/api/models` | Register a new model |
| GET/PUT/DELETE | `/api/models/{name}` | Get/update/delete a model |
| GET | `/api/usage` | Usage statistics |
| GET | `/api/budgets` | List budgets |
| POST | `/api/budgets` | Create/update a budget |
| GET | `/api/budgets/check` | Check budget status |
| GET/POST | `/api/config` | Read/write .env config |
| GET | `/api/stats` | Dashboard statistics |
| POST | `/api/chat/stream` | Chat with SSE streaming |
| POST | `/api/orchestrate/stream` | Task orchestration with SSE |
| GET/POST/DELETE | `/api/conversations` | Conversation CRUD |
| GET | `/api/services/status` | Honest service status |
| POST | `/api/services/{name}/start` | Start a service (ollama/ai) |
| POST | `/api/services/{name}/stop` | Stop a service |
| POST | `/api/services/{name}/restart` | Restart a service |
| GET | `/api/services/{name}/logs` | Service logs |
| POST | `/api/services/smart-restart` | Restart AI service after config change |
| POST | `/api/system/open-folder` | Open a folder |
| POST | `/api/system/open-web` | Open a URL |
| POST | `/api/system/cli` | Run the CLI |

## Tech Stack

| Component | Technology | Purpose |
|-----------|-----------|---------|
| API Server | **Rust + axum 0.8** | Primary REST + SSE server, all business logic |
| Async runtime | tokio | Event loop, async HTTP |
| Database | SQLite (WAL mode, rusqlite) | Model registry, usage tracking, budgets |
| LLM API | litellm SDK (Python) | Unified interface for all LLM providers |
| AI micro-service | FastAPI + Uvicorn | Stateless wrapper around litellm |
| Vector Cache | ChromaDB (PersistentClient) | Semantic cache for Q&A |
| HTTP client | reqwest 0.13 | Async calls to AI service / probes |
| Regex | fancy-regex 0.19 | PII/jailbreak/domain patterns (lookaround support) |
| Desktop | Tauri v2 (Rust) | Window + system tray, frontend over REST |
| Local LLM | Ollama | Zero-cost fallback model runtime |

## Project Structure

```
LLooM/
├── tauri-app/src-tauri/src/      # Rust core + server
│   ├── main.rs                   # Entry: GUI/headless + system tray
│   ├── server.rs                 # axum REST server
│   ├── db.rs                     # SQLite layer
│   ├── router.rs                 # Task classification + model selection
│   ├── security.rs               # Regex security (PII/jailbreak/domain)
│   ├── ai_client.rs              # AI micro-service async client
│   ├── processes.rs              # Child-process management
│   ├── conversations.rs          # Conversation file CRUD
│   ├── models.rs                 # Type definitions
│   ├── config.rs                 # Paths / ports / env
│   └── error.rs                  # Unified error type
├── tauri-app/src-tauri/ui/       # WebUI frontend (index.html SPA)
├── api/
│   └── ai_service.py             # Python AI micro-service (litellm wrapper)
├── scripts/
│   ├── smoke_test.sh             # 19-check smoke test
│   ├── build.sh                  # Build pipeline
│   └── download_ollama.sh        # Ollama binary download
├── ARCHITECTURE.md               # Layer breakdown + REST reference
├── pyproject.toml                # Python project config (AI service)
└── .env.example                  # Environment template
```

## License

MIT
