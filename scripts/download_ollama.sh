#!/bin/bash
# 下载 Ollama 二进制（跨平台：macOS / Linux x86_64）
# 用法: bash scripts/download_ollama.sh
#
# 产物:
#   macOS: tauri-app/src-tauri/resources/ollama        （单文件，macOS arm64/amd64）
#   Linux: tauri-app/src-tauri/resources/ollama        （可执行）
#          tauri-app/src-tauri/resources/lib/ollama     （运行时库，仅 Linux）

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
RESOURCES_DIR="$PROJECT_DIR/dist/ollama"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

mkdir -p "$RESOURCES_DIR"

OS="$(uname -s)"
ARCH="$(uname -m)"
API="https://api.github.com/repos/ollama/ollama/releases/latest"

# 解析最新版本号
VERSION="$(curl -sL "$API" | grep -m1 '"tag_name"' | sed 's/.*"tag_name": "\([^"]*\)".*/\1/')"
if [ -z "$VERSION" ]; then
    echo "✗ 无法获取 Ollama 最新版本号（可能网络不通）"
    exit 1
fi
echo "Ollama 最新版本: $VERSION"

download_and_verify() {
    # $1 = URL, $2 = 输出文件, $3 = 最小字节数
    local url="$1" out="$2" min="${3:-1048576}"
    echo "下载 $url ..."
    curl -L -o "$out" "$url"
    local size
    size="$(stat -c%s "$out" 2>/dev/null || stat -f%z "$out" 2>/dev/null || echo 0)"
    if [ "$size" -lt "$min" ]; then
        echo "⚠ 下载失败（仅 ${size} 字节），尝试使用系统安装的 Ollama..."
        return 1
    fi
    return 0
}

fallback_system() {
    local dest="$1"
    local sys
    sys="$(command -v ollama 2>/dev/null || true)"
    if [ -n "$sys" ]; then
        cp "$sys" "$dest"
        chmod +x "$dest"
        echo "✓ 已从系统复制 Ollama: $($dest --version 2>/dev/null || echo 未知)"
        return 0
    fi
    echo "✗ 无法获取 Ollama 二进制，请手动安装 Ollama"
    return 1
}

if [ "$OS" = "Darwin" ]; then
    # macOS: ollama-darwin.tgz（通用包，含 bin/ollama）
    URL="https://github.com/ollama/ollama/releases/download/$VERSION/ollama-darwin.tgz"
    if download_and_verify "$URL" "$TMP_DIR/ollama.tgz"; then
        tar -xzf "$TMP_DIR/ollama.tgz" -C "$TMP_DIR"
        if [ -f "$TMP_DIR/bin/ollama" ]; then
            cp "$TMP_DIR/bin/ollama" "$RESOURCES_DIR/ollama"
            chmod +x "$RESOURCES_DIR/ollama"
            echo "✓ Ollama 已保存到: $RESOURCES_DIR/ollama"
        else
            echo "✗ tgz 中未找到 bin/ollama，回退到系统 Ollama"
            fallback_system "$RESOURCES_DIR/ollama"
        fi
    else
        fallback_system "$RESOURCES_DIR/ollama"
    fi

elif [ "$OS" = "Linux" ]; then
    if [ "$ARCH" != "x86_64" ]; then
        echo "⚠ 仅支持 x86_64（当前 $ARCH），尝试使用系统安装的 Ollama..."
        fallback_system "$RESOURCES_DIR/ollama" || exit 1
        exit 0
    fi
    # Linux: ollama-linux-amd64.tar.zst（含 bin/ollama + lib/ollama）
    URL="https://github.com/ollama/ollama/releases/download/$VERSION/ollama-linux-amd64.tar.zst"
    echo "⚠ 注意: Linux 版 Ollama tar 含 GPU 库（约 2GB），这里只取 bin/ollama（CPU 模式够用）"
    if download_and_verify "$URL" "$TMP_DIR/ollama.tar.zst"; then
        # 只解出 bin/ollama，跳过 lib/ollama（GPU 库 2GB，CPU 模式不需要）
        tar --zstd -xf "$TMP_DIR/ollama.tar.zst" -C "$TMP_DIR" bin/ollama
        if [ -f "$TMP_DIR/bin/ollama" ]; then
            cp "$TMP_DIR/bin/ollama" "$RESOURCES_DIR/ollama"
            chmod +x "$RESOURCES_DIR/ollama"
            echo "✓ Ollama 已保存到: $RESOURCES_DIR/ollama"
        else
            echo "✗ tar.zst 中未找到 bin/ollama，回退到系统 Ollama"
            fallback_system "$RESOURCES_DIR/ollama"
        fi
    else
        fallback_system "$RESOURCES_DIR/ollama"
    fi

else
    echo "✗ 不支持的平台: $OS"
    echo "   请手动安装 Ollama 并确保其在 PATH 中"
    exit 1
fi
