#!/bin/bash
# ====================================
# Docker 部署包构建脚本
# 将整个 LiteLLM 解决方案打包为可分发的 tar.gz
# 用法: ./build-package.sh [输出目录]
# ====================================
set -e
cd "$(dirname "$0")"

OUTPUT_DIR="${1:-./dist}"
PACKAGE_NAME="litellm-suite-$(date +%Y%m%d)"
PACKAGE_DIR="$OUTPUT_DIR/$PACKAGE_NAME"

echo "=========================================="
echo " Docker 部署包构建"
echo "=========================================="
echo ""

# 创建输出目录
mkdir -p "$PACKAGE_DIR"

# 需要打包的文件列表
FILES=(
  "docker-compose.yml"
  "config_admin.yaml"
  "config_worker.yaml"
  "custom_callbacks.py"
  "task_orchestrator.py"
  "litellm_cli.py"
  "install.sh"
  "offline-install.sh"
  "webui-init.sh"
  "quota_setup.sh"
  "redis.conf"
  "prometheus.yml"
  ".env.example"
  "README.md"
)

DIRS=(
  "webapp"
  "grafana"
)

echo "[1/3] 复制配置文件..."
for f in "${FILES[@]}"; do
  if [ -f "$f" ]; then
    cp "$f" "$PACKAGE_DIR/"
    echo "  ✓ $f"
  else
    echo "  ! 跳过（不存在）: $f"
  fi
done

echo ""
echo "[2/3] 复制目录..."
for d in "${DIRS[@]}"; do
  if [ -d "$d" ]; then
    cp -r "$d" "$PACKAGE_DIR/"
    echo "  ✓ $d/"
  else
    echo "  ! 跳过（不存在）: $d/"
  fi
done

echo ""
echo "[3/3] 生成镜像清单..."
# 提取 docker-compose.yml 中所有 image 引用
grep -E '^\s+image:' docker-compose.yml | sed 's/.*image:\s*//' | sort -u > "$PACKAGE_DIR/images.list"
echo "  ✓ images.list ($(wc -l < "$PACKAGE_DIR/images.list") 个镜像)"

# 创建空的 .env 文件（从 example 复制）
cp .env.example "$PACKAGE_DIR/.env" 2>/dev/null || true

# 打包
echo ""
echo "正在压缩..."
tar -czf "$OUTPUT_DIR/$PACKAGE_NAME.tar.gz" -C "$OUTPUT_DIR" "$PACKAGE_NAME"
rm -rf "$PACKAGE_DIR"

SIZE=$(du -h "$OUTPUT_DIR/$PACKAGE_NAME.tar.gz" | cut -f1)
echo ""
echo "=========================================="
echo " 打包完成"
echo "=========================================="
echo ""
echo "部署包: $OUTPUT_DIR/$PACKAGE_NAME.tar.gz ($SIZE)"
echo ""
echo "分发方式:"
echo "  1. 将 tar.gz 传到目标主机"
echo "  2. 解压: tar xzf $PACKAGE_NAME.tar.gz"
echo "  3. 运行: cd $PACKAGE_NAME && ./offline-install.sh"
echo ""
echo "离线镜像导出（可选）:"
echo "  ./save-images.sh  # 在有网环境导出镜像"
echo "  ./load-images.sh  # 在离线环境导入镜像"
echo ""
