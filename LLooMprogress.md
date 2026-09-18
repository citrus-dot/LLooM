# LLooM v2 项目进度

> 最后更新：**2026-09-18**（边缘测试三项加固：越狱大小写不敏感 / messages 边界校验 / 负预算拒收，120 单测全绿）· 仓库 `citrus-dot/LLooM` · 分支 `v2`
> 最新已提交：E2E 深度测试三项修复（`460b39e`，117 单测全绿、CI success）
> **待办唯一真源：[六、待办事项](#六待办事项todo)**——B 类搁置 15 条 / C 类小项 7 条 / 中期展望，主线已无未开工项

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

- **`openai_compat.rs`**（N1，2026-09-02）— OpenAI 兼容代理：`/v1/chat/completions` + `/v1/models`，Bearer 鉴权，路由/容灾/计价全复用

- **`metrics.rs`**（N3.b，2026-09-15）— Prometheus 文本格式指标导出（`/metrics`，0.0.4），纯函数 `render(db)` 可单测

***

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

***

## 三、计划文档索引（已完结，归档）

原三份详细设计（CONTEXT / PRICING / ROUTING）与阶段计划（NEXT）**已全部落地**，2026-09-16 归档至 [`docs/archive/`](./docs/archive/)，内容原样保留（头部加横幅致行号 +2，本文件台账中的行号引用已同步修正）：

| 归档文档 | 内容 | 落地状态 |
| --- | --- | --- |
| [`docs/archive/CONTEXT-PLAN.md`](./docs/archive/CONTEXT-PLAN.md) | 上下文优化：SQLite 对话存储、预算上下文、两层缓存、原子写 | Phase 1–5 全部落地 |
| [`docs/archive/PRICING-PLAN.md`](./docs/archive/PRICING-PLAN.md) | 定价表系统 PriceSpec、pricing.rs 引擎、校准 job、探针 | PR-1~8 全部落地（§5.5 batch = 台账 B1 远期） |
| [`docs/archive/ROUTING-PLAN.md`](./docs/archive/ROUTING-PLAN.md) | 路由重构：注册表驱动、信号—投影—决策、预算联动 | P0–P5 全部落地（3 个验收指标 = 台账 B7/B8/B9） |
| [`docs/archive/NEXT-PLAN.md`](./docs/archive/NEXT-PLAN.md) | N1 代理 / N2 闭环 / N3 收尾 + 决策门 G1/G2 + 中期展望 | N1–N3、M1 全部落地 |

***

## 四、已落地进展（commit 主线）

| 里程碑 | commit | 单测基线 |
| --- | --- | --- |
| 上下文优化 Phase 1–5 | `a2b8bb5` | — |
| 定价系统 PR-1~8 | `b09d229` | — |
| ROUTING P0.a~g（评分路由替换硬编码） | `09480fa`→`d6912b9` | 54 绿 |
| ROUTING P1.a~d（用量补全/推荐/成效分/影子评测） | `cab03c8` 等 | 54 绿 |
| ROUTING P3/P4/P5（健康/编排/预算联动） | `3551f8c` 等 | 70 绿 |
| ROUTING P2 + PR-6/7/8（定价刷新/WebUI/峰谷） | — | 79 绿 |
| N1 OpenAI 兼容代理（含 O2 收尾、C3） | `1a22825` | 89 绿 |
| N2 闭环评估（policy_review + 网格建议） | — | 94 绿 |
| N3.a 子任务并行 | — | — |
| M1 模型分层 + Db 池化 + per-model key | `2667f11`→`4cf8518` | 110 绿 |
| N3.b `/metrics` + N3.c 对账脚本 + C2 分列 | `4ba43b5`/`417ba29`/`2721d1c` | 110 绿 |
| B2「已对账」徽标 + 静态检查清零 + 质量 CI + C4 入口统一 | `5b39c0a`/`30e17cd`/`33c2fee` | 114 绿 |
| B15 会话级缓存感知路由 + PR-5 潜伏 bug 修复 | `925863e`/`2098fce` | 117 绿 |
| E2E 三项实质修复（影子计价/主聊天缓存激活/SQL 绑定） | `460b39e` | 117 绿 |
| 边缘测试三项加固（越狱 (?i)/messages 边界校验/负预算拒收）+ 3 护栏单测 | 本轮 | 120 绿 |

> 逐项实现细节：**设计依据见 `docs/archive/` 各 PLAN**（按锚点函数名），过程细节 `git show <hash>`。安全修复史见第五节，技术教训见第七节。

***

## 五、安全与健壮性（已修复并验证）

- **SQL 注入**：`db.rs update_model` 列名白名单，非法 key 拒绝。

- **路径穿越**：`conversations.rs validate_id` 校验 `id ∈ [A-Za-z0-9_-]`。

- **密钥泄露**：`get_config` 对 `*_API_KEY/_KEY/_TOKEN/_SECRET` 脱敏为 `****+后4位`，设置页不预填。

- **优雅退出**：SIGINT/SIGTERM 信号处理 + `POST /api/shutdown`，子进程全清理，杜绝端口残留。

- **用量/成本真实落库**：`chat_stream` 与 `orchestrate_stream` 均 `insert_usage`（PR-1 修复原断链 bug）。

- **原子写对话**：`conversations.rs` 写 `{id}.json.tmp` → fsync → rename，崩溃不损坏。

- **越狱拦截大小写不敏感**（2026-09-18 边缘测试发现）：攻击载荷常见大小写混用（"You are DAN"），原正则全小写漏拦；统一 `(?i)` 前缀。

- **messages 形状边界校验**（2026-09-18）：chat 主路径与 OpenAI 代理共用 `validate_messages`——空数组/非法 role/非字符串 content 在边界拒收（中文报错+下一步动作），不再穿透到 litellm 报英文原生错误；多模态分段数组与 content 缺省放行。

- **预算负值拒收**（2026-09-18）：`upsert_budget` 入口校验 `max_budget ≥ 0`（0=立即封顶语义合法），报错带修复提示。

***

***

## 六、待办事项（唯一待办真源）

> 四份计划文档已全部完结并归档（见第三节）；**项目所有剩余待办只在本节维护**，不在归档文档里新增条目。
> 按优先级：`🔥` 高/安全，`⚡` 体验，`🔧` 优化。

### 已完成主线（勾选明细已压缩，逐项细节见归档 PLAN 与 git log）

- ✅ **PRICING-PLAN PR-1~8** 全部落地（含 PR-5 缓存喂路由 + 会话亲和、PR-8 峰谷调度、PR-6/7 前端定价页/探针视图）
- ✅ **ROUTING-PLAN P0.a~g / P1.a~d / P2.a/c / P3 / P4 / P5** 全部落地（评分路由替换硬编码、影子评测、健康/fallback、编排智能升级、预算联动）
- ✅ **CONTEXT-PLAN Phase 2~5** 全部落地（服务端 history、截断+滚动摘要、两层缓存+淘汰、前缀缓存铺路）
- ✅ **NEXT-PLAN N1** OpenAI 兼容代理（2026-09-02）/ **N2** 闭环评估（2026-09-02）/ **N3.a 并行 + N3.b `/metrics` + N3.c 对账**（2026-09-03~15）/ **M1** 模型分层（2026-09-10）
- ✅ 历史遗留小项：O2 绑定收窄（随 N1）、O6 并行（归并入 N3.a）、C1 思考过程折叠、C2 输入侧分列、C3 `api_source` 列

### 搁置项台账（B 类：条件触发，未触发不主动开工）

> 原则：每条只记「名称 + 解锁条件 + 权威落点引用」，**正文与实现细节仍留在原计划文档**，此处不复制内容，避免第二真源。
> 状态图例：`🔓` 前置已就绪可随时做 · `⏳` 阻塞中 · `🕐` 待触发/待数据 · `🚫` 现阶段明确不做

| #   | 搁置项                                     | 解锁条件（触发即做）                                                                                 | 权威落点                                         | 状态 |
| --- | --------------------------------------- | ------------------------------------------------------------------------------------------ | -------------------------------------------- | -- |
| B1  | **batch 通道**（百炼 Batch 5 折，无缓存折扣、非实时）    | ✅ 前置就绪（schema 预留 `batch_multiplier`）；**2026-09-15 评审后继续搁置**：当前编排/代理均为实时流式语义，batch 是小时级异步产物，接入会改变产品语义；且无真实离线批处理流量（夜间评测/大量离线文档）——强行实现是死代码。**触发即做**：出现真实离线批处理场景时，UsageDetail 加 `is_batch` 标记 × `actual_cost` 乘 `batch_multiplier`（计价侧半天量级），通道编排另立项 | `PRICING-PLAN.md:594` §5.5（schema 预留 `:170`） | 🕐 待场景 |
| B2  | **账单对账收尾验证**（N3.c）                           | ✅ **代码链路已全部落地**（脚本 417ba29 + 徽标端点/Tag 5b39c0a，含 4 单测 + 两态冒烟）；**仅剩数据验证**：等真实 DashScope 账单导出（key / 账期）→ 跑 `python3 scripts/bill_reconcile.py --bill <csv> --save` 确认解析与徽标展示 | `NEXT-PLAN.md:71`、`scripts/bill_reconcile.py`、`server.rs read_reconcile_report` | ⏳ 仅数据 |
| B3  | **G1 多租户**                              | 出现家庭之外的固定用户 → 触发则 SQLite 迁 PG + 鉴权/配额层（**架构级分叉，需单独立项**）                                    | `NEXT-PLAN.md:79`                            | 🕐 |
| B4  | **G2 MCP 接入**                           | 开始做 Agent 运行时 / 有外部智能体要消费 LLooM；作 server（暴露路由/缓存/定价为 MCP 工具）与作 client（编排消费 MCP 工具）**先后需定** | `NEXT-PLAN.md:80`                            | 🕐 |
| B5  | **编排状态收归 Rust**（暂停/恢复/人工介入）             | 出现该需求（与 G2 相关）；当前无此需求，B 方案够用                                                               | `ROUTING-PLAN.md:713`、`NEXT-PLAN.md:86`      | 🕐 |
| B6  | function calling / tools、多模态、多 key 分租户  | **G1 之后**才展开                                                                               | `NEXT-PLAN.md:43`                            | 🚫 |
| B7  | **验收① 阶梯价交叉单测**                         | 需真实阶梯价 spec 数据（现 spec 为平价，无真实阶梯）                                                           | `ROUTING-PLAN.md:816`（序 4）                   | 🕐 |
| B8  | **验收② 影子样本成本降 ≥60%**                    | ✅ **机制修复 + 自测达标**（2026-09-16）：影子计价 provider bug 修复后，自测 6 样本平均节省 ~90%（远超 60% 线）；真实现网流量持续复核即可 | `ROUTING-PLAN.md:819`（序 7）+ 注记 `:834`        | 🔓 持续复核 |
| B9  | **验收③ escalation 再降 ≥30%**              | 同 B8，需影子真实样本                                                                               | `ROUTING-PLAN.md:822`（序 10）+ 注记 `:848`       | 🕐 |
| B10 | **Router-R1 式 RL 路由**                   | 影子数据达**千级样本**再评估；当前「描述符评分 + EWMA + 影子评测」已覆盖其核心收益                                           | `ROUTING-PLAN.md:865`（风险注记 8）                | 🕐 |
| B11 | **Switchyard 引入**                       | 等其 **v1.0**（v0.2.0 前 API 破坏性变更）且走 libsy 库路径；当前只借鉴设计                                        | `ROUTING-PLAN.md:859`（风险注记 3）                | 🕐 |
| B12 | **BEST-Route 并行采样**（低置信请求并行采两轻量档 + 裁决）  | 远期，待级联/裁判机制成熟                                                                              | `ROUTING-PLAN.md:215`                        | 🕐 |
| B13 | **OpenRouter** **`usage.cost`** **对账源** | 未来对账增强                                                                                     | `ROUTING-PLAN.md:204`                        | 🕐 |
| B14 | **L3 关键事实抽取**（摘要之上抽实体/偏好/约束）            | 远期为超长项目型对话准备；L2 已落地，按需再加                                                                   | `CONTEXT-PLAN.md:128`                        | 🕐 |
| B15 | **会话级缓存感知路由**                             | ✅ **已落地**（2026-09-15）：① 证据由 `route()` 现查——`db.conversation_cache_avg`（会话×模型，cached_tokens>0 行均值）× 粘滞模型 `pricing.rs cache_price_delta`（主档 input − cache_read）；② 动态粘性 = `cost_weight × loss/(loss+med_ec)`，loss=平均缓存命中×价差，上限 `STICKY_CAP=0.25`，无证据回落 PR-5 固定 +0.05（`router.rs sticky_bonus`）；③ 仅 WebUI chat 路径注入证据（proxy/编排/影子重放无会话上下文走旧值）；④ **顺带修复潜伏 bug**：`recent_conversation_model` 列名错写 `model`（真实列 model_name），错误被调用方 `.ok()` 吞掉——PR-5 会话亲和实际从未生效，B15 落地时修正。+2 单测（合计 117 绿）；cap/权重阈值待真实流量观察后调 | `router.rs sticky_bonus/StickyEvidence`、`db.rs conversation_cache_avg`、`pricing.rs cache_price_delta` | ✅ 已落地 |

### 独立小项台账（C 类：无前置依赖，可随时插队）

| #  | 小项                                                                               | 完成路径（要点）                                                                                                                                                                                                                                                                                                                                                                                         | 权威落点                                                    | 代价            |
| -- | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------- | ------------- |
| C1 | **~~思考过程深度展示~~** ✅ **已完成**（2026-09-02）                                           | `_call_llm`/`_call_llm_stream` 新增 `reasoning_ref` 捕获 litellm `message.reasoning_content`（流式为 delta 累积）；简单/聚合两路径 `result` 事件携带 `reasoning`；Rust orchestrate SSE 原样透传；chatStore 捕获+meta 持久化+历史加载映射；ChatPage `Collapse` 折叠卡（字数标签+滚动容器）。**E2E 已验证**：deepseek-r1 全链路 644 字思考+真实 usage（41/827 tok）。**顺手修复既有生产 bug**：`_call_llm_stream` 引用未定义 `usage` 变量（流式带 usage\_ref 必抛 NameError，聚合阶段长期静默回退子任务原文拼接） | `ai_service.py:1303-1334`、`chatStore.ts`、`ChatPage.tsx` | 已落地           |
| C2 | **~~est\_input\_cost 分列~~** ✅ **已完成**（2026-09-15，2721d1c）                                  | `db.rs` `ensure_columns` 幂等补列（est\_input\_cost/act\_input\_cost，旧库无损升级有单测）+ `pricing.rs::actual_input_cost`（与总额恒等有单测）+ 四路径落库 + 日校准优先输入侧比值；对账已从「总额口径」升级为「输入侧分项」                                                                                                                                                             | `PRICING-PLAN.md:38`                                    | 已落地          |
| C3 | **~~`api_source`\~\~\~\~列~~** ~~（区分代理流量）~~ ✅ 已完成（2026-09-02，1a22825，随 N1 提前独立提交） | 幂等加列默认 `'webui'`，代理流量标 `'proxy'`                                                                                                                                                                                                                                                                                                                                                                 | `NEXT-PLAN.md:36`                                       | 已落地           |
| C4 | **O5 复杂判定调优（仅余阈值部分）** | ✅ **入口统一已落地**（2026-09-15）：`signals::is_complex`（= `complexity_score ≥ 0.5`）为单一真源，`router::is_complex`/`band_for` 委托之，原 router 私有正则删除、模式并入 `complexity_rx` 并集（时序/步骤/复合模式保留）。**行为差异**（已记代码注释）：单独「>100 字」「>2 句」不再硬触发，改为评分项累计（0.3/0.2，与正则或彼此组合才过线）；signals 更宽的单关键词模式（分析/评估等）纳入判定。**剩余：阈值调优**——需 50–100 条标注语料，否则是盲调 | `signals.rs is_complex/complexity_rx`、`router.rs band_for` | 中（**阻塞在语料**） |
| C5 | 多模型拆分真正生效                                                                        | **非开发项（无代码改动）**：需在设置页配置「可用模型 + 有效 Key」才生效，属配置引导                                                                                                                                                                                                                                                                                                                                                  | —                                                       | 配置            |
| C6 | EWMA α 灵敏度调整                                                                     | 仅当出现**日级调价**时（当前 α=0.15 ≈ 10 天半衰，对周级调价够用）                                                                                                                                                                                                                                                                                                                                                        | `PRICING-PLAN.md:933`                                   | 条件触发，**建议不动** |
| C7 | 多时区 chrono                                                                       | 仅当需多时区；当前纯标准库（+8 偏移 + Sakamoto + Hinnant）是**有意规避** crates.io 拉取风险                                                                                                                                                                                                                                                                                                                                | `PRICING-PLAN.md:36`                                    | 条件触发，**建议不动** |

> **C 类建议顺序**：~~C1~~（✅ 已完成）→ ~~C2~~（✅ 已完成）→ （N1 顺带 C3 ✅）。C4 等语料（可用 N1 接入后的真实流量自动采集）；C6/C7 条件未到不动。
> **C1 附注**：为验证推理链路注册了 `deepseek-r1`（dashscope，已配真实单价 5.5e-7/2.2e-6 USD/token + reasoning\_cost），保留在注册表中供 WebUI 思考折叠展示测试。

***

### 中期展望（G1/G2 决策门后再展开，仅占位）

> 源自归档 [`docs/archive/NEXT-PLAN.md:86`](./docs/archive/NEXT-PLAN.md)；触发后各自立项细化。

- Agent 运行时（编排状态收归 Rust，暂停/恢复/人工介入）→ 台账 **B5**
- 学习型路由（Router-R1 路线）→ 台账 **B10**；论文精读见本地 `research/`（01-learned-router 等 4 份，未入库）
- RAG 知识层（语义缓存之上长检索）
- 比价社区与 AIQ 基准发布 / 成本优化报告工具
- LLooM Cloud（多租户 SaaS）→ 台账 **B3**；Agent SDK / MCP 网关 → 台账 **B4**

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

| 文件 | 用途 | 状态 |
| --- | --- | --- |
| `LLooMprogress.md`（本文件） | 总进度、决策、**唯一待办真源（第六节：B 类 15 条 + C 类 7 条 + 中期展望）**、约束、文档索引 | 2026-09-16 整版重整（PLAN 归档 + 流水账压缩） |
| `docs/archive/NEXT-PLAN.md` | 已完结阶段计划（N1–N3/M1 + G1/G2 决策门 + 中期展望） | 归档，行号引用有效（+2） |
| `docs/archive/CONTEXT-PLAN.md` | 已完结上下文优化设计（Phase 1–5） | 归档，行号引用有效（+2） |
| `docs/archive/PRICING-PLAN.md` | 已完结定价系统设计（PR-1~8） | 归档，行号引用有效（+2） |
| `docs/archive/ROUTING-PLAN.md` | 已完结路由重构设计（P0–P5 + 验收指标） | 归档，行号引用有效（+2） |
| `ARCHITECTURE.md` | 分层架构、端点、数据流、技术栈 | 现行，2026-09-15 同步 |
| `README.md` / `README-ZH.md` | 用户文档（功能、快速开始、配置） | 现行，2026-09-15 同步 |
| `TEST-GUIDE.md` | 功能测试指南（含 2.7 /metrics、2.8 对账徽标） | 现行，2026-09-15 同步 |
| `research/`（本地未入库） | 路由优化研究报告集（三篇论文精读 + 开源调研，后续立项评审用） | 本地，2026-09-16 |

> **接手检查清单**：① 待办与下一步看**本文第六节**（唯一待办真源）——C4 等标注语料、B2 等真实账单、其余 B 类条件触发、中期展望等 G1/G2；② 已完结计划的设计与验收依据在 `docs/archive/`（行号引用仍有效，勿在归档文档里续写）；③ 每次改动 `cargo build`/`cargo test` 全绿 → 用户审查 → 才 push。
