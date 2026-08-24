# 模型定价表优化方案 v1

> 定位：`ROUTING-PLAN.md` 阶段 P2（定价表系统）的**细化与扩展版**。
> 分析对象：`v2` 分支 @ 2026-08-24；DB 快照：7 模型（6 活跃）、usage_records 仅 3 条全零脏数据。
> 外部依据：DeepSeek 峰谷计费公告（2026-08-17/08-23）、阿里云百炼 Context Cache 文档、
> LiteLLM cost_calculator 源码、OpenRouter Prompt Caching + Sticky Routing 博客（2026-07-21）。
> 核心主张：**定价表从「两个标量」升级为「分项 × 时段 × 阶梯 × 来源」的价格规格（PriceSpec），
> 计算引擎从「in×p+out×p」升级为与 LiteLLM 同构的分项账单模型，
> 并以真实用量对账驱动持续校准。**

---

## 一、现状与问题（定价视角）

代码与数据复盘结论（与 ROUTING-PLAN §二互相印证，此处只列定价相关）：

| # | 问题 | 证据 | 后果 |
|---|---|---|---|
| 1 | **量纲错误 10 倍** | DB 六模型全部满足 `DB值 = 官方元/M × 1.3889e-06`；DashScope 系虚高 10×，gpt-4o 正确 | 跨供应商比价方向反了（gpt-4o 显得比 qwen3-max 便宜，实际贵 7×） |
| 2 | **价格只有两个标量** | `models` 表仅 `input_cost_per_token` / `output_cost_per_token`；`models.rs:54` 与 `ai_service.py:103` 两处 `calculate_cost` 均为裸乘法 | 缓存命中、缓存写入、阶梯价、峰谷、batch 折扣全部无法表达；成本永远高估 |
| 3 | **KV/prompt cache 完全未计价** | `ai_service.py` 只读 `prompt_tokens`/`completion_tokens`，不读 `prompt_tokens_details.cached_tokens` | 百炼隐式缓存命中本应 20% 计费、DeepSeek 命中 0.1×，账面成本虚高最多 5× |
| 4 | **无时段维度** | 无任何峰谷概念 | DeepSeek 通道夜间/周末半价的机会成本白白放弃；也无法在高峰期"避 DeepSeek 用 qwen" |
| 5 | **无对账** | `server.rs:398` 唯一 `insert_usage` 调用点写死全 0；真实 `res.cost` 拿到不落库 | 表价错没有任何反馈信号；校准无从谈起 |
| 6 | **定价按模型名、不按通道** | `deepseek-v3` 注册在 dashscope provider 下 | 同一模型经百炼（元、显式缓存可用）与 DeepSeek 官方（峰谷、隐式缓存）价格结构完全不同，主键必须是 (provider, model) |
| 7 | **两个成本真源各自为政** | `models.rs` 与 `ai_service.py` 各写一份裸乘法 | 修一处漏一处（量纲修正就必须修两处） |

一句话：**当前"定价表"既算不对账单，也给不出可信的比价信号，路由的成本项等于噪声。**

---

## 二、业界计费事实盘点（2026-08 核实，方案的地基）

### 2.1 缓存计费矩阵（各通道实测口径）

| 通道 | 命中读（cache read） | 写入（cache write） | 触发方式 | 最小缓存 | TTL |
|---|---|---|---|---|---|
| 百炼隐式缓存（qwen 系默认） | **0.20× 输入价** | 1.0×（无溢价） | 自动、不可关 | 256 tok | 不保证，定期清理 |
| 百炼显式缓存（qwen3.x-max/plus/flash、deepseek-v3.2 等） | **0.10× 输入价** | **1.25× 输入价** | `cache_control` 标记，≤4 断点 | 1024 tok | 5 min，命中重置 |
| 百炼 session 缓存（Responses API） | 0.10× | 1.25× | 头 `x-dashscope-session-cache: enable` | 1024 tok | 5 min，命中重置 |
| DeepSeek 官方 | **0.10× 输入价** | 1.0× | 自动 | 64 tok | 不保证 |
| OpenAI（GPT-5.6 及之后） | 0.25–0.50× | 1.25× | 自动/显式 | 1024 tok | 5–60 min |
| Anthropic | 0.10× | 1.25×（5min）/ 2.0×（1h） | 自动/显式 | 1024 tok | 5 min / 1 h |
| Gemini / Grok / Moonshot（隐式） | 0.25× | 免费 | 自动 | — | — |

三条硬结论：
1. **显式缓存有盈亏平衡点**：写价 1.25×，读价 0.1× → 前缀至少要被**复用 ≥2 次**才回本
   （业界口径：命中率低于约 15% 时显式缓存反而更贵）。显式缓存必须做成按任务/会话可开关的旋钮，不能全局开。
2. **隐式缓存是纯收益**（写不溢价、自动生效），唯一代价是**要求 prompt 前缀字节级稳定**。
3. 缓存与 batch 互斥（百炼 Batch 调用不享缓存折扣）；batch 通道独立 5 折。

### 2.2 峰谷分时计费（DeepSeek，2026-08-17 起，08-23 修订）

| 规则 | 内容 |
|---|---|
| 工作日高峰 | 北京时间 9:00–12:00、14:00–18:00，原价 |
| 工作日其余 | 谷价 = 高峰 50% |
| 周六/周日 | **全天谷价**（08-23 新规） |
| 法定节假日 | 全天谷价 |
| 实例（V4-Pro 高峰） | 输入未命中 9 元/M、输出 27 元/M、**命中 0.3 元/M**（谷时各半） |

信号意义：**国产头部 API 已进入"分时电价"时代**（智谱年内三连涨、腾讯云两涨、行业均价 Q2 输入 +48%/输出 +80%）。
静态价目表的保鲜期以周计，"快照 + 刷新 + 对账校准"不再是锦上添花而是必需。

### 2.3 LiteLLM 成本计算结构（本方案直接对齐）

LiteLLM `cost_calculator` 的通用公式（已是事实标准）：

```
cost = prompt_tokens × input_cost_per_token
     + cached_tokens × cache_read_input_token_cost
     + cache_creation_tokens × cache_creation_input_token_cost
     + reasoning_tokens × output_cost_per_reasoning_token
     + …（audio/image/second 等模态分项）
```

- token 分类来自 `usage.prompt_tokens_details.cached_tokens`（LiteLLM 已对各供应商标准化）；
- 价格表字段就是分项键名（`cache_read_input_token_cost` 等）；
- 阶梯价用 `*_above_200k_tokens` 键；batch 用 `_flex` 键；优先级用 `_priority` 键；
- 自定义价通过 `model_info` 覆盖，来源优先级与本项目 P2 设计一致。

**LLooM 的 Python 侧就跑在 litellm 上**：`cached_tokens` 数据已经在响应里，只是我们没读。
这是本方案里性价比最高的一笔：**读一个字段，账单精度立刻从"5 倍误差"到"分项精确"。**

### 2.4 OpenRouter 的两条可移植经验

1. **缓存计价进路由决策**：cache read 价（0.1×–0.5×）参与比价，多轮会话的"有效输入单价"远低于标称价；
2. **Sticky Routing**：同会话后续请求粘住持有热缓存的同一端点（显式 `session_id` 作粘性键），
   防止"轮 2 被路由到冷端点、缓存全废"。LLooM 自己就是路由器，这个能力天然该有。

---

## 三、定价表数据模型：PriceSpec

### 3.1 设计原则

1. **主键是 (provider, model) 通道**，不是模型名——同一模型不同通道价格结构不同；
2. **分项计价**：每个价格分量独立成列，禁止把折扣揉进标量（当前 10× 错误的教训之一就是量纲隐式）；
3. **维度分离**：基础价（分项）× 阶梯（按输入长度）× 时段（按请求时刻）× 通道折扣（batch）四层正交，
   每层可独立为空 = 无该维度；
4. **币种与量纲强制**：全库统一 **USD/token**，写入断言 `[1e-9, 1e-3]`；
   CNY 报价在 overlay/录入侧换算（汇率配置项 `cny_to_usd`，默认 7.2）；
5. **来源分级**：沿用 ROUTING-PLAN 的 `manual > overlay > litellm_remote > litellm_packaged > heuristic`。

### 3.2 Schema（替换 ROUTING-PLAN P0.b 中定价相关列的细化版）

```sql
-- 新表：价格规格。models 表保留 display 用聚合列，真源在这里。
CREATE TABLE price_specs (
    provider        TEXT NOT NULL,
    model           TEXT NOT NULL,
    -- 分项基础价（USD/token；NULL = 该通道无此计费项）
    input_cost              REAL NOT NULL,            -- 未命中输入
    output_cost             REAL NOT NULL,            -- 输出
    cache_read_cost         REAL,                     -- 命中输入（NULL=通道无缓存计费区分）
    cache_write_cost        REAL DEFAULT 0,           -- 显式写价（隐式缓存通道=0）
    reasoning_cost          REAL,                     -- 思考 token 输出价（若有区分）
    -- 维度修饰
    tiered_json     TEXT,    -- [{"max_input":32768,"in":..,"out":..,"cache_read":..}, ...]
    zone_json       TEXT,    -- 时段规则（见 §3.4），NULL=不分时
    batch_multiplier REAL DEFAULT 0.5,                -- batch 通道折扣
    -- 溯源与保鲜（沿用 ROUTING-PLAN P2 设计）
    price_source    TEXT DEFAULT 'unknown',
    price_updated_at TIMESTAMP,
    price_stale     INTEGER DEFAULT 0,
    effective_from  TEXT,    -- 供应商公告生效日，如 '2026-08-23'
    PRIMARY KEY (provider, model)
);

-- 新表：渠道级时段规则（峰谷规则挂在 provider 上，模型默认继承，可覆盖）
CREATE TABLE provider_zones (
    provider   TEXT NOT NULL,
    rule_json  TEXT NOT NULL,   -- 规则数组，见 §3.4
    PRIMARY KEY (provider)
);

-- 新表：实测分项统计（对账校准的存储，见 §六）
CREATE TABLE price_calibration (
    provider   TEXT, model TEXT,
    as_of      TEXT,               -- 'YYYY-MM-DD'，按天聚合
    calls      INTEGER,
    est_cost   REAL,               -- 路由前估算之和
    act_cost   REAL,               -- 分项公式按真实 usage 计算之和
    cache_hit_rate  REAL,          -- cached_tokens / prompt_tokens
    out_in_ratio    REAL,          -- output_tokens / prompt_tokens
    PRIMARY KEY (provider, model, as_of)
);
```

`models` 表中原有两个 cost 列降级为**展示聚合列**（由 PriceSpec 投影生成），路由与计费代码一律读 `price_specs`。
双真源问题（问题 7）就此消除：Rust 与 Python 共享同一套 PriceSpec JSON（Rust 决策、Python 执行时带价）。

### 3.3 overlay（`model_catalog.json`）条目形态

百炼模型仍靠 overlay 维护（自动源无 DashScope 价格，ROUTING-PLAN §3.4 已实测）。示例：

```json
{
  "dashscope/qwen3-max": {
    "input_cost": 3.47e-7, "output_cost": 1.389e-6,
    "cache_read_cost": 6.94e-8,
    "cache_write_cost": 0,
    "tiered_json": [
      {"max_input": 32768,  "in": 3.47e-7, "out": 1.389e-6, "cache_read": 6.94e-8},
      {"max_input": 131072, "in": 5.56e-7, "out": 2.222e-6, "cache_read": 1.11e-7},
      {"max_input": 262144, "in": 9.72e-7, "out": 3.889e-6, "cache_read": 1.94e-7}
    ],
    "explicit_cache": {"read": 0.1, "write": 1.25, "min_tokens": 1024, "ttl_min": 5},
    "currency_hint": "CNY", "cny_list_price": {"in": 2.5, "out": 10.0, "per": "1M"},
    "source": "overlay", "effective_from": "2026-08-01"
  },
  "deepseek-official/deepseek-v4-pro": {
    "input_cost": 1.25e-6, "output_cost": 3.75e-6, "cache_read_cost": 4.17e-8,
    "zone_ref": "deepseek",
    "currency_hint": "CNY", "cny_list_price": {"peak": {"in": 9, "out": 27, "cache_read": 0.3}},
    "source": "overlay", "effective_from": "2026-08-23"
  }
}
```

`cny_list_price` 是**人工核对锚点**：WebUI 校对页直接显示官方人民币原价，保存时按当前汇率换算成 USD/token，
量纲错误从此在录入口被挡住（而不是事后反推）。

### 3.4 峰谷规则形态（zone_json）

DeepSeek 2026-08-23 规则的表达（规则数组**先具体后兜底**，首条命中生效）：

```json
[
  {"days": ["sat", "sun"],                     "hours": "*",        "multiplier": 0.5},
  {"days": ["mon","tue","wed","thu","fri"],    "hours": "9-12,14-18", "multiplier": 1.0},
  {"days": ["mon","tue","wed","thu","fri"],    "hours": "*",        "multiplier": 0.5},
  {"holidays": "CN",                            "hours": "*",        "multiplier": 0.5}
]
```

语义要点：
- `multiplier` 作用于该通道**全部分项价**（含 cache_read——DeepSeek 实例中命中价谷时 0.15 = 峰时 0.3 × 0.5，验证成立）；
- 节假日表内置随版本更新（每年国务院公布后 overlay 一并更新）；
- 时区：规则按**北京时间**解释，落库时存 UTC 时刻 + 明确 `tz: "Asia/Shanghai"` 字段，避免本机时区漂移；
- qwen 系当前无峰谷 → `zone_json IS NULL`，multiplier 恒 1（**设计必须前向兼容：今天只有 DeepSeek 分时，明天可能人人分时**）。

---

## 四、成本计算引擎

### 4.1 分项账单公式（事后精确计算，对齐 LiteLLM）

```
actual_cost(m, usage, t)
  = z(m, t) × [
      (prompt_tokens − cached_tokens) × P.input
      + cached_tokens                × P.cache_read      // 无此列则并入 input
      + cache_write_tokens           × P.cache_write
    ]
  + z(m, t) × completion_tokens × P.output
  + z(m, t) × reasoning_tokens  × P.reasoning           // 无区分则并入 output
```

其中 `P = tier_select(m, prompt_tokens)`（阶梯价按**当次请求输入长度**选档），`z(m,t)` 为时段系数。

实现落点（消除双真源）：
- **Rust `lloom-core/src/pricing.rs`**：唯一实现。`PriceSpec` 结构体 + `actual_cost()` + `effective_price()`（§五）+ 单测；
- Python `ai_service.py` 删除 `_estimate_cost`，改为把 litellm 返回的完整 usage
  （`prompt_tokens`、`completion_tokens`、`prompt_tokens_details.cached_tokens`、
  `completion_tokens_details.reasoning_tokens`、`cache_creation_input_tokens`）
  **原样透传回 Rust**，由 Rust 统一计价落库；
- 阶梯档位判断用 `prompt_tokens`（含缓存命中部分——供应商按请求总输入长度选档）。

### 4.2 usage 字段补全（前置依赖，一处小改）

`ai_service.py` 现有两处（非流式 `:486`、流式 `:511`）只取了两个 token 数。补：

```python
usage_payload = {
    "prompt_tokens": usage.prompt_tokens,
    "completion_tokens": usage.completion_tokens,
    "cached_tokens": getattr(getattr(usage, "prompt_tokens_details", None), "cached_tokens", 0) or 0,
    "reasoning_tokens": getattr(getattr(usage, "completion_tokens_details", None), "reasoning_tokens", 0) or 0,
    "cache_creation_tokens": getattr(usage, "cache_creation_input_tokens", 0) or 0,
}
```

百炼（OpenAI 兼容 + DashScope 原生）与 DeepSeek 官方均返回 `cached_tokens`（litellm 已标准化到
`prompt_tokens_details.cached_tokens`）——**数据已在线上流过，只差接线。**

### 4.3 成本预估器（事前估算，路由门槛用）

```
est_cost(m, task, est_in, est_out, t)
  = z(m,t) × [ est_in × eff_in(m,task) + est_out × P.output ]

eff_in(m, task) = h(m,task) × P.cache_read + (1 − h(m,task)) × P.input
```

- `h(m,task)`：该模型×任务的历史缓存命中率（EWMA，冷启动 0）——来自 `price_calibration`；
- `est_in`：tiktoken 精确算（`tiktoken_cache/` 已有）；
- `est_out`：该 task_type 历史 P50（`price_calibration.out_in_ratio` × est_in 兜底）；
- 预估**不含 cache_write**（保守估计不指望显式缓存收益；显式缓存开启时另行加项）。

这个 `est_cost` 就是 ROUTING-PLAN P0.d `plan()` 硬门槛 `est_cost ≤ max_cost_per_request` 与
P5 预算档位判断的输入。**关键改进：比价用 `eff_in` 而非标称 `P.input`**——
一个命中率 60% 的 qwen3-max，有效输入价 = 0.6×0.1×P + 0.4×P = 0.46×P，
在多轮会话任务上可能反超标称更便宜的模型。

---

## 五、与路由策略的衔接

定价表不是报表，是路由的**成本信号源**。五个衔接点：

### 5.1 有效单价参与软评分（替换标称价）

ROUTING-PLAN P0.d 的软评分 `score = q_w·quality − c_w·norm(est_cost) − l_w·latency + ...` 中，
`est_cost` 全面改用 §4.3 的预估值（含时段系数、命中率期望、阶梯档）。
效果：**多轮/长前缀任务自动偏向缓存友好模型；高峰时段自动偏向无峰谷溢价或谷价通道。**

### 5.2 会话亲和（Sticky Routing 本地化，新增信号）

- 同 `conversation_id` 的后续请求，在候选评分中给"上一轮所用模型"加分（如 +0.05），
  前提：该模型仍在候选集且 `cache_read_cost` 非空（缓存敏感通道才粘）；
- 若上一轮模型已被门槛淘汰（健康/预算），正常切换不硬粘——**粘性是加分项不是约束**；
- 对应 OpenRouter 实测教训：会话轮 2 换端点 = 热缓存全废，长会话成本差可达 2–3×。

### 5.3 Prompt 结构规范（零成本、纯收益，立即可做）

百炼命中逻辑是**前缀匹配**（"已缓存 ABCD，请求 ABE 可命中，BCD 不可"）。落成项目约定：

1. 系统提示词、工具定义、检索文档、few-shot 样例等**稳定内容一律置前**，用户消息与对话尾部置后；
2. 同一会话内前缀保持**字节级稳定**（禁止每轮重排工具列表、在 system prompt 里插时间戳/随机数）；
   需要时间信息时放最后一条 user message；
3. `signals.rs` 加一个启发式信号 `prefix_stability`：对同会话相邻请求做前缀 diff，
   前缀变化率高的会话在 WebUI 提示"缓存命中率受损"。

这一条不写代码也能让现有百炼隐式缓存命中率显著提升（20% 计价直接生效），是**全方案回收最快的一项**。

### 5.4 峰谷感知调度（可延迟任务挪谷时）

- 路由增加"可延迟"标记位：编排批处理、夜间评测、影子评测（P1.d）、**探针（§七）**默认 `deferrable=1`；
- 实时 `chat/stream` 永不延迟；
- 对 `deferrable` 任务：若当前处于某候选通道的高峰时段且该通道存在分时，评分中成本项用
  "预计执行时刻的价"（简单实现：若 2 小时内进入谷时则按谷价估算并给该候选加分，
  任务进延迟队列由 P4 编排器在谷时窗口执行）；
- 首期简化版可只做**提示不做调度**：WebUI 用量页展示"若以下任务挪至谷时可省 ¥X/月"。

### 5.5 batch 通道（远期）

编排路径中无实时要求的子任务理论上可走百炼 Batch（5 折，但无缓存折扣、非实时）。
首期只把 `batch_multiplier` 留在 schema 里，不实现通道——先让单次调用成本算准。

---

## 六、动态校准闭环：用真实消耗修定价表

三层闭环，逐层收紧：

### 6.1 第一层：单次对账（每次请求，自动）

```
每次请求落库两条成本：
  est_cost  —— 路由前估算（§4.3，含所用价与命中率假设）
  act_cost  —— 分项公式按真实 usage 算（§4.1）
偏差比 r = act_cost / est_cost（剔除 est_out 误差：先只比输入侧分项）
```

依赖 ROUTING-PLAN P1.a（usage 落库修复）先行——本方案在其 schema 上追加
`cached_tokens`、`reasoning_tokens`、`est_cost`、`zone_multiplier_at_req`、`conversation_id` 列。

### 6.2 第二层：日级校准因子（后台 job，自动）

每天对每 (provider, model)：

```
calibration = Σ act_cost / Σ est_cost        （≥50 样本才计算）
cache_hit_rate = Σ cached_tokens / Σ prompt_tokens   → EWMA 更新 h(m,task)
```

两个用途：
1. **喂回预估器**：`h` 与 `out_in_ratio` 更新后，下一天的 est_cost 更准（自收敛）；
2. **标记价格过期**：若输入侧分项对账持续 `r > 1.2` 且剔除命中率变化后仍超
   ——说明**表价低于实际**（供应商涨价了），置 `price_stale=1`，
   WebUI 黄点提示；`price_source='manual'` 的不自动改价只提示。

反向（表价高于实际，r < 0.8）同样提示——可能是供应商降价或切换了计费结构。

### 6.3 第三层：月度人工锚定（低频，WebUI 校对页）

- 「模型与定价」页：每模型显示官方人民币原价（`cny_list_price` 锚点）、当前换算 USD、
  `price_source` 徽标、`price_updated_at`、`price_stale`、**近 30 天对账偏差 r**；
- 一键操作：「采纳刷新价」（refresh 价转正为 manual）、「手工改价」（强制写 manual + 记生效日）；
- 可选导入：百炼费用中心月账单 CSV → 按模型比对"账单金额 vs 本地累计 act_cost"，
  偏差 >5% 告警（对应 ROUTING-PLAN P2.c 对账思想，用自家账单而非 OpenRouter）。

### 6.4 校准的探针加速（冷启动）

新注册模型没有流量 → `h` 与校准因子永远是冷启动值。§七的探针兼任**校准燃料**：
每模型每天固定两条最小请求（一条写缓存一条读缓存），一天内即可拿到该通道的
`cache_read 计价是否符合表值`、`cached_tokens 字段是否存在`、`实际单价量级`三重验证。

---

## 七、常开探针（响应性测试）的成本监控设计

用户场景明确：**小数据量、常开、测响应性**。设计原则是让探针"一鱼四吃"且**预算硬顶**。

### 7.1 探针任务定义

| 项 | 设计 |
|---|---|
| 频率 | 每模型每小时 1 轮（可配，默认对 is_active 云端模型） |
| 载荷 | 轮内两条：①固定 300-token 前缀 + "ok?"（写缓存/暖机）；②同前缀 + "ok?"（应命中缓存，验证 cache_read 计价） |
| 度量 | TTFB、总延迟、错误码、`cached_tokens>0`（命中验证）、分项成本 |
| 记账 | `usage_records.task_type='probe'`，`conversation_id='probe:{model}'`——**与用户流量分账**，用量页默认过滤 |
| 执行时刻 | `deferrable=1` → 尽量落谷时窗口（DeepSeek 通道直接半价） |

### 7.2 一鱼四吃

1. **健康探测**（喂 ROUTING-PLAN P3 的 `health_state`：连续失败→degraded/down）；
2. **响应性基线**：每小时 TTFB 序列 → P50/P95 → `avg_latency_ms` 真源；
3. **定价校准**：第二条若未命中或 `cache_read` 账单不符表值 → `price_stale=1` 直接证据；
4. **能力冒烟**：载荷末尾附一个极小判定任务（如"回答 1+1"校验首 token 合法），同时给 `ewma_quality` 供数。

### 7.3 探针自身的预算硬顶（防止"监控比业务还贵"）

```
月探针预算上限 B_probe（默认 ¥5，WebUI 可改，0=关）
单轮成本上限 = max(0.002 USD, 10×该模型历史单轮 P95)   ——异常放大即熔断该模型探针
预算耗尽 → 自动降频（每小时→每天一次），仍超 → 只探本地与免费通道
WebUI 用量页固定展示：「探针本月消耗 ¥X / ¥Y（N 次探测，发现 M 次异常）」
```

按当前价目表测算：5 个云端模型 × 24 轮/天 × 2 条 × ~400 tok
（其中一半 20% 计价）≈ **每月 ¥1–3**，在默认预算内且能覆盖全部校准需求。

---

## 八、落地顺序（对齐 ROUTING-PLAN，编号沿用）

| 顺序 | 内容 | 依赖 | 验收标准 |
|---|---|---|---|
| PR-1 | **usage 字段补全**（cached/reasoning/est_cost/conv_id 列 + `ai_service.py` 透传） | ROUTING-PLAN 步骤 1（P1.a）先行或同做 | 一次百炼对话后 usage_records 出现非零 `cached_tokens`，act_cost 分项正确 |
| PR-2 | **量纲修正 + PriceSpec 表迁移**（price_specs/provider_zones 建表，models 降级为投影，录入断言） | PR-1 | qwen 系列降为 1/10；越界单价被拒；Rust/Python 单一计价真源 |
| PR-3 | **pricing.rs 引擎**（actual_cost + est_cost + tier_select + zone_multiplier，全量单测） | PR-2 | 单测覆盖：缓存分项、阶梯边界、峰谷边界（含周末/节假日）、batch 系数、空 cache_read 通道 |
| PR-4 | **Prompt 结构规范落地**（system prompt 前缀稳定化改造 + `prefix_stability` 信号） | 无（可与 PR-1 并行） | 同会话第 2 轮起百炼 `cached_tokens > 0`；周命中率报表可见 |
| PR-5 | **路由衔接**：eff_in 进评分、会话亲和加分、est_cost 门槛 | PR-3 + ROUTING-PLAN 步骤 4 | 影子评测下多轮任务成本降 ≥25%（缓存 + 亲和贡献）且质量无回退 |
| PR-6 | **日级校准 job + WebUI 定价页**（校准因子、price_stale 黄点、cny 锚点校对、对账偏差展示） | PR-1/2/3 | 人工改 DB 价制造偏差 → 7 天内自动标 stale；改价后对账 r 回归 [0.8,1.2] |
| PR-7 | **探针系统**（probe 任务 + 预算硬顶 + 谷时调度 + 用量页探针视图） | PR-3 | 探针月成本 ≤ 预算；手动改某模型 API key 为错值 → 2 轮内 health_state=down |
| PR-8 | **峰谷调度（deferrable 任务挪谷时）** | PR-5 + ROUTING-PLAN P4 | 可延迟批任务在 DeepSeek 高峰期进入延迟队列，谷时自动执行；实时路径零延迟变化 |

优先级理由：PR-1/PR-4 零架构风险、当周可见收益（分项账单 + 缓存命中率）；
PR-2/PR-3 是地基（其后一切依赖）；PR-5 才开始影响路由行为；PR-6/7/8 逐层自动化。

---

## 九、风险与注意事项

1. **价格波动已成常态**（DeepSeek 一周两调、缓存命中价涨 11 倍）：所有面向用户的金额展示
   一律带「按 PriceSpec @ 更新时间估算」口径说明，不承诺与账单完全一致；
2. **`cached_tokens` 字段兼容性**：老版本百炼响应可能缺 `prompt_tokens_details`，
   代码必须 `getattr` 防御；字段缺失时计价回落为裸乘法并在校准里记 `field_missing`（不告警刷屏）；
3. **显式缓存（cache_control）首期不做**：盈亏平衡点依赖复用率，等 `h` 有数据后
   再按任务类型评估（>2 次复用的长前缀任务才值得开）；
4. **汇率**：`cny_to_usd` 只用于录入换算与展示，不做每日动态汇率（成本核算误差 <1%，
   不值得引入新数据源）；
5. **探针双刃剑**：探针自身产生 usage 会污染 `model_task_score` 的 ewma_quality——
   记账时 `task_type='probe'` 必须在所有用户统计口径中排除；
6. **节假日表**：法定节假日按年更新，overlay 带版本；过期表只影响 DeepSeek 谷价判断（多收费风险），
   校准层会兜底暴露；
7. **本机网络受限**：刷新走镜像（ROUTING-PLAN 既有结论），但**对账校准不依赖外网**——
   这正是三层闭环设计的原因：断网环境下价格新鲜度靠本地流量自证。

---

## 十、与 ROUTING-PLAN.md 的关系总结

| ROUTING-PLAN 条目 | 本方案动作 |
|---|---|
| P0.a 量纲修正 | 并入 PR-2，且从"UPDATE 除以 10"升级为"录入口换算断言"防复发 |
| P0.b `cached_input_cost_per_token` 列 | 扩展为完整 PriceSpec 四分项 + 阶梯 + 时段三维度 |
| P0.d `est_cost` 门槛 | 输入升级为 §4.3 预估器（eff_in + 时段系数） |
| P1.a 用量落库 | 追加 5 列（PR-1），是本方案唯一硬前置 |
| P2.a/b 刷新与 overlay | 保留；overlay 条目升级为 §3.3 形态（含 CNY 锚点） |
| P2.c 对账 | 从"可选"升级为核心闭环（§六），用自家账单而非 OpenRouter |
| P3 健康探测 | 探针系统（§七）是其执行载体，并兼任校准燃料 |
| P4/P5 编排与预算 | 峰谷调度（PR-8）与 batch 系数预留（§5.5）挂接 |

本方案不改变 ROUTING-PLAN 的阶段骨架，只把其中"定价表"一根线抽出来做深做透。
实施顺序建议插在 ROUTING-PLAN 步骤 1（用量落库）之后、步骤 4（评分路由）之前——
**价格信号不准，评分路由就是精确地错。**
