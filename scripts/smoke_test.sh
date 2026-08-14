#!/usr/bin/env bash
# LLooM 冒烟测试 — 验证前端状态报告是否忠实反映真实情况
set -u

BASE="http://localhost:7861"
PASS=0
FAIL=0

check() {
  local desc="$1"
  local expected="$2"
  local actual="$3"
  if [ "$actual" = "$expected" ]; then
    PASS=$((PASS+1))
    echo "  ✓ $desc"
  else
    FAIL=$((FAIL+1))
    echo "  ✗ $desc (期望: $expected, 实际: $actual)"
  fi
}

echo "== 1. 核心服务 =="
R=$(curl -s -m 5 "$BASE/api/health")
check "health 端点" "ok" "$(echo "$R" | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])" 2>/dev/null)"

echo "== 2. 服务状态 (应全 Up 且 healthy) =="
R=$(curl -s -m 5 "$BASE/api/services/status")
check "Core Server healthy" "True" "$(echo "$R" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['services'][0]['healthy'])" 2>/dev/null)"
check "Ollama healthy" "True" "$(echo "$R" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['services'][1]['healthy'])" 2>/dev/null)"
check "AI Service healthy" "True" "$(echo "$R" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['services'][2]['healthy'])" 2>/dev/null)"
echo "  AI 服务详细: $(echo "$R" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['services'][2]['status'])" 2>/dev/null)"

echo "== 3. AI 服务自检 (应 ready=true, 有 Ollama 后端) =="
R=$(curl -s -m 5 http://localhost:7862/v1/health)
check "AI ready" "True" "$(echo "$R" | python3 -c "import sys,json; print(json.load(sys.stdin)['ready'])" 2>/dev/null)"
check "AI 报告 Ollama 可达" "True" "$(echo "$R" | python3 -c "import sys,json; print(json.load(sys.stdin)['backends']['ollama_reachable'])" 2>/dev/null)"

echo "== 4. 模型注册 =="
MODELS=$(curl -s -m 5 "$BASE/api/models")
COUNT=$(echo "$MODELS" | python3 -c "import sys,json; print(len(json.load(sys.stdin)['models']))" 2>/dev/null)
if [ "$COUNT" = "0" ]; then
  echo "  注册 qwen2.5-local..."
  curl -s -m 5 -X POST "$BASE/api/models" -H 'Content-Type: application/json' \
    -d '{"name":"qwen2.5-local","provider":"ollama","litellm_model":"ollama/qwen2.5:0.5b","api_base":"http://localhost:11434","api_key":"ollama","input_cost_per_token":0.000001,"output_cost_per_token":0.000002}' >/dev/null
  MODELS=$(curl -s -m 5 "$BASE/api/models")
  COUNT=$(echo "$MODELS" | python3 -c "import sys,json; print(len(json.load(sys.stdin)['models']))" 2>/dev/null)
fi
check "模型注册" "1" "$COUNT"

echo "== 5. 聊天 (真实 LLM 调用) =="
R=$(curl -s -m 60 -N -X POST "$BASE/api/chat/stream" -H 'Content-Type: application/json' \
  -d '{"messages":[{"role":"user","content":"只回答: 1+1=?"}]}')
CONTENT=$(echo "$R" | grep -o '"content":"[^"]*"' | tail -1 | cut -d'"' -f4)
check "聊天有内容返回" "非空" "$([ -n "$CONTENT" ] && echo 非空 || echo 空)"
check "聊天 done 标记" "True" "$(echo "$R" | python3 -c "import sys,json; evs=[json.loads(l[6:]) for l in sys.stdin.read().splitlines() if l.startswith('data: ')]; print(any('done' in e for e in evs))" 2>/dev/null)"

echo "== 6. 任务编排 =="
R=$(curl -s -m 60 -N -X POST "$BASE/api/orchestrate/stream" -H 'Content-Type: application/json' \
  -d '{"query":"1+1等于几，只回答数字","history":[]}')
check "编排有 decompose 事件" "true" "$(echo "$R" | grep -c 'event: decompose' >/dev/null && echo true || echo false)"
check "编排有 result 事件" "true" "$(echo "$R" | grep -c 'event: result' >/dev/null && echo true || echo false)"

echo "== 7. 用量统计 =="
R=$(curl -s -m 5 "$BASE/api/stats")
check "stats 有 total_spend" "0" "$(echo "$R" | python3 -c "import sys,json; d=json.load(sys.stdin); print('0' if d['total_spend']>=0 else '-1')" 2>/dev/null)"
check "stats 有 model_count" "≥1" "$(echo "$R" | python3 -c "import sys,json; print('≥1' if json.load(sys.stdin)['model_count']>=1 else '<1')" 2>/dev/null)"

echo "== 8. 对话 CRUD =="
R=$(curl -s -m 5 -X POST "$BASE/api/conversations" -H 'Content-Type: application/json' \
  -d '{"title":"冒烟测试","messages":[{"role":"user","content":"hi"}]}')
CID=$(echo "$R" | python3 -c "import sys,json; print(json.load(sys.stdin).get('id',''))" 2>/dev/null)
check "保存对话有 id" "非空" "$([ -n "$CID" ] && echo 非空 || echo 空)"
R=$(curl -s -m 5 "$BASE/api/conversations")
check "对话列表包含新对话" "true" "$(echo "$R" | grep -c "$CID" >/dev/null && echo true || echo false)"

echo "== 9. 预算 =="
R=$(curl -s -m 5 -X POST "$BASE/api/budgets" -H 'Content-Type: application/json' \
  -d '{"scope":"user","scope_id":"default","max_budget":10,"duration":"30d"}')
check "设置预算" "True" "$(echo "$R" | python3 -c "import sys,json; print(json.load(sys.stdin)['set'])" 2>/dev/null)"
R=$(curl -s -m 5 "$BASE/api/budgets/check?scope=user&scope_id=default")
check "预算检查 within" "True" "$(echo "$R" | python3 -c "import sys,json; print(json.load(sys.stdin)['within_budget'])" 2>/dev/null)"

echo "== 10. 服务管理端点 =="
R=$(curl -s -m 10 -X POST "$BASE/api/services/ai/restart")
check "AI 重启返回 message" "非空" "$([ -n "$R" ] && echo 非空 || echo 空)"
sleep 3
R=$(curl -s -m 5 http://localhost:7862/v1/health)
check "AI 重启后仍健康" "ok" "$(echo "$R" | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])" 2>/dev/null)"

echo ""
echo "======================================"
echo "结果: $PASS 通过, $FAIL 失败"
echo "======================================"
[ "$FAIL" = "0" ]
