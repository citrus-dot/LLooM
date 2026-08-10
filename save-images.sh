#!/bin/bash
# ====================================
# Docker 镜像导出脚本 — 在有网环境中导出所有镜像
# 用法: ./save-images.sh [输出文件]
# ====================================
set -e
cd "$(dirname "$0")"

OUTPUT="${1:-images.tar}"
IMAGES_LIST="images.list"

if [ ! -f "$IMAGES_LIST" ]; then
  echo "✗ 未找到 $IMAGES_LIST，请先运行 build-package.sh"
  exit 1
fi

echo "=========================================="
echo " Docker 镜像导出"
echo "=========================================="
echo ""

IMAGES=()
while IFS= read -r line; do
  [ -z "$line" ] && continue
  IMAGES+=("$line")
done < "$IMAGES_LIST"

echo "共 ${#IMAGES[@]} 个镜像需要导出："
for img in "${IMAGES[@]}"; do
  echo "  - $img"
done
echo ""

# 拉取镜像（确保本地存在）
echo "[1/2] 拉取镜像..."
for img in "${IMAGES[@]}"; do
  echo -n "  拉取 $img ... "
  if docker pull "$img" 2>/dev/null; then
    echo "✓"
  else
    echo "已存在或跳过"
  fi
done

# 导出镜像
echo ""
echo "[2/2] 导出到 $OUTPUT ..."
docker save -o "$OUTPUT" "${IMAGES[@]}"

SIZE=$(du -h "$OUTPUT" | cut -f1)
echo ""
echo "=========================================="
echo " 导出完成"
echo "=========================================="
echo "文件: $OUTPUT ($SIZE)"
echo ""
echo "将此文件与部署包一起分发，在目标主机运行："
echo "  docker load -i $OUTPUT"
echo ""
