#!/bin/bash
# LLooM 停止脚本（便携包）— 优先走 /api/shutdown 优雅关闭（服务端会一并
# 清理它拉起的 Ollama），HTTP 不可达或超时才兜底杀进程。
set -uo pipefail

cd "$(dirname "$0")"

# 端口与服务器同源：.env 的 LLOOM_WEB_PORT，缺省 7861（.env 不存在时 grep
# 返回非零是预期，吞掉以免中断脚本）。
PORT="$(grep -E '^LLOOM_WEB_PORT=' .env 2>/dev/null | tail -1 | cut -d= -f2 | tr -d '[:space:]' || true)"
PORT="${PORT:-7861}"

# URL 不带尾斜杠：下面要拼 /api/shutdown 与 /api/health，双斜杠路径 axum 不认。
URL="http://localhost:${PORT}"

if ! curl -sf -m 2 -o /dev/null "$URL/api/health"; then
  echo "LLooM 未在运行 (nothing at $URL)"
  exit 0
fi

echo "Requesting graceful shutdown ..."
curl -s -m 5 -X POST "$URL/api/shutdown" >/dev/null 2>&1 || true

# 优雅关闭路径最多等 10s（服务端 shutdown 后端口随之释放）。
for _ in $(seq 1 20); do
  if ! curl -sf -m 1 -o /dev/null "$URL/api/health"; then
    echo "✓ LLooM stopped"
    exit 0
  fi
  sleep 0.5
done

# 兜底：端口还活着说明优雅关闭失败，按名杀进程（Windows 上由 stop.bat 负责）。
echo "Graceful shutdown timed out, killing by name ..."
if command -v pkill >/dev/null 2>&1; then
  pkill -f "lloom-server" 2>/dev/null || true
fi
for _ in $(seq 1 10); do
  if ! curl -sf -m 1 -o /dev/null "$URL/api/health"; then
    echo "✓ LLooM stopped (forced)"
    exit 0
  fi
  sleep 0.5
done
echo "✗ 仍无法停止，请手动结束 lloom-server 进程" >&2
exit 1
