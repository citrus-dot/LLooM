# LLooM v2 项目进度

> 最后更新：**2026-09-02** · 仓库 `citrus-dot/LLooM` · 分支 `v2` · 工作目录 `/Users/orange/LLooMv2`
> 最新已提交：**N1 OpenAI 兼容代理**（含 O2 绑定收尾 + C3 api\_source 列，89 单测全绿）；此前 CONTEXT-PLAN 2–5 审计收尾（488d156）
> **下一阶段权威计划：[`NEXT-PLAN.md`](./NEXT-PLAN.md)**（✅ N1 代理接入 → N2 闭环评估 → N3 信任收尾 + 决策门 G1/G2）
> **待办台账**：主线见上方「下一阶段」；**搁置项（B 类 14 条）/ 独立小项（C 类 7 条）见** **[六、待办事项](#六待办事项todo)** **末尾两张台账表**

***

## 一、项目定位与架构

基于 LiteLLM 的智能 LLM 路由代理 / 管理工具，核心卖点：控制面/数据面分离 + 成本优化 + 安全可控。

**三大核心目标**：

1. 集约化管理模型及 token 用量 — 模型注册、用量追踪、成本计算、预算控制
2. 根据用量智能规划调用 — 两层分类路由、Fallback 容灾、成本感知选模型
3. 语义感知分配任务 — 复杂任务分解、语义缓存、域分类增强

**当前架构**（纯 Rust + Python 微服务，零 Docker）：

| 层             | 技术栈                                             | 端口     | 说明                                 |
| ------------- | ----------------------------------------------- | ------ | ---------------------------------- |
| Rust 主服务      | `lloom-server` (axum)                           | :7861  | REST + 静态托管 WebUI，拉起 AI 服务与 Ollama |
| Python AI 微服务 | `api/ai_service.py` (FastAPI + litellm)         | :7862  | 无状态，只调 litellm，业务逻辑在 Rust 侧        |
| 本地模型          | Ollama                                          | :11434 | 可选，简单问题走本地                         |
| WebUI         | `webui/` (React + AntD)                         | :7861  | 总览/用量/对话/模型/设置                     |
| CLI / TUI     | `lloom-cli` (Rust) / `tui/` (SolidJS + OpenTUI) | —      | 共享同一 REST 契约                       |

数据：`data/lloom.db`（SQLite）+ `data/conversations/*.json`。密钥：`.env`。
**已发布**：v2.0.0、v2.1.0（GitHub Release）。代码已远超此版本，但未再发布新 tag。

**Rust 核心模块（`crates/lloom-core/src/`）**：

- `server.rs` — axum REST 服务器（全部端点 + SSE）

- `db.rs` — SQLite 层（`Model`/`Budget`/`UsageStats`/`PriceSpec`，含幂等迁移 `migrate_db`）

- `router.rs` — 分类（正则+LLM）+ band 投影 + **`plan()`** **评分路由**（P0.d 已落地：注册表门槛+评分，成本走 price\_specs）

- `security.rs` — PII 检测 / 越狱拦截 / 领域分类

- `ai_client.rs` — Python AI 微服务 async HTTP 客户端（已支持 `UsageDetail` 透传）

- `processes.rs` — 子进程管理（AI 服务 / Ollama）

- `conversations.rs` — 对话 CRUD（JSON 文件，原子写 + 追加端点）

- `models.rs` / `config.rs` / `error.rs` — 类型 / 配置 / 错误

- **`pricing.rs`**（b09d229 已提交）— 定价引擎：PriceSpec / TierBand / ZoneRule / UsageDetail / ZoneResolver + actual\_cost / est\_cost / effective\_input\_cost

- **`probe.rs`**（b09d229 已提交）— 常开探针：ProbeBudget 预算状态机 + 探针循环

- **`signals.rs`**（b09d229 起步，d6912b9 补全 P0.g）— 信号层：`prefix_stability` + `SignalSet`（困难度/难度带/reask/LLM 判定）

- **`metadata.rs`**（d6912b9 已提交）— P0.e 模型元数据五级打标：`resolve_and_fill`（overlay > 启发式，供 `insert_model` 自动回填）

- **`health.rs`**（P3，2026-08-27）— 健康状态机：滑窗 degraded/连续失败 down/熔断/成功恢复，`set_model_health` 持久化

***

## 二、关键技术决策

| 决策点      | 选择                                                                | 原因                   |
| -------- | ----------------------------------------------------------------- | -------------------- |
| LLM 调用   | litellm SDK（PyPI 安装，非本地源码）                                        | 去 Docker 代理，少依赖      |
| 数据库      | SQLite                                                            | 本地零配置                |
| 语义缓存     | ChromaDB（PersistentClient）+ 新增 L1 精确缓存（SQLite `cache_exact`）      | 两层缓存，跨对话共享 FAQ       |
| GUI → 前端 | React WebUI + Rust 无头服务                                           | 控制面/数据面分离            |
| API 框架   | FastAPI + axum                                                    | 原生 async + SSE       |
| 分支策略     | v2 独立开发，旧版存 `legacy` 分支                                           | 保留历史                 |
| 定价引擎     | Rust `pricing.rs` 单一计价真源，Python 只透传 usage                         | 消除双真源、量纲统一 USD/token |
| 时段计算     | 纯标准库（+8 偏移 + Sakamoto 星期 + 公历换算），**不引 chrono**                    | 规避受限网络拉 crates.io 失败 |
| 缓存键      | `hash(model + system_prompt版本 + context_fingerprint + cache_key)` | 上下文相关查询不跨会话命中        |
| 配置层      | 优先级链 **CLI 参数 > 环境变量 > .env > 默认值**（`config::CliOverrides` + OnceLock）；`ai_service_url()` 单源封装防 spawn/调用断链；删除旧 `LLOOM_API_PORT` 残留 | `--help` 自文档、无第二配置真源 |

***

## 三、计划文档索引（三份详细设计，均已编写，状态不同）

| 文档                                     | 编写时间                     | 提交状态                                    | 内容范围                                                   | 落地状态                                                                                                                                                                                                |
| -------------------------------------- | ------------------------ | --------------------------------------- | ------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`CONTEXT-PLAN.md`](./CONTEXT-PLAN.md) | 2026-08-24               | **已提交**（a2b8bb5，本阶段审计确认 Phase 2–5 全部落地） | 上下文优化：SQLite 对话存储、预算上下文、两层缓存、原子写、两阶段落盘                 | Phase 1–5 已全部落地（两阶段追加/更新端点、服务端 history、L1/L2 摘要、两层缓存+单例+淘汰、UsagePage 缓存节省）；本阶段补 conversation 层 5 个守护单测（84 全绿）                                                                                       |
| [`PRICING-PLAN.md`](./PRICING-PLAN.md) | 2026-08-24 编写，持续更新       | **已提交**（694f0a9 等，P2.a/P2.c 已落地）        | 定价表系统 PriceSpec（分项×时段×阶梯×来源）、pricing.rs 引擎、校准 job、探针系统 | 后端 PR-1\~PR-8 已全部落地（含 PR-5 路由衔接、PR-8 峰谷调度已提交）；PR-6/7 前端定价页+探针视图已落地；P2.a 定价刷新 + P2.c 缓存节省已追加落地                                                                                                       |
| [`ROUTING-PLAN.md`](./ROUTING-PLAN.md) | v3（2026-08-24）+ v4–v8 注记 | **已提交**                                 | 路由重构：消除硬编码、注册表驱动、信号—投影—决策管线、预算联动                       | **P0.a/b/c/d/e/f/g 全部落地**（09480fa、8dddc59、50ec431、d6912b9）；P1.a/b/c/d 全部落地（P1 阶段完成）；**P2.a 定价刷新 + P2.c 定价页/徽标已落地**；**P3 健康感知 + fallback + overhead 已落地**；**P4 编排智能升级已落地**；**P5 预算驱动动态调整已落地**（本阶段提交） |

> 三份计划文档是详细设计与落地顺序的权威来源。本进度文档只做索引与高层同步，不重复其细节。

***

## 四、已落地功能进展（commit / 工作区视角）

**已提交 commit 主线**（最新在前）：

- `cab03c8`/`0b7820c` ROUTING P1.a：用量落库补全（usage\_records 加 latency/request\_id、编排逐角色记账）

- `d6912b9` ROUTING P0.e/g：新增模型自动打标（metadata.rs 五级兜底）+ signals.rs 信号层正规化（难度带/reask/LLM 判定，阈值可配置）

- `50ec431` ROUTING P0.f：消除 Python 模型真源（Rust 单一决策，orchestrate assignments 下发）

- `8dddc59` ROUTING P0.b/c/d：models 元数据列+迁移回填、routing\_policy/model\_task\_score/routing\_decisions 三表、plan() 评分路由替换全部硬编码（chat 路径）

- `09480fa` ROUTING P0.a：models 表单价写入断言 \[1e-9,1e-3] + 单测

- `b09d229` Pricing engine, usage telemetry, probes（PRICING-PLAN PR-1\~7）

- `a2b8bb5` Context optimization：SQLite conversation store、budgeted context、two-layer cache

- `dc534f0` Sync CLI/TUI with WebUI streaming, fix minor bugs, consolidate docs

- `091dc31` Semantic cache optimization

- `433362e` Security fixes, graceful shutdown, cache pre-init, rename & markdown

**2026-08-26 落地（ROUTING-PLAN P0.a/b/c/d/f，均已提交并冒烟验证）**：

- P0.a（09480fa）：`db.rs` `validate_cost` 断言进 `insert_model`/`update_model`（单价 0 或 \[1e-9,1e-3] USD/token）

- P0.b（8dddc59）：models 表 +11 元数据列（capability\_tier/quality\_score/context\_window/supports\_stream/health\_state/needs\_calibration 等），幂等迁移 + 一次性名称启发式回填（settings 标记 `migration_routing_meta_v1`；备份 `data/lloom.db.pre-routing-migration.bak`）

- P0.c（8dddc59）：routing\_policy（7 条种子策略）/model\_task\_score（EWMA α=0.15 回填函数）/routing\_decisions（审计+outcome 回填）/routing\_calibration 四表 + CRUD

- P0.d（8dddc59）：**删** **`TASK_MODEL_MAP`/`INFERENCE_MODELS`/`select_model`/`task_model_preference`** **全部硬编码**；`plan()` 门槛（tier/ctx/health/cost-cap/pinned）+ 加权评分（成本走 pricing.rs est\_cost，质量 ewma≥5 样本覆盖冷启动分，needs\_calibration 罚 0.3）；`chat_stream` 删伪造空 spec（direct 未注册→明确 SSE 报错）；`pick_classifier` 注册表驱动；审计落库；router 11 单测全过

- 冒烟：simple\_qa→qwen2.5-local（cost 0）+fallback 链；coding→deepseek-v3(tier3,stream)；routing\_decisions 2 条 outcome=success

- **P0.f（50ec431，2026-08-26）**：`router.rs` 新增 `plan_decision()`；`server.rs` 构造 assignments（general/decompose/aggregate）；`ai_client.rs` 写入请求体；`ai_service.py` 删 `TASK_MODEL_PREFERENCE`/`DECOMPOSER_PREFERENCE`/`_select_model`，新增 `_assigned_model()`（优先 assignments、兜底 models\[0]）。**「无字面量」验收**：`ai_service.py` 无模型名常量/偏好表，模型名仅来自 Rust assignments 或 models 全池。冒烟：轻量 + 复杂（分解→4 子任务→汇总）+ easy 路径全过，Python 无错误

- **P0.e（d6912b9，2026-08-26）**：新增 `metadata.rs::resolve_and_fill`，`db::insert_model` 落库前五级打标（overlay 显式 > 启发式；不覆盖用户/overlay 显式值）：`flash/mini/1b 等`→轻量档、`max/r1 等`→旗舰档、本地端点置零成本+标 `is_local`、未显式上下文回填 32K、一律标 `needs_calibration` 进保守期。6 单测 + `insert_model` 注册冒烟过

- **P0.g（d6912b9，2026-08-26）**：`signals.rs` 补 `SignalSet`/`extract`/`band_from`/`reask_decision`/`llm_classify_needed`，困难度=structure/complexity/context 加权，难度带 easy/medium/hard，权重与阈值走 settings KV 可调；顺带修复 CJK `\b` 词边界致 `工具` 不命中 tools 信号的 bug。7 单测过。**P0 阶段至此全部勾选完成**

**2026-08-27 落地（ROUTING-PLAN P1.b/c/d + 既有 P1.a，P1 阶段完成）**：

- P1.a（cab03c8/0b7820c）：`usage_records` 加 `latency_ms`/`request_id` + `idx_usage_req`，`insert_usage` 扩参（探针回兼容）；chat 落耗时+请求号（失败不写 usage 归 `routing_decisions.outcome`）；编排逐 `task_done` 按 role 建账（decompose/子任务自身/aggregate），model 兜底 unknown；迁移清 `default`+cost=0 旧脏数据

- P1.b（工作区）：`migrate_db` 预置 `routing_policy.pinned_model` 推荐主选（新库 VALUES 带、既有库仅回填 NULL，settings `migration_policy_v1_p1b` 一次性标记）

- P1.c（工作区）：`metadata.rs::cold_start_quality(_in)` overlay 按 task\_type 榜单折算；`db::upsert_model_task_score_signal` 在线 EWMA（α=`signal.ewma_alpha` 默认 0.15，输入 σ 不 clamp、结果 clamp \[0,1]）+ 按信号自增 success/fail/escalation + `sample_count≥20` 解除保守期；server.rs chat 落 Success、orchestrate 按 task\_done error 下发 Success/SubtaskFail（model/role≠unknown 才打点）；`QualitySignalKind`（models.rs）8 信号 σ 值

- P1.d（工作区）：`server.rs` `POST/GET /api/routing/shadow`（采样 `routing.shadow_ratio` 默认 0.10、基线 `routing.shadow_baseline` 否则能力档最高、FNV-1a 哈希防重、成本走 `priced_usage` 真源、结果落 `routing_calibration`）+ `db::insert_routing_calibration/count_routing_calibration` + `config.rs::shadow_ratio`；`scripts/aiq_replay.py` 离线 AIQ 重放（三条线成本—质量、AIQ 预算积分、质量回填写库）

- 冒烟：`cargo test` 54 全绿（新增 EWMA 累计+保守期解除、迁移幂等修 scale 单测）；AIQ 脚本 3 样本冒烟出 AIQ + 95% 节省 + 调参建议

**2026-08-27 落地（ROUTING-PLAN P3 健康感知 + fallback + overhead；P3 阶段完成）**：

- `health.rs`（新增）：纯 Rust 健康状态机——滑窗（默认 5 内 ≥2 失败 = degraded）、连续 ≥3 失败 = down、成功永远向 up 收敛（unknown→up/degraded→up/down→up）、熔断连续 ≥5 强制 down；阈值全走 settings `health.*` KV 可运行时调；**仅状态变化才** **`set_model_health`** **落库**（含 `health_checked_at`），非热路径写

- db.rs：`set_model_health`（UPDATE models）+ `routing_overhead_report`（count/avg/P95/max/slow，`routing_decisions.routing_ms` 真源）

- server.rs chat 故障转移：`chat_with_failover` 按 `primary + fallback_chain` 顺序重试——失败打健康哨点、跳升记 `Escalation` 成效信号、成功按实际响应模型（`used_model`）计价落库；orchestrate 按 `task_done` 成功/失败喂健康哨点（仅 model≠unknown）

- server.rs 后台 `health_probe_loop`：每 `health.probe_sec`（默认 60s）对 down/degraded 模型发最小请求主动探测恢复 → 已挂载 `spawn_background_jobs`

- `GET /api/routing/overhead`（?days=N，0 全部；>100ms 记 slow；`fast_path_healthy` 标注快路径健康度）

- config.rs：`health_fail_window/degraded_fails/down_consecutive/circuit_threshold/probe_sec` 五键，默认 5/2/3/5/60

- 冒烟：`cargo test` 65 全绿（+7 health 状态机 +1 overhead 聚合）；`cargo build` 无警告

**2026-08-27 落地（ROUTING-PLAN P4 编排智能升级；P4 阶段完成）**：

- P4.0 选 A 轻量回调：Rust 新增 `POST /api/routing/plan-subtask`（无状态 `plan_for_task(task_type, est_in, est_out, budget_tier)` 出 primary + fallback 链 + escalation\_enabled）；`router.rs::plan_for_task` 为 `plan()` 的参数化封装，`plan_decision` 复用其默认参（500/1000/"normal"）

- Python `orchestrate_stream` 每子任务按其 `task_type` 回调拿 plan（`_plan_subtask`，urllib 标准库，失败回落原 assignments），primary→fallback\_chain 逐个降级重试（记 `retry_count`，失败绝不美化）；P4.c 零成本质量信号（`_quality_signal_ok`：非空/≥2 字/无失败哨兵）不达标 + `escalation_enabled` → `_strongest_model` 升档强模型重试一次（用单价 in+out 作强档代理，本地免费模型不成为目标、无更高价则不开）；`decompose`/`simple_qa` routing\_policy 种子 `escalation_enabled=1`

- SSE 契约：`task_done` 透传 `escalated_from`/`retry_count`/`tier_bumped`，`result` 透传 `escalations`/`tier_bumped`；Rust 对 `escalated_from` 记 **Escalation** 成效信号（P3 同语义，final 模型仍记 Success）；P4.d 汇总/轻量路径均走 Rust `plan_decision(aggregate/general)`

- 顺带修复两处既有 bug：① **SCHEMA** **`idx_usage_req`** **升级断裂**——该索引引用仅靠 migrate ALTER 才加的 `request_id`，P1.a 前的旧库在 SCHEMA 阶段即失败 → 移入 `migrate_db` ALTER 后幂等建索引（真实 pre-P1a 旧库冒烟：启动成功、列补齐、索引就位）；② `_strongest_model` 曾依赖 Python `ModelSpec` 不存在的 `capability_tier`/`quality_score` → escalation 触发即崩溃 → 改单价代理

- 冒烟：`cargo build` 无警告 + 65 单测全绿；plan-subtask 端点 simple\_qa→qwen2.5-local/fallback\[deepseek-v3,qwen-plus]、coding→deepseek-v3/fallback\[qwen3-max,qwen3.6-plus]、aggregate 走 plan 均正确；Python 助手（quality\_signal/plan\_subtask/strongest\_model/dead-url 兜底）校验全过

**2026-08-27 落地（ROUTING-PLAN P5 预算驱动动态调整；P5 阶段完成）**：

- P5.a 预算档进入决策链：`budget_tier_from_ratio(r)`（normal>0.5 / throttle>0.2 / tight>0.05 / protect≤0.05）+ `tier_cost_multiplier`（throttle×1.5/tight×2.5）注入 `plan()` 评分（cost\_weight 倍率）；**tight 复杂任务降一档**（`band==hard`→medium，tier\_req 放松）；**protect 仅** **`is_local=1`** **或零成本模型**（其余 reject 并给 `protect 仅本地/零成本` 明确报告），预算耗尽推本地 Ollama——降级非硬拒

- P5.b 预算模型扩展：幂等迁移给 `budgets` 加 `scope_task_type`/`soft_limit_ratio`/`action_on_exceed`，`Budget` 结构体与 `upsert_budget`/list/get 同步

- P5.c 预估成本前置校验（B+A 复用，**不引 Rust tiktoken**）：`model_task_score` 加 `avg_out_tokens REAL DEFAULT 500`（幂等），`insert_usage` 对非缓存命中把真实输出 token 滚入 EWMA（`roll_avg_out_tokens`，α 同 `signal.ewma_alpha`）；`task_avg_out_tokens` 冷启动（sample<20）返 500×1.5=750 保守点，样本足用真实均值；`global_budget_ratio()` 从 global 预算水位算 r，`route()` 热路径动态注档

- Python orchestrate 侧（P5.c 编排侧）：回调 `plan-subtask` 前用 tiktoken `count_tokens(query+依赖上下文)` 精确传 `est_in`；`_plan_subtask` 改可选参（est\_out/budget\_tier 不传交 Rust 默认）；`POST /api/routing/plan-subtask` 服务端补默认（est\_out 缺省用 `task_avg_out_tokens` 真实均值、budget\_tier 缺省从 `global_budget_ratio` 注出）

- 冒烟：`cargo build` 无警告 + **70 单测全绿**（+5 预算档：边界阈值/成本倍率/protect 过滤/throttle 换选/tight 降档）；plan-subtask 端点 simple\_qa→qwen2.5-local、coding→deepseek-v3、complex\_reasoning+protect 非本地全 reject 报明确错误；schema 列已落库

**2026-08-27 落地（ROUTING-PLAN P2 定价刷新 + WebUI；PR-6/7 前端；P2 阶段完成）**：

- P2.a 定价刷新：`server.rs` `pricing_refresh_loop` 24h 后台 job（jsdelivr 主源 + ghproxy 回退，断网失败静默保留本地值）+ `POST /api/pricing/refresh`（手动触发）、`POST /api/pricing/specs/{provider}/{model}/accept`（采纳转 manual，此后不被覆盖）；`pricing.rs::parse_remote_prices` 纯函数解析（跳过非 provider/model 键、负价，离线单测）+ `db::refresh_price_spec`（COALESCE 保 cache\_read，不覆盖 manual）

- P2.c WebUI 定价页：新增 `PricingPage.tsx`（price\_source 徽标 manual/overlay/litellm\_remote…、`price_stale` 黄点、手工改价强制转 manual、采纳建议价、探针统计卡 + 近 30 天校准曲线），路由/导航接入（定价页）；`api.ts` 增 `listPriceSpecs/updatePriceSpec/acceptPriceSpec/refreshPricing/listPriceCalibration/getProbeStats/setProbeBudget`

- P2.c 缓存节省：`usage_records.cache_saved_cost` 列 + `UsageExtra` 透传 + `get_usage_stats` SUM 聚合；用量页「缓存为您节省 ¥X」卡片 + 「缓存节省」列（CNY 展示）

- 冒烟：`cargo test` 57 全绿（新增 parse\_remote\_\*、cache\_saved\_cost 聚合）；`tsc --noEmit` + `vite build` 全过

**过往已实现并验证**（见 memory / 历史 commit）：

- 编辑对话名称（`rename_conversation`，PUT `/api/conversations/{id}`）

- 三大安全修复：SQL 注入列名白名单、路径穿越 id 白名单、密钥脱敏

- 优雅退出：SIGINT/SIGTERM + `POST /api/shutdown`

- SemanticCache 预初始化 + `_cache_ready` 门控（解决 chroma 79MB 模型下载挂死）

- CLI/TUI 同步 WebUI 真流式输出、模型标注、对话重命名

***

## 五、安全与健壮性（已修复并验证）

- **SQL 注入**：`db.rs update_model` 列名白名单，非法 key 拒绝。

- **路径穿越**：`conversations.rs validate_id` 校验 `id ∈ [A-Za-z0-9_-]`。

- **密钥泄露**：`get_config` 对 `*_API_KEY/_KEY/_TOKEN/_SECRET` 脱敏为 `****+后4位`，设置页不预填。

- **优雅退出**：SIGINT/SIGTERM 信号处理 + `POST /api/shutdown`，子进程全清理，杜绝端口残留。

- **用量/成本真实落库**：`chat_stream` 与 `orchestrate_stream` 均 `insert_usage`（PR-1 修复原断链 bug）。

- **原子写对话**：`conversations.rs` 写 `{id}.json.tmp` → fsync → rename，崩溃不损坏。

***

## 六、待办事项（TODO）

按优先级：`🔥` 高/安全，`⚡` 体验，`🔧` 优化。

### 来自 PRICING-PLAN.md

- [x] ✅ **PR-5 路由衔接**（2026-08-27，commit 864b552）：缓存命中率喂 `effective_input_cost` 进 `plan()`/`plan_for_task()`（按 task\_type 聚合真实 cached/prompt）；会话亲和 sticky（+0.05，仅缓存敏感通道，缺省 0 不偏袒）+ `conversation_id` 透传；+3 单测（合计 73）

- [x] ✅ **PR-6 WebUI 定价页**：`GET /api/pricing/specs` 等后端 + PricingPage 前端均已落地（2026-08-27）

- [x] ✅ **PR-7 探针视图**：`GET /api/probe/stats` 后端 + 用量页/定价页探针视图均已落地（2026-08-27）

- [x] ✅ **PR-8 峰谷调度**（2026-08-27，本阶段）：可延迟任务挪谷时——`Zone::multiplier_at`/`first_valley_epoch`（30-min 步进扫 2h 内首个折扣窗口，`[lo,hi)` 边界 12/18 时即谷）+ `ZoneResolver::zones()`；`router::next_valley_epoch`（取最早进谷渠道）+ `cost_epoch`（deferrable 且 2h 内进谷按谷时估成本）；`plan-subtask` 收 `deferrable` 回传 `defer_until`；探针 `valley_wait_secs` 高峰自动挪谷执行；实时 chat 路径 `deferrable=false` 零延迟；+9 单测（合计 79 全绿）

### 来自 ROUTING-PLAN.md（v3，P0/P1/P2 阶段均已完结）

- [x] ✅ **P0.a 量纲写入断言**（09480fa）

- [x] ✅ **P0.b/c 元数据列 + 策略/审计表**（8dddc59）

- [x] ✅ **P0.d 路由重构**：`plan()` 评分路由已替换全部硬编码（chat 路径），11 单测 + 冒烟过（8dddc59）

- [x] ✅ **P0.e 增删模型自动打标**：`metadata.rs` 五级兜底已在 `insert_model` 自动回填并标需校准（d6912b9）

- [x] ✅ **P0.f 消除 Python 真源**：`plan_decision` + assignments 下发，`ai_service.py` 无模型名字面量（50ec431）

- [x] ✅ **P0.g 信号层正规化**：`SignalSet`/难度带/reask/LLM 判定，阈值走 settings KV，有单测（d6912b9）

- [x] ✅ **P1.a 用量落库补全**（2026-08-27）

- [x] ✅ **P1.b 推荐分配**（2026-08-27）

- [x] ✅ **P1.c 成效分**（2026-08-27）

- [x] ✅ **P1.d 影子评测 + AIQ 重放**（2026-08-27，热路径自动采样已接入）

- [x] ✅ **P2.a 定价刷新 + P2.c 定价页/徽标**（2026-08-27）：24h 刷新 job + 手动触发 + 采纳转 manual；WebUI PricingPage（specs/徽标/改价/采纳/校准）；用量页缓存节省卡片 + 探针视图

- [x] ✅ **P3 健康感知 + fallback + overhead**（2026-08-27，commit 3551f8c）：`health.rs` 状态机（滑窗 degraded/连续失败 down/熔断+成功恢复）；chat `chat_with_failover` 按 fallback 链重试；后台 `health_probe_loop` 主动探测；`GET /api/routing/overhead`（count/avg/p95/max/slow）；65 单测全绿

- [x] ✅ **P4 编排智能升级**（2026-08-27）：子任务级独立 plan + 阶段降级重试 + escalate 升档；修复 SCHEMA 旧库升级断裂 + Python 升档崩溃；70 测试全绿；冒烟验证端点正确

- [x] ✅ **P5 预算联动**（2026-08-27，本阶段）：预算档进入决策链（throttle/tight cost 倍率 + tight 复杂降档 + protect 仅本地）+ `avg_out_tokens` 真实输出 EWMA + tiktoken 精确 est\_in + plan-subtask 服务端默认注档；70 单测全绿

- [x] ✅ **PR-5 路由衔接**（2026-08-27，commit 864b552）：缓存命中率喂 `effective_input_cost` 进 `plan()`/`plan_for_task()`（按 task\_type 聚合真实 cached/prompt）；会话亲和 sticky（+0.05，仅缓存敏感通道，缺省 0 不偏袒）+ `conversation_id` 透传；+3 单测（合计 73）

### 来自 CONTEXT-PLAN.md（审计确认：Phase 2–5 已落地，详见 a2b8bb5）

- [x] ✅ **Phase 2 上下文架构迁移**：前端只发 (conversation\_id, query)，Rust 构建历史（`load_history_for_orchestrate`）+ meta 持久化回显 + `interrupted` 崩溃恢复 UI（chatStore.persistPhase1 / ChatPage）

- [x] ✅ **Phase 3 压缩**：tiktoken 预算器 + L1 截断（`build_context`，`LLOOM_CONTEXT_BUDGET` 配）+ L2 滚动摘要（`_make_summary`，`get/set_summary` 持久化、`SUMMARY_BLOCK` 区间增量重算）

- [x] ✅ **Phase 4 缓存增强**：L1 精确缓存（SQLite `sha256(model|system|fingerprint|q)`）+ 上下文无关判别器（`_is_context_free` 启发式）+ `SemanticCache`/`ExactCache` 单例 + 5min 淘汰线程（TTL+LRU `LLOOM_CACHE_MAX_ENTRIES`）+ UsagePage 缓存节省卡

- [x] ✅ **Phase 5 供应商前缀缓存（代码侧）**：`build_context` 固定 `[system][?summary][kept]` 前缀 + summary 块状更新，已为 DashScope 前缀缓存铺路；**账单侧命中核对留待办**（需 DashScope key）

### 下一阶段（权威计划：[NEXT-PLAN.md](./NEXT-PLAN.md)，2026-08-29 起）

- [x] ✅ **N1 OpenAI 兼容代理**（2026-09-02，本阶段）：新模块 `openai_compat.rs`——`POST /v1/chat/completions`（流/非流，SSE 帧序列 role→content→finish→\[DONE]）+ `GET /v1/models`（auto 恒在首位）+ Bearer 鉴权（`LLOOM_PROXY_TOKEN` 未设不鉴权，设了错误凭据 401）；model 语义：注册名直连 / `auto` 或未知名走 `route()` 评分路由；透明复用 security 检查、P3 failover、`priced_usage` 计价、`insert_usage`（api\_source='proxy'，C3 已提交 1a22825）；**O2 收尾**：默认绑 `127.0.0.1`（`config::bind_addr()`，`LLOOM_BIND` 可覆盖）；`chat_with_failover` 加 max\_tokens/temperature 参数透传（客户端未指定缺省 4096/1.0）；89 单测全绿（+5：鉴权/模型解析/响应形状/帧序列）；curl 冒烟全过（auto→plan 真实选择入 usage、直连/未知名、401 鉴权）

- [x] ✅ **N2 闭环评估**（2026-09-02，本阶段）：**N2.a 报告闭环**——`aiq_replay.py` 加 `--json`（计算与输出分离，与文本报告数字同源一致；顺手修 SELECT 缺 `id` 列 KeyError 与 sqlite `?1` 占位符弃用警告）；新表 `policy_review`（幂等建表，三线成本/质量 + AIQ + saved\_pct + 预算档分布 + 建议快照）；周期 job `aiq_report_loop` 挂 `spawn_background_jobs()`（6h，首 tick 立即出报告，失败只打日志下周期自愈）；`GET /api/routing/review`（最新报告）+ `POST /api/routing/review/refresh`（手动立即体检）。**N2.b 权重建议**——Rust 侧 `review.rs::grid_search_suggestions` 用 `plan()` 对影子样本无副作用网格重放（cost/quality 权重 7×7，打分 = 质量增益 − 0.5×成本增幅，物性门槛 0.05），找支配当前策略的帕累托点；`POST /api/routing/review/adopt` 人工采纳后 upsert `routing_policy`（每请求读库，下一请求即生效；**不自动改策略**）；WebUI OverviewPage「路由体检」折叠卡（三线表 + 预算档分布 + 建议对比表 + 全部采纳/立即体检按钮，遵循重要信息折叠收纳约定）。**验收**：94 单测全绿（+4 review：网格建议更便宜模型 / 无空间不出建议 / 采纳后换选 / policy\_review 往返+档分布）；curl 冒烟全过（启动首跑写报告 id=1、refresh 追加 id=2、无建议时 adopt 正确拒绝）；`--json` 与文本数字一致已核

- [ ] **N3 信任与收尾**：a) ✅ **O6 子任务并行**（2026-09-03）——`ai_service.py` 编排路径按 `depends_on` 分波：同波无依赖子任务以 `ThreadPoolExecutor` 并发（`_call_llm` 为同步调用，等价 `asyncio.gather` 语义；ExactCache/SemanticCache 内部锁 + `completed`/计数器仅主线程变动，线程安全），`task_start` 先行、`task_done` 按原序在波末统一下发，依赖环兜底为无上下文执行（同旧串行行为）；子任务执行体重构为 `_execute_task`（plan 回调 → fallback 链 → escalation 不变），SSE 契约无改动。冒烟：3 子任务时长和 17.17s > 墙钟 17.7s（含 decompose+汇总 1.93s）证实并发，聚合输入完整；94 单测全绿 b) `/metrics` Prometheus 导出 c) 账单对账脚本（**阻塞：需真实账单导出**，脚本先行）

### 历史遗留（LLooMprogress 原 TODO）

- [x] ✅ **O2 绑定收窄**（2026-09-02，随 N1 落地）：默认绑 `127.0.0.1`，`LLOOM_BIND` env 可覆盖

- [x] ✂️ **O6 子任务并行**：已归并进 NEXT-PLAN **N3.a**

- [ ] ⚡ **O5 复杂判定调优** → 已收编进下方 **C 类台账**（需标注语料）

- [ ] 🔧 多模型拆分需配置「可用模型 + 有效 Key」才真正生效 → 见 **C 类台账**

- [ ] 🔧 思考过程深度展示（可选）→ 见 **C 类台账**

### 搁置项台账（B 类：条件触发，未触发不主动开工）

> 原则：每条只记「名称 + 解锁条件 + 权威落点引用」，**正文与实现细节仍留在原计划文档**，此处不复制内容，避免第二真源。
> 状态图例：`🔓` 前置已就绪可随时做 · `⏳` 阻塞中 · `🕐` 待触发/待数据 · `🚫` 现阶段明确不做

| #   | 搁置项                                     | 解锁条件（触发即做）                                                                                 | 权威落点                                         | 状态 |
| --- | --------------------------------------- | ------------------------------------------------------------------------------------------ | -------------------------------------------- | -- |
| B1  | **batch 通道**（百炼 Batch 5 折，无缓存折扣、非实时）    | ✅ **前置已全部就绪**（schema 预留 `batch_multiplier`，PR-8 峰谷调度已落地）；属「省钱」非「提能力」，建议排 N1 之后才有量可省        | `PRICING-PLAN.md:592` §5.5（schema 预留 `:170`） | 🔓 |
| B2  | **账单对账**（N3.c）                          | 需 DashScope 真实账单导出（等 key / 账期）；脚本可先行                                                       | `NEXT-PLAN.md:69`                            | ⏳  |
| B3  | **G1 多租户**                              | 出现家庭之外的固定用户 → 触发则 SQLite 迁 PG + 鉴权/配额层（**架构级分叉，需单独立项**）                                    | `NEXT-PLAN.md:77`                            | 🕐 |
| B4  | **G2 MCP 接入**                           | 开始做 Agent 运行时 / 有外部智能体要消费 LLooM；作 server（暴露路由/缓存/定价为 MCP 工具）与作 client（编排消费 MCP 工具）**先后需定** | `NEXT-PLAN.md:78`                            | 🕐 |
| B5  | **编排状态收归 Rust**（暂停/恢复/人工介入）             | 出现该需求（与 G2 相关）；当前无此需求，B 方案够用                                                               | `ROUTING-PLAN.md:711`、`NEXT-PLAN.md:84`      | 🕐 |
| B6  | function calling / tools、多模态、多 key 分租户  | **G1 之后**才展开                                                                               | `NEXT-PLAN.md:41`                            | 🚫 |
| B7  | **验收① 阶梯价交叉单测**                         | 需真实阶梯价 spec 数据（现 spec 为平价，无真实阶梯）                                                           | `ROUTING-PLAN.md:814`（序 4）                   | 🕐 |
| B8  | **验收② 影子样本成本降 ≥60%**                    | 需影子真实样本（随 N2 / N1 接入后现网流量自然达成）                                                             | `ROUTING-PLAN.md:817`（序 7）+ 注记 `:834`        | 🕐 |
| B9  | **验收③ escalation 再降 ≥30%**              | 同 B8，需影子真实样本                                                                               | `ROUTING-PLAN.md:820`（序 10）+ 注记 `:848`       | 🕐 |
| B10 | **Router-R1 式 RL 路由**                   | 影子数据达**千级样本**再评估；当前「描述符评分 + EWMA + 影子评测」已覆盖其核心收益                                           | `ROUTING-PLAN.md:863`（风险注记 8）                | 🕐 |
| B11 | **Switchyard 引入**                       | 等其 **v1.0**（v0.2.0 前 API 破坏性变更）且走 libsy 库路径；当前只借鉴设计                                        | `ROUTING-PLAN.md:857`（风险注记 3）                | 🕐 |
| B12 | **BEST-Route 并行采样**（低置信请求并行采两轻量档 + 裁决）  | 远期，待级联/裁判机制成熟                                                                              | `ROUTING-PLAN.md:213`                        | 🕐 |
| B13 | **OpenRouter** **`usage.cost`** **对账源** | 未来对账增强                                                                                     | `ROUTING-PLAN.md:202`                        | 🕐 |
| B14 | **L3 关键事实抽取**（摘要之上抽实体/偏好/约束）            | 远期为超长项目型对话准备；L2 已落地，按需再加                                                                   | `CONTEXT-PLAN.md:126`                        | 🕐 |

### 独立小项台账（C 类：无前置依赖，可随时插队）

| #  | 小项                                                                               | 完成路径（要点）                                                                                                                                                                                                                                                                                                                                                                                         | 权威落点                                                    | 代价            |
| -- | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------- | ------------- |
| C1 | **~~思考过程深度展示~~** ✅ **已完成**（2026-09-02）                                           | `_call_llm`/`_call_llm_stream` 新增 `reasoning_ref` 捕获 litellm `message.reasoning_content`（流式为 delta 累积）；简单/聚合两路径 `result` 事件携带 `reasoning`；Rust orchestrate SSE 原样透传；chatStore 捕获+meta 持久化+历史加载映射；ChatPage `Collapse` 折叠卡（字数标签+滚动容器）。**E2E 已验证**：deepseek-r1 全链路 644 字思考+真实 usage（41/827 tok）。**顺手修复既有生产 bug**：`_call_llm_stream` 引用未定义 `usage` 变量（流式带 usage\_ref 必抛 NameError，聚合阶段长期静默回退子任务原文拼接） | `ai_service.py:1303-1334`、`chatStore.ts`、`ChatPage.tsx` | 已落地           |
| C2 | **est\_input\_cost 分列**（精确输入侧对账）                                                 | `db.rs` 幂等 ALTER 列表加两列（迁移框架已支持），对账从「总额口径」升级为「输入侧分项」                                                                                                                                                                                                                                                                                                                                              | `PRICING-PLAN.md:36`                                    | 低             |
| C3 | **~~`api_source`\~\~\~\~列~~** ~~（区分代理流量）~~ ✅ 已完成（2026-09-02，1a22825，随 N1 提前独立提交） | 幂等加列默认 `'webui'`，代理流量标 `'proxy'`                                                                                                                                                                                                                                                                                                                                                                 | `NEXT-PLAN.md:34`                                       | 已落地           |
| C4 | **O5 复杂判定调优**                                                                    | ⚠️ **先统一入口**：现 `router.rs:86 is_complex`（正则）与 `signals.rs:234 complexity_score`（评分，`:322` 调用）**两处判定并存**，须合并后再调阈值；且需 50–100 条标注语料，否则是盲调                                                                                                                                                                                                                                                           | `router.rs:86`、`signals.rs:234`                         | 中（**阻塞在语料**）  |
| C5 | 多模型拆分真正生效                                                                        | **非开发项（无代码改动）**：需在设置页配置「可用模型 + 有效 Key」才生效，属配置引导                                                                                                                                                                                                                                                                                                                                                  | —                                                       | 配置            |
| C6 | EWMA α 灵敏度调整                                                                     | 仅当出现**日级调价**时（当前 α=0.15 ≈ 10 天半衰，对周级调价够用）                                                                                                                                                                                                                                                                                                                                                        | `PRICING-PLAN.md:931`                                   | 条件触发，**建议不动** |
| C7 | 多时区 chrono                                                                       | 仅当需多时区；当前纯标准库（+8 偏移 + Sakamoto + Hinnant）是**有意规避** crates.io 拉取风险                                                                                                                                                                                                                                                                                                                                | `PRICING-PLAN.md:34`                                    | 条件触发，**建议不动** |

> **C 类建议顺序**：~~C1~~（✅ 已完成）→ C2 → （N1 顺带 C3 ✅）。C4 等语料（可用 N1 接入后的真实流量自动采集）；C6/C7 条件未到不动。
> **C1 附注**：为验证推理链路注册了 `deepseek-r1`（dashscope，已配真实单价 5.5e-7/2.2e-6 USD/token + reasoning\_cost），保留在注册表中供 WebUI 思考折叠展示测试。

***

## 七、重要问题记录（保留有长期价值的技术教训）

### 1. 语义缓存模型下载卡死

- **根因**：ChromaDB 的 `ONNXMiniLM_L6_V2` 默认从 **AWS S3** 拉 \~79MB 模型，**不是** HuggingFace；`HF_ENDPOINT` 对 S3 下载是 **no-op**。受限网络下 S3 直连 \~6KB/s，卡死数小时。

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

### 5. 路由双真源（2026-08-26 已闭合）

- 现状（已消除）：Rust 侧 `TASK_MODEL_MAP` 硬编码 + Python 侧 `TASK_MODEL_PREFERENCE` 另一份硬编码，两份互不相通且都不读 DB、不看 is\_active。删模型后路由名找不到会伪造空 spec 直接失败。

- **进展**：`plan()` 评分路由已落地（P0.d），Rust chat 路径单一决策。**P0.f（50ec431）已消除 orchestrate 路径的 Python 真源**：`plan_decision` 构造 assignments（general/decompose/aggregate）下发，`ai_service.py` 删 `TASK_MODEL_PREFERENCE`/`DECOMPOSER_PREFERENCE`/`_select_model`，改读 `_assigned_model()`（assignments 优先、`models[0]` 兜底），Python 无模型名字面量。仅剩 P4.a 子任务级分配（当前复用 general 决策）。

***

## 八、关键约束（勿踩坑）

- **不可删** `data/`（真实数据）、`.env`（密钥）；可删可重建 `target/`、`build/`、`dist/`、`.venv/`、`node_modules/` 等。

- v2 与 Docker 完全解耦；旧 Docker 栈仅存 `legacy` 分支。

- **网络受限**：官方源（bun.sh/GitHub releases/huggingface/ChromaDB S3）常下载不动；统一镜像 —— npm/bun→npmmirror、pip→清华、Ollama→ghproxy.net、embedding→hf-mirror/modelscope。`HF_ENDPOINT` 对 ChromaDB S3 下载无效。

- `api/` 无 `__init__.py`，需 `pip install -e .` 才能被 `uvicorn api.ai_service:app` 导入。

- `.env.example` 的 `LLOOM_API_PORT=7860` 是旧残留，实际端口由 `LLOOM_WEB_PORT`（默认 7861）控制。

- 对话工具内 `nohup` 启动的进程跨工具调用会被回收，不能持久；持久运行用 `.command` 或系统服务。

- **改 Python(`api/ai_service.py`) 后必须重启 Rust 服务**才重拉 AI 服务（既有踩坑）。

- 所有 `ALTER TABLE` 迁移先 `PRAGMA table_info` 去重、迁移前备份 `data/lloom.db`（定价迁移已备份 `data/lloom.db.pre-pricing-migration.bak`）。

***

## 九、开发环境

- Python：**uv 管理**（`.venv/`，依赖按入库 `uv.lock` 冻结）：`uv sync --extra dev --extra build`；uv 不读 pip.conf，受限网络 `export UV_DEFAULT_INDEX=https://pypi.tuna.tsinghua.edu.cn/simple`；无 uv 回落 `pip install -e ".[dev]"`。CI/打包走 `uv sync --frozen --extra build`（PyInstaller 产物可复现）。

- Rust：`cargo build -p lloom-server`（全部模块已提交；改动后仍须 `cargo build` + `cargo test` 全绿再提交）。

- WebUI：`cd webui && npm install && npm run build`（根 `.npmrc` 已固定 npmmirror）。

- TUI：`cd tui && bun install && bun run build`（需 `bun`，可走 npmmirror CDN 镜像）。

- 端口：服务器 :7861、AI 服务 :7862、Ollama :11434。

- 启动：`./start-lloom.command`（orange 目录下测试用脚本，非软件界面功能）。

***

## 十、文档地图（所有 md 同步状态）

| 文件                           | 用途                                                   | 同步状态                       |
| ---------------------------- | ---------------------------------------------------- | -------------------------- |
| `LLooMprogress.md`（本文件）      | 项目总进度、决策、待办台账（主线 + B 类搁置 14 条 + C 类小项 7 条）、约束、文档索引   | 更新至 2026-08-31，同步至 488d156 |
| **`NEXT-PLAN.md`**           | **下一阶段权威计划**：N1 代理接入 → N2 闭环评估 → N3 信任收尾 + 决策门 G1/G2 | 2026-08-29 纳入（本次提交）        |
| `ARCHITECTURE.md`            | 分层架构、端点、数据流、技术栈                                      | 已同步至 488d156               |
| `README.md` / `README-ZH.md` | 用户文档（功能、快速开始、配置）                                     | 已同步至 488d156               |
| `CONTEXT-PLAN.md`            | 上下文优化方案（Phase 1–5 已全部落地）                             | 已提交，与 488d156 一致           |
| `PRICING-PLAN.md`            | 定价表系统详细设计与落地（PR-1\~8 全部落地）                           | 已提交；§5.5 batch 通道为远期搁置项    |
| `ROUTING-PLAN.md`            | 路由重构方案（v3 + 注记至 P5，P0–P5 全部落地）                       | 已提交；遗留 3 个待真实样本复验的验收指标     |
| `ROUTING-PLAN.md` 引用的外部研究    | Switchyard / vLLM Semantic Router / Router-R1 等      | 设计借鉴，不引入依赖                 |

> **接手检查清单**：① **先读** **`NEXT-PLAN.md`**（下一阶段权威计划），再读 CONTEXT/PRICING/ROUTING-PLAN 三份（均已完结，作背景）；② 内核 P0–P5、PR-1\~8、Phase 1–5 已全部完成；③ 下一优先项 = **N1 OpenAI 兼容代理**（含 O2 收尾）→ N2 → N3；④ **全部待办看本文第六节末尾两张台账**：B 类 14 条（条件触发，未触发不动）+ C 类 7 条（无前置，可插队），每条含解锁条件与权威落点行号，不必全项目 grep。每次 `cargo build`/`cargo test` 全绿 → 用户审查 → 才 push。

