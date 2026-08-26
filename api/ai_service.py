"""LLooM AI micro-service — the only remaining Python layer.

Stateless as to business data: the Rust host passes explicit model params
(litellm_model, api_base, api_key, pricing) and the conversation context with
every request. Python owns only the LLM calls, the two-layer cache stores
(exact sqlite + chroma vectors) and the context compressor; conversation
records live in the Rust host's SQLite (`lloom.db`).

Endpoints:
  GET  /v1/health                 liveness
  POST /v1/chat                   single non-streaming LLM call
  POST /v1/chat/stream            streaming LLM call (SSE)
  POST /v1/classify               LLM-based task classification
  POST /v1/domain                 LLM-based domain classification
  POST /v1/orchestrate/stream     full task orchestration (SSE)
"""

import json
import os
import re
import sys
import time
import hashlib
import sqlite3
import threading
from dataclasses import dataclass, field
from enum import Enum
from typing import Any
from collections.abc import Iterator

import litellm
from fastapi import FastAPI
from fastapi.responses import StreamingResponse
from pydantic import BaseModel

# Embedding-model provisioner. Imported three different ways depending on how
# this service was launched: as `api.ai_service` under uvicorn (dev), as a bare
# script from `resources/` (installed), or from a PyInstaller bundle.
try:
    from api import embedding_model as embed_model
except ImportError:  # pragma: no cover - depends on launch mode
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import embedding_model as embed_model

# ── Token counting (context budgeting) ──
# Point tiktoken at the repo's offline cache BEFORE importing it, so no encoder
# download is attempted in restricted-network environments. Falls back to a
# char-based approximation when the cache (or tiktoken itself) is unavailable.
_TIKTOKEN_CACHE = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "tiktoken_cache"
)
if os.path.isdir(_TIKTOKEN_CACHE):
    os.environ.setdefault("TIKTOKEN_CACHE_DIR", _TIKTOKEN_CACHE)

try:
    import tiktoken as _tiktoken
except Exception:  # pragma: no cover - tiktoken ships with litellm anyway
    _tiktoken = None

_ENCODER: Any = None
_ENCODER_TRIED = False


def _encoder():
    global _ENCODER, _ENCODER_TRIED
    if not _ENCODER_TRIED:
        _ENCODER_TRIED = True
        if _tiktoken is not None:
            try:
                _ENCODER = _tiktoken.get_encoding("cl100k_base")
            except Exception:
                _ENCODER = None
    return _ENCODER


def count_tokens(text: str) -> int:
    """Token count for budgeting. tiktoken cl100k when available; otherwise a
    CJK-aware character approximation (CJK ≈ 1 token, latin ≈ 3.5 chars/token)."""
    if not text:
        return 0
    enc = _encoder()
    if enc is not None:
        try:
            return len(enc.encode(text, disallowed_special=()))
        except Exception:
            pass
    cjk = sum(1 for ch in text if "\u4e00" <= ch <= "\u9fff")
    return int(cjk + (len(text) - cjk) / 3.5)


# Prompt-token budget for [summary + history]. The current query and system
# prompt are always kept outside this budget. Configurable via .env.
CONTEXT_BUDGET = int(os.getenv("LLOOM_CONTEXT_BUDGET", "24000"))

# Rolling-summary update policy: re-summarize only when at least this many
# uncovered messages have fallen out of the kept window. Between updates the
# summary prefix stays byte-stable, which also maximizes provider-side
# prefix-cache hits (CONTEXT-PLAN §3.2).
SUMMARY_BLOCK = int(os.getenv("LLOOM_SUMMARY_BLOCK", "6"))

app = FastAPI(title="LLooM AI Service", version="2.0.0")


# ── Request models ──


class ModelSpec(BaseModel):
    name: str
    litellm_model: str
    api_base: str = ""
    api_key: str = ""
    input_cost_per_token: float = 0.0
    output_cost_per_token: float = 0.0


class ChatRequest(BaseModel):
    model: ModelSpec
    messages: list[dict]
    max_tokens: int = 500
    temperature: float = 0.3
    timeout: int = 60


class ClassifyRequest(BaseModel):
    text: str
    classifier: ModelSpec
    system_prompt: str = ""
    valid_types: list[str] = []
    max_tokens: int = 20
    timeout: int = 10


class DomainRequest(BaseModel):
    text: str
    classifier: ModelSpec
    system_prompt: str = ""
    timeout: int = 10


class OrchestrateRequest(BaseModel):
    query: str
    history: list[dict] = field(default_factory=list)
    sr_domain: str = ""
    models: list[ModelSpec] = field(default_factory=list)
    # Optional semantic-cache dir; empty disables caching
    cache_dir: str = ""
    similarity_threshold: float = 0.80
    ttl: int = 86400
    # Server-side context building (CONTEXT-PLAN §3.2): the Rust host loads the
    # conversation, computes a rolling-summary fingerprint, and passes both the
    # persisted summary text and how many leading messages it covers. Empty
    # `conversation_id` disables cache namespacing + context fingerprinting.
    conversation_id: str = ""
    summary: str = ""
    summary_upto: int = 0
    # P0.f: Rust 统一决策结果（role -> model name）。Python 优先用，缺则回落 models[0]，
    # 从而彻底移除 TASK_MODEL_PREFERENCE / DECOMPOSER_PREFERENCE 硬编码真源。
    assignments: dict = field(default_factory=dict)


# ── Helpers ──


def _litellm_kwargs(model: ModelSpec, **extra) -> dict[str, Any]:
    kwargs: dict[str, Any] = {"model": model.litellm_model}
    if model.api_base:
        kwargs["api_base"] = model.api_base
    if model.api_key:
        kwargs["api_key"] = model.api_key
    kwargs.update(extra)
    return kwargs


def _estimate_cost(model: ModelSpec, input_tokens: int, output_tokens: int) -> float:
    return (
        input_tokens * model.input_cost_per_token
        + output_tokens * model.output_cost_per_token
    )


def _usage_detail(usage) -> dict:
    """Extract full usage breakdown from a litellm response. Never raises.

    PRICING-PLAN §4.3 — the Rust side prices from these components; Python no
    longer computes cost itself. `field_missing` records providers that do not
    report cached_tokens (calibration input, not an error).
    """
    if usage is None:
        return {
            "prompt_tokens": 0,
            "completion_tokens": 0,
            "cached_tokens": 0,
            "reasoning_tokens": 0,
            "cache_creation_tokens": 0,
            "field_missing": True,
        }
    ptd = getattr(usage, "prompt_tokens_details", None)
    ctd = getattr(usage, "completion_tokens_details", None)
    cached = getattr(ptd, "cached_tokens", None)
    return {
        "prompt_tokens": getattr(usage, "prompt_tokens", 0) or 0,
        "completion_tokens": getattr(usage, "completion_tokens", 0) or 0,
        "cached_tokens": cached or 0,
        "reasoning_tokens": getattr(ctd, "reasoning_tokens", 0) or 0,
        "cache_creation_tokens": getattr(usage, "cache_creation_input_tokens", 0) or 0,
        "field_missing": cached is None,
    }


def _model_by_name(models: list[ModelSpec], name: str) -> ModelSpec | None:
    for m in models:
        if m.name == name:
            return m
    return None


def _sse(event: str, data: dict) -> str:
    return f"event: {event}\ndata: {json.dumps(data, ensure_ascii=False)}\n\n"


def _plain_sse(data: dict) -> str:
    return f"data: {json.dumps(data, ensure_ascii=False)}\n\n"


# ── Semantic cache (optional, in-process ChromaDB) ──

# Whether the embedding model is available. The SemanticCache stays disabled
# until this is True, so the default (cold-start) path never triggers chroma's
# own ~79MB download from its S3 bucket (measured 6 KB/s here — hours, or never).
#
# Seeded from disk: once the model has been provisioned, a service restart
# re-enables the semantic cache automatically instead of making the user click
# "初始化" again.
_cache_ready = embed_model.is_provisioned()

# In-memory state for the cache-init workflow, polled by the frontend.
_cache_init: dict[str, Any] = {
    "status": "idle",        # idle | running | done | error | timeout
    "started_at": 0.0,       # epoch seconds
    "finished_at": 0.0,
    "detail": "",
    "error": "",
}
_CACHE_INIT_TIMEOUT = 300.0  # seconds before the status endpoint flags a timeout


class SemanticCache:
    """Optional chroma-backed semantic cache (L2 in the two-layer design).

    Singleton across requests: chroma's PersistentClient is expensive to
    construct and the underlying sqlite is lock-guarded, so one instance +
    a threading.Lock is both faster and safer than per-request clients.

    Scope rules (CONTEXT-PLAN §3.3 C-a):
      - context-free queries → global namespace (where model=<m>, conv_id IS NULL)
      - context-dependent queries → per-conversation namespace
        (where model=<m> AND conv_id=<cid>)
    """

    _singleton: "SemanticCache | None" = None
    _singleton_key: tuple = ()
    _lock = threading.Lock()

    def __init__(self, path: str, threshold: float, ttl: int):
        self.path = path
        self.threshold = threshold
        self.ttl = ttl
        self._collection = None
        self._enabled = False
        if not path or not _cache_ready:
            return
        try:
            import chromadb
            from chromadb.config import Settings as ChromaSettings

            client = chromadb.PersistentClient(
                path=path,
                settings=ChromaSettings(anonymized_telemetry=False),
            )
            self._collection = client.get_or_create_collection(
                name="lloom_cache",
                metadata={"hnsw:space": "cosine"},
            )
            self._enabled = True
        except Exception:
            self._enabled = False

    @classmethod
    def get(cls, path: str, threshold: float, ttl: int) -> "SemanticCache | None":
        key = (path, round(threshold, 4), ttl)
        with cls._lock:
            if cls._singleton is None or cls._singleton_key != key:
                cls._singleton = cls(path, threshold, ttl)
                cls._singleton_key = key
            return cls._singleton if cls._singleton._enabled else None

    def _where(self, model: str, conv_id: str | None) -> dict | None:
        if model == "default" and not conv_id:
            return None
        clauses = []
        if model != "default":
            clauses.append({"model": model})
        if conv_id:
            clauses.append({"conv_id": conv_id})
        if not clauses:
            return None
        return clauses[0] if len(clauses) == 1 else {"$and": clauses}

    def lookup(self, query: str, model: str, conv_id: str | None) -> dict | None:
        if not self._enabled or not self._collection:
            return None
        try:
            with self._lock:
                results = self._collection.query(
                    query_texts=[query],
                    n_results=1,
                    where=self._where(model, conv_id),
                )
            if not results or not results["ids"] or not results["ids"][0]:
                return None
            similarity = 1 - results["distances"][0][0]
            if similarity < self.threshold:
                return None
            meta = results["metadatas"][0][0]
            if self.ttl > 0 and (time.time() - meta.get("cached_at", 0)) > self.ttl:
                return None
            return {
                "response": meta.get("response", ""),
                "similarity": similarity,
            }
        except Exception:
            return None

    def best_sim(self, query: str, model: str, conv_id: str | None = None) -> float | None:
        """Top-1 cosine similarity to the nearest cached query, ignoring the
        threshold. Used for calibration — we need the score even on a miss."""
        if not self._enabled or not self._collection:
            return None
        try:
            with self._lock:
                results = self._collection.query(
                    query_texts=[query],
                    n_results=1,
                    where=self._where(model, conv_id),
                )
            if not results or not results["ids"] or not results["ids"][0]:
                return None
            return 1 - results["distances"][0][0]
        except Exception:
            return None

    def store(self, query: str, response: str, model: str, conv_id: str | None) -> None:
        if not self._enabled or not self._collection:
            return
        # conv_id in the id makes global vs scoped entries distinct on disk;
        # global entries (conv_id None) dedupe across conversations.
        doc_id = hashlib.md5(
            f"{model}:{'g' if conv_id is None else conv_id}:{query}".encode()
        ).hexdigest()
        meta: dict[str, Any] = {
            "response": response,
            "model": model,
            "cached_at": time.time(),
        }
        if conv_id:
            meta["conv_id"] = conv_id
        try:
            with self._lock:
                self._collection.upsert(
                    ids=[doc_id],
                    documents=[query],
                    metadatas=[meta],
                )
        except Exception:
            pass

    def sweep(self) -> int:
        """Drop TTL-expired entries and evict the oldest beyond the size cap.
        Best-effort; failures are non-fatal (cache is best-effort)."""
        if not self._enabled or not self._collection:
            return 0
        max_entries = int(os.getenv("LLOOM_CACHE_MAX_ENTRIES", "5000"))
        removed = 0
        try:
            with self._lock:
                all_rows = self._collection.get(include=["metadatas"])
                ids = all_rows.get("ids", [])
                metas = all_rows.get("metadatas", [])
                now = time.time()
                expired = [
                    ids[i]
                    for i, m in enumerate(metas)
                    if self.ttl > 0 and (now - m.get("cached_at", 0)) > self.ttl
                ]
                if expired:
                    self._collection.delete(ids=expired)
                    removed += len(expired)
                # LRU cap: keep newest max_entries by cached_at.
                remaining = self._collection.get(include=["metadatas"])
                r_ids = remaining.get("ids", [])
                r_metas = remaining.get("metadatas", [])
                if len(r_ids) > max_entries:
                    order = sorted(
                        range(len(r_ids)), key=lambda i: r_metas[i].get("cached_at", 0)
                    )
                    drop = [r_ids[i] for i in order[: len(r_ids) - max_entries]]
                    if drop:
                        self._collection.delete(ids=drop)
                        removed += len(drop)
        except Exception:
            pass
        return removed


# ── Exact-match cache (L1): O(1) hash lookup, zero false positives ──


class ExactCache:
    """SQLite key→response store. Key = sha256(model + system_id + fingerprint
    + normalized_query). Cross-conversation sharing for context-free queries
    (fingerprint empty), per-conversation otherwise. Own sqlite file, separate
    from the Rust host's business DB."""

    _singleton: "ExactCache | None" = None
    _lock = threading.Lock()

    def __init__(self, path: str, ttl: int):
        self.path = path
        self.ttl = ttl
        try:
            os.makedirs(os.path.dirname(path), exist_ok=True)
            self._conn = sqlite3.connect(path, check_same_thread=False)
            self._conn.execute(
                "CREATE TABLE IF NOT EXISTS exact_cache ("
                " key TEXT PRIMARY KEY, model TEXT, response TEXT, "
                " conv_id TEXT, created_at REAL, hits INTEGER DEFAULT 0)"
            )
            self._conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_exact_created ON exact_cache(created_at)"
            )
            self._conn.commit()
            self._enabled = True
        except Exception:
            self._enabled = False

    @classmethod
    def get(cls, path: str, ttl: int) -> "ExactCache | None":
        with cls._lock:
            if cls._singleton is None or cls._singleton.path != path:
                cls._singleton = cls(path, ttl)
            return cls._singleton if cls._singleton._enabled else None

    def lookup(self, key: str) -> str | None:
        if not self._enabled:
            return None
        try:
            with self._lock:
                row = self._conn.execute(
                    "SELECT response, created_at FROM exact_cache WHERE key = ?",
                    (key,),
                ).fetchone()
                if not row:
                    return None
                if self.ttl > 0 and (time.time() - (row[1] or 0)) > self.ttl:
                    return None
                self._conn.execute(
                    "UPDATE exact_cache SET hits = hits + 1 WHERE key = ?", (key,)
                )
                self._conn.commit()
                return row[0]
        except Exception:
            return None

    def store(self, key: str, model: str, response: str, conv_id: str | None) -> None:
        if not self._enabled:
            return
        try:
            with self._lock:
                self._conn.execute(
                    "INSERT OR REPLACE INTO exact_cache "
                    "(key, model, response, conv_id, created_at, hits) "
                    "VALUES (?, ?, ?, ?, ?, 0)",
                    (key, model, response, conv_id, time.time()),
                )
                self._conn.commit()
        except Exception:
            pass

    def sweep(self) -> int:
        if not self._enabled:
            return 0
        max_entries = int(os.getenv("LLOOM_CACHE_MAX_ENTRIES", "5000"))
        removed = 0
        try:
            with self._lock:
                if self.ttl > 0:
                    c = self._conn.execute(
                        "DELETE FROM exact_cache WHERE created_at < ?",
                        (time.time() - self.ttl,),
                    )
                    removed += c.rowcount
                n = self._conn.execute("SELECT COUNT(*) FROM exact_cache").fetchone()[0]
                if n > max_entries:
                    c = self._conn.execute(
                        "DELETE FROM exact_cache WHERE key IN ("
                        " SELECT key FROM exact_cache ORDER BY created_at ASC LIMIT ?)",
                        (n - max_entries,),
                    )
                    removed += c.rowcount
                self._conn.commit()
        except Exception:
            pass
        return removed


# Background cache-eviction thread (runs every 5 min, daemon).
_EVICTION_PERIOD = int(os.getenv("LLOOM_CACHE_SWEEP_SECS", "300"))
_eviction_started = False


def _start_eviction_thread(exact: ExactCache | None, sem: SemanticCache | None) -> None:
    global _eviction_started
    if _eviction_started or (not exact and not sem):
        return
    _eviction_started = True

    def _loop():
        while True:
            time.sleep(_EVICTION_PERIOD)
            if exact:
                exact.sweep()
            if sem:
                sem.sweep()

    threading.Thread(target=_loop, daemon=True).start()


# ── Cache key helpers (CONTEXT-PLAN §3.3) ──

# Anaphora / deictic markers: when present in a follow-up question, the query
# depends on prior context and must NOT be served from the global cache.
_ANAPHORA = re.compile(
    r"(它|他|她|它们|他们|她们|这个|那个|这些|那些|上面|上文|刚才|之前|"
    r"继续|再说|再讲|再说一遍|另外|那么|此|该|前面|上述|以上|接下来|这样|那样)"
)
# Time-sensitive queries should never be cached (stale answers).
_TIME_SENSITIVE = re.compile(r"(今天|现在|当前|最新|最近|today|now|latest)")


def _normalize_query(q: str) -> str:
    return re.sub(r"\s+", " ", q.strip().lower())


def _is_context_free(query: str, history: list[dict]) -> bool:
    """Decide whether a query can be served from the *global* (cross-conversation)
    cache. Heuristic v1 (no extra LLM cost): empty history → free; anaphora →
    bound; very short follow-ups → bound (conservative). The gray zone is left
    to the calibration loop to label."""
    if not history:
        return True
    if _ANAPHORA.search(query):
        return False
    # Short follow-ups without an explicit subject are almost always contextual.
    if len(query) < 12:
        return False
    return True


def _fingerprint(conv_id: str, history: list[dict]) -> str:
    """Stable digest of the conversation tail (last 2 messages) — the part a
    context-dependent query is most likely to refer back to."""
    tail = "".join(m.get("content", "")[:80] for m in history[-2:])
    return hashlib.sha256(f"{conv_id}:{tail}".encode()).hexdigest()[:16]


def _exact_key(model: str, system_id: str, fingerprint: str, query: str) -> str:
    raw = f"{model}|{system_id}|{fingerprint}|{_normalize_query(query)}"
    return hashlib.sha256(raw.encode()).hexdigest()


def _cacheable(query: str, temperature: float) -> bool:
    """Gate writes: skip non-deterministic or time-sensitive answers."""
    if temperature > 0.7:
        return False
    if _TIME_SENSITIVE.search(query):
        return False
    return True


# Back-compat shims: the legacy module-level functions in this file called
# SemanticCache(query, model) / .put(query, response, model). We route them
# through the singleton so existing call sites keep compiling.
def _semantic_for(cache_dir: str, threshold: float, ttl: int) -> SemanticCache | None:
    return SemanticCache.get(cache_dir, threshold, ttl)


# ── Semantic-cache pre-initialization ──


@app.post("/v1/cache/init")
def cache_init() -> dict:
    """Provision the embedding model in a background thread, then warm chroma.

    Downloads the model ourselves from a fast, checksum-verified mirror (see
    `embedding_model`) instead of letting chroma pull it from its S3 bucket.
    Returns immediately; poll /v1/cache/status for byte-level progress. The
    SemanticCache stays disabled until this completes successfully.
    """
    import threading

    global _cache_ready, _cache_init

    if _cache_init["status"] == "running":
        return {"status": "running", "detail": "initialization already in progress"}
    if _cache_ready:
        return {"status": "done", "detail": "already initialized"}

    _cache_init = {
        "status": "running",
        "started_at": time.time(),
        "finished_at": 0.0,
        "detail": "fetching embedding model from mirror...",
        "error": "",
    }

    def _run():
        global _cache_ready, _cache_init
        try:
            # 1. Fetch + checksum-verify the six ONNX files chroma expects.
            res = embed_model.provision()
            # 2. Prove the model actually drives chroma's pipeline correctly
            #    (right dims, L2-normalised, sane semantic ordering) — a valid
            #    checksum alone doesn't prove it's the right kind of export.
            checks = embed_model.verify_model()
            # 3. Warm the real collection so the first query isn't a cold start.
            import chromadb
            from chromadb.config import Settings as ChromaSettings

            cache_path = os.getenv(
                "LLOOM_CACHE_DIR",
                os.path.join(os.getenv("LLOOM_DATA_DIR", "data"), "chroma"),
            )
            client = chromadb.PersistentClient(
                path=cache_path,
                settings=ChromaSettings(anonymized_telemetry=False),
            )
            col = client.get_or_create_collection(
                name="lloom_cache",
                metadata={"hnsw:space": "cosine"},
            )
            col.upsert(ids=["__init__"], documents=["warmup"], metadatas=[{"init": True}])
            col.query(query_texts=["warmup"], n_results=1)
            col.delete(ids=["__init__"])

            _cache_ready = True
            source = res.get("mirror", "mirror")
            _cache_init = {
                "status": "done",
                "started_at": _cache_init["started_at"],
                "finished_at": time.time(),
                "detail": (
                    f"就绪（来源 {source}，{checks['dim']} 维，语义校验 "
                    f"{checks['similarity_related']:.2f} / "
                    f"{checks['similarity_unrelated']:.2f}）"
                ),
                "error": "",
            }
        except Exception as e:
            _cache_init = {
                "status": "error",
                "started_at": _cache_init["started_at"],
                "finished_at": time.time(),
                "detail": "",
                "error": str(e),
            }

    threading.Thread(target=_run, daemon=True).start()
    return {"status": "running", "detail": "initialization started"}


@app.get("/v1/cache/status")
def cache_status() -> dict:
    """Report cache-init progress. Flags a timeout if running too long."""
    global _cache_init
    elapsed = 0.0
    if _cache_init["started_at"]:
        end = _cache_init["finished_at"] or time.time()
        elapsed = end - _cache_init["started_at"]
    # Surface a timeout without killing the thread (Python threads aren't
    # cancelable); the frontend can offer cleanup once this is flagged.
    status = _cache_init["status"]
    if status == "running" and elapsed > _CACHE_INIT_TIMEOUT:
        status = "timeout"

    # Byte-level download progress, so the UI can show a real bar instead of
    # guessing from elapsed/timeout.
    prog = embed_model.progress()
    return {
        "status": status,
        "ready": _cache_ready,
        "elapsed": round(elapsed, 1),
        "timeout": _CACHE_INIT_TIMEOUT,
        "detail": _cache_init["detail"],
        "error": _cache_init["error"],
        "phase": prog["phase"],
        "mirror": prog["mirror"],
        "file": prog["file"],
        "percent": prog["percent"],
        "file_done": prog["file_done"],
        "file_total": prog["file_total"],
        "file_percent": prog["file_percent"],
        "done_bytes": prog["done_bytes"],
        "total_bytes": prog["total_bytes"],
        "speed_bps": round(prog["speed_bps"], 1),
    }


@app.post("/v1/cache/cleanup")
def cache_cleanup(purge_model: bool = False) -> dict:
    """Reset init state and drop the cached vectors so a fresh init can run.

    Does NOT kill an in-flight download thread (can't); call this only after the
    user confirms a timeout/failure. Flips _cache_ready back to False so the
    SemanticCache stops trying to use a half-broken store.

    The ~87MB embedding model is kept by default — it is the expensive part, and
    keeping it makes re-initialisation instant. Pass `purge_model=true` to also
    delete it and force a fresh download.
    """
    global _cache_ready, _cache_init
    import shutil

    _cache_ready = False
    _cache_init = {
        "status": "idle",
        "started_at": 0.0,
        "finished_at": 0.0,
        "detail": "",
        "error": "",
    }
    cache_path = os.getenv(
        "LLOOM_CACHE_DIR",
        os.path.join(os.getenv("LLOOM_DATA_DIR", "data"), "chroma"),
    )
    removed = False
    try:
        if os.path.isdir(cache_path):
            shutil.rmtree(cache_path)
            removed = True
    except Exception:
        pass
    # Always sweep download leftovers (staging dir, truncated S3 archive).
    purged = embed_model.purge(model=purge_model)
    return {
        "cleaned": True,
        "removed_dir": removed,
        "model_kept": not purge_model and embed_model.is_provisioned(),
        "purged": purged["removed"],
    }


# ── Task classification (LLM fallback) ──

DEFAULT_CLASSIFY_PROMPT = """你是一个任务分类器。将用户请求分类为以下类别之一，只返回类别名称，不要输出其他内容：

- simple_qa: 简单问答、问候、翻译、天气/时间查询
- general: 日常对话、摘要、一般性任务
- coding: 写代码、调试、编程问题、API 设计
- math_logic: 数学计算、逻辑推理、概率统计
- complex_reasoning: 深度分析、方案对比、研究综述、架构设计"""


@app.post("/v1/classify")
def classify(req: ClassifyRequest) -> dict:
    prompt = req.system_prompt or DEFAULT_CLASSIFY_PROMPT
    valid = req.valid_types or ["simple_qa", "general", "coding", "math_logic", "complex_reasoning"]
    try:
        response = litellm.completion(
            **_litellm_kwargs(
                req.classifier,
                messages=[
                    {"role": "system", "content": prompt},
                    {"role": "user", "content": req.text[:500]},
                ],
                max_tokens=req.max_tokens,
                timeout=req.timeout,
                temperature=0,
            )
        )
        content = response.choices[0].message.content.strip().lower()
        for t in valid:
            if t in content:
                return {"task_type": t}
    except Exception:
        pass
    return {"task_type": "general"}


# ── Domain classification (LLM fallback) ──

DEFAULT_DOMAIN_PROMPT = """你是领域分类器。将内容分类为以下MMLU领域之一，只返回领域名称：
physics, chemistry, biology, math, computer_science, engineering, medicine,
law, history, philosophy, economics, psychology, sociology, other"""


@app.post("/v1/domain")
def classify_domain(req: DomainRequest) -> dict:
    prompt = req.system_prompt or DEFAULT_DOMAIN_PROMPT
    try:
        response = litellm.completion(
            **_litellm_kwargs(
                req.classifier,
                messages=[
                    {"role": "system", "content": prompt},
                    {"role": "user", "content": req.text[:500]},
                ],
                max_tokens=20,
                timeout=req.timeout,
                temperature=0,
            )
        )
        content = response.choices[0].message.content.strip().lower()
        valid = ["physics", "chemistry", "biology", "math", "computer_science",
                 "engineering", "medicine", "law", "history", "philosophy",
                 "economics", "psychology", "sociology", "other"]
        for d in valid:
            if d in content:
                return {"domain": d}
    except Exception:
        pass
    return {"domain": ""}


# ── Single chat completion ──


@app.post("/v1/chat")
def chat(req: ChatRequest) -> dict:
    kwargs = _litellm_kwargs(
        req.model,
        messages=req.messages,
        max_tokens=req.max_tokens,
        temperature=req.temperature,
        timeout=req.timeout,
    )
    response = litellm.completion(**kwargs)
    content = response.choices[0].message.content or ""
    u = _usage_detail(getattr(response, "usage", None))
    return {
        "content": content,
        "input_tokens": u["prompt_tokens"],
        "output_tokens": u["completion_tokens"],
        "usage": u,
        "model": req.model.name,
    }


@app.post("/v1/chat/stream")
def chat_stream(req: ChatRequest) -> StreamingResponse:
    kwargs = _litellm_kwargs(
        req.model,
        messages=req.messages,
        max_tokens=req.max_tokens,
        temperature=req.temperature,
        timeout=req.timeout,
        stream=True,
        stream_options={"include_usage": True},
    )

    def gen():
        usage = None
        try:
            for chunk in litellm.completion(**kwargs):
                delta = chunk.choices[0].delta
                if delta and delta.content:
                    yield _plain_sse({"chunk": delta.content})
                # Providers that support include_usage attach usage on the last
                # chunk; capture it for the final "done" event.
                u = getattr(chunk, "usage", None)
                if u is not None:
                    usage = u
            u = _usage_detail(usage)
            yield _plain_sse({
                "done": True,
                "usage": u,
                "input_tokens": u["prompt_tokens"],
                "output_tokens": u["completion_tokens"],
            })
        except Exception as e:
            yield _plain_sse({"error": True, "detail": str(e)})

    return StreamingResponse(gen(), media_type="text/event-stream")


# ── Orchestration (decompose → execute → aggregate) ──


COMPLEXITY_INDICATORS = [
    r"(然后|接着|再|之后|最后).{2,}",
    r"(第[一二三四五1-5]步|Step\s?\d)",
    r"(同时|并且|此外|另外)",
    r"(对比|比较|分析|评估).+(和|与|跟|vs)",
    r"(写|实现|开发).+(并|然后|接着).*(测试|验证|部署)",
    r"(翻译|总结|摘要).+(并|然后).+(分析|评论)",
    # 序列/枚举标记：首先…其次…最后、第一/第二、1. 2. 等
    r"(首先|其次|然后|再次|最后|第一|第二|第三|第四|第五)",
    r"(\d+[\.、])\s*\S+.*(\d+[\.、])\s*\S+",
    # 多子任务提示词
    r"(分别|各自|逐一).{2,}(说明|分析|列出|给出|介绍|总结|处理)",
    # 显式对比/权衡
    r"(权衡|优缺点|利弊|方案).{2,}(对比|比较|选择)",
]

# 分解阶段需要一个「便宜 + 快」的模型做结构化抽取。P0.f 起模型由 Rust 决策
# （assignments.decompose）下发，不再在 Python 侧硬编码模型真源。

DECOMPOSE_SYSTEM_PROMPT = """你是一个任务分解专家。将用户的复杂任务分解为2-5个子任务。

规则：
1. 每个子任务应该是独立的、可执行的
2. 标注子任务之间的依赖关系（depends_on）
3. 为每个子任务选择合适的类型：simple_qa / general / coding / math_logic / complex_reasoning
4. 估算每个子任务的输出 token 数

只输出JSON数组，不要其他文字：
[
  {
    "id": 1,
    "description": "子任务描述",
    "task_type": "coding",
    "depends_on": [],
    "estimated_output_tokens": 200
  }
]"""

AGGREGATE_SYSTEM_PROMPT = """你是一个结果汇总专家。用户提出了一个复杂任务，已被分解为多个子任务并分别执行。
请将所有子任务的结果汇总成一个连贯、完整的最终回答。

要求：
1. 保持逻辑连贯，按子任务顺序组织
2. 去除冗余信息
3. 突出关键结论
4. 使用中文回答
5. 结合对话上下文，确保回答与之前的对话连贯
6. 严禁编造不存在的测试脚本、文件或可执行代码。如果子任务结果中包含代码示例，必须明确说明它来自子任务结果；如果子任务结果中没有代码，不要主动生成“附：一键运行测试脚本”之类的内容。
7. 如果某个子任务执行失败，结果中会包含“执行失败:”前缀，请如实说明该部分失败，不要替它编造内容或假装已完成。"""


# 比较/对比类关键词。这类问题往往需要把多个对象分别分析再综合，
# 因此当出现「比较/对比 + 多个并列实体」时应进入复杂路径做分解。
_COMPARE_KW = re.compile(r"(比较|对比|对照|区别|差异|异同|优缺点|利弊|对比分析|vs|VS|Vs)")
# 实体分隔符：顿号/逗号/斜杠/空格，以及「和/与/跟/vs」等连接词。
_ENTITY_SEP = re.compile(r"[、，,；;／/\s]+|(?:和|与|跟|vs|VS|Vs)")


def _is_comparison(query: str) -> bool:
    """含比较类关键词，且能切分出 ≥2 个并列实体，视为多对象比较 → 复杂任务。

    反例：纯「排序算法比较」「快速排序的优缺点」只有一个对象，不触发，
    保持单模型轻快直答（符合「混合：默认轻快」策略）。
    """
    if not _COMPARE_KW.search(query):
        return False
    entities = [e.strip() for e in _ENTITY_SEP.split(query) if len(e.strip()) >= 2]
    return len(entities) >= 2


def _is_complex(query: str) -> bool:
    for pattern in COMPLEXITY_INDICATORS:
        if re.search(pattern, query, re.IGNORECASE):
            return True
    if len(query) > 100:
        return True
    sentences = [s.strip() for s in re.split(r"[。！？.!?]", query) if s.strip()]
    if len(sentences) > 2:
        return True
    # 含多个换行/编号项也视为多子任务
    numbered = re.findall(r"^\s*(?:\d+[\.、]|[-*])\s+\S", query, re.MULTILINE)
    if len(numbered) >= 2:
        return True
    # 多对象比较（比较 A、B、C 的优缺点；A vs B 区别 等）
    if _is_comparison(query):
        return True
    return False


def _assigned_model(models: list[ModelSpec], assignments: dict, role: str) -> ModelSpec | None:
    """Resolve a Rust-assigned role to a ModelSpec from the pool.

    P0.f 双字段契约：优先 `assignments[role]`（Rust 决策的模型名），
    缺则回落 `models[0]` 首个存活模型——绝不依赖 Python 侧硬编码模型名。
    """
    name = (assignments or {}).get(role) or ""
    if name:
        spec = _model_by_name(models, name)
        if spec is not None:
            return spec
    return models[0] if models else None


def _system_id(messages: list[dict]) -> str:
    """Stable digest of the system prompt so cache keys invalidate when the
    system prompt changes (different role instructions ≠ same answer)."""
    for m in messages:
        if m.get("role") == "system":
            return hashlib.sha256((m.get("content") or "").encode()).hexdigest()[:16]
    return "none"


def _two_layer_lookup(
    exact: ExactCache | None,
    sem: SemanticCache | None,
    query: str,
    model: str,
    conv_id: str,
    fingerprint: str,
    context_free: bool,
    system_id: str,
) -> tuple[str | None, float | None, bool]:
    """L1 exact → L2 semantic. Returns (response, similarity, hit_layer).

    L2 (semantic) is only consulted for context-free queries — a context-
    dependent query must never match a global entry from another conversation.
    """
    scope = fingerprint if not context_free else ""
    key = _exact_key(model, system_id, scope, query)
    if exact:
        resp = exact.lookup(key)
        if resp is not None:
            return resp, 1.0, True
    if sem and context_free:
        hit = sem.lookup(query, model, None)
        if hit:
            return hit["response"], hit["similarity"], False
    return None, None, False


def _two_layer_store(
    exact: ExactCache | None,
    sem: SemanticCache | None,
    query: str,
    response: str,
    model: str,
    conv_id: str,
    fingerprint: str,
    context_free: bool,
    system_id: str,
) -> None:
    scope = fingerprint if not context_free else ""
    key = _exact_key(model, system_id, scope, query)
    if exact:
        exact.store(key, model, response, conv_id if not context_free else None)
    # Semantic store only for context-free queries (cross-conversation reuse).
    if sem and context_free:
        sem.store(query, response, model, None)


def _call_llm(
    model_spec: ModelSpec,
    messages: list[dict],
    max_tokens: int = 500,
    temperature: float = 0.3,
    timeout: int = 60,
    *,
    exact: ExactCache | None = None,
    sem: SemanticCache | None = None,
    cache_query: str | None = None,
    conv_id: str = "",
    fingerprint: str = "",
    context_free: bool = True,
    cache_hit_ref: list[bool] | None = None,
    usage_ref: dict | None = None,
) -> str:
    """Non-streaming LLM call with two-layer cache + real usage accounting.

    `cache_query` is the user-facing query text used to key the cache (NOT the
    full prompt — dependency context for subtasks is folded into the messages
    but the cache key still uses the query so independent subtasks stay keyed
    on the question). When None, caching is disabled for this call.
    """
    system_id = _system_id(messages)
    if cache_query and exact is not None or sem is not None:
        resp, sim, _layer = _two_layer_lookup(
            exact, sem, cache_query, model_spec.name, conv_id,
            fingerprint, context_free, system_id,
        )
        if resp is not None:
            if cache_hit_ref is not None:
                cache_hit_ref.append(True)
            if usage_ref is not None:
                in_est = sum(count_tokens(m.get("content", "")) for m in messages)
                out_est = count_tokens(resp)
                usage_ref.update({
                    "input_tokens": in_est,
                    "output_tokens": out_est,
                    "usage": {
                        "prompt_tokens": in_est,
                        "completion_tokens": out_est,
                        "cached_tokens": 0,
                        "reasoning_tokens": 0,
                        "cache_creation_tokens": 0,
                        "field_missing": True,
                    },
                    "cost": 0.0,
                    "saved_cost": _estimate_cost(model_spec, in_est, out_est),
                    "cache_hit": True,
                    "cache_sim": sim,
                })
            return resp

    kwargs = _litellm_kwargs(
        model_spec,
        messages=messages,
        max_tokens=max_tokens,
        temperature=temperature,
        timeout=timeout,
    )
    try:
        response = litellm.completion(**kwargs)
        content = response.choices[0].message.content or ""
        usage = getattr(response, "usage", None)
        in_tok = getattr(usage, "prompt_tokens", 0) or 0
        out_tok = getattr(usage, "completion_tokens", 0) or 0
    except Exception as e:
        raise RuntimeError(f"LLM call failed: {e}")

    if usage_ref is not None:
        u = _usage_detail(getattr(response, "usage", None))
        usage_ref.update({
            "input_tokens": in_tok,
            "output_tokens": out_tok,
            "usage": u,
            "cost": 0.0,  # Rust 按分项计价；保留 0 兼容旧读端
            "saved_cost": 0.0,
            "cache_hit": False,
        })

    if cache_query and _cacheable(cache_query, temperature) and (exact or sem):
        _two_layer_store(
            exact, sem, cache_query, content, model_spec.name,
            conv_id, fingerprint, context_free, system_id,
        )
    return content


def _estimate_subtask_cost(model_spec: ModelSpec, est_tokens: int) -> float:
    return (
        est_tokens * model_spec.input_cost_per_token
        + est_tokens * model_spec.output_cost_per_token
    )


def _call_llm_stream(
    model_spec: ModelSpec,
    messages: list[dict],
    max_tokens: int = 500,
    temperature: float = 0.3,
    timeout: int = 60,
    *,
    exact: ExactCache | None = None,
    sem: SemanticCache | None = None,
    cache_query: str | None = None,
    conv_id: str = "",
    fingerprint: str = "",
    context_free: bool = True,
    cache_hit_ref: list[bool] | None = None,
    usage_ref: dict | None = None,
) -> Iterator[str]:
    """Streaming LLM call. Yields content deltas (str) as they arrive.

    Mirrors `_call_llm` but uses litellm's streaming mode so the orchestrator
    can forward tokens to the client incrementally (true SSE, not buffered).
    """
    system_id = _system_id(messages)
    if cache_query and (exact is not None or sem is not None):
        resp, sim, _layer = _two_layer_lookup(
            exact, sem, cache_query, model_spec.name, conv_id,
            fingerprint, context_free, system_id,
        )
        if resp is not None:
            if cache_hit_ref is not None:
                cache_hit_ref.append(True)
            if usage_ref is not None:
                in_est = sum(count_tokens(m.get("content", "")) for m in messages)
                out_est = count_tokens(resp)
                usage_ref.update({
                    "input_tokens": in_est,
                    "output_tokens": out_est,
                    "usage": {
                        "prompt_tokens": in_est,
                        "completion_tokens": out_est,
                        "cached_tokens": 0,
                        "reasoning_tokens": 0,
                        "cache_creation_tokens": 0,
                        "field_missing": True,
                    },
                    "cost": 0.0,
                    "saved_cost": _estimate_cost(model_spec, in_est, out_est),
                    "cache_hit": True,
                    "cache_sim": sim,
                })
            yield resp
            return

    kwargs = _litellm_kwargs(
        model_spec,
        messages=messages,
        max_tokens=max_tokens,
        temperature=temperature,
        timeout=timeout,
        stream=True,
        stream_options={"include_usage": True},
    )
    full: list[str] = []
    in_tok = 0
    out_tok = 0
    try:
        for chunk in litellm.completion(**kwargs):
            delta = chunk.choices[0].delta if chunk.choices else None
            if delta and delta.content:
                full.append(delta.content)
                yield delta.content
            u = getattr(chunk, "usage", None)
            if u is not None:
                in_tok = getattr(u, "prompt_tokens", 0) or 0
                out_tok = getattr(u, "completion_tokens", 0) or 0
    except Exception as e:
        raise RuntimeError(f"LLM stream failed: {e}")

    if usage_ref is not None:
        u = _usage_detail(usage)
        usage_ref.update({
            "input_tokens": in_tok,
            "output_tokens": out_tok,
            "usage": u,
            "cost": 0.0,  # Rust 按分项计价；保留 0 兼容旧读端
            "saved_cost": 0.0,
            "cache_hit": False,
        })

    content = "".join(full)
    if cache_query and content and _cacheable(cache_query, temperature) and (exact or sem):
        _two_layer_store(
            exact, sem, cache_query, content, model_spec.name,
            conv_id, fingerprint, context_free, system_id,
        )


def _fallback_decompose(query: str) -> list[dict]:
    """LLM 分解失败时的启发式兜底：按编号/换行/标点把问题拆成多个子任务。"""
    parts = re.split(
        r"(?m)^\s*(?:\d+[\.、]|[一二三四五六七八九十]+[、.]|[-*])\s+", query
    )
    parts = [p.strip(" \t-*\n") for p in parts if p.strip(" \t-*\n")]
    if len(parts) < 2:
        parts = [s.strip() for s in re.split(r"[。！？.!?]", query) if s.strip()]
    tasks = []
    for i, p in enumerate(parts, 1):
        if not p:
            continue
        tasks.append({
            "id": i,
            "description": p,
            "task_type": "general",
            "depends_on": [],
            "estimated_output_tokens": 300,
        })
    return tasks


LIGHT_SYSTEM = "你是一个专业的AI助手。请认真完成以下任务。请结合对话上下文回答。"


def build_context(
    query: str,
    history: list[dict],
    summary: str,
    summary_upto: int,
    system_prompt: str,
    budget: int = CONTEXT_BUDGET,
) -> tuple[list[dict], str, int, dict]:
    """Assemble a token-budgeted prompt: [system][?summary][kept history][query].

    PRICING-PLAN §5.3 (prefix stability): the system prompt, summary and kept
    history form a **byte-stable prefix**; only the trailing `query` changes
    between turns. Provider-side prefix caches (DashScope implicit cache etc.)
    hit on this prefix — do NOT insert timestamps/random ids into the system
    prompt or reorder tools between turns, or the cache is invalidated every
    turn (billed at full input price).

    Returns (messages, new_summary, new_upto, stats). When the kept window
    drops messages not yet covered by the summary and at least
    `SUMMARY_BLOCK` such uncovered messages accumulate, a fresh rolling
    summary is generated (the caller — orchestrate_stream — performs the LLM
    call and emits `summary_updated`). Between updates the summary prefix is
    byte-stable, which maximizes provider-side prefix-cache hits.

    stats keys: budget, query_tokens, history_total, kept, dropped, summary_used,
                needs_summary, uncovered.
    """
    q_tokens = count_tokens(query)
    sys_tokens = count_tokens(system_prompt)
    summary_tokens = count_tokens(summary) if summary else 0
    avail = max(0, budget - q_tokens - sys_tokens - summary_tokens)

    # Walk from newest to oldest, packing history into the budget.
    kept_rev: list[dict] = []
    used = 0
    for m in reversed(history):
        t = count_tokens(m.get("content", ""))
        if used + t > avail:
            break
        kept_rev.append(m)
        used += t
    kept = list(reversed(kept_rev))
    kept_count = len(kept)
    dropped = history[: len(history) - kept_count]

    upto = min(summary_upto, len(history))
    uncovered = dropped[upto:] if upto < len(dropped) else (
        dropped[upto:] if upto <= len(dropped) else []
    )
    needs_summary = bool(uncovered) and (
        not summary or len(uncovered) >= SUMMARY_BLOCK
    )

    messages: list[dict] = [{"role": "system", "content": system_prompt}]
    if summary:
        messages.append({
            "role": "system",
            "content": f"以下是本对话早前内容的摘要，供你参考：\n{summary}",
        })
    messages.extend(kept)
    messages.append({"role": "user", "content": query})

    stats = {
        "budget": budget,
        "query_tokens": q_tokens,
        "history_total": len(history),
        "kept": kept_count,
        "dropped": len(dropped),
        "summary_used": bool(summary),
        "needs_summary": needs_summary,
        "uncovered": len(uncovered),
    }
    return messages, summary, upto, stats


def _make_summary(
    models: list[ModelSpec],
    prev_summary: str,
    uncovered: list[dict],
    assignments: dict,
) -> str | None:
    """Generate/extend the rolling summary of older turns via a cheap model.
    Returns None on failure (caller keeps the old summary / falls back)."""
    spec = _assigned_model(models, assignments, "decompose")
    if spec is None:
        return None
    transcript = "\n".join(
        f"{('用户' if m.get('role') == 'user' else '助手')}: {m.get('content', '')[:400]}"
        for m in uncovered
    )
    base = f"已有摘要：\n{prev_summary}\n\n" if prev_summary else ""
    prompt = (
        f"{base}请把以下对话片段压缩成一段简洁的事实摘要（≤300字），保留关键事实、"
        f"用户意图与已做决定，忽略寒暄：\n{transcript}"
    )
    try:
        out = _call_llm(
            spec,
            messages=[{"role": "user", "content": prompt}],
            max_tokens=400,
            temperature=0,
            timeout=30,
        )
        return out.strip() or None
    except Exception:
        return None


@app.post("/v1/orchestrate/stream")
def orchestrate_stream(req: OrchestrateRequest) -> StreamingResponse:
    # Two-layer cache singletons. L1 (exact, sqlite) is always available when a
    # cache_dir is given; L2 (semantic, chroma) only after /v1/cache/init.
    exact: ExactCache | None = None
    sem: SemanticCache | None = None
    if req.cache_dir:
        exact_path = os.path.join(
            os.path.dirname(req.cache_dir.rstrip("/")) or ".",
            "cache_exact.sqlite3",
        )
        exact = ExactCache.get(exact_path, req.ttl)
        sem = SemanticCache.get(req.cache_dir, req.similarity_threshold, req.ttl)
    _start_eviction_thread(exact, sem)

    def gen():
        # ── Context building (L1 truncation + L2 summary) ──
        history = req.history
        summary = req.summary
        upto = req.summary_upto
        messages, summary, upto, ctx_stats = build_context(
            req.query, history, summary, upto, LIGHT_SYSTEM
        )
        # NOTE: `messages` already ends with the user query; downstream LLM
        # calls reuse this prefix (light path) or derive subtask messages from
        # `history` (complex path) — both stay within the same token budget.
        yield _sse("context", ctx_stats)

        # If the budget dropped older turns not yet summarized, generate/extend
        # the rolling summary now (one cheap LLM call, persisted by the Rust
        # host on the `summary_updated` event). Skipped when there are too few
        # uncovered messages — keep the prefix stable a little longer.
        if ctx_stats["needs_summary"]:
            uncovered = history[upto: len(history) - ctx_stats["kept"]]
            new_summary = _make_summary(req.models, summary, uncovered, req.assignments)
            if new_summary:
                summary = new_summary
                upto = len(history) - ctx_stats["kept"]
                # Rebuild messages so the fresh summary is in the prompt.
                messages, _, _, _ = build_context(
                    req.query, history, summary, upto, LIGHT_SYSTEM
                )
                yield _sse("summary_updated", {"text": summary, "upto": upto})

        # Context-free flag + per-conversation fingerprint drive cache scope.
        context_free = _is_context_free(req.query, history)
        fingerprint = "" if context_free else _fingerprint(req.conversation_id, history)

        if not _is_complex(req.query):
            # 轻量默认路径：单模型直接流式回答（最快，边生成边下发 token）
            model_spec = _assigned_model(req.models, req.assignments, "general")
            if model_spec is None:
                yield _sse("error", {"message": "无可用模型"})
                return
            model_name = model_spec.name
            yield _sse("decompose", {
                "sub_tasks": [{"id": 1, "description": req.query,
                               "selected_model": model_name, "cost": 0.0001}],
                "total_cost": 0.0001,
            })
            yield _sse("task_start", {"id": 1, "description": req.query, "model": model_name})

            start = time.time()
            cache_hit: list[bool] = []
            usage: dict = {}
            parts: list[str] = []
            try:
                for delta in _call_llm_stream(
                    model_spec,
                    messages=messages,
                    max_tokens=2000,
                    timeout=120,
                    exact=exact,
                    sem=sem,
                    cache_query=req.query,
                    conv_id=req.conversation_id,
                    fingerprint=fingerprint,
                    context_free=context_free,
                    cache_hit_ref=cache_hit,
                    usage_ref=usage,
                ):
                    parts.append(delta)
                    yield _sse("token", {"id": 1, "model": model_name, "delta": delta})
                ok = True
            except Exception as e:
                parts.append(f"执行失败: {e}")
                ok = False
            duration = time.time() - start
            content = "".join(parts)
            sim = sem.best_sim(req.query, model_name) if sem else usage.get("cache_sim")
            hit = bool(cache_hit) or usage.get("cache_hit", False)

            yield _sse("task_done", {
                "id": 1, "model": model_name,
                "duration": duration,
                "cost": usage.get("cost", 0.0),
                "input_tokens": usage.get("input_tokens", 0),
                "output_tokens": usage.get("output_tokens", 0),
                "saved_cost": usage.get("saved_cost", 0.0),
                "cache_hit": hit,
                "cache_sim": sim,
            })
            yield _sse("result", {
                "response": content,
                "model": model_name,
                "cost": usage.get("cost", 0.0),
                "input_tokens": usage.get("input_tokens", 0),
                "output_tokens": usage.get("output_tokens", 0),
                "saved_cost": usage.get("saved_cost", 0.0),
                "total_duration": duration,
                "models_used": [model_name],
                "sr_info": f"SR域分类: {req.sr_domain}" if req.sr_domain else "",
                "ok": ok,
                "cache_hit": hit,
                "cache_sim": sim,
            })
            return

        # Complex: decompose —— Rust 决策的分解模型（便宜 + 快），避免占用重型推理模型
        model_spec = _assigned_model(req.models, req.assignments, "decompose")
        if model_spec is None:
            yield _sse("error", {"message": "无可用分解模型"})
            return
        # P0.f: 子任务级分配属 P4.a 待办；当前统一复用 Rust 的 general 决策，
        # 保证每个子任务同样由注册表驱动，Python 无模型字面量。
        default_sub_model = _assigned_model(req.models, req.assignments, "general") or model_spec
        tasks_data: list[dict] = []
        try:
            content = _call_llm(
                model_spec,
                messages=[
                    {"role": "system", "content": DECOMPOSE_SYSTEM_PROMPT},
                    {"role": "user", "content": req.query},
                ],
                max_tokens=800,
                temperature=0,
                timeout=30,
            )
            # 去掉可能的 ```json 代码围栏后再抽取数组
            cleaned = re.sub(r"```(?:json)?", "", content).strip()
            json_match = re.search(r"\[.*\]", cleaned, re.DOTALL)
            if json_match:
                try:
                    tasks_data = json.loads(json_match.group())
                except Exception:
                    tasks_data = []
        except Exception:
            tasks_data = []

        # 分解失败时的启发式兜底：若原始问题含多个编号项，则按编号拆分
        if not tasks_data:
            numbered = re.findall(
                r"(?m)^\s*(?:\d+[\.、]|[一二三四五六七八九十]+[、.]|[-*])\s+\S", req.query
            )
            if len(numbered) >= 2:
                tasks_data = _fallback_decompose(req.query)

        if not tasks_data:
            tasks_data = [{"id": 1, "description": req.query, "task_type": "complex_reasoning",
                           "depends_on": [], "estimated_output_tokens": 500}]

        sub_tasks = []
        for td in tasks_data:
            sub_tasks.append({
                "id": td.get("id", len(sub_tasks) + 1),
                "description": td.get("description", ""),
                "task_type": td.get("task_type", "general"),
                "depends_on": td.get("depends_on", []),
                "estimated_output_tokens": td.get("estimated_output_tokens", 1024),
                "selected_model": default_sub_model.name,
            })
        for t in sub_tasks:
            spec = _model_by_name(req.models, t["selected_model"])
            t["cost"] = _estimate_subtask_cost(spec, t["estimated_output_tokens"]) if spec else 0.0

        # 多模型可视化分配：可用模型 ≥2 时，按序轮询把各子任务分配到不同模型
        if len(req.models) >= 2:
            names = [m.name for m in req.models]
            for i, t in enumerate(sub_tasks):
                t["selected_model"] = names[i % len(names)]

        yield _sse("decompose", {
            "sub_tasks": sub_tasks,
            "total_cost": sum(t["cost"] for t in sub_tasks),
        })

        completed: dict[int, dict] = {}
        any_cache_hit = False
        total_in = total_out = 0
        total_cost = 0.0
        total_saved = 0.0
        # Complex subtasks use the budgeted history window (kept turns only —
        # the summary already covers dropped older turns).
        kept_history = history[len(history) - ctx_stats["kept"]:] if ctx_stats["kept"] else []
        for task in sub_tasks:
            context_parts = []
            for dep_id in task["depends_on"]:
                if dep_id in completed:
                    dep = completed[dep_id]
                    context_parts.append(f"[子任务{dep_id}] {dep['description']}\n结果: {dep['result']}")
            context = "\n\n".join(context_parts) if context_parts else ""

            yield _sse("task_start", {"id": task["id"], "description": task["description"],
                                      "model": task["selected_model"]})

            spec = _model_by_name(req.models, task["selected_model"])
            if not spec:
                spec = ModelSpec(name=task["selected_model"], litellm_model=task["selected_model"])
            user_content = task["description"]
            if context:
                user_content = f"前置任务结果：\n{context}\n\n当前任务：{task['description']}"

            start = time.time()
            task_hit: list[bool] = []
            task_usage: dict = {}
            try:
                result = _call_llm(
                    spec,
                    messages=[
                        {"role": "system", "content": LIGHT_SYSTEM},
                        *kept_history[-10:],
                        {"role": "user", "content": user_content},
                    ],
                    max_tokens=task["estimated_output_tokens"],
                    timeout=120,
                    exact=exact,
                    sem=sem,
                    cache_query=user_content,
                    conv_id=req.conversation_id,
                    fingerprint=fingerprint,
                    context_free=context_free,
                    cache_hit_ref=task_hit,
                    usage_ref=task_usage,
                )
                task["result"] = result
                task["status"] = "done"
            except Exception as e:
                task["result"] = f"执行失败: {e}"
                task["status"] = "failed"
                task["error"] = str(e)
            task["duration"] = time.time() - start
            hit = bool(task_hit) or task_usage.get("cache_hit", False)
            sim = task_usage.get("cache_sim") or (sem.best_sim(user_content, task["selected_model"]) if sem else None)
            task["cache_hit"] = hit
            task["cache_sim"] = sim
            any_cache_hit = any_cache_hit or hit
            total_in += task_usage.get("input_tokens", 0)
            total_out += task_usage.get("output_tokens", 0)
            total_cost += task_usage.get("cost", 0.0)
            total_saved += task_usage.get("saved_cost", 0.0)
            completed[task["id"]] = task

            done_payload: dict = {
                "id": task["id"], "model": task["selected_model"],
                "duration": task["duration"],
                "cost": task_usage.get("cost", 0.0),
                "input_tokens": task_usage.get("input_tokens", 0),
                "output_tokens": task_usage.get("output_tokens", 0),
                "saved_cost": task_usage.get("saved_cost", 0.0),
                "cache_hit": hit,
                "cache_sim": sim,
            }
            if task.get("status") == "failed":
                done_payload["error"] = task.get("error") or "执行失败"
            yield _sse("task_done", done_payload)

        # Aggregate —— 流式输出最终回答
        failed_tasks = [t for t in sub_tasks if t.get("status") == "failed"]
        agg_model = _assigned_model(req.models, req.assignments, "aggregate")
        if agg_model is None:
            yield _sse("error", {"message": "无可用汇总模型"})
            return

        if failed_tasks:
            final = "## 部分子任务执行失败\n\n" + "\n\n".join(
                f"**子任务 {t['id']}**（{t['selected_model']}）: {t['description']}\n\n执行失败：{t.get('error', '未知错误')}"
                for t in sub_tasks
            )
            yield _sse("task_start", {"id": 0, "description": "汇总最终回答（部分子任务失败）", "model": agg_model.name})
            for chunk in [final[i:i+30] for i in range(0, len(final), 30)]:
                yield _sse("token", {"id": 0, "model": agg_model.name, "delta": chunk})
            agg_duration = 0.0
            agg_usage: dict = {}
        else:
            summary_parts = [f"## 子任务 {t['id']}: {t['description']}\n\n{t['result']}" for t in sub_tasks]
            yield _sse("task_start", {"id": 0, "description": "汇总最终回答", "model": agg_model.name})
            start = time.time()
            agg_parts: list[str] = []
            agg_usage: dict = {}
            try:
                for delta in _call_llm_stream(
                    agg_model,
                    messages=[
                        {"role": "system", "content": AGGREGATE_SYSTEM_PROMPT},
                        *kept_history[-6:],
                        {"role": "user", "content": f"原始任务：{req.query}\n\n子任务执行结果：\n\n{chr(10).join(summary_parts)}\n\n请汇总以上结果，生成最终回答。"},
                    ],
                    max_tokens=4096,
                    temperature=0.3,
                    timeout=120,
                    exact=exact,
                    sem=sem,
                    cache_query=req.query,
                    conv_id=req.conversation_id,
                    fingerprint=fingerprint,
                    context_free=context_free,
                    usage_ref=agg_usage,
                ):
                    agg_parts.append(delta)
                    yield _sse("token", {"id": 0, "model": agg_model.name, "delta": delta})
                final = "".join(agg_parts)
            except Exception:
                final = "\n\n---\n\n".join(f"**子任务 {t['id']}**: {t['result']}" for t in sub_tasks)
            agg_duration = time.time() - start

        total_in += agg_usage.get("input_tokens", 0)
        total_out += agg_usage.get("output_tokens", 0)
        total_cost += agg_usage.get("cost", 0.0)
        total_saved += agg_usage.get("saved_cost", 0.0)

        yield _sse("task_done", {"id": 0, "model": agg_model.name,
                                 "duration": agg_duration,
                                 "cost": agg_usage.get("cost", 0.0),
                                 "input_tokens": agg_usage.get("input_tokens", 0),
                                 "output_tokens": agg_usage.get("output_tokens", 0),
                                 "saved_cost": agg_usage.get("saved_cost", 0.0),
                                 "cache_hit": agg_usage.get("cache_hit", False)})

        models_used = sorted({t["selected_model"] for t in sub_tasks} | {agg_model.name})
        yield _sse("result", {
            "response": final,
            "model": agg_model.name,
            "cost": total_cost,
            "input_tokens": total_in,
            "output_tokens": total_out,
            "saved_cost": total_saved,
            "total_duration": 0,
            "models_used": models_used,
            "aggregator": agg_model.name,
            "cache_hit": any_cache_hit,
            "cache_sim": max((t.get("cache_sim") or 0.0) for t in sub_tasks) if sub_tasks else 0.0,
            "sr_info": f"SR域分类: {req.sr_domain}" if req.sr_domain else "",
        })

    return StreamingResponse(gen(), media_type="text/event-stream")


@app.get("/v1/health")
def health() -> dict:
    """Report the AI service's real readiness.

    `ready` is false if no LLM backend is reachable at all (no cloud API key
    configured AND local Ollama is down). This lets the Rust core show an
    honest status instead of claiming healthy when every model call fails.
    """
    has_cloud_key = any(
        os.getenv(k)
        for k in ("DASHSCOPE_API_KEY", "OPENAI_API_KEY", "ANTHROPIC_API_KEY")
    )
    ollama_ok = _ollama_reachable()
    ready = has_cloud_key or ollama_ok
    return {
        "status": "ok",
        "service": "ai",
        "version": "2.0.0",
        "ready": ready,
        "backends": {
            "cloud_key_configured": has_cloud_key,
            "ollama_reachable": ollama_ok,
        },
    }


def _ollama_reachable() -> bool:
    """Check whether the local Ollama endpoint responds."""
    import urllib.request

    base = os.getenv("OLLAMA_API_BASE", "http://localhost:11434")
    try:
        with urllib.request.urlopen(f"{base}/api/tags", timeout=2) as resp:
            return resp.status == 200
    except Exception:
        return False


def main() -> None:
    """CLI entry point for the AI micro-service."""
    import argparse
    import uvicorn

    parser = argparse.ArgumentParser(description="LLooM AI micro-service")
    parser.add_argument("--port", type=int, default=int(os.getenv("LLOOM_AI_PORT", "7862")))
    parser.add_argument("--host", default="0.0.0.0")
    args = parser.parse_args()

    uvicorn.run(app, host=args.host, port=args.port)


if __name__ == "__main__":
    main()
