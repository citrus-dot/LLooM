"""LiteLLM custom callback — records usage to SQLite after every completion."""

import litellm
from core import database as db


class UsageTrackerCallback:
    """Custom callback handler that logs token usage and cost to SQLite.

    Register with litellm via:
        litellm.success_callback = [UsageTrackerCallback()]
    """

    def __init__(self, user_id: str = "default"):
        self.user_id = user_id

    def log_success_event(self, kwargs, completion_obj, start_time, end_time):
        """Called by litellm after a successful completion."""
        try:
            response = completion_obj[0] if isinstance(completion_obj, list) else completion_obj
            usage = getattr(response, "usage", None)
            if not usage:
                return

            model_name = kwargs.get("model", "unknown")
            input_tokens = getattr(usage, "prompt_tokens", 0) or 0
            output_tokens = getattr(usage, "completion_tokens", 0) or 0

            cost = self._calculate_cost(model_name, input_tokens, output_tokens)

            metadata = kwargs.get("litellm_metadata", {}) or {}
            task_type = metadata.get("task_type")
            user_id = metadata.get("user_id", self.user_id)
            cache_hit = metadata.get("cache_hit", False)

            db.insert_usage(
                model_name=model_name,
                input_tokens=input_tokens,
                output_tokens=output_tokens,
                cost=cost,
                user_id=user_id,
                task_type=task_type,
                cache_hit=cache_hit,
            )
        except Exception:
            pass

    def _calculate_cost(self, model_name: str, input_tokens: int, output_tokens: int) -> float:
        model = db.get_model(model_name)
        if not model:
            return litellm.completion_cost(
                model=model_name,
                prompt="x" * input_tokens,
                completion="x" * output_tokens,
            )
        return (
            input_tokens * model["input_cost_per_token"]
            + output_tokens * model["output_cost_per_token"]
        )


_tracker: UsageTrackerCallback | None = None


def get_tracker() -> UsageTrackerCallback:
    global _tracker
    if _tracker is None:
        _tracker = UsageTrackerCallback()
    return _tracker


def install(user_id: str = "default") -> UsageTrackerCallback:
    """Install the usage tracker as a litellm success callback."""
    global _tracker
    _tracker = UsageTrackerCallback(user_id=user_id)
    if _tracker not in litellm.success_callback:
        litellm.success_callback.append(_tracker)
    return _tracker
