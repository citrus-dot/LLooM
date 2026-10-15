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
# 用法: bash scripts/package.sh          # 组装并压缩
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"          # linux / darwin
ARCH="$(uname -m)"                                      # x86_64 / arm64 / aarch64
case "$ARCH" in aarch64) ARCH=arm64 ;; esac
VERSION="$(git describe --tags --always 2>/dev/null || echo dev)"
STAGE="dist/pkg/LLooM"
OUT="dist/LLooM-${VERSION}-${OS}-${ARCH}.tar.gz"

for f in "target/release/lloom-server" "webui/dist/index.html" "packaging/start.sh" "packaging/stop.sh"; do
  if [ ! -e "$f" ]; then
    echo "✗ 缺少构建产物: $f （先跑 build.sh 或 CI 对应步骤）" >&2
    exit 1
  fi
done

rm -rf "$STAGE"
mkdir -p "$STAGE/resources"

cp target/release/lloom-server "$STAGE/"
[ -f target/release/lloom-cli ] && cp target/release/lloom-cli "$STAGE/"
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
