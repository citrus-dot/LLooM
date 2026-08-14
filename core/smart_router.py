"""SmartRouter — hybrid task classification, model routing, fallback chains.

Two-layer classification:
  Layer 1: regex rules (zero cost, zero latency)
  Layer 2: LLM fallback (low cost, low latency) — auto-selects cloud vs local

After classification, selects model from TASK_MODEL_MAP, auto-enables
stream for inference models, and builds litellm.Router fallback chains.
"""

import os
import re
from typing import Any

import litellm

from core import database as db
from core.config import get_env
from core.model_manager import ModelManager

# ── Task → Model mapping ──

TASK_MODEL_MAP: dict[str, str] = {
    "simple_qa": "qwen2.5-local",
    "general": "qwen-plus",
    "coding": "deepseek-v3",
    "math_logic": "deepseek-v3",
    "complex_reasoning": "qwen3.6-plus",
}

DEFAULT_MODEL = "qwen2.5-local"

# ── Inference models (auto-enable stream) ──

INFERENCE_MODELS = {"qwen3.6-flash", "qwen3.6-plus", "qwen3-max", "deepseek-v3"}

# ── Auto-route virtual model names ──

AUTO_MODEL_NAMES = {"auto", "auto-route"}

# ── Regex rules (priority: complex_reasoning > coding > math_logic > simple_qa) ──

TASK_RULES: dict[str, list[str]] = {
    "complex_reasoning": [
        r"(分析|analyze|对比|compare|评估|evaluate)",
        r"(方案|plan|策略|strategy|架构|architecture)",
        r"(论文|paper|研究|research|综述|review)",
    ],
    "coding": [
        r"(写代码|write code|implement|函数|function|class|bug|debug)",
        r"(python|java|javascript|go|rust|c\+\+|sql)",
        r"(api|endpoint|refactor|优化|重构)",
    ],
    "math_logic": [
        r"(数学|math|计算|calculate|方程|equation)",
        r"(逻辑|logic|推理|reason|证明|prove)",
        r"(概率|probability|统计|statistics)",
    ],
    "simple_qa": [
        r"^(你好|hi|hello|在吗)",
        r"(天气|时间|日期)",
        r"(翻译|translate)",
    ],
}

# ── Classifier prompt ──

CLASSIFY_SYSTEM_PROMPT = """你是一个任务分类器。将用户请求分类为以下类别之一，只返回类别名称，不要输出其他内容：

- simple_qa: 简单问答、问候、翻译、天气/时间查询
- general: 日常对话、摘要、一般性任务
- coding: 写代码、调试、编程问题、API 设计
- math_logic: 数学计算、逻辑推理、概率统计
- complex_reasoning: 深度分析、方案对比、研究综述、架构设计"""

CLASSIFIER_TIMEOUT = 10
CLASSIFIER_MAX_TOKENS = 20
CLASSIFIER_MAX_TEXT_LEN = 500

VALID_TASK_TYPES = set(TASK_MODEL_MAP.keys())


class SmartRouter:
    """Hybrid task classification + model routing with fallback chains."""

    def __init__(self, mgr: ModelManager | None = None):
        self.mgr = mgr or ModelManager()
        self._router: litellm.Router | None = None
        self._stats: dict[str, int] = {}

    # ── Layer 1: Rule-based classification ──

    @staticmethod
    def _rule_classify(text: str) -> str | None:
        """Regex-based classification. Returns task type or None."""
        text_lower = text.lower()
        if not text_lower.strip():
            return None
        for task_type in ["complex_reasoning", "coding", "math_logic", "simple_qa"]:
            for pattern in TASK_RULES[task_type]:
                if re.search(pattern, text_lower, re.IGNORECASE):
                    return task_type
        return None

    # ── Layer 2: LLM fallback classification ──

    def _get_classifier_params(self) -> dict[str, Any]:
        """Auto-select classifier: cloud if DASHSCOPE_API_KEY set, else Ollama local."""
        dashscope_key = get_env("DASHSCOPE_API_KEY")
        if dashscope_key:
            model = db.get_model("qwen3.6-flash")
            return {
                "model": model["litellm_model"] if model else "openai/qwen3.6-flash",
                "api_base": get_env("DASHSCOPE_API_BASE") or "https://dashscope.aliyuncs.com/compatible-mode/v1",
                "api_key": dashscope_key,
            }
        else:
            model = db.get_model("qwen2.5-local")
            return {
                "model": model["litellm_model"] if model else "ollama/qwen2.5:latest",
                "api_base": get_env("OLLAMA_API_BASE") or "http://localhost:11434",
                "api_key": "ollama",
            }

    def _llm_classify(self, user_text: str) -> str:
        """LLM-based classification. Returns task type, defaults to 'general'."""
        params = self._get_classifier_params()
        truncated = user_text[:CLASSIFIER_MAX_TEXT_LEN]
        try:
            response = litellm.completion(
                model=params["model"],
                api_base=params.get("api_base"),
                api_key=params.get("api_key"),
                messages=[
                    {"role": "system", "content": CLASSIFY_SYSTEM_PROMPT},
                    {"role": "user", "content": truncated},
                ],
                max_tokens=CLASSIFIER_MAX_TOKENS,
                timeout=CLASSIFIER_TIMEOUT,
                temperature=0,
            )
            content = response.choices[0].message.content.strip().lower()
            for t in VALID_TASK_TYPES:
                if t in content:
                    return t
        except Exception:
            pass
        return "general"

    # ── Hybrid classification ──

    def classify(self, messages: list[dict]) -> tuple[str, str, str]:
        """Classify user messages. Returns (task_type, method, selected_model).

        method is 'rule' or 'llm' indicating which layer matched.
        """
        user_text = self._extract_user_text(messages)
        rule_result = self._rule_classify(user_text)
        if rule_result:
            self._inc_stat(f"rule:{rule_result}")
            return rule_result, "rule", TASK_MODEL_MAP[rule_result]

        llm_result = self._llm_classify(user_text)
        self._inc_stat(f"llm:{llm_result}")
        return llm_result, "llm", TASK_MODEL_MAP.get(llm_result, DEFAULT_MODEL)

    @staticmethod
    def _extract_user_text(messages: list[dict]) -> str:
        """Extract the last user message text from a messages list."""
        for msg in reversed(messages):
            if msg.get("role") == "user":
                content = msg.get("content", "")
                if isinstance(content, list):
                    return " ".join(
                        part.get("text", "") for part in content if isinstance(part, dict)
                    )
                return content
        return ""

    # ── Domain enhancement (Semantic Router integration) ──

    @staticmethod
    def _enhance_with_domain(task_type: str, sr_domain: str) -> tuple[str, str]:
        """Enhance task type based on Semantic Router domain classification."""
        if not sr_domain:
            return task_type, ""
        if sr_domain in ("math", "physics", "chemistry", "biology"):
            if task_type not in ("math_logic", "complex_reasoning"):
                return "math_logic", f"+sr:{sr_domain}"
        elif sr_domain in ("computer_science", "engineering"):
            if task_type not in ("coding", "complex_reasoning"):
                return "coding", f"+sr:{sr_domain}"
        return task_type, ""

    # ── Main routing entry point ──

    def route(
        self,
        model: str,
        messages: list[dict],
        sr_domain: str = "",
    ) -> dict[str, Any]:
        """Route a request. If model is 'auto', classify and select model.

        Returns a dict with:
          - model: the selected model name
          - task_type: classified task type
          - method: classification method ('rule' / 'llm' / 'direct')
          - stream: whether to use streaming
          - metadata: routing info
        """
        if model in AUTO_MODEL_NAMES:
            task_type, method, selected = self.classify(messages)

            if sr_domain:
                new_type, suffix = self._enhance_with_domain(task_type, sr_domain)
                if suffix:
                    task_type = new_type
                    method = f"{method}{suffix}"
                    selected = TASK_MODEL_MAP.get(task_type, selected)
        else:
            task_type = "direct"
            method = "direct"
            selected = model

        stream = selected in INFERENCE_MODELS

        self._inc_stat(f"route:{selected}")

        return {
            "model": selected,
            "task_type": task_type,
            "method": method,
            "stream": stream,
            "metadata": {
                "task_router": {
                    "task_type": task_type,
                    "method": method,
                    "original_model": model,
                    "routed_model": selected,
                }
            },
        }

    # ── litellm.Router integration ──

    def build_fallbacks(self) -> list[dict]:
        """Build fallback chain from v1 logic, filtered by active models in DB."""
        active = {m["name"] for m in db.list_models(active_only=True)}
        chain = {
            "qwen3-max": ["qwen3.6-plus"],
            "qwen3.6-plus": ["qwen-plus"],
            "deepseek-v3": ["qwen3.6-plus"],
            "qwen-plus": ["qwen3.6-flash"],
            "qwen3.6-flash": ["qwen2.5-local"],
        }
        fallbacks = []
        for src, targets in chain.items():
            if src in active:
            # D10S fallback targets to only those active (we may have some DB-only models)
                filtered = [t for t in targets if t in active]
                if filtered:
                    fallbacks.append({src: filtered})
        return fallbacks

    def get_router(self) -> litellm.Router:
        """Build a litellm.Router from registered models with fallback chains."""
        if self._router is not None:
            return self._router

        model_list = []
        active = db.list_models(active_only=True)
        for m in active:
            params: dict[str, Any] = {"model": m["litellm_model"]}
            if m["api_base"]:
                api_base = get_env(m["api_base"]) or m["api_base"]
                if api_base.startswith("http"):
                    params["api_base"] = api_base
            if m["api_key_env"]:
                api_key = get_env(m["api_key_env"])
                if api_key:
                    params["api_key"] = api_key

            model_list.append({
                "model_name": m["name"],
                "litellm_params": params,
            })

        router_params: dict[str, Any] = {"model_list": model_list}
        fallbacks = self.build_fallbacks()
        if fallbacks:
            router_params["fallbacks"] = fallbacks

        self._router = litellm.Router(**router_params)
        return self._router

    def completion(
        self,
        model: str,
        messages: list[dict],
        sr_domain: str = "",
        **kwargs,
    ) -> Any:
        """Main entry: route + call litellm.Router.completion with fallback."""
        routing = self.route(model, messages, sr_domain)

        final_model = routing["model"]
        if routing["stream"]:
            kwargs["stream"] = True

        metadata = kwargs.get("metadata", {})
        metadata.update(routing["metadata"])
        kwargs["metadata"] = metadata

        router = self.get_router()
        try:
            return router.completion(model=final_model, messages=messages, **kwargs)
        except Exception:
            pass

        return litellm.completion(
            model=final_model,
            messages=messages,
            **{k: v for k, v in kwargs.items() if k != "metadata"},
        )

    # ── Stats ──

    def _inc_stat(self, key: str) -> None:
        self._stats[key] = self._stats.get(key, 0) + 1

    def get_stats(self) -> dict[str, int]:
        return dict(self._stats)

    def reset_stats(self) -> None:
        self._stats.clear()
