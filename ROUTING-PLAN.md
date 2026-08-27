# 智能路由：现状分析与优化计划 v3

> 分析对象：`v2` 分支 @ `a2b8bb5`（"Context optimization: SQLite conversation store, budgeted context, two-layer cache"）
> 关注文件：`crates/lloom-core/src/{router,db,models,ai_client,server,config}.rs`、`api/ai_service.py`
> 目标：让路由自适应用户任意模型集（含增删）、按「成本 × 成效」分配任务、为预算动态调整留出接口
>
> **行号约定**：本文行号基于 `a2b8bb5`，会随版本变动——实施时以**函数名**为锚点定位，行号仅作辅助。
>
> **版本演进**：
> - v1（@ 091dc31）：现状分析 + 九问题 + P0–P3 骨架。
> - v2（2026-08-24）：融合 NeMo Switchyard、vLLM Semantic Router、定价系统、Router-R1/R2-Router/AutoMix/BEST-Route/RouterBench。
> - **v3（本次）**：核对当前代码（行号/签名/已变化点）+ 把每个阶段落实到**可执行技术细节**（结构体改动、函数签名、幂等迁移脚本、评分公式、EWMA 更新、价格刷新 job 骨架、stage 路由伪代码、SSE 事件契约、WebUI 字段）。

---

## 一、现状：真实生效的决策链（v3 复核 @ a2b8bb5）

代码里存在的路由逻辑比实际生效的多。实测调用链只有这一条：

```
POST /api/chat/stream  (server.rs:chat_stream @ 327)
  │
  ├─ security::check(&user_text, true, true)          PII / 越狱拦截
  ├─ db::list_models(true)                             取 is_active=1 模型
  ├─ pick_classifier(&models)  (server.rs:870)        硬编码 ["qwen3.6-flash","qwen3-max","qwen-plus"]
  │
  └─ router::route(model, user_text, classifier)      (router.rs:178)
       └─ classify(text, classifier)                   (router.rs:163)
            ├─ rule_classify(text)                      4 组正则，命中即返回
            │    └─ default_model_for_task(task)       ← TASK_MODEL_MAP 硬编码单模型 (router.rs:74)
            └─ (未命中) ai_client::classify            LLM 兜底 → 同样走 TASK_MODEL_MAP
       └─ stream = INFERENCE_MODELS.contains(selected)  ← 硬编码 4 名 (router.rs:22)
  └─ spec = models.find(name) ?? 伪造空 spec           (server.rs:369-380) ← 删模型后直接失败
  └─ ai_client::chat(&spec, msgs, 500, 0.3)            (server.rs:391) → 返回 ChatResult{content,cost,tokens}
       └─ ⚠️ 拿到 res.cost/tokens 却【从不落库】       ← P0-5 未修一半
```

`orchestrate_stream`（`server.rs:412`）走另一套：Rust 把全部 `ModelSpec` 打包（`server.rs:422`）丢给 Python
`ai_client::orchestrate_stream`（`ai_client.rs:144`），由 `ai_service.py` 的 `_select_model()`
（`ai_service.py:966`）用自己那份 `TASK_MODEL_PREFERENCE`（`ai_service.py:888`）再决策一次；
复杂路径的分解器选择遍历 `DECOMPOSER_PREFERENCE`（`ai_service.py:886` 与 `1453`）。

**即：路由策略有两份互不相通的真源，且两份都是硬编码。**

> **v4 注记（2026-08-26，commit 8dddc59）**：上图为 v3 时点快照。P0.d 落地后 **chat 路径已切换**：
> `router::route` → `classify`（仅分类，无模型映射）→ `band_for` → `plan()`（注册表门槛+评分，
> 成本走 `price_specs`/`pricing.rs`）→ primary + fallback_chain；`pick_classifier` 改注册表驱动；
> direct 未注册模型明确报错（SSE error）。**orchestrate 路径也已由 `router::plan_decision` 下发
> assignments（general/decompose/aggregate），Python 读 assignments 兜底 `models[0]`，无模型名字面量**（P0.f 已修）。
> 双真源已合一到 Rust，仅剩 P4.a 子任务级分配未落。

---

## 二、九个核心问题（v3 复核，状态更新）

### P0-1　`select_model` 是死代码，生效路径不检查可用性　【已修 2026-08-26：select_model/TASK_MODEL_MAP 已删，plan() 全程查注册表】

`router.rs` 的 `select_model()`（131）、`task_model_preference()`（119）、`is_complex()`（101）
在全 workspace 无外部调用点。生效的是 `default_model_for_task()` → `TASK_MODEL_MAP`（router.rs:14），
**完全不看 is_active**。

后果（`server.rs:369-380`）：路由名在注册表找不到时 `unwrap_or_else` 伪造空 `ModelSpec`
（`api_base`/`api_key` 空、cost 0）照常发请求 → 删 `deepseek-v3` 后 coding/math 直接失败而非降级。

### P0-2　`models.task_type` 是死字段　【仍成立】

DB `models.task_type` 全仓只有写入点（`db::insert_model`/`update_model` 白名单），无读取点。
`_select_model(task_type, models)`（ai_service.py:966）遍历的是 `TASK_MODEL_PREFERENCE` 常量，不读 DB 字段。
用户在 UI 改 `task_type` 毫无效果。

### P0-3　成本字段不参与决策　【已修 2026-08-26：plan() 评分含 est_cost（price_specs 真源）+ max_cost_per_request 门槛】

`select_model` 注释 "Pick the cheapest available model"，实现只遍历写死顺序表，从不读
`input_cost_per_token`/`output_cost_per_token`。

### P0-4　成本数据存在 10 倍量纲错误　【已修：÷10 迁移（PRICING-PLAN）+ models 写入断言 [1e-9,1e-3]（09480fa）】

`DB值 = 官方元每百万token × 1.3889e-06`（= 10÷7.2÷1e6），六模型逐位吻合：

| 模型 | DB in | DB out | 反推官方价（元/百万） | 交叉验证 |
|---|---|---|---|---|
| qwen-plus | 1.11e-06 | 2.78e-06 | 0.8 / 2.0 | ✅ 官方 0–128K 档 |
| qwen3.6-flash | 1.67e-06 | 1.0e-05 | 1.2 / 7.2 | ✅ 百炼定价页 |
| qwen3.6-plus | 2.78e-06 | 1.667e-05 | 2.0 / 12.0 | — |
| qwen3-max | 3.47e-06 | 1.389e-05 | 2.5 / 10.0 | ✅ 官方 0–32K 档 |
| deepseek-v3 | 1.39e-06 | 1.111e-05 | 1.0 / 8.0 | — |
| gpt-4o | 2.5e-06 | 1.0e-05 | 2.5 / 10.0 **USD** | ✅ litellm 内置表逐位一致 |

即 DashScope 系列意图是「7.2 汇率 USD/token」但放大 10 倍；gpt-4o 是正确 USD/token。
后果：①qwen 成本/预算虚高 10 倍；②跨供应商比价倒置（DB 现值 gpt-4o 看着比 qwen3-max 便宜，真实相反，差约 7 倍）。

### P0-5　用量记录链路　【v3 状态：半修，仍需补】　⚠️

v3 复核发现编排路径已部分修复，但 chat 路径仍坏：

- **`chat_stream`（server.rs:391-404）**：拿到 `ChatResult{cost, input_tokens, output_tokens, model}`
  却**从不调用 `insert_usage`** → 简单问答零用量记录。
- **`orchestrate_stream`（server.rs:486-493）**：已在 `result` 事件里调用
  `db::insert_usage(&model, "default", in_tok, out_tok, cost, None, is_hit)` ——
  token/cost 已是真实值（从 Python `usage_ref` 读，非写死 0），**但 `task_type` 仍传 `None`、
  `model` 取不到时回落 `"default"`、且无 `latency_ms`/`request_id`**。
- `usage_records` 表（`db.rs:24`）**无 `latency_ms`/`request_id`/`task_type` 已有列**——`task_type` 列存在但未被填充。

结论：自适应燃料缺口缩小到「chat 路径零记录 + 编排路径 task_type 缺失 + 无延迟维度」。P1.a 范围相应收窄。

### P1-6　预算未接入决策链　【仍成立】

`check_budget`（server.rs:195-211）实现完整（`get_budget`+`get_total_spend`→`within_budget`），
路由注册于 `server.rs:767`。但 `chat_stream`/`orchestrate_stream` **从不调用它**——
预算纯展示（`UsagePage.tsx` 进度条），不构成约束。

### P1-7　无 fallback、无健康感知　【仍成立】

`legacy` 的「5 级 failover」未移植。单点失败即整体失败，无重试/降级/熔断/健康记录。

### P1-8　`stream` 标志硬编码　【已修 2026-08-26：读 models.supports_stream（须流式=推理系），随迁移回填】

`INFERENCE_MODELS`（router.rs:22）= 4 个写死名字。新增模型 `stream` 永远 false。

### P2-9　任务分配经济次优　【部分已修 2026-08-26：chat 路径 plan() 评分已成本感知；orchestrate 路径待 P0.f/P4】

反推真实价重算（混合成本 = 输入 + 2×输出，元/百万）：

| 模型 | 输入 | 输出 | 混合成本 | 相对 |
|---|---|---|---|---|
| qwen2.5-local | 0 | 0 | **0** | 本地 |
| qwen-plus | 0.8 | 2.0 | **4.8** | 1.0× |
| qwen3.6-flash | 1.2 | 7.2 | **15.6** | 3.3× |
| deepseek-v3 | 1.0 | 8.0 | **17.0** | 3.5× |
| qwen3-max | 2.5 | 10.0 | **22.5** | 4.7× |
| qwen3.6-plus | 2.0 | 12.0 | **26.0** | 5.4× |

两处误配（P0.f 已从根上消除：`assignments` 由 `plan()` 按 `price_specs` 真源成本评分给出，
Python 不再有 `TASK_MODEL_PREFERENCE`/`DECOMPOSER_PREFERENCE` 静态首选字面量）：
- **qwen3.6-flash 比 qwen-plus 贵 3.3×**，历史 `simple_qa`/分解静态首选把 flash 排在 plus 前。
  名字 flash 只代表快不代表便宜。现由 `plan()` 成本评分自动规避。
  **内部分解/分类换 qwen-plus 直接省约 69%。**
- **qwen3.6-plus 被 qwen3-max 全面支配**（max 更便宜且更强），历史 `complex_reasoning` 首选 3.6-plus。
  例外：qwen3-max 阶梯定价（32K–128K→4/16，128K–252K→7/28），3.6-plus 支持 1M 窗口 →
  长上下文才该用 3.6-plus。**正解：路由感知阶梯价 + 上下文长度，而非换静态首选。**

---

## 三、外部依据（v2 引入，v3 保留）

### 3.1 路由范式与收益（诚实口径）

| 范式 | 实测收益 | 代价 |
|---|---|---|
| Classifier 分类路由 | RouteLLM（ICLR 2025）：MT-Bench 省 85%、MMLU 45%、GSM8K 35%，保 95% GPT-4 质量 | 需标注；判错损质 |
| Cascade/级联 | FrugalGPT 最好情形 98%；AutoMix（NeurIPS 2024）>50%；BEST-Route（ICML 2025）60%/<1%降 | 困难样例延迟翻倍；开放式需 judge |
| Stage 路由 | Cognition：距 Opus 5 差 2.8 点，成本降 28% | 需执行事件流（LLooM 已有 `task_done.error`） |
| Escalation 升级 | LangChain：145 任务成本降 74%（仅 7% 走前沿），精度损约 6 点 | 每请求多一次裁判 |
| 编排路由 | R2-Router：子任务级分配；Switchyard 内部基准：成本降到 Opus 4.8 独跑的约 1/3 | 系统复杂度最高 |

**诚实口径**：收益 35–85% 取决于负载难度分布（chat 简单 14–26% 需强模型，知识考试 54% 需强模型）；
Bedrock 官方 16–56%、开销约 85ms。验收写「随难度可变」的 AIQ 式指标，不承诺固定百分比。

六条经验：①按任务类型分别设权重；②上线前影子评测；③分类器 ~1500 样本即有效、开销 <100ms；
④级联只用于非实时路径；⑤先规则兜底后学习路由；⑥RouterBench 的 AIQ 思路（全弱下界/全强上界/填补比例）。

### 3.2 NeMo Switchyard（借鉴设计，不引入依赖，pre-alpha v0.2.0）

- 决策信号三分类 → LLooM：能力（`quality_score`/`ewma_quality`）、成本画像（`price_tiers_json`+`avg_latency`）、
  基础设施（`health_state`+预算水位+定价新鲜度）。
- 四算法映射：Random（影子评测基线）/ LLM classifier capability（`ai_client::classify` 正规化）/
  escalation（P4 编排升级）/ stage_router（`task_done.error` 即现成 stage 信号）。
- 可观测：每次路由记五元组（所选模型/决策依据/token/延迟/结果）→ `routing_decisions` 表；
  routing overhead 为一等指标（与模型调用耗时分开，预算 <100ms）。
- libsy Step 流 = P0.f「Rust 决策 / Python 执行」分离的同款设计。

### 3.3 vLLM Semantic Router（信号—投影—决策三层，不引入 Envoy）

```
信号 Signal（检测，只答"看到了什么"）
  启发式：authz/conversation/context/keyword/language/structure/event/metadata
  学习型：classifier/complexity/domain/embedding/modality/fact-check/jailbreak/pii/preference/reask/kb/user-feedback
投影 Projection（协调）：partitions（独占域）/ scores（weighted_sum）/ mappings（threshold_bands → easy/medium/hard）
决策 Decision（规则）：对信号/投影的 AND/OR → 候选集
```

LLooM 现有组件映射（收敛现状，非重写）：

| 现有组件 | → 信号 | 类型 |
|---|---|---|
| `security::check` | `pii`/`jailbreak` | 学习型（已有） |
| `router::rule_classify` 4 组正则 | `keyword`/`structure` | 启发式（待正规化） |
| `ai_service.py::_is_complex`/`_is_comparison`（947/935） | `complexity`/`structure` | 启发式（待上移 Rust） |
| `ai_client::classify` LLM 兜底 | `classifier` | 学习型（低置信才触发） |
| 语义缓存 embedding | `embedding` + 插件 `response_cache` | 学习型（复用向量） |
| `cache_feedback` 点赞点踩 | `user-feedback` | 学习型（已有） |
| 短间隔重问 | `reask`（隐式不满） | 待新增 |
| tiktoken 上下文计数 | `context` | 启发式 |

### 3.4 定价与元数据源实测（2026-08-24 复测）

| 源 | 覆盖 | 本机可达 | 结论 |
|---|---|---|---|
| litellm 打包表 `litellm.model_cost` | 2982 条 | ✅ 本地 | gpt-4o✅ deepseek-chat✅ **qwen-plus/qwen-max ❌ MISS** |
| litellm GitHub raw | 同上最新 | ⚠️ SSL 失败→走镜像 | 刷新源 |
| models.dev `api.json` | 75+ provider/2000+ | ⚠️ 直连被重置→镜像 | **无 dashscope 条目**；七牛托管 qwen 但无 cost；国际模型+context 补充源 |
| OpenRouter `/api/v1/models` | 400+，实时 | 未启用前不适用 | 未来 `usage.cost` 对账源 |
| 项目 overlay `model_catalog.json` | 自维护 | ✅ | **唯一覆盖百炼的层** |

结论：①百炼在所有自动源均无价 → overlay 必需；②正确形态 = 快照+刷新+overlay+人工校准；
③受限网络走镜像（jsdelivr/ghproxy，复用 `GH_MIRROR`），失败静默保持本地值并标陈旧。

### 3.5 自进化路由研究

- **Router-R1（2026）**：RL 序列决策路由器，**以模型描述符（价格/延迟/样本表现）为条件 → 泛化到未见模型**
  （10 个训练能路由第 11 个）。验证 P0「注册表驱动 + 描述符评分」路线。
- **R2-Router**：查询分解为子任务跨异构 LLM 分配 → 编排路径目标形态。
- **BEST-Route（ICML 2025）**：联合「模型×采样次数」省 60% → 远期对低置信请求并行采样两轻量档+裁决。

---

## 四、目标架构：信号—投影—决策管线

```
请求 → [信号层] 启发式快路径(<10ms) + 学习型慢路径(按需,<100ms)
      → [投影层] difficulty=Σwᵢ·signal / band=easy|medium|hard / intent=task_type
      → [决策层 plan()] 硬门槛过滤 + 加权评分 → 主选 + fallback 链 + 审计
      → [执行] Python 纯执行器 / 编排路径 stage 信号升级
      → [反馈] usage→EWMA / feedback→质量信号 / 影子评测+AIQ / 定价刷新
```

- v1 的 `plan()`（门槛+评分+fallback）保留为决策层；v2 新增信号/投影层把难度从布尔升级为加权分+分带。
- 信号层配置化：命名注册，决策只引用名字；落地 = `lloom-core/src/signals.rs` + DB `settings` KV 存阈值。

### 4.4 执行期 stage 信号与升级（编排路径）
`task_done.error` 非空 → 子任务按 fallback 链降级重试一次（记 `escalation_count`）；重试仍失败如实汇报；
连续 ≥2 失败 → 剩余子任务整体升一档；全绿且耗时 <P50 → 同批后续可降一档试探（失败回滚）。

### 4.5 Escalation 模式（可选，仅编排路径）
开关打开时子任务先跑轻量档，裁判信号判质量（结构化解析成功/JSON schema 校验=零成本信号优先，
不够才用 LLM judge）→ 不达标升级强档。默认只对 `decompose`/`classify`/`simple_qa` 这类
「解析即判对错」任务开启；开放式不开启（置信度不可靠）。

---

## 五、优化计划（v3 落实技术细节）

### 设计原则
1. 单一真源（Rust 决策，Python 纯执行）；2. 注册表驱动/描述符条件化（不看模型名，Router-R1 验证）；
3. 决策可解释（审计+ SSE 头）；4. 量纲统一 USD/token；5. 路由自身有预算（<10ms/<100ms）；
6. 检测与决策分离。

---

### 阶段 P0：拆硬编码，注册表驱动路由

#### P0.a　修数据 + 写入断言

```sql
-- 幂等：只在量纲仍错（DashScope 且单价 >1e-5）时执行
UPDATE models SET input_cost_per_token  = input_cost_per_token  / 10.0,
                  output_cost_per_token = output_cost_per_token / 10.0
WHERE provider = 'dashscope' AND input_cost_per_token > 1.0e-5;
```

写入断言加在 `db::insert_model` 与 `db::update_model`：
```rust
fn validate_cost(in: f64, out: f64) -> Result<()> {
    const LO: f64 = 1e-9; const HI: f64 = 1e-3; // USD/token
    if !(in == 0.0 || (LO..=HI).contains(&in)) || !(out == 0.0 || (LO..=HI).contains(&out)) {
        return Err(AppError::InvalidRequest(format!(
            "单价越界 [1e-9, 1e-3] USD/token: in={in} out={out}；疑似量纲错误（百炼价是否忘了 /10 或汇率）")));
    }
    Ok(())
}
```
`insert_model`（db.rs:120）在执行前调 `validate_cost`；`update_model`（db.rs:176）当 updates 含
`input_cost_per_token`/`output_cost_per_token` 时调。**两者都已校验完才写库**，否则 422 拒绝。

#### P0.b　扩展模型元数据（含 v2 定价溯源三列）

**幂等迁移**（放 `db::init_db` 后或单独 `migrate_v2()`，每条 `ALTER` 先查 `PRAGMA table_info(models)` 去重）：

```sql
ALTER TABLE models ADD COLUMN capability_tier   INTEGER DEFAULT 2;
ALTER TABLE models ADD COLUMN quality_score     REAL    DEFAULT 0.6;
ALTER TABLE models ADD COLUMN context_window    INTEGER DEFAULT 32768;
ALTER TABLE models ADD COLUMN supports_tools    INTEGER DEFAULT 0;
ALTER TABLE models ADD COLUMN supports_vision   INTEGER DEFAULT 0;
ALTER TABLE models ADD COLUMN supports_stream   INTEGER DEFAULT 1;
ALTER TABLE models ADD COLUMN is_local          INTEGER DEFAULT 0;
ALTER TABLE models ADD COLUMN priority          INTEGER DEFAULT 0;
ALTER TABLE models ADD COLUMN price_tiers_json  TEXT;
ALTER TABLE models ADD COLUMN cached_input_cost_per_token REAL DEFAULT 0;
ALTER TABLE models ADD COLUMN health_state      TEXT DEFAULT 'unknown';
ALTER TABLE models ADD COLUMN health_checked_at TIMESTAMP;
ALTER TABLE models ADD COLUMN needs_calibration INTEGER DEFAULT 1;
ALTER TABLE models ADD COLUMN price_source     TEXT DEFAULT 'unknown';
ALTER TABLE models ADD COLUMN price_updated_at TIMESTAMP;
ALTER TABLE models ADD COLUMN price_stale      INTEGER DEFAULT 0;
```

`price_tiers_json` 形如 `[{"max_input":32768,"in":3.47e-07,"out":1.39e-06},...]`（USD/token，升序按 max_input）。
来源优先级：`manual > overlay > litellm_remote > litellm_packaged > heuristic`（manual 永不自动覆盖）。

**联动改动清单**（新增列必须同步改这些点，否则读不到/写不进）：
1. `models.rs::Model` 结构体加全部新字段（带 `#[serde(default)]`）。
2. `db.rs::model_from_row`（229）加 `row.get("capability_tier")?` 等读取。
3. `db.rs::insert_model`（120）SQL 列与 `params!` 补新字段（非用户填的给默认/由 P0.e 回填）。
4. `db.rs::update_model` 的 `ALLOWED` 白名单（184）加：`capability_tier`/`priority`/`quality_score`/
   `context_window`/`supports_tools`/`supports_vision`/`supports_stream`/`is_local`/`price_tiers_json`/
   `input_cost_per_token`/`output_cost_per_token`/`price_source`/`is_active`（用户可在 UI 改这些）。
   ⚠️ `health_state`/`needs_calibration`/`price_stale` 由系统写，**不放白名单**（防用户手改）。
5. `models.rs::Model::to_ai_spec`（43）：Python 端只需 `name/litellm_model/api_base/api_key/cost`，
   **新字段是 Rust 决策用，不必全传 Python**（保持 ModelSpec 不变，见 P0.f 契约）。

#### P0.c　新增策略表与审计表（幂等建表）

```sql
CREATE TABLE IF NOT EXISTS routing_policy (
    task_type            TEXT PRIMARY KEY,
    min_capability_tier  INTEGER DEFAULT 1,
    cost_weight          REAL    DEFAULT 0.4,
    quality_weight       REAL    DEFAULT 0.5,
    latency_weight       REAL    DEFAULT 0.1,
    max_cost_per_request REAL,
    pinned_model         TEXT,
    fallback_depth       INTEGER DEFAULT 2,
    escalation_enabled   INTEGER DEFAULT 0,
    updated_at           TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS model_task_score (
    model_name TEXT, task_type TEXT,
    success_count INTEGER DEFAULT 0, fail_count INTEGER DEFAULT 0,
    escalation_count INTEGER DEFAULT 0,
    avg_cost REAL DEFAULT 0, avg_latency_ms REAL DEFAULT 0,
    ewma_quality REAL DEFAULT 0.6, sample_count INTEGER DEFAULT 0,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (model_name, task_type)
);
CREATE TABLE IF NOT EXISTS routing_decisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id TEXT, created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    task_type TEXT, band TEXT,
    signals_json TEXT, candidates_json TEXT,
    selected TEXT, fallback_chain TEXT,
    routing_ms REAL, outcome TEXT
);
CREATE INDEX IF NOT EXISTS idx_rd_created ON routing_decisions(created_at);
CREATE INDEX IF NOT EXISTS idx_rd_task ON routing_decisions(task_type);
CREATE TABLE IF NOT EXISTS routing_calibration (   -- 影子评测（复用 cache_calibration 模式）
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    task_type TEXT, query_hash TEXT,
    routed_model TEXT, baseline_model TEXT,
    routed_cost REAL, baseline_cost REAL,
    routed_quality REAL, baseline_quality REAL, label INTEGER, source TEXT
);
```

**种子策略**（按 §P1.b 推荐分配预填，用户可改）：
```sql
INSERT OR IGNORE INTO routing_policy VALUES
('simple_qa',1,0.7,0.2,0.1,NULL,NULL,2,0,CURRENT_TIMESTAMP),
('general',2,0.5,0.4,0.1,NULL,NULL,2,0,CURRENT_TIMESTAMP),
('coding',3,0.3,0.6,0.1,NULL,NULL,2,0,CURRENT_TIMESTAMP),
('math_logic',3,0.3,0.6,0.1,NULL,NULL,2,0,CURRENT_TIMESTAMP),
('complex_reasoning',3,0.2,0.7,0.1,NULL,NULL,2,0,CURRENT_TIMESTAMP),
('decompose',1,0.6,0.3,0.1,NULL,NULL,1,1,CURRENT_TIMESTAMP),
('aggregate',2,0.4,0.5,0.1,NULL,NULL,2,0,CURRENT_TIMESTAMP);
```

新增 CRUD：`db::{get_routing_policy(task)->Option<RoutingPolicy>, list_routing_policy(),
upsert_routing_policy(..), insert_routing_decision(..), update_routing_decision_outcome(id,outcome),
get_model_task_score(model,task)->Option<ModelTaskScore>, upsert_model_task_score_signal(model,task,signal:QualitySignal)}`。
`upsert_model_task_score_signal` 内部做 EWMA（见 P1.c 公式）。

#### P0.d　替换决策函数（router.rs 核心）

删 `TASK_MODEL_MAP`（14）、`INFERENCE_MODELS`（22）、`task_model_preference`（119）、`select_model`（131）、
`default_model_for_task`（74）。新增：

```rust
// router.rs
pub struct Candidate { pub model: Model, pub score: f64, pub reason: String }

pub struct PlanInput<'a> {
    pub task_type: &'a str,
    pub band: &'a str,            // easy/medium/hard（来自投影层）
    pub policy: &'a RoutingPolicy,
    pub models: &'a [Model],
    pub scores: &'a ModelTaskScoreMap, // (model,task)->ewma+avg_cost+avg_latency
    pub est_in_tokens: u32,
    pub est_out_tokens: u32,
    pub budget_tier: BudgetTier,  // normal/throttle/tight/protect（来自 P5）
}

pub enum PlanError { NoCandidate(String), AllDown }

pub fn plan(input: &PlanInput) -> Result<PlanOutcome, PlanError> {
    let tier_req = band_to_min_tier(input.band); // easy→1, medium→2, hard→3
    let mut cands: Vec<Candidate> = Vec::new();
    for m in input.models {
        // ── 硬门槛 gate ──
        if m.is_active != 1 { continue; }
        if m.capability_tier < tier_req.max(input.policy.min_capability_tier) { continue; }
        if m.context_window < (input.est_in_tokens + input.est_out_tokens) as i64 { continue; }
        if m.health_state == "down" { continue; }
        // tools/vision 门槛按输入需求（PlanInput 可加 needs_tools/needs_vision）
        let est_cost = estimate_cost(m, input.est_in_tokens, input.est_out_tokens);
        if let Some(maxc) = input.policy.max_cost_per_request { if est_cost > maxc { continue; } }
        // 预算保护档：tight/protect 只留 is_local 或 cost<阈值
        if matches!(input.budget_tier, BudgetTier::Protect) && m.is_local != 1 && est_cost > 0.0 { continue; }
        // ── 软评分 ──
        let q = quality(m, input.task_type, input.scores); // 融合 quality_score + ewma
        let c = normalize_cost(est_cost, input.models, input.task_type, input.scores);
        let l = normalize_latency(m, input.task_type, input.scores);
        let mut s = input.policy.quality_weight*q - input.policy.cost_weight*c
                  - input.policy.latency_weight*l + 0.05*m.priority as f64;
        if m.needs_calibration == 1 && tier_req > 1 { s -= 0.3; } // 保守期不接复杂任务
        cands.push(Candidate{ model:m.clone(), score:s, reason: format!("q={q:.2} c={c:.2} l={l:.2}") });
    }
    if cands.is_empty() { return Err(PlanError::NoCandidate(
        format!("无可用模型满足 task={} band={} 的约束", input.task_type, input.band))); }
    cands.sort_by(|a,b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    let depth = input.policy.fallback_depth.max(1) as usize;
    let primary = cands[0].model.name.clone();
    let chain: Vec<String> = cands.iter().take(depth).map(|c| c.model.name.clone()).collect();
    Ok(PlanOutcome{ primary, fallback_chain: chain, candidates: cands })
}

/// 阶梯价：按 est_in_tokens 选 max_input >= tokens 的最低档返回 in/out 单价。
fn unit_price(m: &Model, est_in_tokens: u32) -> (f64, f64) {
    if let Some(tiers) = parse_price_tiers(&m.price_tiers_json) {
        for t in tiers { if t.max_input as u32 >= est_in_tokens { return (t.in_cost, t.out_cost); } }
    }
    (m.input_cost_per_token, m.output_cost_per_token)
}
fn estimate_cost(m: &Model, est_in: u32, est_out: u32) -> f64 {
    let (ic, oc) = unit_price(m, est_in);
    ic*est_in as f64 + oc*est_out as f64
}
```

`server.rs:chat_stream`（369-380）的「伪造空 spec」删除，改用 `plan()` 结果；
`stream` 改读 `m.supports_stream`。`route()`（router.rs:178）改造为返回 `PlanOutcome` 摘要（
含 primary/task_type/band/method/stream），`RoutingDecision`（models.rs:146）扩字段或新增 `RoutingOutcome`。

**单测**（`router.rs` `#[cfg(test)]`）：空候选集报错；单模型；删主选后自动改选；阶梯价交叉点
（est_in=40000 时 qwen3-max 升档 vs 3.6-plus）；保守期模型被复杂任务剔除；预算 protect 档只剩 local。

#### P0.e　增删模型自动打标（五级兜底）

```rust
// metadata.rs (新模块)
pub fn resolve_and_fill(m: &mut Model) {
    // 顺序：命即填并设 price_source，后级不覆盖高级
    // 1. litellm 打包表（运行时 import litellm.model_cost，字段映射）
    // 2. litellm 远端刷新结果（若 P2 job 已落库，读 price_source=='litellm_remote' 的值）
    // 3. models.dev api.json（镜像拉取，补 context_window + 国际模型；实测无 dashscope）
    // 4. overlay: data/model_catalog.json[{provider}/{name}] → cost/tiers/quality/context/supports
    // 5. 启发式（最后兜底）
    fill_from_litellm_packaged(m).or_else(|| fill_from_remote(m))
        .or_else(|| fill_from_models_dev(m)).or_else(|| fill_from_overlay(m))
        .or_else(|| fill_heuristic(m));
}
fn fill_heuristic(m: &mut Model) -> bool {
    let n = m.name.to_lowercase();
    let tier = if regex!("flash|mini|turbo|small|lite|air|8b|4b|1\\.5b").is_match(&n) {1}
        else if regex!("max|opus|ultra|pro|reasoner|thinking|r1|o1|o3").is_match(&n) {3} else {2};
    m.capability_tier = tier;
    if m.provider=="ollama" || m.api_base.contains("127.0.0.1") || m.api_base.contains("localhost") {
        m.is_local=1; m.input_cost_per_token=0.0; m.output_cost_per_token=0.0;
    }
    m.needs_calibration=1; m.price_source="heuristic"; true
}
```
触发点：`db::insert_model` 成功后调 `resolve_and_fill` + `db::update_model` 回填（用内部直写绕开白名单）。
保守期：`needs_calibration=1` 且 `sample_count<20` 的模型在 `plan()` 门槛 `tier_req>1` 时扣分（见 P0.d），
真实结果经 `upsert_model_task_score_signal` 回填 `ewma_quality`，`sample_count>=20` 后 `db` 置 `needs_calibration=0`。
删除路径：保持软删（`db::delete_model`），若被删者是某 `pinned_model` 则 `UPDATE routing_policy SET pinned_model=NULL` + UI 提示。

#### P0.f　消除双真源（Python 契约细化）

> **✅ 已落地 2026-08-26（50ec431）**。下方契约按此实现：`router.rs` 新增 `plan_decision()` 按固定
> 编排角色（general/decompose/aggregate）复用 `plan()` 评分；`server.rs` 构造 `assignments`，
> `ai_client.rs` 写入请求体；`ai_service.py` 删 `TASK_MODEL_PREFERENCE`/`DECOMPOSER_PREFERENCE`/
> `_select_model`，新增 `_assigned_model()`（优先 `assignments[role]`，缺则回落 `models[0]`），
> 摘要/轻量/分解器/汇总兜底全部改读 assignments。**「无字面量」验收 = `ai_service.py` 不再含任何
> 模型名常量/硬编码偏好表，模型名只来自 Rust 下发的 `assignments` 或 `models` 全池**（子任务级
> 分配属 P4.a，当前统一复用 general 决策）。

`ai_service.py` 删 `TASK_MODEL_PREFERENCE`（888）、`DECOMPOSER_PREFERENCE`（886/1453）、`_select_model`（966）。
**关键约束**：Python 的 `_make_summary`（1295，摘要生成遍历 `DECOMPOSER_PREFERENCE`）与复杂路径分解器选择（1451）
仍需「全模型池」做兜底/摘要。故 Rust→Python 契约为**双字段**：

```jsonc
// /v1/orchestrate/stream 请求体（ai_client::orchestrate_stream body）
{
  "query": "...", "history": [...], "sr_domain": "...",
  "models": [<全 active ModelSpec 池>],          // 供 Python summary/兜底
  "assignments": {                                // Rust 决策结果，Python 优先用
     "decompose":  <ModelSpec>,                   // 分解器（Rust 按 task_type="decompose" plan()）
     "aggregate":  <ModelSpec>,                   // 汇总器（task_type="aggregate"）
     "subtasks":   [{"id":1,"model":<ModelSpec>,"task_type":"coding"}, ...]  // P4.a 子任务级分配
  },
  "cache_dir": "...", "similarity_threshold": 0.80, ...
}
```
Python 端 `_select_model`/分解器选择改为「优先 `assignments`，缺则回落 `models[0]`」；
`_make_summary` 优先用 `assignments.decompose`。轻量路径（1382）`_select_model("general")` 改读 `assignments` 里
Rust 已选的模型（或 `models[0]` 兜底）。这样**两份真源合一到 Rust**，Python 无任何模型名字面量。

#### P0.g　信号层正规化（signals.rs 新模块）

```rust
// lloom-core/src/signals.rs
pub struct SignalSet { pub task_type: Option<String>, pub difficulty: f64, pub band: String,
    pub needs_tools: bool, pub needs_vision: bool, pub context_tokens: u32, pub budget_tier: BudgetTier,
    pub reasons: Vec<(&'static str, f64)> }  // (signal_name, contribution)

pub fn extract(user_text: &str, history: &[Value], conv_id: Option<&str>) -> SignalSet {
    // 启发式快路径（纯 Rust，<10ms）：
    //   keyword/structure: router::rule_classify + ai_service::_is_complex/_is_comparison 上移版
    //   context: tiktoken count（项目已有 tiktoken_cache）
    //   budget_tier: 读 settings budget_ratio（P5）
    // 学习型慢路径（按需）：
    //   embedding: 复用语义缓存已算向量（若缓存未初始化则跳过，不阻塞）
    //   classifier: 仅启发式 task_type=None 且 difficulty 处于灰区时触发 ai_client::classify
    // difficulty = weighted_sum(structure, complexity, context, embedding_conf?) → band = threshold_bands
    ...
}
```
阈值配置走 `settings` KV：`signal.reask.sim_threshold=0.85`、`signal.reask.interval_sec=300`、
`signal.classifier.confidence_floor=0.5`、`signal.difficulty.weights=structure:0.3,complexity:0.3,context:0.2,embedding:0.2`、
`signal.band.easy=0.33`、`signal.band.medium=0.66`。WebUI「路由策略」页可读写。
本阶段不追求信号齐备，只求命名、可配置、有单测（含 reask 判定、band 边界）。

---

### 阶段 P1：用量闭环 + 成本×成效

#### P1.a　修用量落库（v3 范围收窄）

`usage_records` 加列（幂等）：
```sql
ALTER TABLE usage_records ADD COLUMN latency_ms REAL;
ALTER TABLE usage_records ADD COLUMN request_id TEXT;
CREATE INDEX IF NOT EXISTS idx_usage_req ON usage_records(request_id);
```
`db::insert_usage`（248）签名扩参 `latency_ms: Option<f64>, request_id: Option<&str>`（保持向后兼容：旧调用用 None）。

- **chat_stream**（server.rs:391-404）：`ai_client::chat` 成功后立即
  `db::insert_usage(&res.model, "default", res.input_tokens, res.output_tokens, res.cost,
  Some(&routing.task_type), false, Some(latency), Some(&req_id))`。失败**不写 `usage_records`**：失败/重试/升级 attempt 记 `routing_decisions.outcome`（P0.c 已定义 `success/fail/escalated/cache_hit`，`request_id` 串联 cascade 的多次 attempt）。

**`usage_records` 语义边界**（v3 厘清——避免成本账本混入诊断信号）：
- **只记最终成功**（含 cache 命中：`cache_hit=1`、`cost=0` 但属一次省钱服务，该计入 `request_count`）。
- **预算口径不受影响**：`check_budget`/`get_total_spend`（db.rs:322）用 `SUM(cost)`，失败 `cost=0` → 记不记都不扣预算；受影响的只是 `request_count` 分母类展示指标（命中率、平均成本/请求），故不能让失败污染。
- **EWMA 失败信号**：`fail_count` 从 `routing_decisions WHERE outcome='fail' GROUP BY model` 聚合写 `model_task_score`（见 P1.c），与成本账本解耦。
- **不加 `outcome` 列到 `usage_records`**：纯成本账本无需状态枚举；状态归 `routing_decisions`。
- **orchestrate_stream**：v3 原写「用 `routing.task_type`」**前提已过时**——编排路径 per-role plan（P0.f/P4.a：decompose / 各子任务 / aggregate 各自独立 `plan()`），**无单一 task_type**。改为**按事件携带的 role 细分**：
  - 落库点从 `result` 事件（server.rs:486 一条汇总）**下沉到每个 `task_done` 事件**：每条 usage 的 `task_type` = 该 role 的 task_type（`decompose` / 子任务自身类型 / `aggregate`）；`model`/`cost`/`tokens` 取自 `task_done` 携带值，取不到才回落 `"unknown"`；补 `latency`（`task_done.duration`）与 `request_id`。
  - `result` 事件不再重复落 usage（避免与 `task_done` 重复计费），只保留 `insert_cache_calibration`（cache 校准）+ 前端汇总展示。
  - **Python 端需补字段**：`task_done` 加 `task_type`（从分解结果透传；轻量路径标 `general`）；`aggregate` 调用补发 `task_done`（或 `aggregate_done`）携带 `task_type="aggregate"` + 其 cost/tokens，**否则聚合成本漏记**（落地时需确认复杂路径 aggregate 是否已发 task_done）。
  - 一次编排产生多条 usage（decompose 1 + 子任务 N + aggregate 1）= 真实 LLM 调用数，`request_count` 相应增加——与「失败不记 usage」不冲突（失败记 `routing_decisions`，成功调用才记 usage）。
- 清 3 条旧脏数据（`DELETE FROM usage_records WHERE model_name='default' AND cost=0`，迁移脚本里做，先备份）。

#### P1.b　推荐分配（保留）

| 任务类型 | 建议主选 | 混合成本 | 降级链 | 相对现状 |
|---|---|---|---|---|
| simple_qa | qwen2.5-local | 0 | qwen-plus→qwen3.6-flash | 兜底由 flash 改 plus |
| general | qwen-plus | 4.8 | qwen3.6-flash→local | 不变 |
| decompose/classify | **qwen-plus** | 4.8 | qwen3.6-flash | **省约 69%** |
| coding | deepseek-v3 | 17.0 | qwen-plus→qwen3-max | 升级位 plus→max |
| math_logic | deepseek-v3 | 17.0 | qwen3-max→qwen-plus | 改 max |
| complex_reasoning(≤32K) | **qwen3-max** | 22.5 | deepseek-v3→qwen-plus | 主选 3.6-plus→max |
| complex_reasoning(>32K) | qwen3.6-plus | 26.0 | qwen3-max | 阶梯价交叉后切换 |

最后一行正是 `price_tiers_json` 的价值：同一任务不同上下文长度最优模型会切换，静态偏好表表达不了。

#### P1.c　成效分：冷启动 + 在线 EWMA

- **冷启动 `quality_score`**：overlay 里按 task_type 存榜单折算分（公开评测→0..1），**按任务分别给分**
  （coding 0.8、math 0.5 是正常态）= RouterBench「模型×任务×对错×成本」本地化。
- **在线 `ewma_quality`**，更新公式（`db::upsert_model_task_score_signal`）：
  `ewma ← α·σ + (1-α)·ewma`，`α=0.15`（settings 可调）。信号值 σ：

| 信号 | σ 值 | 来源 |
|---|---|---|
| 正常完成 | +0.7 | `result.ok=true` |
| 子任务失败 | −0.5 | `task_done.error` 非空 |
| cascade 升级 | −0.4 | `escalation_count+1` |
| 重生成/切模型重问 | −0.6 | 同 conv 短间隔新请求 + 不同模型 |
| reask 隐式不满 | −0.4 | `signal.reask` 命中（同对话相似度>0.85 且间隔<300s） |
| 点赞 | +1.0 | `cache_feedback` label=1 |
| 点踩 | −1.0 | label=0 |
| 结构化解析失败 | −0.3 | JSON schema 校验失败 |

`sample_count` 每次 ±1；达 20 解除保守期。

#### P1.d　影子评测 + AIQ

- `POST /api/routing/shadow`：采样（默认 10%）双跑「路由选择」与「强模型基线」，两份落 `routing_calibration`，
  只返回路由结果。开关存 `settings.routing.shadow_ratio`。
- **AIQ 重放**：离线脚本读 `usage_records`+`routing_decisions`，重放对比「全弱基线」/「全强基线」/「当前策略」
  三条成本—质量曲线，输出 AIQ = (策略质量 − 全弱质量)/(全强质量 − 全弱质量) 的预算积分。
  这是调 `routing_policy` 权重的依据，避免凭感觉。

---

### 阶段 P2：定价表系统与动态更新

> **代码级细化见 [`PRICING-PLAN.md`](./PRICING-PLAN.md)**（PriceSpec 分项计价 × 时段 × 阶梯 × 来源、
> `pricing.rs` 单一计价真源、幂等迁移 SQL、校准 tokio job、探针预算状态机、WebUI REST 端点、单测矩阵）。
> 本节只列**与路由决策的衔接点**，不重复实现细节。

前置：P0.a/b 量纲修正 + `price_source` 列。

**与路由的衔接（决策层如何用定价）**：
- `plan()` 的 `estimate_cost()` 调 `pricing::effective_input_cost(model, est_in)` —— 后者按
  `PriceSpec` 算「缓存读折扣后的有效输入单价 + 阶梯价 + 峰谷系数」，使路由选型直接体现缓存命中率
  与时段红利（PRICING-PLAN §路由衔接）。
- `price_source` 优先级 `manual > overlay > litellm_remote > litellm_packaged > heuristic` 仍由本计划 P2 维护，
  PRICING-PLAN 的 `PriceSpec.source` 与之一致（同字段语义）。
- 刷新 job（24h tokio，走 jsdelivr/ghproxy 镜像）+ overlay（百炼唯一覆盖层）+ WebUI 徽标/stale/采纳建议价，
  三者细节见 PRICING-PLAN §六/§九，本计划不重述。
- 对账（OpenRouter `usage.cost`）与 models.dev 无 DashScope 的结论见 §3.4。

#### P2.a　刷新 job（tokio 后台）

```rust
// lloom-server 启动时 spawn
tokio::spawn(async {
    let mut int = tokio::time::interval(Duration::from_secs(86400)); // 24h
    loop {
        int.tick().await;
        if let Err(e) = pricing::refresh().await {
            tracing::warn!("pricing refresh failed: {e}"); // 静默，不刷屏
        }
    }
});
// pricing::refresh
const RAW: &str = "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
const JSDELIVR: &str = "https://cdn.jsdelivr.net/gh/BerriAI/litellm@main/model_prices_and_context_window.json";
async fn refresh() -> Result<()> {
    let db = match fetch(RAW).or_else(|| fetch(JSDELIVR)).or_else(|| fetch_with_ghproxy(RAW)).await? {
        .. }; // 复用 GH_MIRROR
    for m in db::list_models(false)? {
        if matches!(m.price_source.as_str(), "manual"|"overlay") { continue; } // 永不覆盖
        if let Some(new) = db.get(&m.litellm_model) {
            let drift = rel_drift(m.input_cost_per_token, new.input_cost_per_token);
            if drift > 0.20 {
                if m.price_source == "manual" { db::set_price_stale(&m.name, 1)?; } // 黄点提示
                else { db::apply_price(&m.name, new, "litellm_remote")?; } // 直接更新
            }
        }
    }
    db::set_setting("pricing.last_refresh", &now_iso())?;
    Ok(())
}
```
失败计数达 7 天才 `tracing::error`（网络受限是常态）。
`POST /api/pricing/refresh`（手动触发）、`GET /api/pricing/status`（last_refresh + stale 数）。

#### P2.b　overlay 维护

`data/model_catalog.json` 按 `{provider}/{model}` 键存：单价（USD/token）、阶梯价、context、supports_*、
按 task_type 的冷启动 quality_score。百炼模型在此。版本随仓库；刷新 job 不碰它。

#### P2.c　WebUI 字段

「模型与定价」页每行加：`price_source` 徽标（manual/overlay/litellm_remote/litellm_packaged/heuristic）、
`price_updated_at`、`price_stale` 黄点、「采纳建议价」按钮（`POST /api/models/{name}` 设 price_source=manual）。

---

### 阶段 P3：健康感知与故障转移

- 状态机：`unknown → up`（首次成功）/ `up → degraded`（滑窗 5 次内失败≥2）/ `degraded → down`（连续 3 失败）/
  `down → up`（主动探测成功）。`health_checked_at` 每次更新。
- 被动：`ai_client::chat` 失败/超时/429 在 `server.rs` 记 `db::record_health(model, outcome)`；滑窗存 `settings` 或内存 LRU。
- 主动探测：`down` 每 60s（`settings.health.probe_sec`）发最小请求（1 token）试探。
- 故障转移：`plan()` 已产 fallback 链；`ai_client::chat` 失败时 server 按链顺序重试，每次 `db::inc_escalation_count`。
- 熔断：单模型连续失败达阈值（默认 5）临时剔除候选集（`health_state='down'`）。
- routing overhead：`routing_decisions.routing_ms` 聚合暴露 `GET /api/routing/overhead`；
  快路径 >10ms / 全路径 >100ms 视为实现 bug（断言）。

---

### 阶段 P4：编排智能升级（多模型协作核心）

前置：P0.f（Rust 统一决策）、P1.a（子任务记账）。

#### P4.0　架构选型：轻量回调（A）vs 双手协商重构（B）

| 维度 | A 轻量回调（单次 SSE） | B 双手协商（拆 decompose 独立端点） |
|---|---|---|
| 决策真源 | Rust（`plan-subtask` 端点跑 `plan()`） | Rust（收 decompose 后 plan 每子任务下发） |
| 执行期降级主导 | Python（litellm 在此，降级自然在此） | Python（降级仍必在 Python）→ Rust 主导只在初始分配 |
| Python 重构 | 局部（generator 内加回调 + 降级循环） | 大（拆 decompose/execute 两端点） |
| Rust 角色 | 透传 SSE + 1 个无状态 plan 端点 | 编排驱动者（有状态，持 decompose 结果 midway） |
| 客户端 SSE | 单次不中断 | decompose 段同步无流 → execute 段才流 |
| 依赖 P0.g | 否（用 Python 既有 `_is_complex`） | 是（Rust 入口须复刻 `_is_complex` 判走哪路） |
| 每子任务开销 | +1 次 localhost 回调（~1ms） | decompose→execute 多一次完整往返 + 序列化 |
| 预算时点 | 按需 plan，每子任务拿最新预算水位（更准） | 初始一次性 plan，用请求初预算 |

**选 A**。理由：
1. **降级重试必在 Python**（litellm 调用在那）——B 的「Rust 主动 plan」只体现在初始分配，执行期降级仍回 Python，却付了拆两段 + Rust 有状态的代价；A 用回调拿 plan、Python 内降级，执行期主导统一在 Python，决策真源仍在 Rust。
2. **重构小**：Python generator 结构不变；Rust 加 1 个无状态端点。
3. **不阻塞 P0.g**：P4 可先于 signals 上移落地；B 强依赖 Rust 复刻 `_is_complex`。
4. **预算更准**：每子任务时点回调拿最新预算水位（P5 动态档），而非请求初一次性。

A 的代价：新增 Python→Rust 调用方向（Python 需知 Rust URL，经 `OrchestrateRequest.rust_base_url` 或 env `LLOOM_ROUTER_URL` 传入）；每子任务一次同步回调（localhost ~1ms，可忽略）。
B 的适用场景（未来）：若要把编排状态/可观测完全收归 Rust，或支持编排中途暂停/恢复/人工介入——届时 Rust 主导更合适，当前无此需求。

#### P4.a　子任务级评分分配（A 架构：Python 回调 Rust plan）
分解产出子任务带 `task_type`（DECOMPOSE_SYSTEM_PROMPT 已要求标注，ai_service.py:901）。
Python 在执行每个子任务前，回调 Rust 新增端点拿 plan：

- Rust 新增 `POST /api/routing/plan-subtask`：入 `{task_type, est_in_tokens, est_out_tokens, budget_tier, needs_tools, needs_vision, request_id}`，出 `{primary: ModelSpec, fallback_chain: [ModelSpec], escalation_enabled, tier_req}`。纯 `plan()` + 查 `model_task_score`，**无状态**。
- Python `orchestrate_stream` generator 内：子任务执行前 `httpx.post(rust_base_url+"/api/routing/plan-subtask", ...)`，用 `primary` 执行，失败按 `fallback_chain` 重试（见 P4.b）。
- 轻量路径（不分解）同样回调 `plan-subtask(task_type="general")` 拿模型，统一入口。
- `assignments`（P0.f 请求体）此时只承载 `decompose`/`aggregate` 的 spec（请求时可算的），子任务 plan 走回调——与 P0.f 双字段契约一致（`models` 全池仍下发，供 summary/兜底）。

#### P4.b　Stage 路由降级（Python 内，Switchyard 核心借鉴）
```python
# Python orchestrate_stream generator 内，per subtask
for sub in subtasks:
    tier_bump = 0
    while True:                                   # 升级重试循环
        plan = httpx.post(rust+"/api/routing/plan-subtask",
                          {task_type: sub.task_type, est_in: est(sub),
                           budget_tier, tier_bump, request_id})
        result, ok = run_with_fallback(plan, sub) # primary → fallback_chain 逐个试
        if ok and quality_signal_ok(result):      # 零成本信号：解析成功/JSON schema/长度合理
            emit task_done(ok, model=result.model, retry_count=result.retries,
                           escalated_from=result.escalated_from, task_type=sub.task_type)
            break
        else:
            if plan.escalation_enabled and tier_bump < MAX_TIER_BUMP:
                tier_bump += 1; continue          # 整体升一档再 plan
            consecutive_fail += 1
            emit task_done(error, 如实); break     # 不美化
# run_with_fallback: 每次失败 attempt → routing_decisions.outcome='fail'（不进 usage）；
#                    成功 attempt → task_done 触发 usage 落库（P1.a，task_type 细分）
```
`result.escalated_from` = 实际服务模型 ≠ primary 时记 primary 名；`tier_bumped` = tier_bump>0。
`result` 事件补 `escalations`（总升级次数）。Rust 在 SSE 透传时把这些写 `routing_decisions.outcome`。
无依赖子任务可并行（与已知待办 O6 合并）——并行时各子任务独立回调 plan-subtask，互不阻塞。

#### P4.c　Escalation 模式
`routing_policy.escalation_enabled=1` 的任务启用 §4.5：先跑轻量档候选，零成本信号判质量不达标才升级强档
（裁判优先 `JSON schema 校验`/`解析成功`，不够才 LLM judge）。默认只对 `decompose`/`classify`/`simple_qa` 开。

#### P4.d　汇总也走评分
聚合模型按 `task_type="aggregate"` 走 `plan()`（种子策略已预填）。长输入汇总受 `context_window` 门槛约束
（子任务结果拼接可能超小窗口 → 自动选大窗口模型或分批汇总）。

#### SSE 事件契约（Python 侧补字段，Rust 透传）
`task_done` 加 `escalated_from`（升级前的模型名，无则省）、`retry_count`；
`result` 加 `escalations`（总升级次数）、`tier_bumped`（是否触发整体升档）。
Rust 在 SSE 处理（server.rs:462-501）把这些写入 `routing_decisions.outcome`。

---

### 阶段 P5：预算驱动动态调整

#### P5.a　预算进入决策链
`budget_tier` 由 `budget_ratio(r)` 决定，注入 `PlanInput`：

| 剩余预算 r | tier | 行为 |
|---|---|---|
| r>50% | normal | 原权重 |
| 20<r≤50% | throttle | cost_weight×1.5 |
| 5<r≤20% | tight | cost_weight×2.5；复杂任务降一档；强制开语义缓存 |
| r≤5% | protect | 仅 is_local 或 cost=0；超限任务明确提示 |

降级非硬拒：耗尽推本地 Ollama。`qwen2.5-local` 始终留候选集。
接入点：`chat_stream`/`orchestrate_stream` 在 `plan()` 前调 `db::check_budget(...)`（P1-6 那个现成函数）算 r。

#### P5.b　预算模型扩展
```sql
ALTER TABLE budgets ADD COLUMN scope_task_type TEXT;
ALTER TABLE budgets ADD COLUMN soft_limit_ratio REAL DEFAULT 0.8;
ALTER TABLE budgets ADD COLUMN action_on_exceed TEXT DEFAULT 'degrade';
```
`scope` 扩 `user/model/task_type/global`（`scope_id` 复用）。

#### P5.c　预估成本前置校验
tiktoken 算输入（`tiktoken_cache/` 已有），输出用该 task_type 历史 P50（`model_task_score.avg_*` 估），
得 `est_cost` 参与 `max_cost_per_request` 门槛与 tier 判断。无价格数据时退化为 token 硬顶（token-only budget）。

---

## 六、落地顺序与验收（v3）

| 序 | 内容 | 验收标准 | 状态 |
|---|---|---|---|
| 1 | P1.a 修用量落库 | chat 一次对话后 `usage_records` 有正确 model/tokens/cost/task_type/latency/request_id；编排按角色(task_type)逐 LLM 动作建账，model 兜底 unknown | ✅ 2026-08-27（cab03c8）`usage_records` 加 `latency_ms`/`request_id`（SCHEMA+迁移幂等）并建 `idx_usage_req`；`insert_usage` 扩参（探针向后兼容）；chat 落耗时+请求号（失败不写 usage，归 `routing_decisions.outcome`）；编排改逐 `task_done` 按 role 细分记账（轻量=general/子任务=自身 task_type/汇总=aggregate）；迁移清 3 条 `default`+cost=0 旧脏数据；52 单测 + P1.a 持久化冒烟过 |
| 2 | P0.a 修量纲 + 写入断言 | qwen 单价降为 1/10；越界单价写入被 422 拒 | ✅ 2026-08-26（09480fa）量纲迁移此前已落；models 表 insert/update 断言 + 单测 |
| 3 | P0.b/c 建表迁移（幂等） | 重跑不报错、旧数据不丢；迁移前已备份 `data/lloom.db` | ✅ 2026-08-26（8dddc59）备份 `lloom.db.pre-routing-migration.bak`；回填经 settings 标记只跑一次 |
| 4 | P0.d 评分 plan() + router 单测 | 删任一模型自动改选；不再返回未注册名；空候选集明确报错；阶梯价交叉单测过 | ✅ 2026-08-26（8dddc59）11 个单测 + 冒烟；阶梯价交叉单测随 P2 tiered 数据补（现 spec 平价）|
| 5 | P0.e 五级打标 | 新增未知模型→元数据自动填充、标 needs_calibration、不接复杂任务 | ✅ 2026-08-26（d6912b9）新增 `metadata.rs::resolve_and_fill`（overlay 显式 > 启发式，不覆盖用户/overlay 显式值），`db::insert_model` 落库前自动打标（能力档/上下文/本地端点置零成本/needs_calibration）；6 单测 + 注册冒烟过 |
| 6 | P0.f+g 消除 Python 真源 + 信号正规化 | `ai_service.py` 无模型名字面量；signals 可配置有单测 | ✅ 2026-08-26 P0.f（50ec431）删 `TASK_MODEL_PREFERENCE`/`DECOMPOSER_PREFERENCE`/`_select_model`，Python 读 `assignments` 兜底 `models[0]`；P0.g（d6912b9）`signals.rs` 补 `SignalSet`/`extract`/band/reask/LLM 判定，阈值走 settings KV；7 单测过 |
| 7 | P1.b/c 成效分 + 推荐分配 | 影子评测下内部分解路径成本降 ≥60%，质量无显著回退 | ✅ 2026-08-27（P1.b/c）P1.b `migrate_db` 按 §P1.b 表为新库预置 `pinned_model` 推荐主选（INSERT OR IGNORE）+ 既有库仅回填 NULL（settings 标记 `migration_policy_v1_p1b`，绝不覆盖用户钦定）；P1.c `metadata.rs::cold_start_quality` overlay 按 task_type 榜单折算分 + `db::upsert_model_task_score_signal` 线上 EWMA（α 读 `signal.ewma_alpha` 默认 0.15，输入 σ 不 clamp、结果 clamp）并按信号自增 success/fail/escalation、`sample_count≥20` 解除保守期；server.rs chat 落 Success、orchestrate 按 task_done error 落 Success/SubtaskFail（model/role≠unknown 才打点）；≥60% 指标待影子真实样本验收 |
| 8 | P2 定价刷新 + WebUI 徽标 | 手动刷新更新非 manual 来源；断网静默保持本地值 | ✅ 2026-08-27（P2）P2.a `server.rs` `pricing_refresh_loop` 24h 后台 job（jsdelivr 主源 + ghproxy 回退，断网失败静默保留本地值）+ `POST /api/pricing/refresh`（手动触发）、`POST /api/pricing/specs/{provider}/{model}/accept`（采纳转 manual，此后不被覆盖）；`pricing.rs::parse_remote_prices` 纯函数解析（跳过非 provider/model 键、负价），`db::refresh_price_spec`（COALESCE 保 cache_read，不覆盖 manual）；P2.c WebUI 新增 **PricingPage**（`price_source` 徽标 manual/overlay/litellm_remote…、`price_updated_at`、`price_stale` 黄点、手工改价强制转 manual、采纳建议价）+ 用量页「缓存为您节省 ¥X」卡片与「缓存节省」列（`cache_saved_cost` 聚合，CNY 展示）；PR-6 定价页 + PR-7 探针视图（`GET /api/probe/stats`）一并落地；57 单测 + tsc + vite build 全过 |
| 9 | P3 健康 + fallback + overhead | 停 Ollama/错 key→自动降级；routing_ms 快路径 <10ms | ✅ 2026-08-27（P3）新增 `health.rs` 状态机：滑窗（默认 5 内 ≥2 失败 degraded）/连续 ≥3 失败 down/成功永远向 up 收敛/熔断连续 ≥5 强制 down（阈值全走 settings `health.*` KV），状态变化才 `set_model_health` 落库 + `health_checked_at`；chat 路径 `chat_with_failover` 按 `primary + fallback_chain` 顺序重试（失败打健康哨点、跳升记 Escalation 成效信号、成功按实际响应模型计价），orchestrate 按 `task_done` 成功/失败喂哨点；后台 `health_probe_loop` 每 `health.probe_sec`（默认 60s）对 down/degraded 模型发最小请求主动探测恢复；`GET /api/routing/overhead`（count/avg/p95/max/slow，>100ms 记慢，`routing_decisions.routing_ms` 真源）；65 单测全绿（+7 health 状态机 +1 overhead 聚合）|
| 10 | P4 编排升级 | 子任务失败自动降级重试成功；escalation 任务成本再降 ≥30%（相对序 7） | ✅ 2026-08-27（P4）P4.0 选 A 轻量回调：Rust 新增 `POST /api/routing/plan-subtask`（无状态 `plan_for_task(task_type, est_in, est_out, budget_tier)` 出 primary + fallback 链 + escalation_enabled）`router.rs::plan_for_task` 为 `plan()` 参数化封装，`plan_decision` 复用其默认参；Python `orchestrate_stream` 每子任务按其 `task_type` 回调拿 plan，primary→fallback_chain 逐个降级重试（记 `retry_count`），失败绝不美化；质量信号（非空/长度/失败哨兵）不达标 + `escalation_enabled` → 升档强模型重试一次（`_strongest_model` 用单价 in+out 作强档代理，「先轻量档→零成本判质→升强档」，无更高价则不开）；SSE 契约：`task_done` 透传 `escalated_from`/`retry_count`/`tier_bumped`，`result` 透传 `escalations`/`tier_bumped`；Rust 侧对 `escalated_from` 记 Escalation 成效信号（P3 同语义），final 模型记 Success；`decompose`/`simple_qa` routing_policy 种子 `escalation_enabled=1`；P4.d 汇总与轻量路径均走 Rust `plan_decision(aggregate/general)`（assignments 常已含，P4.a 统一入口可选）。顺带修复两处既有 bug（见 v8 注记）：① SCHEMA `idx_usage_req` 引用仅靠 migrate ALTER 才加的 `request_id` → 旧库升级路径断裂，改为 migrate_db 内 ALTER 后幂等建索引（真实旧库冒烟验证）；② 升档助手曾依赖 Python `ModelSpec` 不存在的 `capability_tier`/`quality_score` → 运行时崩溃，改单价代理。`cargo build` 无警告、全量测试 65 全绿；plan-subtask 冒烟 simple_qa→qwen2.5-local/fallback[deepseek-v3,qwen-plus]、coding→deepseek-v3/fallback[qwen3-max,qwen3.6-plus]、aggregate 走 plan 均正确；≥30% 成本指标待影子真实样本验收 |
| 11 | P5 预算联动 | 预算近耗尽→逐档降级至只走本地 | ⏳ 待办 |
| 12 | P1.d AIQ 重放 | 离线重放输出成本—质量曲线；调参有数据依据 | ✅ 2026-08-27（P1.d）`POST/GET /api/routing/shadow` 采样双跑「路由选择 × 旗舰基线」（基线用 settings `routing.shadow_baseline` 钦定否则取能力档最高，采样率 `routing.shadow_ratio` 默认 0.10 可零成本关），FNV-1a 查询哈希防重，成本走 `priced_usage` 真源、结果落 `routing_calibration` 只导路由结果；`scripts/aiq_replay.py` 离线重放对比全弱基线/当前策略/全强基线三条成本—质量线，输出 AIQ（RouterBench 预算积分）与相对全强成本节省、质量缺失时从 `models`/`model_task_score` 回填并写库；**已在 chat/orchestrate 请求热路径按 `shadow_ratio` 概率接入 `maybe_shadow_sample` 后台自动采样**（抽取复用 `run_shadow_pair`，tokio spawn 不阻塞响应/不改返回，`ratio=0` 零成本关）；冒烟 3 样本出 AIQ=0 且 95% 节省+调参建议 |

**测试基建**：`router.rs` 已有 11 个单测（空候选/门槛/评分/回填链/pin/覆盖/band，2026-08-26）；`signals.rs`/`metadata.rs` 纯函数单测已补（2026-08-26，P0.g 7 个 + P0.e 6 个）。
覆盖：空候选集、单模型、删主选后降级、阶梯价交叉点、预算各档、band 边界、reask 判定、保守期解除、
健康状态机迁移、price drift 阈值、EWMA 信号累计 + 保守期解除、迁移幂等修 scale。测试共 **65** 项全绿（2026-08-27，P3 落地后）。
> **v5 注记（2026-08-26，commit d6912b9）**：P0.e/g 落地后 **P0 阶段全部勾选完成**——
> 新增模型由 `metadata::resolve_and_fill` 自动打标入保守期，不再需要人工填元数据；
> 信号层 `extract` 读 settings KV 输出难度带/reask/LLM 判定，路由决策（plan）与信号规范均已就绪。
> **v6 注记（2026-08-27，P1 阶段）**：**P1.a/b/c/d 全部落地并勾选完成**。
> P1.a 用量落库（latency/request_id/逐角色）、P1.b 推荐分配（pinned_model 种子预填）、
> P1.c 成效分（overlay 冷启动 + 在线 EWMA + 信号打点 + 保守期解除）、P1.d 影子评测 + AIQ 重放
> 均已实现；EWMA 输入 σ 不 clamp、结果 clamp 到 [0,1]，杜绝「模型永远学不坏」。54 单测全绿。
> 「影子样本成本降 ≥60%」属需真实数据的验收指标，P4 编排升级前可在现网采集 shadow 样本后复验。
> **v7 注记（2026-08-27，P3 阶段）**：**P3 健康感知 + fallback 故障转移 + overhead 报告已落地并勾选完成**。
> P3 新增 `health.rs` 状态机（滑窗 degraded/连续失败 down/熔断+成功恢复）、`chat_with_failover` 主链+fallback 重试、
> 后台 `health_probe_loop` 主动探测、`/api/routing/overhead` 端点；65 单测全绿。`plan()` 的 `health_state=="down"` 硬门
> 已在 P0.d 产出、P3 补全了状态写入与持久化回路。下一阶段 P4 编排升级（子任务失败降级重试）需等减法合并后启动。
> **v8 注记（2026-08-27，P4 阶段）**：**P4 编排智能升级已落地并勾选完成**（见序 10）。
> A 架构「Python 回调 Rust `plan-subtask`」落地：每子任务按其 `task_type` 独立 plan，primary→fallback_chain 阶段降级，
> 零成本质量信号不达标 + `escalation_enabled` → 升档强模型重试；`task_done`/`result` 按 SSE 契约透传
> `escalated_from`/`retry_count`/`tier_bumped`/`escalations`，Rust 对被跳过的轻量模型记 Escalation 成效信号（P3 同语义）。
> 顺带修复两处既有 bug：① SCHEMA 的 `idx_usage_req` 引用仅靠 migrate ALTER 才加的 `request_id` 列，
> 导致 P1.a 之前创建的旧库升级路径在 SCHEMA 阶段即失败 → 移入 `migrate_db` 在 ALTER 后幂等建索引
> （以真实 pre-P1a 旧库冒烟验证：启动成功、列补齐、索引就位）；② `_strongest_model` 曾依赖 Python `ModelSpec`
> 不存在的 `capability_tier`/`quality_score` → escalation 触发即运行时崩溃 → 改以单价 in+out 作强档代理，
> 本地免费模型天然不成为升档目标、无更高价则不开档。`cargo build` 无警告、65 单测全绿、冒烟过。
> ≥30% 成本指标与 P1 的 ≥60% 同属需影子真实样本的验收，待现网采集后复验。

---

## 七、风险与注意事项（v3）

1. **价格会过期** → DB+overlay+刷新 job+manual 校准入口，代码零价格字面量。litellm 远端 SSL 失败走镜像；
   `qwen3.6-flash` 存 1.2/7.2 与 0.367/2.936 两口径（采信 1.2/7.2，与 DB 反推逐位吻合）。
2. **models.dev 无 DashScope**（2026-08-24 实测）→ 百炼价只能 overlay+人工核对。
3. **Switchyard pre-alpha**（v0.2.0，v1.0 前 API 破坏性变更，Python launcher 已移除）→ 只借鉴设计，
   未来引入走 libsy（库路径）且等 v1.0。
4. **vLLM Semantic Router 是 Envoy ext_proc** → 借分层思想与契约形态，不引入 Envoy。
5. **收益按区间管理**（35–85% 随负载难度），验收用 AIQ 式指标不承诺固定百分比。
6. **级联/升级延迟翻倍** → 只在编排路径（P4）启用；`chat/stream` 保持一次性分类路由。
7. **开放式生成不能靠置信度做级联裁判** → escalation 默认只对结构化可判任务开。
8. **Router-R1 式 RL 是远期可选** → 当前「描述符评分+EWMA+影子评测」已覆盖其核心收益（泛化新模型），
   RL 回路等影子数据千级样本再评估。
9. **迁移幂等性** → `data/lloom.db` 含真实对话/向量，所有 `ALTER` 先 `PRAGMA table_info` 去重，迁移前 `cp` 备份。
10. **改 Python 后必须重启 Rust 服务**才重拉 AI 服务（既有踩坑）；P0.f 后该类问题消失。
11. **（v3 新增）update_model 白名单**：新增列要同步加白名单否则 WebUI 改不了；`health_state`/`needs_calibration`/
    `price_stale` 由系统写不放白名单（防用户手改破坏自适应）。
12. **（v3 新增）Python 仍需全模型池**：P0.f 下发 `assignments` 同时必须保留 `models` 全池，
    否则 `_make_summary`（摘要）与分解兜底失去模型来源 → 契约见 §P0.f 双字段。
