#!/usr/bin/env python3
"""首次运行设置 — 拉取默认 Ollama 模型并初始化数据库。

在 Tauri 应用首次启动时由 main.rs 调用。
完成以下步骤：
1. 初始化 SQLite 数据库 + 种子模型数据
2. 检查 Ollama 可用性
3. 拉取默认本地模型 (qwen2.5:latest)
"""

import os
import sys
import subprocess
import time


def check_ollama(ollama_path: str = "ollama") -> bool:
    """检查 Ollama 是否可用"""
    try:
        result = subprocess.run(
            [ollama_path, "list"],
            capture_output=True, text=True, timeout=10,
            env={**os.environ, "PATH": os.environ.get("PATH", "")},
        )
        return result.returncode == 0
    except Exception:
        return False


def pull_model(ollama_path: str, model: str) -> bool:
    """拉取 Ollama 模型"""
    print(f"  拉取模型 {model}...")
    try:
        proc = subprocess.run(
            [ollama_path, "pull", model],
            capture_output=True, text=True, timeout=600,
        )
        if proc.returncode == 0:
            print(f"  ✓ 模型 {model} 拉取成功")
            return True
        else:
            print(f"  ✗ 拉取失败: {proc.stderr[:200]}")
            return False
    except subprocess.TimeoutExpired:
        print(f"  ✗ 拉取超时 (10分钟)")
        return False
    except Exception as e:
        print(f"  ✗ 拉取异常: {e}")
        return False


def main():
    base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    os.chdir(base_dir)
    sys.path.insert(0, base_dir)

    print("=== LLooM v2 首次运行设置 ===")

    # 1. 初始化数据库
    print("\n[1/3] 初始化数据库...")
    from core.database import init_db
    from core.seed_models import seed_models
    init_db()
    seed_models()
    print("  ✓ 数据库就绪")

    # 2. 检查 Ollama
    print("\n[2/3] 检查 Ollama...")
    ollama_path = "ollama"
    # 优先使用内置的 Ollama 二进制
    bundled = os.path.join(base_dir, "tauri-app", "src-tauri", "resources", "ollama")
    if os.path.exists(bundled) and os.access(bundled, os.X_OK):
        ollama_path = bundled
        os.environ["OLLAMA_API_BASE"] = "http://localhost:11434"

    if check_ollama(ollama_path):
        print("  ✓ Ollama 可用")
    else:
        print("  ⚠ Ollama 不可用，跳过模型拉取")
        print("\n首次运行设置完成（部分功能受限）")
        return

    # 3. 拉取默认模型
    print("\n[3/3] 检查默认模型...")
    try:
        result = subprocess.run(
            [ollama_path, "list"],
            capture_output=True, text=True, timeout=10,
        )
        installed = result.stdout
    except Exception:
        installed = ""

    if "qwen2.5:latest" in installed:
        print("  ✓ qwen2.5:latest 已安装")
    else:
        pull_model(ollama_path, "qwen2.5:latest")

    print("\n✓ 首次运行设置完成")


if __name__ == "__main__":
    main()
