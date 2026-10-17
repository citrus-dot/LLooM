#!/bin/bash
# LLooM 启动脚本（便携包）— 前台运行，Ctrl+C 退出（服务端会清理子进程）。
# 端口来源与服务器一致：.env 的 LLOOM_WEB_PORT，缺省 7861。
set -euo pipefail

cd "$(dirname "$0")"

# 端口与服务器同源：.env 的 LLOOM_WEB_PORT，缺省 7861。注意 grep 在
# .env 不存在（用户还没复制 .env.example）时返回非零，不能让 set -e 杀脚本。
PORT="$(grep -E '^LLOOM_WEB_PORT=' .env 2>/dev/null | tail -1 | cut -d= -f2 | tr -d '[:space:]' || true)"
PORT="${PORT:-7861}"
# URL 不带尾斜杠（与 stop.sh 一致；后续统一拼 /api/health 等路径）。
URL="http://localhost:${PORT}"

# Intel Mac（x86_64）的 ONNX Runtime 走运行期 dlopen：找不到 libonnxruntime.dylib
# 时先明确提示安装（brew），不装也能启动——仅 L2 语义缓存降级。
if [ "$(uname -m)" = "x86_64" ] && [ "$(uname -s)" = "Darwin" ]; then
  if ! ls /opt/homebrew/lib/libonnxruntime.dylib /usr/local/lib/libonnxruntime.dylib \
         "${LLOOM_ORT_DYLIB_PATH:-}" >/dev/null 2>&1; then
    echo "──────────────────────────────────────────────"
    echo "提示：未找到 ONNX Runtime（libonnxruntime.dylib）"
    echo "  L2 语义缓存将不可用；L1 精确缓存与模型路由不受影响。"
    echo "  启用 L2 请安装：brew install onnxruntime"
    echo "──────────────────────────────────────────────"
  fi
fi

# 已有实例在跑就直接开浏览器，不重复启动（幂等，stop.sh 可关闭）。
if curl -sf -m 2 -o /dev/null "$URL/api/health"; then
  echo "✓ LLooM 已在运行: $URL"
  command -v lloom-server >/dev/null 2>&1 && exec lloom-server --open || true
  exit 0
fi

echo "Starting LLooM on $URL ..."
exec ./lloom-server --open
