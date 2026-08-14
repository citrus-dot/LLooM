"""Phase 1 unit tests — ModelManager, database CRUD, budget, callbacks."""

import os
import sys
import tempfile
import shutil

# Set test data dir before importing core modules
_test_dir = tempfile.mkdtemp(prefix="lloom_test_")
os.environ["LLOOM_DATA_DIR"] = _test_dir

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from core import database as db
from core.model_manager import ModelManager

PASS = 0
FAIL = 0


def check(name: str, condition: bool):
    global PASS, FAIL
    if condition:
        PASS += 1
        print(f"  ✓ {name}")
    else:
        FAIL += 1
        print(f"  ✗ {name}")


def reset_db():
    """Drop all tables and recreate, giving each test a fresh DB."""
    from core.config import get_db_path
    db_path = get_db_path()
    if db_path.exists():
        db_path.unlink()
    db.init_db()


def test_model_crud():
    print("\n[1] Model CRUD")
    mgr = ModelManager()
    reset_db()

    # Register
    row_id = mgr.register_model(
        name="test-model",
        provider="dashscope",
        litellm_model="openai/test-model",
        api_base="https://test.api.com",
        api_key_env="TEST_API_KEY",
        task_type="general",
        input_cost_per_token=0.000001,
        output_cost_per_token=0.000002,
        rpm=60,
    )
    check("register_model returns row id", row_id is not None)

    # Duplicate name
    dup = mgr.register_model(name="test-model", provider="x", litellm_model="x")
    check("duplicate name rejected", dup is None)

    # Get
    model = mgr.get_model("test-model")
    check("get_model returns correct name", model and model["name"] == "test-model")
    check("get_model has correct pricing", model and model["input_cost_per_token"] == 0.000001)

    # List
    models = mgr.list_models()
    check("list_models returns 1 active model", len(models) == 1)

    # Update
    updated = mgr.update_model("test-model", {"rpm": 120, "task_type": "coding"})
    check("update_model succeeds", updated)
    model = mgr.get_model("test-model")
    check("rpm updated to 120", model["rpm"] == 120)
    check("task_type updated to coding", model["task_type"] == "coding")

    # Soft delete
    deleted = mgr.remove_model("test-model")
    check("remove_model (soft delete) succeeds", deleted)
    models = mgr.list_models(active_only=True)
    check("list_models active_only=0 after delete", len(models) == 0)
    models = mgr.list_models(active_only=False)
    check("list_models all=1 after delete", len(models) == 1)


def test_usage_tracking():
    print("\n[2] Usage Tracking")
    mgr = ModelManager()
    reset_db()

    # Register a model for testing
    mgr.register_model(
        name="usage-test",
        provider="dashscope",
        litellm_model="openai/test",
        input_cost_per_token=0.000001,
        output_cost_per_token=0.000002,
    )

    # Record usage
    uid = mgr.record_usage(
        model_name="usage-test",
        input_tokens=1000,
        output_tokens=500,
        cost=0.002,
        user_id="test_user",
        task_type="general",
    )
    check("record_usage returns id", uid > 0)

    # Record more usage
    mgr.record_usage("usage-test", 2000, 1000, 0.004, user_id="test_user")
    mgr.record_usage("usage-test", 500, 200, 0.001, user_id="other_user")

    # Usage summary
    stats = mgr.get_usage_summary(user_id="test_user")
    check("usage_summary for test_user has 2 records", len(stats) == 1 and stats[0]["request_count"] == 2)
    check("total input tokens = 3000", stats[0]["total_input_tokens"] == 3000)
    check("total cost = 0.006", abs(stats[0]["total_cost"] - 0.006) < 0.0001)

    # Total spend
    spend = mgr.get_total_spend(user_id="test_user")
    check("total_spend = 0.006", abs(spend - 0.006) < 0.0001)

    spend_all = mgr.get_total_spend()
    check("total_spend all = 0.007", abs(spend_all - 0.007) < 0.0001)


def test_budget():
    print("\n[3] Budget Control")
    mgr = ModelManager()
    reset_db()

    # Set budget
    mgr.set_budget("user", "budget_user", max_budget=5.0, duration="30d")
    budget = mgr.get_budget("user", "budget_user")
    check("budget created", budget is not None)
    check("budget max = 5.0", budget["max_budget"] == 5.0)
    check("budget duration = 30d", budget["duration"] == "30d")

    # Update budget
    mgr.set_budget("user", "budget_user", max_budget=10.0, duration="7d")
    budget = mgr.get_budget("user", "budget_user")
    check("budget updated max = 10.0", budget["max_budget"] == 10.0)

    # Budget check
    mgr.register_model(
        name="budget-test",
        provider="x",
        litellm_model="x",
        input_cost_per_token=0.001,
        output_cost_per_token=0.001,
    )
    mgr.record_usage("budget-test", 1000, 1000, cost=2.0, user_id="budget_user")
    check("check_budget allows (2.0 < 10.0)", mgr.check_budget("user", "budget_user") is True)

    # Fill up to near limit
    mgr.record_usage("budget-test", 1000, 1000, cost=7.0, user_id="budget_user")
    # Now spent = 9.0, budget = 10.0
    check("check_budget allows (9.0 < 10.0)", mgr.check_budget("user", "budget_user") is True)
    check("check_budget blocks prospective 2.0 (9+2>10)", mgr.check_budget("user", "budget_user", prospective_cost=2.0) is False)

    # No budget = always allowed
    check("check_budget no budget = True", mgr.check_budget("user", "no_budget_user") is True)


def test_cost_calculation():
    print("\n[4] Cost Calculation")
    mgr = ModelManager()
    reset_db()

    mgr.register_model(
        name="cost-test",
        provider="x",
        litellm_model="x",
        input_cost_per_token=0.00000111,
        output_cost_per_token=0.00000278,
    )

    cost = mgr.calculate_cost("cost-test", input_tokens=10000, output_tokens=5000)
    expected = 10000 * 0.00000111 + 5000 * 0.00000278
    check(f"cost calculation = {cost:.6f} (expected {expected:.6f})", abs(cost - expected) < 0.000001)

    # Unknown model
    cost_unknown = mgr.calculate_cost("nonexistent", 1000, 1000)
    check("unknown model cost = 0", cost_unknown == 0.0)


def test_litellm_params():
    print("\n[5] litellm Params")
    mgr = ModelManager()
    reset_db()
    import os as _os

    _os.environ["DASHSCOPE_API_BASE"] = "https://dashscope.aliyuncs.com/compatible-mode/v1"
    _os.environ["DASHSCOPE_API_KEY"] = "test-key-123"

    mgr.register_model(
        name="param-test",
        provider="dashscope",
        litellm_model="openai/qwen-plus",
        api_base="DASHSCOPE_API_BASE",
        api_key_env="DASHSCOPE_API_KEY",
    )

    params = mgr.get_litellm_params("param-test")
    check("params has model", params and params.get("model") == "openai/qwen-plus")
    check("params has api_base (env resolved)", params and params.get("api_base", "").startswith("http"))
    check("params has api_key (env resolved)", params and params.get("api_key") == "test-key-123")

    mgr.register_model(
        name="direct-url-test",
        provider="ollama",
        litellm_model="ollama/qwen2.5:latest",
        api_base="http://localhost:11434",
    )
    params2 = mgr.get_litellm_params("direct-url-test")
    check("direct url in api_base", params2 and params2.get("api_base") == "http://localhost:11434")

    params_none = mgr.get_litellm_params("nonexistent")
    check("nonexistent returns None", params_none is None)


def test_seed():
    print("\n[6] Seed Models")
    from core.seed_models import seed_models, DEFAULT_MODELS
    reset_db()
    seed_models()
    models = ModelManager().list_models()
    check(f"seeded {len(DEFAULT_MODELS)} models", len(models) == len(DEFAULT_MODELS))
    check("qwen-plus in list", any(m["name"] == "qwen-plus" for m in models))
    check("qwen2.5-local in list", any(m["name"] == "qwen2.5-local" for m in models))
    check("gpt-4o in list", any(m["name"] == "gpt-4o" for m in models))

    # Re-run seed should skip
    seed_models()
    check("re-run seed doesn't duplicate", len(ModelManager().list_models()) == len(DEFAULT_MODELS))


def main():
    print("=" * 60)
    print("LLooM v2 — Phase 1 Unit Tests")
    print("=" * 60)

    test_model_crud()
    test_usage_tracking()
    test_budget()
    test_cost_calculation()
    test_litellm_params()
    test_seed()

    print("\n" + "=" * 60)
    print(f"Results: {PASS} passed, {FAIL} failed")
    print("=" * 60)

    # Cleanup
    shutil.rmtree(_test_dir, ignore_errors=True)

    return 1 if FAIL > 0 else 0


if __name__ == "__main__":
    sys.exit(main())
