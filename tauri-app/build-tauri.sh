#!/bin/bash
# ====================================
# Tauri 桌面应用构建脚本
# 用法: ./build-tauri.sh
# ====================================
set -e
cd "$(dirname "$0")"

echo "=========================================="
echo " Tauri 桌面应用构建"
echo "=========================================="
echo ""

# 检查 Node.js
if ! command -v node >/dev/null 2>&1; then
  echo "✗ 需要 Node.js (v18+)，请先安装"
  exit 1
fi
echo "✓ Node.js: $(node --version)"

# 检查 Rust
if ! command -v rustc >/dev/null 2>&1; then
  echo ""
  echo "✗ 需要 Rust 工具链，正在安装..."
  echo ""
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source "$HOME/.cargo/env"
  echo "✓ Rust: $(rustc --version)"
else
  echo "✓ Rust: $(rustc --version)"
fi

# 检查 Cargo
if ! command -v cargo >/dev/null 2>&1; then
  echo "✗ 需要 Cargo（Rust 包管理器）"
  source "$HOME/.cargo/env" 2>/dev/null || true
fi
echo "✓ Cargo: $(cargo --version)"
echo ""

# 安装 Tauri CLI
echo "[1/3] 安装 Tauri CLI..."
npm install
echo "  ✓ Tauri CLI 已安装"

# 开发模式或构建模式
if [ "$1" == "--dev" ]; then
  echo ""
  echo "[2/3] 启动开发模式..."
  echo "  注意：请确保 Docker 服务已启动 (docker compose up -d)"
  echo ""
  npx tauri dev
else
  echo ""
  echo "[2/3] 构建桌面应用..."
  npx tauri build

  echo ""
  echo "[3/3] 构建完成！"
  echo ""
  echo "产物位置："
  echo "  macOS: src-tauri/target/release/bundle/"
  echo "  - .app  (macOS 应用包)"
  echo "  - .dmg  (macOS 安装包)"
  echo ""
  echo "运行方式："
  echo "  双击 .app 文件，或拖入 Applications 文件夹"
  echo ""
fi
