# 模型定价表优化方案 v2

> 定位：`ROUTING-PLAN.md` 阶段 P2（定价表系统）的**细化与代码级落地版**。
> 分析对象：`v2` 分支 @ 2026-08-24；DB 快照：7 模型（6 活跃）、usage_records 仅 3 条全零脏数据。
> 外部依据：DeepSeek 峰谷计费公告（2026-08-17/08-23）、阿里云百炼 Context Cache 文档、
> LiteLLM `cost_calculator` 源码、OpenRouter Prompt Caching + Sticky Routing 博客（2026-07-21）。
> **v2 修订（2026-08-24）**：在 v1 基础上补全代码级落地细节——
> 接入点行号清单、Rust 结构体与函数签名、迁移幂等 SQL、校准 tokio job、探针状态机、
> WebUI REST 端点契约、单测矩阵。核心主张不变：
> **定价表从「两个标量」升级为「分项 × 时段 × 阶梯 × 来源」的 PriceSpec，
> 计算引擎从「in×p+out×p」升级为与 LiteLLM 同构的分项账单模型，
> 并以真实用量对账驱动持续校准。**

---

## 实施状态（2026-08-24 更新）

| 阶段 | 状态 | 说明 |
|---|---|---|
| PR-1 usage 透传 + 落库 | ✅ 已实施 | `ai_service.py` 新增 `_usage_detail` 透传；`ChatResult` 改 `usage` 结构；`chat_stream` 补 insert_usage（原无）；orchestrate `result` 事件落库移出 cache_sim 分支（原断链 bug）；语义缓存命中 act_cost 强制 0 |
| PR-2 量纲修正 + PriceSpec 迁移 | ✅ 已实施 | `db.rs` 新增 `migrate_db()`：usage 7 列 ALTER、dashscope ÷10（settings 标记幂等）、models→price_specs 投影、deepseek 峰谷规则预置；迁移测试覆盖旧库升级路径 |
| PR-3 pricing.rs 引擎 | ✅ 已实施 | 新建 `crates/lloom-core/src/pricing.rs`：PriceSpec/TierBand/ZoneRule/UsageDetail + actual_cost/est_cost/effective_input_cost/zone_multiplier + ZoneResolver + 北京时间纯标准库换算；16 个单测 |
| PR-4 prompt 稳定化 | ✅ 已实施 | 新建 `signals.rs` 的 `prefix_stability` 信号（5 单测）；`build_context` 补前缀稳定约定文档（代码结构本已符合） |
| PR-5 路由衔接（eff_in/sticky） | ⏳ **待办** | 依赖 ROUTING-PLAN 步骤 4（plan() 评分路由重构），跨计划，另行实施 |
| PR-6 校准 job + WebUI 定价页 | ✅ **已实施** | `calibration_job` 每日聚合（样本 ≥50 才计算）、对账比 act/est、命中率与 out/in 落 `price_calibration`、偏差连续 3 天越界标 `price_stale`；REST：`GET /api/pricing/specs`、`PUT /api/pricing/specs/{provider}/{model}`（含 USD/token 断言，强制转 manual）、`GET /api/pricing/calibration`。WebUI 定价页**已完成**（PricingPage：price_source 徽标/stale 黄点/改价转 manual/采纳建议价/近 30 天校准曲线） |
| PR-7 探针系统 | ✅ **已实施** | `probe.rs`：每小时一轮（固定 >512 字符稳定前缀，暖机+命中验证两条）、预算状态机（默认 ¥5/月、单轮 0.002 USD 熔断、Hourly→Daily→SuspendedCloud 降频、连续 8 轮失败暂停）、记账 `task_type='probe'`（失败 cost=-1 哨兵）；REST：`GET /api/probe/stats`、`PUT /api/probe/budget`。用量页**探针视图已完成** |
| PR-8 峰谷调度 | ⏳ 待办 | 依赖 ROUTING-PLAN P4 |
| P2.a 定价刷新（追加） | ✅ **已实施** | ROUTING-PLAN P2.a：`server.rs` `pricing_refresh_loop` 24h 后台 job（jsdelivr 主源 + ghproxy 回退，断网失败静默保留本地值）+ `POST /api/pricing/refresh`、`POST /api/pricing/specs/{provider}/{model}/accept`；`pricing.rs::parse_remote_prices` 纯函数解析 + `db::refresh_price_spec`（COALESCE 保 cache_read，不覆盖 manual） |
| P2.c 缓存节省（追加） | ✅ **已实施** | `usage_records.cache_saved_cost` 列（语义缓存命中省下的费用）+ `UsageExtra` 透传 + `get_usage_stats` SUM 聚合；用量页「缓存为您节省 ¥X」卡片 + 「缓存节省」列（CNY 展示） |

**实施偏差记录**（相对文档原设计，均为更稳妥的取舍）：
1. 投影 `price_source` 标 **`overlay`** 而非 manual：存量值来自早期 overlay 口径录入、未经人工核对，标 overlay 允许刷新覆盖与校准标 stale，比"永不覆盖"的 manual 更安全；
2. `Model` 结构体**未**内嵌 `price_spec` 字段，改为 `db::get_price_spec(provider, model)` 独立读取——避免 models/pricing 循环依赖，效果等价；
3. **未引入 chrono 依赖**：北京时间换算用纯标准库（+8 固定偏移 + Sakamoto 星期算法 + Howard Hinnant 公历转换），规避本机受限网络拉取 crates.io 失败风险；后续如需多时区再换 chrono；
4. 语义缓存命中（LLooM 自身缓存，非供应商 KV 缓存）时 `act_cost` 强制 0——未真正调用供应商不产生费用，`saved_cost` 仍保留展示"节省金额"；
5. 校准对账比用**总额**口径（act_cost/est_cost）而非输入侧分项（PR-6 实现）：usage 未存 est_input_cost 分列，总额比在 est_out=P50 估计下偏差有限，且命中率分项可交叉验证；如需精确输入侧对账，后续加两列再 ALTER（迁移框架已支持）；
6. 探针失败记账用 `cost=-1` 哨兵（usage_records 无状态列），stats 以 `cost<0` 计数；
7. 探针预算扣费用**请求后实际成本**（record_probe_usage 返回 act_cost），执行前仅按单轮上限 0.002 USD 检查。

**迁移执行时机**：`migrate_db()` 在 `init_db()` 内调用，即 `lloom-server` 下次启动时自动执行。
已备份 `data/lloom.db.pre-pricing-migration.bak`（integrity_check 通过），迁移幂等可重跑。

---

## 一、现状与问题（定价视角）

代码与数据复盘结论（与 ROUTING-PLAN §二互相印证，此处只列定价相关）：

| # | 问题 | 证据 | 后果 |
|---|---|---|---|
| 1 | **量纲错误 10 倍** | DB 六模型全部满足 `DB值 = 官方元/M × 1.3889e-06`；DashScope 系虚高 10×，gpt-4o 正确 | 跨供应商比价方向反了（gpt-4o 显得比 qwen3-max 便宜，实际贵 7×） |
| 2 | **价格只有两个标量** | `models` 表仅 `input_cost_per_token` / `output_cost_per_token`；`models.rs:54` 与 `ai_service.py:171` 两处 `calculate_cost` 均为裸乘法 | 缓存命中、缓存写入、阶梯价、峰谷、batch 折扣全部无法表达；成本永远高估 |
| 3 | **KV/prompt cache 完全未计价** | `ai_service.py:817/852/1087/1180` 只读 `prompt_tokens`/`completion_tokens`，不读 `prompt_tokens_details.cached_tokens` | 百炼隐式缓存命中本应 20% 计费、DeepSeek 命中 0.1×，账面成本虚高最多 5× |
| 4 | **无时段维度** | 无任何峰谷概念 | DeepSeek 通道夜间/周末半价的机会成本白白放弃；也无法在高峰期"避 DeepSeek 用 qwen" |
| 5 | **用量落库链路断裂** | `server.rs:327-410` `chat_stream` **无任何 `insert_usage` 调用**；仅 `server.rs:492` orchestrate 路径有且写死 `task_type=None`、tokens 偶尔非零但 cost 来自 Python 裸乘法 | 真实 `res.cost`（server.rs:398）拿到不落库；校准无燃料 |
| 6 | **定价按模型名、不按通道** | `deepseek-v3` 注册在 dashscope provider 下 | 同一模型经百炼（元、显式缓存可用）与 DeepSeek 官方（峰谷、隐式缓存）价格结构完全不同，主键必须是 (provider, model) |
| 7 | **两个成本真源各自为政** | `models.rs:54` 与 `ai_service.py:171` 各写一份裸乘法；Rust `ChatResult.cost`（ai_client.rs:43）直接信 Python 算的值 | 修一处漏一处（量纲修正就必须修两处）；Rust 对成本零主权 |

### 1.8 当前 cost 数据流（要改的就是这条链）

```
Python litellm 响应 (usage: prompt_tokens / completion_tokens / prompt_tokens_details.cached_tokens*)
     │  ai_service.py:817-825 /v1/chat 只取两个 token 数
     │  cost = _estimate_cost(spec, in, out)  ← 裸乘法，cached_tokens 丢弃
     ▼
HTTP JSON {content, input_tokens, output_tokens, cost, model}  ← cached_tokens 永远不到 Rust
     │  ai_client.rs:112  resp.json::<ChatResult>()  ← Rust 信 Python 的 cost
     ▼
server.rs:398 SSE {done, content, model, cost, input_tokens, output_tokens}
     │  chat_stream: ❌ 无 insert_usage
     │  orchestrate(server.rs:492): insert_usage(model, "default", in, out, cost, None, is_hit)
     ▼
DB usage_records（3 条全零脏数据）
```

**本方案改的就是这条链：Python 只透传 token 分项、Rust 单一计价、两条路径都落库、校准 job 回写 PriceSpec。**
打 ✱ 的字段（`cached_tokens`）litellm 响应里本来就有，是"接线"不是"开发"。

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
    cache_read_cost         REAL,                     -- 命中输入（NULL=通道无缓存计价区分）
    cache_write_cost        REAL DEFAULT 0,           -- 显式写价（隐式缓存通道=0）
    reasoning_cost          REAL,                     -- 思考 token 输出价（若有区分）
    -- 维度修饰
    tiered_json     TEXT,    -- [{"max_input":32768,"in":..,"out":..,"cache_read":..}, ...]
    zone_ref        TEXT,    -- 引用 provider_zones.provider；NULL=不分时
    batch_multiplier REAL DEFAULT 0.5,                -- batch 通道折扣
    -- 溯源与保鲜（沿用 ROUTING-PLAN P2 设计）
    price_source    TEXT DEFAULT 'unknown',
    price_updated_at TIMESTAMP,
    price_stale     INTEGER DEFAULT 0,
    stale_reason    TEXT,                             -- v2：stale 触因（'calibration_drift'|'refresh_mismatch'|'manual_hint'）
    effective_from  TEXT,    -- 供应商公告生效日，如 '2026-08-23'
    cny_list_price_json TEXT,                          -- v2：官方人民币原价锚点（校对页展示用，不参与计算）
    PRIMARY KEY (provider, model)
);

-- 新表：渠道级时段规则（峰谷规则挂在 provider 上，模型默认继承，可覆盖）
CREATE TABLE provider_zones (
    provider   TEXT NOT NULL,
    rule_json  TEXT NOT NULL,   -- 规则数组，见 §3.4
    tz         TEXT DEFAULT 'Asia/Shanghai',
    holidays_json TEXT,          -- 节假日日期数组 ["2026-10-01", ...]，年度更新
    PRIMARY KEY (provider)
);

-- 新表：实测分项统计（对账校准的存储，见 §六）
CREATE TABLE price_calibration (
    provider   TEXT, model TEXT,
    as_of      TEXT,               -- 'YYYY-MM-DD'，按天聚合
    calls      INTEGER,
    est_cost   REAL,               -- 路由前估算之和
    act_cost   REAL,               -- 分项公式按真实 usage 计算之和
    input_side_ratio  REAL,        -- 输入侧 act/est（剔除 est_out 误差，用于 stale 判定）
    cache_hit_rate  REAL,          -- cached_tokens / prompt_tokens（EWMA 前的原始值）
    out_in_ratio    REAL,          -- output_tokens / prompt_tokens
    field_missing_count INTEGER DEFAULT 0,  -- usage 缺 cached_tokens 字段的次数
    PRIMARY KEY (provider, model, as_of)
);
```

`models` 表中原有两个 cost 列降级为**展示聚合列**（由 PriceSpec 投影生成，迁移脚本 §九回填），
路由与计费代码一律读 `price_specs`。双真源问题（问题 7）就此消除：
Rust 与 Python 共享同一套 PriceSpec JSON（Rust 决策、Python 执行时带价）。

### 3.3 overlay（`model_catalog.json`）条目形态

百炼模型仍靠 overlay 维护（自动源无 DashScope 价格，ROUTING-PLAN §3.4 已实测）。示例：

```json
{
  "dashscope/qwen3-max": {
    "input_cost": 3.47e-7, "output_cost": 1.389e-6,
    "cache_read_cost": 6.94e-8, "cache_write_cost": 0,
    "tiered_json": [
      {"max_input": 32768,  "in": 3.47e-7, "out": 1.389e-6, "cache_read": 6.94e-8},
      {"max_input": 131072, "in": 5.56e-7, "out": 2.222e-6, "cache_read": 1.11e-7},
      {"max_input": 262144, "in": 9.72e-7, "out": 3.889e-6, "cache_read": 1.94e-7}
    ],
    "explicit_cache": {"read": 0.1, "write": 1.25, "min_tokens": 1024, "ttl_min": 5},
    "zone_ref": null,
    "cny_list_price_json": {"in": 2.5, "out": 10.0, "per": "1M", "currency": "CNY"},
    "source": "overlay", "effective_from": "2026-08-01"
  },
  "deepseek-official/deepseek-v4-pro": {
    "input_cost": 1.25e-6, "output_cost": 3.75e-6, "cache_read_cost": 4.17e-8,
    "zone_ref": "deepseek",
    "cny_list_price_json": {"peak": {"in": 9, "out": 27, "cache_read": 0.3}, "per": "1M", "currency": "CNY"},
    "source": "overlay", "effective_from": "2026-08-23"
  }
}
```

`cny_list_price_json` 是**人工核对锚点**：WebUI 校对页直接显示官方人民币原价，
保存时按当前汇率换算成 USD/token，量纲错误从此在录入口被挡住（而不是事后反推）。

### 3.4 峰谷规则形态（zone_json）

DeepSeek 2026-08-23 规则的表达（规则数组**先具体后兜底**，首条命中生效）：

```json
[
  {"days": ["sat", "sun"],                     "hours": "*",        "multiplier": 0.5},
  {"days": ["mon","tue","wed","thu","fri"],    "hours": "9-12,14-18", "multiplier": 1.0},
  {"days": ["mon","tue","wed","thu","fri"],    "hours": "*",        "multiplier": 0.5},
  {"holidays": true,                            "hours": "*",        "multiplier": 0.5}
]
```

语义要点：
- `multiplier` 作用于该通道**全部分项价**（含 cache_read——DeepSeek 实例中命中价谷时 0.15 = 峰时 0.3 × 0.5，验证成立）；
- `holidays: true` 命中条件 = 请求日期在 `provider_zones.holidays_json` 数组中；
- 时区：规则按 `provider_zones.tz`（默认 `Asia/Shanghai`）解释；落库时刻存 UTC，计算时转目标时区（避免本机时区漂移）；
- qwen 系当前无峰谷 → `zone_ref IS NULL`，multiplier 恒 1（**设计必须前向兼容：今天只有 DeepSeek 分时，明天可能人人分时**）。

---

## 四、成本计算引擎

### 4.1 PriceSpec Rust 结构体（`lloom-core/src/pricing.rs`）

```rust
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc, TimeZone};
use chrono_tz::Asia::Shanghai;

/// 单档阶梯价（max_input 含义：输入长度 ≤ 该值时适用本档；末档 max_input = i64::MAX）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TierBand {
    pub max_input: i64,
    pub input_cost: f64,
    pub output_cost: f64,
    pub cache_read_cost: Option<f64>,   // None = 该档无缓存计价区分（并入 input_cost）
    pub cache_write_cost: Option<f64>,
    pub reasoning_cost: Option<f64>,
}

/// 时段规则一条
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneRule {
    pub days: Option<Vec<String>>,      // ["mon","tue",...]; None=不限（holidays 规则用）
    pub hours: String,                   // "*" 或 "9-12,14-18"
    pub multiplier: f64,
    pub holidays: bool,                  // true=仅节假日命中
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceSpec {
    pub provider: String,
    pub model: String,
    pub input_cost: f64,
    pub output_cost: f64,
    pub cache_read_cost: Option<f64>,
    pub cache_write_cost: Option<f64>,
    pub reasoning_cost: Option<f64>,
    pub tiered: Option<Vec<TierBand>>,
    pub zone_ref: Option<String>,
    pub batch_multiplier: f64,           // 默认 0.5
    pub price_source: String,
    pub price_stale: bool,
    pub effective_from: Option<String>,
}

/// 真实用量（Python 透传，§4.2）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageDetail {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cached_tokens: i64,              // prompt_tokens_details.cached_tokens
    pub reasoning_tokens: i64,           // completion_tokens_details.reasoning_tokens
    pub cache_creation_tokens: i64,      // cache_creation_input_tokens
    pub field_missing: bool,             // usage 缺 cached_tokens 字段（校准记账）
}
```

### 4.2 成本计算函数（事后精确计算，对齐 LiteLLM）

```rust
impl PriceSpec {
    /// 阶梯档选择：按当次请求总输入长度（含缓存命中部分，供应商如此计价）
    fn tier_select(&self, prompt_tokens: i64) -> &TierBand {
        if let Some(tiers) = &self.tiered {
            for band in tiers {
                if prompt_tokens <= band.max_input { return band; }
            }
            return tiers.last().unwrap();
        }
        // 无阶梯 → 构造一个虚拟末档引用 self（简化：返回 owned；实际用 enum 或引用包装）
        // 实现细节：tiered 为 None 时直接用顶层字段，下文 pick() 统一取值
        &TierBand { max_input: i64::MAX,
            input_cost: self.input_cost, output_cost: self.output_cost,
            cache_read_cost: self.cache_read_cost,
            cache_write_cost: self.cache_write_cost,
            reasoning_cost: self.reasoning_cost }
    }

    fn pick_in(&self, band: &TierBand)  -> f64 { band.input_cost }
    fn pick_out(&self, band: &TierBand) -> f64 { band.output_cost }
    fn pick_cread(&self, band: &TierBand) -> f64 {
        band.cache_read_cost.unwrap_or(self.input_cost)  // 无区分→命中按原价（不优惠）
    }
    fn pick_cwrite(&self, band: &TierBand) -> f64 {
        band.cache_write_cost.unwrap_or(0.0)
    }
    fn pick_reason(&self, band: &TierBand) -> f64 {
        band.reasoning_cost.unwrap_or(self.output_cost)  // 无区分→思考按普通输出价
    }

    /// 时段系数。zone_resolver 从 provider_zones 表加载并缓存。
    pub fn zone_multiplier(&self, t: DateTime<Utc>, zone_resolver: &ZoneResolver) -> f64 {
        let Some(zref) = &self.zone_ref else { return 1.0; };
        let Some(zone) = zone_resolver.get(zref) else { return 1.0; }; // 规则缺失=不优惠
        let local = t.with_timezone(&Shanghai);
        let dow = local.format("%a").to_string().to_lowercase(); // mon..sun
        let hh = local.format("%H").to_string().parse::<u32>().unwrap_or(0);
        let is_holiday = zone.is_holiday(local.date_naive());
        for rule in &zone.rules {
            let day_ok = rule.days.as_ref().map(|d| d.contains(&dow)).unwrap_or(true);
            let hol_ok = if rule.holidays { is_holiday } else { day_ok };
            if !hol_ok { continue; }
            if hours_match(&rule.hours, hh) { return rule.multiplier; }
        }
        1.0 // 无规则命中（不应发生，规则数组应有兜底）
    }

    /// 事后精确账单（对齐 LiteLLM cost_calculator）
    pub fn actual_cost(&self, u: &UsageDetail, t: DateTime<Utc>, zr: &ZoneResolver) -> f64 {
        let band = self.tier_select(u.prompt_tokens);
        let z = self.zone_multiplier(t, zr);
        // 容错：cached_tokens 不应超过 prompt_tokens（供应商偶发数据异常）
        let cached = u.cached_tokens.min(u.prompt_tokens).max(0);
        let non_cached = (u.prompt_tokens - cached).max(0);
        z * ( non_cached       * self.pick_in(band)
            + cached           * self.pick_cread(band)
            + u.cache_creation_tokens * self.pick_cwrite(band))
          + z * u.completion_tokens * self.pick_out(band)
          + z * u.reasoning_tokens  * self.pick_reason(band)
    }
}

fn hours_match(spec: &str, hh: u32) -> bool {
    if spec == "*" { return true; }
    for part in spec.split(',') {
        let mut iter = part.split('-');
        let lo: u32 = iter.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
        let hi: u32 = iter.next().and_then(|s| s.trim().parse().ok()).unwrap_or(lo);
        if hh >= lo && hh <= hi { return true; }
    }
    false
}
```

边界与容错（已在代码体现，单测必覆盖）：
- `cached_tokens > prompt_tokens` → 截断到 prompt_tokens（供应商偶发字段错误）；
- `cache_read_cost = None` → 命中按原价（保守，不假设优惠）；
- `zone_ref` 指向不存在的规则 → 回落 1.0（规则缺失不报错，校准层会暴露）；
- `tiered` 末档 `max_input` 必须是 `i64::MAX`（迁移脚本断言）。

### 4.3 usage 透传协议（Python 改法，精确到函数）

Python 侧删除 `_estimate_cost`（ai_service.py:171）与所有调用点的 `cost` 字段，
改为透传完整 usage 详情。改 4 处返回点（非流式 `/v1/chat`、流式 `/v1/chat/stream`、
orchestrate 的 `_run_task` 1086/1180、子任务 cost）：

```python
# ai_service.py:171 替换为
def _usage_detail(usage) -> dict:
    """Extract full usage breakdown from a litellm response. Never raises."""
    if usage is None:
        return {"prompt_tokens": 0, "completion_tokens": 0, "cached_tokens": 0,
                "reasoning_tokens": 0, "cache_creation_tokens": 0, "field_missing": True}
    def _g(obj, *path, default=0):
        cur = obj
        for p in path:
            cur = getattr(cur, p, None)
            if cur is None: return default
        return cur or default
    cached = _g(usage, "prompt_tokens_details", "cached_tokens")
    return {
        "prompt_tokens": _g(usage, "prompt_tokens"),
        "completion_tokens": _g(usage, "completion_tokens"),
        "cached_tokens": cached,
        "reasoning_tokens": _g(usage, "completion_tokens_details", "reasoning_tokens"),
        "cache_creation_tokens": _g(usage, "cache_creation_input_tokens"),
        "field_missing": cached is None,   # None=供应商没返回该字段
    }
```

返回点改法（以 `/v1/chat` 为例，ai_service.py:819-825）：

```python
u = _usage_detail(getattr(response, "usage", None))
return {
    "content": content,
    "usage": u,                  # ← 透传分项，Rust 计价
    "model": req.model.name,
    # cost 字段删除（Rust 算）；保留 saved_cost 仅用于语义缓存命中展示
}
```

Rust `ChatResult`（ai_client.rs:39-45）改：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResult {
    pub content: String,
    pub usage: UsageDetail,    // ← 替换 input_tokens/output_tokens/cost
    pub model: String,
}
```

`server.rs:398` SSE 头改：

```rust
"done": true,
"content": res.content,
"model": res.model,
"cost": act_cost,                      // ← Rust 算好回填
"input_tokens": res.usage.prompt_tokens,
"output_tokens": res.usage.completion_tokens,
"cached_tokens": res.usage.cached_tokens,   // 前端展示"缓存为您节省 ¥X"
```

### 4.4 成本预估器（事前估算，路由门槛用）

```rust
/// 有效输入单价（路由评分用）：命中率期望加权
pub fn effective_input_cost(&self, hit_rate_ewma: f64, t: DateTime<Utc>, zr: &ZoneResolver) -> f64 {
    let z = self.zone_multiplier(t, zr);
    let p_read = self.cache_read_cost.unwrap_or(self.input_cost); // 无区分=不优惠
    z * (hit_rate_ewma * p_read + (1.0 - hit_rate_ewma) * self.input_cost)
}

/// 事前估算成本（喂 plan() 门槛）
pub fn est_cost(&self, hit_rate_ewma: f64, est_in: i64, est_out: i64,
                t: DateTime<Utc>, zr: &ZoneResolver) -> f64 {
    let z = self.zone_multiplier(t, zr);
    let eff_in = self.effective_input_cost(hit_rate_ewma, t, zr);
    // 输出用顶层价（预估阶段不知命中长度，不查阶梯；或用末档，保守）
    z * (est_in as f64 * eff_in + est_out as f64 * self.output_cost)
}
```

- `hit_rate_ewma`：该模型×任务的历史缓存命中率（EWMA，冷启动 0）——来自 `price_calibration`，§六更新；
- `est_in`：tiktoken 精确算（`tiktoken_cache/` 已有）；
- `est_out`：该 task_type 历史 P50（`price_calibration.out_in_ratio` × est_in 兜底）；
- 预估**不含 cache_write**（保守估计不指望显式缓存收益；显式缓存开启时另行加项）。

这个 `est_cost` 就是 ROUTING-PLAN P0.d `plan()` 硬门槛 `est_cost ≤ max_cost_per_request` 与
P5 预算档位判断的输入。**关键改进：比价用 `eff_in` 而非标称 `input_cost`**——
一个命中率 60% 的 qwen3-max，有效输入价 = 0.6×0.1×P + 0.4×P = 0.46×P，
在多轮会话任务上可能反超标称更便宜的模型。

---

## 五、与路由策略的衔接

定价表不是报表，是路由的**成本信号源**。五个衔接点：

### 5.1 有效单价参与软评分（替换标称价）

ROUTING-PLAN P0.d `plan()` 的软评分升级为：

```rust
// router.rs 新 plan()（替换 select_model / task_model_preference / TASK_MODEL_MAP）
pub struct Candidate { pub model: Model, pub spec: PriceSpec, pub score: f64, pub est_cost: f64, pub reason: String }

pub fn plan(task: &str, policy: &RoutingPolicy, models: &[Model], specs: &PriceSpecMap,
            hit_rates: &HitRateMap, est_in: i64, est_out: i64, t: DateTime<Utc>,
            zr: &ZoneResolver) -> Vec<Candidate> {
    let mut out = vec![];
    for m in models.iter().filter(|m| m.is_active == 1) {
        let Some(spec) = specs.get(&m.provider, &m.name) else { continue; };
        // 硬门槛（gate）
        if !gate_ok(m, spec, task, policy, est_in) { continue; }
        let h = hit_rates.get(&m.provider, &m.name, task);
        let ec = spec.est_cost(h, est_in, est_out, t, zr);
        if ec > policy.max_cost_per_request.unwrap_or(f64::INFINITY) { continue; }
        // 软评分：q_w·quality − c_w·norm(ec) − l_w·norm(latency) + sticky_bonus + priority·0.05
        let norm_cost = ec / (ec + median_cost(specs));   // 软归一，避免量纲压制
        let score = policy.quality_weight * quality(m, task)
                  - policy.cost_weight * norm_cost
                  - policy.latency_weight * norm_latency(m)
                  + sticky_bonus(m, &task)               // §5.2
                  + 0.05 * m.priority as f64;
        out.push(Candidate { model: m.clone(), spec: spec.clone(), score, est_cost: ec, reason: ... });
    }
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    out.into_iter().take(policy.fallback_depth as usize + 1).collect()
}
```

**关键**：`ec` 用 `est_cost`（含时段系数 + 命中率期望 + 阶梯），`norm_cost` 软归一避免量纲压制。
效果：多轮/长前缀任务自动偏向缓存友好模型；高峰时段自动偏向无峰谷溢价或谷价通道。

### 5.2 会话亲和（Sticky Routing 本地化，新增信号）

```rust
fn sticky_bonus(m: &Model, conv_ctx: &ConvCtx) -> f64 {
    // 仅缓存敏感通道才粘；同会话上一轮用过该模型才加分
    if m.cache_read_cost.is_none() { return 0.0; }          // 无缓存计价=不粘
    let Some(last_model) = conv_ctx.last_model_in_conv() else { return 0.0; };
    if last_model != m.name { return 0.0; }
    0.05   // 加分项非约束；门槛淘汰的候选不计入
}
```

- 同 `conversation_id` 的后续请求，在候选评分中给"上一轮所用模型"加分（+0.05）；
- 若上一轮模型已被门槛淘汰（健康/预算），正常切换不硬粘——**粘性是加分项不是约束**；
- `ConvCtx` 由 `server.rs` 从 `req.conversation_id` + 最近一次 `routing_decisions` 查得；
- 对应 OpenRouter 实测教训：会话轮 2 换端点 = 热缓存全废，长会话成本差可达 2–3×。

### 5.3 Prompt 结构规范（零成本、纯收益，立即可做）

百炼命中逻辑是**前缀匹配**（"已缓存 ABCD，请求 ABE 可命中，BCD 不可"）。落成项目约定：

1. 系统提示词、工具定义、检索文档、few-shot 样例等**稳定内容一律置前**，用户消息与对话尾部置后；
2. 同一会话内前缀保持**字节级稳定**（禁止每轮重排工具列表、在 system prompt 里插时间戳/随机数）；
   需要时间信息时放最后一条 user message；
3. `signals.rs`（ROUTING-PLAN P0.g 新建模块）加启发式信号 `prefix_stability`：

```rust
// signals.rs：相邻请求前缀漂移检测
pub fn prefix_stability(curr_messages: &[Value], prev_prefix_hash: Option<u64>) -> (f64, Option<u64>) {
    // 取前 N=512 token 的归一化字符串，算 fnv1a 哈希
    let prefix = normalize_prefix(curr_messages, 512);
    let curr_hash = fnv1a(&prefix);
    let drift = match prev_prefix_hash {
        Some(h) if h == curr_hash => 0.0,            // 完全稳定
        Some(_) => 1.0,                              // 漂移
        None => 0.0,                                 // 首轮无参照
    };
    (drift, Some(curr_hash))
}
```

`drift > 0` 的会话在 WebUI 提示"缓存命中率受损"。这一条不写代码也能让现有百炼隐式缓存
命中率显著提升（20% 计价直接生效），是**全方案回收最快的一项**。

### 5.4 峰谷感知调度（可延迟任务挪谷时）

- 路由增加"可延迟"标记位：编排批处理、夜间评测、影子评测（P1.d）、**探针（§七）**默认 `deferrable=1`；
- 实时 `chat/stream` 永不延迟；
- 对 `deferrable` 任务：若当前处于某候选通道的高峰时段且该通道存在分时，
  `est_cost` 用"预计执行时刻的价"（简单实现：若 2 小时内进入谷时则按谷价估算并给该候选加分，
  任务进延迟队列由 P4 编排器在谷时窗口执行）；
- 首期简化版可只做**提示不做调度**：WebUI 用量页展示"若以下任务挪至谷时可省 ¥X/月"。

### 5.5 batch 通道（远期）

编排路径中无实时要求的子任务理论上可走百炼 Batch（5 折，但无缓存折扣、非实时）。
首期只把 `batch_multiplier` 留在 schema 里，不实现通道——先让单次调用成本算准。

---

## 六、动态校准闭环：用真实消耗修定价表

三层闭环，逐层收紧。核心：**对账不依赖外网**，断网环境下价格新鲜度靠本地流量自证。

### 6.1 第一层：单次对账（每次请求，自动）

每次请求落库两条成本（依赖 ROUTING-PLAN P1.a 用量落库，本方案在其 schema 上追加列）：

```sql
-- usage_records 追加列（迁移见 §九）
ALTER TABLE usage_records ADD COLUMN cached_tokens INTEGER DEFAULT 0;
ALTER TABLE usage_records ADD COLUMN reasoning_tokens INTEGER DEFAULT 0;
ALTER TABLE usage_records ADD COLUMN est_cost REAL DEFAULT 0;
ALTER TABLE usage_records ADD COLUMN act_cost REAL DEFAULT 0;
ALTER TABLE usage_records ADD COLUMN zone_multiplier REAL DEFAULT 1.0;
ALTER TABLE usage_records ADD COLUMN conversation_id TEXT;
ALTER TABLE usage_records ADD COLUMN field_missing INTEGER DEFAULT 0;
```

`server.rs` 落库逻辑（chat_stream 与 orchestrate 两处）：

```rust
// 计算两条成本
let act_cost = spec.actual_cost(&res.usage, req_timestamp, &zone_resolver);
// est_cost 在路由时已算（plan() 输出），从 routing_decisions 或 Candidate 带过来
let _ = db::insert_usage(&res.model, &user_id,
    res.usage.prompt_tokens, res.usage.completion_tokens, act_cost,
    Some(&task_type), is_cache_hit,
    Some(res.usage.cached_tokens), Some(res.usage.reasoning_tokens),
    Some(est_cost), Some(zone_mult), req.conversation_id.as_deref(),
    res.usage.field_missing);
```

`insert_usage` 签名扩展（db.rs:211）——新增可选参数走 Option 避免破坏现有调用点。

### 6.2 第二层：日级校准因子（tokio 后台 job，自动）

```rust
// lloom-server 启动时 spawn（与现有 health 检查 job 同模式）
async fn calibration_job(state: Arc<AppState>) {
    let mut ticker = tokio::time::interval(Duration::from_secs(86_400)); // 每天
    ticker.tick().await; // 跳过立即触发
    loop {
        ticker.tick().await;
        if let Err(e) = run_daily_calibration(&state).await {
            tracing::warn!("calibration job failed: {e}");  // 失败不告警刷屏
        }
    }
}

async fn run_daily_calibration(state: &AppState) -> Result<()> {
    let yesterday = Utc::now().date_naive() - Duration::days(1);
    // SQL 聚合（一条 query）
    let rows = db::aggregate_usage_by_model_day(yesterday)?;
    for r in &rows {
        if r.calls < 50 { continue; }   // 样本不足跳过
        let input_side_ratio = if r.est_input_cost > 0.0 { r.act_input_cost / r.est_input_cost } else { 1.0 };
        let cache_hit_rate = if r.prompt_tokens > 0 { r.cached_tokens as f64 / r.prompt_tokens as f64 } else { 0.0 };
        let out_in_ratio = if r.prompt_tokens > 0 { r.completion_tokens as f64 / r.prompt_tokens as f64 } else { 0.0 };
        db::upsert_price_calibration(r.provider, r.model, yesterday, r.calls,
            r.est_cost, r.act_cost, input_side_ratio, cache_hit_rate, out_in_ratio, r.field_missing)?;

        // 喂回命中率 EWMA（hit_rate map，plan() 读）
        state.hit_rates.update(r.provider, r.model, cache_hit_rate, 0.15); // α=0.15

        // stale 判定（去抖：连续 3 天 |input_side_ratio−1|>0.2 才标）
        if r.input_side_ratio > 1.2 || r.input_side_ratio < 0.8 {
            let streak = db::stale_streak(r.provider, r.model, 3)?; // 最近 3 天
            if streak >= 3 {
                db::mark_price_stale(r.provider, r.model, true, "calibration_drift")?;
            }
        }
    }
    Ok(())
}
```

EWMA 公式：`h_new = α·h_observed + (1−α)·h_old`，α=0.15（约 10 天半衰，对周级价格波动够灵敏又不抖）。

两个用途：
1. **喂回预估器**：`h` 与 `out_in_ratio` 更新后，下一天的 est_cost 更准（自收敛）；
2. **标记价格过期**：`input_side_ratio` 持续 >1.2 → 表价低于实际（供应商涨价）；<0.8 → 表价高于实际。
   `price_source='manual'` 的只置 `price_stale=1` 提示，不自动改价。

反向（表价高于实际，ratio < 0.8）同样提示——可能是供应商降价或切换了计费结构。

### 6.3 第三层：月度人工锚定（低频，WebUI 校对页）

- 「模型与定价」页：每模型显示官方人民币原价（`cny_list_price_json` 锚点）、当前换算 USD、
  `price_source` 徽标、`price_updated_at`、`price_stale` + `stale_reason`、**近 30 天对账偏差 `input_side_ratio`**；
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

### 7.3 探针预算状态机（防止"监控比业务还贵"）

```rust
// lloom-server 内，与 calibration_job 同模式 spawn
struct ProbeBudget {
    monthly_limit_usd: AtomicU64,      // 默认 0.0007 美元（≈¥5，WebUI 可改；0=关）
    spent_this_month: AtomicU64,        // 微美分计数避免 f64
    per_model_freq: RwLock<HashMap<(String,String), Freq>>, // (provider,model)→每小时/每天
    month_cursor: MonthBucket,         // 跨月重置
}

impl ProbeBudget {
    fn try_charge(&self, provider: &str, model: &str, est_cost_usd: f64) -> bool {
        // 1. 单轮成本上限：异常放大即熔断该模型
        let cap = 0.002_f64.max(10.0 * self.p95_cost(provider, model));
        if est_cost_usd > cap { return false; }
        // 2. 月预算：超限自动降频
        let mut freq = self.per_model_freq.write().unwrap();
        let f = freq.entry((provider.into(), model.into())).or_insert(Freq::Hourly);
        if self.spent_this_month.load(Acquire) as f64 / 1e6 + est_cost_usd
           > self.monthly_limit_usd.load(Acquire) as f64 / 1e6 {
            match f {
                Freq::Hourly => { *f = Freq::Daily; return true; }   // 降频仍执行本轮
                Freq::Daily => { *f = Freq::SuspendedCloud; return true; } // 再降，仅本轮
                Freq::SuspendedCloud => return false,               // 暂停云端探针
            }
        }
        self.spent_this_month.fetch_add((est_cost_usd * 1e6) as u64, Relaxed);
        true
    }
}
```

预算硬顶规则：
- 月探针预算上限 B_probe（默认 ¥5，WebUI 可改，0=关）
- 单轮成本上限 = max(0.002 USD, 10×该模型历史单轮 P95)——异常放大即熔断该模型探针
- 预算耗尽 → 自动降频（每小时→每天一次），仍超 → 只探本地与免费通道
- WebUI 用量页固定展示：「探针本月消耗 ¥X / ¥Y（N 次探测，发现 M 次异常）」

按当前价目表测算：5 个云端模型 × 24 轮/天 × 2 条 × ~400 tok
（其中一半 20% 计价）≈ **每月 ¥1–3**，在默认预算内且能覆盖全部校准需求。

### 7.4 探针注入点与降级

探针复用 `ai_client::chat`（同一条调用链，不走 chat_stream），`task_type="probe"` 走 plan() 时
跳过用户预算门槛（探针有独立预算）。探针请求的 `conversation_id='probe:{provider}:{model}'`
天然形成前缀稳定的会话，第 2 条即可验证命中。

失败处理：连续 3 轮 TTFB 超时或错误 → `health_state=degraded`；连续 8 轮 → `down` + 指数退避探测
（喂 ROUTING-PLAN P3 状态机）。探针失败**不触发 fallback 链**（探针本就是探测，不是用户任务）。

---

## 八、接入点清单（文件:行号 → 改法）

精确到当前代码位置的修改清单（行号基于 2026-08-24 快照，后续迁移以函数名为准）：

| 文件:位置 | 当前 | 改法 | 阶段 |
|---|---|---|---|
| `ai_service.py:171` | `_estimate_cost` 裸乘法 | 删除，替换为 `_usage_detail` 透传函数（§4.3） | PR-1 |
| `ai_service.py:817-825` | `/v1/chat` 返回 cost | 返回 `usage` 分项，删 cost 字段 | PR-1 |
| `ai_service.py:852-858` | 流式 `/v1/chat/stream` 返回 | 同上 | PR-1 |
| `ai_service.py:1087/1180` | orchestrate `_run_task` 取 token | 同上透传 | PR-1 |
| `ai_service.py:1070/1153` | 语义缓存命中 `saved_cost` | 保留（语义缓存层面，与供应商 KV 缓存分账） | 不变 |
| `ai_client.rs:39-45` | `ChatResult{content,input_tokens,output_tokens,cost,model}` | 改 `usage: UsageDetail`，删 input/output/cost | PR-1 |
| `ai_client.rs:112` | `resp.json::<ChatResult>()` | 自动跟随结构变化 | PR-1 |
| `server.rs:327-410` `chat_stream` | **无 insert_usage** | 加 actual_cost 计算 + insert_usage（§6.1） | PR-1 |
| `server.rs:398` SSE | 透传 res.cost | 改为 Rust 算的 act_cost + cached_tokens 展示 | PR-1 |
| `server.rs:492` orchestrate | insert_usage(model,"default",…,None,…) | task_type 改为真实子任务类型；补 act_cost/est_cost | PR-1 |
| `models.rs:11-31` `Model` | 含两个 cost 字段 | 保留作展示聚合列（由 PriceSpec 投影），加 `price_spec: PriceSpec` 字段 | PR-2 |
| `models.rs:54` `calculate_cost` | 裸乘法 | 删除（路由/计费改读 PriceSpec.actual_cost） | PR-3 |
| `models.rs:43-52` `to_ai_spec` | 带 cost 给 Python | 带 PriceSpec JSON 给 Python（Python 不再计价，仅执行） | PR-2 |
| `db.rs:17-18` | models 表两个 cost 列 | 保留（投影列），新增 price_specs/provider_zones/price_calibration 建表（§3.2） | PR-2 |
| `db.rs:211-235` `insert_usage` | 7 参数 | 扩展可选参数（cached/reasoning/est_cost/act_cost/zone_mult/conv_id/field_missing） | PR-1 |
| `router.rs:14-22` | `TASK_MODEL_MAP`/`INFERENCE_MODELS` 硬编码 | 删除，换 `plan()` 评分（§5.1） | ROUTING-PLAN P0.d / PR-5 |
| `router.rs:119-139` | `select_model`/`task_model_preference` 死代码 | 删除 | PR-5 |
| `router.rs:178-196` `route` | 返回 RoutingDecision | plan() 取代，route 保留作"直接指定模型"入口 | PR-5 |
| `server.rs:778-779` | 路由注册 | 新增 `/api/pricing/specs` 等（§十） | PR-6 |
| 新文件 `lloom-core/src/pricing.rs` | — | PriceSpec + actual_cost + est_cost + ZoneResolver | PR-3 |
| 新文件 `lloom-core/src/probe.rs` | — | ProbeBudget + probe_loop | PR-7 |

---

## 九、迁移与启动脚本（幂等）

```sql
-- migrate_pricing.sql —— 幂等，可重跑；迁移前备份 data/lloom.db

-- 1. 量纲修正（DashScope 系除以 10）
UPDATE models SET input_cost_per_token  = input_cost_per_token  / 10.0,
                  output_cost_per_token = output_cost_per_token / 10.0
WHERE provider = 'dashscope';

-- 2. usage_records 追加列（PRAGMA 检查避免重复 ALTER）
-- Rust 侧 db.rs init 里加 PRAGMA table_info 判断后 ALTER
ALTER TABLE usage_records ADD COLUMN cached_tokens INTEGER DEFAULT 0;
ALTER TABLE usage_records ADD COLUMN reasoning_tokens INTEGER DEFAULT 0;
ALTER TABLE usage_records ADD COLUMN est_cost REAL DEFAULT 0;
ALTER TABLE usage_records ADD COLUMN act_cost REAL DEFAULT 0;
ALTER TABLE usage_records ADD COLUMN zone_multiplier REAL DEFAULT 1.0;
ALTER TABLE usage_records ADD COLUMN conversation_id TEXT;
ALTER TABLE usage_records ADD COLUMN field_missing INTEGER DEFAULT 0;

-- 3. 新表（CREATE IF NOT EXISTS）
CREATE TABLE IF NOT EXISTS price_specs ( ... §3.2 ... );
CREATE TABLE IF NOT EXISTS provider_zones ( ... §3.2 ... );
CREATE TABLE IF NOT EXISTS price_calibration ( ... §3.2 ... );

-- 4. 数据搬迁：从 models 投影到 price_specs（只搬有价的）
INSERT OR IGNORE INTO price_specs
  (provider, model, input_cost, output_cost, cache_read_cost, cache_write_cost,
   reasoning_cost, tiered_json, zone_ref, batch_multiplier, price_source, price_updated_at)
SELECT provider, name, input_cost_per_token, output_cost_per_token,
  NULL, 0, NULL, NULL, NULL, 0.5,
  'manual',   -- 现有值视为 manual（已核对），后续校准会标 stale
  CURRENT_TIMESTAMP
FROM models WHERE input_cost_per_token > 0 OR output_cost_per_token > 0;

-- 5. 写入断言（Rust 写入路径加，非 SQL）：
--    input_cost ∈ [1e-9, 1e-3] 否则拒绝；tiered 末档 max_input == i64::MAX

-- 6. 节假日表初始化（DeepSeek 通道，2026 年）
INSERT OR REPLACE INTO provider_zones (provider, rule_json, tz, holidays_json)
VALUES ('deepseek', '[...§3.4 规则...]', 'Asia/Shanghai',
  '["2026-01-01","2026-02-16","2026-02-17",...,"2026-10-07"]');
-- 注：百炼/qwen 系不写 zone 条目，zone_ref=NULL，multiplier 恒 1
```

Rust 侧 `db.rs::init()` 加迁移调用：先 `PRAGMA table_info` 检查列是否存在再 ALTER，
避免重跑报错（与 ROUTING-PLAN §七-9 幂等性要求一致）。

---

## 十、WebUI REST 端点契约

| 方法 路径 | 用途 | 请求/响应要点 |
|---|---|---|
| `GET /api/pricing/specs` | 列出所有 PriceSpec | 返回 `[{provider,model,input,output,cache_read,zone_ref,price_source,price_stale,stale_reason,effective_from,cny_list_price}]`；支持 `?stale=true` 过滤 |
| `GET /api/pricing/specs/:provider/:model` | 单条详情 | 含 tiered_json、calibration 近 30 天 |
| `PUT /api/pricing/specs/:provider/:model` | 手工改价 | body 含分项价 + cny_list_price；强制 `price_source='manual'`、`price_stale=0`、记 `effective_from` |
| `POST /api/pricing/refresh` | 触发刷新 job | 拉取 litellm 远端/overlay，返回新增/更新条数；不覆盖 manual |
| `POST /api/pricing/specs/:provider/:model/accept` | 采纳刷新价 | 把刷新价转正为 manual |
| `GET /api/pricing/calibration` | 校准视图 | `?days=30`；返回每 (provider,model) 的 input_side_ratio 曲线、cache_hit_rate、calls |
| `GET /api/probe/stats` | 探针视图 | 月消耗/预算/各模型 freq/异常数 |
| `PUT /api/probe/budget` | 改探针月预算 | body `{monthly_limit_cny}` |

前端用量页增「缓存为您节省 ¥X」卡片（基于 `saved_cost` + 命中分项差价），与"探针本月消耗"分开展示。

---

## 十一、单测矩阵（pricing.rs，零依赖纯函数优先）

| 用例 | 输入 | 期望 |
|---|---|---|
| 裸乘法等价（无缓存区分） | spec.cache_read_cost=None, cached=1000 | cached 按 input_cost 计（不优惠） |
| 隐式缓存命中 | cache_read=0.2×input, cached=5000, prompt=10000 | 输入成本 = 5000×in + 5000×0.2×in |
| 显式缓存写入 | cache_write=1.25×input, cache_creation=1000 | 写入项 = 1000×1.25×in |
| cached > prompt 容错 | cached=15000, prompt=10000 | 截断 cached=10000 |
| 阶梯档切换 | tiered=[{max:32k…},{max:131k…}], prompt=40000 | 命中第 2 档价 |
| 阶梯边界 | prompt=32768 vs 32769 | 分别命中第 1/2 档 |
| 峰谷-工作日高峰 | t=北京 10:00 周一, deepseek zone | multiplier=1.0 |
| 峰谷-工作日谷时 | t=北京 23:00 周一 | multiplier=0.5 |
| 峰谷-周末 | t=北京 10:00 周六 | multiplier=0.5 |
| 峰谷-节假日 | holidays 含当日, t=北京 10:00 周一 | multiplier=0.5（holidays 规则优先） |
| zone 缺失 | zone_ref 指向不存在 provider_zones | multiplier=1.0 |
| est_cost 含时段 | eff_in 在谷时折半 | est_cost 随之折半 |
| est_cost 命中率加权 | h=0.6, cache_read=0.1×in | eff_in = 0.46×in |
| field_missing | usage.cached_tokens=None | field_missing=true, 计价回落裸乘法 |
| actual_cost vs LiteLLM | 同 usage 喂 litellm cost_per_token | 偏差 <1%（对齐性验证） |

`prefix_stability` 与 `hours_match` 同样纯函数优先测。

---

## 十二、落地顺序（对齐 ROUTING-PLAN，编号沿用）

| 顺序 | 内容 | 依赖 | 验收标准 |
|---|---|---|---|
| PR-1 | **usage 字段补全 + 透传 + 落库**（§4.3 + §6.1 + §八接入点表） | ROUTING-PLAN 步骤 1（P1.a）先行或同做 | 一次百炼对话后 usage_records 出现非零 `cached_tokens`、`act_cost`；chat_stream 路径有 insert_usage |
| PR-2 | **量纲修正 + PriceSpec 表迁移**（§九迁移脚本，models 降级为投影，录入断言） | PR-1 | qwen 系列降为 1/10；越界单价被拒；Rust/Python 单一计价真源（Python 不再算 cost） |
| PR-3 | **pricing.rs 引擎**（§4.1-4.4 + ZoneResolver + §十一单测全绿） | PR-2 | 单测 15+ 用例全过；actual_cost 与 litellm 偏差 <1% |
| PR-4 | **Prompt 结构规范落地**（system prompt 前缀稳定化改造 + `prefix_stability` 信号 §5.3） | 无（可与 PR-1 并行） | 同会话第 2 轮起百炼 `cached_tokens > 0`；周命中率报表可见 |
| PR-5 | **路由衔接**：eff_in 进评分、会话亲和加分、est_cost 门槛（§5.1-5.2） | PR-3 + ROUTING-PLAN 步骤 4 | 影子评测下多轮任务成本降 ≥25%（缓存 + 亲和贡献）且质量无回退 |
| PR-6 | **日级校准 job + WebUI 定价页**（§6.2 job + §十端点 + cny 锚点校对 + 对账偏差展示） | PR-1/2/3 | 人工改 DB 价制造偏差 → 连续 3 天后自动标 stale；改价后 input_side_ratio 回归 [0.8,1.2] |
| PR-7 | **探针系统**（§7 probe_loop + 预算状态机 + 谷时调度 + 用量页探针视图 + §十端点） | PR-3 | 探针月成本 ≤ 预算；手动改某模型 API key 为错值 → 2 轮内 health_state=down |
| PR-8 | **峰谷调度（deferrable 任务挪谷时）** | PR-5 + ROUTING-PLAN P4 | 可延迟批任务在 DeepSeek 高峰期进入延迟队列，谷时自动执行；实时路径零延迟变化 |

优先级理由：PR-1/PR-4 零架构风险、当周可见收益（分项账单 + 缓存命中率）；
PR-2/PR-3 是地基（其后一切依赖）；PR-5 才开始影响路由行为；PR-6/7/8 逐层自动化。

---

## 十三、风险与注意事项

1. **价格波动已成常态**（DeepSeek 一周两调、缓存命中价涨 11 倍）：所有面向用户的金额展示
   一律带「按 PriceSpec @ 更新时间估算」口径说明，不承诺与账单完全一致；
2. **`cached_tokens` 字段兼容性**：老版本百炼响应可能缺 `prompt_tokens_details`，
   代码必须 `getattr` 防御（§4.3 `_usage_detail` 已处理）；字段缺失时计价回落为裸乘法
   并在校准里记 `field_missing`（不告警刷屏）；
3. **显式缓存（cache_control）首期不做**：盈亏平衡点依赖复用率，等 `h` 有数据后
   再按任务类型评估（>2 次复用的长前缀任务才值得开）；
4. **汇率**：`cny_to_usd` 只用于录入换算与展示，不做每日动态汇率（成本核算误差 <1%，
   不值得引入新数据源）；
5. **探针双刃剑**：探针自身产生 usage 会污染 `model_task_score` 的 ewma_quality——
   记账时 `task_type='probe'` 必须在所有用户统计口径中排除；探针预算扣减用微整数计数避免 f64 竞态；
6. **节假日表**：法定节假日按年更新，overlay 带版本；过期表只影响 DeepSeek 谷价判断（多收费风险），
   校准层会兜底暴露；
7. **本机网络受限**：刷新走镜像（ROUTING-PLAN 既有结论），但**对账校准不依赖外网**——
   这正是三层闭环设计的原因：断网环境下价格新鲜度靠本地流量自证；
8. **EWMA α 选择**：0.15 ≈ 10 天半衰，对周级调价够灵敏；若未来出现日级调价（DeepSeek 一周两调已逼近），
   α 可调到 0.25（约 5 天半衰），配置项化；
9. **stale 去抖**：单日抖动不标 stale（连续 3 天），避免供应商单日计费异常触发误报；
   `stale_reason` 记录触因便于人工判断。

---

## 十四、与 ROUTING-PLAN.md 的关系总结

| ROUTING-PLAN 条目 | 本方案动作 |
|---|---|
| P0.a 量纲修正 | 并入 PR-2，且从"UPDATE 除以 10"升级为"录入口换算断言"防复发 |
| P0.b `cached_input_cost_per_token` 列 | 扩展为完整 PriceSpec 四分项 + 阶梯 + 时段三维度（独立 price_specs 表） |
| P0.d `est_cost` 门槛 | 输入升级为 §4.4 预估器（eff_in + 时段系数 + 阶梯） |
| P1.a 用量落库 | 追加 7 列（PR-1），是本方案唯一硬前置 |
| P2.a/b 刷新与 overlay | 保留；overlay 条目升级为 §3.3 形态（含 CNY 锚点） |
| P2.c 对账 | 从"可选"升级为核心闭环（§六），用自家账单而非 OpenRouter |
| P3 健康探测 | 探针系统（§七）是其执行载体，并兼任校准燃料 |
| P4/P5 编排与预算 | 峰谷调度（PR-8）与 batch 系数预留（§5.5）挂接 |
| `signals.rs`（P0.g） | 新增 `prefix_stability` 信号（§5.3） |
| `routing_decisions` 表（P0.c） | 校准 job 读其 est_cost，与 price_calibration 联动 |

本方案不改变 ROUTING-PLAN 的阶段骨架，只把其中"定价表"一根线抽出来做深做透。
实施顺序建议插在 ROUTING-PLAN 步骤 1（用量落库）之后、步骤 4（评分路由）之前——
**价格信号不准，评分路由就是精确地错。**

---

## 附录 A：关键公式速查

| 用途 | 公式 |
|---|---|
| 事后精确账单 | `actual_cost = z·[(p−c)·in + c·cache_read + cw·cache_write] + z·out·output + z·reason·reasoning` |
| 事前估算 | `est_cost = z·[est_in·eff_in + est_out·output]` |
| 有效输入单价 | `eff_in = z·[h·cache_read + (1−h)·in]` |
| 命中率 EWMA | `h_new = α·h_observed + (1−α)·h_old`，α=0.15 |
| 对账偏差 | `input_side_ratio = Σact_input_cost / Σest_input_cost`（剔除 est_out 误差） |
| stale 判定 | 连续 3 天 `input_side_ratio ∉ [0.8, 1.2]` → `price_stale=1` |
| 显式缓存盈亏平衡 | 复用次数 ≥ (write_mult − 1) / (1 − read_mult) ≈ 2 次（1.25/0.1 口径） |
| 探针单轮成本上限 | `max(0.002 USD, 10×P95)` |
