# LLooM v2 项目进度

> 最后更新：**2026-08-26** · 仓库 `citrus-dot/LLooM` · 分支 `v2` · 工作目录 `/Users/orange/LLooMv2`
> 最新已提交：`8dddc59`（"P0.b/c/d: registry-driven plan() scoring router"）
> 工作区状态：干净。本日新增 commit：`b09d229`（定价/探针/信号）→ `09480fa`（P0.a 断言）→ `8dddc59`（P0.b/c/d 路由重构）。

---

## 一、项目定位与架构

基于 LiteLLM 的智能 LLM 路由代理 / 管理工具，核心卖点：控制面/数据面分离 + 成本优化 + 安全可控。

**三大核心目标**：
1. 集约化管理模型及 token 用量 — 模型注册、用量追踪、成本计算、预算控制
2. 根据用量智能规划调用 — 两层分类路由、Fallback 容灾、成本感知选模型
3. 语义感知分配任务 — 复杂任务分解、语义缓存、域分类增强

**当前架构**（纯 Rust + Python 微服务，零 Docker）：

| 层 | 技术栈 | 端口 | 说明 |
|----|--------|------|------|
| Rust 主服务 | `lloom-server` (axum) | :7861 | REST + 静态托管 WebUI，拉起 AI 服务与 Ollama |
| Python AI 微服务 | `api/ai_service.py` (FastAPI + litellm) | :7862 | 无状态，只调 litellm，业务逻辑在 Rust 侧 |
| 本地模型 | Ollama | :11434 | 可选，简单问题走本地 |
| WebUI | `webui/` (React + AntD) | :7861 | 总览/用量/对话/模型/设置 |
| CLI / TUI | `lloom-cli` (Rust) / `tui/` (SolidJS + OpenTUI) | — | 共享同一 REST 契约 |

数据：`data/lloom.db`（SQLite）+ `data/conversations/*.json`。密钥：`.env`。
**已发布**：v2.0.0、v2.1.0（GitHub Release）。代码已远超此版本，但未再发布新 tag。

**Rust 核心模块（`crates/lloom-core/src/`）**：
- `server.rs` — axum REST 服务器（全部端点 + SSE）
- `db.rs` — SQLite 层（`Model`/`Budget`/`UsageStats`/`PriceSpec`，含幂等迁移 `migrate_db`）
- `router.rs` — 分类（正则+LLM）+ band 投影 + **`plan()` 评分路由**（P0.d 已落地：注册表门槛+评分，成本走 price_specs）
- `security.rs` — PII 检测 / 越狱拦截 / 领域分类
- `ai_client.rs` — Python AI 微服务 async HTTP 客户端（已支持 `UsageDetail` 透传）
- `processes.rs` — 子进程管理（AI 服务 / Ollama）
- `conversations.rs` — 对话 CRUD（JSON 文件，原子写 + 追加端点）
- `models.rs` / `config.rs` / `error.rs` — 类型 / 配置 / 错误
- **`pricing.rs`**（新增，未提交）— 定价引擎：PriceSpec / TierBand / ZoneRule / UsageDetail / ZoneResolver + actual_cost / est_cost / effective_input_cost
- **`probe.rs`**（新增，未提交）— 常开探针：ProbeBudget 预算状态机 + 探针循环
- **`signals.rs`**（新增，未提交）— 信号层：`prefix_stability` 等启发式信号

---

## 二、关键技术决策

| 决策点 | 选择 | 原因 |
|--------|------|------|
| LLM 调用 | litellm SDK（PyPI 安装，非本地源码）| 去 Docker 代理，少依赖 |
| 数据库 | SQLite | 本地零配置 |
| 语义缓存 | ChromaDB（PersistentClient）+ 新增 L1 精确缓存（SQLite `cache_exact`）| 两层缓存，跨对话共享 FAQ |
| GUI → 前端 | React WebUI + Rust 无头服务 | 控制面/数据面分离 |
| API 框架 | FastAPI + axum | 原生 async + SSE |
| 分支策略 | v2 独立开发，旧版存 `legacy` 分支 | 保留历史 |
| 定价引擎 | Rust `pricing.rs` 单一计价真源，Python 只透传 usage | 消除双真源、量纲统一 USD/token |
| 时段计算 | 纯标准库（+8 偏移 + Sakamoto 星期 + 公历换算），**不引 chrono** | 规避受限网络拉 crates.io 失败 |
| 缓存键 | `hash(model + system_prompt版本 + context_fingerprint + cache_key)` | 上下文相关查询不跨会话命中 |

---

## 三、计划文档索引（三份详细设计，均已编写，状态不同）

| 文档 | 编写时间 | 提交状态 | 内容范围 | 落地状态 |
|---|---|---|---|---|
| [`CONTEXT-PLAN.md`](./CONTEXT-PLAN.md) | 2026-08-24 | **已提交**（a2b8bb5）| 上下文优化：SQLite 对话存储、预算上下文、两层缓存、原子写、两阶段落盘 | Phase 1 + 部分 Phase 4 已落地（追加端点存在于 server.rs）|
| [`PRICING-PLAN.md`](./PRICING-PLAN.md) | 2026-08-24 编写，持续更新 | **未提交**（工作区 M）| 定价表系统 PriceSpec（分项×时段×阶梯×来源）、pricing.rs 引擎、校准 job、探针系统 | 后端 PR-1~PR-7 已落地（未提交）；PR-5/PR-8 待办；WebUI 定价页/探针视图待做 |
| [`ROUTING-PLAN.md`](./ROUTING-PLAN.md) | v3（2026-08-24）+ v4 注记 | **已提交** | 路由重构：消除硬编码、注册表驱动、信号—投影—决策管线、预算联动 | **P0.a/P0.b/P0.c/P0.d 已落地**（09480fa + 8dddc59）；P0.f（Python 真源）、P1+ 待办 |

> 三份计划文档是详细设计与落地顺序的权威来源。本进度文档只做索引与高层同步，不重复其细节。

---

## 四、已落地功能进展（commit / 工作区视角）

**已提交 commit 主线**（最新在前）：
- `8dddc59` ROUTING P0.b/c/d：models 元数据列+迁移回填、routing_policy/model_task_score/routing_decisions 三表、plan() 评分路由替换全部硬编码（chat 路径）
- `09480fa` ROUTING P0.a：models 表单价写入断言 [1e-9,1e-3] + 单测
- `b09d229` Pricing engine, usage telemetry, probes（PRICING-PLAN PR-1~7）
- `a2b8bb5` Context optimization：SQLite conversation store、budgeted context、two-layer cache
- `dc534f0` Sync CLI/TUI with WebUI streaming, fix minor bugs, consolidate docs
- `091dc31` Semantic cache optimization
- `433362e` Security fixes, graceful shutdown, cache pre-init, rename & markdown

**2026-08-26 落地（ROUTING-PLAN P0.a/b/c/d，均已提交并冒烟验证）**：
- P0.a（09480fa）：`db.rs` `validate_cost` 断言进 `insert_model`/`update_model`（单价 0 或 [1e-9,1e-3] USD/token）
- P0.b（8dddc59）：models 表 +11 元数据列（capability_tier/quality_score/context_window/supports_stream/health_state/needs_calibration 等），幂等迁移 + 一次性名称启发式回填（settings 标记 `migration_routing_meta_v1`；备份 `data/lloom.db.pre-routing-migration.bak`）
- P0.c（8dddc59）：routing_policy（7 条种子策略）/model_task_score（EWMA α=0.15 回填函数）/routing_decisions（审计+outcome 回填）/routing_calibration 四表 + CRUD
- P0.d（8dddc59）：**删 `TASK_MODEL_MAP`/`INFERENCE_MODELS`/`select_model`/`task_model_preference` 全部硬编码**；`plan()` 门槛（tier/ctx/health/cost-cap/pinned）+ 加权评分（成本走 pricing.rs est_cost，质量 ewma≥5 样本覆盖冷启动分，needs_calibration 罚 0.3）；`chat_stream` 删伪造空 spec（direct 未注册→明确 SSE 报错）；`pick_classifier` 注册表驱动；审计落库；router 11 单测全过
- 冒烟：simple_qa→qwen2.5-local（cost 0）+fallback 链；coding→deepseek-v3(tier3,stream)；routing_decisions 2 条 outcome=success

**过往已实现并验证**（见 memory / 历史 commit）：
- 编辑对话名称（`rename_conversation`，PUT `/api/conversations/{id}`）
- 三大安全修复：SQL 注入列名白名单、路径穿越 id 白名单、密钥脱敏
- 优雅退出：SIGINT/SIGTERM + `POST /api/shutdown`
- SemanticCache 预初始化 + `_cache_ready` 门控（解决 chroma 79MB 模型下载挂死）
- CLI/TUI 同步 WebUI 真流式输出、模型标注、对话重命名

---

## 五、安全与健壮性（已修复并验证）

- **SQL 注入**：`db.rs update_model` 列名白名单，非法 key 拒绝。
- **路径穿越**：`conversations.rs validate_id` 校验 `id ∈ [A-Za-z0-9_-]`。
- **密钥泄露**：`get_config` 对 `*_API_KEY/_KEY/_TOKEN/_SECRET` 脱敏为 `****+后4位`，设置页不预填。
- **优雅退出**：SIGINT/SIGTERM 信号处理 + `POST /api/shutdown`，子进程全清理，杜绝端口残留。
- **用量/成本真实落库**：`chat_stream` 与 `orchestrate_stream` 均 `insert_usage`（PR-1 修复原断链 bug）。
- **原子写对话**：`conversations.rs` 写 `{id}.json.tmp` → fsync → rename，崩溃不损坏。

---

## 六、待办事项（TODO）

按优先级：`🔥` 高/安全，`⚡` 体验，`🔧` 优化。

### 来自 PRICING-PLAN.md
- [ ] 🔶 **PR-5 路由衔接**：`effective_input_cost` 进 `plan()` 评分（依赖 ROUTING-PLAN P0.d）
- [ ] 🔶 **PR-6 WebUI 定价页**：`GET /api/pricing/specs` 等后端已就绪，前端定价/校准视图未做
- [ ] 🔶 **PR-7 探针视图**：`GET /api/probe/stats` 后端已就绪，用量页探针视图未做
- [ ] ⏳ **PR-8 峰谷调度**：可延迟任务挪谷时（依赖 ROUTING-PLAN P4）

### 来自 ROUTING-PLAN.md（v3，P0.a/b/c/d 已于 2026-08-26 完成）
- [x] ✅ **P0.a 量纲写入断言**（09480fa）
- [x] ✅ **P0.b/c 元数据列 + 策略/审计表**（8dddc59）
- [x] ✅ **P0.d 路由重构**：`plan()` 评分路由已替换全部硬编码（chat 路径），11 单测 + 冒烟过（8dddc59）
- [ ] 🔥 **P0.f 消除 Python 真源**：orchestrate 路径仍走 `ai_service.py` 的 `TASK_MODEL_PREFERENCE`（双真源剩一半）
- [ ] ⚡ **P1.a 用量落库完善**：chat 路径已落库，orchestrate 的 `task_type`/`latency`/`request_id` 补全
- [ ] ⚡ **P0.e 增删模型自动打标**（metadata.rs 新模块，尚未创建）
- [ ] ⚡ **P0.g 信号层正规化**：signals.rs 已落地 `prefix_stability`，其余信号待补
- [ ] 🔧 **P3 健康感知与故障转移**、**P4 编排升级**、**P5 预算联动**

### 来自 CONTEXT-PLAN.md
- [ ] ⚡ **Phase 2 上下文架构迁移**：前端只发 (conversation_id, query)，Rust 构建历史
- [ ] ⚡ **Phase 3 压缩**：tiktoken 预算器、L1 截断、L2 滚动摘要
- [ ] ⚡ **Phase 4 缓存增强**：L1 精确缓存表、上下文无关判别器、单例化、淘汰线程、看板
- [ ] ⚡ **Phase 5 供应商前缀缓存验证**

### 历史遗留（LLooMprogress 原 TODO）
- [ ] 🔧 **O2**：`main.rs` 绑定 `0.0.0.0`，建议改 `127.0.0.1`（本地工具；涉及局域网访问，需确认后再改）
- [ ] ⚡ **O5 复杂判定调优**：多对象比较检测 + 多模型轮询分配已落地，判定边界/过度触发仍需真实语料打磨
- [ ] ⚡ **O6 子任务并行**：无依赖子任务改并行提速
- [ ] 🔧 多模型拆分需配置「可用模型 + 有效 Key」才真正生效
- [ ] 🔧 思考过程深度展示（可选）

---

## 七、重要问题记录（保留有长期价值的技术教训）

### 1. 语义缓存模型下载卡死
- **根因**：ChromaDB 的 `ONNXMiniLM_L6_V2` 默认从 **AWS S3** 拉 ~79MB 模型，**不是** HuggingFace；`HF_ENDPOINT` 对 S3 下载是 **no-op**。受限网络下 S3 直连 ~6KB/s，卡死数小时。
- **解决**：`api/embedding_model.py` 自己预置 6 个文件（sha256 清单 + 多镜像 hf-mirror/modelscope/huggingface 自动选最快 + 断点续传 + 原子落盘）。冷启动 86.9MiB/13s。量化 int8 实测中文语义坍缩（无关相似度 0.516 ≫ 0.3），**保持 fp32**。

### 2. 定价 10 倍量纲错误
- **根因**：DashScope 系 DB 单价 = 官方元/M × 1.3889e-06（=10÷7.2÷1e6），虚高 10×；gpt-4o 正确。导致跨供应商比价方向反了。
- **解决**：`migrate_db()` 对 `provider='dashscope'` 的单价 ÷10；录入端加 `[1e-9,1e-3] USD/token` 写入断言防复发（ROUTING-PLAN P0.a / PRICING-PLAN PR-2）。

### 3. 缓存命中率自校准（为什么要问「灰区未命中」）
- 若只在命中时收集标签，样本全在阈值之上，Youden's J 会随阈值降低单调增大，把阈值压到 0.70 地板引发大量误命中。
- 因此在灰区未命中（sim 距阈值 ≤0.06）补问「与之前问过的相似吗？」，提供阈值下方负样本，使调优可上下收敛。硬约束 FPR≤1%、clamp 0.70–0.92。

### 4. 失败子任务防幻觉
- 子任务失败时若仍把「执行失败: …」喂给汇总模型，模型会编造「子任务X因API错误中断」甚至生成不存在的测试脚本。
- **解决**：`task_done` 带 `error` 字段；只要有失败子任务就直接拼接失败信息作答，**不再调用汇总模型**；`AGGREGATE_SYSTEM_PROMPT` 加硬约束禁编造。

### 5. 路由双真源（当前最大技术债）
- 现状：Rust 侧 `TASK_MODEL_MAP` 硬编码 + Python 侧 `TASK_MODEL_PREFERENCE` 另一份硬编码，两份互不相通且都不读 DB、不看 is_active。删模型后路由名找不到会伪造空 spec 直接失败。
- **进展**：`plan()` 评分路由已落地（P0.d），Rust chat 路径单一决策；仅剩 orchestrate 路径的 Python `TASK_MODEL_PREFERENCE`（P0.f 待办）。

---

## 八、关键约束（勿踩坑）

- **不可删** `data/`（真实数据）、`.env`（密钥）；可删可重建 `target/`、`build/`、`dist/`、`.venv/`、`node_modules/` 等。
- v2 与 Docker 完全解耦；旧 Docker 栈仅存 `legacy` 分支。
- **网络受限**：官方源（bun.sh/GitHub releases/huggingface/ChromaDB S3）常下载不动；统一镜像 —— npm/bun→npmmirror、pip→清华、Ollama→ghproxy.net、embedding→hf-mirror/modelscope。`HF_ENDPOINT` 对 ChromaDB S3 下载无效。
- `api/` 无 `__init__.py`，需 `pip install -e .` 才能被 `uvicorn api.ai_service:app` 导入。
- `.env.example` 的 `LLOOM_API_PORT=7860` 是旧残留，实际端口由 `LLOOM_WEB_PORT`（默认 7861）控制。
- 对话工具内 `nohup` 启动的进程跨工具调用会被回收，不能持久；持久运行用 `.command` 或系统服务。
- **改 Python(`api/ai_service.py`) 后必须重启 Rust 服务**才重拉 AI 服务（既有踩坑）。
- 所有 `ALTER TABLE` 迁移先 `PRAGMA table_info` 去重、迁移前备份 `data/lloom.db`（定价迁移已备份 `data/lloom.db.pre-pricing-migration.bak`）。

---

## 九、开发环境

- Python：`.venv/`（3.13.12），`pip install -e ".[dev]"`（清华镜像）。
- Rust：`cargo build -p lloom-server`（⚠️ 当前未提交改动含新模块，提交前必须先 `cargo build` 验证编译）。
- WebUI：`cd webui && npm install && npm run build`（根 `.npmrc` 已固定 npmmirror）。
- TUI：`cd tui && bun install && bun run build`（需 `bun`，可走 npmmirror CDN 镜像）。
- 端口：服务器 :7861、AI 服务 :7862、Ollama :11434。
- 启动：`./start-lloom.command`（orange 目录下测试用脚本，非软件界面功能）。

---

## 十、文档地图（所有 md 同步状态）

| 文件 | 用途 | 同步状态 |
|---|---|---|
| `LLooMprogress.md`（本文件）| 项目总进度、决策、待办、约束、文档索引 | 本次大幅更新至 2026-08-26 |
| `ARCHITECTURE.md` | 分层架构、端点、数据流、技术栈 | 已补新模块/新路由（本次）|
| `README.md` / `README-ZH.md` | 用户文档（功能、快速开始、配置）| 已补新模块/新路由（本次）|
| `CONTEXT-PLAN.md` | 上下文优化方案（已落地部分）| 已提交，与 a2b8bb5 一致 |
| `PRICING-PLAN.md` | 定价表系统详细设计与落地 | 未提交（工作区 M），内容新且准 |
| `ROUTING-PLAN.md` | 路由重构方案（v3+v4 注记）| 已提交，P0.a/b/c/d 落地状态已更新 |
| `ROUTING-PLAN.md` 引用的外部研究 | Switchyard / vLLM Semantic Router / Router-R1 等 | 设计借鉴，不引入依赖 |

> **接手检查清单**：① 读 CONTEXT/PRICING/ROUTING-PLAN 三份；② 从 ROUTING-PLAN P0.f（Python 真源消除）、P0.e（新增模型自动打标）或 PRICING-PLAN PR-6（定价页）任一未做项切入。
