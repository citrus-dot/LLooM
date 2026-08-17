"""LLooM AI micro-service — the only remaining Python layer.

Stateless: the Rust host passes explicit model params (litellm_model, api_base,
api_key, pricing) with every request. Python never touches SQLite, .env config,
or business logic — it only calls litellm.

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
    def __init__(self, path: str, threshold: float, ttl: int):
        self.path = path
        self.threshold = threshold
        self.ttl = ttl
        self._collection = None
        self._enabled = False
        # Only enable when a path is given AND the embedding model has been
        # pre-initialized. This prevents the synchronous download that hangs.
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

    def get(self, query: str, model: str) -> dict | None:
        if not self._enabled or not self._collection:
            return None
        try:
            results = self._collection.query(
                query_texts=[query],
                n_results=1,
                where={"model": model} if model != "default" else None,
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

    def best_sim(self, query: str, model: str) -> float | None:
        """Top-1 cosine similarity to the nearest cached query, ignoring the
        threshold. Used for calibration — we need the score even on a miss."""
        if not self._enabled or not self._collection:
            return None
        try:
            results = self._collection.query(
                query_texts=[query],
                n_results=1,
                where={"model": model} if model != "default" else None,
            )
            if not results or not results["ids"] or not results["ids"][0]:
                return None
            return 1 - results["distances"][0][0]
        except Exception:
            return None

    def put(self, query: str, response: str, model: str) -> None:
        if not self._enabled or not self._collection:
            return
        import hashlib

        doc_id = hashlib.md5(f"{model}:{query}".encode()).hexdigest()
        try:
            self._collection.upsert(
                ids=[doc_id],
                documents=[query],
                metadatas=[{
                    "response": response,
                    "model": model,
                    "cached_at": time.time(),
                }],
            )
        except Exception:
            pass


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
    usage = getattr(response, "usage", None)
    input_tokens = getattr(usage, "prompt_tokens", 0) or 0
    output_tokens = getattr(usage, "completion_tokens", 0) or 0
    return {
        "content": content,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "cost": _estimate_cost(req.model, input_tokens, output_tokens),
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
            input_tokens = getattr(usage, "prompt_tokens", 0) if usage else 0
            output_tokens = getattr(usage, "completion_tokens", 0) if usage else 0
            yield _plain_sse({
                "done": True,
                "cost": _estimate_cost(req.model, input_tokens, output_tokens),
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
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

# 分解阶段需要一个「便宜 + 快」的模型做结构化抽取，不占用重型推理模型。
DECOMPOSER_PREFERENCE = ["qwen3.6-flash", "qwen-plus", "deepseek-v3", "qwen2.5-local"]

TASK_MODEL_PREFERENCE = {
    "simple_qa": ["qwen2.5-local", "qwen3.6-flash", "qwen-plus"],
    "general": ["qwen-plus", "qwen3.6-flash", "qwen2.5-local"],
    "coding": ["deepseek-v3", "qwen-plus", "qwen2.5-local"],
    "math_logic": ["deepseek-v3", "qwen-plus", "qwen3.6-plus"],
    "complex_reasoning": ["qwen3.6-plus", "deepseek-v3", "qwen-plus"],
}

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


def _select_model(task_type: str, models: list[ModelSpec]) -> str:
    available = {m.name for m in models}
    for model in TASK_MODEL_PREFERENCE.get(task_type, ["qwen-plus"]):
        if not available or model in available:
            return model
    return "qwen2.5-local"


def _call_llm(
    model_spec: ModelSpec,
    messages: list[dict],
    max_tokens: int = 500,
    temperature: float = 0.3,
    timeout: int = 60,
    cache: SemanticCache | None = None,
    cache_key: str | None = None,
    cache_hit_ref: list[bool] | None = None,
) -> str:
    if cache and cache_key:
        hit = cache.get(cache_key, model_spec.name)
        if hit:
            if cache_hit_ref is not None:
                cache_hit_ref.append(True)
            return hit["response"]
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
    except Exception as e:
        raise RuntimeError(f"LLM call failed: {e}")
    if cache and cache_key:
        cache.put(cache_key, content, model_spec.name)
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
    cache: SemanticCache | None = None,
    cache_key: str | None = None,
    cache_hit_ref: list[bool] | None = None,
) -> Iterator[str]:
    """Streaming LLM call. Yields content deltas (str) as they arrive.

    Mirrors `_call_llm` but uses litellm's streaming mode so the orchestrator
    can forward tokens to the client incrementally (true SSE, not buffered).
    """
    if cache and cache_key:
        hit = cache.get(cache_key, model_spec.name)
        if hit:
            if cache_hit_ref is not None:
                cache_hit_ref.append(True)
            yield hit["response"]
            return
    kwargs = _litellm_kwargs(
        model_spec,
        messages=messages,
        max_tokens=max_tokens,
        temperature=temperature,
        timeout=timeout,
        stream=True,
    )
    full: list[str] = []
    try:
        for chunk in litellm.completion(**kwargs):
            delta = chunk.choices[0].delta
            if delta and delta.content:
                full.append(delta.content)
                yield delta.content
    except Exception as e:
        raise RuntimeError(f"LLM stream failed: {e}")
    if cache and cache_key and full:
        cache.put(cache_key, "".join(full), model_spec.name)


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


@app.post("/v1/orchestrate/stream")
def orchestrate_stream(req: OrchestrateRequest) -> StreamingResponse:
    cache = SemanticCache(req.cache_dir, req.similarity_threshold, req.ttl)

    def gen():
        if not _is_complex(req.query):
            # 轻量默认路径：单模型直接流式回答（最快，边生成边下发 token）
            model_name = _select_model("general", req.models)
            model_spec = _model_by_name(req.models, model_name) or ModelSpec(
                name=model_name, litellm_model=model_name
            )
            yield _sse("decompose", {
                "sub_tasks": [{"id": 1, "description": req.query,
                               "selected_model": model_name, "cost": 0.0001}],
                "total_cost": 0.0001,
            })
            yield _sse("task_start", {"id": 1, "description": req.query, "model": model_name})

            start = time.time()
            cache_hit: list[bool] = []
            parts: list[str] = []
            try:
                for delta in _call_llm_stream(
                    model_spec,
                    messages=[
                        {"role": "system", "content": "你是一个专业的AI助手。请认真完成以下任务。请结合对话上下文回答。"},
                        *req.history[-10:],
                        {"role": "user", "content": req.query},
                    ],
                    max_tokens=2000,
                    timeout=120,
                    cache=cache,
                    cache_key=req.query,
                    cache_hit_ref=cache_hit,
                ):
                    parts.append(delta)
                    yield _sse("token", {"id": 1, "model": model_name, "delta": delta})
                ok = True
            except Exception as e:
                parts.append(f"执行失败: {e}")
                ok = False
            duration = time.time() - start
            content = "".join(parts)
            sim = cache.best_sim(req.query, model_name)

            yield _sse("task_done", {
                "id": 1, "model": model_name,
                "duration": duration, "cost": 0.0, "tokens": 0,
                "cache_hit": bool(cache_hit),
                "cache_sim": sim,
            })
            yield _sse("result", {
                "response": content,
                "total_cost": 0.0,
                "total_tokens": 0,
                "total_duration": duration,
                "models_used": [model_name],
                "sr_info": f"SR域分类: {req.sr_domain}" if req.sr_domain else "",
                "ok": ok,
                "cache_hit": bool(cache_hit),
                "cache_sim": sim,
            })
            return

        # Complex: decompose —— 优先用便宜快速的分解模型，避免占用重型推理模型
        model_spec = None
        for pref in DECOMPOSER_PREFERENCE:
            spec = _model_by_name(req.models, pref)
            if spec:
                model_spec = spec
                break
        if model_spec is None:
            model_spec = req.models[0] if req.models else ModelSpec(name="auto", litellm_model="auto")
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
                "selected_model": _select_model(td.get("task_type", "general"), req.models),
            })
        for t in sub_tasks:
            spec = _model_by_name(req.models, t["selected_model"])
            t["cost"] = _estimate_subtask_cost(spec, t["estimated_output_tokens"]) if spec else 0.0

        # 多模型可视化分配：可用模型 ≥2 时，按序轮询把各子任务分配到不同模型，
        # 使计划卡片清晰展示「哪部分交给哪个模型」——这是用户最想看到的信息。
        # 代价是牺牲一点类型-模型最优匹配（当前 active 模型均为通用对话模型，
        # 对分析/对比类子任务影响极小）。聚合阶段仍用较强的汇总模型。
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
            try:
                result = _call_llm(
                    spec,
                    messages=[
                        {"role": "system", "content": "你是一个专业的AI助手。请认真完成以下任务。请结合对话上下文回答。"},
                        *req.history[-10:],
                        {"role": "user", "content": user_content},
                    ],
                    max_tokens=task["estimated_output_tokens"],
                    timeout=120,
                    cache=cache,
                    # Key on the full prompt (incl. dependency context) so a
                    # subtask with prerequisites never returns a stale cached
                    # answer from a different context. For independent subtasks
                    # user_content == description, so hits are preserved.
                    cache_key=user_content,
                    cache_hit_ref=task_hit,
                )
                task["result"] = result
                task["status"] = "done"
            except Exception as e:
                task["result"] = f"执行失败: {e}"
                task["status"] = "failed"
                task["error"] = str(e)
            task["duration"] = time.time() - start
            task["tokens"] = 0
            task["cache_hit"] = bool(task_hit)
            task["cache_sim"] = cache.best_sim(user_content, task["selected_model"])
            any_cache_hit = any_cache_hit or bool(task_hit)
            completed[task["id"]] = task

            done_payload: dict = {"id": task["id"], "model": task["selected_model"],
                                  "duration": task["duration"], "cost": task["cost"],
                                  "tokens": 0, "cache_hit": bool(task_hit),
                                  "cache_sim": task.get("cache_sim")}
            if task.get("status") == "failed":
                done_payload["error"] = task.get("error") or "执行失败"
            yield _sse("task_done", done_payload)

        # Aggregate —— 流式输出最终回答（轻量模式：仅展示子任务进度，最终答案逐 token 下发）
        failed_tasks = [t for t in sub_tasks if t.get("status") == "failed"]
        agg_model = None
        for pref in ["qwen-plus", "qwen3.6-flash", "deepseek-v3", "qwen2.5-local"]:
            spec = _model_by_name(req.models, pref)
            if spec:
                agg_model = spec
                break
        if agg_model is None:
            agg_model = req.models[0] if req.models else ModelSpec(name="qwen-plus", litellm_model="qwen-plus")

        # 如果有子任务失败，直接拼接结果，避免汇总模型对失败内容编造/美化。
        # 同时把真实错误信息保留在最终回答里，让用户知道哪一步出了问题。
        if failed_tasks:
            final = "## 部分子任务执行失败\n\n" + "\n\n".join(
                f"**子任务 {t['id']}**（{t['selected_model']}）: {t['description']}\n\n执行失败：{t.get('error', '未知错误')}"
                for t in sub_tasks
            )
            yield _sse("task_start", {"id": 0, "description": "汇总最终回答（部分子任务失败）", "model": agg_model.name})
            for chunk in [final[i:i+30] for i in range(0, len(final), 30)]:
                yield _sse("token", {"id": 0, "model": agg_model.name, "delta": chunk})
            agg_duration = 0.0
        else:
            summary_parts = [f"## 子任务 {t['id']}: {t['description']}\n\n{t['result']}" for t in sub_tasks]
            yield _sse("task_start", {"id": 0, "description": "汇总最终回答", "model": agg_model.name})
            start = time.time()
            agg_parts: list[str] = []
            try:
                for delta in _call_llm_stream(
                    agg_model,
                    messages=[
                        {"role": "system", "content": AGGREGATE_SYSTEM_PROMPT},
                        *req.history[-6:],
                        {"role": "user", "content": f"原始任务：{req.query}\n\n子任务执行结果：\n\n{chr(10).join(summary_parts)}\n\n请汇总以上结果，生成最终回答。"},
                    ],
                    max_tokens=4096,
                    temperature=0.3,
                    timeout=120,
                ):
                    agg_parts.append(delta)
                    yield _sse("token", {"id": 0, "model": agg_model.name, "delta": delta})
                final = "".join(agg_parts)
            except Exception:
                final = "\n\n---\n\n".join(f"**子任务 {t['id']}**: {t['result']}" for t in sub_tasks)
            agg_duration = time.time() - start

        yield _sse("task_done", {"id": 0, "model": agg_model.name,
                                 "duration": agg_duration, "cost": 0.0, "tokens": 0,
                                 "cache_hit": False})

        total_cost = sum(t["cost"] for t in sub_tasks)
        models_used = sorted({t["selected_model"] for t in sub_tasks} | {agg_model.name})
        yield _sse("result", {
            "response": final,
            "total_cost": total_cost,
            "total_tokens": 0,
            "total_duration": 0,
            "models_used": models_used,
            "aggregator": agg_model.name,
            "cache_hit": any_cache_hit,
            "cache_sim": max((t.get("cache_sim") or 0.0) for t in sub_tasks),
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
