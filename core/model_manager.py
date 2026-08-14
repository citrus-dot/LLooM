"""ModelManager — model registration, pricing, usage tracking, budget control."""

import os
from typing import Any

from core import database as db
from core.config import get_env


class ModelManager:
    """Manages model lifecycle, usage tracking, and budget enforcement."""

    def register_model(
        self,
        name: str,
        provider: str,
        litellm_model: str,
        api_base: str = "",
        api_key_env: str = "",
        task_type: str = "general",
        input_cost_per_token: float = 0.0,
        output_cost_per_token: float = 0.0,
        rpm: int = 60,
    ) -> int | None:
        """Register a new model. Returns row id, or None if name already exists."""
        return db.insert_model({
            "name": name,
            "provider": provider,
            "litellm_model": litellm_model,
            "api_base": api_base,
            "api_key_env": api_key_env,
            "task_type": task_type,
            "input_cost_per_token": input_cost_per_token,
            "output_cost_per_token": output_cost_per_token,
            "rpm": rpm,
        })

    def remove_model(self, name: str) -> bool:
        """Soft-delete a model (sets is_active=0)."""
        return db.delete_model(name)

    def get_model(self, name: str) -> dict | None:
        return db.get_model(name)

    def list_models(self, active_only: bool = True) -> list[dict]:
        return db.list_models(active_only)

    def update_model(self, name: str, updates: dict) -> bool:
        return db.update_model(name, updates)

    def get_litellm_params(self, name: str) -> dict[str, Any] | None:
        """Return litellm-compatible params dict for a registered model."""
        model = db.get_model(name)
        if not model:
            return None
        params: dict[str, Any] = {"model": model["litellm_model"]}
        if model["api_base"]:
            api_base = get_env(model["api_base"]) or model["api_base"]
            if api_base.startswith("http"):
                params["api_base"] = api_base
        if model["api_key_env"]:
            api_key = get_env(model["api_key_env"])
            if api_key:
                params["api_key"] = api_key
        return params

    # ── Usage ──

    def record_usage(
        self,
        model_name: str,
        input_tokens: int,
        output_tokens: int,
        cost: float,
        user_id: str = "default",
        task_type: str | None = None,
        cache_hit: bool = False,
    ) -> int:
        return db.insert_usage(
            model_name, input_tokens, output_tokens, cost,
            user_id, task_type, cache_hit,
        )

    def get_usage_summary(
        self,
        model_name: str | None = None,
        user_id: str | None = None,
        since: str | None = None,
    ) -> list[dict]:
        return db.get_usage_stats(model_name, user_id, since)

    def get_total_spend(
        self,
        user_id: str | None = None,
        since: str | None = None,
    ) -> float:
        return db.get_total_spend(user_id, since)

    # ── Budget ──

    def set_budget(self, scope: str, scope_id: str, max_budget: float, duration: str) -> None:
        """Set or update a budget. scope: 'user'|'model'. scope_id: user_id or model_name."""
        db.upsert_budget(scope, scope_id, max_budget, duration)

    def get_budget(self, scope: str, scope_id: str) -> dict | None:
        return db.get_budget(scope, scope_id)

    def list_budgets(self) -> list[dict]:
        return db.list_budgets()

    def check_budget(self, scope: str, scope_id: str, prospective_cost: float = 0.0) -> bool:
        """Return True if a new call would stay within budget."""
        budget = db.get_budget(scope, scope_id)
        if not budget:
            return True
        spent = db.get_total_spend(
            user_id=scope_id if scope == "user" else None,
            model_name=scope_id if scope == "model" else None,
        )
        return (spent + prospective_cost) <= budget["max_budget"]

    # ── Pricing helpers ──

    def calculate_cost(self, model_name: str, input_tokens: int, output_tokens: int) -> float:
        model = db.get_model(model_name)
        if not model:
            return 0.0
        return (
            input_tokens * model["input_cost_per_token"]
            + output_tokens * model["output_cost_per_token"]
        )
