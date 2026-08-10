#!/bin/bash
# ====================================
# Phase 2 验证脚本 — 配额管理 + 智能路由
# ====================================
# 用法：
#   chmod +x quota_setup.sh
#   ./quota_setup.sh
#
# 特性：幂等设计，可重复运行
#   - 用户已存在时自动跳过
#   - 密钥使用时间戳后缀，保证唯一
#   - /metrics 端点自动补全末尾斜杠
#
# 前提：docker compose 服务已启动
#       DASHSCOPE_API_KEY 已填写 → 云端模型可用，分类器使用 qwen3.6-flash
#       DASHSCOPE_API_KEY 为空   → 仅 Ollama 本地模型可用，分类器使用 qwen2.5:latest
#       Ollama 已在宿主机运行（localhost:11434）
# ====================================

ADMIN_URL="http://localhost:4000"
WORKER_URL="http://localhost:4001"
MASTER_KEY="${LITELLM_MASTER_KEY:-sk-1234}"
USER_ID="dev-user"
KEY_ALIAS="dev-key-$(date +%s)"

echo "=========================================="
echo " Phase 2 验证：配额管理 + 智能路由"
echo "=========================================="
echo ""

# ----------------------------------
# 1. 创建测试用户（带月预算 $5）
#    幂等：已存在则跳过
# ----------------------------------
echo "[1/7] 创建测试用户 ${USER_ID}（月预算 \$5）..."

USER_INFO=$(curl -sS "${ADMIN_URL}/user/info?user_id=${USER_ID}" \
  --header "Authorization: Bearer ${MASTER_KEY}" 2>/dev/null || echo "")

if echo "$USER_INFO" | grep -q "user_id"; then
  echo "  -> 用户 ${USER_ID} 已存在，跳过创建"
else
  curl -sS --location "${ADMIN_URL}/user/new" \
    --header "Authorization: Bearer ${MASTER_KEY}" \
    --header 'Content-Type: application/json' \
    --data-raw "{
      \"user_id\": \"${USER_ID}\",
      \"max_budget\": 5,
      \"budget_duration\": \"30d\"
    }" > /dev/null 2>&1
  echo "  -> 用户 ${USER_ID} 创建完成"
fi
echo ""

# ----------------------------------
# 2. 为用户创建虚拟密钥（带月预算 $2）
#    幂等：使用时间戳后缀保证唯一
# ----------------------------------
echo "[2/7] 创建虚拟密钥 ${KEY_ALIAS}（月预算 \$2，允许 auto 模型）..."
KEY_RESPONSE=$(curl -sS --location "${ADMIN_URL}/key/generate" \
  --header "Authorization: Bearer ${MASTER_KEY}" \
  --header 'Content-Type: application/json' \
  --data-raw "{
    \"user_id\": \"${USER_ID}\",
    \"key_alias\": \"${KEY_ALIAS}\",
    \"models\": [\"auto\", \"qwen2.5-local\", \"qwen-plus\", \"qwen3.6-flash\", \"deepseek-v3\", \"qwen3.6-plus\"],
    \"max_budget\": 2,
    \"budget_duration\": \"30d\",
    \"rpm_limit\": 60,
    \"tpm_limit\": 10000
  }" 2>/dev/null || echo "")

# 用 grep + sed 提取密钥（不依赖 python3，避免 pipefail 问题）
DEV_KEY=$(echo "$KEY_RESPONSE" | grep -o '"key":"sk-[^"]*"' | head -1 | sed 's/"key":"//;s/"//')

if [ -z "$DEV_KEY" ]; then
  echo "  ✗ 密钥创建失败"
  echo "  响应: $(echo "$KEY_RESPONSE" | head -c 300)"
  exit 1
fi
echo "  -> 密钥: ${DEV_KEY:0:16}... (alias: ${KEY_ALIAS})"
echo ""

# ----------------------------------
# 3. 测试智能路由 — 简单问答（应路由到 qwen2.5-local）
# ----------------------------------
echo "[3/7] 测试智能路由 — 简单问答（预期 → qwen2.5-local）..."
RESPONSE=$(curl -sS --max-time 60 --location "${WORKER_URL}/chat/completions" \
  --header "Authorization: Bearer ${DEV_KEY}" \
  --header 'Content-Type: application/json' \
  --data '{
    "model": "auto",
    "messages": [{"role": "user", "content": "你好，今天天气怎么样？"}],
    "max_tokens": 50
  }' 2>/dev/null || echo "")

if echo "$RESPONSE" | grep -q '"content"'; then
  CONTENT=$(echo "$RESPONSE" | grep -o '"content":"[^"]*"' | head -1 | sed 's/"content":"//;s/"$//')
  echo "  -> 请求成功: ${CONTENT:0:60}..."
else
  echo "  -> 请求失败: $(echo "$RESPONSE" | head -c 200)"
fi
echo ""

# ----------------------------------
# 4. 测试智能路由 — 代码生成（应路由到 deepseek-v3）
# ----------------------------------
echo "[4/7] 测试智能路由 — 代码生成（预期 → deepseek-v3，自动流式）..."
RESPONSE=$(curl -sS --max-time 60 --location "${WORKER_URL}/chat/completions" \
  --header "Authorization: Bearer ${DEV_KEY}" \
  --header 'Content-Type: application/json' \
  --data '{
    "model": "auto",
    "messages": [{"role": "user", "content": "用 Python 写一个快速排序函数"}],
    "max_tokens": 100
  }' 2>/dev/null || echo "")

if echo "$RESPONSE" | grep -q "^data:"; then
  echo "  -> 流式响应成功（deepseek-v3 自动启用流式）"
elif echo "$RESPONSE" | grep -q '"content"'; then
  CONTENT=$(echo "$RESPONSE" | grep -o '"content":"[^"]*"' | head -1 | sed 's/"content":"//;s/"$//')
  echo "  -> 请求成功: ${CONTENT:0:60}..."
else
  echo "  -> 请求失败: $(echo "$RESPONSE" | head -c 200)"
fi
echo ""

# ----------------------------------
# 5. 查询配额使用情况
# ----------------------------------
echo "[5/7] 查询配额使用情况..."

echo "  --- Virtual Key 花费 ---"
KEY_INFO=$(curl -sS "${ADMIN_URL}/key/info?key=${DEV_KEY}" \
  --header "Authorization: Bearer ${MASTER_KEY}" 2>/dev/null || echo "")
SPEND=$(echo "$KEY_INFO" | grep -o '"spend":[0-9.]*' | head -1 | sed 's/"spend"://')
MAX_BUDGET=$(echo "$KEY_INFO" | grep -o '"max_budget":[0-9.]*' | head -1 | sed 's/"max_budget"://')
echo "  -> spend=\$${SPEND:-0} / max=\$${MAX_BUDGET:-0}"

echo "  --- User 花费 ---"
USER_INFO=$(curl -sS "${ADMIN_URL}/user/info?user_id=${USER_ID}" \
  --header "Authorization: Bearer ${MASTER_KEY}" 2>/dev/null || echo "")
USER_SPEND=$(echo "$USER_INFO" | grep -o '"spend":[0-9.]*' | head -1 | sed 's/"spend"://')
USER_MAX=$(echo "$USER_INFO" | grep -o '"max_budget":[0-9.]*' | head -1 | sed 's/"max_budget"://')
echo "  -> spend=\$${USER_SPEND:-0} / max=\$${USER_MAX:-0}"
echo ""

# ----------------------------------
# 6. 检查 Prometheus 指标
#    注意：/metrics 需要末尾斜杠 /metrics/
# ----------------------------------
echo "[6/7] 检查 Prometheus 自定义指标..."
echo "  --- 任务分类指标 ---"
curl -sS "${WORKER_URL}/metrics/" 2>/dev/null | grep "litellm_task_router_classification_total" || echo "  (暂无数据)"
echo ""
echo "  --- 配额追踪指标 ---"
curl -sS "${WORKER_URL}/metrics/" 2>/dev/null | grep "litellm_quota_key_spend_total" || echo "  (暂无数据)"
echo ""

# ----------------------------------
# 7. 测试推理模型流式响应（直接调用 qwen3.6-flash）
# ----------------------------------
echo "[7/7] 测试推理模型流式响应（qwen3.6-flash，预期 → SSE 分块数据）..."
RESPONSE=$(curl -sS --max-time 60 -N "${WORKER_URL}/chat/completions" \
  --header "Authorization: Bearer ${DEV_KEY}" \
  --header 'Content-Type: application/json' \
  --data '{
    "model": "qwen3.6-flash",
    "messages": [{"role": "user", "content": "1+1=?"}],
    "max_tokens": 10
  }' 2>/dev/null | head -5)

if echo "$RESPONSE" | grep -q "^data:"; then
  FIRST_CHUNK=$(echo "$RESPONSE" | grep "^data:" | head -1 | head -c 80)
  echo "  -> 流式响应成功: ${FIRST_CHUNK}..."
else
  echo "  -> 请求失败: $(echo "$RESPONSE" | head -c 200)"
fi
echo ""

echo "=========================================="
echo " 验证完成！"
echo "=========================================="
echo ""
echo "Grafana 仪表盘："
echo "  - LiteLLM Overview:   http://localhost:3000/d/litellm-overview"
echo "  - 配额与智能路由:       http://localhost:3000/d/litellm-quota"
echo ""
echo "DEV_KEY=${DEV_KEY}"
echo "KEY_ALIAS=${KEY_ALIAS}"
