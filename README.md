# LLooM v2 — Intelligent LLM Routing Platform

A self-contained desktop application that manages multiple LLM models, routes requests intelligently based on task type, tracks token usage and costs, and provides security filtering — all without external infrastructure.

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

### Desktop GUI (Tauri)
- 5-page interface: Overview, Usage, Chat, Models, Settings
- Process management: Start/stop API server and Ollama from the UI
- API key configuration with smart restart (auto-reloads server on config change)
- Conversation history persistence (local JSON files)
- System tray with quick access

## Architecture

```
LLooM.app (308MB, self-contained)
├── Tauri Binary (Rust backend)
│   ├── Process management (spawn API + Ollama)
│   ├── API proxy (curl-based, avoids mixed content)
│   ├── Conversation CRUD
│   └── System tray
├── Python Core (PyInstaller bundle, 222MB)
│   ├── FastAPI Server (port 7860, REST + SSE)
│   ├── SmartRouter (classification + routing)
│   ├── TaskOrchestrator (decompose + aggregate)
│   ├── Security (PII + jailbreak + domain)
│   ├── SemanticCache (ChromaDB)
│   ├── ModelManager (CRUD + usage + budget)
│   └── litellm SDK (unified LLM API)
├── Ollama Binary (63MB, local LLM runtime)
└── Resources (scripts, .env template)
```

**Zero external dependencies**: SQLite replaces PostgreSQL, ChromaDB replaces Qdrant, embedded Python replaces Docker, bundled Ollama replaces system install.

## Quick Start

### Option A: Download the App

1. Download `LLooM.app` from [GitHub Releases](https://github.com/citrus-dot/LLooM/releases)
2. Drag to Applications folder
3. Double-click to launch
4. Configure API keys in Settings → API Keys
5. Start chatting

### Option B: Build from Source

#### Prerequisites

- Python 3.10+ with pip
- Rust toolchain (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- Node.js 18+ and npm
- Xcode Command Line Tools (`xcode-select --install`)
- [Ollama](https://ollama.com) installed (for local model support)

#### Build Steps

```bash
git clone -b v2 https://github.com/citrus-dot/LLooM.git
cd LLooM

# Install Python dependencies
pip install -e ".[dev]"

# Run the full build
bash scripts/build.sh

# Or build step by step:
bash scripts/build.sh --skip-ollama   # Skip Ollama download
bash scripts/build.sh --skip-tauri    # Skip Tauri build (PyInstaller only)
bash scripts/build.sh --skip-pyinstaller  # Skip Python packaging
```

The build produces:
- `dist/lloom-server/` — PyInstaller bundle (222MB)
- `tauri-app/src-tauri/target/release/bundle/macos/LLooM.app` — Final app (308MB)

### Option C: Development Mode

```bash
git clone -b v2 https://github.com/citrus-dot/LLooM.git
cd LLooM

# Install dependencies
pip install -e ".[dev]"
cd tauri-app && npm install && cd ..

# Copy and edit environment
cp .env.example .env
# Edit .env with your API keys

# Run tests
python3 tests/test_phase1.py  # 37 tests
python3 tests/test_phase2.py  # 64 tests
python3 tests/test_phase4.py  # 115 tests
python3 tests/test_phase5.py  # 78 tests
python3 tests/test_phase6.py  # 55 tests

# Start API server
python3 -m uvicorn api.server:app --port 7860

# Start Tauri dev mode
cd tauri-app && npx tauri dev
```

## Configuration

All configuration is via environment variables in `.env`:

| Key | Default | Description |
|-----|---------|-------------|
| `DASHSCOPE_API_KEY` | (empty) | Alibaba DashScope API key |
| `DASHSCOPE_API_BASE` | `https://dashscope.aliyuncs.com/compatible-mode/v1` | DashScope endpoint |
| `OPENAI_API_KEY` | (empty) | OpenAI API key |
| `OPENAI_BASE_URL` | (empty) | OpenAI base URL override |
| `ANTHROPIC_API_KEY` | (empty) | Anthropic API key |
| `LLOOM_API_PORT` | `7860` | FastAPI server port |
| `LLOOM_DATA_DIR` | `./data` | Data directory (SQLite, ChromaDB, conversations) |
| `OLLAMA_API_BASE` | `http://localhost:11434` | Ollama endpoint |

### Default Models

7 models pre-seeded with pricing data:

| Model | Provider | Task Type | Input Cost (per 1K tokens) | Output Cost |
|-------|----------|-----------|---------------------------|------------|
| qwen-plus | DashScope | general | $0.005 | $0.02 |
| qwen3.6-flash | DashScope | classification | $0.001 | $0.003 |
| qwen3.6-plus | DashScope | coding | $0.008 | $0.02 |
| qwen3-max | DashScope | math_logic | $0.02 | $0.06 |
| deepseek-v3 | DashScope | general | $0.002 | $0.008 |
| qwen2.5-local | Ollama | fallback | Free | Free |
| gpt-4o | OpenAI | general | $0.005 | $0.015 |

### Default Budget

- Max budget: $10
- Duration: 30 days
- Upper bound: $1000 / 365 days

## API Endpoints

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

## CLI Usage

```bash
# Initialize database
python3 cli/lloom.py init

# List models
python3 cli/lloom.py model list

# Add a model
python3 cli/lloom.py model add --name my-model --provider openai --litellm-model openai/gpt-4o

# View status
python3 cli/lloom.py status

# Chat
python3 cli/lloom.py chat "What is 2+2?"

# Orchestrate complex task
python3 cli/lloom.py orchestrate "Write a Python web scraper and explain how it works"

# Start API server
python3 cli/lloom.py serve
```

## Tech Stack

| Component | Technology | Purpose |
|-----------|-----------|---------|
| LLM API | litellm SDK | Unified interface for all LLM providers |
| API Server | FastAPI + Uvicorn | REST + SSE endpoints |
| Database | SQLite (WAL mode) | Model registry, usage tracking, budgets |
| Vector Cache | ChromaDB (PersistentClient) | Semantic cache for Q&A |
| CLI | Click | Developer-friendly command interface |
| Desktop | Tauri v2 (Rust) | Process management + native GUI |
| Packaging | PyInstaller | Bundles Python runtime + all deps |
| Local LLM | Ollama (bundled binary) | Zero-cost fallback model runtime |

## Project Structure

```
LLooM/
├── core/                    # Business logic
│   ├── config.py            # Environment configuration
│   ├── database.py          # SQLite CRUD (models, usage, budgets)
│   ├── model_manager.py     # Model lifecycle + cost calculation
│   ├── smart_router.py      # Two-layer classification + routing
│   ├── orchestrator.py      # Task decomposition + aggregation
│   ├── cache.py             # ChromaDB semantic cache
│   ├── security.py          # PII + jailbreak + domain classification
│   ├── callbacks.py         # litellm usage tracking callback
│   └── seed_models.py       # Default model pricing data
├── api/
│   └── server.py            # FastAPI REST + SSE server
├── cli/
│   └── lloom.py             # Click CLI (7 commands)
├── tauri-app/
│   ├── src-tauri/
│   │   ├── src/main.rs      # Rust backend (24 Tauri commands)
│   │   ├── ui/index.html    # 5-page SPA frontend
│   │   ├── tauri.conf.json  # Tauri bundle config
│   │   └── resources/       # Bundled PyInstaller + Ollama
│   └── package.json
├── tests/                   # 6-phase test suite (401 tests)
├── scripts/
│   ├── build.sh             # Full build pipeline
│   ├── download_ollama.sh   # Ollama binary download
│   └── first_run_setup.py   # First-run DB init + model pull
├── lloom_server.py          # PyInstaller entry point
├── lloom.spec               # PyInstaller spec
├── pyproject.toml           # Python project config
├── .env.example             # Environment template
└── progress.md              # Development progress tracker
```

## License

MIT
