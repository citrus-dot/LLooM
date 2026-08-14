# LLooM v2 重构进度文档

> 本文档记录 v2 重构的完整规划、待办事项和具体进度，随开发过程持续更新。

---

## 一、项目概述

**项目名称**：LLooM v2 — Core + GUI 两层自包含桌面应用

**核心目标**：
1. **集约化管理模型及其 token 使用量** — 模型注册、用量追踪、成本计算、预算控制
2. **根据用量智能规划调用过程** — 两层分类路由、Fallback 容灾、成本感知选模型
3. **通过语义感知合理化分配任务** — 复杂任务分解、语义缓存、域分类增强

**架构**：Python 核心（litellm SDK 作为库）+ Tauri 桌面应用，零外部基础设施
- 嵌入式 Python 3.11 运行时（用户无需安装 Python）
- Ollama 二进制内置（用户无需安装 Ollama）
- SQLite 替代 PostgreSQL，ChromaDB 替代 Qdrant
- 完整文档见 `LLooM-v2-重构计划.docx`

**仓库**：`citrus-dot/LLooM` 仓库 `v2` 分支

---

## 二、完整开发计划

| Phase | 内容 | 依赖 | 预估时间 | 状态 |
|-------|------|------|----------|------|
| Phase 0 | 项目骨架 + 打包验证 | 无 | 1-2 天 | 进行中 |
| Phase 1 | ModelManager（模型注册/用量追踪/预算）| Phase 0 | 2-3 天 | 待开始 |
| Phase 2 | SmartRouter（两层分类/Fallback/成本感知）| Phase 1 | 2-3 天 | 待开始 |
| Phase 3 | Orchestrator + 语义缓存（ChromaDB）| Phase 2 | 3-4 天 | 待开始 |
| Phase 4 | 安全层（PII/越狱/域分类）| Phase 2（可并行）| 1-2 天 | 待开始 |
| Phase 5 | API 服务层（FastAPI + SSE）| Phase 1,2,3 | 2-3 天 | 待开始 |
| Phase 6 | CLI 工具（init/model/status/chat）| Phase 1（可并行）| 1-2 天 | 待开始 |
| Phase 7 | Tauri GUI + 进程管理 | Phase 5 | 3-5 天 | 待开始 |
| Phase 8 | 打包构建（PyInstaller + Ollama + Tauri）| Phase 7 | 2-3 天 | 待开始 |
| Phase 9 | 集成测试 + 文档 + GitHub Release | Phase 8 | 2-3 天 | 待开始 |

**总预估**：17-30 天

**可并行时间段**：
- Phase 4（安全层）与 Phase 3（编排器）可同时开发
- Phase 6（CLI）与 Phase 5（API）可同时开发

---

## 三、待办事项

### Phase 0: 项目骨架 + 打包验证 ✅ 已完成
- [x] 创建项目目录结构（core/ api/ cli/ tauri-app/ data/）
- [x] 创建 `pyproject.toml` 依赖定义
- [x] 创建 `.env.example` 模板
- [x] 创建 `.gitignore`
- [x] 创建最小 `api/server.py`（health 端点）
- [x] 创建 `core/__init__.py` + `core/config.py` + `core/database.py` 骨架
- [x] 创建 `cli/lloom.py` 入口骨架
- [x] 验证：SQLite 数据库初始化成功（3 张表：models/usage_records/budgets）
- [x] 验证：FastAPI app 创建成功，/api/health 端点可用
- [x] 验证：CLI 入口可运行（init/serve/model 命令）
- [ ] 创建 `v2` 分支（待用户确认后执行）
- [ ] PyInstaller 打包验证（待 litellm/chromadb 依赖安装后执行）
- [ ] Tauri 拉起打包二进制验证（待 Phase 7）

### Phase 1: ModelManager
- [ ] SQLite schema 建表（models / usage_records / budgets）
- [ ] `core/database.py` CRUD 操作
- [ ] `core/model_manager.py` ModelManager 类
- [ ] `core/callbacks.py` UsageTrackerCallback
- [ ] 从 v1 config_worker.yaml 迁移模型定价数据
- [ ] 单元测试

### Phase 2: SmartRouter
- [ ] `core/smart_router.py` SmartRouter 类
- [ ] 两层分类器（正则规则 + LLM 兜底）
- [ ] 从 v1 迁移 TASK_MODEL_MAP / INFERENCE_MODELS
- [ ] litellm.Router 集成 + Fallback 链构建
- [ ] 推理模型自动流式
- [ ] 单元测试

### Phase 3: Orchestrator + 语义缓存
- [ ] `core/cache.py` SemanticCache（ChromaDB）
- [ ] `core/orchestrator.py` TaskOrchestrator
- [ ] 从 v1 迁移 is_complex() / decompose() / aggregate() prompt
- [ ] 流式 SSE 事件输出
- [ ] 域分类集成
- [ ] 集成测试

### Phase 4: 安全层
- [ ] `core/security.py` PII 检测（7 类正则）
- [ ] 越狱拦截（5 类模式）
- [ ] 域分类（MMLU 14 类关键词 + LLM 兜底）
- [ ] 从 v1 semantic_router/app.py 迁移正则规则

### Phase 5: API 服务层
- [ ] `api/server.py` FastAPI 应用
- [ ] REST 端点（models/usage/budget/config/health/stats）
- [ ] SSE 端点（chat/stream, orchestrate/stream）
- [ ] 对话历史 CRUD（JSON 文件）
- [ ] CORS 配置
- [ ] 端到端测试

### Phase 6: CLI 工具
- [ ] `cli/lloom.py` click 框架
- [ ] init 命令（交互式向导）
- [ ] model add/remove/list 命令
- [ ] status 命令
- [ ] chat / orchestrate 命令
- [ ] serve 命令

### Phase 7: Tauri GUI
- [ ] 从 v1 迁移 tauri-app/ 目录
- [ ] main.rs 重写进程管理（拉起 Ollama + Python API）
- [ ] index.html 改 API URL（:7860）
- [ ] 总览页：docker compose → API 调用
- [ ] 用量页：Prometheus → API + SVG 图表
- [ ] 对话页：SSE URL 改为 localhost:7860
- [ ] 模型页：CLI JSON → REST API
- [ ] 设置页：.env 读写 → API 配置端点

### Phase 8: 打包构建
- [ ] PyInstaller 打包 Python 核心（hiddenimports 调试）
- [ ] 下载 Ollama macOS ARM64 二进制
- [ ] 配置 tauri.conf.json bundle.resources
- [ ] 首次运行模型拉取逻辑
- [ ] cargo tauri build 生成 .app
- [ ] 验证双击运行

### Phase 9: 集成测试 + 发布
- [ ] 全新机器端到端测试
- [ ] README.md 更新
- [ ] 项目文档.md 更新
- [ ] v2 分支合并到 main
- [ ] GitHub Release（tag v2.0.0）

---

## 四、进度记录

### 2026-08-14
- 完成需求确认：三个核心目标、Python核心+Tauri、零外部基础设施、新建v2分支
- 完成打包方案确认：嵌入式Python运行时 + Ollama二进制内置 + Tauri自动拉起进程
- 完成重构计划制定（10个Phase，17-30天预估）
- 生成 Word 格式介绍文档 `LLooM-v2-重构计划.docx`
- 创建进度文档 `progress.md`
- **Phase 0 完成**：项目骨架搭建
  - 创建目录结构 core/ api/ cli/ tauri-app/ data/
  - 创建 pyproject.toml（litellm/fastapi/uvicorn/chromadb/click/python-dotenv）
  - 创建 .env.example 和 .gitignore
  - 实现 core/config.py（配置管理：端口、DB路径、.env读写）
  - 实现 core/database.py（SQLite schema：models/usage_records/budgets 三张表）
  - 实现 api/server.py（FastAPI 最小服务，/api/health 端点）
  - 实现 cli/lloom.py（click CLI 框架，init/serve/model 命令）
  - 验证：DB 初始化成功、FastAPI app 创建成功、CLI 可运行
  - 待完成：v2 分支创建、PyInstaller 打包验证（需 litellm/chromadb 依赖）

---

## 五、关键技术决策记录

| 决策点 | 选择 | 原因 |
|--------|------|------|
| LLM 调用方式 | litellm Python SDK（库）| 去掉 Docker Proxy，减少依赖 |
| 数据库 | SQLite | 本地文件，零配置，够用 |
| 语义缓存 | ChromaDB | pip 安装即可，无需服务端 |
| Python 运行时 | 嵌入式（PyInstaller 打包）| 用户无需安装 Python |
| Ollama 集成 | 二进制内置打包 | 用户无需安装 Ollama |
| GUI 框架 | Tauri v2（复用 v1）| 已有 5 页面 UI 基础 |
| API 框架 | FastAPI | 原生 async + SSE 支持 |
| CLI 框架 | click | 比 argparse 更友好 |
| 分支策略 | 新建 v2 分支 | 保留 v1 git 历史 |
| Open WebUI | 不保留 | Tauri 内置对话页足够 |

---

## 六、v1 → v2 代码迁移清单

| v1 文件 | v2 归属 | 迁移方式 |
|---------|---------|----------|
| custom_callbacks.py 正则规则 | core/smart_router.py RULES | 直接复制 |
| custom_callbacks.py INFERENCE_MODELS | core/smart_router.py | 直接复制 |
| custom_callbacks.py QuotaTracker | core/callbacks.py | Prometheus → SQLite |
| task_orchestrator.py is_complex() | core/orchestrator.py | 直接复制 |
| task_orchestrator.py decompose() prompt | core/orchestrator.py | 直接复制，urllib → litellm |
| task_orchestrator.py aggregate() prompt | core/orchestrator.py | 直接复制 |
| semantic_router/app.py PII 正则 | core/security.py | 直接复制 |
| semantic_router/app.py 越狱正则 | core/security.py | 直接复制 |
| semantic_router/app.py MMLU 关键词 | core/security.py | 直接复制 |
| config_worker.yaml model_list | SQLite models 表 | 迁移数据 |
| config_worker.yaml fallbacks | SmartRouter._build_fallbacks() | 迁移逻辑 |
| litellm_cli.py init 向导 | cli/lloom.py init | 精简（去 Docker）|
| webapp/app.py API 端点 | api/server.py | Flask→FastAPI 重写 |
| tauri-app main.rs 进程管理 | main.rs | Docker→子进程 重写 |
| tauri-app index.html UI | index.html | 改 API URL，保留布局 |
