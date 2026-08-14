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
import time
from dataclasses import dataclass, field
from enum import Enum
from typing import Any

import litellm
from fastapi import FastAPI
from fastapi.responses import StreamingResponse
from pydantic import BaseModel

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
    similarity_threshold: float = 0.95
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


class SemanticCache:
    def __init__(self, path: str, threshold: float, ttl: int):
        self.path = path
        self.threshold = threshold
        self.ttl = ttl
        self._collection = None
        self._enabled = False
        if not path:
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
    )

    def gen():
        input_tokens = 0
        output_tokens = 0
        try:
            for chunk in litellm.completion(**kwargs):
                delta = chunk.choices[0].delta
                if delta and delta.content:
                    yield _plain_sse({"chunk": delta.content})
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
]

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
5. 结合对话上下文，确保回答与之前的对话连贯"""


def _is_complex(query: str) -> bool:
    for pattern in COMPLEXITY_INDICATORS:
        if re.search(pattern, query, re.IGNORECASE):
            return True
    if len(query) > 100:
        return True
    sentences = [s.strip() for s in re.split(r"[。！？.!?]", query) if s.strip()]
    return len(sentences) > 2


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
) -> str:
    if cache and cache_key:
        hit = cache.get(cache_key, model_spec.name)
        if hit:
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


@app.post("/v1/orchestrate/stream")
def orchestrate_stream(req: OrchestrateRequest) -> StreamingResponse:
    cache = SemanticCache(req.cache_dir, req.similarity_threshold, req.ttl)

    def gen():
        if not _is_complex(req.query):
            yield _sse("decompose", {
                "sub_tasks": [{"id": 1, "description": req.query,
                               "selected_model": "auto", "cost": 0.0001}],
                "total_cost": 0.0001,
            })
            yield _sse("task_start", {"id": 1, "description": req.query, "model": "auto"})

            model_name = _select_model("general", req.models)
            model_spec = _model_by_name(req.models, model_name) or ModelSpec(
                name=model_name, litellm_model=model_name
            )
            start = time.time()
            try:
                content = _call_llm(
                    model_spec,
                    messages=[
                        {"role": "system", "content": "你是一个专业的AI助手。请认真完成以下任务。请结合对话上下文回答。"},
                        *req.history[-10:],
                        {"role": "user", "content": req.query},
                    ],
                    max_tokens=500,
                    timeout=120,
                    cache=cache,
                    cache_key=req.query,
                )
                ok = True
            except Exception as e:
                content = f"执行失败: {e}"
                ok = False
            duration = time.time() - start

            yield _sse("task_done", {
                "id": 1, "model": model_name,
                "duration": duration, "cost": 0.0, "tokens": 0,
            })
            yield _sse("result", {
                "response": content,
                "total_cost": 0.0,
                "total_tokens": 0,
                "total_duration": duration,
                "sr_info": f"SR域分类: {req.sr_domain}" if req.sr_domain else "",
                "ok": ok,
            })
            return

        # Complex: decompose
        model_spec = _model_by_name(req.models, "qwen3.6-plus") or (
            _model_by_name(req.models, "deepseek-v3")
            or (req.models[0] if req.models else ModelSpec(name="auto", litellm_model="auto"))
        )
        try:
            content = _call_llm(
                model_spec,
                messages=[
                    {"role": "system", "content": DECOMPOSE_SYSTEM_PROMPT},
                    {"role": "user", "content": req.query},
                ],
                max_tokens=500,
                temperature=0,
                timeout=30,
            )
            json_match = re.search(r"\[.*\]", content, re.DOTALL)
            tasks_data = json.loads(json_match.group()) if json_match else []
        except Exception:
            tasks_data = []

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

        yield _sse("decompose", {
            "sub_tasks": sub_tasks,
            "total_cost": sum(t["cost"] for t in sub_tasks),
        })

        completed: dict[int, dict] = {}
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
                    cache_key=task["description"],
                )
                task["result"] = result
                task["status"] = "done"
            except Exception as e:
                task["result"] = f"执行失败: {e}"
                task["status"] = "failed"
            task["duration"] = time.time() - start
            task["tokens"] = 0
            completed[task["id"]] = task

            yield _sse("task_done", {"id": task["id"], "model": task["selected_model"],
                                     "duration": task["duration"], "cost": task["cost"],
                                     "tokens": 0})

        # Aggregate
        summary_parts = [f"## 子任务 {t['id']}: {t['description']}\n\n{t['result']}" for t in sub_tasks]
        agg_model = _model_by_name(req.models, "qwen-plus") or (
            req.models[0] if req.models else ModelSpec(name="qwen-plus", litellm_model="qwen-plus")
        )
        try:
            final = _call_llm(
                agg_model,
                messages=[
                    {"role": "system", "content": AGGREGATE_SYSTEM_PROMPT},
                    *req.history[-6:],
                    {"role": "user", "content": f"原始任务：{req.query}\n\n子任务执行结果：\n\n{chr(10).join(summary_parts)}\n\n请汇总以上结果，生成最终回答。"},
                ],
                max_tokens=4096,
                temperature=0.3,
                timeout=120,
            )
        except Exception:
            final = "\n\n---\n\n".join(f"**子任务 {t['id']}**: {t['result']}" for t in sub_tasks)

        total_cost = sum(t["cost"] for t in sub_tasks)
        yield _sse("result", {
            "response": final,
            "total_cost": total_cost,
            "total_tokens": 0,
            "total_duration": 0,
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


if __name__ == "__main__":
    import uvicorn

    uvicorn.run(app, host="0.0.0.0", port=7862)
