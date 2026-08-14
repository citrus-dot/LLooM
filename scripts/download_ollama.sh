#!/bin/bash
# 下载 Ollama macOS ARM64 二进制文件
# 用法: bash scripts/download_ollama.sh

set -e

ARCH=$(uname -m)
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
RESOURCES_DIR="$PROJECT_DIR/tauri-app/src-tauri/resources"

mkdir -p "$RESOURCES_DIR"

if [ "$ARCH" = "arm64" ]; then
    OLLAMA_URL="https://github.com/ollama/ollama/releases/latest/download/ollama-darwin-arm64"
elif [ "$ARCH" = "x86_64" ]; then
    OLLAMA_URL="https://github.com/ollama/ollama/releases/latest/download/ollama-darwin-amd64"
else
    echo "不支持的架构: $ARCH"
    exit 1
fi

OUTPUT="$RESOURCES_DIR/ollama"

echo "下载 Ollama ($ARCH)..."
curl -L -o "$OUTPUT" "$OLLAMA_URL" 2>&1
chmod +x "$OUTPUT"

# Check if download was valid (should be >1MB)
FILE_SIZE=$(stat -f%z "$OUTPUT" 2>/dev/null || echo 0)
if [ "$FILE_SIZE" -lt 1048576 ]; then
    echo "⚠ 下载失败 (文件仅 ${FILE_SIZE} 字节)，尝试使用系统安装的 Ollama..."
    SYS_OLLAMA=$(which ollama 2>/dev/null)
    if [ -n "$SYS_OLLAMA" ]; then
        cp "$SYS_OLLAMA" "$OUTPUT"
        chmod +x "$OUTPUT"
        echo "✓ 已从系统复制 Ollama: $($OUTPUT --version 2>/dev/null || echo '未知')"
    else
        echo "✗ 无法获取 Ollama 二进制，请手动安装 Ollama"
        rm -f "$OUTPUT"
        exit 1
    fi
else
    echo "Ollama 已保存到: $OUTPUT"
    echo "版本: $($OUTPUT --version 2>/dev/null || echo '未知')"
fi
