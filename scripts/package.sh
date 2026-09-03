#!/bin/bash
# LLooM 安装包组装脚本（Linux / macOS）
#
# 前置：以下产物已就绪（CI 或手动执行 build 步骤）：
#   target/release/lloom-server          主服务器
#   target/release/lloom-cli             CLI（可选，缺失则跳过）
#   webui/dist/                          前端静态文件（npm run build）
#   dist/ai-service/                     PyInstaller onedir 产物（入口 + _internal/）
#
# 产出：dist/LLooM-<version>-<os>-<arch>.tar.gz，目录布局与
# config::install_dir / ui_dir / processes::start_ai 的查找逻辑一一对应：
#   LLooM/
#   ├── lloom-server
#   ├── lloom-cli
#   ├── resources/ai-service/ai-service (+ _internal/)
#   ├── resources/webui/dist/
#   ├── start.sh
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

for f in "target/release/lloom-server" "webui/dist/index.html" "dist/ai-service"; do
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
mkdir -p "$STAGE/resources/ai-service"
cp -r dist/ai-service/. "$STAGE/resources/ai-service/"
mkdir -p "$STAGE/scripts"
cp scripts/aiq_replay.py "$STAGE/scripts/"   # N2 路由体检 job 按 install_dir/scripts 查找
cp .env.example "$STAGE/"

cat > "$STAGE/start.sh" <<'EOF'
#!/bin/bash
cd "$(dirname "$0")"
exec ./lloom-server
EOF
chmod +x "$STAGE/start.sh" "$STAGE/lloom-server"
[ -f "$STAGE/lloom-cli" ] && chmod +x "$STAGE/lloom-cli"

mkdir -p dist/pkg
tar -czf "$OUT" -C dist/pkg LLooM
rm -rf "$STAGE"

echo "✓ 安装包: $OUT ($(du -h "$OUT" | cut -f1))"
