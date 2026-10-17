#!/bin/bash
# LLooM 安装包组装脚本（Linux / macOS）
#
# 前置：以下产物已就绪（CI 或手动执行 build 步骤）：
#   target/release/lloom-server          主服务器
#   target/release/lloom-cli             CLI（可选，缺失则跳过）
#   webui/dist/                          前端静态文件（npm run build）
#
# 产出：dist/LLooM-<version>-<os>-<arch>.tar.gz，目录布局与
# config::install_dir / ui_dir / processes::start_ai 的查找逻辑一一对应：
#   LLooM/
#   ├── lloom-server
#   ├── lloom-cli
#   ├── resources/webui/dist/
#   ├── start.sh / stop.sh               ← packaging/（start 带 --open 自动开浏览器）
#   └── .env.example
#
# 用法: bash scripts/package.sh [--target x86_64-apple-darwin]
#
# --target：从 Rust 交叉编译输出目录（target/<triple>/release）取二进制，
#           产物名仍按 uname 目标平台命名（CI 里 arm64 runner 交叉产 Intel 包用）。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

# ── 解析 --target ──
RUST_TARGET=""
if [ "${1:-}" = "--target" ]; then
  RUST_TARGET="${2:-}"
  [ -n "$RUST_TARGET" ] || { echo "✗ --target 缺少参数" >&2; exit 1; }
fi
if [ -n "$RUST_TARGET" ]; then
  TARGET_DIR="target/$RUST_TARGET/release"
else
  TARGET_DIR="target/release"
fi

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"          # linux / darwin
# 架构按 Rust target 定（交叉编译时 uname -m 是宿主而非产物架构）。
if [ -n "$RUST_TARGET" ]; then
  case "$RUST_TARGET" in
    *x86_64*) ARCH="x86_64" ;;
    *aarch64*|*arm64*) ARCH="arm64" ;;
    *) echo "✗ 未知 target 架构: $RUST_TARGET" >&2; exit 1 ;;
  esac
else
  ARCH="$(uname -m)"                                    # x86_64 / arm64 / aarch64
  case "$ARCH" in aarch64) ARCH=arm64 ;; esac
fi
VERSION="$(git describe --tags --always 2>/dev/null || echo dev)"
STAGE="dist/pkg/LLooM"
OUT="dist/LLooM-${VERSION}-${OS}-${ARCH}.tar.gz"

for f in "$TARGET_DIR/lloom-server" "webui/dist/index.html" "packaging/start.sh" "packaging/stop.sh"; do
  if [ ! -e "$f" ]; then
    echo "✗ 缺少构建产物: $f （先跑 build.sh 或 CI 对应步骤）" >&2
    exit 1
  fi
done

rm -rf "$STAGE"
mkdir -p "$STAGE/resources"

cp "$TARGET_DIR/lloom-server" "$STAGE/"
[ -f "$TARGET_DIR/lloom-cli" ] && cp "$TARGET_DIR/lloom-cli" "$STAGE/"
mkdir -p "$STAGE/resources/webui"
cp -r webui/dist "$STAGE/resources/webui/dist"
cp .env.example "$STAGE/"

# 便携包启动/停止脚本（含 --open 自动开浏览器；stop 走 /api/shutdown 优雅关闭）
install -m 755 packaging/start.sh "$STAGE/start.sh"
install -m 755 packaging/stop.sh "$STAGE/stop.sh"
chmod +x "$STAGE/lloom-server"
[ -f "$STAGE/lloom-cli" ] && chmod +x "$STAGE/lloom-cli"

mkdir -p dist/pkg
tar -czf "$OUT" -C dist/pkg LLooM
rm -rf "$STAGE"

echo "✓ 安装包: $OUT ($(du -h "$OUT" | cut -f1))"
