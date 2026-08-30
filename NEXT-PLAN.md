
> 基线：`488d156`（ROUTING P0–P5 / PRICING PR-1~8 / CONTEXT Phase 1–5 全部落地，79 单测全绿）。
> 主题：内核已可信，下一阶段**不补内核，向外扩**——代理接入（N1）→ 数据闭环（N2）→ 信任与收尾（N3）。
> 执行约定：沿用 executing-plans 流程；每阶段构建+全量测试全绿 → 用户审查 → 才 push。本文只定阶段契约与验收，实现以函数名为锚点。

---

## 一、阶段落地表

| # | 阶段 | 一句话 | 依赖 | 状态 |
|---|---|---|---|---|
| N1 | OpenAI 兼容代理 | `/v1/chat/completions`（流/非流）+ `/v1/models`，透明走既有路由/缓存/健康/预算链 | 无 | ⏳ |
| N2 | 闭环评估 | 影子数据定时出 AIQ 报告 → UI 审查 → 一键采纳回写 routing_policy | 无（可与 N1 并行） | ⏳ |
| N3 | 信任与收尾 | O6 子任务并行 + `/metrics` 指标导出 + DashScope 账单对账 | 无（小项可插队） | ⏳ |

顺序建议 N1 → N2 → N3；N3 各小项独立，可随时插队。

---

## 二、N1：OpenAI 兼容代理（本迭代主攻）

**目标**：任意 OpenAI 客户端（ChatBox / Open WebUI / 沉浸式翻译 / Agent 框架）把 `http://127.0.0.1:7861/v1` 当上游，零改造获得 `plan()` 评分路由、两层缓存、健康容灾、预算档。

**契约**
- `POST /v1/chat/completions`：body 兼容 OpenAI（`model` / `messages` / `temperature` / `max_tokens` / `stream`）。
- `model` 语义：注册表内激活名字 → 直连该模型（查 `list_models` 拿 spec 后走既有 chat 内部路径）；`"auto"` 或未知名 → 走 `router::route()` 智能路由。
- 流式：SSE 帧序列 `role delta → content delta → finish_reason → data: [DONE]`（charset=utf-8，沿用既有 SSE 约定）；非流式：标准 `choices` + `usage`（prompt/completion/total_tokens，取自 ChatResult）。
- `GET /v1/models`：返回激活模型列表。
- 鉴权：`Authorization: Bearer $LLOOM_PROXY_TOKEN`（env 未设则不鉴权，仅限环回）。
- **配套 O2 收尾**：默认绑定 `127.0.0.1`，`LLOOM_BIND` env 可覆盖（默认关闭局域网暴露，需要时显式开）。

**落点**
- 新模块 `crates/lloom-core/src/openai_compat.rs`：OpenAI 请求/响应结构体 + 映射 + handler，挂到 server.rs 路由（与 metadata.rs / health.rs 同风格单文件模块）。
- 用量照常落 `insert_usage`（进 UsagePage 统一观测）；可选：幂等加 `api_source` 列区分代理流量（默认 `'webui'`，此项可延后）。

**验收**
- curl 非流式：`.choices[0].message.content` 与 `.usage.total_tokens` 正确；流式：帧序列完整、以 `[DONE]` 结尾。
- ChatBox 或 Open WebUI 实连可用（人工冒烟）；`model:"auto"` 请求后 usage_records 中的模型为 plan() 真实选择。
- token 配置后错误凭据 → 401；79 既有测试全绿 + 新增映射/帧序列单测。

**明确不做**（留待后续）：function calling / tools、多模态、多 key 分租户（那是 G1 之后的事）。

---

## 三、N2：闭环评估（路由策略随流量自进化）

**N2.a 报告闭环**
- `scripts/aiq_replay.py` 加 `--json` 输出；新周期 job（挂 `spawn_background_jobs()`，如 6h 间隔）执行重放，结果写新表 `policy_review`（幂等建表）。
- `GET /api/routing/review`：最近报告（三线成本/质量、AIQ、节省额、样本数、预算档触发分布）。
- WebUI：概览页或 ModelsPage 加可折叠「路由体检」卡片（遵循重要信息折叠收纳约定）。

**N2.b 权重建议（单一真源原则）**
- 在 **Rust 侧**离线重放：对 `routing_calibration` 样本网格搜索 (cost, quality, latency) 权重三元组，用 `plan()` 无副作用重放选模，输出帕累托最优建议——**不在 Python 复刻评分逻辑**（避免双真源回潮，这是 ROUTING-PLAN P0.f 的教训）。
- `POST /api/routing/review/adopt`：采纳建议权重 → upsert `routing_policy`。已核实 `get_routing_policy()` 在 route()/plan_for_task() 内**每请求读库**，采纳后下一请求即生效，无需缓存失效。
- UI：建议权重 vs 当前权重对比 + 采纳按钮；**不自动生效**，保留人工审查点。

**验收**
- job 幂等可重跑；review 端点返回真实数字（非空表）；采纳后下一请求 plan() 用新权重（单测：改库 → 重路由断言换选）。
- `--json` 输出与文本报告数字一致。

---

## 四、N3：信任与收尾（小项，可插队）

| 项 | 内容 | 验收 | 备注 |
|---|---|---|---|
| N3.a O6 并行 | Python 编排：无依赖子任务 `asyncio.gather` 并行执行（依赖关系来自 decomposer 输出的任务结构） | 多子任务复杂查询端到端延迟下降；结果拼接顺序不乱、聚合输入完整 | 当前为串行执行，依赖上下文已折叠进 messages |
| N3.b 指标导出 | `GET /metrics` Prometheus 文本格式：按模型/任务类型/预算档计数、缓存命中、fallback 事件、路由开销 | curl 可抓取、格式合法（promtool 校验可选） | 不引 Docker，只开端点 |
| N3.c 账单对账 | `scripts/bill_reconcile.py`：DashScope 账单导出 × `usage_records.actual_cost` 对账，报告偏差；UsagePage 节省卡加「已对账」徽标 | 对账报告含总偏差与分模型偏差 | **阻塞项：需真实账单导出**（等 key/账期），脚本先行 |

---

## 五、决策门（未拍板，不阻塞 N1–N3）

| 门 | 问题 | 触发条件 | 影响 |
|---|---|---|---|
| G1 | 是否多租户？ | 出现家庭之外的固定用户 | 是 → SQLite 迁 PG + 鉴权/配额层（架构级分叉，需单独立项）；否 → 继续单用户 SQLite |
| G2 | 是否接 MCP？ | 开始做 Agent 运行时 / 有外部智能体要消费 LLooM | 作 server（路由/缓存/定价暴露为 MCP 工具）与作 client（编排消费 MCP 工具）先后需定 |

---

## 六、中期与衍生（决策门后再展开，仅占位）

Agent 运行时（编排状态收归 Rust，暂停/恢复/人工介入）／RAG 知识层（语义缓存之上长检索）／学习型路由（Router-R1 路线）／比价社区与 AIQ 基准发布／LLooM Cloud（多租户 SaaS）／Agent SDK／MCP 网关／成本优化报告工具。
