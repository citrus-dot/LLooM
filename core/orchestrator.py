"""TaskOrchestrator — complex task decomposition, execution, and aggregation.

Migrated from v1 task_orchestrator.py with key changes:
- litellm.completion() replaces urllib HTTP calls to Worker
- SmartRouter replaces direct model selection
- SemanticCache (ChromaDB) replaces Qdrant
- SSE events as a generator
"""

import json
import re
import time
from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Generator

import litellm

from core.cache import SemanticCache, get_cache
from core.model_manager import ModelManager
from core.smart_router import SmartRouter, INFERENCE_MODELS, TASK_MODEL_MAP


class SubTaskStatus(Enum):
    PENDING = "pending"
    RUNNING = "running"
    DONE = "done"
    FAILED = "failed"


@dataclass
class SubTask:
    id: int
    description: str
    task_type: str = "general"
    depends_on: list = field(default_factory=list)
    estimated_output_tokens: int = 200
    selected_model: str = ""
    status: str = "pending"
    result: str = ""
    cost: float = 0.0
    tokens_used: int = 0
    duration: float = 0.0


@dataclass
class OrchestrationResult:
    final_response: str = ""
    decomposed: bool = False
    sub_tasks: list = field(default_factory=list)
    total_cost: float = 0.0
    total_tokens: int = 0
    total_duration: float = 0.0
    original_query: str = ""


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


class TaskOrchestrator:
    """Decompose complex tasks, execute subtasks, aggregate results."""

    def __init__(self, mgr: ModelManager | None = None, router: SmartRouter | None = None):
        self.mgr = mgr or ModelManager()
        self.router = router or SmartRouter(self.mgr)
        self.cache = get_cache()
        self.available_models: set[str] = set()

    # ── Complexity detection ──

    def is_complex(self, query: str) -> bool:
        """Check if a query needs decomposition."""
        for pattern in COMPLEXITY_INDICATORS:
            if re.search(pattern, query, re.IGNORECASE):
                return True
        if len(query) > 100:
            return True
        sentences = re.split(r'[。！？.!?]', query)
        sentences = [s.strip() for s in sentences if s.strip()]
        if len(sentences) > 2:
            return True
        return False

    # ── Model selection ──

    def _select_model(self, task_type: str) -> str:
        """Select cost-optimal model for a task type."""
        preferences = TASK_MODEL_PREFERENCE.get(task_type, ["qwen-plus"])
        for model in preferences:
            if not self.available_models or model in self.available_models:
                return model
        return "qwen2.5-local"

    def plan_costs(self, sub_tasks: list[SubTask]) -> list[SubTask]:
        """Assign models and estimate costs for subtasks."""
        for task in sub_tasks:
            task.selected_model = self._select_model(task.task_type)
            model = self.mgr.get_model(task.selected_model)
            if model:
                task.cost = (
                    task.estimated_output_tokens * model["input_cost_per_token"]
                    + task.estimated_output_tokens * model["output_cost_per_token"]
                )
        return sub_tasks

    # ── Decomposition ──

    def decompose(self, query: str, classifier_model: str = "auto") -> list[SubTask]:
        """Use LLM to decompose a complex task into subtasks."""
        try:
            content = self._call_llm(
                model=classifier_model,
                messages=[
                    {"role": "system", "content": DECOMPOSE_SYSTEM_PROMPT},
                    {"role": "user", "content": query},
                ],
                max_tokens=500,
                temperature=0,
                timeout=30,
            )
            json_match = re.search(r'\[.*\]', content, re.DOTALL)
            if not json_match:
                return [SubTask(id=1, description=query, task_type="complex_reasoning",
                                estimated_output_tokens=500)]
            tasks_data = json.loads(json_match.group())
            sub_tasks = []
            for td in tasks_data:
                sub_tasks.append(SubTask(
                    id=td.get("id", len(sub_tasks) + 1),
                    description=td.get("description", ""),
                    task_type=td.get("task_type", "general"),
                    depends_on=td.get("depends_on", []),
                    estimated_output_tokens=td.get("estimated_output_tokens", 1024),
                ))
            return sub_tasks
        except Exception:
            return [SubTask(id=1, description=query, task_type="complex_reasoning",
                            estimated_output_tokens=500)]

    # ── Task execution ──

    def execute_task(self, task: SubTask, context: str = "", history: list | None = None) -> SubTask:
        """Execute a single subtask."""
        task.status = SubTaskStatus.RUNNING.value
        start = time.time()

        user_content = task.description
        if context:
            user_content = f"前置任务结果：\n{context}\n\n当前任务：{task.description}"

        messages = [{"role": "system", "content": "你是一个专业的AI助手。请认真完成以下任务。请结合对话上下文回答。"}]
        if history:
            messages.extend(history[-10:])
        messages.append({"role": "user", "content": user_content})

        try:
            use_cache = (task.task_type in ("simple_qa", "general") and not context)

            cache_hit = None
            if use_cache and self.cache.enabled:
                cache_hit = self.cache.get(task.description, task.selected_model)
            if cache_hit:
                task.result = cache_hit["response"]
                task.tokens_used = 0
                task.duration = time.time() - start
                task.status = SubTaskStatus.DONE.value
                task.cost = 0.0
                return task

            content = self._call_llm(
                model=task.selected_model,
                messages=messages,
                max_tokens=task.estimated_output_tokens,
                timeout=120,
                use_cache=use_cache,
            )

            task.result = content
            task.duration = time.time() - start
            task.status = SubTaskStatus.DONE.value

            if use_cache and self.cache.enabled:
                self.cache.put(task.description, content, task.selected_model)

        except Exception as e:
            task.status = SubTaskStatus.FAILED.value
            task.result = f"执行失败: {e}"
            task.duration = time.time() - start

        return task

    # ── Aggregation ──

    def aggregate(self, query: str, sub_tasks: list[SubTask], history: list | None = None) -> str:
        """Aggregate subtask results into a final response."""
        if len(sub_tasks) == 1:
            return sub_tasks[0].result

        summary_parts = []
        for task in sub_tasks:
            summary_parts.append(f"## 子任务 {task.id}: {task.description}\n\n{task.result}")

        user_content = f"""原始任务：{query}

子任务执行结果：

{chr(10).join(summary_parts)}

请汇总以上结果，生成最终回答。"""

        messages = [{"role": "system", "content": AGGREGATE_SYSTEM_PROMPT}]
        if history:
            messages.extend(history[-6:])
        messages.append({"role": "user", "content": user_content})

        try:
            return self._call_llm(
                model="qwen-plus",
                messages=messages,
                max_tokens=4096,
                temperature=0.3,
                timeout=120,
            )
        except Exception:
            return "\n\n---\n\n".join(
                f"**子任务 {t.id}**: {t.result}" for t in sub_tasks
            )

    # ── Main entry (non-streaming) ──

    def orchestrate(self, query: str, history: list | None = None) -> OrchestrationResult:
        """Full orchestration: detect → decompose → plan → execute → aggregate."""
        start = time.time()
        result = OrchestrationResult(original_query=query)

        if not self.is_complex(query):
            task = SubTask(id=1, description=query, task_type="general",
                           estimated_output_tokens=500, selected_model="auto")
            self.execute_task(task, history=history)
            result.final_response = task.result
            result.total_cost = task.cost
            result.total_tokens = task.tokens_used
            result.total_duration = time.time() - start
            return result

        result.decomposed = True
        sub_tasks = self.decompose(query)
        sub_tasks = self.plan_costs(sub_tasks)

        completed: dict[int, SubTask] = {}
        for task in sub_tasks:
            context_parts = []
            for dep_id in task.depends_on:
                if dep_id in completed:
                    dep = completed[dep_id]
                    context_parts.append(f"[子任务{dep_id}] {dep.description}\n结果: {dep.result}")
            context = "\n\n".join(context_parts) if context_parts else ""
            self.execute_task(task, context, history=history)
            completed[task.id] = task

        result.sub_tasks = [vars(t) for t in sub_tasks]
        result.final_response = self.aggregate(query, sub_tasks, history=history)
        result.total_cost = sum(t.cost for t in sub_tasks)
        result.total_tokens = sum(t.tokens_used for t in sub_tasks)
        result.total_duration = time.time() - start
        return result

    # ── SSE streaming orchestration ──

    def orchestrate_stream(
        self,
        query: str,
        history: list | None = None,
        sr_domain: str = "",
    ) -> Generator[str, None, None]:
        """Stream orchestration as SSE events. Yields 'event: type\\ndata: json\\n\\n' strings."""
        def sse(event: str, data: dict) -> str:
            return f"event: {event}\ndata: {json.dumps(data, ensure_ascii=False)}\n\n"

        if not self.is_complex(query):
            task = SubTask(id=1, description=query, task_type="general",
                           estimated_output_tokens=500, selected_model="auto")
            yield sse("decompose", {"sub_tasks": [{"id": 1, "description": query,
                       "selected_model": "auto", "cost": 0.0001}], "total_cost": 0.0001})
            yield sse("task_start", {"id": 1, "description": query, "model": "auto"})

            self.execute_task(task, history=history)
            yield sse("task_done", {"id": 1, "model": task.selected_model,
                       "duration": task.duration, "cost": task.cost, "tokens": task.tokens_used})
            yield sse("result", {"response": task.result, "total_cost": task.cost,
                       "total_tokens": task.tokens_used, "total_duration": task.duration,
                       "sr_info": f"SR域分类: {sr_domain}" if sr_domain else ""})
            return

        sub_tasks = self.decompose(query)
        sub_tasks = self.plan_costs(sub_tasks)

        yield sse("decompose", {"sub_tasks": [{"id": t.id, "description": t.description,
                   "selected_model": t.selected_model, "cost": t.cost,
                   "task_type": t.task_type} for t in sub_tasks],
                   "total_cost": sum(t.cost for t in sub_tasks)})

        completed: dict[int, SubTask] = {}
        for task in sub_tasks:
            context_parts = []
            for dep_id in task.depends_on:
                if dep_id in completed:
                    dep = completed[dep_id]
                    context_parts.append(f"[子任务{dep_id}] {dep.description}\n结果: {dep.result}")
            context = "\n\n".join(context_parts) if context_parts else ""

            yield sse("task_start", {"id": task.id, "description": task.description,
                       "model": task.selected_model})

            self.execute_task(task, context, history=history)
            completed[task.id] = task

            yield sse("task_done", {"id": task.id, "model": task.selected_model,
                       "duration": task.duration, "cost": task.cost,
                       "tokens": task.tokens_used})

        final = self.aggregate(query, sub_tasks, history=history)
        total_cost = sum(t.cost for t in sub_tasks)
        total_tokens = sum(t.tokens_used for t in sub_tasks)

        yield sse("result", {"response": final, "total_cost": total_cost,
                   "total_tokens": total_tokens, "total_duration": 0,
                   "sr_info": f"SR域分类: {sr_domain}" if sr_domain else ""})

    # ── LLM call (unified, uses litellm SDK) ──

    def _call_llm(
        self,
        model: str,
        messages: list[dict],
        max_tokens: int = 500,
        temperature: float = 0.3,
        timeout: int = 60,
        use_cache: bool = False,
    ) -> str:
        """Call LLM via litellm. Returns response content string."""
        if use_cache and self.cache.enabled:
            cache_hit = self.cache.get(
                messages[-1].get("content", ""), model
            )
            if cache_hit:
                return cache_hit["response"]

        routing = self.router.route(model, messages)
        final_model = routing["model"]
        stream = routing["stream"]

        params = self.mgr.get_litellm_params(final_model) or {"model": final_model}
        params["messages"] = messages
        params["max_tokens"] = max_tokens
        params["temperature"] = temperature
        params["timeout"] = timeout
        if stream:
            params["stream"] = True

        if stream:
            content_parts = []
            for chunk in litellm.completion(**params):
                delta = chunk.choices[0].delta
                if delta and delta.content:
                    content_parts.append(delta.content)
            content = "".join(content_parts)
        else:
            response = litellm.completion(**params)
            content = response.choices[0].message.content

        if use_cache and self.cache.enabled:
            self.cache.put(messages[-1].get("content", ""), content, model)

        return content
