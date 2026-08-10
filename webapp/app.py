"""
LiteLLM 可视化管理界面（Phase 4）

功能：
  - 仪表盘：服务状态、成本统计、模型用量
  - 智能对话：复杂任务自动分解、成本可视化、SSE 实时推送
  - 配置向导：交互式模型配置、API Key 管理
  - 模型管理：查看、添加、删除模型

端口：3002
依赖：Flask（容器内安装）
"""

import json
import os
import sys
import time
import logging
import urllib.request
import urllib.error
from flask import Flask, request, jsonify, Response, render_template_string

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")
logger = logging.getLogger("webapp")

# 将项目根目录加入路径以导入 task_orchestrator
PROJECT_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, PROJECT_ROOT)

from task_orchestrator import Orchestrator, MODEL_PRICING

app = Flask(__name__)

# ==================================================
# 配置
# ==================================================

WORKER_URL = os.environ.get("LITELLM_WORKER_URL", "http://litellm-worker:4000")
ADMIN_URL = os.environ.get("LITELLM_ADMIN_URL", "http://litellm-admin:4000")
SR_URL = os.environ.get("SEMANTIC_ROUTER_URL", "http://semantic-router:8888")
API_KEY = os.environ.get("LITELLM_MASTER_KEY", "sk-1234")

# .env 文件路径（容器内挂载）
ENV_FILE_PATH = "/app/.env"


def api_get(url, timeout=5):
    """GET 请求辅助"""
    req = urllib.request.Request(url)
    req.add_header("Authorization", f"Bearer {API_KEY}")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return json.loads(resp.read().decode())
    except Exception as e:
        return {"error": str(e)}


def api_post(url, data, timeout=60):
    """POST 请求辅助"""
    body = json.dumps(data).encode()
    req = urllib.request.Request(url, data=body, method="POST")
    req.add_header("Authorization", f"Bearer {API_KEY}")
    req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return json.loads(resp.read().decode())
    except Exception as e:
        return {"error": str(e)}


# ==================================================
# 页面模板
# ==================================================

BASE_HTML = """<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} — LiteLLM 管理平台</title>
<style>
* {{ margin:0; padding:0; box-sizing:border-box; }}
body {{ font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif; background:#f5f6fa; color:#2d3436; }}
.nav {{ background:#2d3436; padding:0 20px; display:flex; align-items:center; height:56px; position:sticky; top:0; z-index:100; }}
.nav a {{ color:#dfe6e9; text-decoration:none; padding:0 16px; line-height:56px; font-size:14px; transition:0.2s; }}
.nav a:hover {{ background:#636e72; }}
.nav a.active {{ color:#00b894; border-bottom:2px solid #00b894; }}
.nav .logo {{ font-weight:700; font-size:16px; color:#00b894; margin-right:auto; }}
.container {{ max-width:1200px; margin:20px auto; padding:0 20px; }}
.card {{ background:#fff; border-radius:12px; padding:20px; margin-bottom:16px; box-shadow:0 1px 3px rgba(0,0,0,0.08); }}
.grid {{ display:grid; gap:16px; }}
.grid-2 {{ grid-template-columns:1fr 1fr; }}
.grid-3 {{ grid-template-columns:1fr 1fr 1fr; }}
.grid-4 {{ grid-template-columns:1fr 1fr 1fr 1fr; }}
.stat {{ text-align:center; padding:16px; }}
.stat .num {{ font-size:28px; font-weight:700; color:#0984e3; }}
.stat .label {{ font-size:13px; color:#636e72; margin-top:4px; }}
.stat .num.green {{ color:#00b894; }}
.stat .num.orange {{ color:#e17055; }}
.stat .num.red {{ color:#d63031; }}
table {{ width:100%; border-collapse:collapse; font-size:14px; }}
th {{ text-align:left; padding:10px 12px; background:#f5f6fa; border-bottom:2px solid #dfe6e9; color:#636e72; font-weight:600; }}
td {{ padding:10px 12px; border-bottom:1px solid #f1f2f6; }}
.badge {{ display:inline-block; padding:2px 8px; border-radius:4px; font-size:12px; font-weight:600; }}
.badge-green {{ background:#e6fffa; color:#00b894; }}
.badge-orange {{ background:#fff3e0; color:#e17055; }}
.badge-red {{ background:#ffebee; color:#d63031; }}
.badge-blue {{ background:#e3f2fd; color:#0984e3; }}
.btn {{ display:inline-block; padding:8px 20px; border:none; border-radius:6px; cursor:pointer; font-size:14px; transition:0.2s; }}
.btn-primary {{ background:#0984e3; color:#fff; }}
.btn-primary:hover {{ background:#0773c5; }}
.btn-success {{ background:#00b894; color:#fff; }}
.btn-success:hover {{ background:#00a381; }}
.chat-box {{ background:#fff; border-radius:12px; height:500px; display:flex; flex-direction:column; overflow:hidden; }}
.chat-msgs {{ flex:1; overflow-y:auto; padding:16px; }}
.msg {{ margin-bottom:12px; max-width:80%; }}
.msg-user {{ margin-left:auto; }}
.msg-bubble {{ padding:10px 14px; border-radius:10px; font-size:14px; line-height:1.6; }}
.msg-user .msg-bubble {{ background:#0984e3; color:#fff; }}
.msg-ai .msg-bubble {{ background:#f1f2f6; }}
.msg-decomp {{ background:#fff8e1; border:1px solid #ffe082; border-radius:10px; padding:12px; margin:8px 0; font-size:13px; }}
.msg-decomp .task-line {{ margin:4px 0; padding:4px 8px; background:#fff; border-radius:4px; }}
.chat-input {{ padding:12px; border-top:1px solid #f1f2f6; display:flex; gap:8px; }}
.chat-input input {{ flex:1; padding:10px; border:1px solid #dfe6e9; border-radius:6px; font-size:14px; }}
.chat-input button {{ padding:10px 24px; }}
.sse-status {{ font-size:12px; color:#636e72; padding:4px 12px; }}
input.form-input, select.form-input {{ width:100%; padding:8px 12px; border:1px solid #dfe6e9; border-radius:6px; font-size:14px; margin-bottom:12px; }}
label.form-label {{ display:block; font-size:13px; color:#636e72; margin-bottom:4px; font-weight:600; }}
h2 {{ font-size:20px; margin-bottom:16px; }}
h3 {{ font-size:16px; margin-bottom:12px; color:#2d3436; }}
.muted {{ color:#636e72; font-size:13px; }}
.cost-table {{ font-size:13px; }}
.cost-table .model {{ font-weight:600; }}
.progress {{ height:4px; background:#f1f2f6; border-radius:2px; margin:4px 0; }}
.progress-bar {{ height:100%; background:#00b894; border-radius:2px; transition:width 0.3s; }}
.env-section {{ margin-bottom: 20px; }}
.env-section-header {{ display:flex; align-items:center; gap:8px; font-size:15px; font-weight:600; color:#2d3436; margin-bottom:12px; cursor:pointer; user-select:none; }}
.env-section-header .arrow {{ transition:transform 0.2s; font-size:12px; }}
.env-section-header .arrow.collapsed {{ transform:rotate(-90deg); }}
.env-section-body {{ overflow:hidden; }}
.env-section-body.collapsed {{ display:none; }}
.env-field {{ display:flex; align-items:center; gap:12px; padding:10px 0; border-bottom:1px solid #f1f2f6; }}
.env-field:last-child {{ border:none; }}
.env-field-label {{ min-width:160px; font-size:13px; font-weight:500; }}
.env-field-label .desc {{ display:block; font-size:11px; color:#b2bec3; margin-top:2px; }}
.env-field input {{ flex:1; padding:8px 12px; border:1px solid #dfe6e9; border-radius:6px; font-size:13px; font-family:monospace; }}
.env-field input:focus {{ border-color:#0984e3; outline:none; }}
.env-field .status-dot {{ width:8px; height:8px; border-radius:50%; flex-shrink:0; }}
.env-field .status-dot.set {{ background:#00b894; }}
.env-field .status-dot.empty {{ background:#dfe6e9; }}
.env-field .toggle-vis {{ cursor:pointer; font-size:16px; color:#b2bec3; user-select:none; }}
.btn-sm {{ padding:4px 12px; font-size:12px; }}
.btn-warning {{ background:#e17055; color:#fff; }}
.btn-warning:hover {{ background:#c95f3f; }}
.toast {{ position:fixed; top:20px; right:20px; padding:12px 24px; border-radius:8px; color:#fff; font-size:14px; z-index:9999; opacity:0; transition:opacity 0.3s; }}
.toast.show {{ opacity:1; }}
.toast.success {{ background:#00b894; }}
.toast.error {{ background:#d63031; }}
.toast.info {{ background:#0984e3; }}
</style>
</head>
<body>
<div class="nav">
  <span class="logo">LiteLLM 管理平台</span>
  <a href="/" class="{active_home}">仪表盘</a>
  <a href="/chat" class="{active_chat}">智能对话</a>
  <a href="/config" class="{active_config}">配置向导</a>
  <a href="/models" class="{active_models}">模型管理</a>
</div>
<div class="container">
{content}
</div>
</body>
</html>"""


# ==================================================
# 路由：仪表盘
# ==================================================

@app.route("/")
def dashboard():
    # 获取服务状态
    health = api_get(f"{WORKER_URL}/health/liveliness", timeout=3)
    worker_ok = "error" not in health

    # 获取模型列表
    models_data = api_get(f"{WORKER_URL}/v1/models", timeout=5)
    models = models_data.get("data", []) if "error" not in models_data else []

    # 获取 Prometheus 指标
    try:
        with urllib.request.urlopen(f"{WORKER_URL}/metrics/", timeout=5) as resp:
            metrics_text = resp.read().decode()
    except Exception:
        metrics_text = ""

    # 解析路由统计
    import re
    router_stats = []
    for line in metrics_text.split("\n"):
        if "litellm_task_router_classification_total" in line and not line.startswith("#"):
            m = re.search(r'method="([^"]*)".*target_model="([^"]*)".*task_type="([^"]*)"\}\s+(\S+)', line)
            if m:
                router_stats.append({"method": m.group(1), "model": m.group(2),
                                     "task_type": m.group(3), "count": int(float(m.group(4)))})

    # 解析缓存命中
    cache_hits = 0
    for line in metrics_text.split("\n"):
        if "litellm_cache_hits_metric_total" in line and not line.startswith("#"):
            m = re.search(r'\}\s+(\S+)', line)
            if m:
                cache_hits += int(float(m.group(1)))

    # 解析花费
    spend_lines = [l for l in metrics_text.split("\n")
                   if "litellm_quota_key_spend_by_model_total" in l
                   and not l.startswith("#") and "_created" not in l]
    total_spend = 0.0
    for line in spend_lines:
        m = re.search(r'\}\s+(\S+)', line)
        if m:
            total_spend += float(m.group(1))

    stats_html = f"""
    <div class="grid grid-4">
        <div class="card stat"><div class="num {'green' if worker_ok else 'red'}">{'正常' if worker_ok else '异常'}</div><div class="label">Worker 状态</div></div>
        <div class="card stat"><div class="num">{len(models)}</div><div class="label">可用模型</div></div>
        <div class="card stat"><div class="num green">{cache_hits}</div><div class="label">缓存命中</div></div>
        <div class="card stat"><div class="num orange">${total_spend:.6f}</div><div class="label">累计花费</div></div>
    </div>
    """

    # 路由统计表
    router_rows = ""
    for s in router_stats:
        router_rows += f"<tr><td>{s['task_type']}</td><td><span class='badge badge-blue'>{s['method']}</span></td><td>{s['model']}</td><td>{s['count']}</td></tr>"

    if not router_rows:
        router_rows = "<tr><td colspan='4' class='muted' style='text-align:center'>暂无路由数据</td></tr>"

    # 模型列表
    model_rows = ""
    for m in models:
        mid = m["id"]
        pricing = MODEL_PRICING.get(mid, {"label": "未定价"})
        model_rows += f"<tr><td><span class='badge badge-green'>{mid}</span></td><td class='muted'>{pricing.get('label','')}</td></tr>"

    content = f"""
    <h2>仪表盘</h2>
    {stats_html}
    <div class="grid grid-2">
        <div class="card">
            <h3>智能路由统计</h3>
            <table>
                <thead><tr><th>任务类型</th><th>方法</th><th>目标模型</th><th>次数</th></tr></thead>
                <tbody>{router_rows}</tbody>
            </table>
        </div>
        <div class="card">
            <h3>模型列表</h3>
            <table>
                <thead><tr><th>模型</th><th>说明</th></tr></thead>
                <tbody>{model_rows}</tbody>
            </table>
        </div>
    </div>
    <div class="card">
        <h3>快捷操作</h3>
        <a href="/chat" class="btn btn-primary">开始智能对话</a>
        <a href="/config" class="btn btn-success">配置向导</a>
        <a href="/models" class="btn btn-primary">模型管理</a>
    </div>
    """
    return render_template_string(BASE_HTML.format(title="仪表盘", content=content,
                                                    active_home="active", active_chat="",
                                                    active_config="", active_models=""))


# ==================================================
# 路由：智能对话（SSE 实时推送）
# ==================================================

@app.route("/chat")
def chat_page():
    content = """
    <h2>智能对话</h2>
    <div class="card">
        <div class="chat-box">
            <div class="chat-msgs" id="messages">
                <div class="msg msg-ai"><div class="msg-bubble">你好！我是智能编排助手。复杂任务会自动分解为子任务，选择最优模型执行。请输入你的问题。</div></div>
            </div>
            <div class="sse-status" id="status"></div>
            <div class="chat-input">
                <input type="text" id="input" placeholder="输入消息... (Shift+Enter 换行)" onkeydown="if(event.key==='Enter')send()">
                <button class="btn btn-primary" onclick="send()">发送</button>
            </div>
        </div>
    </div>
    <div class="card">
        <h3>使用说明</h3>
        <p class="muted">• 输入 <code>auto</code> 模型的请求，系统会自动检测复杂度并分解</p>
        <p class="muted">• 复杂任务（多步骤、长文本）会被拆解为子任务，每个子任务选择成本最优模型</p>
        <p class="muted">• 实时显示分解过程、模型选择、成本估算和执行进度</p>
    </div>
    <script>
    function send() {
        var input = document.getElementById('input');
        var msg = input.value.trim();
        if (!msg) return;
        input.value = '';
        
        // 显示用户消息
        var msgs = document.getElementById('messages');
        msgs.innerHTML += '<div class="msg msg-user"><div class="msg-bubble">' + escapeHtml(msg) + '</div></div>';
        msgs.scrollTop = msgs.scrollHeight;
        
        // SSE 连接
        var status = document.getElementById('status');
        status.textContent = '正在处理...';
        
        var es = new EventSource('/api/chat/stream?q=' + encodeURIComponent(msg));
        
        es.addEventListener('decompose', function(e) {
            var data = JSON.parse(e.data);
            var html = '<div class="msg-decomp"><b>任务分解</b> (' + data.sub_tasks.length + ' 个子任务)<br>';
            data.sub_tasks.forEach(function(t) {
                html += '<div class="task-line">→ 子任务' + t.id + ': ' + escapeHtml(t.description) + ' <span class="badge badge-blue">' + t.selected_model + '</span> $' + t.cost.toFixed(6) + '</div>';
            });
            html += '<div class="muted">预估总成本: $' + data.total_cost.toFixed(6) + '</div></div>';
            msgs.innerHTML += html;
            msgs.scrollTop = msgs.scrollHeight;
        });
        
        es.addEventListener('task_start', function(e) {
            var data = JSON.parse(e.data);
            status.textContent = '执行子任务 ' + data.id + ': ' + data.description.substring(0,40) + '...';
            msgs.innerHTML += '<div class="msg msg-ai" id="task-' + data.id + '"><div class="msg-bubble">⏳ 子任务 ' + data.id + ' 执行中 (' + data.model + ')...</div></div>';
            msgs.scrollTop = msgs.scrollHeight;
        });
        
        es.addEventListener('task_done', function(e) {
            var data = JSON.parse(e.data);
            var el = document.getElementById('task-' + data.id);
            if (el) {
                el.innerHTML = '<div class="msg-bubble">✅ 子任务 ' + data.id + ' 完成 (' + data.model + ', ' + data.duration.toFixed(1) + 's, $' + data.cost.toFixed(6) + ')</div>';
            }
            msgs.scrollTop = msgs.scrollHeight;
        });
        
        es.addEventListener('result', function(e) {
            var data = JSON.parse(e.data);
            msgs.innerHTML += '<div class="msg msg-ai"><div class="msg-bubble">' + escapeHtml(data.response) + '</div></div>';
            msgs.innerHTML += '<div class="msg-decomp"><b>执行汇总</b><br>总成本: $' + data.total_cost.toFixed(6) + ' | 总Token: ' + data.total_tokens + ' | 耗时: ' + data.total_duration.toFixed(1) + 's</div>';
            msgs.scrollTop = msgs.scrollHeight;
            status.textContent = '';
        });
        
        es.onerror = function() {
            status.textContent = '连接结束';
            es.close();
        };
    }
    function escapeHtml(s) {
        return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
    }
    </script>
    """
    return render_template_string(BASE_HTML.format(title="智能对话", content=content,
                                                    active_home="", active_chat="active",
                                                    active_config="", active_models=""))


# ==================================================
# API: SSE 流式编排
# ==================================================

@app.route("/api/chat/stream", methods=["GET", "POST"])
def chat_stream():
    if request.method == "POST":
        data = request.get_json(silent=True) or {}
        query = data.get("q", "")
        history = data.get("history", [])
    else:
        query = request.args.get("q", "")
        history = []

    def generate():
        # Semantic Router pre-check: PII / jailbreak / domain classification
        sr_domain = "other"
        sr_info = ""
        try:
            check_resp = api_post(f"{SR_URL}/check", {"text": query}, timeout=15)
            if check_resp.get("blocked"):
                reason = check_resp.get("block_reason", "unknown")
                yield f"event: error\ndata: {json.dumps({'message': f'请求被语义路由器拦截: {reason}'}, ensure_ascii=False)}\n\n"
                return
            sr_domain = check_resp.get("domain", "other")
            parts = [f"SR域分类: {sr_domain}"]
            pii_report = check_resp.get("pii", {})
            if pii_report and pii_report.get("detected"):
                pii_types = list(pii_report.get("detected", {}).keys())
                parts.append(f"PII脱敏: {','.join(pii_types)}")
            jb_report = check_resp.get("jailbreak", {})
            if jb_report and jb_report.get("detected"):
                parts.append("越狱: 已拦截")
            sr_info = " | ".join(parts)
        except Exception as e:
            logger.warning("Semantic Router pre-check failed: %s", e)
        orch = Orchestrator(WORKER_URL, API_KEY)

        # 步骤 1: 复杂度检测
        is_complex = orch.is_complex(query)
        if not is_complex:
            # 简单任务直接执行
            yield f"event: decompose\ndata: {json.dumps({'sub_tasks': [{'id':1,'description':query,'selected_model':'auto','cost':0.0001}], 'total_cost': 0.0001}, ensure_ascii=False)}\n\n"
            yield f"event: task_start\ndata: {json.dumps({'id':1,'description':query,'model':'auto'}, ensure_ascii=False)}\n\n"

            result = orch.orchestrate(query, history=history)
            yield f"event: task_done\ndata: {json.dumps({'id':1,'model':result.sub_tasks[0].get('selected_model','auto'),'duration':result.total_duration,'cost':result.total_cost}, ensure_ascii=False)}\n\n"
            yield f"event: result\ndata: {json.dumps({'response':result.final_response,'total_cost':result.total_cost,'total_tokens':result.total_tokens,'total_duration':result.total_duration,'sr_info':sr_info}, ensure_ascii=False)}\n\n"
            return

        # 步骤 2: 分解
        sub_tasks = orch.decompose(query)
        sub_tasks = orch.plan_costs(sub_tasks)

        decompose_data = {
            "sub_tasks": [{"id": t.id, "description": t.description, "selected_model": t.selected_model,
                            "cost": t.cost, "task_type": t.task_type} for t in sub_tasks],
            "total_cost": sum(t.cost for t in sub_tasks),
        }
        yield f"event: decompose\ndata: {json.dumps(decompose_data, ensure_ascii=False)}\n\n"

        # 步骤 3: 逐个执行
        completed = {}
        for task in sub_tasks:
            context_parts = []
            for dep_id in task.depends_on:
                if dep_id in completed:
                    dep = completed[dep_id]
                    context_parts.append(f"[子任务{dep_id}] {dep.description}\n结果: {dep.result}")
            context = "\n\n".join(context_parts) if context_parts else ""

            yield f"event: task_start\ndata: {json.dumps({'id':task.id,'description':task.description,'model':task.selected_model}, ensure_ascii=False)}\n\n"

            orch.execute_task(task, context, history=history)
            completed[task.id] = task

            yield f"event: task_done\ndata: {json.dumps({'id':task.id,'model':task.selected_model,'duration':task.duration,'cost':task.cost,'tokens':task.tokens_used}, ensure_ascii=False)}\n\n"

        # 步骤 4: 汇总
        final = orch.aggregate(query, sub_tasks, history=history)
        total_cost = sum(t.cost for t in sub_tasks)
        total_tokens = sum(t.tokens_used for t in sub_tasks)

        yield f"event: result\ndata: {json.dumps({'response':final,'total_cost':total_cost,'total_tokens':total_tokens,'total_duration':0,'sr_info':sr_info}, ensure_ascii=False)}\n\n"

    return Response(generate(), content_type="text/event-stream; charset=utf-8")


# ==================================================
# 路由：配置向导
# ==================================================

@app.route("/config")
def config_page():
    # 获取 Ollama 状态
    ollama_base = os.environ.get("OLLAMA_API_BASE", "http://host.docker.internal:11434")
    try:
        with urllib.request.urlopen(f"{ollama_base}/api/tags", timeout=3) as resp:
            ollama_data = json.loads(resp.read().decode())
            ollama_models = [m["name"] for m in ollama_data.get("models", [])]
            ollama_ok = True
    except Exception:
        ollama_models = []
        ollama_ok = False

    ollama_rows = ""
    for m in ollama_models:
        ollama_rows += f"<tr><td>{m}</td></tr>"
    if not ollama_models:
        ollama_rows = "<tr><td class='muted'>无已安装模型</td></tr>"

    # 路由策略表
    from task_orchestrator import TASK_MODEL_PREFERENCE
    task_labels = {"simple_qa":"简单问答","general":"日常对话","coding":"代码生成","math_logic":"数学推理","complex_reasoning":"复杂分析"}
    route_rows = ""
    for task_type, models in TASK_MODEL_PREFERENCE.items():
        model = models[0]
        pricing = MODEL_PRICING.get(model, {"label":""})
        route_rows += f"<tr><td>{task_labels.get(task_type,task_type)}</td><td class='muted'>{task_type}</td><td><span class='badge badge-blue'>{model}</span></td><td>{pricing['label']}</td></tr>"

    # 定价表
    pricing_rows = ""
    for model, pricing in MODEL_PRICING.items():
        pricing_rows += f"<tr><td class='model'>{model}</td><td>${pricing['input']:.8f}</td><td>${pricing['output']:.8f}</td><td class='muted'>{pricing['label']}</td></tr>"

    content = f"""
    <h2>配置向导</h2>

    <div class="grid grid-2" style="margin-bottom:16px;">
        <div class="card">
            <h3>Ollama 本地模型 ({'运行中' if ollama_ok else '未运行'})</h3>
            <table>
                <thead><tr><th>模型</th></tr></thead>
                <tbody>{ollama_rows}</tbody>
            </table>
        </div>
        <div class="card">
            <h3>快捷操作</h3>
            <div style="display:flex;flex-wrap:wrap;gap:8px;">
                <button class="btn btn-primary" onclick="saveAllEnv()">💾 保存全部配置</button>
                <button class="btn btn-warning" onclick="saveAndRestart()">🔄 保存并重启服务</button>
            </div>
            <p class="muted" style="margin-top:8px;">修改 API Key 后需重启服务使配置生效</p>
        </div>
    </div>

    <div class="card">
        <h3>API 密钥配置</h3>
        <p class="muted" style="margin-bottom:16px;">点击编辑各供应商的 API Key，修改后点击"保存"或"保存全部配置"</p>
        <div id="env-form"><span class="badge badge-blue">加载中...</span></div>
    </div>

    <div class="card">
        <h3>路由策略</h3>
        <p class="muted">当前路由策略根据已配置的 API Key 自动适配：</p>
        <table class="cost-table">
            <thead><tr><th>任务类型</th><th>说明</th><th>首选模型</th><th>成本</th></tr></thead>
            <tbody>{route_rows}</tbody>
        </table>
    </div>
    <div class="card">
        <h3>模型定价表</h3>
        <table class="cost-table">
            <thead><tr><th>模型</th><th>输入 ($/token)</th><th>输出 ($/token)</th><th>说明</th></tr></thead>
            <tbody>{pricing_rows}</tbody>
        </table>
    </div>

    <div class="toast" id="toast"></div>

    <script>
    var ENV_SCHEMA = [
        {{ title: '阿里云百炼（DashScope）', icon: '☁️', collapsed: false, items: [
            {{ key: 'DASHSCOPE_API_KEY', label: 'API Key', type: 'password', desc: '主要 LLM 供应商' }},
            {{ key: 'DASHSCOPE_API_BASE', label: 'API Base', type: 'text', desc: '默认 dashscope.aliyuncs.com' }},
        ]}},
        {{ title: 'OpenAI', icon: '🤖', collapsed: true, items: [
            {{ key: 'OPENAI_API_KEY', label: 'API Key', type: 'password', desc: 'sk-...' }},
            {{ key: 'OPENAI_BASE_URL', label: 'Base URL', type: 'text', desc: '可选代理地址' }},
        ]}},
        {{ title: 'Anthropic', icon: '🧠', collapsed: true, items: [
            {{ key: 'ANTHROPIC_API_KEY', label: 'API Key', type: 'password', desc: 'sk-ant-...' }},
        ]}},
        {{ title: 'OpenRouter', icon: '🔄', collapsed: true, items: [
            {{ key: 'OR_API_KEY', label: 'API Key', type: 'password', desc: 'sk-or-...' }},
            {{ key: 'OR_SITE_URL', label: 'Site URL', type: 'text', desc: '可选' }},
        ]}},
        {{ title: '其他提供商', icon: '🔌', collapsed: true, items: [
            {{ key: 'AZURE_API_KEY', label: 'Azure API Key', type: 'password', desc: 'Azure OpenAI' }},
            {{ key: 'AZURE_API_BASE', label: 'Azure Base', type: 'text', desc: 'Azure endpoint' }},
            {{ key: 'COHERE_API_KEY', label: 'Cohere', type: 'password', desc: 'Cohere API Key' }},
            {{ key: 'REPLICATE_API_TOKEN', label: 'Replicate', type: 'password', desc: 'Replicate Token' }},
            {{ key: 'NOVITA_API_KEY', label: 'Novita AI', type: 'password', desc: 'Novita API Key' }},
        ]}},
        {{ title: '核心配置', icon: '⚙️', collapsed: false, items: [
            {{ key: 'LITELLM_MASTER_KEY', label: 'Master Key', type: 'password', desc: 'LiteLLM 管理密钥' }},
            {{ key: 'REDIS_PASSWORD', label: 'Redis 密码', type: 'password', desc: '须与 redis.conf 一致' }},
            {{ key: 'OLLAMA_API_BASE', label: 'Ollama API Base', type: 'text', desc: '本地 Ollama 地址' }},
            {{ key: 'QDRANT_API_KEY', label: 'Qdrant API Key', type: 'password', desc: '语义缓存向量库（可选）' }},
        ]}},
    ];

    var envValues = {{}};

    function showToast(msg, type) {{
        var el = document.getElementById('toast');
        el.textContent = msg;
        el.className = 'toast show ' + (type || 'info');
        setTimeout(function() {{ el.className = 'toast ' + (type || 'info'); }}, 3000);
    }}

    function escapeAttr(s) {{
        return String(s).replace(/"/g,'&quot;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
    }}

    function toggleSection(idx) {{
        var body = document.getElementById('section-' + idx);
        var arrows = document.querySelectorAll('.env-section-header .arrow');
        if (body) body.classList.toggle('collapsed');
        if (arrows[idx]) arrows[idx].classList.toggle('collapsed');
    }}

    function togglePw(id) {{
        var el = document.getElementById(id);
        if (el) el.type = el.type === 'password' ? 'text' : 'password';
    }}

    function renderForm() {{
        var container = document.getElementById('env-form');
        if (!container) return;
        var html = '';
        ENV_SCHEMA.forEach(function(section, si) {{
            var cls = section.collapsed ? ' collapsed' : '';
            html += '<div class="env-section">';
            html += '<div class="env-section-header" onclick="toggleSection(' + si + ')">';
            html += '<span class="arrow' + cls + '">▼</span> ' + section.icon + ' ' + section.title;
            html += '</div>';
            html += '<div class="env-section-body' + cls + '" id="section-' + si + '">';
            section.items.forEach(function(item) {{
                var val = envValues[item.key] || '';
                var isSet = val.trim().length > 0;
                var displayVal = item.type === 'password' ? '' : val;
                html += '<div class="env-field">';
                html += '<div class="status-dot ' + (isSet?'set':'empty') + '" title="' + (isSet?'已配置':'未配置') + '"></div>';
                html += '<div class="env-field-label">' + item.label + '<span class="desc">' + (item.desc||'') + '</span></div>';
                if (item.type === 'password') {{
                    html += '<input type="password" id="env-' + item.key + '" placeholder="' + (isSet?'已配置（输入新值可覆盖）':(item.desc||'')) + '" value="">';
                    html += '<span class="toggle-vis" onclick="togglePw(\\'env-' + item.key + '\\')">👁</span>';
                }} else {{
                    html += '<input type="text" id="env-' + item.key + '" placeholder="' + (item.desc||'') + '" value="' + escapeAttr(displayVal) + '">';
                }}
                html += '<button class="btn btn-sm btn-primary" onclick="saveKey(\\'' + item.key + '\\')">保存</button>';
                html += '</div>';
            }});
            html += '</div></div>';
        }});
        container.innerHTML = html;
    }}

    function loadEnv() {{
        fetch('/api/env').then(function(r) {{ return r.json(); }}).then(function(data) {{
            envValues = data;
            renderForm();
        }}).catch(function(e) {{
            document.getElementById('env-form').innerHTML = '<span class="badge badge-red">读取 .env 失败: ' + e + '</span>';
        }});
    }}

    function saveKey(key) {{
        var el = document.getElementById('env-' + key);
        if (!el) return;
        var val = el.value.trim();
        if (val === '' && el.type === 'password') {{
            showToast('请输入新值', 'info');
            return;
        }}
        var payload = {{}};
        payload[key] = val;
        fetch('/api/env', {{
            method: 'POST',
            headers: {{ 'Content-Type': 'application/json' }},
            body: JSON.stringify({{ updates: payload }})
        }}).then(function(r) {{ return r.json(); }}).then(function(data) {{
            if (data.error) {{ showToast('保存失败: ' + data.error, 'error'); return; }}
            envValues[key] = val;
            showToast(key + ' 已保存', 'success');
            renderForm();
        }}).catch(function(e) {{ showToast('保存失败: ' + e, 'error'); }});
    }}

    function saveAllEnv() {{
        var updates = {{}};
        var changed = 0;
        ENV_SCHEMA.forEach(function(section) {{
            section.items.forEach(function(item) {{
                var el = document.getElementById('env-' + item.key);
                if (!el) return;
                var val = el.value.trim();
                if (val === '' && item.type === 'password') return;
                if (val !== (envValues[item.key] || '')) {{
                    updates[item.key] = val;
                    changed++;
                }}
            }});
        }});
        if (changed === 0) {{ showToast('没有需要保存的更改', 'info'); return; }}
        fetch('/api/env', {{
            method: 'POST',
            headers: {{ 'Content-Type': 'application/json' }},
            body: JSON.stringify({{ updates: updates }})
        }}).then(function(r) {{ return r.json(); }}).then(function(data) {{
            if (data.error) {{ showToast('保存失败: ' + data.error, 'error'); return; }}
            Object.assign(envValues, updates);
            showToast('已保存 ' + changed + ' 项配置', 'success');
            renderForm();
        }}).catch(function(e) {{ showToast('保存失败: ' + e, 'error'); }});
    }}

    function saveAndRestart() {{
        saveAllEnv();
        showToast('请在主机上重启 Docker 服务使配置生效', 'info');
    }}

    loadEnv();
    </script>
    """
    return render_template_string(BASE_HTML.format(title="配置向导", content=content,
                                                    active_home="", active_chat="",
                                                    active_config="active", active_models=""))


# ==================================================
# 路由：模型管理
# ==================================================

@app.route("/models")
def models_page():
    models_data = api_get(f"{WORKER_URL}/v1/models", timeout=5)
    models = models_data.get("data", []) if "error" not in models_data else []

    model_cards = ""
    for m in models:
        mid = m["id"]
        pricing = MODEL_PRICING.get(mid, {"label": "自定义/未定价", "input": 0, "output": 0})
        model_cards += f"""
        <div class="card" style="display:inline-block;width:280px;margin:8px;">
            <h3>{mid}</h3>
            <p class="muted">{pricing['label']}</p>
            <table style="margin-top:8px;">
                <tr><td class="muted">输入</td><td>${pricing['input']:.8f}/token</td></tr>
                <tr><td class="muted">输出</td><td>${pricing['output']:.8f}/token</td></tr>
            </table>
        </div>
        """

    content = f"""
    <h2>模型管理</h2>
    <div class="card">
        <h3>已注册模型 ({len(models)} 个)</h3>
        {model_cards if model_cards else '<p class="muted">暂无模型</p>'}
    </div>
    <div class="card">
        <h3>添加新模型</h3>
        <p class="muted">通过 CLI 交互式添加: <code>python3 litellm_cli.py add-model</code></p>
    </div>
    """
    return render_template_string(BASE_HTML.format(title="模型管理", content=content,
                                                    active_home="", active_chat="",
                                                    active_config="", active_models="active"))


# ==================================================
# API: 直接对话（非 SSE）
# ==================================================

@app.route("/api/chat", methods=["POST"])
def chat_api():
    data = request.json
    query = data.get("message", "")
    use_orchestrate = data.get("orchestrate", True)
    history = data.get("history", [])

    if use_orchestrate:
        orch = Orchestrator(WORKER_URL, API_KEY)
        result = orch.orchestrate(query, history=history)
        return jsonify({
            "response": result.final_response,
            "decomposed": result.decomposed,
            "sub_tasks": result.sub_tasks,
            "total_cost": result.total_cost,
            "total_tokens": result.total_tokens,
            "total_duration": result.total_duration,
        })
    else:
        # 通过语义路由器调用 auto 模型（PII/越狱检测 + 域分类）
        result = api_post(f"{SR_URL}/v1/chat/completions", {
            "model": "auto",
            "messages": [{"role": "user", "content": query}],
            "max_tokens": 500,
        })
        if "error" not in result:
            return jsonify({
                "response": result["choices"][0]["message"]["content"],
                "decomposed": False,
            })
        return jsonify({"error": result["error"]}), 500


# ==================================================
# API: .env 配置读写
# ==================================================

def read_env_file():
    """读取 .env 文件，返回 dict"""
    env_map = {}
    try:
        with open(ENV_FILE_PATH, "r") as f:
            for line in f:
                line = line.strip()
                if not line or line.startswith("#"):
                    continue
                if "=" in line:
                    key, _, val = line.partition("=")
                    env_map[key.strip()] = val.strip()
    except FileNotFoundError:
        pass
    return env_map


def write_env_file(updates):
    """批量更新 .env 文件"""
    lines = []
    try:
        with open(ENV_FILE_PATH, "r") as f:
            lines = f.read().splitlines()
    except FileNotFoundError:
        lines = []

    for key, value in updates.items():
        prefix = f"{key}="
        found = False
        for i, line in enumerate(lines):
            if line.startswith(prefix):
                lines[i] = f"{key}={value}"
                found = True
                break
        if not found:
            lines.append(f"{key}={value}")

    with open(ENV_FILE_PATH, "w") as f:
        f.write("\n".join(lines) + "\n")


@app.route("/api/env", methods=["GET"])
def get_env():
    return jsonify(read_env_file())


@app.route("/api/env", methods=["POST"])
def save_env():
    data = request.json
    updates = data.get("updates", {})
    if not updates:
        return jsonify({"error": "无更新内容"}), 400
    try:
        write_env_file(updates)
        return jsonify({"ok": True, "updated": len(updates)})
    except Exception as e:
        return jsonify({"error": str(e)}), 500


# ==================================================
# API: 用量统计（供 Tauri app 调用）
# ==================================================

@app.route("/api/stats")
def get_stats():
    import re

    # Worker 健康状态
    health = api_get(f"{WORKER_URL}/health/liveliness", timeout=3)
    worker_ok = "error" not in health

    # 模型列表
    models_data = api_get(f"{WORKER_URL}/v1/models", timeout=5)
    models = models_data.get("data", []) if "error" not in models_data else []
    model_list = [{"id": m["id"], "pricing": MODEL_PRICING.get(m["id"], {"label": "未定价"})} for m in models]

    # Prometheus 指标
    try:
        with urllib.request.urlopen(f"{WORKER_URL}/metrics/", timeout=5) as resp:
            metrics_text = resp.read().decode()
    except Exception:
        metrics_text = ""

    # 路由统计
    router_stats = []
    for line in metrics_text.split("\n"):
        if "litellm_task_router_classification_total" in line and not line.startswith("#"):
            m = re.search(r'method="([^"]*)".*target_model="([^"]*)".*task_type="([^"]*)"\}\s+(\S+)', line)
            if m:
                router_stats.append({"method": m.group(1), "model": m.group(2),
                                     "task_type": m.group(3), "count": int(float(m.group(4)))})

    # 缓存命中
    cache_hits = 0
    for line in metrics_text.split("\n"):
        if "litellm_cache_hits_metric_total" in line and not line.startswith("#"):
            m = re.search(r'\}\s+(\S+)', line)
            if m:
                cache_hits += int(float(m.group(1)))

    # 花费 — 优先从 LiteLLM 数据库查询累计值（Prometheus Counter 在 worker 重启时会重置）
    total_spend = 0.0
    model_spend = {}
    try:
        spend_resp = api_get(f"{ADMIN_URL}/spend/logs", timeout=5)
        if isinstance(spend_resp, list):
            for entry in spend_resp:
                val = entry.get("total_spend", 0) or 0
                model = entry.get("model") or entry.get("model_group") or "unknown"
                total_spend += val
                model_spend[model] = model_spend.get(model, 0) + val
        elif isinstance(spend_resp, dict) and "data" in spend_resp:
            for entry in spend_resp["data"]:
                val = entry.get("total_spend", 0) or 0
                model = entry.get("model") or entry.get("model_group") or "unknown"
                total_spend += val
                model_spend[model] = model_spend.get(model, 0) + val
    except Exception:
        pass

    # 如果数据库查询失败，回退到 Prometheus 指标（仅当前进程生命周期）
    if total_spend == 0.0:
        spend_lines = [l for l in metrics_text.split("\n")
                       if "litellm_quota_key_spend_by_model_total" in l
                       and not l.startswith("#") and "_created" not in l]
        for line in spend_lines:
            m = re.search(r'model="([^"]*)".*\}\s+(\S+)', line)
            if m:
                val = float(m.group(2))
                total_spend += val
                model_spend[m.group(1)] = model_spend.get(m.group(1), 0) + val

    # 路由偏好表
    from task_orchestrator import TASK_MODEL_PREFERENCE
    task_labels = {"simple_qa": "简单问答", "general": "日常对话", "coding": "代码生成",
                   "math_logic": "数学推理", "complex_reasoning": "复杂分析"}
    routing_table = []
    for task_type, model_list_pref in TASK_MODEL_PREFERENCE.items():
        model = model_list_pref[0]
        pricing = MODEL_PRICING.get(model, {"label": ""})
        routing_table.append({"task_type": task_type, "label": task_labels.get(task_type, task_type),
                              "model": model, "pricing_label": pricing["label"]})

    # Ollama 状态
    ollama_base = os.environ.get("OLLAMA_API_BASE", "http://host.docker.internal:11434")
    try:
        with urllib.request.urlopen(f"{ollama_base}/api/tags", timeout=3) as resp:
            ollama_data = json.loads(resp.read().decode())
            ollama_models = [m["name"] for m in ollama_data.get("models", [])]
            ollama_ok = True
    except Exception:
        ollama_models = []
        ollama_ok = False

    # Semantic Router 状态
    sr_ok = False
    sr_stats = {}
    try:
        sr_resp = api_get(f"{SR_URL}/stats", timeout=5)
        if "error" not in sr_resp:
            sr_ok = True
            sr_stats = sr_resp
    except Exception:
        pass

    return jsonify({
        "worker_ok": worker_ok,
        "model_count": len(models),
        "models": model_list,
        "cache_hits": cache_hits,
        "total_spend": round(total_spend, 6),
        "model_spend": {k: round(v, 6) for k, v in model_spend.items()},
        "router_stats": router_stats,
        "routing_table": routing_table,
        "pricing_table": {k: {"input": v["input"], "output": v["output"], "label": v["label"]}
                          for k, v in MODEL_PRICING.items()},
        "ollama_ok": ollama_ok,
        "ollama_models": ollama_models,
        "sr_ok": sr_ok,
        "sr_stats": sr_stats,
    })


# ==================================================
# API: 配额管理
# ==================================================

@app.route("/api/quota")
def get_quota():
    """获取配额使用情况：用户预算 + 虚拟密钥列表"""
    import re

    # 用户信息
    user_info = api_get(f"{ADMIN_URL}/user/info?user_id=dev-user", timeout=5)
    user_data = {
        "user_id": user_info.get("user_id", "dev-user"),
        "spend": user_info.get("spend") or 0,
        "max_budget": user_info.get("max_budget") or 0,
        "budget_duration": user_info.get("budget_duration") or "30d",
    }

    # 密钥列表
    try:
        req = urllib.request.Request(f"{ADMIN_URL}/key/list?return_full_object=true",
                                      headers={"Authorization": f"Bearer {API_KEY}"})
        with urllib.request.urlopen(req, timeout=10) as resp:
            keys_data = json.loads(resp.read().decode())
    except Exception:
        keys_data = {"keys": []}

    keys = []
    for k in keys_data.get("keys", []):
        if isinstance(k, dict):
            spend = k.get("spend") or 0
            max_budget = k.get("max_budget") or 0
            keys.append({
                "key_alias": k.get("key_alias", "unknown"),
                "spend": round(spend, 6),
                "max_budget": max_budget,
                "budget_duration": k.get("budget_duration") or "30d",
                "models": k.get("models", []),
                "rpm_limit": k.get("rpm_limit"),
                "tpm_limit": k.get("tpm_limit"),
                "user_id": k.get("user_id", ""),
                "progress": round((spend / max_budget * 100), 1) if max_budget else 0,
            })

    return jsonify({
        "user": user_data,
        "keys": keys,
        "total_keys": len(keys),
    })


@app.route("/api/quota/user", methods=["POST"])
def update_user_quota():
    """更新用户预算"""
    data = request.json
    max_budget = data.get("max_budget")
    budget_duration = data.get("budget_duration", "30d")
    if max_budget is None:
        return jsonify({"error": "缺少 max_budget"}), 400

    result = api_post(f"{ADMIN_URL}/user/update", {
        "user_id": "dev-user",
        "max_budget": float(max_budget),
        "budget_duration": budget_duration,
    })
    if "error" in result:
        return jsonify(result), 500
    return jsonify({"ok": True})


@app.route("/api/quota/key", methods=["POST"])
def update_key_quota():
    """更新密钥预算"""
    data = request.json
    key_alias = data.get("key_alias")
    max_budget = data.get("max_budget")
    if not key_alias or max_budget is None:
        return jsonify({"error": "缺少 key_alias 或 max_budget"}), 400

    result = api_post(f"{ADMIN_URL}/key/update", {
        "key_alias": key_alias,
        "max_budget": float(max_budget),
    })
    if "error" in result:
        return jsonify(result), 500
    return jsonify({"ok": True})


# ==================================================
# API: 服务日志（错误诊断）
# ==================================================

@app.route("/api/service-logs/<service_name>")
def get_service_logs(service_name):
    """获取指定服务的最近日志"""
    import subprocess
    try:
        result = subprocess.run(
            ["docker", "compose", "logs", "--tail=50", "--no-color", service_name],
            capture_output=True, text=True, timeout=15,
            cwd="/app" if os.path.exists("/app/.env") else "."
        )
        logs = result.stdout if result.stdout else result.stderr
        return jsonify({"logs": logs[-8000:] if len(logs) > 8000 else logs})
    except Exception as e:
        return jsonify({"error": str(e)}), 500


# ==================================================
# API: 历史趋势数据（基于 Prometheus 查询）
# ==================================================

@app.route("/api/trends")
def get_trends():
    """查询 Prometheus 获取历史趋势数据"""
    import re
    try:
        with urllib.request.urlopen(f"{WORKER_URL}/metrics/", timeout=5) as resp:
            metrics_text = resp.read().decode()
    except Exception:
        metrics_text = ""

    # 按模型的花费趋势（累积值）
    model_spend = {}
    for line in metrics_text.split("\n"):
        if "litellm_quota_key_spend_by_model_total" in line and not line.startswith("#") and "_created" not in line:
            m = re.search(r'model="([^"]*)".*\}\s+(\S+)', line)
            if m:
                model_spend[m.group(1)] = model_spend.get(m.group(1), 0) + float(m.group(2))

    # 按任务类型的路由次数
    route_counts = {}
    for line in metrics_text.split("\n"):
        if "litellm_task_router_classification_total" in line and not line.startswith("#"):
            m = re.search(r'task_type="([^"]*)".*\}\s+(\S+)', line)
            if m:
                tt = m.group(1)
                route_counts[tt] = route_counts.get(tt, 0) + int(float(m.group(2)))

    # 按 method 的分类统计
    method_counts = {}
    for line in metrics_text.split("\n"):
        if "litellm_task_router_classification_total" in line and not line.startswith("#"):
            m = re.search(r'method="([^"]*)".*\}\s+(\S+)', line)
            if m:
                method = m.group(1)
                method_counts[method] = method_counts.get(method, 0) + int(float(m.group(2)))

    return jsonify({
        "model_spend": {k: round(v, 6) for k, v in model_spend.items()},
        "route_counts": route_counts,
        "method_counts": method_counts,
    })


# ==================================================
# 入口
# ==================================================

if __name__ == "__main__":
    app.run(host="0.0.0.0", port=5000, debug=False)
