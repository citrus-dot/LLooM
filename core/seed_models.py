"""Seed database with default model pricing data migrated from v1 config."""

from core import database as db
from core.model_manager import ModelManager

DEFAULT_MODELS = [
    {
        "name": "qwen-plus",
        "provider": "dashscope",
        "litellm_model": "openai/qwen-plus",
        "api_base": "DASHSCOPE_API_BASE",
        "api_key_env": "DASHSCOPE_API_KEY",
        "task_type": "general",
        "input_cost_per_token": 0.00000111,
        "output_cost_per_token": 0.00000278,
        "rpm": 60,
    },
    {
        "name": "qwen3.6-flash",
        "provider": "dashscope",
        "litellm_model": "openai/qwen3.6-flash",
        "api_base": "DASHSCOPE_API_BASE",
        "api_key_env": "DASHSCOPE_API_KEY",
        "task_type": "classification",
        "input_cost_per_token": 0.00000167,
        "output_cost_per_token": 0.00001,
        "rpm": 60,
    },
    {
        "name": "qwen3.6-plus",
        "provider": "dashscope",
        "litellm_model": "openai/qwen3.6-plus",
        "api_base": "DASHSCOPE_API_BASE",
        "api_key_env": "DASHSCOPE_API_KEY",
        "task_type": "complex_reasoning",
        "input_cost_per_token": 0.00000278,
        "output_cost_per_token": 0.00001667,
        "rpm": 60,
    },
    {
        "name": "qwen3-max",
        "provider": "dashscope",
        "litellm_model": "openai/qwen3-max",
        "api_base": "DASHSCOPE_API_BASE",
        "api_key_env": "DASHSCOPE_API_KEY",
        "task_type": "complex_reasoning",
        "input_cost_per_token": 0.00000347,
        "output_cost_per_token": 0.00001389,
        "rpm": 60,
    },
    {
        "name": "deepseek-v3",
        "provider": "dashscope",
        "litellm_model": "openai/deepseek-v3",
        "api_base": "DASHSCOPE_API_BASE",
        "api_key_env": "DASHSCOPE_API_KEY",
        "task_type": "complex_reasoning",
        "input_cost_per_token": 0.00000139,
        "output_cost_per_token": 0.00001111,
        "rpm": 60,
    },
    {
        "name": "qwen2.5-local",
        "provider": "ollama",
        "litellm_model": "ollama/qwen2.5:latest",
        "api_base": "OLLAMA_API_BASE",
        "api_key_env": "",
        "task_type": "general",
        "input_cost_per_token": 0.0,
        "output_cost_per_token": 0.0,
        "rpm": 30,
    },
    {
        "name": "gpt-4o",
        "provider": "openai",
        "litellm_model": "gpt-4o",
        "api_base": "",
        "api_key_env": "OPENAI_API_KEY",
        "task_type": "general",
        "input_cost_per_token": 0.0000025,
        "output_cost_per_token": 0.00001,
        "rpm": 60,
    },
]

DEFAULT_BUDGETS = [
    {"scope": "user", "scope_id": "default", "max_budget": 10.0, "duration": "30d"},
]


def seed_models() -> None:
    """Insert default models if database is empty."""
    db.init_db()
    existing = db.list_models(active_only=False)
    if existing:
        print(f"Database already has {len(existing)} models, skipping seed.")
        return
    mgr = ModelManager()
    for m in DEFAULT_MODELS:
        row_id = mgr.register_model(**m)
        status = f"  ✓ {m['name']}" if row_id else f"  ✗ {m['name']} (skipped)"
        print(status)
    for b in DEFAULT_BUDGETS:
        mgr.set_budget(**b)
        print(f"  ✓ budget {b['scope']}/{b['scope_id']} = ${b['max_budget']}/{b['duration']}")
    print(f"Seeded {len(DEFAULT_MODELS)} models and {len(DEFAULT_BUDGETS)} budgets.")


if __name__ == "__main__":
    seed_models()
