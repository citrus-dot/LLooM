#!/bin/bash
# LLooM v2 完整构建脚本
# 用法: bash scripts/build.sh [--skip-pyinstaller] [--skip-ollama] [--skip-tauri]

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

SKIP_PYINSTALLER=false
SKIP_OLLAMA=false
SKIP_TAURI=false

for arg in "$@"; do
    case $arg in
        --skip-pyinstaller) SKIP_PYINSTALLER=true ;;
        --skip-ollama) SKIP_OLLAMA=true ;;
        --skip-tauri) SKIP_TAURI=true ;;
    esac
done

echo "============================================"
echo "LLooM v2 构建脚本"
echo "============================================"

# 1. PyInstaller 打包 Python 核心
if [ "$SKIP_PYINSTALLER" = false ]; then
    echo "\n[1/3] PyInstaller 打包..."
    echo "----------------------------------------"
    pyinstaller lloom.spec --noconfirm --clean 2>&1 || {
        echo "PyInstaller 打包失败！"
        exit 1
    }
    echo "✓ PyInstaller 打包完成: dist/lloom-server/"

    # 复制到 Tauri resources 目录
    echo "复制到 Tauri resources..."
    rm -rf tauri-app/src-tauri/resources/lloom-server
    cp -R dist/lloom-server tauri-app/src-tauri/resources/lloom-server
    echo "✓ PyInstaller 输出已复制到 Tauri resources"
else
    echo "\n[1/3] 跳过 PyInstaller"
fi

# 2. 下载 Ollama 二进制
if [ "$SKIP_OLLAMA" = false ]; then
    echo "\n[2/3] 检查 Ollama 二进制..."
    echo "----------------------------------------"
    OLLAMA_PATH="tauri-app/src-tauri/resources/ollama"
    if [ -f "$OLLAMA_PATH" ] && [ -x "$OLLAMA_PATH" ]; then
        echo "✓ Ollama 二进制已存在"
    else
        echo "下载 Ollama..."
        bash scripts/download_ollama.sh || {
            echo "⚠ Ollama 下载失败，将使用系统安装的 Ollama"
        }
    fi
else
    echo "\n[2/3] 跳过 Ollama 下载"
fi

# 3. Tauri 构建
if [ "$SKIP_TAURI" = false ]; then
    echo "\n[3/3] Tauri 构建..."
    echo "----------------------------------------"
    cd tauri-app

    # 安装 npm 依赖
    if [ ! -d node_modules ]; then
        echo "安装 npm 依赖..."
        npm install 2>&1 || {
            echo "npm install 失败！"
            exit 1
        }
    fi

    # 构建
    echo "构建 Tauri 应用..."
    npx tauri build --bundles app 2>&1 || {
        echo "Tauri 构建失败！"
        echo "提示: 确保已安装 Rust 工具链和 Xcode 命令行工具"
        exit 1
    }

    echo "✓ Tauri 构建完成"
    echo "输出: tauri-app/src-tauri/target/release/bundle/macos/LLooM.app"
else
    echo "\n[3/3] 跳过 Tauri 构建"
fi

echo "\n============================================"
echo "✓ 构建完成！"
echo "============================================"
echo ""
echo "产物位置:"
echo "  Python 核心: dist/lloom-server/"
echo "  Tauri 应用: tauri-app/src-tauri/target/release/bundle/"
echo ""
echo "运行方式:"
echo "  Python API: ./dist/lloom-server/lloom-server"
echo "  Tauri 应用: 打开 tauri-app/src-tauri/target/release/bundle/macos/LLooM.app"
