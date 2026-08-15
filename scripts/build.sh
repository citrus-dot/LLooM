#!/bin/bash
# LLooM 构建脚本
#
# 架构：Rust 是主服务器（axum），Python 只剩一个 AI 微服务（litellm 封装）。
# 构建产物：
#   - dist/ai-service/ai-service — PyInstaller 打包的 AI 微服务可执行
#   - tauri-app/.../bundle/       — Tauri 打包的桌面应用（含 Rust 核心 + AI 服务 + Ollama）
#
# 用法:
#   bash scripts/build.sh                  # 完整构建
#   bash scripts/build.sh --skip-ai        # 跳过 AI 微服务打包
#   bash scripts/build.sh --skip-ollama    # 跳过 Ollama 下载
#   bash scripts/build.sh --skip-tauri     # 跳过 Tauri 打包（只做 Rust 编译 + AI 打包）

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

SKIP_AI=false
SKIP_OLLAMA=false
SKIP_TAURI=false

for arg in "$@"; do
    case $arg in
        --skip-ai) SKIP_AI=true ;;
        --skip-ollama) SKIP_OLLAMA=true ;;
        --skip-tauri) SKIP_TAURI=true ;;
    esac
done

echo "============================================"
echo "LLooM 构建脚本"
echo "============================================"

# ── 系统依赖检测 ──

FAILED_DEPS=0

require_cmd() {
    local name="$1"
    shift
    if ! command -v "$name" >/dev/null 2>&1; then
        echo "  ✗ 缺少命令: $name"
        FAILED_DEPS=$((FAILED_DEPS+1))
        return 1
    fi
}

check_common() {
    echo "检查通用依赖..."
    require_cmd cargo || echo "    → 安装 Rust: https://rustup.rs"
    require_cmd node || echo "    → 安装 Node.js: https://nodejs.org"
    require_cmd npm || echo "    → npm 随 Node.js 安装"
}

check_python() {
    echo "检查 Python 依赖..."
    # 自动探测：优先项目 .venv，其次显式 PYTHON_BIN，最后系统 python3
    local py="${PYTHON_BIN:-}"
    if [ -z "$py" ]; then
        for cand in ".venv/bin/python" "venv/bin/python"; do
            if [ -x "$cand" ]; then
                py="$cand"
                break
            fi
        done
    fi
    if [ -z "$py" ]; then
        py="python3"
    fi
    PYTHON_BIN="$py"
    echo "  → 使用 Python: $py"
    if ! command -v "$py" >/dev/null 2>&1; then
        echo "  ✗ 缺少命令: $py"
        FAILED_DEPS=$((FAILED_DEPS+1))
        return 1
    fi
    # 检查 AI 服务所需模块
    if ! "$py" -c "import litellm, fastapi, uvicorn, pydantic" 2>/dev/null; then
        echo "  ✗ Python 缺少 AI 服务依赖 (litellm/fastapi/uvicorn/pydantic)"
        echo "    → 运行: pip install -e '.[dev]'"
        FAILED_DEPS=$((FAILED_DEPS+1))
    fi
    if [ "$SKIP_AI" = false ]; then
        if ! "$py" -c "import PyInstaller" 2>/dev/null; then
            echo "  ✗ Python 缺少 PyInstaller"
            echo "    → 运行: pip install pyinstaller"
            FAILED_DEPS=$((FAILED_DEPS+1))
        fi
    fi
}

check_linux_tauri() {
    echo "检查 Linux Tauri 系统依赖..."
    # 通过 pkg-config 检查 webkit2gtk 4.1 及关联库
    for pkg in webkit2gtk-4.1 gtk+-3.0 libsoup-3.0 javascriptcoregtk-4.1; do
        if ! pkg-config --exists "$pkg" 2>/dev/null; then
            echo "  ✗ 缺少 pkg-config 库: $pkg"
            FAILED_DEPS=$((FAILED_DEPS+1))
        fi
    done
    if [ "$FAILED_DEPS" -gt 0 ]; then
        echo "    → Linux 构建 WebView 界面需要以下系统包:"
        echo "      Arch:   sudo pacman -S webkit2gtk-4.1 gtk3 libsoup3 javascriptcoregtk4.1"
        echo "      Debian: sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev"
        echo "      Fedora: sudo dnf install webkit2gtk4.1-devel gtk3-devel libsoup3-devel javascriptcoregtk4.1-devel"
    fi
}

check_macos() {
    echo "检查 macOS 依赖..."
    if ! xcode-select -p >/dev/null 2>&1; then
        echo "  ✗ 缺少 Xcode 命令行工具"
        echo "    → 运行: xcode-select --install"
        FAILED_DEPS=$((FAILED_DEPS+1))
    fi
}

echo ""
echo "[0/3] 系统依赖检测..."
echo "----------------------------------------"
check_common
check_python
if [ "$SKIP_TAURI" = false ]; then
    if [ "$(uname)" = "Darwin" ]; then
        check_macos
    elif [ "$(uname)" = "Linux" ]; then
        check_linux_tauri
    fi
fi
if [ "$FAILED_DEPS" -gt 0 ]; then
    echo ""
    echo "✗ 检测到 $FAILED_DEPS 项缺失依赖，请先安装后再构建。"
    exit 1
fi
echo "✓ 系统依赖检查通过"
echo ""

# 1. 打包 Python AI 微服务（litellm 封装）
if [ "$SKIP_AI" = false ]; then
    echo "[1/3] 打包 Python AI 微服务 (PyInstaller)..."
    echo "----------------------------------------"
    $PYTHON_BIN -m PyInstaller ai_service.spec --noconfirm --clean 2>&1 || {
        echo "✗ AI 服务打包失败！"
        exit 1
    }
    echo "✓ AI 服务打包完成: dist/ai-service/ai-service"

    echo "复制到 Tauri resources..."
    rm -rf tauri-app/src-tauri/resources/ai-service
    cp -R dist/ai-service tauri-app/src-tauri/resources/ai-service
    echo "✓ AI 服务已复制到 resources"
else
    echo "[1/3] 跳过 AI 服务打包"
fi

# 2. Ollama 二进制（可选内置）
if [ "$SKIP_OLLAMA" = false ]; then
    echo "[2/3] 检查 Ollama 二进制..."
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
    echo "[2/3] 跳过 Ollama 下载"
fi

# 3. Tauri 构建
if [ "$SKIP_TAURI" = false ]; then
    echo "[3/3] Tauri 构建..."
    echo "----------------------------------------"
    cd tauri-app

    if [ ! -d node_modules ]; then
        echo "安装 npm 依赖..."
        npm install 2>&1 || { echo "✗ npm install 失败！"; exit 1; }
    fi

    echo "构建 Tauri 应用..."
    # 目标平台自动选择 bundle 格式（Linux: deb/rpm/AppImage，macOS: app/dmg）
    npx tauri build 2>&1 || {
        echo "✗ Tauri 构建失败！"
        echo "提示: 确保已安装 Rust 工具链（Linux 还需 webkit2gtk/gtk 等系统依赖）"
        exit 1
    }

    echo "✓ Tauri 构建完成"
else
    echo "[3/3] 跳过 Tauri 打包"
fi

echo ""
echo "============================================"
echo "✓ 构建完成！"
echo "============================================"
echo ""
echo "产物位置:"
echo "  AI 微服务:  dist/ai-service/ai-service"
echo "  Tauri 应用: tauri-app/src-tauri/target/release/bundle/"
echo ""
echo "运行方式:"
echo "  Rust 核心 (headless): cd tauri-app/src-tauri && cargo run -- --headless"
echo "  Tauri 应用:           打开 tauri-app/src-tauri/target/release/bundle/ 下的安装包"
