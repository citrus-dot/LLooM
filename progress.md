# LiteLLM 智能路由代理平台 — 项目进展

> **最后更新**: 2026-08-07  
> **项目路径**: `/Users/orange/litellm-install`  
> **当前状态**: Phase 1-5 全部完成，优化迭代中

---

## 项目阶段总览

| 阶段 | 名称 | 状态 | 完成时间 | 说明 |
|------|------|------|----------|------|
| Phase 1 | 基础部署 | ✅ 已完成 | 2026-07 | Docker 环境搭建、LiteLLM 部署、基础配置 |
| Phase 2 | 智能路由与配额 | ✅ 已完成 | 2026-07 | task_router 回调、配额追踪、Fallback 容灾 |
| Phase 3 | 缓存与监控 | ✅ 已完成 | 2026-07 | Qdrant 语义缓存、Prometheus+Grafana 监控 |
| Phase 4 | 编排与可视化 | ✅ 已完成 | 2026-08 | 任务编排引擎、网页管理端、SSE 流式对话 |
| Phase 5 | 安全与桌面应用 | ✅ 已完成 | 2026-08 | Semantic Router、Tauri App、配置生效自动化 |
| 优化迭代 | 持续优化 | 🔄 进行中 | — | UI 修复、上下文感知、缓存优化、文档 |

---

## 各阶段详细进展

### Phase 1: 基础部署 ✅

**目标**: 搭建 LiteLLM 基础环境，实现多模型代理

**完成内容**:
- [x] Docker Compose 编排（10 个服务）
- [x] 控制面 (Admin :4000) + 数据面 (Worker :4001) 分离架构
- [x] PostgreSQL 数据库配置
- [x] Redis 7 配置（密码认证、AOF/RDB 持久化、LRU 淘退、Lazyfree、危险命令禁用）
- [x] 模型列表配置（百炼 qwen 系列、deepseek-v3、Ollama 本地、OpenAI、Anthropic）
- [x] .env 环境变量管理
- [x] install.sh 一键安装脚本
- [x] litellm_cli.py CLI 工具（init/health/list-models）

**关键文件**:
- docker-compose.yml, config_admin.yaml, config_worker.yaml
- .env.example, redis.conf, prometheus.yml
- install.sh, litellm_cli.py

---

### Phase 2: 智能路由与配额管理 ✅

**目标**: 实现基于任务类型的智能路由和配额追踪

**完成内容**:
- [x] custom_callbacks.py — 智能任务路由回调 (task_router)
  - [x] 两层混合分类: 正则规则优先 → LLM 兜底
  - [x] 分类器自动选择: DASHSCOPE_API_KEY → qwen3.6-flash / 空 → qwen2.5:latest
  - [x] 5 种任务类型映射 (simple_qa/general/coding/math_logic/complex_reasoning)
  - [x] SR 域分类增强路由 (读取 X-SR-Domain 头)
- [x] custom_callbacks.py — 配额追踪回调 (quota_tracker)
  - [x] 按 Key + User + Model 维度追踪花费
  - [x] Prometheus Counter 指标导出
  - [x] 分类请求排除（避免双重计数）
- [x] 三层 Fallback 容灾链
  - [x] 云端高质量 → 云端降级 → 本地兜底 (qwen2.5-local)
- [x] 推理模型自动启用流式响应
  - [x] qwen3.6-flash/plus、qwen3-max、deepseek-v3 自动 stream=true
- [x] 预算管理
  - [x] 默认: max_budget=$10/30d
  - [x] 上限: max_budget=$1000/365d
- [x] CLI add-model 命令（交互式添加模型，自动更新 4 个文件）
- [x] quota_setup.sh 验证脚本

**关键文件**:
- custom_callbacks.py, litellm_cli.py (add-model 命令), quota_setup.sh

---

### Phase 3: 语义缓存与监控 ✅

**目标**: 实现语义缓存降本和可视化监控

**完成内容**:
- [x] Qdrant 语义缓存向量数据库
  - [x] DashScope text-embedding-v3 embedding 模型
  - [x] similarity_threshold: 0.95（防误命中）
  - [x] TTL: 86400 秒（24 小时自动过期）
  - [x] binary 量化（节省存储）
- [x] Prometheus 指标采集
  - [x] Admin (:4000) + Worker (:4001) 双端采集
  - [x] 15 秒采集间隔
  - [x] 自定义指标: 路由分类、配额追踪、缓存命中
- [x] Grafana 可视化面板
  - [x] LiteLLM Overview 仪表盘 (UID: litellm-overview)
  - [x] 配额与智能路由仪表盘 (UID: litellm-quota)
  - [x] YAML provisioning 自动加载
- [x] CLI status 命令（运行时状态查看）
- [x] CLI logs 命令（服务日志查看）

**关键文件**:
- config_worker.yaml (cache_params), prometheus.yml
- grafana/dashboards/*.json, grafana/provisioning/*.yml

---

### Phase 4: 任务编排与可视化管理 ✅

**目标**: 实现复杂任务自动分解和可视化管理界面

**完成内容**:
- [x] task_orchestrator.py — 复杂任务编排引擎
  - [x] 复杂度检测（多步骤关键词、长度、句子数）
  - [x] LLM 任务分解（2-5 个子任务，带依赖关系）
  - [x] 成本规划（为每个子任务选最优模型）
  - [x] 按序执行（前置结果作为上下文）
  - [x] 结果汇总（LLM 合并为连贯回答）
- [x] webapp/app.py — 网页端管理界面 (Flask, :3002)
  - [x] 仪表盘页面（服务状态、模型数、缓存命中、花费、路由统计）
  - [x] 智能对话页面（SSE 实时推送分解/执行/汇总过程）
  - [x] 配置向导页面（API Key 管理、路由策略表、定价表）
  - [x] 模型管理页面（模型卡片、定价信息）
  - [x] .env 读写 API（GET/POST /api/env）
  - [x] 用量统计 API（/api/stats, Tauri App 调用）
  - [x] 配额管理 API（/api/quota）
  - [x] 服务日志 API（/api/service-logs/<name>）
  - [x] 趋势数据 API（/api/trends）
- [x] Open WebUI 集成
  - [x] 连接 Semantic Router (:8888) 而非直连 Worker
  - [x] webui-init.sh 启动时注入配置
- [x] CLI orchestrate 命令

**关键文件**:
- task_orchestrator.py, webapp/app.py, webui-init.sh

---

### Phase 5: 安全检测与桌面应用 ✅

**目标**: 实现语义路由安全代理和 Tauri 桌面应用

**完成内容**:

#### 5.1 Semantic Router 安全代理
- [x] semantic_router/app.py — Flask 代理服务 (:8888)
  - [x] PII 检测（7 种类型: email/phone/SSN/credit_card/IP/ID_card/bank_account）
    - [x] 正则使用 lookbehind/lookahead 断言（支持中文）
    - [x] 默认 mask 动作
  - [x] 越狱检测（5 种攻击: DAN/instruction_override/role_manipulation/safety_bypass/prompt_injection）
    - [x] 默认 block 动作
  - [x] 域分类（MMLU 14 类）
    - [x] 关键词预筛（零成本）→ LLM 兜底
    - [x] 分类器自动选择（与 task_router 一致）
  - [x] 幻觉检测（LLM-based, 默认关闭）
  - [x] SR 响应头注入（X-SR-Domain/X-SR-PII-Types/X-SR-Jailbreak-Types）
  - [x] /check 端点（编排预检）
  - [x] /stats 端点（统计信息）
  - [x] 流式和非流式响应均返回 SR 头

#### 5.2 Tauri 桌面应用
- [x] tauri-app/src-tauri/src/main.rs — Rust 后端
  - [x] Docker 服务管理（启动/停止/重启/状态查询）
  - [x] .env 配置读写（read_env/write_env/write_env_batch）
  - [x] 用量统计/配额/趋势 API 代理
  - [x] SSE 聊天代理（chat_request, 解决混合内容拦截）
  - [x] 对话历史管理（list/load/save/delete conversations）
  - [x] 智能重启（smart_restart, 按变更键映射服务）
  - [x] 模型管理（get/add/remove models, 调用 CLI JSON 命令）
  - [x] 系统托盘
  - [x] PATH 环境增强（解决 macOS GUI 应用 PATH 精简问题）
  - [x] Python 环境清理（移除 PYTHONHOME/PYTHONPATH）
- [x] tauri-app/src-tauri/ui/index.html — 前端 UI
  - [x] 5 页面: 总览/用量/对话/模型管理/设置
  - [x] Markdown 渲染（标题/粗体/代码块/表格/列表/引用）
  - [x] 表格渲染修复（\x00 占位符处理 <br>）
  - [x] 可折叠区域（toggleCollapse 模式, 懒加载）
  - [x] SR 状态面板集成
  - [x] 对话历史列表和自动保存
  - [x] 模型管理页面（添加弹窗/删除确认/初始化向导）
  - [x] .env 配置编辑表单（分组可折叠）

#### 5.3 配置生效自动化
- [x] smart_restart 命令
  - [x] 变更键 → 受影响服务映射
  - [x] `docker compose up -d --force-recreate --no-deps <services>`
  - [x] 健康检查轮询（最多 60s）

#### 5.4 CLI 非交互式 JSON 命令
- [x] add-model-json（供 Tauri App 调用）
- [x] remove-model-json
- [x] list-models-json

**关键文件**:
- semantic_router/app.py, semantic_router/config.yaml
- tauri-app/src-tauri/src/main.rs, tauri-app/src-tauri/ui/index.html
- tauri-app/src-tauri/tauri.conf.json
- litellm_cli.py (JSON 命令)

---

## 优化迭代记录

### 优化 1: Markdown 渲染修复 ✅
- **问题**: AI 回复中 # * 符号原样显示，escapeHtml 转义了所有内容
- **解决**: 新增 renderMarkdown() 函数，AI 消息改用 renderMarkdown 替代 escapeHtml
- **文件**: tauri-app/src-tauri/ui/index.html

### 优化 2: 用量数据持久化 ✅
- **问题**: Prometheus Counter 在 Worker 重启时重置，用量数据被清空
- **解决**: /api/stats 端点优先从 LiteLLM Admin 数据库查询累计花费，Prometheus 作为回退
- **文件**: webapp/app.py

### 优化 3: SR 状态面板集成 ✅
- **问题**: SR 功能未在 Tauri App 中可视化展示
- **解决**: 总览页新增可折叠 SR 状态卡片，对话底部显示 SR 域分类信息
- **文件**: tauri-app/src-tauri/ui/index.html, webapp/app.py

### 优化 4: 表格渲染修复 ✅
- **问题**: 表格只渲染一行数据行，模型输出省略尾部管道符
- **解决**: 补全尾部管道 + \x00 占位符处理 <br> 标签
- **文件**: tauri-app/src-tauri/ui/index.html

### 优化 5: Python 环境冲突修复 ✅
- **问题**: Tauri App 调用 python3 时崩溃，TRAE 注入的 PYTHONHOME/PYTHONPATH 干扰
- **解决**: cmd() 函数中 env_remove("PYTHONHOME") 和 env_remove("PYTHONPATH")
- **文件**: tauri-app/src-tauri/src/main.rs

### 优化 6: auto 模型选项恢复 ✅
- **问题**: Open WebUI 模型选择器缺少 auto 选项
- **解决**: config_worker.yaml 中补充 auto 虚拟模型条目
- **文件**: config_worker.yaml

### 优化 7: 对话上下文感知 ✅
- **问题**: 对话不读取上下文，多轮对话不连贯
- **解决**:
  - 前端: sendChat() 构建对话历史（最近 10 轮）并传入 history 参数
  - 后端: /api/chat/stream 接收 history 参数，传递给 Orchestrator
  - 编排: orchestrate/execute_task/aggregate 方法接收 history，注入到 LLM messages
- **文件**: tauri-app/src-tauri/ui/index.html, webapp/app.py, task_orchestrator.py

### 优化 8: 语义缓存选择性启用 ✅
- **问题**: 所有 LLM 调用都设置 no-cache: True，缓存从未命中
- **解决**:
  - _call_llm 和 _call_llm_stream 新增 use_cache 参数
  - simple_qa/general 类型且无上下文时启用缓存
  - 复杂子任务禁用缓存
  - config_worker.yaml 设置 similarity_threshold: 0.95 + ttl: 86400
- **文件**: task_orchestrator.py, config_worker.yaml

### 优化 9: YAML 语法修复 ✅
- **问题**: config_worker.yaml fallbacks 中多余逗号导致 Worker 循环重启
- **解决**: 修正 YAML flow sequence 语法
- **文件**: config_worker.yaml

### 优化 10: PII 正则中文支持 ✅
- **问题**: \b word boundary 不支持中文字符，PII 检测在中文文本中失效
- **解决**: 使用 (?<!\d) 和 (?!\d) lookbehind/lookahead 断言替代 \b
- **文件**: semantic_router/app.py

### 优化 11: SR 响应头客户端可见 ✅
- **问题**: SR 头只在转发请求中添加，客户端响应中看不到
- **解决**: 在流式和非流式响应路径中都添加 SR 头到客户端响应
- **文件**: semantic_router/app.py

### 优化 12: 对话历史持久化 ✅
- **问题**: 切换页面后对话记录清空
- **解决**: 对话存储为 JSON 文件 (conversations/{id}.json)，Tauri 命令 CRUD，前端自动保存和加载
- **文件**: tauri-app/src-tauri/src/main.rs, tauri-app/src-tauri/ui/index.html

### 优化 13: 项目文档完善 (v2.0) ✅
- **目标**: 完善项目文档，加入重要改动和功能实现细节，方便后续工作交接
- **新增内容**:
  - 核心模块代码级实现注释（custom_callbacks / task_orchestrator / semantic_router / webapp / main.rs）
  - 关键技术决策说明（两层分类策略、流式自动启用、SR headers 双向注入、选择性缓存机制等）
  - 踩坑记录（PII 正则中文失效、Python 环境冲突、PATH 精简、Prometheus Counter 重置等）
  - 第 15 节: 技术参考与外部文档（LiteLLM 回调/缓存/Fallback、Tauri CSP、Qdrant 量化）
  - 对话上下文数据流图（前端 → Rust → Orchestrator → Worker 完整路径）
  - 前端 sendChat() 实现注释（对话历史收集与 SSE 代理调用）
- **文件**: 项目文档.md (v1.0 → v2.0), progress.md

---

## 当前服务状态

10 个 Docker 服务全部健康运行:

| 服务 | 容器名 | 状态 |
|------|--------|------|
| litellm-admin | litellm_admin | ✅ healthy |
| litellm-worker | litellm_worker | ✅ healthy |
| db | litellm_db | ✅ healthy |
| redis | litellm_redis | ✅ healthy |
| qdrant | litellm_qdrant | ✅ healthy |
| prometheus | litellm_prometheus | ✅ healthy |
| grafana | litellm_grafana | ✅ healthy |
| open-webui | litellm_webui | ✅ running |
| semantic-router | litellm_semantic_router | ✅ healthy |
| orchestrator-web | litellm_orchestrator | ✅ healthy |

---

## 项目文件清单

### 核心配置文件
| 文件 | 用途 | 修改频率 |
|------|------|----------|
| docker-compose.yml | Docker 服务编排 | 低 |
| config_admin.yaml | 控制面配置 | 低 |
| config_worker.yaml | 数据面配置（模型/路由/缓存/回调） | 中（添加模型时修改） |
| .env | 环境变量（API Key/密码） | 中（配置变更时修改） |
| .env.example | 环境变量模板 | 极低 |
| redis.conf | Redis 配置 | 极低 |
| prometheus.yml | Prometheus 采集配置 | 极低 |

### Python 核心模块
| 文件 | 用途 | 行数 |
|------|------|------|
| custom_callbacks.py | 智能路由 + 配额追踪回调 | ~450 |
| task_orchestrator.py | 复杂任务编排引擎 | ~520 |
| litellm_cli.py | CLI 管理工具 | ~1260 |
| webapp/app.py | 网页端管理界面 + API | ~1130 |
| semantic_router/app.py | Semantic Router 安全代理 | ~775 |

### Tauri 桌面应用
| 文件 | 用途 | 行数 |
|------|------|------|
| src-tauri/src/main.rs | Rust 后端（Docker/配置/对话/模型管理） | ~776 |
| src-tauri/ui/index.html | 前端 UI（5 页面） | ~2000+ |
| src-tauri/tauri.conf.json | Tauri 配置 | 65 |

### Shell 脚本
| 文件 | 用途 |
|------|------|
| install.sh | 一键安装 |
| quota_setup.sh | Phase 2 验证脚本 |
| webui-init.sh | Open WebUI 启动初始化 |
| build-package.sh | Tauri App 打包 |
| save-images.sh | Docker 镜像导出（离线部署） |
| load-images.sh | Docker 镜像导入（离线部署） |
| offline-install.sh | 离线安装 |

### Grafana 配置
| 文件 | 用途 |
|------|------|
| grafana/dashboards/litellm-overview.json | 总览仪表盘 |
| grafana/dashboards/litellm-quota.json | 配额与路由仪表盘 |
| grafana/provisioning/dashboards/dashboards.yml | 仪表盘自动加载 |
| grafana/provisioning/datasources/datasource.yml | 数据源配置 |

---

## 待办事项与未来规划

### 短期待办（近期优化）

- [ ] **幻觉检测启用测试**: 当前默认关闭，需要测试延迟影响后决定是否启用
- [ ] **缓存命中率监控**: 添加缓存命中率仪表盘到 Grafana
- [ ] **SR 统计持久化**: 当前 SR 统计在进程内存中，重启后丢失，考虑写入数据库
- [ ] **模型健康检查优化**: Tauri App 总览页添加模型可用性实时检测
- [ ] **对话导出功能**: 支持导出对话为 Markdown/JSON

### 中期规划

- [ ] **多用户支持**: Open WebUI 启用认证 (WEBUI_AUTH=true)，支持多用户隔离
- [ ] **API 调用日志**: 记录完整请求/响应日志到数据库，支持审计
- [ ] **模型 A/B 测试**: 支持同一任务类型配置多个模型进行对比
- [ ] **自定义插件系统**: SR 插件链支持动态加载自定义插件
- [ ] **缓存预热**: 对高频问题预生成缓存
- [ ] **成本告警**: 配额接近上限时自动通知

### 长期规划

- [ ] **Kubernetes 部署**: 将 Docker Compose 迁移到 K8s，支持水平扩展
- [ ] **多区域容灾**: 跨可用区部署，提高可用性
- [ ] **模型微调管理**: 集成模型微调管道
- [ ] **RAG 知识库**: 集成向量数据库实现检索增强生成
- [ ] **多模态支持**: 支持图片、音频、视频输入
- [ ] **Windows/Linux 支持**: Tauri App 跨平台编译

---

## 经验总结

### 技术经验

1. **控制面/数据面分离**: Admin 和 Worker 分离部署，职责清晰，互不影响
2. **规则优先于 LLM**: 正则规则分类零成本零延迟，应优先使用，LLM 仅作兜底
3. **推理模型流式响应**: qwen3.6-flash/plus 等推理模型生成 reasoning tokens 耗时长，必须启用流式避免 HTTP 超时
4. **语义缓存阈值**: similarity_threshold 必须 ≥ 0.95，低于此值会导致语义相似但含义不同的请求误命中
5. **Tauri PATH 问题**: macOS GUI 应用从 Finder/Dock 启动时 PATH 极其精简，必须手动注入常见二进制路径
6. **Tauri 混合内容**: tauri://localhost 是安全上下文，阻止 http:// 连接，必须通过 Rust 后端代理 SSE
7. **Redis 安全**: requirepass + 危险命令禁用 + protected-mode 三重防护
8. **模型名一致性**: TASK_MODEL_MAP/fallbacks/quota_setup.sh 中的模型名必须与 config_worker.yaml 完全一致

### 踩过的坑

1. **\b 不支持中文**: PII 正则使用 \b word boundary 在中文环境下失效，改用 lookbehind/lookahead
2. **YAML 多余逗号**: `[, "item"]` 语法导致 Worker 循环重启，需验证 YAML 语法
3. **Prometheus Counter 重置**: 进程内计数器重启后归零，需从数据库查询累计值
4. **Qdrant 镜像无 wget/curl**: Debian 13 镜像不含这些工具，健康检查使用 bash tcp 探测
5. **PYTHONHOME/PYTHONPATH 冲突**: TRAE 注入的 Python 环境变量导致系统 Python 崩溃
6. **缓存 401 阻断**: DASHSCOPE_API_KEY 为空时 embedding 调用 401 阻断主请求，需禁用缓存
7. **SSE 中文乱码**: 必须指定 `charset=utf-8`
8. **Docker Compose 工作目录**: Tauri App 中 docker compose 命令必须指定 current_dir

---

## 变更历史

| 日期 | 变更内容 | 文件 |
|------|----------|------|
| 2026-07 | Phase 1: 基础部署 | docker-compose.yml, config_*.yaml, .env, redis.conf, install.sh |
| 2026-07 | Phase 2: 智能路由与配额 | custom_callbacks.py, litellm_cli.py, quota_setup.sh |
| 2026-07 | Phase 3: 缓存与监控 | config_worker.yaml, prometheus.yml, grafana/ |
| 2026-08 | Phase 4: 编排与可视化 | task_orchestrator.py, webapp/app.py, webui-init.sh |
| 2026-08 | Phase 5: 安全与桌面应用 | semantic_router/, tauri-app/, litellm_cli.py |
| 2026-08 | 优化: Markdown 渲染 | tauri-app/src-tauri/ui/index.html |
| 2026-08 | 优化: 用量持久化 | webapp/app.py |
| 2026-08 | 优化: SR 面板集成 | tauri-app/src-tauri/ui/index.html, webapp/app.py |
| 2026-08 | 优化: 表格渲染 | tauri-app/src-tauri/ui/index.html |
| 2026-08 | 修复: Python 环境冲突 | tauri-app/src-tauri/src/main.rs |
| 2026-08 | 修复: auto 模型缺失 | config_worker.yaml |
| 2026-08 | 优化: 上下文感知 | index.html, app.py, task_orchestrator.py |
| 2026-08 | 优化: 选择性缓存 | task_orchestrator.py, config_worker.yaml |
| 2026-08 | 修复: YAML 语法 | config_worker.yaml |
| 2026-08 | 修复: PII 中文支持 | semantic_router/app.py |
| 2026-08 | 修复: SR 响应头 | semantic_router/app.py |
| 2026-08 | 优化: 对话历史持久化 | main.rs, index.html |
| 2026-08 | 文档: 项目文档 + 进展 | 项目文档.md, progress.md |
| 2026-08 | 文档 v2.0: 代码级实现注释 + 技术参考 | 项目文档.md |
