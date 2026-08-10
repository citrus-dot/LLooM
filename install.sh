#!/bin/bash
# ====================================
# LiteLLM 一键安装脚本
# 用法: ./install.sh
# ====================================
set -e
cd "$(dirname "$0")"

echo "=========================================="
echo " LiteLLM 一键安装"
echo "=========================================="
echo ""

# 检查 Python3
if ! command -v python3 >/dev/null 2>&1; then
  echo "✗ 需要 Python3，请先安装"
  exit 1
fi
echo "✓ Python3: $(python3 --version)"

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
echo ""

# 启动 CLI 配置向导
python3 litellm_cli.py init

echo ""
echo "=========================================="
echo " 安装完成！"
echo "=========================================="
echo ""
echo "服务端口："
echo "  管理界面 (Admin):    http://localhost:4000"
echo "  推理 API (Worker):   http://localhost:4001"
echo "  聊天界面 (WebUI):    http://localhost:3001"
echo "  编排平台 (Orchestrator): http://localhost:3002"
echo "  Grafana 仪表盘:      http://localhost:3000"
echo ""
echo "CLI 命令："
echo "  python3 litellm_cli.py health        # 健康检查"
echo "  python3 litellm_cli.py list-models   # 列出模型"
echo "  python3 litellm_cli.py add-model     # 添加模型"
echo "  python3 litellm_cli.py status        # 运行时状态"
echo "  python3 litellm_cli.py orchestrate '任务描述'  # 编排复杂任务"
echo ""
