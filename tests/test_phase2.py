"""Phase 2 unit tests — SmartRouter classification, routing, fallback chains."""

import os
import sys
import tempfile
import shutil

_test_dir = tempfile.mkdtemp(prefix="lloom_test_")
os.environ["LLOOM_DATA_DIR"] = _test_dir

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from core import database as db
from core.model_manager import ModelManager
from core.smart_router import (
    SmartRouter,
    TASK_MODEL_MAP,
    TASK_RULES,
    INFERENCE_MODELS,
    AUTO_MODEL_NAMES,
    DEFAULT_MODEL,
)

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
    from core.config import get_db_path
    db_path = get_db_path()
    if db_path.exists():
        db_path.unlink()
    db.init_db()


def seed_test_models():
    """Seed minimal models for routing tests."""
    mgr = ModelManager()
    models = [
        ("qwen-plus", "dashscope", "openai/qwen-plus", "DASHSCOPE_API_BASE", "DASHSCOPE_API_KEY", "general"),
        ("qwen3.6-flash", "dashscope", "openai/qwen3.6-flash", "DASHSCOPE_API_BASE", "DASHSCOPE_API_KEY", "classification"),
        ("qwen3.6-plus", "dashscope", "openai/qwen3.6-plus", "DASHSCOPE_API_BASE", "DASHSCOPE_API_KEY", "complex_reasoning"),
        ("qwen3-max", "dashscope", "openai/qwen3-max", "DASHSCOPE_API_BASE", "DASHSCOPE_API_KEY", "complex_reasoning"),
        ("deepseek-v3", "dashscope", "openai/deepseek-v3", "DASHSCOPE_API_BASE", "DASHSCOPE_API_KEY", "complex_reasoning"),
        ("qwen2.5-local", "ollama", "ollama/qwen2.5:latest", "OLLAMA_API_BASE", "", "general"),
        ("gpt-4o", "openai", "gpt-4o", "", "OPENAI_API_KEY", "general"),
    ]
    for name, provider, litellm_model, api_base, api_key_env, task_type in models:
        mgr.register_model(
            name=name, provider=provider, litellm_model=litellm_model,
            api_base=api_base, api_key_env=api_key_env, task_type=task_type,
            input_cost_per_token=0.000001, output_cost_per_token=0.000002,
        )


def test_rule_classify():
    print("\n[1] Rule-based Classification")
    reset_db()
    seed_test_models()
    router = SmartRouter()

    # complex_reasoning
    cases = [
        ("请分析这个方案的优缺点", "complex_reasoning"),
        ("compare these two architectures", "complex_reasoning"),
        ("评估这个策略的效果", "complex_reasoning"),
        # coding
        ("帮我写代码实现这个功能", "coding"),
        ("write a python function", "coding"),
        ("帮我debug这个bug", "coding"),
        ("implement a REST api endpoint", "coding"),
        # math_logic
        ("计算这个数学方程", "math_logic"),
        ("solve the equation", "math_logic"),
        ("统计概率分布", "math_logic"),
        # simple_qa
        ("你好", "simple_qa"),
        ("hello world", "simple_qa"),
        ("今天天气怎么样", "simple_qa"),
    ]

    for text, expected in cases:
        result = SmartRouter._rule_classify(text)
        check(f"'{text[:20]}...' → {expected}", result == expected)

    # No match
    result = SmartRouter._rule_classify("随便聊聊日常对话")
    check("unmatched text → None", result is None)

    # Empty
    check("empty text → None", SmartRouter._rule_classify("") is None)
    check("whitespace → None", SmartRouter._rule_classify("   ") is None)


def test_route_auto():
    print("\n[2] Auto Routing")
    reset_db()
    seed_test_models()
    router = SmartRouter()

    # auto → rule-based routing
    result = router.route("auto", [{"role": "user", "content": "帮我写代码实现排序算法"}])
    check("auto + coding prompt → deepseek-v3", result["model"] == "deepseek-v3")
    check("task_type = coding", result["task_type"] == "coding")
    check("method = rule", result["method"] == "rule")
    check("stream = True (inference model)", result["stream"] is True)

    # auto → simple_qa (rule)
    result = router.route("auto", [{"role": "user", "content": "你好"}])
    check("auto + '你好' → qwen2.5-local", result["model"] == "qwen2.5-local")
    check("task_type = simple_qa", result["task_type"] == "simple_qa")
    check("stream = False (non-inference)", result["stream"] is False)

    # auto → complex_reasoning (rule)
    result = router.route("auto", [{"role": "user", "content": "请分析这个架构方案"}])
    check("auto + analysis → qwen3.6-plus", result["model"] == "qwen3.6-plus")
    check("stream = True (qwen3.6-plus is inference)", result["stream"] is True)

    # auto-route variant
    result = router.route("auto-route", [{"role": "user", "content": "统计这组数据的概率"}])
    check("auto-route works same as auto", result["task_type"] == "math_logic")


def test_route_direct():
    print("\n[3] Direct Routing (non-auto)")
    reset_db()
    seed_test_models()
    router = SmartRouter()

    result = router.route("qwen-plus", [{"role": "user", "content": "hello"}])
    check("direct model passthrough", result["model"] == "qwen-plus")
    check("task_type = direct", result["task_type"] == "direct")
    check("method = direct", result["method"] == "direct")
    check("stream = False (qwen-plus not inference)", result["stream"] is False)

    # direct to inference model
    result = router.route("deepseek-v3", [{"role": "user", "content": "test"}])
    check("direct to deepseek-v3", result["model"] == "deepseek-v3")
    check("stream auto-enabled for inference", result["stream"] is True)


def test_domain_enhancement():
    print("\n[4] Semantic Router Domain Enhancement")
    reset_db()
    seed_test_models()
    router = SmartRouter()

    # Domain: math → override simple_qa to math_logic
    result = router.route(
        "auto",
        [{"role": "user", "content": "你好"}],
        sr_domain="math",
    )
    check("math domain overrides simple_qa → math_logic", result["task_type"] == "math_logic")
    check("method includes +sr:math", "+sr:math" in result["method"])
    check("model = deepseek-v3 for math", result["model"] == "deepseek-v3")

    # Domain: computer_science → override to coding
    result = router.route(
        "auto",
        [{"role": "user", "content": "你好"}],
        sr_domain="computer_science",
    )
    check("cs domain overrides → coding", result["task_type"] == "coding")
    check("model = deepseek-v3 for coding", result["model"] == "deepseek-v3")

    # Domain: biology but task already complex_reasoning → no override
    result = router.route(
        "auto",
        [{"role": "user", "content": "请分析这个架构方案"}],
        sr_domain="biology",
    )
    check("biology doesn't override complex_reasoning", result["task_type"] == "complex_reasoning")

    # No domain → no change
    result = router.route(
        "auto",
        [{"role": "user", "content": "帮我写代码"}],
        sr_domain="",
    )
    check("empty domain → no enhancement", "+sr:" not in result["method"])


def test_fallback_chain():
    print("\n[5] Fallback Chain Construction")
    reset_db()
    seed_test_models()
    router = SmartRouter()
    fallbacks = router.build_fallbacks()

    check("fallback chain has 5 entries", len(fallbacks) == 5)

    # Verify specific chains
    chains = {}
    for entry in fallbacks:
        for src, targets in entry.items():
            chains[src] = targets

    check("qwen3-max → qwen3.6-plus", chains.get("qwen3-max") == ["qwen3.6-plus"])
    check("qwen3.6-plus → qwen-plus", chains.get("qwen3.6-plus") == ["qwen-plus"])
    check("deepseek-v3 → qwen3.6-plus", chains.get("deepseek-v3") == ["qwen3.6-plus"])
    check("qwen-plus → qwen3.6-flash", chains.get("qwen-plus") == ["qwen3.6-flash"])
    check("qwen3.6-flash → qwen2.5-local", chains.get("qwen3.6-flash") == ["qwen2.5-local"])


def test_fallback_with_missing_models():
    print("\n[6] Fallback Chain with Missing Models")
    reset_db()
    mgr = ModelManager()
    # Only register 3 models
    mgr.register_model(name="qwen-plus", provider="dashscope", litellm_model="openai/qwen-plus")
    mgr.register_model(name="qwen2.5-local", provider="ollama", litellm_model="ollama/qwen2.5:latest")
    mgr.register_model(name="deepseek-v3", provider="dashscope", litellm_model="openai/deepseek-v3")

    router = SmartRouter()
    fallbacks = router.build_fallbacks()

    chains = {}
    for entry in fallbacks:
        for src, targets in entry.items():
            chains[src] = targets

    check("deepseek-v3 fallback excluded (qwen3.6-plus inactive)", "deepseek-v3" not in chains)
    check("qwen-plus fallback excluded (qwen3.6-flash inactive)", "qwen-plus" not in chains)
    # qwen3.6-flash and qwen3.6-plus not registered, so no chains for them
    check("no chain for qwen3-max (not registered)", "qwen3-max" not in chains)


def test_stats():
    print("\n[7] Routing Statistics")
    reset_db()
    seed_test_models()
    router = SmartRouter()

    router.route("auto", [{"role": "user", "content": "帮我写代码"}])  # rule:coding
    router.route("auto", [{"role": "user", "content": "你好"}])  # rule:simple_qa
    router.route("auto", [{"role": "user", "content": "请分析方案"}])  # rule:complex_reasoning
    router.route("qwen-plus", [{"role": "user", "content": "test"}])  # route:qwen-plus

    stats = router.get_stats()
    check("stats has rule:coding", stats.get("rule:coding") == 1)
    check("stats has rule:simple_qa", stats.get("rule:simple_qa") == 1)
    check("stats has rule:complex_reasoning", stats.get("rule:complex_reasoning") == 1)
    check("stats has route:qwen-plus", stats.get("route:qwen-plus") == 1)

    router.reset_stats()
    check("reset clears stats", len(router.get_stats()) == 0)


def test_metadata():
    print("\n[8] Routing Metadata")
    reset_db()
    seed_test_models()
    router = SmartRouter()

    result = router.route("auto", [{"role": "user", "content": "帮我写python函数"}])
    meta = result["metadata"]["task_router"]
    check("metadata has task_type", meta["task_type"] == "coding")
    check("metadata has method", meta["method"] == "rule")
    check("metadata original_model = auto", meta["original_model"] == "auto")
    check("metadata routed_model = deepseek-v3", meta["routed_model"] == "deepseek-v3")


def test_extract_user_text():
    print("\n[9] User Text Extraction")
    reset_db()
    seed_test_models()

    messages = [
        {"role": "system", "content": "You are helpful"},
        {"role": "user", "content": "first message"},
        {"role": "assistant", "content": "response"},
        {"role": "user", "content": "final question"},
    ]
    text = SmartRouter._extract_user_text(messages)
    check("extracts last user message", text == "final question")

    # Multi-part content
    messages = [
        {"role": "user", "content": [{"type": "text", "text": "part1"}, {"type": "text", "text": "part2"}]},
    ]
    text = SmartRouter._extract_user_text(messages)
    check("extracts from multi-part content", "part1" in text and "part2" in text)

    # No user message
    text = SmartRouter._extract_user_text([{"role": "system", "content": "system"}])
    check("no user message → empty string", text == "")


def test_classifier_params():
    print("\n[10] Classifier Parameter Selection")
    reset_db()
    seed_test_models()
    router = SmartRouter()

    # Without DASHSCOPE_API_KEY → Ollama
    os.environ.pop("DASHSCOPE_API_KEY", None)
    params = router._get_classifier_params()
    check("no API key → Ollama model", "ollama" in params["model"])
    check("Ollama api_base = localhost:11434", "11434" in params.get("api_base", ""))

    # With DASHSCOPE_API_KEY → cloud
    os.environ["DASHSCOPE_API_KEY"] = "test-key"
    params = router._get_classifier_params()
    check("with API key → cloud model", "qwen3.6-flash" in params["model"])
    check("cloud api_key set", params.get("api_key") == "test-key")
    os.environ.pop("DASHSCOPE_API_KEY", None)


def main():
    print("=" * 60)
    print("LLooM v2 — Phase 2 Unit Tests")
    print("=" * 60)

    test_rule_classify()
    test_route_auto()
    test_route_direct()
    test_domain_enhancement()
    test_fallback_chain()
    test_fallback_with_missing_models()
    test_stats()
    test_metadata()
    test_extract_user_text()
    test_classifier_params()

    print("\n" + "=" * 60)
    print(f"Results: {PASS} passed, {FAIL} failed")
    print("=" * 60)

    shutil.rmtree(_test_dir, ignore_errors=True)
    return 1 if FAIL > 0 else 0


if __name__ == "__main__":
    sys.exit(main())
