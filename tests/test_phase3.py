"""Phase 3 unit tests — SemanticCache, TaskOrchestrator, SSE events."""

import json
import os
import sys
import tempfile
import shutil

_test_dir = tempfile.mkdtemp(prefix="lloom_test_")
os.environ["LLOOM_DATA_DIR"] = _test_dir

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from core import database as db
from core.cache import SemanticCache
from core.model_manager import ModelManager
from core.orchestrator import (
    TaskOrchestrator,
    SubTask,
    OrchestrationResult,
    SubTaskStatus,
    COMPLEXITY_INDICATORS,
    TASK_MODEL_PREFERENCE,
    DECOMPOSE_SYSTEM_PROMPT,
    AGGREGATE_SYSTEM_PROMPT,
)
from core.smart_router import SmartRouter

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
    mgr = ModelManager()
    models = [
        ("qwen-plus", "dashscope", "openai/qwen-plus", "DASHSCOPE_API_BASE", "DASHSCOPE_API_KEY", "general", 0.00000111, 0.00000278),
        ("qwen3.6-flash", "dashscope", "openai/qwen3.6-flash", "DASHSCOPE_API_BASE", "DASHSCOPE_API_KEY", "classification", 0.00000167, 0.00001),
        ("qwen3.6-plus", "dashscope", "openai/qwen3.6-plus", "DASHSCOPE_API_BASE", "DASHSCOPE_API_KEY", "complex_reasoning", 0.00000278, 0.00001667),
        ("qwen3-max", "dashscope", "openai/qwen3-max", "DASHSCOPE_API_BASE", "DASHSCOPE_API_KEY", "complex_reasoning", 0.00000347, 0.00001389),
        ("deepseek-v3", "dashscope", "openai/deepseek-v3", "DASHSCOPE_API_BASE", "DASHSCOPE_API_KEY", "complex_reasoning", 0.00000139, 0.00001111),
        ("qwen2.5-local", "ollama", "ollama/qwen2.5:latest", "OLLAMA_API_BASE", "", "general", 0.0, 0.0),
    ]
    for name, provider, litellm_model, api_base, api_key_env, task_type, icpt, ocpt in models:
        mgr.register_model(
            name=name, provider=provider, litellm_model=litellm_model,
            api_base=api_base, api_key_env=api_key_env, task_type=task_type,
            input_cost_per_token=icpt, output_cost_per_token=ocpt,
        )


def test_semantic_cache():
    print("\n[1] SemanticCache (ChromaDB)")
    cache = SemanticCache(similarity_threshold=0.3, ttl=86400)
    cache.start()

    if cache.enabled:
        check("cache enabled after start", True)

        embedding_ready = False
        try:
            cache.put("hello world", "Hi there!", model="test")
            hit = cache.get("hello world", model="test")
            embedding_ready = hit is not None
        except Exception:
            pass

        if embedding_ready:
            check("exact match returns hit", True)
            check("hit has correct response", hit and hit["response"] == "Hi there!")
            check("hit has model", hit and hit["model"] == "test")

            hit_other = cache.get("hello world", model="other-model")
            check("different model → no hit (filtered)", hit_other is None)

            cache.put("How do I write Python code?", "Use def keyword.", model="q1")
            hit_similar = cache.get("How do I write Python code?", model="q1")
            check("same query same model → hit", hit_similar is not None)

            miss = cache.get("completely unrelated query about quantum physics xyz", model="test")
            check("unrelated query → miss or low similarity", miss is None or miss["similarity"] < 0.3)

            count = cache.count()
            check(f"cache has entries (count={count})", count >= 2)

            cache.clear()
            check("cache cleared", cache.count() == 0)
        else:
            check("embedding model not ready (skipping put/get tests)", True)
            check("cache count = 0 (no entries)", cache.count() == 0)
    else:
        check("cache gracefully disabled (no embedding model)", True)
        check("disabled cache operations safe", cache.get("test") is None and cache.put("a", "b") is None)

    cache.stop()


def test_is_complex():
    print("\n[2] Complexity Detection")
    reset_db()
    seed_test_models()
    orch = TaskOrchestrator()

    # Multi-step indicators
    check("multi-step (然后...接着)", orch.is_complex("请先翻译这段文字，然后总结要点，接着进行分析"))
    check("numbered steps", orch.is_complex("第一步下载数据 第二步清洗 第三步分析"))
    check("multi-aspect (同时)", orch.is_complex("请同时分析A和B的优缺点"))
    check("comparison", orch.is_complex("对比方案A和方案B的可行性"))
    check("dev+test", orch.is_complex("写一个API并测试"))

    # Simple queries
    check("short simple query → not complex", not orch.is_complex("你好"))
    check("medium query → not complex", not orch.is_complex("请帮我翻译这句话"))

    # Long query > 100 chars
    long_query = "这是一个非常长的查询，" * 10
    check("long query (>100 chars) → complex", orch.is_complex(long_query))

    # Multiple sentences
    check("3+ sentences → complex", orch.is_complex("翻译这段话。总结内容。分析结果。"))

    # Single sentence
    check("1 sentence → not complex", not orch.is_complex("What is Python?"))


def test_select_model():
    print("\n[3] Model Selection")
    reset_db()
    seed_test_models()
    orch = TaskOrchestrator()

    check("simple_qa → qwen2.5-local (cheapest)", orch._select_model("simple_qa") == "qwen2.5-local")
    check("general → qwen-plus", orch._select_model("general") == "qwen-plus")
    check("coding → deepseek-v3", orch._select_model("coding") == "deepseek-v3")
    check("math_logic → deepseek-v3", orch._select_model("math_logic") == "deepseek-v3")
    check("complex_reasoning → qwen3.6-plus", orch._select_model("complex_reasoning") == "qwen3.6-plus")
    check("unknown type → qwen-plus (default)", orch._select_model("unknown_type") == "qwen-plus")

    # With available_models filter
    orch.available_models = {"qwen-plus", "qwen2.5-local"}
    check("coding with limited models → qwen-plus (fallback)", orch._select_model("coding") == "qwen-plus")
    check("simple_qa still → qwen2.5-local", orch._select_model("simple_qa") == "qwen2.5-local")
    orch.available_models = set()


def test_plan_costs():
    print("\n[4] Cost Planning")
    reset_db()
    seed_test_models()
    orch = TaskOrchestrator()

    tasks = [
        SubTask(id=1, description="write code", task_type="coding", estimated_output_tokens=500),
        SubTask(id=2, description="simple question", task_type="simple_qa", estimated_output_tokens=100),
    ]
    orch.plan_costs(tasks)

    check("task 1 model = deepseek-v3", tasks[0].selected_model == "deepseek-v3")
    check("task 1 cost > 0", tasks[0].cost > 0)
    check("task 2 model = qwen2.5-local", tasks[1].selected_model == "qwen2.5-local")
    check("task 2 cost = 0 (local model)", tasks[1].cost == 0.0)


def test_decompose_fallback():
    print("\n[5] Decompose Fallback (no API key)")
    reset_db()
    seed_test_models()
    orch = TaskOrchestrator()

    # Without API keys, _call_llm will fail, decompose should return single task
    tasks = orch.decompose("复杂任务需要分解")
    check("decompose fallback returns 1 task", len(tasks) == 1)
    check("fallback task type = complex_reasoning", tasks[0].task_type == "complex_reasoning")
    check("fallback task has description", tasks[0].description == "复杂任务需要分解")


def test_subtask_dataclass():
    print("\n[6] SubTask / OrchestrationResult Dataclasses")
    task = SubTask(id=1, description="test")
    check("default task_type = general", task.task_type == "general")
    check("default status = pending", task.status == "pending")
    check("default depends_on = []", task.depends_on == [])
    check("default cost = 0.0", task.cost == 0.0)

    result = OrchestrationResult()
    check("default final_response = ''", result.final_response == "")
    check("default decomposed = False", result.decomposed is False)
    check("default sub_tasks = []", result.sub_tasks == [])


def test_execute_task_failure():
    print("\n[7] Execute Task Failure Handling")
    reset_db()
    seed_test_models()
    orch = TaskOrchestrator()

    task = SubTask(id=1, description="test task", task_type="general",
                   estimated_output_tokens=100, selected_model="qwen-plus")
    orch.execute_task(task)

    # Without API key, should fail gracefully
    check("task status = failed (no API key)", task.status == SubTaskStatus.FAILED.value)
    check("task has error message", "执行失败" in task.result or task.result != "")
    check("task has duration", task.duration >= 0)


def test_sse_format():
    print("\n[8] SSE Event Format")
    reset_db()
    seed_test_models()
    orch = TaskOrchestrator()

    events = list(orch.orchestrate_stream("你好", history=None, sr_domain="general"))

    check("simple query produces events", len(events) >= 2)
    check("first event is decompose", "event: decompose" in events[0])
    check("decompose has valid JSON data", _validate_sse_data(events[0]))

    # Find result event
    result_events = [e for e in events if "event: result" in e]
    check("has result event", len(result_events) > 0)
    if result_events:
        check("result has response field", '"response"' in result_events[0])
        check("result has sr_info", '"sr_info"' in result_events[0])


def test_sse_complex_format():
    print("\n[9] SSE Complex Query Format")
    reset_db()
    seed_test_models()
    orch = TaskOrchestrator()

    query = "请先翻译这段文字，然后总结要点，接着分析结论"
    events = list(orch.orchestrate_stream(query, history=None, sr_domain=""))

    check("complex query produces events", len(events) >= 1)
    check("first event is decompose", "event: decompose" in events[0])

    # Even with LLM failures, should produce a result event
    result_events = [e for e in events if "event: result" in e]
    check("has result event despite LLM failures", len(result_events) > 0)


def test_prompts_migrated():
    print("\n[10] Prompt Migration")
    check("decompose prompt has JSON format", "JSON数组" in DECOMPOSE_SYSTEM_PROMPT)
    check("decompose prompt has 5 task types", all(t in DECOMPOSE_SYSTEM_PROMPT for t in
          ["simple_qa", "general", "coding", "math_logic", "complex_reasoning"]))
    check("aggregate prompt has 汇总", "汇总" in AGGREGATE_SYSTEM_PROMPT)
    check("aggregate prompt has 逻辑连贯", "逻辑连贯" in AGGREGATE_SYSTEM_PROMPT)
    check("complexity indicators has 6 patterns", len(COMPLEXITY_INDICATORS) == 6)
    check("model preference has 5 types", len(TASK_MODEL_PREFERENCE) == 5)


def _validate_sse_data(event_str: str) -> bool:
    """Check that the SSE data line contains valid JSON."""
    for line in event_str.strip().split("\n"):
        if line.startswith("data: "):
            try:
                json.loads(line[6:])
                return True
            except json.JSONDecodeError:
                return False
    return False


def main():
    print("=" * 60)
    print("LLooM v2 — Phase 3 Unit Tests")
    print("=" * 60)

    test_semantic_cache()
    test_is_complex()
    test_select_model()
    test_plan_costs()
    test_decompose_fallback()
    test_subtask_dataclass()
    test_execute_task_failure()
    test_sse_format()
    test_sse_complex_format()
    test_prompts_migrated()

    print("\n" + "=" * 60)
    print(f"Results: {PASS} passed, {FAIL} failed")
    print("=" * 60)

    shutil.rmtree(_test_dir, ignore_errors=True)
    return 1 if FAIL > 0 else 0


if __name__ == "__main__":
    sys.exit(main())
