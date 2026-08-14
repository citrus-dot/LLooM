"""FastAPI server — REST + SSE endpoints for LLooM v2.

Endpoints:
  REST:  /api/health, /api/models, /api/usage, /api/budgets, /api/config, /api/stats
  SSE:   /api/chat/stream, /api/orchestrate/stream
  CRUD:  /api/conversations
"""

import json
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import litellm
from fastapi import FastAPI, HTTPException
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import StreamingResponse
from pydantic import BaseModel, Field

from core import database as db
from core.config import (
    get_api_port,
    get_conversations_dir,
    read_env_file,
    write_env_file,
)
from core.database import init_db
from core.model_manager import ModelManager
from core.orchestrator import TaskOrchestrator
from core.security import check as security_check
from core.security import extract_user_text
from core.smart_router import SmartRouter
from core.cache import get_cache
from core.seed_models import seed_models

# ── App setup ──

app = FastAPI(title="LLooM v2 API", version="2.0.0")

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_methods=["*"],
    allow_headers=["*"],
)

init_db()
seed_models()
mgr = ModelManager()
router = SmartRouter(mgr)
orchestrator = TaskOrchestrator(mgr, router)
try:
    get_cache().start()
except Exception:
    pass


# ── Pydantic request models ──


class ChatRequest(BaseModel):
    model: str = "auto"
    messages: list[dict]
    sr_domain: str = ""


class OrchestrateRequest(BaseModel):
    query: str
    history: list[dict] = Field(default_factory=list)
    sr_domain: str = ""


class ModelRegisterRequest(BaseModel):
    name: str
    provider: str
    litellm_model: str
    api_base: str = ""
    api_key_env: str = ""
    task_type: str = "general"
    input_cost_per_token: float = 0.0
    output_cost_per_token: float = 0.0
    rpm: int = 60


class ModelUpdateRequest(BaseModel):
    api_base: str | None = None
    api_key_env: str | None = None
    task_type: str | None = None
    input_cost_per_token: float | None = None
    output_cost_per_token: float | None = None
    rpm: int | None = None
    is_active: int | None = None


class BudgetRequest(BaseModel):
    scope: str
    scope_id: str
    max_budget: float
    duration: str = "30d"


class ConfigUpdate(BaseModel):
    updates: dict[str, str]


class ConversationSave(BaseModel):
    id: str = ""
    title: str = ""
    messages: list[dict] = Field(default_factory=list)


# ── Health ──


@app.get("/api/health")
async def health():
    return {"status": "ok", "version": "2.0.0"}


# ── Models ──


@app.get("/api/models")
async def list_models(active_only: bool = True):
    return {"models": db.list_models(active_only)}


@app.post("/api/models")
async def register_model(req: ModelRegisterRequest):
    row_id = db.insert_model({
        "name": req.name,
        "provider": req.provider,
        "litellm_model": req.litellm_model,
        "api_base": req.api_base,
        "api_key_env": req.api_key_env,
        "task_type": req.task_type,
        "input_cost_per_token": req.input_cost_per_token,
        "output_cost_per_token": req.output_cost_per_token,
        "rpm": req.rpm,
    })
    if row_id is None:
        raise HTTPException(409, f"Model '{req.name}' already exists")
    return {"id": row_id, "name": req.name}


@app.get("/api/models/{name}")
async def get_model(name: str):
    model = db.get_model(name)
    if not model or not model.get("is_active"):
        raise HTTPException(404, f"Model '{name}' not found")
    return model


@app.put("/api/models/{name}")
async def update_model(name: str, req: ModelUpdateRequest):
    updates = {k: v for k, v in req.model_dump().items() if v is not None}
    if not updates:
        raise HTTPException(400, "No fields to update")
    if not db.update_model(name, updates):
        raise HTTPException(404, f"Model '{name}' not found")
    return {"updated": True}


@app.delete("/api/models/{name}")
async def delete_model(name: str):
    if not db.delete_model(name):
        raise HTTPException(404, f"Model '{name}' not found")
    return {"deleted": True}


# ── Usage ──


@app.get("/api/usage")
async def get_usage(
    model_name: str | None = None,
    user_id: str | None = None,
    since: str | None = None,
):
    stats = db.get_usage_stats(model_name, user_id, since)
    total = db.get_total_spend(user_id, since)
    return {"usage": stats, "total_spend": total}


# ── Budgets ──


@app.get("/api/budgets")
async def list_budgets():
    return {"budgets": db.list_budgets()}


@app.post("/api/budgets")
async def set_budget(req: BudgetRequest):
    db.upsert_budget(req.scope, req.scope_id, req.max_budget, req.duration)
    return {"set": True}


@app.get("/api/budgets/check")
async def check_budget(scope: str, scope_id: str, prospective_cost: float = 0.0):
    budget = db.get_budget(scope, scope_id)
    if not budget:
        return {"within_budget": True, "budget": None}
    spent = db.get_total_spend(
        user_id=scope_id if scope == "user" else None,
        model_name=scope_id if scope == "model" else None,
    )
    within = (spent + prospective_cost) <= budget["max_budget"]
    return {"within_budget": within, "budget": budget, "spent": spent}


# ── Config ──


@app.get("/api/config")
async def get_config():
    return read_env_file()


@app.post("/api/config")
async def update_config(req: ConfigUpdate):
    write_env_file(req.updates)
    return {"updated": list(req.updates.keys())}


# ── Stats ──


@app.get("/api/stats")
async def get_stats():
    model_count = len(db.list_models(active_only=True))
    usage = db.get_usage_stats()
    total_spend = db.get_total_spend()
    routing_stats = router.get_stats() if router else {}
    cache = get_cache()
    return {
        "model_count": model_count,
        "total_spend": total_spend,
        "model_spend": usage,
        "routing_stats": routing_stats,
        "cache_enabled": cache.enabled,
    }


# ── SSE: Chat stream ──


def _sse(data: dict) -> str:
    return f"data: {json.dumps(data, ensure_ascii=False)}\n\n"


def _chat_stream_generator(
    messages: list[dict],
    model: str,
    sr_domain: str,
) -> Any:
    user_text = extract_user_text(messages)

    sec = security_check(user_text)
    if sec["blocked"]:
        yield _sse({
            "error": True,
            "block_reason": sec["block_reason"],
            "detail": sec.get("pii") or sec.get("jailbreak"),
        })
        return

    processed_messages = list(messages)
    if sec["processed_text"] != user_text:
        for msg in reversed(processed_messages):
            if msg.get("role") == "user":
                msg["content"] = sec["processed_text"]
                break

    routing = router.route(model, processed_messages, sr_domain)
    final_model = routing["model"]
    stream = routing["stream"]

    yield _sse({
        "routing": routing,
        "security": {
            "domain": sec["domain"],
            "domain_method": sec["domain_method"],
            "pii": sec["pii"],
        },
    })

    params = mgr.get_litellm_params(final_model) or {"model": final_model}
    params["messages"] = processed_messages
    if stream:
        params["stream"] = True

    content_parts: list[str] = []
    input_tokens = 0
    output_tokens = 0

    try:
        if stream:
            for chunk in litellm.completion(**params):
                delta = chunk.choices[0].delta
                if delta and delta.content:
                    content_parts.append(delta.content)
                    yield _sse({"chunk": delta.content})
        else:
            response = litellm.completion(**params)
            content = response.choices[0].message.content
            content_parts.append(content)
            yield _sse({"chunk": content})
            usage = getattr(response, "usage", None)
            if usage:
                input_tokens = getattr(usage, "prompt_tokens", 0) or 0
                output_tokens = getattr(usage, "completion_tokens", 0) or 0

    except Exception as e:
        yield _sse({"error": True, "detail": str(e)})
        return

    full_content = "".join(content_parts)
    cost = mgr.calculate_cost(final_model, input_tokens, output_tokens)
    db.insert_usage(
        final_model, input_tokens, output_tokens, cost,
        task_type=routing["task_type"],
    )

    yield _sse({
        "done": True,
        "content": full_content,
        "model": final_model,
        "task_type": routing["task_type"],
        "method": routing["method"],
        "cost": cost,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
    })


@app.post("/api/chat/stream")
async def chat_stream(req: ChatRequest):
    return StreamingResponse(
        _chat_stream_generator(req.messages, req.model, req.sr_domain),
        media_type="text/event-stream",
    )


# ── SSE: Orchestrate stream ──


@app.post("/api/orchestrate/stream")
async def orchestrate_stream(req: OrchestrateRequest):
    sec = security_check(req.query)
    if sec["blocked"]:
        def blocked_gen():
            yield _sse({
                "error": True,
                "block_reason": sec["block_reason"],
                "detail": sec.get("pii") or sec.get("jailbreak"),
            })
        return StreamingResponse(blocked_gen(), media_type="text/event-stream")

    query = sec["processed_text"]
    domain = sec["domain"] or req.sr_domain

    def gen():
        for event in orchestrator.orchestrate_stream(
            query, history=req.history, sr_domain=domain
        ):
            yield event

    return StreamingResponse(gen(), media_type="text/event-stream")


# ── Conversations CRUD ──


def _conv_path(conv_id: str) -> Path:
    return get_conversations_dir() / f"{conv_id}.json"


@app.get("/api/conversations")
async def list_conversations():
    conv_dir = get_conversations_dir()
    conversations = []
    for f in sorted(conv_dir.glob("*.json"), key=lambda x: x.stat().st_mtime, reverse=True):
        try:
            data = json.loads(f.read_text())
            conversations.append({
                "id": data.get("id", f.stem),
                "title": data.get("title", ""),
                "message_count": len(data.get("messages", [])),
                "updated_at": data.get("updated_at", ""),
            })
        except Exception:
            continue
    return {"conversations": conversations}


@app.get("/api/conversations/{conv_id}")
async def get_conversation(conv_id: str):
    path = _conv_path(conv_id)
    if not path.exists():
        raise HTTPException(404, f"Conversation '{conv_id}' not found")
    return json.loads(path.read_text())


@app.post("/api/conversations")
async def save_conversation(req: ConversationSave):
    conv_id = req.id or uuid.uuid4().hex[:12]
    now = datetime.now(timezone.utc).isoformat()
    data = {
        "id": conv_id,
        "title": req.title or _auto_title(req.messages),
        "messages": req.messages,
        "updated_at": now,
    }
    path = _conv_path(conv_id)
    if path.exists():
        existing = json.loads(path.read_text())
        data["created_at"] = existing.get("created_at", now)
    else:
        data["created_at"] = now
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2))
    return {"id": conv_id, "saved": True}


@app.delete("/api/conversations/{conv_id}")
async def delete_conversation(conv_id: str):
    path = _conv_path(conv_id)
    if not path.exists():
        raise HTTPException(404, f"Conversation '{conv_id}' not found")
    path.unlink()
    return {"deleted": True}


def _auto_title(messages: list[dict]) -> str:
    for msg in messages:
        if msg.get("role") == "user":
            content = msg.get("content", "")
            if isinstance(content, str):
                return content[:20]
    return "新对话"


# ── Entry point ──

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=get_api_port())
