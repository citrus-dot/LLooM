#!/bin/bash
# ====================================
# Docker 镜像导入脚本 — 在离线环境中导入所有镜像
# 用法: ./load-images.sh [镜像文件]
# ====================================
set -e
cd "$(dirname "$0")"

INPUT="${1:-images.tar}"

if [ ! -f "$INPUT" ]; then
  echo "✗ 未找到镜像文件: $INPUT"
  exit 1
fi

echo "=========================================="
echo " Docker 镜像导入"
echo "=========================================="
echo ""

echo "正在导入镜像: $INPUT"
docker load -i "$INPUT"

echo ""
echo "=========================================="
echo " 导入完成"
echo "=========================================="
echo ""
echo "已导入的镜像："
docker images --format "  {{.Repository}}:{{.Tag}} ({{.Size}})" | head -20
echo ""
echo "下一步：运行 ./offline-install.sh 启动服务"
echo ""
