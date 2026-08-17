# 智能路由：现状分析与优化计划

> 分析对象：`v2` 分支 @ `091dc31`
> 关注文件：`crates/lloom-core/src/router.rs`、`server.rs`、`db.rs`、`api/ai_service.py`
> 目标：让路由自适应用户任意模型集（含增删）、按「成本 × 成效」分配任务、为预算动态调整留出接口

---

## 一、现状：真实生效的决策链

代码里存在的路由逻辑比实际生效的多。实测调用链只有这一条：

```
POST /api/chat/stream  (server.rs:267)
  │
  ├─ security::check                      PII / 越狱拦截
  ├─ db::list_models(true)                取出 is_active=1 的模型
  ├─ pick_classifier(&models)             硬编码顺序 ["qwen3.6-flash","qwen3-max","qwen-plus"]
  │
  └─ router::route(model="auto", text)    router.rs:178
       └─ classify(text)                  router.rs:163
            ├─ rule_classify(text)        4 组正则，命中即返回
            │    └─ default_model_for_task(task)  ← TASK_MODEL_MAP 硬编码单模型
            └─ (未命中) ai_client::classify  LLM 兜底 → 同样走 TASK_MODEL_MAP
       └─ stream = INFERENCE_MODELS.contains(selected)   ← 硬编码 4 个名字
```

`orchestrate_stream` 走另一套：Rust 把全部 `ModelSpec` 打包丢给 Python，由 `ai_service.py` 的
`_select_model()`（`ai_service.py:629`）用自己那份 `TASK_MODEL_PREFERENCE` 再决策一次。

**即：路由策略有两份互不相通的真源，且两份都是硬编码。**

---

## 二、九个核心问题

### P0-1　`select_model` 是死代码，生效路径不检查可用性

`router.rs` 里的 `select_model()`、`task_model_preference()`、`is_complex()` **在整个 Rust
workspace 中没有任何外部调用点**（已 grep 全仓确认）。真正生效的是
`default_model_for_task()` → `TASK_MODEL_MAP`，它是一个 `task → 单个模型名` 的静态映射，
**完全不看模型是否已注册、是否 is_active**。

后果（`server.rs:309-320`）：路由选出的名字若在注册表里找不到，会**伪造一个空 spec**——
`api_base` 空、`api_key` 空、cost 记 0——然后照常发请求。用户删掉 `deepseek-v3` 之后，
所有 coding/math 请求都会走进这个空 spec 分支直接失败，而不是降级。

### P0-2　`models.task_type` 是死字段

DB 里每个模型都标了 `task_type`，但 router 从不读它（全仓只有写入点，无读取点）。
用户在 WebUI / TUI / CLI 里改这个字段**毫无效果**。
实测数据里 `qwen3.6-flash` 标的是 `classification`，这个值甚至不在 `VALID_TASK_TYPES` 五类之内。

### P0-3　成本字段不参与决策

`select_model` 的文档注释写着 "Pick the cheapest available model"，实现却只是遍历一个
写死的顺序表，**从未读取 `input_cost_per_token` / `output_cost_per_token`**。
成本数据存在、但对路由零影响。

### P0-4　成本数据存在 10 倍量纲错误（实测反推确认）

把 DB 现值反推回官方价，六个模型全部命中同一个换算式
`DB值 = 官方元每百万token × 1.3889e-06`：

| 模型 | DB in | DB out | 反推官方价（元/百万） | 交叉验证 |
|---|---|---|---|---|
| qwen-plus | 1.11e-06 | 2.78e-06 | 0.8 / 2.0 | ✅ 与官方 qwen-plus 0–128K 档一致 |
| qwen3.6-flash | 1.67e-06 | 1.0e-05 | 1.2 / 7.2 | ✅ 与百炼定价页 Qwen3.6-Flash 一致 |
| qwen3.6-plus | 2.78e-06 | 1.667e-05 | 2.0 / 12.0 | — |
| qwen3-max | 3.47e-06 | 1.389e-05 | 2.5 / 10.0 | ✅ 与官方 qwen3-max 0–32K 档一致 |
| deepseek-v3 | 1.39e-06 | 1.111e-05 | 1.0 / 8.0 | — |
| gpt-4o | 2.5e-06 | 1.0e-05 | 2.5 / 10.0 **USD** | ✅ 与 litellm 内置价目表逐位一致 |

`1.3889e-06 = 10 ÷ 7.2 ÷ 1e6`。也就是说：**DashScope 系列的意图是「按 7.2 汇率折算的
USD/token」，但整体放大了 10 倍**；而 gpt-4o 的值是正确的 USD/token。

两个后果：
1. 所有面向用户的成本 / 预算数字，在 qwen 系列上**虚高 10 倍**。
2. **跨供应商比价完全反了**。按 DB 现值，gpt-4o（2.5e-06）看起来比 qwen3-max（3.47e-06）便宜；
   真实情况是 qwen3-max ≈ 0.347 USD/M，比 gpt-4o 便宜约 7 倍。目前 gpt-4o 是 `is_active=0`
   所以没爆，但用户一旦启用任何 OpenAI / Anthropic 模型，成本排序立刻倒置。

### P0-5　用量记录链路是坏的 → 自适应没有燃料

全仓只有一个 `insert_usage` 调用点，在 `server.rs:398`：

```rust
let _ = db::insert_usage(&model, "default", 0, 0, 0.0, None, is_hit);
```

tokens 和 cost 写死 0、`task_type` 传 None、`model` 取不到时回落成字符串 `"default"`，
而且只在语义缓存事件里触发。`chat_stream` 明明拿到了
`res.cost / res.input_tokens / res.output_tokens`（`server.rs:331-341`）却**从不落库**。

DB 实测印证：三条记录 `model_name` 全是 `default`、cost 全 0。

**没有真实用量数据，任何「自适应」「预算动态调整」都无从谈起。这是所有后续工作的前置。**

### P1-6　预算未接入决策链

`/api/budgets/check`（`server.rs:174`）实现完整，但 `chat_stream` / `orchestrate_stream`
**从未调用它**。预算目前纯展示（`UsagePage.tsx` 画个进度条），不构成任何约束。

### P1-7　无 fallback、无健康感知

README 宣称「5 级故障转移」，那是 `legacy` 分支的遗产，v2 没有移植。
当前单点失败即整体失败，没有重试、降级、熔断，也没有任何模型健康状态记录。

### P1-8　`stream` 标志硬编码

`INFERENCE_MODELS` 是 4 个写死的名字。用户新增的任何模型，`stream` 永远是 `false`。

### P2-9　任务分配本身在经济上是次优的

用**反推出的真实价格**重算（混合成本 = 输入 + 2×输出，近似 1:2 的 chat 收发比）：

| 模型 | 输入 | 输出 | 混合成本 | 相对 |
|---|---|---|---|---|
| qwen2.5-local | 0 | 0 | **0** | 本地 |
| qwen-plus | 0.8 | 2.0 | **4.8** | 1.0× |
| qwen3.6-flash | 1.2 | 7.2 | **15.6** | 3.3× |
| deepseek-v3 | 1.0 | 8.0 | **17.0** | 3.5× |
| qwen3-max | 2.5 | 10.0 | **22.5** | 4.7× |
| qwen3.6-plus | 2.0 | 12.0 | **26.0** | 5.4× |

由此暴露两处明确的误配：

- **`qwen3.6-flash` 比 `qwen-plus` 贵 3.3 倍**，但当前 `simple_qa` 偏好表和
  `DECOMPOSER_PREFERENCE`（`ai_service.py:549`）都把 flash 排在 plus 之前。
  名字里的 "flash" 只代表快，不代表便宜。**内部分解/分类路径换用 qwen-plus 可直接省约 69%。**
- **`qwen3.6-plus` 被 `qwen3-max` 全面支配**：max 更便宜（22.5 vs 26.0）且能力更强，
  但 `complex_reasoning` 当前首选 `qwen3.6-plus`。唯一例外是长上下文——qwen3-max
  是阶梯定价（32K–128K 涨到 4/16，128K–252K 涨到 7/28），而 3.6-plus 支持 1M 窗口。
  **所以正确做法不是换个静态首选，而是让路由感知阶梯定价 + 上下文长度。**

---

## 三、外部依据

### 3.1 路由范式与收益（业界实测）

| 范式 | 机制 | 实测收益 | 代价 |
|---|---|---|---|
| Classifier 分类路由 | 轻量分类器一次性判难度 | RouteLLM：MT-Bench 省 48–85%，保 95% GPT-4 质量；仅 14–26% 请求走强模型 | 需标注数据；判错直接损质 |
| Cascade 级联 | 先便宜跑，质量不足再升级 | 97% GPT-4 准确率 @ 24% 成本 | 困难样例延迟翻倍 |
| Semantic 语义路由 | 向量比对意图样例 | 适合固定领域分流 | 需维护意图库 |

三条可直接采纳的经验：

1. **必须按任务类型分别设阈值**。全局单一阈值在异构负载下会明显欠优。
2. **上线前做影子评测（shadow evaluation）**：双跑廉价/强模型，只返回路由结果、两者都记日志，
   抽样人评确认廉价档质量达标后再切流。这能抓住聚合指标掩盖的「某类查询被持续误判为简单」。
3. **分类器可跨模型对迁移**，训练样本 ~1500 条即有效；路由本身开销仅 10–30ms。

RouteLLM 的局限也要记住：它是**二元**（强/弱）路由，本项目是 6+ 模型多档，不能直接套用其预训练路由器。

### 3.2 模型元数据的自动来源（已实测）

项目已依赖 litellm，其内置价目表可直接复用：

```
litellm.model_cost  →  2982 条
字段：input_cost_per_token / output_cost_per_token / cache_read_input_token_cost
     / max_input_tokens / max_output_tokens / supports_function_calling
     / supports_vision / mode / litellm_provider
```

实测覆盖情况：
- `gpt-4o` ✅ HIT（且与 DB 值逐位一致，可作为校验基线）
- `deepseek/deepseek-chat` ✅ HIT
- `qwen-plus` / `qwen-max` ❌ **MISS** —— 百炼 OpenAI 兼容端点（`openai/qwen-*`）不在表内

另有一个运维问题：本机 venv 拉取远端最新价目表时 **SSL 证书校验失败**
（`CERTIFICATE_VERIFY_FAILED`），litellm 回退到打包的本地备份。价格同步要么修证书，
要么显式接受「用本地快照 + 项目 overlay」。

**结论：元数据解析必须做三级兜底**，不能只依赖 litellm。

---

## 四、优化计划

### 设计原则

1. **单一真源**：路由策略下沉到 SQLite + 配置。Rust 是唯一决策者，Python 降级为纯执行器
   （删掉 `ai_service.py` 里的 `TASK_MODEL_PREFERENCE` / `DECOMPOSER_PREFERENCE`，
   改为由 Rust 在请求里下发 `task → model` 的决策结果）。
2. **注册表驱动**：路由只在「当前 is_active 的模型」里做选择，代码里不出现任何具体模型名。
3. **决策可解释**：每次路由在 SSE 头部输出候选集、各项得分、选中理由，便于调试与影子评测。
4. **量纲统一**：全库统一为 **USD per token**，写入前强制归一。

---

### 阶段 P0：拆掉硬编码，让注册表驱动路由

**目标：用户增删任何模型，路由立刻自适应，不改一行代码。**

#### P0.a　修数据（先做，否则后面全是错的）

```sql
-- DashScope 系列统一除以 10，修正量纲
UPDATE models SET input_cost_per_token  = input_cost_per_token  / 10.0,
                  output_cost_per_token = output_cost_per_token / 10.0
WHERE provider = 'dashscope';
```
并在 `Model` 写入路径加断言：单价落在 `[1e-9, 1e-3]` USD/token 之外则拒绝并提示，
防止再次出现量纲错误。

#### P0.b　扩展模型元数据

```sql
ALTER TABLE models ADD COLUMN capability_tier   INTEGER DEFAULT 2;  -- 1 轻量 / 2 通用 / 3 强推理
ALTER TABLE models ADD COLUMN quality_score     REAL    DEFAULT 0.6; -- 0..1 成效基准分
ALTER TABLE models ADD COLUMN context_window    INTEGER DEFAULT 32768;
ALTER TABLE models ADD COLUMN supports_tools    INTEGER DEFAULT 0;
ALTER TABLE models ADD COLUMN supports_vision   INTEGER DEFAULT 0;
ALTER TABLE models ADD COLUMN supports_stream   INTEGER DEFAULT 1;   -- 取代 INFERENCE_MODELS
ALTER TABLE models ADD COLUMN is_local          INTEGER DEFAULT 0;
ALTER TABLE models ADD COLUMN priority          INTEGER DEFAULT 0;   -- 用户手动置顶 / 降权
ALTER TABLE models ADD COLUMN price_tiers_json  TEXT;                -- 阶梯定价
ALTER TABLE models ADD COLUMN cached_input_cost_per_token REAL DEFAULT 0;
ALTER TABLE models ADD COLUMN health_state      TEXT    DEFAULT 'unknown'; -- up/degraded/down
ALTER TABLE models ADD COLUMN health_checked_at TIMESTAMP;
ALTER TABLE models ADD COLUMN needs_calibration INTEGER DEFAULT 1;
```

`price_tiers_json` 形如
`[{"max_input":32768,"in":3.47e-07,"out":1.39e-06},{"max_input":131072,...}]`，
用于 qwen3-max 这类阶梯计价模型 —— 这是 §2 P2-9 那个「max 还是 3.6-plus」问题的正解。

#### P0.c　新增策略表（用户可增删的路由旋钮）

```sql
CREATE TABLE routing_policy (
    task_type            TEXT PRIMARY KEY,
    min_capability_tier  INTEGER DEFAULT 1,
    cost_weight          REAL    DEFAULT 0.4,
    quality_weight       REAL    DEFAULT 0.5,
    latency_weight       REAL    DEFAULT 0.1,
    max_cost_per_request REAL,          -- NULL = 不限
    pinned_model         TEXT,          -- 非空 = 用户强制指定，跳过评分
    fallback_depth       INTEGER DEFAULT 2,
    updated_at           TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE model_task_score (         -- 自适应学到的实际成效
    model_name        TEXT,
    task_type         TEXT,
    success_count     INTEGER DEFAULT 0,
    fail_count        INTEGER DEFAULT 0,
    escalation_count  INTEGER DEFAULT 0,  -- 被 cascade 升级掉的次数
    avg_cost          REAL    DEFAULT 0,
    avg_latency_ms    REAL    DEFAULT 0,
    ewma_quality      REAL    DEFAULT 0.6,
    updated_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (model_name, task_type)
);
```

任务类型本身也应可增删（当前 5 类写死在 `VALID_TASK_TYPES`）。`routing_policy` 以
`task_type` 为主键即天然支持：插一行 = 新增一类任务，删一行 = 回落到默认策略。

#### P0.d　替换决策函数

`router.rs` 里删掉 `TASK_MODEL_MAP`、`INFERENCE_MODELS`、`task_model_preference`，
换成基于评分的选择：

```rust
pub struct Candidate { pub model: Model, pub score: f64, pub reason: String }

/// 在当前可用模型里为 task 选出最优 + fallback 链。
pub fn plan(task: &str, policy: &RoutingPolicy, models: &[Model],
            est_in_tokens: u32, est_out_tokens: u32) -> Vec<Candidate>
```

硬门槛（gate，不满足直接剔除）：
- `is_active == 1`
- `capability_tier >= policy.min_capability_tier`
- `context_window >= est_in_tokens + est_out_tokens`
- `health_state != "down"`
- 需要工具/视觉时 `supports_tools` / `supports_vision` 满足
- `est_cost <= policy.max_cost_per_request`

软打分（0..1 归一后加权）：
```
score = q_w · quality(m, task)          // quality_score 与 ewma_quality 融合
      - c_w · norm(est_cost(m))          // 按阶梯价 + 预估 token 算出的实际花费
      - l_w · norm(avg_latency_ms)
      + 0.05 · priority
```
`argmax` 为主选，其余按 score 降序取前 `fallback_depth` 个作为降级链。

**关键修复**：不再有任何硬编码兜底。候选集为空时返回明确错误
（"当前无可用模型满足 X 任务的约束"），而不是伪造空 spec 或回落到可能不存在的 `qwen2.5-local`。

`stream` 改读 `supports_stream`。

#### P0.e　增删模型时的自动打标（自适应的核心）

`POST /api/models` 之后跑一次元数据解析，三级兜底：

1. **litellm 价目表**：查 `litellm.model_cost[litellm_model]`，命中即填单价、
   `cache_read_input_token_cost`、`max_input_tokens`、`supports_*`。覆盖 2982 个模型。
2. **项目 overlay**：内置一份 `model_catalog.json`，补 litellm 缺的百炼 / 国产模型
   （已实测 `qwen-plus`、`qwen-max` 在 litellm 里 MISS，必须有这层），随版本更新。
3. **名称启发式**（最后兜底）：
   - 含 `flash|mini|turbo|small|lite|air|8b|4b|1.5b` → tier 1
   - 含 `max|opus|ultra|pro|reasoner|thinking|r1|o1|o3` → tier 3
   - 其余 → tier 2
   - `provider == "ollama"` 或 `api_base` 指向 localhost → `is_local=1`，单价 0
   - 命中启发式的标记 `needs_calibration=1`

`needs_calibration=1` 的模型进入**保守期**：只参与低风险任务（`simple_qa` / `general`）
且限流，用真实调用结果回填 `ewma_quality`，达到样本阈值后解除。
这样用户加一个从未见过的模型也不会一上来就承接关键任务。

**删除路径**：保持现有软删（`is_active=0`）。额外做两件事——
① 决策时天然过滤，无需清理引用；
② 若被删模型是某条 `routing_policy.pinned_model`，把该字段置 NULL 并在 UI 提示
「原固定模型已停用，已恢复自动选择」，避免留下悬空引用。

#### P0.f　消除双真源

`ai_service.py` 删掉 `TASK_MODEL_PREFERENCE` / `DECOMPOSER_PREFERENCE` / `_select_model`。
`/v1/orchestrate/stream` 的请求体改为携带 Rust 已决策好的
`{subtask_id → model_spec}` 映射；Python 只负责执行与流式返回。
分解器用哪个模型也由 Rust 按 `task_type="decompose"` 走同一套评分选出。

---

### 阶段 P1：打通用量闭环 + 成本×成效落地

#### P1.a　修用量落库（**最高优先级**）

`chat_stream` 成功返回后写入真实数据：

```rust
db::insert_usage(&spec.name, user_id,
                 res.input_tokens, res.output_tokens, res.cost,
                 Some(&routing.task_type), cache_hit)?;
```
`orchestrate_stream` 同理，按子任务逐条记账。同时把 `server.rs:398` 那个
「全 0 + model_name='default'」的调用改成只写缓存标定、不冒充用量记录，
并清理 DB 里已有的 3 条脏数据。

补齐后应新增：`latency_ms` 字段、`request_id`（串联 cascade 的多次调用）。

#### P1.b　基于当前模型集的推荐分配

修正价格后的建议（`成本` 列为混合成本，元/百万 token，输入:输出按 1:2）：

| 任务类型 | 建议主选 | 成本 | 降级链 | 相对现状 |
|---|---|---|---|---|
| `simple_qa` | qwen2.5-local | 0 | qwen-plus → qwen3.6-flash | 云端兜底由 flash 改 plus |
| `general` | qwen-plus | 4.8 | qwen3.6-flash → local | 不变 |
| `decompose` / `classify`（内部） | **qwen-plus** | 4.8 | qwen3.6-flash | **省约 69%**（原首选 flash） |
| `coding` | deepseek-v3 | 17.0 | qwen-plus → qwen3-max | 升级位由 plus 改 max |
| `math_logic` | deepseek-v3 | 17.0 | **qwen3-max** → qwen-plus | **升级位由 3.6-plus 改 max**（更便宜且更强） |
| `complex_reasoning` | **qwen3-max**（输入 ≤32K） | 22.5 | deepseek-v3 → qwen-plus | **主选由 3.6-plus 改 max，省 13%** |
| `complex_reasoning`（输入 >32K） | qwen3.6-plus | 26.0 | qwen3-max | 阶梯价交叉后 3.6-plus 才划算 |

最后一行正是 `price_tiers_json` 的价值：同一任务在不同上下文长度下最优模型会切换，
静态偏好表表达不了这件事。

#### P1.c　成效分的两个来源

- **冷启动**：litellm 元数据 + overlay 里的基准分（可用公开榜单折算到 0..1）。
- **在线修正**：`ewma_quality` 由本地信号更新——
  用户重新生成 / 手动切模型重问（负）、cascade 升级率（负）、
  正常完成（正）、显式点赞点踩（强信号）、结构化输出解析失败率（负）。
  `ewma_quality ← α · 本次信号 + (1-α) · 旧值`，α 取 0.1~0.2。

#### P1.d　分任务阈值 + 影子评测

按外部经验，阈值必须**按 task_type 分别标定**。落地方式：
`routing_policy` 的三个 weight 就是每类任务独立的旋钮，默认值按任务给不同预设
（`simple_qa` 偏成本 c_w=0.7，`complex_reasoning` 偏质量 q_w=0.8）。

新增 `POST /api/routing/shadow` 开关：开启后对采样流量同时跑「路由选择」与
「强模型基线」，两份结果都落 `model_task_score`，只把路由结果返回用户。
项目已有 `cache_calibration` 表的类似模式（`sim`/`decision`/`label`/`source`），
可复用其设计，新增 `routing_calibration` 表。

---

### 阶段 P2：健康感知与故障转移

- 被动健康：调用失败 / 超时 / 429 累计，滑窗内超阈值 → `health_state='degraded'`，
  再超 → `'down'` 并进入指数退避探测。
- 主动探测：`down` 状态每 N 秒发一次最小请求试探恢复。
- 故障转移：P0.d 已产出 fallback 链，`ai_client::chat` 失败时按链顺序重试，
  每次重试记 `escalation_count`。这才是 README 承诺的「多级 failover」的真实实现。
- 熔断：单模型连续失败达阈值，临时从候选集剔除，不阻塞其他模型。

---

### 阶段 P3：预算驱动的动态调整

前置条件：P1.a 用量数据必须先真实可信，否则预算是空中楼阁。

#### P3.a　预算进入决策链

在 `plan()` 之前插入预算查询，把「剩余预算比例」`r` 作为路由的全局调节量：

| 剩余预算 | 档位 | 行为 |
|---|---|---|
| r > 50% | 正常 | 按 `routing_policy` 原权重 |
| 20% < r ≤ 50% | 节流 | `cost_weight × 1.5`，`min_capability_tier` 不变 |
| 5% < r ≤ 20% | 紧缩 | `cost_weight × 2.5`；`complex_reasoning` 降一档；强制开语义缓存 |
| r ≤ 5% | 保护 | 仅允许 `is_local=1` 或混合成本低于阈值的模型；超限任务返回明确提示 |

关键在于**降级而非硬拒**：预算耗尽时把请求推给本地 Ollama，而不是报错。
这也解释了为什么 `qwen2.5-local`（成本 0）在候选集里必须始终保留。

#### P3.b　预算模型扩展

```sql
ALTER TABLE budgets ADD COLUMN scope_task_type TEXT;  -- 按任务类型分预算
ALTER TABLE budgets ADD COLUMN soft_limit_ratio REAL DEFAULT 0.8;  -- 触发节流的水位
ALTER TABLE budgets ADD COLUMN action_on_exceed TEXT DEFAULT 'degrade'; -- degrade / block
```
`scope` 从当前的 `user` 扩到 `user` / `model` / `task_type` / `global`。

#### P3.c　预估成本前置校验

请求进入时用 tiktoken（项目已有 `tiktoken_cache/`）算输入 token，
输出 token 用该 `task_type` 的历史 P50 估计，得到 `est_cost`；
`est_cost` 参与 §P0.d 的 `max_cost_per_request` 门槛与 §P3.a 的档位判断。

---

## 五、落地顺序与验收

| 顺序 | 内容 | 验收标准 |
|---|---|---|
| 1 | P1.a 修用量落库 | 一次对话后 `usage_records` 出现正确的 model_name / tokens / cost / task_type |
| 2 | P0.a 修成本量纲 + 写入断言 | qwen 系列单价降为原 1/10；写入越界单价被拒 |
| 3 | P0.b/c 建表与迁移 | 迁移在已有 `lloom.db` 上幂等可重跑，旧数据不丢 |
| 4 | P0.d 评分式 `plan()` | 单测：删掉任一模型后路由自动改选，且**不再返回未注册的名字** |
| 5 | P0.e 自动打标 | 新增一个未知模型 → 元数据自动填充、标 `needs_calibration`、不承接复杂任务 |
| 6 | P0.f 消除 Python 侧真源 | `ai_service.py` 中不再出现任何模型名字面量 |
| 7 | P1.b/c 成效分 + 推荐分配 | 影子评测下，内部分解路径成本下降 ≥60%，质量无显著回退 |
| 8 | P2 健康与 fallback | 手动停掉 Ollama / 改错 key，请求自动降级而非失败 |
| 9 | P3 预算联动 | 把预算调到接近耗尽，观察路由逐档降级直至只走本地 |

**当前缺失的测试基建**：`tests/` 下只有 `__pycache__`，Rust 侧无任何单测。
`router.rs` 是纯函数为主的模块，最适合先补单测——建议在第 4 步同时建立
`crates/lloom-core/src/router.rs` 的 `#[cfg(test)] mod tests`，覆盖：
空候选集、单模型、删除主选后降级、阶梯价交叉点、预算各档位。

---

## 六、风险与注意事项

1. **价格数据会过期**。§2 P0-4 的官方价来自 2026-08 的公开资料，且不同来源对
   `qwen3.6-flash` 存在 `1.2/7.2` 与 `0.367/2.936` 两种口径（本文采信 1.2/7.2，
   因其与 DB 反推值逐位吻合且来自百炼定价页）。
   **所以不要把价格写进代码**——放 DB + overlay JSON，并提供「从控制台核对并校准」的入口。
2. **litellm 远端价目表拉取在本机 SSL 校验失败**，当前静默回退本地快照。
   若依赖自动同步，需先修证书链，否则价格会长期停在打包版本。
3. **不要用 RouteLLM 的预训练路由器直接套用**：它面向二元强/弱决策，本项目是 6+ 档。
   可借用其「分类器 + 分任务阈值」的思路，但路由器要自己按本地日志训练。
4. **cascade 会让困难请求延迟翻倍**。建议只在 `orchestrate` 这类非实时路径启用，
   `chat/stream` 保持一次性分类路由。
5. **迁移幂等性**：`data/lloom.db` 是真实数据（含对话与向量），所有 ALTER 必须写成
   可重复执行（先查 `PRAGMA table_info` 再决定是否加列），且迁移前备份。
