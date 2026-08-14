"""LLooM v2 CLI — init, model, status, chat, orchestrate, serve.

Usage:
  lloom init                    Initialize database and seed models
  lloom model list              List registered models
  lloom model add               Add a model (interactive)
  lloom model remove <name>     Remove a model
  lloom status                  Show system status
  lloom chat "你好"             Chat with auto-routing
  lloom orchestrate "复杂任务"   Orchestrate a complex task
  lloom serve                   Start API server
"""

import json as json_lib
import os
import sys

import click

# Ensure project root is on sys.path
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


@click.group()
@click.version_option("2.0.0", prog_name="LLooM")
def cli():
    """LLooM v2 — Intelligent LLM Routing Platform."""
    pass


# ── init ──


@cli.command()
def init():
    """Initialize database and seed default models."""
    from core.database import init_db
    from core.seed_models import seed_models

    click.echo("=== LLooM v2 Initialization ===")

    env_path = ".env"
    if not os.path.exists(env_path):
        example = os.path.join(os.path.dirname(os.path.dirname(__file__)), ".env.example")
        if os.path.exists(example):
            import shutil
            shutil.copy(example, env_path)
            click.echo(f"  Created .env from .env.example")
        else:
            click.echo("  .env not found, using environment variables")

    init_db()
    click.echo("  Database initialized")
    seed_models()
    click.echo("  Done! Run 'lloom status' to verify.")


# ── model ──


@cli.group()
def model():
    """Model management."""
    pass


@model.command(name="list")
def model_list():
    """List registered models."""
    from core import database as db
    from core.database import init_db

    init_db()
    models = db.list_models(active_only=True)
    if not models:
        click.echo("No models registered. Run 'lloom init' first.")
        return

    click.echo(f"\n{'Name':<18} {'Provider':<12} {'LiteLLM Model':<28} "
                f"{'In $/1K':>10} {'Out $/1K':>10} {'RPM':>5}")
    click.echo("-" * 90)
    for m in models:
        in_cost = m["input_cost_per_token"] * 1000
        out_cost = m["output_cost_per_token"] * 1000
        click.echo(f"{m['name']:<18} {m['provider']:<12} {m['litellm_model']:<28} "
                    f"{in_cost:>10.6f} {out_cost:>10.6f} {m['rpm']:>5}")
    click.echo(f"\nTotal: {len(models)} active models")


@model.command(name="add")
@click.option("--name", prompt="Model name (e.g. qwen-plus)")
@click.option("--provider", prompt="Provider (dashscope/openai/ollama)")
@click.option("--litellm-model", prompt="LiteLLM model (e.g. openai/qwen-plus)")
@click.option("--api-base", default="", help="API base URL or env var name")
@click.option("--api-key-env", default="", help="Env var name for API key")
@click.option("--task-type", default="general", help="Task type")
@click.option("--input-cost", type=float, default=0.0, help="Input cost per token (USD)")
@click.option("--output-cost", type=float, default=0.0, help="Output cost per token (USD)")
@click.option("--rpm", type=int, default=60, help="Requests per minute limit")
def model_add(name, provider, litellm_model, api_base, api_key_env,
              task_type, input_cost, output_cost, rpm):
    """Add a new model."""
    from core import database as db
    from core.database import init_db

    init_db()
    row_id = db.insert_model({
        "name": name,
        "provider": provider,
        "litellm_model": litellm_model,
        "api_base": api_base,
        "api_key_env": api_key_env,
        "task_type": task_type,
        "input_cost_per_token": input_cost,
        "output_cost_per_token": output_cost,
        "rpm": rpm,
    })
    if row_id is None:
        click.echo(f"Model '{name}' already exists!", err=True)
        sys.exit(1)
    click.echo(f"Added model '{name}' (id={row_id})")


@model.command(name="remove")
@click.argument("name")
def model_remove(name):
    """Remove a model (soft-delete)."""
    from core import database as db
    from core.database import init_db

    init_db()
    if not db.delete_model(name):
        click.echo(f"Model '{name}' not found!", err=True)
        sys.exit(1)
    click.echo(f"Removed model '{name}'")


# ── status ──


@cli.command()
def status():
    """Show system status."""
    from core import database as db
    from core.database import init_db
    from core.config import get_env
    from core.cache import get_cache

    init_db()
    models = db.list_models(active_only=True)
    total_spend = db.get_total_spend()
    usage = db.get_usage_stats()
    budgets = db.list_budgets()

    click.echo("=== LLooM v2 Status ===")
    click.echo(f"\nModels: {len(models)} active")
    for m in models:
        click.echo(f"  - {m['name']:<18} ({m['provider']})")

    click.echo(f"\nUsage: ${total_spend:.6f} total spend")
    for u in usage[:5]:
        click.echo(f"  {u['model_name']:<18} "
                    f"req={u['request_count']:>4} "
                    f"in={u['total_input_tokens']:>8} "
                    f"out={u['total_output_tokens']:>8} "
                    f"${u['total_cost']:.6f}")

    click.echo(f"\nBudgets: {len(budgets)}")
    for b in budgets:
        click.echo(f"  {b['scope']}/{b['scope_id']}: "
                    f"${b['max_budget']}/{b['duration']}")

    dashscope = bool(get_env("DASHSCOPE_API_KEY"))
    ollama_base = get_env("OLLAMA_API_BASE", "http://localhost:11434")
    click.echo(f"\nConfig:")
    click.echo(f"  DashScope API Key: {'✓ set' if dashscope else '✗ not set'}")
    click.echo(f"  Ollama base: {ollama_base}")

    cache = get_cache()
    click.echo(f"  Semantic cache: {'enabled' if cache.enabled else 'disabled'}")


# ── chat ──


@cli.command()
@click.argument("message")
@click.option("--model", default="auto", help="Model name or 'auto' for smart routing")
def chat(message, model):
    """Chat with auto-routing. Streams response to stdout."""
    from core.database import init_db
    from core.seed_models import seed_models
    from core.model_manager import ModelManager
    from core.smart_router import SmartRouter
    from core.security import check as security_check
    from core.cache import get_cache
    from core import database as db
    import litellm

    init_db()
    seed_models()
    get_cache()._enabled = False

    mgr = ModelManager()
    router = SmartRouter(mgr)

    sec = security_check(message)
    if sec["blocked"]:
        click.echo(f"[Blocked: {sec['block_reason']}]")
        return

    text = sec["processed_text"]
    messages = [{"role": "user", "content": text}]

    routing = router.route(model, messages, sec.get("domain", ""))
    final_model = routing["model"]
    click.echo(f"[routed: {final_model} via {routing['method']} "
                f"(task={routing['task_type']})]\n")

    params = mgr.get_litellm_params(final_model) or {"model": final_model}
    params["messages"] = messages
    if routing["stream"]:
        params["stream"] = True

    try:
        if routing["stream"]:
            for chunk in litellm.completion(**params):
                delta = chunk.choices[0].delta
                if delta and delta.content:
                    click.echo(delta.content, nl=False)
            click.echo()
        else:
            response = litellm.completion(**params)
            content = response.choices[0].message.content
            click.echo(content)
    except Exception as e:
        click.echo(f"\n[Error: {e}]", err=True)


# ── orchestrate ──


@cli.command()
@click.argument("query")
def orchestrate(query):
    """Orchestrate a complex task with decomposition."""
    from core.database import init_db
    from core.seed_models import seed_models
    from core.model_manager import ModelManager
    from core.smart_router import SmartRouter
    from core.orchestrator import TaskOrchestrator
    from core.security import check as security_check
    from core.cache import get_cache

    init_db()
    seed_models()
    get_cache()._enabled = False

    sec = security_check(query)
    if sec["blocked"]:
        click.echo(f"[Blocked: {sec['block_reason']}]")
        return

    text = sec["processed_text"]
    domain = sec.get("domain", "")

    mgr = ModelManager()
    router = SmartRouter(mgr)
    orch = TaskOrchestrator(mgr, router)

    click.echo(f"[domain: {domain or 'N/A'}]")

    for event in orch.orchestrate_stream(text, sr_domain=domain):
        if event.startswith("event: "):
            lines = event.strip().split("\n")
            event_type = lines[0].replace("event: ", "")
            data_line = "\n".join(lines[1:])
            if data_line.startswith("data: "):
                data = json_lib.loads(data_line[6:])
                if event_type == "decompose":
                    subs = data.get("sub_tasks", [])
                    click.echo(f"\n[Decomposed into {len(subs)} subtasks]")
                    for s in subs:
                        click.echo(f"  #{s['id']} {s.get('description', '')[:60]} "
                                    f"→ {s.get('selected_model', '?')}")
                elif event_type == "task_start":
                    click.echo(f"\n--- Task #{data['id']}: "
                                f"{data.get('description', '')[:60]} ---")
                elif event_type == "task_done":
                    click.echo(f"    [done in {data.get('duration', 0):.1f}s, "
                                f"cost=${data.get('cost', 0):.6f}]")
                elif event_type == "result":
                    click.echo(f"\n{'=' * 60}")
                    click.echo(data.get("response", ""))
                    click.echo(f"{'=' * 60}")
                    click.echo(f"Total cost: ${data.get('total_cost', 0):.6f}")


# ── serve ──


@cli.command()
@click.option("--port", default=None, type=int, help="Override port")
def serve(port):
    """Start the API server."""
    import uvicorn
    from core.config import get_api_port

    p = port or get_api_port()
    click.echo(f"Starting LLooM API on port {p}...")
    uvicorn.run("api.server:app", host="0.0.0.0", port=p)


if __name__ == "__main__":
    cli()
