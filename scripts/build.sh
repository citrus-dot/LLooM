#!/bin/bash
# LLooM 构建脚本
#
# 架构：Rust workspace（lloom-core + lloom-server + lloom-cli），
# TUI 为 SolidJS+OpenTUI (tui/)，Provider 调用由 Rust 原生实现。
# 构建产物：
#   - target/release/lloom-server   — 主服务器（REST + WebUI）
#   - target/release/lloom-cli      — 命令行界面
#
# TUI 运行（需 bun）：cd tui && bun install && bun run src/index.tsx
#
# 用法:
#   bash scripts/build.sh                   # 完整构建
#
# Ollama 不捆绑。服务器复用系统 Ollama（PATH 或 localhost:11434）；
# 缺失时 CLI / WebUI / TUI 会在用到本地模型时提示安装。

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"


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

echo ""
echo "[0/3] 系统依赖检测..."
echo "----------------------------------------"
check_common
if [ "$FAILED_DEPS" -gt 0 ]; then
    echo ""
    echo "✗ 检测到 $FAILED_DEPS 项缺失依赖，请先安装后再构建。"
    exit 1
fi
echo "✓ 系统依赖检查通过"
echo ""

# 1. 构建 React WebUI
echo "[1/3] 构建 React WebUI..."
echo "----------------------------------------"
if [ ! -d webui/node_modules ]; then
    echo "安装 npm 依赖..."
    (cd webui && npm install) 2>&1 || { echo "✗ npm install 失败！"; exit 1; }
fi
(cd webui && npm run build) 2>&1 || {
    echo "✗ React 构建失败！"
    exit 1
}
echo "✓ React 构建完成: webui/dist/"

# 2. 编译 Rust workspace
echo "[2/3] 编译 Rust workspace (release)..."
echo "----------------------------------------"
cargo build --workspace --release 2>&1 || {
    echo "✗ Rust 编译失败！"
    exit 1
}
echo "✓ Rust 编译完成:"
echo "    target/release/lloom-server"
echo "    target/release/lloom-cli"

echo ""
echo "============================================"
echo "✓ 构建完成！"
echo "============================================"
echo ""
echo "产物位置:"
echo "  服务器:  target/release/lloom-server"
echo "  CLI:     target/release/lloom-cli"
echo ""
echo "运行方式:"
echo "  服务器:  target/release/lloom-server      (WebUI: http://localhost:7861)"
echo "  CLI:     target/release/lloom-cli --help"
echo "  TUI:     cd tui && bun install && bun run src/index.tsx"
echo "  开发:    cargo run -p lloom-server"
