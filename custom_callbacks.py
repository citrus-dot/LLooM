"""
LiteLLM 自定义回调（Phase 2）

本文件包含两个回调处理器，在 config_worker.yaml 中通过以下方式注册：
  callbacks:
    - "prometheus"
    - "custom_callbacks.task_router"       # 智能任务路由
    - "custom_callbacks.quota_tracker"     # 配额追踪

LiteLLM 加载机制：
  1. 导入 custom_callbacks 模块（即本文件）
  2. 查找名为 task_router / quota_tracker 的全局实例
  3. 将实例注册到回调链

任务分级与模型映射（模型名与 config_worker.yaml model_list 一致）：
  ┌──────────────────────┬─────────────────┬──────────────────────────────┐
  │ 任务类型              │ 推荐模型组       │ 说明                          │
  ├──────────────────────┼─────────────────┼──────────────────────────────┤
  │ simple_qa            │ qwen2.5-local   │ Ollama 本地，零 API 费用      │
  │ general              │ qwen-plus       │ 百炼通用性价比                 │
  │ coding               │ deepseek-v3     │ DeepSeek 擅长编程             │
  │ math_logic           │ deepseek-v3     │ DeepSeek 擅长推理             │
  │ complex_reasoning    │ qwen3.6-plus    │ 百炼高质量综合推理             │
  └──────────────────────┴─────────────────┴──────────────────────────────┘

  分类器策略：
    1. DASHSCOPE_API_KEY 已设置 → 使用 qwen3.6-flash（云端，极速）
    2. DASHSCOPE_API_KEY 为空   → 使用 qwen2.5:latest（Ollama 本地兜底）
    3. 两者均失败               → 降级为 general

  注意：模型名必须与 config_worker.yaml 或 Admin 数据库中已注册的模型一致。
"""

import os
import re
import logging
import sys
from typing import Optional, Any

import litellm
from litellm.integrations.custom_logger import CustomLogger
from prometheus_client import Counter, REGISTRY

# 确保 logger 有 handler 输出到 stdout，否则 logger.info 会被静默丢弃
logger = logging.getLogger(__name__)
if not logger.handlers:
    handler = logging.StreamHandler(sys.stdout)
    handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)s [%(name)s] %(message)s"))
    logger.addHandler(handler)
    logger.setLevel(logging.INFO)
    logger.propagate = False


# ==================================================
# 工具函数
# ==================================================


def _get_or_create_counter(name: str, doc: str, labels: list[str]) -> Counter:
    """获取或创建 Prometheus Counter，避免重复注册报错"""
    # 先检查是否已注册（模块可能被 LiteLLM 多次导入）
    # prometheus_client 将 Counter 注册为 name_total 键
    registry_key = name if name.endswith("_total") else name + "_total"
    existing = REGISTRY._names_to_collectors.get(registry_key)
    if existing is not None:
        return existing  # type: ignore
    try:
        return Counter(name, doc, labels)
    except ValueError:
        # 并发场景下的兜底
        return REGISTRY._names_to_collectors[registry_key]  # type: ignore


# ==================================================
# 智能任务路由 — 配置
# ==================================================

# LLM 分类器配置
# 策略：DASHSCOPE_API_KEY 已设置 → 云端 qwen3.6-flash（极速低成本）
#       DASHSCOPE_API_KEY 为空   → Ollama 本地 qwen2.5:latest（零成本兜底）
DASHSCOPE_API_KEY = os.getenv("DASHSCOPE_API_KEY", "")

if DASHSCOPE_API_KEY:
    # 云端分类器（极速、低成本）
    CLASSIFIER_MODEL = "openai/qwen3.6-flash"
    CLASSIFIER_API_BASE = os.getenv(
        "DASHSCOPE_API_BASE", "https://dashscope.aliyuncs.com/compatible-mode/v1"
    )
    CLASSIFIER_API_KEY = DASHSCOPE_API_KEY
else:
    # Ollama 本地分类器（零成本兜底，无需 API Key）
    CLASSIFIER_MODEL = "ollama/qwen2.5:latest"
    CLASSIFIER_API_BASE = os.getenv("OLLAMA_API_BASE", "http://host.docker.internal:11434")
    CLASSIFIER_API_KEY = "ollama"  # Ollama 不需要真实 Key，但 litellm 需要非空值

CLASSIFIER_TIMEOUT = 10  # 秒，Ollama 本地推理稍慢，放宽到 10s
CLASSIFIER_MAX_TOKENS = 20  # 分类只需要一个词
CLASSIFIER_MAX_TEXT_LEN = 500  # 截断用户输入，节省 token

# 分类提示词
CLASSIFY_SYSTEM_PROMPT = """你是一个任务分类器。将用户请求分类为以下类别之一，只返回类别名称，不要输出其他内容：

- simple_qa: 简单问答、问候、翻译、天气/时间查询
- general: 日常对话、摘要、一般性任务
- coding: 写代码、调试、编程问题、API 设计
- math_logic: 数学计算、逻辑推理、概率统计
- complex_reasoning: 深度分析、方案对比、研究综述、架构设计"""

# 任务类型 → 模型组映射
# 模型名必须与 config_worker.yaml model_list 中定义的 model_name 一致
# 优化：simple_qa 路由到 qwen2.5-local（Ollama 本地，零 API 费用）
#       DEFAULT_MODEL 也设为 qwen2.5-local，确保未分类请求零成本
TASK_MODEL_MAP: dict[str, str] = {
    "simple_qa": "qwen2.5-local",         # Ollama 本地，零 API 费用
    "general": "qwen-plus",               # 百炼通用性价比
    "coding": "deepseek-v3",              # DeepSeek 擅长编程
    "math_logic": "deepseek-v3",          # DeepSeek 擅长推理
    "complex_reasoning": "qwen3.6-plus",  # 百炼高质量综合推理
}

DEFAULT_MODEL = "qwen2.5-local"

# 推理模型集合 — 生成 reasoning tokens 耗时较长，自动启用流式响应避免 HTTP 超时
INFERENCE_MODELS = {"qwen3.6-flash", "qwen3.6-plus", "qwen3-max", "deepseek-v3"}

# 触发智能路由的模型名称
AUTO_MODEL_NAMES = {"auto", "auto-route"}

# 正则规则（优先级从高到低）
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


# ==================================================
# Prometheus 指标
# ==================================================

# 任务分类指标
classification_total = _get_or_create_counter(
    "litellm_task_router_classification_total",
    "任务分类统计（按类型、方法、目标模型）",
    ["task_type", "method", "target_model"],
)

# 配额追踪指标
key_spend_total = _get_or_create_counter(
    "litellm_quota_key_spend_total",
    "按 Virtual Key 累计的花费（美元）",
    ["key_alias", "user_id", "model"],
)

user_spend_total = _get_or_create_counter(
    "litellm_quota_user_spend_total",
    "按 User ID 累计的花费（美元）",
    ["user_id", "model"],
)

key_requests_total = _get_or_create_counter(
    "litellm_quota_key_requests_total",
    "按 Virtual Key 累计的请求数",
    ["key_alias", "user_id"],
)

key_spend_by_model = _get_or_create_counter(
    "litellm_quota_key_spend_by_model",
    "按 Virtual Key + Model 累计的花费（美元）",
    ["key_alias", "model"],
)


# ==================================================
# 分类逻辑
# ==================================================


def _extract_user_text(messages: list) -> str:
    """提取所有 user 消息内容，拼接为纯文本"""
    parts: list[str] = []
    for msg in messages:
        if msg.get("role") == "user":
            content = msg.get("content", "")
            if isinstance(content, list):
                # 多模态消息，只取 text 部分
                for part in content:
                    if isinstance(part, dict) and part.get("type") == "text":
                        parts.append(part.get("text", ""))
            elif isinstance(content, str):
                parts.append(content)
    return " ".join(parts)


def _rule_classify(text: str) -> Optional[str]:
    """基于正则规则分类，返回任务类型或 None"""
    text_lower = text.lower()
    if not text_lower.strip():
        return None

    for task_type in ["complex_reasoning", "coding", "math_logic", "simple_qa"]:
        for pattern in TASK_RULES[task_type]:
            if re.search(pattern, text_lower, re.IGNORECASE):
                return task_type
    return None


async def _llm_classify(text: str) -> str:
    """
    调用 LLM 做意图识别。

    分类器自动选择：
      - DASHSCOPE_API_KEY 已设置 → qwen3.6-flash（云端极速）
      - DASHSCOPE_API_KEY 为空   → qwen2.5:latest（Ollama 本地兜底）

    返回值: 任务类型字符串（若调用失败返回 "general"）
    """
    truncated = text[:CLASSIFIER_MAX_TEXT_LEN]

    try:
        response = await litellm.acompletion(
            model=CLASSIFIER_MODEL,
            api_base=CLASSIFIER_API_BASE,
            api_key=CLASSIFIER_API_KEY,
            messages=[
                {"role": "system", "content": CLASSIFY_SYSTEM_PROMPT},
                {"role": "user", "content": truncated},
            ],
            max_tokens=CLASSIFIER_MAX_TOKENS,
            temperature=0,
            timeout=CLASSIFIER_TIMEOUT,
            metadata={"_task_router_classify": True},  # 标记为分类调用
        )

        result = response.choices[0].message.content.strip().lower()

        # 精确匹配
        if result in TASK_MODEL_MAP:
            return result

        # 模糊匹配（LLM 可能返回额外文字）
        for key in TASK_MODEL_MAP:
            if key in result:
                return key

        logger.warning("LLM 分类返回未知类别: %s，降级为 general", result)
        return "general"

    except Exception as e:
        logger.warning("LLM 分类调用失败 (model=%s): %s，降级为 general", CLASSIFIER_MODEL, e)
        return "general"


async def classify_task(messages: list) -> tuple[str, str, str]:
    """
    混合分类：先规则，后 LLM 兜底。

    返回: (task_type, method, model)
      - task_type: 任务类型
      - method: "rule" 或 "llm"
      - model: 推荐模型组名称
    """
    user_text = _extract_user_text(messages)

    # 第一层：规则匹配（零成本、零延迟）
    rule_result = _rule_classify(user_text)
    if rule_result:
        return rule_result, "rule", TASK_MODEL_MAP[rule_result]

    # 第二层：LLM 意图识别（低成本、低延迟）
    llm_result = await _llm_classify(user_text)
    return llm_result, "llm", TASK_MODEL_MAP[llm_result]


# ==================================================
# 回调 1: 智能任务路由
# ==================================================


class TaskRouterHandler(CustomLogger):
    """
    智能任务路由回调。

    在请求到达 LiteLLM Router 之前，根据任务复杂度
    自动选择最合适的模型组，实现降本增效。

    触发条件：请求中 model="auto" 或 model="auto-route"
    用户显式指定具体模型时不干预。
    """

    async def async_pre_call_hook(
        self,
        user_api_key_dict: dict,
        cache: Any,
        data: dict,
        call_type: str,
    ) -> Optional[dict]:
        """
        请求前置钩子：在 LLM 调用前拦截。
        - model="auto" 时：分类并路由到最合适的模型
        - 推理模型：自动启用流式响应，避免 reasoning tokens 导致 HTTP 超时
        """
        original_model = data.get("model", "")
        modified = False

        # === 智能路由：model="auto" 时分类并选模型 ===
        if original_model in AUTO_MODEL_NAMES:
            messages = data.get("messages", [])
            if not messages:
                data["model"] = DEFAULT_MODEL
            else:
                task_type, method, selected_model = await classify_task(messages)

                # === Semantic Router 域分类增强 ===
                # 读取 Semantic Router 添加的 X-SR-Domain 头
                sr_domain = ""
                try:
                    proxy_req = data.get("proxy_server_request") or {}
                    headers = proxy_req.get("headers") or {}
                    sr_domain = headers.get("x-sr-domain") or headers.get("X-SR-Domain") or ""
                except Exception:
                    pass

                if sr_domain:
                    # STEM 域 → 数学/逻辑推理模型
                    if sr_domain in ("math", "physics", "chemistry", "biology") and task_type not in ("math_logic", "complex_reasoning"):
                        task_type = "math_logic"
                        selected_model = TASK_MODEL_MAP["math_logic"]
                        method = f"{method}+sr:{sr_domain}"
                    # 计算机/工程 → 编程模型
                    elif sr_domain in ("computer_science", "engineering") and task_type not in ("coding", "complex_reasoning"):
                        task_type = "coding"
                        selected_model = TASK_MODEL_MAP["coding"]
                        method = f"{method}+sr:{sr_domain}"

                    logger.info(
                        "[TaskRouter] SR domain=%s, adjusted -> %s (type=%s, method=%s)",
                        sr_domain, selected_model, task_type, method,
                    )

                classification_total.labels(
                    task_type=task_type,
                    method=method,
                    target_model=selected_model,
                ).inc()

                logger.info(
                    "[TaskRouter] model=auto -> %s (type=%s, method=%s)",
                    selected_model,
                    task_type,
                    method,
                )

                data["model"] = selected_model

                metadata = data.get("metadata", {})
                metadata["task_router"] = {
                    "task_type": task_type,
                    "method": method,
                    "original_model": original_model,
                    "routed_model": selected_model,
                }
                data["metadata"] = metadata
            modified = True

        # === 推理模型自动启用流式响应 ===
        # 推理模型生成 reasoning tokens 耗时较长，流式响应让首个 token 快速返回，避免 HTTP 超时
        final_model = data.get("model", "")
        if final_model in INFERENCE_MODELS and not data.get("stream", False):
            data["stream"] = True
            logger.info("[TaskRouter] 推理模型 %s 自动启用流式响应", final_model)
            modified = True

        return data if modified else None


# ==================================================
# 回调 2: 配额追踪
# ==================================================


class QuotaTrackerHandler(CustomLogger):
    """
    配额追踪回调。

    在每次请求成功完成后，记录花费和请求数到 Prometheus。
    预算限制和执行由 LiteLLM 内置的 budget 管理完成。

    指标可在 Grafana 中通过以下 PromQL 查询：
      - 某个 Key 的总花费:
        sum(litellm_quota_key_spend_total{key_alias="my-key"}) by (key_alias)
      - 某个 User 的总花费:
        sum(litellm_quota_user_spend_total{user_id="user-123"}) by (user_id)
      - 按 Model 的花费分布:
        sum(litellm_quota_key_spend_by_model) by (model)
      - 某个 Key 的请求数:
        litellm_quota_key_requests_total{key_alias="my-key"}
    """

    async def async_log_success_event(
        self,
        kwargs: dict,
        response_obj: Any,
        start_time: float,
        end_time: float,
    ) -> None:
        """异步成功日志钩子：请求成功完成后记录花费。"""
        try:
            # 获取标准日志对象
            slo = kwargs.get("standard_logging_object") or {}
            if not slo:
                return

            # 跳过任务路由器的分类调用（不计入用户配额）
            slo_metadata = slo.get("metadata", {}) or {}
            if slo_metadata.get("_task_router_classify"):
                return

            # 提取花费（美元）
            spend = float(slo.get("response_cost", 0.0) or 0.0)
            model = slo.get("model", "unknown") or "unknown"

            # 从 litellm_params.metadata 获取 key/user 信息
            litellm_params = kwargs.get("litellm_params", {}) or {}
            params_metadata = litellm_params.get("metadata", {}) or {}

            key_alias = params_metadata.get("user_api_key_alias") or "unknown"
            user_id = params_metadata.get("user_api_key_user_id") or "anonymous"

            # 记录 Counter 指标
            key_spend_total.labels(
                key_alias=key_alias,
                user_id=user_id,
                model=model,
            ).inc(spend)

            user_spend_total.labels(
                user_id=user_id,
                model=model,
            ).inc(spend)

            key_requests_total.labels(
                key_alias=key_alias,
                user_id=user_id,
            ).inc()

            key_spend_by_model.labels(
                key_alias=key_alias,
                model=model,
            ).inc(spend)

            logger.debug(
                "[QuotaTracker] key=%s user=%s model=%s spend=$%.6f",
                key_alias,
                user_id,
                model,
                spend,
            )

        except Exception as e:
            logger.warning("[QuotaTracker] 记录花费失败: %s", e)

    async def async_log_failure_event(
        self,
        kwargs: dict,
        response_obj: Any,
        start_time: float,
        end_time: float,
    ) -> None:
        """异步失败日志钩子：请求失败时也记录（用于统计错误率）。"""
        try:
            litellm_params = kwargs.get("litellm_params", {}) or {}
            params_metadata = litellm_params.get("metadata", {}) or {}

            key_alias = params_metadata.get("user_api_key_alias") or "unknown"
            user_id = params_metadata.get("user_api_key_user_id") or "anonymous"

            # 失败请求仍计入请求数
            key_requests_total.labels(
                key_alias=key_alias,
                user_id=user_id,
            ).inc()

        except Exception as e:
            logger.warning("[QuotaTracker] 记录失败请求失败: %s", e)


# ==================================================
# 全局实例（LiteLLM 回调加载机制需要）
# 实例名必须与 config_worker.yaml 中 callbacks 配置一致
# ==================================================

task_router = TaskRouterHandler()
quota_tracker = QuotaTrackerHandler()
