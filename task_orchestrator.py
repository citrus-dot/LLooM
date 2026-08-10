"""
复杂任务编排引擎（Phase 4）

当用户请求包含多个步骤或需要多种能力时，自动：
1. 检测任务复杂度（是否需要分解）
2. 用 LLM 将复杂任务拆解为子任务
3. 为每个子任务估算成本并选择最优模型
4. 按依赖顺序执行子任务
5. 汇总结果生成最终回答

调用方式：
    from task_orchestrator import Orchestrator

    orch = Orchestrator("http://localhost:4001", "sk-1234")
    result = await orch.orchestrate("写一个Python爬虫并测试它")

    # result = {
    #     "final_response": "汇总后的回答",
    #     "decomposed": True/False,
    #     "sub_tasks": [...],
    #     "total_cost": 0.0023,
    #     "total_tokens": 1500
    # }
"""

import json
import os
import re
import time
import urllib.request
import urllib.error
import logging
import sys
from typing import Optional
from dataclasses import dataclass, field, asdict
from enum import Enum

logger = logging.getLogger(__name__)
if not logger.handlers:
    handler = logging.StreamHandler(sys.stdout)
    handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)s [%(name)s] %(message)s"))
    logger.addHandler(handler)
    logger.setLevel(logging.INFO)
    logger.propagate = False


# ==================================================
# 数据结构
# ==================================================

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


# ==================================================
# 模型定价表（与 config_worker.yaml 保持一致）
# ==================================================

MODEL_PRICING = {
    "qwen2.5-local":    {"input": 0.0,           "output": 0.0,           "label": "Ollama 本地（零成本）"},
    "qwen3.6-flash":    {"input": 0.00000167,    "output": 0.00001,       "label": "百炼 Flash（极速）"},
    "qwen-plus":        {"input": 0.00000111,    "output": 0.00000278,    "label": "百炼 Plus（性价比）"},
    "deepseek-v3":      {"input": 0.00000139,    "output": 0.00001111,    "label": "DeepSeek V3（推理）"},
    "qwen3.6-plus":     {"input": 0.00000278,    "output": 0.00001667,    "label": "百炼 Plus+（高质量）"},
    "qwen3-max":        {"input": 0.00000347,    "output": 0.00001389,    "label": "百炼 Max（旗舰）"},
}

# 任务类型 → 模型偏好（按成本升序，首选最便宜的可胜任模型）
TASK_MODEL_PREFERENCE = {
    "simple_qa":         ["qwen2.5-local", "qwen3.6-flash", "qwen-plus"],
    "general":           ["qwen-plus", "qwen3.6-flash", "qwen2.5-local"],
    "coding":            ["deepseek-v3", "qwen-plus", "qwen2.5-local"],
    "math_logic":        ["deepseek-v3", "qwen-plus", "qwen3.6-plus"],
    "complex_reasoning": ["qwen3.6-plus", "deepseek-v3", "qwen-plus"],
}

# 复杂任务检测关键词
COMPLEXITY_INDICATORS = [
    r"(然后|接着|再|之后|最后).{2,}",           # 多步骤：然后...接着...
    r"(第[一二三四五1-5]步|Step\s?\d)",          # 编号步骤
    r"(同时|并且|此外|另外)",                     # 多方面
    r"(对比|比较|分析|评估).+(和|与|跟|vs)",      # 对比分析
    r"(写|实现|开发).+(并|然后|接着).*(测试|验证|部署)",  # 开发+测试
    r"(翻译|总结|摘要).+(并|然后).+(分析|评论)",   # 多步处理
]


# ==================================================
# 编排引擎
# ==================================================

class Orchestrator:
    """复杂任务编排器"""

    def __init__(self, worker_url: str, api_key: str):
        self.worker_url = worker_url.rstrip("/")
        self.api_key = api_key
        self.available_models: list = []

    def _call_llm(self, model: str, messages: list, max_tokens: int = 500,
                  temperature: float = 0.3, timeout: int = 60, use_cache: bool = False) -> dict:
        """调用 LiteLLM Worker API"""
        url = f"{self.worker_url}/v1/chat/completions"
        body_dict = {
            "model": model,
            "messages": messages,
            "max_tokens": max_tokens,
            "temperature": temperature,
        }
        if not use_cache:
            body_dict["cache"] = {"no-cache": True}
        body = json.dumps(body_dict).encode()

        req = urllib.request.Request(url, data=body, method="POST")
        req.add_header("Authorization", f"Bearer {self.api_key}")
        req.add_header("Content-Type", "application/json")

        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return json.loads(resp.read().decode())

    def _call_llm_stream(self, model: str, messages: list, max_tokens: int = 500,
                         temperature: float = 0.3, timeout: int = 60, use_cache: bool = False) -> dict:
        """调用 LLM（推理模型自动流式），返回完整响应"""
        url = f"{self.worker_url}/v1/chat/completions"
        body_dict = {
            "model": model,
            "messages": messages,
            "max_tokens": max_tokens,
            "temperature": temperature,
            "stream": True,
        }
        if not use_cache:
            body_dict["cache"] = {"no-cache": True}
        body = json.dumps(body_dict).encode()

        req = urllib.request.Request(url, data=body, method="POST")
        req.add_header("Authorization", f"Bearer {self.api_key}")
        req.add_header("Content-Type", "application/json")

        content_parts = []
        usage = {}
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            for line in resp:
                line = line.decode().strip()
                if not line or not line.startswith("data:"):
                    continue
                data_str = line[5:].strip()
                if data_str == "[DONE]":
                    break
                try:
                    chunk = json.loads(data_str)
                    delta = chunk.get("choices", [{}])[0].get("delta", {})
                    if delta.get("content"):
                        content_parts.append(delta["content"])
                    if chunk.get("usage"):
                        usage = chunk["usage"]
                except json.JSONDecodeError:
                    continue

        return {
            "content": "".join(content_parts),
            "usage": usage,
            "model": model,
        }

    def _get_available_models(self) -> list:
        """获取 Worker 可用模型列表"""
        url = f"{self.worker_url}/v1/models"
        req = urllib.request.Request(url)
        req.add_header("Authorization", f"Bearer {self.api_key}")
        try:
            with urllib.request.urlopen(req, timeout=5) as resp:
                data = json.loads(resp.read().decode())
                self.available_models = [m["id"] for m in data.get("data", [])]
                return self.available_models
        except Exception:
            return []

    def _select_model(self, task_type: str) -> str:
        """为子任务选择成本最优模型"""
        preferences = TASK_MODEL_PREFERENCE.get(task_type, ["qwen-plus"])
        for model in preferences:
            if not self.available_models or model in self.available_models:
                return model
        return "qwen2.5-local"  # 最终兜底

    def _estimate_cost(self, model: str, input_tokens: int, output_tokens: int) -> float:
        """估算单次调用成本"""
        pricing = MODEL_PRICING.get(model, {"input": 0, "output": 0})
        return pricing["input"] * input_tokens + pricing["output"] * output_tokens

    # ==================================================
    # 步骤 1: 复杂度检测
    # ==================================================

    def is_complex(self, query: str) -> bool:
        """检测任务是否需要分解"""
        # 规则检测：多步骤关键词
        for pattern in COMPLEXITY_INDICATORS:
            if re.search(pattern, query, re.IGNORECASE):
                return True

        # 长度检测：超过 100 字的请求大概率复杂
        if len(query) > 100:
            return True

        # 句子数量检测：超过 2 句大概率多任务
        sentences = re.split(r'[。！？.!?]', query)
        sentences = [s.strip() for s in sentences if s.strip()]
        if len(sentences) > 2:
            return True

        return False

    # ==================================================
    # 步骤 2: 任务分解
    # ==================================================

    def decompose(self, query: str, classifier_model: str = "auto") -> list[SubTask]:
        """用 LLM 将复杂任务分解为子任务"""
        system_prompt = """你是一个任务分解专家。将用户的复杂任务分解为2-5个子任务。

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

        try:
            # 使用 auto 模型（让 task_router 自动选择分类器）
            # auto 可能路由到推理模型（qwen3.6-flash），需用流式避免 HTTP 超时
            result = self._call_llm_stream(
                model=classifier_model,
                messages=[
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": query},
                ],
                max_tokens=500,
                temperature=0,
                timeout=30,
            )

            content = result["content"].strip()

            # 提取 JSON 数组
            json_match = re.search(r'\[.*\]', content, re.DOTALL)
            if not json_match:
                logger.warning("分解结果未找到JSON数组，返回单任务")
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

            logger.info("任务分解完成: %d 个子任务", len(sub_tasks))
            return sub_tasks

        except Exception as e:
            logger.warning("任务分解失败: %s，返回单任务", e)
            return [SubTask(id=1, description=query, task_type="complex_reasoning",
                            estimated_output_tokens=500)]

    # ==================================================
    # 步骤 3: 成本规划
    # ==================================================

    def plan_costs(self, sub_tasks: list[SubTask]) -> list[SubTask]:
        """为每个子任务选择最优模型并估算成本"""
        self._get_available_models()

        for task in sub_tasks:
            task.selected_model = self._select_model(task.task_type)
            # 估算输入 token（描述 + 上下文）
            est_input = len(task.description) // 2 + 50
            task.cost = self._estimate_cost(
                task.selected_model, est_input, task.estimated_output_tokens
            )

        total = sum(t.cost for t in sub_tasks)
        logger.info("成本规划完成: 总预算 $%.6f", total)
        for t in sub_tasks:
            logger.info("  子任务 %d: %s → %s ($%.6f)",
                        t.id, t.description[:30], t.selected_model, t.cost)

        return sub_tasks

    # ==================================================
    # 步骤 4: 执行子任务
    # ==================================================

    def execute_task(self, task: SubTask, context: str = "", history: list = None) -> SubTask:
        """执行单个子任务"""
        task.status = SubTaskStatus.RUNNING.value
        start = time.time()

        # 构建消息（包含依赖任务的上下文）
        user_content = task.description
        if context:
            user_content = f"前置任务结果：\n{context}\n\n当前任务：{task.description}"

        messages = [
            {"role": "system", "content": "你是一个专业的AI助手。请认真完成以下任务。请结合对话上下文回答。"},
        ]
        # 注入对话历史（最多保留最近 10 轮）
        if history:
            messages.extend(history[-10:])
        messages.append({"role": "user", "content": user_content})

        try:
            # 简单任务启用缓存，复杂子任务不启用
            use_cache = (task.task_type in ("simple_qa", "general") and not context)
            # 推理模型使用流式
            if task.selected_model in {"qwen3.6-flash", "qwen3.6-plus", "qwen3-max", "deepseek-v3"}:
                result = self._call_llm_stream(
                    task.selected_model, messages,
                    max_tokens=task.estimated_output_tokens,
                    timeout=120,
                    use_cache=use_cache,
                )
            else:
                response = self._call_llm(
                    task.selected_model, messages,
                    max_tokens=task.estimated_output_tokens,
                    timeout=120,
                    use_cache=use_cache,
                )
                result = {
                    "content": response["choices"][0]["message"]["content"],
                    "usage": response.get("usage", {}),
                    "model": task.selected_model,
                }

            task.result = result["content"]
            usage = result.get("usage", {})
            task.tokens_used = usage.get("total_tokens", 0)
            task.duration = time.time() - start
            task.status = SubTaskStatus.DONE.value

            # 计算实际成本
            if task.tokens_used > 0:
                prompt_tokens = usage.get("prompt_tokens", 0)
                completion_tokens = usage.get("completion_tokens", 0)
                task.cost = self._estimate_cost(
                    task.selected_model, prompt_tokens, completion_tokens
                )

            logger.info("子任务 %d 完成: %s (%.1fs, $%.6f, %d tokens)",
                        task.id, task.selected_model, task.duration, task.cost, task.tokens_used)

        except Exception as e:
            task.status = SubTaskStatus.FAILED.value
            task.result = f"执行失败: {e}"
            task.duration = time.time() - start
            logger.error("子任务 %d 失败: %s", task.id, e)

        return task

    def execute_all(self, sub_tasks: list[SubTask], history: list = None) -> list[SubTask]:
        """按依赖顺序执行所有子任务"""
        completed = {}

        for task in sub_tasks:
            # 收集依赖任务的上下文
            context_parts = []
            for dep_id in task.depends_on:
                if dep_id in completed:
                    dep = completed[dep_id]
                    context_parts.append(f"[子任务{dep_id}] {dep.description}\n结果: {dep.result}")

            context = "\n\n".join(context_parts) if context_parts else ""
            self.execute_task(task, context, history=history)
            completed[task.id] = task

        return sub_tasks

    # ==================================================
    # 步骤 5: 结果汇总
    # ==================================================

    def aggregate(self, query: str, sub_tasks: list[SubTask], history: list = None) -> str:
        """汇总子任务结果生成最终回答"""
        # 如果只有一个子任务，直接返回结果
        if len(sub_tasks) == 1:
            return sub_tasks[0].result

        # 多个子任务：用 LLM 汇总
        summary_parts = []
        for task in sub_tasks:
            summary_parts.append(f"## 子任务 {task.id}: {task.description}\n\n{task.result}")

        system_prompt = """你是一个结果汇总专家。用户提出了一个复杂任务，已被分解为多个子任务并分别执行。
请将所有子任务的结果汇总成一个连贯、完整的最终回答。

要求：
1. 保持逻辑连贯，按子任务顺序组织
2. 去除冗余信息
3. 突出关键结论
4. 使用中文回答
5. 结合对话上下文，确保回答与之前的对话连贯"""

        user_content = f"""原始任务：{query}

子任务执行结果：

{chr(10).join(summary_parts)}

请汇总以上结果，生成最终回答。"""

        messages = [
            {"role": "system", "content": system_prompt},
        ]
        if history:
            messages.extend(history[-6:])
        messages.append({"role": "user", "content": user_content})

        try:
            response = self._call_llm(
                model="qwen-plus",
                messages=messages,
                max_tokens=4096,
                temperature=0.3,
                timeout=120,
            )
            return response["choices"][0]["message"]["content"]
        except Exception as e:
            logger.warning("汇总失败: %s，拼接返回", e)
            return "\n\n---\n\n".join(
                f"**子任务 {t.id}**: {t.result}" for t in sub_tasks
            )

    # ==================================================
    # 主流程
    # ==================================================

    def orchestrate(self, query: str, history: list = None) -> OrchestrationResult:
        """完整编排流程：检测 → 分解 → 规划 → 执行 → 汇总"""
        start_time = time.time()
        result = OrchestrationResult(original_query=query)

        # 步骤 1: 复杂度检测
        if not self.is_complex(query):
            logger.info("任务不复杂，直接执行")
            task = SubTask(id=1, description=query, task_type="general",
                           estimated_output_tokens=500)
            self.plan_costs([task])
            self.execute_task(task, history=history)
            result.final_response = task.result
            result.sub_tasks = [asdict(task)]
            result.total_cost = task.cost
            result.total_tokens = task.tokens_used
            result.total_duration = time.time() - start_time
            return result

        # 步骤 2: 任务分解
        logger.info("检测到复杂任务，开始分解...")
        sub_tasks = self.decompose(query)
        result.decomposed = True

        # 步骤 3: 成本规划
        sub_tasks = self.plan_costs(sub_tasks)

        # 步骤 4: 执行
        sub_tasks = self.execute_all(sub_tasks, history=history)

        # 步骤 5: 汇总
        result.final_response = self.aggregate(query, sub_tasks, history=history)
        result.sub_tasks = [asdict(t) for t in sub_tasks]
        result.total_cost = sum(t.cost for t in sub_tasks)
        result.total_tokens = sum(t.tokens_used for t in sub_tasks)
        result.total_duration = time.time() - start_time

        logger.info("编排完成: %d 子任务, $%.6f, %d tokens, %.1fs",
                     len(sub_tasks), result.total_cost, result.total_tokens, result.total_duration)

        return result
