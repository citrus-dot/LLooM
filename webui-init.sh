#!/bin/sh
# ====================================
# Open WebUI 启动前初始化脚本
# 确保 config.json 包含 LiteLLM Worker 的 OpenAI 连接配置
# 即使 volume 被重建，启动时也会自动注入配置
# ====================================

CONFIG_PATH="/app/backend/data/config.json"
LITELLM_URL="${OPENAI_API_BASE_URL:-http://litellm-worker:4000/v1}"
LITELLM_KEY="${OPENAI_API_KEYS:-${OPENAI_API_KEY:-sk-1234}}"

# 确保数据目录存在
mkdir -p /app/backend/data

# 如果 config.json 不存在，创建基础结构
if [ ! -f "$CONFIG_PATH" ]; then
  echo '{"version": 0, "ui": {}}' > "$CONFIG_PATH"
fi

# 使用 Python 合并 OpenAI 配置到 config.json
python3 -c "
import json

config_path = '$CONFIG_PATH'
litellm_url = '$LITELLM_URL'
litellm_key = '$LITELLM_KEY'

with open(config_path, 'r') as f:
    data = json.load(f)

# 始终从环境变量更新 URL 和 Key（Docker Compose 管理配置）
if 'openai' not in data:
    data['openai'] = {}
data['openai']['enable'] = True
data['openai']['url'] = litellm_url
data['openai']['key'] = litellm_key
with open(config_path, 'w') as f:
    json.dump(data, f, indent=2)
print('[webui-init] OpenAI config updated: url=' + litellm_url)
"

# 启动 Open WebUI 原始入口
exec bash start.sh
