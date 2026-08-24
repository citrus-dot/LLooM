# 模型上下文优化方案（CONTEXT-PLAN）

> 分析对象：`v2` 分支（2026-08-24 工作区）
> 关注文件：`api/ai_service.py`、`crates/lloom-core/src/{conversations.rs, server.rs, ai_client.rs, config.rs}`、`webui/src/{store/chatStore.ts, api.ts}`
> 目标：
> ① 单对话独立存储，服务关闭后记录不丢失；
> ② 正确高效传递对话上下文，必要时合理压缩；
> ③ 正确实现缓存命中，通过合理的缓存共享节省成本；
> ④ 其他优化。

---

## 一、现状：真实生效的数据流

```
WebUI chatStore.send()                     chatStore.ts:176
  │  history = cur.messages 全量组装       chatStore.ts:187   ← 不截断、不算 token
  ▼
POST /api/orchestrate/stream               server.rs:352
  │  security::check(仅 query)             server.rs:353
  │  cache_dir = data/chroma               server.rs:367
  │  history 原样透传                       ai_client.rs: orchestrate_stream
  ▼
POST /v1/orchestrate/stream                ai_service.py:749
  │  cache = SemanticCache(...)            ai_service.py:751  ← 每请求新建 chroma PersistentClient
  │  轻量路径: messages = [system, *history[-10:], query]   ai_service.py:774
  │  复杂路径: 子任务 *history[-10:]，聚合 *history[-6:]    ai_service.py:911 / 978
  │  cache_key = query 或 user_content     ai_service.py:781 / 922
  ▼
流结束后前端 persist() 整包回存             chatStore.ts:149 / 282
  │  messages 仅保留 role+content          chatStore.ts:150   ← 模型/成本/缓存命中/计划卡全部丢弃
  ▼
data/conversations/{id}.json 全量重写       conversations.rs:65 (save)
```

**即：存储已是"单对话单文件"（需求①的底子已在），但正确性与效率问题集中在"落盘时机、上下文构建、缓存键"三处。**

---

## 二、问题清单

### P0（正确性，必须修）

| # | 问题 | 位置 | 后果 |
|---|------|------|------|
| P0-1 | **缓存键忽略对话上下文**：`cache_key=req.query`，但 messages 里带着 `history[-10:]` | ai_service.py:781 | 同一问题在不同上下文中返回旧答案（"再详细说说"在 A 对话命中 B 对话的缓存）；反过来，跨对话的共性问答（"帮我写个快排"）又因无门控而与指代类查询混在同一键空间，命中不可信 |
| P0-2 | **流中断即整轮丢失**：用户消息 + 助手回复只在 SSE 流完整结束后 `persist()` | chatStore.ts:256-288 | 关页 / 服务崩溃 / 网络断 → 该轮完全消失，连用户输入都没落盘 |
| P0-3 | **写入非原子**：`std::fs::write` 直接覆盖 JSON | conversations.rs:68 / 158 | 崩溃在写入中途 → 整个对话文件损坏（无备份、无日志结构） |
| P0-4 | **token/cost 恒为 0**：`_call_llm` 丢弃 litellm 返回的 usage；Rust 端 `insert_usage(model, "default", 0, 0, 0.0, …)` | ai_service.py:644-674、server.rs:398 | Usage 页成本统计失真；**缓存命中"省了多少钱"无法量化**（需求③的省钱论证缺失） |

### P1（效率/健壮性）

| # | 问题 | 位置 |
|---|------|------|
| P1-1 | `SemanticCache` 每请求实例化 → 每次新建 chroma `PersistentClient`，开销大且有 sqlite 锁竞争风险 | ai_service.py:751 |
| P1-2 | 缓存无淘汰：TTL 只在读取时判断，过期条目永不删除；无容量上限，chroma 无限膨胀 | ai_service.py:189 |
| P1-3 | 历史无 token 预算、无压缩：前端全量回传 → Python 盲取 `[-10:]`；一条超长消息即可撑爆上下文 | chatStore.ts:187、ai_service.py:775 |
| P1-4 | 消息元数据不持久化（模型、token、成本、缓存命中、子任务计划卡），重开对话全部丢失 | chatStore.ts:150 |
| P1-5 | 三层重复的历史截断逻辑（前端全量 / Rust 透传 / Python 截 10），真源不唯一 | 全链路 |

### P2（改进项）

- P2-1 `security::check` 只查当前 query，不查 history（每条消息首次发送时已查过，风险低，但需在文档中明示这一假设）。
- P2-2 未利用供应商前缀缓存（DashScope qwen 系列的 context cache 对命中前缀 token 打折；Anthropic `cache_control` 经 litellm 可用）。
- P2-3 `save_or_create` 为取 title / created_at 读两次旧文件；`rand_hex` 用 nanos^pid 生成 id，概率碰撞可接受但可换标准 uuid。
- P2-4 并发写同一对话：前端整包覆盖语义下两个标签页并发保存是 last-writer-wins（会丢消息）。

---

## 三、方案设计

### 3.1 存储层：单对话独立 + 关闭不丢（需求①）

**保留 `data/conversations/{id}.json` 单对话单文件布局**（独立性已满足，前端/后端共享该布局的契约不动），做四项加固：

1. **原子写**：写 `{id}.json.tmp` → `fsync` → `rename` 覆盖。任何时刻崩溃，磁盘上要么是旧完整文件要么是新完整文件。
2. **追加式保存（新端点）**：
   ```
   POST /api/conversations/{id}/messages   body: { message: ChatMessage }
   ```
   服务器端向已存在对话**追加**单条消息（读-改-原子写），前端不再整包回传。多标签页并发、刷新竞态天然解决（追加语义无覆盖丢失）。`POST /api/conversations` 保留用于新建/重命名。
3. **两阶段落盘**（修复 P0-2）：
   - 阶段一：用户点击发送，**立即**追加 `{"role":"user", …, "meta":{"status":"pending"}}`，同轮生成并追加占位 `{"role":"assistant","content":"","meta":{"status":"generating"}}`；
   - 阶段二：流结束（正常/失败/中断），以 `PATCH /api/conversations/{id}/messages/{index}`（或带消息 id 的追加语义）回填 assistant 内容与元数据。
   - 崩溃恢复：加载对话时若发现 `status:"generating"` 的末尾消息，标记为"回答中断"并在 UI 展示，可点"重试"。
4. **消息 schema 扩展**（修复 P1-4，前端向后兼容——旧文件无 meta 字段照常渲染）：
   ```json
   {
     "role": "user" | "assistant",
     "content": "…",
     "meta": {
       "created_at": "ISO-8601",
       "status": "done" | "generating" | "interrupted",
       "model": "qwen-plus",
       "input_tokens": 1234, "output_tokens": 567,
       "cost": 0.0123,
       "cache_hit": false, "cache_sim": 0.87,
       "plan": { "sub_tasks": [ … ] }
     }
   }
   ```

> **决策点 A（存储载体）**
> a) **沿用 JSON 单文件 + 原子写 + 追加端点（推荐）**——改动最小，百轮以内对话读写都是毫秒级，且保留与现有前端/备份工具的兼容；
> b) SQLite `messages` 表（`lloom.db` 内）——长对话读写更稳、可做范围查询，但放弃"单文件即一个对话"的可拷贝性，前端 load 接口要加聚合层；
> c) 每对话一个 JSONL 追加日志——写入最廉价（纯 append），但读取端需做合并与压缩整理，复杂度前移。

### 3.2 上下文构建与压缩（需求②）

**真源迁移**：前端 `send()` 只发 `{ conversation_id, query }`（新建对话仍走旧整包路径一次拿 id）；**Rust 端负责加载对话并构建 history**，Python 只收构建好的 messages。三处截断逻辑收敛为一处。

**Token 预算器**：Python 端用 `tiktoken`（仓库已有 `tiktoken_cache/`，离线可用）按消息计数。模型上下文预算：

```
budget = model_ctx_window − max_output − 安全余量(512)
         ├─ system prompt     固定保留
         ├─ 滚动摘要          ≤ 15% budget（可选，见 L2）
         ├─ 近期轮次          从最新往前装满剩余预算
         └─ 当前 query        永远保留（超长 query 单独截尾并提示）
```

**压缩阶梯**（决策点 B）：

- **L1 token 预算截断**（默认启用）：从最新消息向前装满预算，装不下的直接丢弃。零额外成本、零延迟。
- **L2 滚动摘要**（超预算时触发）：用便宜模型（`DECOMPOSER_PREFERENCE` 里的 `qwen3.6-flash`）把"被 L1 丢弃的更早轮次"摘要为一段 `<summary>` 文本，作为 system 之后的第二条消息注入。摘要**持久化进对话 JSON**（`"summary": {"text": …, "covers": [0, 23]}`，covers 为消息区间），下次只有区间外新增轮次才需要增量重算——**摘要本身不重复计费**。
- **L3 关键事实抽取**（远期可选）：从摘要再抽实体/偏好/约束清单，格式化注入，适合超长项目型对话。

**前缀稳定性**（为 3.4-2 供应商前缀缓存铺路）：摘要按**块**更新（每约 10 轮重算一次，而非每轮），保证 system+summary 前缀在多轮间稳定，最大化供应商侧前缀命中。

> **决策点 B（压缩策略）**
> a) **L1+L2 混合（推荐）**——短对话零开销，长对话摘要一次多次复用；摘要成本用 flash 级模型约 ¥0.001/次；
> b) 仅 L1 截断——实现最简，但长对话早期关键信息静默丢失，用户感知为"AI 忘事"；
> c) L1+L2+L3 全家桶——信息保真最高，实现与维护成本翻倍，建议等 L2 落地后按真实需求再加。

### 3.3 缓存命中与共享（需求③）

**第一步先修正确性（P0-1），再谈共享：**

1. **缓存键改造**：
   ```
   cache_key = normalize(query)
   context_fingerprint = None                      # 上下文无关查询
                     | hash(conv_id + 最近2轮摘要)   # 上下文相关查询
   存取键 = hash(model + system_prompt 版本 + context_fingerprint + cache_key)
   ```
2. **上下文无关判别**（决定能否跨对话共享）：
   - 启发式先行：代词/指代词正则（它/这/那/上面/刚才/继续/再…）+ 首问检测（对话前 2 轮内默认无关）+ 独立成问（含完整主谓/命令式）；
   - 命中启发式灰区时走轻量 LLM 判别（与 classify 共用 flash 模型，输出 `context_free: bool`）；
   - **上下文相关查询只在本会话命名空间内命中**（chroma `where={"conv_id": …}`），永不跨对话。
3. **两层缓存结构**：
   - **L1 精确缓存（新增，SQLite `cache_exact` 表）**：键为上述哈希，O(1) 查询、零误报，跨对话共享的主力（FAQ、常用指令、代码模板类请求）；
   - **L2 语义缓存（现有 chroma，保留）**：仅服务上下文无关查询，容错"措辞不同但意图相同"；命中 L1 后短路 L2。
4. **单例化（P1-1）**：`SemanticCache` 改为模块级单例 + `threading.Lock` 包住 query/upsert，chroma client 全请求复用。
5. **淘汰策略（P1-2）**：后台线程每 5 分钟清理 TTL 过期条目；LRU 容量上限默认 5000 条（`.env` 可配 `LLOOM_CACHE_MAX_ENTRIES`），超出按 `cached_at` 淘汰。
6. **命中计量（P0-4 联动）**：命中时按"未命中本应花费的估算成本"记 `saved_cost` 入 usage 表；UsagePage 新增"缓存命中率 / 累计节省"卡片。
7. **写缓存门控**：仅缓存 `status=done` 且非流中断的响应；`temperature > 0.7` 或含"随机/今天/现在"等时效词的查询不写缓存。

> **决策点 C（共享范围）**
> a) **上下文无关全局共享 + 上下文相关会话内共享（推荐）**——正确性与省钱兼得，实现重心在判别器；
> b) 仅会话内缓存——最安全但省钱空间小（LLooM 单用户场景重复问答主要跨会话发生）；
> c) 全部全局共享（现状键修复版）——命中率高但脏命中风险高，不建议。

### 3.4 其他优化（需求④）

1. **精确成本核算**：`_call_llm` / `_call_llm_stream` 捕获 litellm 响应的 `usage`，经 `task_done` / `result` SSE 事件回传；Rust 端 `insert_usage` 写真实 token 数与成本（非流式路径 litellm 直接给 usage；流式路径 `stream_options.include_usage` 已开，编排路径补上即可）。子任务成本展示从"估算值"升级为"实际值 + 估算值"对照。
2. **供应商前缀缓存**：qwen 系列（DashScope）的隐式 context cache 对命中前缀的输入 token 计价打折；配合 3.2 的前缀稳定策略（system+summary 块状更新、消息只追加不重写）可稳定吃到折扣。Anthropic 路径经 litellm 的 `cache_control` 显式标记。落地项：消息拼装顺序固定化 + 验证 DashScope 账单侧缓存命中量。
3. **缓存/成本看板**：UsagePage 扩展——命中率曲线、节省金额累计、按模型缓存分布；数据全部来自修复后的 usage 表。
4. **安全边界文档化**：`security::check` 的"每条消息首次发送时已检查"假设写入 `ARCHITECTURE.md`；追加端点对 content 做与首查一致的 PII 规则（防止绕过首查的持久化注入）。
5. **杂项**：`save_or_create` 合并两次旧文件读取为一次；`rand_hex` 换 `uuid::Uuid::now_v7()`（时间有序，利于排序与索引）；对话导出功能（单文件 JSON 天然适合）。

---

## 四、实施阶段

| 阶段 | 内容 | 涉及文件 | 依赖 |
|------|------|----------|------|
| **Phase 1：正确性修复** | 原子写、两阶段落盘 + 追加端点、缓存键加 context_fingerprint、usage 捕获回传 | conversations.rs、server.rs、ai_client.rs、ai_service.py、chatStore.ts、api.ts | 无 |
| **Phase 2：上下文架构迁移** | 前端只发 (conversation_id, query)，Rust 构建历史；消息 meta 持久化与回显；`interrupted` 恢复 UI | 同上 + ChatPage.tsx | Phase 1 |
| **Phase 3：压缩** | tiktoken 预算器、L1 截断、L2 滚动摘要（持久化 + 区间增量重算） | ai_service.py、conversations.rs | Phase 2 |
| **Phase 4：缓存增强** | L1 精确缓存表、上下文无关判别器、单例化、淘汰线程、看板 | ai_service.py、db.rs、server.rs、UsagePage.tsx | Phase 1（可与 Phase 2/3 并行） |
| **Phase 5：供应商前缀缓存** | 前缀稳定化验证、DashScope 缓存命中量核对 | ai_service.py | Phase 3 |

## 五、测试不变量（验收标准）

1. **持久性**：发送中 `kill` lloom-server 进程 → 重启 → 对话列表完整，末轮显示"回答中断"，用户消息在，无损坏 JSON 文件（`jq` 全量校验通过）。
2. **缓存正确性**：对话 A 问"它怎么样"（指代前文）→ 对话 B 相同问法，**不得**返回 A 的缓存答案；跨对话问"用 Rust 写一个快排"两次 → 第二次精确缓存命中，响应含 `cache_hit:true` 且 `saved_cost>0`。
3. **上下文预算**：构造 30 轮长对话（含一条 8k token 长消息）→ 每次请求拼装后的 prompt tokens ≤ 预算上限（日志断言），早期关键事实经摘要保留（人工抽查问答可用）。
4. **成本核算**：单轮对话后 usage 表的 input/output tokens 与供应商控制台账单同数量级（±10%）。
5. **并发**：双标签页同时向同一对话发送 → 两条用户消息与两条回复**全部**落盘、无覆盖丢失。
6. **缓存淘汰**：灌入 > 上限条目并等待清理周期 → chroma 条目数回落到上限内，TTL 过期条目被物理删除。

## 六、风险与回退

- 每阶段独立可发布、可回退（feature flag：`.env` 的 `LLOOM_CONTEXT_V2=1` 切换新旧行为，旧整包保存路径保留一个版本周期）；
- chroma schema 变更（metadata 加 `conv_id`/`fingerprint` 字段）对旧条目不兼容 → 首次启动检测到旧集合时整体重建（缓存可弃，对话数据无损）；
- 滚动摘要的"摘要质量"依赖 flash 模型，摘要错误会污染后续上下文 → 摘要消息在 UI 可折叠查看原文区间，用户可强制重算。
