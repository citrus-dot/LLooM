#!/usr/bin/env python3
"""
LiteLLM CLI — 模型管理与环境配置工具

命令：
  init          首次环境配置（检测 Docker/Ollama、收集 API Key、适配路由、启动服务）
  add-model     交互式添加新模型（自动更新 config_worker.yaml / custom_callbacks.py / quota_setup.sh）
  list-models   列出已配置的模型
  remove-model  删除指定模型
  health        检查所有服务健康状态

用法：
  python3 litellm_cli.py init
  python3 litellm_cli.py add-model
  python3 litellm_cli.py list-models
  python3 litellm_cli.py remove-model --name gpt-4o
  python3 litellm_cli.py health

约束：仅依赖 Python 3 标准库，无需 pip install
"""

import argparse
import json
import os
import re
import subprocess
import sys
import time
import urllib.request
import urllib.error
from pathlib import Path
from typing import Optional

# ==================================================
# 常量
# ==================================================

PROJECT_ROOT = Path(__file__).parent.resolve()
ENV_FILE = PROJECT_ROOT / ".env"
ENV_EXAMPLE = PROJECT_ROOT / ".env.example"
CONFIG_WORKER = PROJECT_ROOT / "config_worker.yaml"
CUSTOM_CALLBACKS = PROJECT_ROOT / "custom_callbacks.py"
QUOTA_SETUP = PROJECT_ROOT / "quota_setup.sh"

ADMIN_URL = "http://localhost:4000"
WORKER_URL = "http://localhost:4001"
WEBUI_URL = "http://localhost:3001"
GRAFANA_URL = "http://localhost:3000"
ORCHESTRATOR_URL = "http://localhost:3002"

# 供应商定义
PROVIDERS = {
    "dashscope": {
        "label": "阿里云百炼 (DashScope)",
        "env_key": "DASHSCOPE_API_KEY",
        "env_base": "DASHSCOPE_API_BASE",
        "default_base": "https://dashscope.aliyuncs.com/compatible-mode/v1",
        "models": ["qwen-plus", "qwen3.6-flash", "qwen3.6-plus", "qwen3-max", "deepseek-v3"],
    },
    "openai": {
        "label": "OpenAI",
        "env_key": "OPENAI_API_KEY",
        "env_base": "OPENAI_BASE_URL",
        "default_base": "https://api.openai.com/v1",
        "models": ["gpt-4o"],
    },
    "anthropic": {
        "label": "Anthropic",
        "env_key": "ANTHROPIC_API_KEY",
        "env_base": None,
        "default_base": None,
        "models": ["claude-3-5-sonnet"],
    },
    "openrouter": {
        "label": "OpenRouter",
        "env_key": "OR_API_KEY",
        "env_base": None,
        "default_base": "https://openrouter.ai/api/v1",
        "models": [],
    },
}

# 任务类型 → 模型偏好（按优先级降序，第一个可用的胜出）
TASK_PREFERENCES = {
    "simple_qa": ["qwen2.5-local", "qwen3.6-flash", "gpt-4o"],
    "general": ["qwen-plus", "gpt-4o", "claude-3-5-sonnet", "qwen2.5-local"],
    "coding": ["deepseek-v3", "gpt-4o", "qwen-plus", "qwen2.5-local"],
    "math_logic": ["deepseek-v3", "gpt-4o", "qwen-plus", "qwen2.5-local"],
    "complex_reasoning": ["qwen3.6-plus", "gpt-4o", "claude-3-5-sonnet", "qwen-plus", "qwen2.5-local"],
}

TASK_LABELS = {
    "simple_qa": "简单问答（零成本优先）",
    "general": "日常对话/摘要",
    "coding": "代码生成/调试",
    "math_logic": "数学/逻辑推理",
    "complex_reasoning": "复杂推理/深度分析",
}


# ==================================================
# 辅助函数
# ==================================================

def ok(msg):
    print(f"  \033[32m✓\033[0m {msg}")

def fail(msg):
    print(f"  \033[31m✗\033[0m {msg}")

def warn(msg):
    print(f"  \033[33m!\033[0m {msg}")

def prompt(message, default=None):
    suffix = f" [{default}]: " if default else ": "
    user_input = input(f"{message}{suffix}").strip()
    return user_input if user_input else (default or "")

def confirm(message, default=True):
    hint = "Y/n" if default else "y/N"
    user_input = input(f"{message} ({hint}): ").strip().lower()
    if not user_input:
        return default
    return user_input in ("y", "yes")

def select(message, choices, default=None):
    print(f"\n{message}")
    for i, choice in enumerate(choices, 1):
        marker = " →" if choice == default else "  "
        print(f"{marker} {i}. {choice}")
    while True:
        user_input = input(f"选择 (1-{len(choices)}): ").strip()
        if user_input.isdigit() and 1 <= int(user_input) <= len(choices):
            return choices[int(user_input) - 1]
        if default and not user_input:
            return default
        print("  无效选择，请重试")

def run_cmd(cmd, check=True, capture=False):
    if capture:
        result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
        if check and result.returncode != 0:
            fail(f"命令失败: {cmd}")
            if result.stderr:
                print(f"  {result.stderr.strip()}")
            return None
        return result.stdout.strip()
    else:
        result = subprocess.run(cmd, shell=True)
        if check and result.returncode != 0:
            fail(f"命令失败: {cmd}")
            return False
        return True

def http_get(url, headers=None, timeout=5):
    req = urllib.request.Request(url)
    if headers:
        for k, v in headers.items():
            req.add_header(k, v)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return json.loads(resp.read().decode())
    except Exception:
        return None

def http_get_text(url, timeout=5):
    """获取纯文本响应（如 Prometheus 指标）"""
    try:
        with urllib.request.urlopen(url, timeout=timeout) as resp:
            return resp.read().decode()
    except Exception:
        return None

def read_file(path):
    return Path(path).read_text(encoding="utf-8")

def write_file(path, content):
    Path(path).write_text(content, encoding="utf-8")


# ==================================================
# 文件操作 — config_worker.yaml
# ==================================================

def get_config_models():
    """从 config_worker.yaml 解析已注册的 model_name 列表（去重保序）"""
    if not CONFIG_WORKER.exists():
        return []
    content = read_file(CONFIG_WORKER)
    matches = re.findall(r'^\s+- model_name:\s+(\S+)', content, re.MULTILINE)
    seen = set()
    result = []
    for m in matches:
        if m not in seen:
            seen.add(m)
            result.append(m)
    return result

def add_model_to_config(model_name, provider_model, api_base_env, api_key_env,
                        rpm, tpm, input_cost, output_cost):
    """在 config_worker.yaml 的 model_list 末尾（auto 条目之前）插入新模型"""
    content = read_file(CONFIG_WORKER)

    entry = f"""
  # === {model_name}（CLI 添加） ===
  - model_name: {model_name}
    litellm_params:
      model: {provider_model}
      api_base: {api_base_env}
      api_key: {api_key_env}
      rpm: {rpm}
      tpm: {tpm}
      input_cost_per_token: {input_cost}
      output_cost_per_token: {output_cost}
"""

    marker = "  # ==================================================\n  # auto — 智能路由入口"
    if marker in content:
        content = content.replace(marker, entry + marker)
    else:
        marker2 = "# ---- 全局设置 ----"
        content = content.replace(marker2, entry + "\n" + marker2)

    write_file(CONFIG_WORKER, content)

def remove_model_from_config(model_name):
    """从 config_worker.yaml 中删除指定模型条目"""
    content = read_file(CONFIG_WORKER)
    pattern = rf'\n\s*(?:# === [^\n]*\n)?\s*- model_name:\s+{re.escape(model_name)}\n(?:\s+[^\n#-][^\n]*\n)*'
    new_content = re.sub(pattern, '\n', content, count=1)
    if new_content == content:
        return False
    write_file(CONFIG_WORKER, new_content)
    return True


def add_model_to_fallbacks(model_name, fallback_for=None):
    """
    将新模型添加到 router_settings.fallbacks 链中。

    Args:
        model_name: 新添加的模型名称
        fallback_for: 作为哪个模型的 fallback（如 "qwen-plus"）。
                      如果为 None，则将新模型作为 qwen2.5-local 之前的 fallback。
    """
    content = read_file(CONFIG_WORKER)

    if fallback_for:
        # 在指定模型的 fallback 列表中添加新模型
        # 格式: - { "qwen-plus": ["qwen3.6-flash"] }
        # 改为: - { "qwen-plus": ["new-model", "qwen3.6-flash"] }
        pattern = rf'(- \{{\s*"{re.escape(fallback_for)}":\s*\[)([^\]]+)(\]}}\s*)'
        match = re.search(pattern, content)
        if match:
            existing = match.group(2).strip()
            if f'"{model_name}"' in existing:
                return False  # 已存在
            new_list = f'"{model_name}", {existing}'
            new_content = content[:match.start(2)] + new_list + content[match.end(2):]
            write_file(CONFIG_WORKER, new_content)
            return True
    else:
        # 在 qwen2.5-local 之前插入新模型作为最终 fallback 之前的一层
        # 找到 qwen2.5-local 作为 fallback 的条目，在其前面添加新模型作为某个模型的 fallback
        # 策略：将新模型添加到 fallback 链末尾（qwen2.5-local 之前）
        pattern = r'(- \{[^}]*"qwen2\.5-local"[^}]*\})'
        match = re.search(pattern, content)
        if match:
            existing_line = match.group(1)
            if f'"{model_name}"' in existing_line:
                return False
            # 在 qwen2.5-local 前插入新模型
            new_line = existing_line.replace('"qwen2.5-local"', f'"{model_name}", "qwen2.5-local"')
            new_content = content.replace(existing_line, new_line, 1)
            write_file(CONFIG_WORKER, new_content)
            return True
    return False


def remove_model_from_fallbacks(model_name):
    """从 router_settings.fallbacks 链中删除模型"""
    content = read_file(CONFIG_WORKER)
    # 删除 fallback 列表中的模型名引用
    new_content = re.sub(rf',?\s*"{re.escape(model_name)}"', '', content)
    if new_content != content:
        write_file(CONFIG_WORKER, new_content)
        return True
    return False


# ==================================================
# 文件操作 — custom_callbacks.py
# ==================================================

def update_task_model_map(model_map):
    """替换 custom_callbacks.py 中的 TASK_MODEL_MAP 字典"""
    content = read_file(CUSTOM_CALLBACKS)

    new_map_lines = ["TASK_MODEL_MAP: dict[str, str] = {"]
    for task_type, model in model_map.items():
        new_map_lines.append(f'    "{task_type}": "{model}",')
    new_map_lines.append("}")
    new_map = "\n".join(new_map_lines)

    pattern = r'TASK_MODEL_MAP: dict\[str, str\] = \{[^}]+\}'
    new_content = re.sub(pattern, new_map, content, count=1)

    if new_content != content:
        write_file(CUSTOM_CALLBACKS, new_content)
        return True
    return False

def add_task_mapping(task_type, model_name):
    """在 TASK_MODEL_MAP 中添加或替换某任务类型的模型"""
    content = read_file(CUSTOM_CALLBACKS)
    pattern = rf'("{task_type}":\s*)"[^"]*"'
    replacement = f'\\1"{model_name}"  # CLI 更新'
    new_content = re.sub(pattern, replacement, content, count=1)
    if new_content == content:
        pattern2 = r'(TASK_MODEL_MAP: dict\[str, str\] = \{[^}]+)'
        insert = f'\n    "{task_type}": "{model_name}",  # CLI 添加'
        new_content = re.sub(pattern2, r'\1' + insert, content, count=1)
    write_file(CUSTOM_CALLBACKS, new_content)

def add_inference_model(model_name):
    """将模型添加到 INFERENCE_MODELS 集合"""
    content = read_file(CUSTOM_CALLBACKS)
    if f'"{model_name}"' in content.split("INFERENCE_MODELS")[1].split("\n")[0] if "INFERENCE_MODELS" in content else False:
        return
    pattern = r'(INFERENCE_MODELS\s*=\s*\{)([^}]+)(\})'
    match = re.search(pattern, content)
    if match:
        current = match.group(2).rstrip()
        if not current.endswith(","):
            current += ","
        new_set = f'{current} "{model_name}"'
        new_content = content[:match.start()] + match.group(1) + new_set + match.group(3) + content[match.end():]
        write_file(CUSTOM_CALLBACKS, new_content)


# ==================================================
# 文件操作 — quota_setup.sh
# ==================================================

def add_model_to_quota_setup(model_name):
    """在 quota_setup.sh 的 models 列表中添加模型"""
    content = read_file(QUOTA_SETUP)
    pattern = r'("models":\s*\[)([^\]]+)(\])'
    match = re.search(pattern, content)
    if match:
        models_str = match.group(2)
        if f'"{model_name}"' in models_str:
            return
        new_models = models_str.rstrip().rstrip(",") + f', "{model_name}"'
        new_content = content[:match.start(2)] + new_models + content[match.end(2):]
        write_file(QUOTA_SETUP, new_content)

def remove_model_from_quota_setup(model_name):
    """从 quota_setup.sh 的 models 列表中删除模型"""
    content = read_file(QUOTA_SETUP)
    new_content = re.sub(rf',?\s*"{re.escape(model_name)}"', '', content)
    if new_content != content:
        write_file(QUOTA_SETUP, new_content)


# ==================================================
# 文件操作 — .env
# ==================================================

def generate_env(api_keys, ollama_base=None, redis_password="litellm_redis_2026"):
    """从 .env.example 生成 .env 文件，填入用户提供的 API Key"""
    if ENV_EXAMPLE.exists():
        content = read_file(ENV_EXAMPLE)
    elif ENV_FILE.exists():
        content = read_file(ENV_FILE)
    else:
        content = ""

    for key, value in api_keys.items():
        pattern = rf'^({re.escape(key)}=).*$'
        if re.search(pattern, content, re.MULTILINE):
            content = re.sub(pattern, rf'\g<1>{value}', content, flags=re.MULTILINE)
        else:
            content += f"\n{key}={value}"

    if redis_password:
        redis_url = f"redis://:{redis_password}@redis:6379"
        for key, value in [("REDIS_PASSWORD", redis_password), ("REDIS_URL", redis_url)]:
            pattern = rf'^({re.escape(key)}=).*$'
            if re.search(pattern, content, re.MULTILINE):
                content = re.sub(pattern, rf'\g<1>{value}', content, flags=re.MULTILINE)
            else:
                content += f"\n{key}={value}"

    if ollama_base:
        pattern = r'^(OLLAMA_API_BASE=).*$'
        if re.search(pattern, content, re.MULTILINE):
            content = re.sub(pattern, rf'\g<1>{ollama_base}', content, flags=re.MULTILINE)
        else:
            content += f"\nOLLAMA_API_BASE={ollama_base}\n"

    write_file(ENV_FILE, content)


# ==================================================
# 环境检测
# ==================================================

def check_docker():
    ver = run_cmd("docker --version", check=False, capture=True)
    if not ver:
        return None
    compose_ver = run_cmd("docker compose version", check=False, capture=True)
    return {"docker": ver, "compose": compose_ver}

def check_ollama():
    try:
        with urllib.request.urlopen("http://localhost:11434/api/tags", timeout=3) as resp:
            if resp.status == 200:
                data = json.loads(resp.read().decode())
                models = [m["name"] for m in data.get("models", [])]
                return {"running": True, "models": models}
    except Exception:
        pass
    return {"running": False, "models": []}

def determine_available_models(api_keys, ollama_ok):
    """根据已配置的 API Key 和 Ollama 状态，确定可用模型列表"""
    available = []
    if ollama_ok:
        available.append("qwen2.5-local")
    for provider, config in PROVIDERS.items():
        key = config["env_key"]
        if key and api_keys.get(key):
            available.extend(config["models"])
    # 去重保序
    seen = set()
    result = []
    for m in available:
        if m not in seen:
            seen.add(m)
            result.append(m)
    return result

def adapt_routing(available_models):
    """根据可用模型列表，生成最优 TASK_MODEL_MAP"""
    model_map = {}
    for task_type, preferences in TASK_PREFERENCES.items():
        chosen = "qwen2.5-local"  # 最终兜底
        for pref in preferences:
            if pref in available_models:
                chosen = pref
                break
        model_map[task_type] = chosen
    return model_map


# ==================================================
# 命令: init
# ==================================================

def cmd_init(args):
    print("=" * 55)
    print(" LiteLLM 环境配置向导")
    print("=" * 55)

    # 1. 前置条件
    print("\n[1/7] 检查前置条件...")
    docker_info = check_docker()
    if not docker_info:
        fail("Docker 未安装，请先安装 Docker")
        sys.exit(1)
    ok(f"Docker: {docker_info['docker']}")
    if docker_info["compose"]:
        ok(f"Docker Compose: {docker_info['compose']}")
    else:
        fail("Docker Compose 未安装")
        sys.exit(1)

    # 2. Ollama
    print("\n[2/7] 检查 Ollama...")
    ollama = check_ollama()
    ollama_ok = ollama["running"]
    if ollama_ok:
        ok("Ollama 正在运行 (localhost:11434)")
        if "qwen2.5:latest" not in ollama["models"]:
            warn("qwen2.5:latest 未安装，正在拉取...")
            run_cmd("ollama pull qwen2.5:latest", check=False)
        else:
            ok("qwen2.5:latest 已安装")
    else:
        warn("Ollama 未运行")
        if confirm("是否安装 Ollama？(参见 https://ollama.com)", default=False):
            print("  请访问 https://ollama.com 下载安装，安装后运行: ollama pull qwen2.5:latest")
            print("  然后重新执行: python3 litellm_cli.py init")
            sys.exit(0)
        warn("将跳过本地模型层（qwen2.5-local 不可用）")

    # 3. API 密钥
    print("\n[3/7] 配置 API 密钥...")
    api_keys = {}
    for pid, config in PROVIDERS.items():
        if confirm(f"\n  是否使用 {config['label']}？", default=(pid == "dashscope")):
            key = prompt("  输入 API Key")
            if key:
                api_keys[config["env_key"]] = key
                if config["env_base"]:
                    base = prompt("  输入 API Base URL", config["default_base"])
                    api_keys[config["env_base"]] = base

    if not api_keys and not ollama_ok:
        fail("未配置任何 API Key 且 Ollama 不可用，无法运行")
        sys.exit(1)

    # 4. 生成 .env
    print("\n[4/7] 生成 .env 文件...")
    ollama_base = "http://host.docker.internal:11434" if ollama_ok else None
    generate_env(api_keys, ollama_base)
    ok(".env 已生成")

    # 5. 路由适配
    print("\n[5/7] 适配路由策略...")
    available = determine_available_models(api_keys, ollama_ok)
    model_map = adapt_routing(available)

    classifier = "qwen3.6-flash (云端)" if api_keys.get("DASHSCOPE_API_KEY") else "qwen2.5:latest (Ollama 本地)"
    print(f"  可用模型: {', '.join(available)}")
    print(f"  分类器: {classifier}")
    print("  路由映射:")
    for task_type, model in model_map.items():
        label = TASK_LABELS.get(task_type, task_type)
        print(f"    {task_type:<20} → {model}  ({label})")

    if CUSTOM_CALLBACKS.exists():
        update_task_model_map(model_map)
        ok("TASK_MODEL_MAP 已更新")
    else:
        warn("custom_callbacks.py 不存在，跳过路由更新")

    # 6. 启动服务
    print("\n[6/7] 启动服务...")
    print("  正在启动 Docker Compose 服务（可能需要几分钟）...")
    run_cmd("docker compose up -d", check=False)
    ok("docker compose up -d 已执行")

    # 7. 健康检查
    print("\n[7/7] 健康检查...")
    print("  等待服务就绪...")
    time.sleep(20)
    cmd_health(args)

    print("\n" + "=" * 55)
    print(" 配置完成！")
    print("=" * 55)
    print(f"  Admin UI:      {ADMIN_URL}")
    print(f"  Worker API:    {WORKER_URL}")
    print(f"  Chat UI:       {WEBUI_URL}")
    print(f"  Grafana:       {GRAFANA_URL}")
    if ollama_ok:
        print(f"  本地模型:       已启用（qwen2.5-local，零成本层）")


# ==================================================
# 命令: add-model
# ==================================================

def cmd_add_model(args):
    print("=" * 55)
    print(" 添加新模型")
    print("=" * 55)

    # 1. 基本信息
    print("\n[1/5] 模型基本信息")
    model_name = prompt("  模型名称（model_name，如 my-gpt-4o）")
    if not model_name:
        fail("模型名称不能为空")
        sys.exit(1)

    existing = get_config_models()
    if model_name in existing:
        warn(f"模型 '{model_name}' 已存在于 config_worker.yaml")
        if not confirm("是否继续（将创建同名条目用于负载均衡）？", default=False):
            sys.exit(0)

    # 2. 供应商
    print("\n[2/5] 供应商配置")
    provider = select("  选择供应商", list(PROVIDERS.keys()) + ["custom"])
    config = PROVIDERS.get(provider, {})

    if provider == "custom":
        provider_model = prompt("  litellm model 字符串（如 openai/my-model）")
        api_base_env = prompt("  api_base 环境变量名（如 MY_API_BASE）")
        api_key_env = prompt("  api_key 环境变量名（如 MY_API_KEY）")
    else:
        model_prefix = {"dashscope": "openai", "openai": "openai", "anthropic": "anthropic", "openrouter": "openrouter"}
        default_model = f"{model_prefix.get(provider, 'openai')}/{model_name}"
        provider_model = prompt("  litellm model 字符串", default_model)

        if config.get("env_base"):
            api_base_env = f"os.environ/{config['env_base']}"
        else:
            api_base_env = prompt("  api_base 环境变量名（留空则不设置）") or ""
        api_key_env = f"os.environ/{config['env_key']}"

    # 3. 限制与定价
    print("\n[3/5] 速率限制与定价")
    rpm = int(prompt("  RPM 限制", "100") or "100")
    tpm = int(prompt("  TPM 限制", "50000") or "50000")
    input_cost = prompt("  输入价格 ($/token，如 0.000001)", "0") or "0"
    output_cost = prompt("  输出价格 ($/token，如 0.000002)", "0") or "0"

    # 4. 任务路由 + Fallback 链（可选）
    print("\n[4/6] 任务路由与容灾（可选）")
    task_types = list(TASK_LABELS.keys())
    selected_task = select("  将此模型分配给哪个任务类型？", ["跳过"] + task_types, "跳过")
    if selected_task == "跳过":
        selected_task = None

    is_inference = confirm("\n  这是推理模型吗？（自动启用流式响应）", default=False)

    # Fallback 配置
    print("\n  Fallback 链配置:")
    print("  当其他模型失败时，可以回退到此模型作为备用。")
    fallback_for = None
    if confirm("  是否将此模型加入 fallback 容灾链？", default=True):
        existing_models = [m for m in get_config_models() if m != model_name and m != "auto"]
        if existing_models:
            fallback_for = select("  作为哪个模型的 fallback？", existing_models + ["(最终兜底，qwen2.5-local 之前)"])
            if fallback_for == "(最终兜底，qwen2.5-local 之前)":
                fallback_for = None  # None = 插入到 qwen2.5-local 之前
        else:
            warn("  无其他模型可选，将作为最终兜底")
            fallback_for = None

    # 5. 确认
    print("\n[5/6] 确认")
    print(f"  模型名称:     {model_name}")
    print(f"  供应商模型:   {provider_model}")
    print(f"  API Base:     {api_base_env or '(无)'}")
    print(f"  API Key:      {api_key_env}")
    print(f"  RPM/TPM:      {rpm}/{tpm}")
    print(f"  输入/输出价格: ${input_cost}/${output_cost} per token")
    if selected_task:
        print(f"  任务路由:     {selected_task} → {model_name}")
    if is_inference:
        print(f"  推理模型:     是（自动流式）")
    if fallback_for is not None:
        print(f"  Fallback:     {model_name} 作为 {fallback_for} 的备用")
    elif fallback_for is None and "fallback_for" in dir():
        print(f"  Fallback:     {model_name} 作为最终兜底（qwen2.5-local 之前）")

    if not confirm("\n  确认添加？", default=True):
        print("  已取消")
        sys.exit(0)

    # 6. 写入文件
    print("\n[6/6] 写入配置...")
    add_model_to_config(model_name, provider_model, api_base_env, api_key_env,
                        rpm, tpm, input_cost, output_cost)
    ok(f"已添加到 {CONFIG_WORKER.name}")

    if selected_task and CUSTOM_CALLBACKS.exists():
        add_task_mapping(selected_task, model_name)
        ok(f"已更新 TASK_MODEL_MAP: {selected_task} → {model_name}")

    if is_inference and CUSTOM_CALLBACKS.exists():
        add_inference_model(model_name)
        ok(f"已添加到 INFERENCE_MODELS: {model_name}")

    if QUOTA_SETUP.exists():
        add_model_to_quota_setup(model_name)
        ok(f"已添加到 {QUOTA_SETUP.name}")

    # Fallback 链更新
    if fallback_for is not None:
        if add_model_to_fallbacks(model_name, fallback_for=fallback_for):
            ok(f"已加入 fallbacks: {fallback_for} → {model_name}")
        else:
            warn(f"fallbacks 更新失败或已存在（{fallback_for} → {model_name}）")
    else:
        if add_model_to_fallbacks(model_name, fallback_for=None):
            ok(f"已加入 fallbacks: 最终兜底（qwen2.5-local 之前）")
        else:
            warn(f"fallbacks 更新失败或已存在")

    if confirm("\n  是否重启 Worker 使配置生效？", default=True):
        run_cmd("docker compose restart litellm-worker", check=False)
        ok("Worker 已重启")
        print("  等待服务就绪...")
        time.sleep(10)

    print(f"\n  模型 '{model_name}' 添加完成！")


# ==================================================
# 命令: list-models
# ==================================================

def cmd_list_models(args):
    print("已配置模型（config_worker.yaml）:")
    print("-" * 60)
    models = get_config_models()
    if not models:
        print("  (无)")
    else:
        for i, m in enumerate(models, 1):
            print(f"  {i:2d}. {m}")

    # 也查询 Worker 运行时模型
    data = http_get(f"{WORKER_URL}/v1/models",
                    headers={"Authorization": "Bearer sk-1234"}, timeout=5)
    if data and "data" in data:
        runtime_models = [m["id"] for m in data["data"]]
        print(f"\nWorker 运行时模型 ({len(runtime_models)}):")
        print("-" * 60)
        for m in runtime_models:
            tag = " (DB)" if m not in models else ""
            print(f"  • {m}{tag}")


# ==================================================
# 命令: remove-model
# ==================================================

def cmd_remove_model(args):
    model_name = args.name
    if not model_name:
        model_name = prompt("输入要删除的模型名称")
    if not model_name:
        fail("模型名称不能为空")
        sys.exit(1)

    print(f"正在删除模型: {model_name}")
    if not confirm("确认删除？", default=False):
        print("  已取消")
        sys.exit(0)

    removed = remove_model_from_config(model_name)
    if removed:
        ok(f"已从 config_worker.yaml 删除")
    else:
        warn(f"config_worker.yaml 中未找到")

    # 同时从 fallbacks 链中删除
    if remove_model_from_fallbacks(model_name):
        ok(f"已从 fallbacks 容灾链删除")

    if QUOTA_SETUP.exists():
        remove_model_from_quota_setup(model_name)
        ok(f"已从 quota_setup.sh 删除")

    if confirm("是否重启 Worker？", default=True):
        run_cmd("docker compose restart litellm-worker", check=False)
        ok("Worker 已重启")


# ==================================================
# 命令: health
# ==================================================

def cmd_health(args):
    print("服务健康状态:")
    print("-" * 60)

    # Docker 容器状态
    containers = run_cmd(
        'docker compose ps --format "{{.Name}}|{{.Status}}" 2>/dev/null',
        check=False, capture=True
    )
    if containers:
        for line in containers.strip().split("\n"):
            if "|" in line:
                name, status = line.split("|", 1)
                is_healthy = "healthy" in status.lower() or "up" in status.lower()
                if is_healthy:
                    ok(f"{name}: {status}")
                else:
                    fail(f"{name}: {status}")

    # API 端点
    print()
    admin_health = http_get(f"{ADMIN_URL}/health/liveliness", timeout=5)
    if admin_health:
        ok(f"Admin API (4000): 正常")
    else:
        fail(f"Admin API (4000): 不可达")

    worker_health = http_get(f"{WORKER_URL}/health/liveliness", timeout=5)
    if worker_health:
        ok(f"Worker API (4001): 正常")
    else:
        fail(f"Worker API (4001): 不可达")

    # 模型列表
    models = http_get(f"{WORKER_URL}/v1/models",
                      headers={"Authorization": "Bearer sk-1234"}, timeout=5)
    if models and "data" in models:
        ok(f"Worker 模型: {len(models['data'])} 个可用")
    else:
        warn("Worker 模型: 无法获取")

    # Ollama
    ollama = check_ollama()
    if ollama["running"]:
        ok(f"Ollama: 运行中 ({len(ollama['models'])} 个模型)")
    else:
        warn("Ollama: 未运行")

    # Chat UI
    webui = http_get(f"{WEBUI_URL}/health", timeout=3)
    if webui is not None or True:
        try:
            urllib.request.urlopen(WEBUI_URL, timeout=3)
            ok(f"Chat UI (3001): 正常")
        except Exception:
            warn(f"Chat UI (3001): 未启动")

    # Orchestrator Web
    try:
        urllib.request.urlopen(f"{ORCHESTRATOR_URL}/", timeout=3)
        ok(f"编排界面 (3002): 正常")
    except Exception:
        warn(f"编排界面 (3002): 未启动")


# ==================================================
# 命令: logs
# ==================================================

def cmd_logs(args):
    """查看服务日志"""
    service = args.service or "litellm-worker"
    lines = args.lines or 50

    valid_services = ["litellm-admin", "litellm-worker", "redis", "db", "qdrant",
                      "prometheus", "grafana", "open-webui"]

    if service not in valid_services:
        # 模糊匹配
        matches = [s for s in valid_services if service in s]
        if len(matches) == 1:
            service = matches[0]
        else:
            fail(f"未知服务: {service}")
            print(f"  可用服务: {', '.join(valid_services)}")
            sys.exit(1)

    print(f"--- {service} 最近 {lines} 行日志 ---")
    run_cmd(f"docker compose logs --tail {lines} {service}", check=False)


# ==================================================
# 命令: status
# ==================================================

def cmd_status(args):
    """查看运行时状态：路由统计、模型用量、缓存命中"""
    print("=" * 55)
    print(" LiteLLM 运行时状态")
    print("=" * 55)

    # 1. 容器资源使用
    print("\n[1/4] 容器资源使用:")
    stats = run_cmd(
        'docker stats --no-stream --format "{{.Name}}|{{.CPUPerc}}|{{.MemUsage}}|{{.NetIO}}" '
        'litellm_admin litellm_worker litellm_redis litellm_db litellm_webui 2>/dev/null',
        check=False, capture=True
    )
    if stats:
        print(f"  {'容器':<25} {'CPU':<8} {'内存':<20} {'网络IO'}")
        print(f"  {'-'*70}")
        for line in stats.strip().split("\n"):
            if "|" in line:
                parts = line.split("|")
                if len(parts) >= 4:
                    name, cpu, mem, net = parts
                    print(f"  {name:<25} {cpu:<8} {mem:<20} {net}")
    else:
        warn("无法获取容器资源统计")

    # 2. 路由分类统计（从 Prometheus 指标）
    print("\n[2/4] 智能路由分类统计:")
    metrics = http_get_text(f"{WORKER_URL}/metrics/", timeout=5)
    if metrics:
        router_lines = [l for l in metrics.split("\n") if "litellm_task_router_classification_total" in l
                        and not l.startswith("#")]
        if router_lines:
            print(f"  {'任务类型':<20} {'方法':<8} {'目标模型':<20} {'次数'}")
            print(f"  {'-'*60}")
            for line in router_lines:
                # 格式: litellm_task_router_classification_total{method="rule",target_model="qwen2.5-local",task_type="simple_qa"} 3.0
                m = re.search(r'method="([^"]*)".*target_model="([^"]*)".*task_type="([^"]*)"\}\s+(\S+)', line)
                if m:
                    method, model, task_type, count = m.groups()
                    print(f"  {task_type:<20} {method:<8} {model:<20} {float(count):.0f}")
        else:
            print("  (暂无路由数据)")
    else:
        warn("无法获取 Prometheus 指标")

    # 3. 配额追踪
    print("\n[3/4] 配额追踪 (Top 5 模型花费):")
    if metrics:
        spend_lines = [l for l in metrics.split("\n") if "litellm_quota_key_spend_by_model_total" in l
                       and not l.startswith("#") and "_created" not in l]
        if spend_lines:
            spends = []
            for line in spend_lines:
                m = re.search(r'key_alias="([^"]*)".*model="([^"]*)"\}\s+(\S+)', line)
                if m:
                    key, model, amount = m.groups()
                    spends.append((model, float(amount)))
            spends.sort(key=lambda x: x[1], reverse=True)
            for model, amount in spends[:5]:
                print(f"  {model:<25} ${amount:.6f}")
        else:
            print("  (暂无花费数据)")
    else:
        warn("无法获取配额指标")

    # 4. 模型健康状态
    print("\n[4/4] 模型可用性:")
    models_data = http_get(f"{WORKER_URL}/v1/models",
                           headers={"Authorization": "Bearer sk-1234"}, timeout=5)
    if models_data and "data" in models_data:
        for m in models_data["data"]:
            model_id = m["id"]
            # 快速健康检查
            try:
                req = urllib.request.Request(
                    f"{WORKER_URL}/v1/chat/completions",
                    method="POST"
                )
                req.add_header("Authorization", "Bearer sk-1234")
                req.add_header("Content-Type", "application/json")
                body = json.dumps({
                    "model": model_id,
                    "messages": [{"role": "user", "content": "hi"}],
                    "max_tokens": 1
                }).encode()
                urllib.request.urlopen(req, timeout=10, data=body)
                ok(f"{model_id}")
            except Exception as e:
                err = str(e)
                if "400" in err or "401" in err:
                    ok(f"{model_id} (需认证)")
                else:
                    fail(f"{model_id}: {err[:60]}")
    else:
        warn("无法获取模型列表")

    print("\n" + "=" * 55)


# ==================================================
# 命令: orchestrate
# ==================================================

def cmd_orchestrate(args):
    """编排复杂任务：自动分解 + 多模型执行 + 成本追踪"""
    query = args.query
    if not query:
        query = prompt("输入任务描述")
    if not query:
        fail("任务描述不能为空")
        sys.exit(1)

    print("=" * 55)
    print(" 复杂任务编排")
    print("=" * 55)
    print(f"\n任务: {query[:80]}{'...' if len(query) > 80 else ''}")

    # 导入编排引擎
    sys.path.insert(0, str(PROJECT_ROOT))
    try:
        from task_orchestrator import Orchestrator
    except ImportError:
        fail("无法导入 task_orchestrator.py，请确保文件存在")
        sys.exit(1)

    master_key = os.environ.get("LITELLM_MASTER_KEY", "sk-1234")
    orch = Orchestrator(WORKER_URL, master_key)

    # 步骤 1: 复杂度检测
    print("\n[1/5] 复杂度检测...")
    is_complex = orch.is_complex(query)
    if is_complex:
        ok("检测到复杂任务，需要分解")
    else:
        ok("任务较简单，直接执行")

    if args.no_decompose or not is_complex:
        print("\n[2/5] 跳过分解，直接执行...")
        result = orch.orchestrate(query)
        print(f"\n{'=' * 55}")
        print(" 执行结果")
        print(f"{'=' * 55}")
        print(f"\n{result.final_response}")
        print(f"\n{'─' * 40}")
        print(f"耗时: {result.total_duration:.1f}s | 成本: ${result.total_cost:.6f} | Tokens: {result.total_tokens}")
        return

    # 步骤 2: 任务分解
    print("\n[2/5] 任务分解...")
    sub_tasks = orch.decompose(query)
    ok(f"分解为 {len(sub_tasks)} 个子任务:")
    for t in sub_tasks:
        print(f"  {t.id}. [{t.task_type}] {t.description[:50]}")
        if t.depends_on:
            print(f"     依赖: {t.depends_on}")

    # 步骤 3: 成本规划
    print("\n[3/5] 成本规划...")
    sub_tasks = orch.plan_costs(sub_tasks)
    total_est = sum(t.cost for t in sub_tasks)
    for t in sub_tasks:
        pricing_label = ""
        for name, pricing in {
            "qwen2.5-local": "零成本",
            "qwen3.6-flash": "极低成本",
            "qwen-plus": "低成本",
            "deepseek-v3": "中成本",
            "qwen3.6-plus": "高成本",
        }.items():
            if t.selected_model == name:
                pricing_label = pricing
                break
        print(f"  子任务 {t.id} → {t.selected_model} ({pricing_label}) 预估 ${t.cost:.6f}")
    print(f"  总预估成本: ${total_est:.6f}")

    # 步骤 4: 执行
    print("\n[4/5] 执行子任务...")
    sub_tasks = orch.execute_all(sub_tasks)
    for t in sub_tasks:
        status_icon = "✓" if t.status == "done" else "✗"
        print(f"  {status_icon} 子任务 {t.id}: {t.selected_model} | {t.duration:.1f}s | ${t.cost:.6f} | {t.tokens_used} tokens")

    # 步骤 5: 汇总
    print("\n[5/5] 汇总结果...")
    final = orch.aggregate(query, sub_tasks)

    print(f"\n{'=' * 55}")
    print(" 最终结果")
    print(f"{'=' * 55}")
    print(f"\n{final}")

    total_cost = sum(t.cost for t in sub_tasks)
    total_tokens = sum(t.tokens_used for t in sub_tasks)
    total_duration = sum(t.duration for t in sub_tasks)

    print(f"\n{'─' * 40}")
    print(f"子任务数: {len(sub_tasks)} | 总成本: ${total_cost:.6f} | 总Tokens: {total_tokens} | 总耗时: {total_duration:.1f}s")
    print(f"\n管理界面: {ORCHESTRATOR_URL}")


# ==================================================
# 命令: add-model-json（非交互式，供 Tauri app 调用）
# ==================================================

def cmd_add_model_json(args):
    """非交互式添加模型，参数通过 JSON 字符串传入"""
    try:
        config = json.loads(args.json_config)
    except json.JSONDecodeError as e:
        print(json.dumps({"ok": False, "error": f"JSON 解析失败: {e}"}))
        sys.exit(1)

    model_name = config.get("model_name", "").strip()
    if not model_name:
        print(json.dumps({"ok": False, "error": "model_name 不能为空"}))
        sys.exit(1)

    existing = get_config_models()
    if model_name in existing:
        print(json.dumps({"ok": False, "error": f"模型 '{model_name}' 已存在"}))
        sys.exit(1)

    provider_model = config.get("provider_model", f"openai/{model_name}")
    api_base_env = config.get("api_base_env", "")
    api_key_env = config.get("api_key_env", "")
    rpm = int(config.get("rpm", 100))
    tpm = int(config.get("tpm", 50000))
    input_cost = config.get("input_cost", "0")
    output_cost = config.get("output_cost", "0")
    task_type = config.get("task_type")  # None = skip
    is_inference = config.get("is_inference", False)
    fallback_for = config.get("fallback_for")  # None = insert before qwen2.5-local

    results = []

    add_model_to_config(model_name, provider_model, api_base_env, api_key_env,
                        rpm, tpm, input_cost, output_cost)
    results.append(f"config_worker.yaml: 已添加 {model_name}")

    if task_type and CUSTOM_CALLBACKS.exists():
        add_task_mapping(task_type, model_name)
        results.append(f"custom_callbacks.py: TASK_MODEL_MAP[{task_type}] → {model_name}")

    if is_inference and CUSTOM_CALLBACKS.exists():
        add_inference_model(model_name)
        results.append(f"custom_callbacks.py: INFERENCE_MODELS += {model_name}")

    if QUOTA_SETUP.exists():
        add_model_to_quota_setup(model_name)
        results.append(f"quota_setup.sh: 已添加 {model_name}")

    if fallback_for is not None:
        if add_model_to_fallbacks(model_name, fallback_for=fallback_for):
            results.append(f"config_worker.yaml: fallbacks[{fallback_for}] += {model_name}")
    else:
        if add_model_to_fallbacks(model_name, fallback_for=None):
            results.append(f"config_worker.yaml: fallbacks 最终兜底 += {model_name}")

    print(json.dumps({"ok": True, "model_name": model_name, "details": results}, ensure_ascii=False))


# ==================================================
# 命令: remove-model-json（非交互式，供 Tauri app 调用）
# ==================================================

def cmd_remove_model_json(args):
    """非交互式删除模型"""
    model_name = args.name
    if not model_name:
        print(json.dumps({"ok": False, "error": "模型名称不能为空"}))
        sys.exit(1)

    results = []
    removed = remove_model_from_config(model_name)
    if removed:
        results.append(f"config_worker.yaml: 已删除 {model_name}")

    if remove_model_from_fallbacks(model_name):
        results.append(f"config_worker.yaml: fallbacks 已移除 {model_name}")

    if QUOTA_SETUP.exists():
        remove_model_from_quota_setup(model_name)
        results.append(f"quota_setup.sh: 已移除 {model_name}")

    # 从 TASK_MODEL_MAP 中移除引用（重置为默认）
    if CUSTOM_CALLBACKS.exists():
        content = read_file(CUSTOM_CALLBACKS)
        pattern = rf'("{re.escape(model_name)}"\s*:\s*")'
        if re.search(pattern, content):
            # 不直接删除，而是标记警告
            results.append(f"custom_callbacks.py: 警告 — TASK_MODEL_MAP 中仍有 {model_name} 引用，请手动检查")

    if not results:
        print(json.dumps({"ok": False, "error": f"未找到模型 {model_name}"}))
        sys.exit(1)

    print(json.dumps({"ok": True, "model_name": model_name, "details": results}, ensure_ascii=False))


# ==================================================
# 命令: list-models-json（供 Tauri app 调用）
# ==================================================

def cmd_list_models_json(args):
    """以 JSON 格式输出已配置模型列表"""
    models = get_config_models()

    # 解析每个模型的详细信息
    content = read_file(CONFIG_WORKER) if CONFIG_WORKER.exists() else ""
    model_details = []
    for name in models:
        if name == "auto":
            continue
        detail = {"model_name": name, "provider_model": "", "input_cost": "0", "output_cost": "0"}
        # 简单解析：找到 model_name 对应的块
        pattern = rf'model_name:\s+{re.escape(name)}\s*\n((?:\s+[^\n]*\n)*?)\s*(?:- model_name:|# ===|$)'
        match = re.search(pattern, content)
        if match:
            block = match.group(1)
            pm = re.search(r'model:\s+(\S+)', block)
            if pm:
                detail["provider_model"] = pm.group(1)
            ic = re.search(r'input_cost_per_token:\s+(\S+)', block)
            if ic:
                detail["input_cost"] = ic.group(1)
            oc = re.search(r'output_cost_per_token:\s+(\S+)', block)
            if oc:
                detail["output_cost"] = oc.group(1)
        model_details.append(detail)

    print(json.dumps({"models": model_details}, ensure_ascii=False))


# ==================================================
# 主入口
# ==================================================

def main():
    parser = argparse.ArgumentParser(
        description="LiteLLM CLI — 模型管理与环境配置工具",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    sub = parser.add_subparsers(dest="command")

    sub.add_parser("init", help="首次环境配置（交互式向导）")
    sub.add_parser("add-model", help="添加新模型（交互式向导）")
    sub.add_parser("list-models", help="列出已配置的模型")

    rm = sub.add_parser("remove-model", help="删除指定模型")
    rm.add_argument("--name", help="模型名称")

    add_json = sub.add_parser("add-model-json", help="非交互式添加模型（JSON 参数）")
    add_json.add_argument("json_config", help="模型配置 JSON 字符串")

    rm_json = sub.add_parser("remove-model-json", help="非交互式删除模型")
    rm_json.add_argument("--name", required=True, help="模型名称")

    sub.add_parser("list-models-json", help="以 JSON 格式列出已配置模型")

    sub.add_parser("health", help="检查服务健康状态")

    logs_parser = sub.add_parser("logs", help="查看服务日志")
    logs_parser.add_argument("service", nargs="?", default="litellm-worker",
                             help="服务名 (如 litellm-worker, redis, db)")
    logs_parser.add_argument("--lines", "-n", type=int, default=50,
                             help="显示行数 (默认 50)")

    sub.add_parser("status", help="查看运行时状态（路由统计、模型用量、缓存命中）")

    orch_parser = sub.add_parser("orchestrate", help="编排复杂任务（自动分解+多模型执行）")
    orch_parser.add_argument("query", nargs="?", help="要编排的任务描述")
    orch_parser.add_argument("--no-decompose", action="store_true",
                             help="跳过分解，直接执行")

    args = parser.parse_args()

    if args.command == "init":
        cmd_init(args)
    elif args.command == "add-model":
        cmd_add_model(args)
    elif args.command == "list-models":
        cmd_list_models(args)
    elif args.command == "remove-model":
        cmd_remove_model(args)
    elif args.command == "add-model-json":
        cmd_add_model_json(args)
    elif args.command == "remove-model-json":
        cmd_remove_model_json(args)
    elif args.command == "list-models-json":
        cmd_list_models_json(args)
    elif args.command == "health":
        cmd_health(args)
    elif args.command == "logs":
        cmd_logs(args)
    elif args.command == "status":
        cmd_status(args)
    elif args.command == "orchestrate":
        cmd_orchestrate(args)
    else:
        parser.print_help()


if __name__ == "__main__":
    main()
