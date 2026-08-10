#!/bin/bash
# ====================================
# 离线安装脚本 — 在目标主机上部署 LiteLLM 套件
# 用法: ./offline-install.sh
# ====================================
set -e
cd "$(dirname "$0")"

echo "=========================================="
echo " LiteLLM 套件 — 离线安装"
echo "=========================================="
echo ""

# 检查 Docker
if ! command -v docker >/dev/null 2>&1; then
  echo "✗ 需要 Docker，请先安装 Docker Desktop"
  exit 1
fi
echo "✓ Docker: $(docker --version)"

# 检查 Docker Compose
if ! docker compose version >/dev/null 2>&1; then
  echo "✗ 需要 Docker Compose"
  exit 1
fi
echo "✓ Docker Compose: $(docker compose version --short)"

# 检查 Python3
if ! command -v python3 >/dev/null 2>&1; then
  echo "✗ 需要 Python3"
  exit 1
fi
echo "✓ Python3: $(python3 --version)"
echo ""

# 导入离线镜像（如果存在）
if [ -f "images.tar" ]; then
  echo "[1/4] 导入离线镜像..."
  docker load -i images.tar
  echo "  ✓ 镜像导入完成"
else
  echo "[1/4] 未找到离线镜像包，将使用在线拉取"
fi

# 检查 .env 是否已配置
if [ ! -f ".env" ]; then
  echo "[2/4] 创建 .env 文件..."
  cp .env.example .env
  echo "  ✓ .env 已创建，请编辑填入 API Key"
else
  echo "[2/4] .env 已存在"
fi

# 检查是否已有 API Key 配置
if grep -q "DASHSCOPE_API_KEY=your" .env 2>/dev/null; then
  echo ""
  echo "  ⚠ .env 中的 API Key 仍为默认值"
  echo "  请先编辑 .env 填入真实的 API Key，或运行："
  echo "    python3 litellm_cli.py init"
  echo ""
  read -p "是否现在运行配置向导？(Y/n): " run_init
  if [ "$run_init" != "n" ]; then
    python3 litellm_cli.py init
  fi
fi

# 启动服务
echo ""
echo "[3/4] 启动 Docker 服务..."
docker compose up -d
echo "  ✓ 服务已启动"

# 等待健康检查
echo ""
echo "[4/4] 等待服务就绪..."
for i in $(seq 1 30); do
  if curl -sf http://localhost:4001/health/liveliness >/dev/null 2>&1; then
    echo "  ✓ Worker 已就绪"
    break
  fi
  sleep 2
  echo "  等待中... ($i/30)"
done

echo ""
echo "=========================================="
echo " 安装完成！"
echo "=========================================="
echo ""
echo "服务端口："
echo "  管理界面 (Admin):        http://localhost:4000"
echo "  推理 API (Worker):       http://localhost:4001"
echo "  聊天界面 (WebUI):        http://localhost:3001"
echo "  编排平台 (Orchestrator): http://localhost:3002"
echo "  Grafana 仪表盘:          http://localhost:3000"
echo ""
echo "CLI 命令："
echo "  python3 litellm_cli.py health        # 健康检查"
echo "  python3 litellm_cli.py status        # 运行状态"
echo "  python3 litellm_cli.py orchestrate '任务'  # 编排复杂任务"
echo ""
