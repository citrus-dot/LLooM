# 智能路由：现状分析与优化计划 v2

> 分析对象：`v2` 分支 @ `091dc31`
> 关注文件：`crates/lloom-core/src/router.rs`、`server.rs`、`db.rs`、`api/ai_service.py`
> 目标：让路由自适应用户任意模型集（含增删）、按「成本 × 成效」分配任务、为预算动态调整留出接口
>
> **v2 修订（2026-08-24）**：在 v1 基础上融合四路外部输入——
> ① NeMo Switchyard（NVIDIA 开源路由库）的决策信号模型、算法族与可观测性设计；
> ② vLLM Semantic Router 的「信号—投影—决策」三层控制面架构；
> ③ 定价表系统设计（静态四级解析 + 动态更新机制，已实测 litellm / models.dev / OpenRouter 三个数据源）；
> ④ Router-R1 / R2-Router / AutoMix / BEST-Route / RouterBench 等路由研究的新证据。
> v1 的「现状分析」「九个核心问题」「P0–P3 骨架」经复核仍然成立，本版保留并扩充。

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

## 二、九个核心问题（v1 复核，全部仍成立）

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
   真实情况是 qwen3-max ≈ 0.347 USD/M，比 gpt-4o 便宜约 7 倍。

### P0-5　用量记录链路是坏的 → 自适应没有燃料

全仓只有一个 `insert_usage` 调用点，在 `server.rs:398`：

```rust
let _ = db::insert_usage(&model, "default", 0, 0, 0.0, None, is_hit);
```

tokens 和 cost 写死 0、`task_type` 传 None、`model` 取不到时回落成字符串 `"default"`，
而且只在语义缓存事件里触发。`chat_stream` 明明拿到了
`res.cost / res.input_tokens / res.output_tokens`（`server.rs:331-341`）却**从不落库**。

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

## 三、外部依据（v2 大幅扩充）

### 3.1 路由范式与收益（业界实测，含诚实口径）

| 范式 | 机制 | 实测收益 | 代价 |
|---|---|---|---|
| Classifier 分类路由 | 轻量分类器一次性判难度 | RouteLLM（ICLR 2025）：MT-Bench 省 85%、MMLU 省 45%、GSM8K 省 35%，均保 95% GPT-4 质量 | 需标注数据；判错直接损质 |
| Cascade / 级联 | 先便宜跑，质量不足再升级 | FrugalGPT：HEADLINES 省 80% 且精度 +1.5%（最好情形 98%）；AutoMix（NeurIPS 2024）：自验证决定升级，省 >50%；BEST-Route（ICML 2025，微软）：联合路由「模型 × 采样次数」，省 60% 且性能降 <1% | 困难样例延迟翻倍；开放式生成需质量裁判而非置信度 |
| Stage / 执行期路由 | 用对话内信号（工具结果、错误、进度）路由，不额外调模型 | Switchyard 合作伙伴 Cognition：FrontierCode Main 距 Opus 5 精度差 2.8 点，成本降 28% | 需要执行事件流（LLooM 已有 `task_done.error`） |
| Escalation 升级 | 每轮先跑弱层，裁判读答案再决定升级 | Switchyard 合作伙伴 LangChain：145 个多轮 Deep Agents 任务，成本降 74%（仅 7% 调用走前沿模型），精度损失约 6 点 | 每请求多一次裁判调用 |
| Semantic 语义路由 | 向量比对意图样例 | 适合固定领域分流；LLooM 语义缓存实测同义改写聚 0.81、异话题 ≤0.53 | 需维护意图库 |
| 编排路由 | 分解 → 子任务级分配 → 聚合 | R2-Router：查询分解为子任务、跨异构 LLM 分配；Switchyard 内部基准：成本降到单独用 Opus 4.8 的约 1/3 | 系统复杂度最高 |

**重要修正（诚实口径）**：路由收益不是常数而是区间（RouteLLM 35–85%），**完全由负载难度分布决定**——
开放式 chat 大多简单（仅 14–26% 需强模型），知识考试型负载大多困难（54% 需强模型）。
Amazon Bedrock 智能路由官方口径：平均省 30%，按模型族分 16%（Llama）~56%（Anthropic），
路由开销约 85ms。任何验收目标都应写成「随负载难度可变」的形式，而不是固定百分比。

可直接采纳的六条经验：
1. **按任务类型分别设阈值/权重**。全局单一策略在异构负载下明显欠优。
2. **上线前影子评测**：双跑廉价/强模型，只返回路由结果、两者都记日志，抽样人评确认后再切流。
3. 分类器可跨模型对迁移，训练样本 ~1500 条即有效；路由本身开销目标 <100ms。
4. **级联/升级只用于非实时路径**（编排、批处理），`chat/stream` 保持一次性分类路由。
5. **先规则路由兜底、后学习路由**：在没建立夜间评测回路之前，把「确定简单」的任务
   （抽取、分类、格式化）固定给轻量档，其余走强模型；有评测回路后再放开。
6. **RouterBench 的 AIQ 指标**思路可借用：以「全弱基线」为下界、「全强基线」为上界，
   衡量路由器填补差距的比例（0..1），综合所有预算水平打分，而非单点。

### 3.2 NeMo Switchyard：决策信号、算法族与可观测性

NVIDIA 2026-08-11 发布的 Rust 路由库（pre-alpha，v0.2.0）。**定位与 LLooM 高度同构**
——Rust 代理/库、多后端、按策略路由、协议翻译——但**不直接引入依赖**（pre-alpha，API 会破坏性变更），
只借鉴三个层面的设计：

**（a）决策信号的三分类**（这是它对 LLooM 最有价值的贡献）：

| 信号类别 | Switchyard 信号源 | LLooM 对应物 |
|---|---|---|
| 模型能力（能否做对） | logprobs、agentic trace、分类器 | `capability_tier` + `quality_score`/`ewma_quality`（v1 已设计） |
| 模型成本画像 | 延迟、成本、冗余度 | `price_tiers_json` + `avg_latency_ms` + `avg_cost`（v1 已设计） |
| 基础设施状态 | 负载、错误、定价 | `health_state` + 预算水位 + 定价表新鲜度（v2 新增） |

**（b）四种路由算法与 LLooM 的映射**：

| Switchyard 算法 | 机制 | LLooM 落点 |
|---|---|---|
| Random | 加权分流，A/B 与成本实验 | P5 影子评测的对照分流（做基线对比用） |
| LLM classifier（capability） | 分类器判请求走弱/强层 | 现有 `ai_client::classify` 兜底的正规化（低置信才触发） |
| LLM classifier（escalation） | 先跑高效层，裁判读答案决定是否送强层 | **P4 新增**：编排路径的升级重试 |
| Stage router | 用对话内信号（工具结果、错误、进度）选模型，不额外调模型 | **P4 新增**：`task_done.error` / 子任务失败即现成的 stage 信号 |

**（c）可观测性设计**（直接抄）：
- 每次路由记录五元组：**所选模型、决策依据、token 用量、延迟、调用结果** → 本计划新增
  `routing_decisions` 审计表（P1）。
- 暴露 **routing overhead（路由开销）** 为一等指标 → 路由自身耗时（信号提取 + 决策）
  必须与模型调用耗时分开统计，并设预算（<100ms，启发式快路径 <10ms）。
- libsy 的 **Step 流**心智模型（库只产决策流、宿主自己执行调用）= 本计划 P0.f
  「Rust 决策 / Python 执行」分离的同款设计，验证了该路线可行。

### 3.3 vLLM Semantic Router：信号—投影—决策三层控制面

vLLM 社区把路由做成 Envoy ext_proc 控制平面。**不引入 Envoy**（对单机 LLooM 过重），
但它的三层架构正是把 LLooM 现在「散落在 security.rs / router.rs / ai_service.py 里的
检测逻辑」组织起来的正确方式：

```
信号 Signal（检测，只回答"看到了什么"）
  ├─ 启发式：authz / conversation / context / keyword / language / structure / event / metadata
  └─ 学习型：classifier / complexity / domain / embedding / modality / fact-check / jailbreak / pii / preference / reask / kb / user-feedback

投影 Projection（协调竞争的信号，产出中间事实）
  ├─ partitions：独占域分区（多信号竞争时选出唯一意图）
  ├─ scores：加权聚合分（如 request_difficulty = Σ wᵢ·signalᵢ）
  └─ mappings：阈值分带（score → easy/medium/hard 档位名）

决策 Decision（策略规则，回答"该做什么"）
  └─ 对信号/投影的 AND/OR 规则 → 活动路由 + 模型候选集
```

它的 YAML 契约示例（值得抄结构）：

```yaml
routing:
  signals:
    keywords:  [{name: urgent_keywords, operator: OR, keywords: [urgent, asap]}]
    embeddings: [{name: technical_support, threshold: 0.75, candidates: [...]}]
  projections:
    scores:    [{name: request_difficulty, method: weighted_sum,
                 inputs: [{type: embedding, name: technical_support, weight: 0.18}]}]
    mappings:  [{name: request_band, source: request_difficulty, method: threshold_bands,
                 outputs: [{name: escalated, gte: 0.25}]}]
```

**LLooM 现有组件 → 信号层的映射**（这张表说明三层架构不是推倒重来，而是收敛现状）：

| LLooM 现有组件 | 语义路由概念 | 类型 |
|---|---|---|
| `security::check`（PII/越狱拦截） | 信号 `pii` / `jailbreak` | 学习型（已有） |
| `rule_classify` 4 组正则 | 信号 `keyword` / `structure` | 启发式（已有，待正规化） |
| `ai_service.py::_is_complex` / `_is_comparison` | 信号 `complexity` / `structure` | 启发式（已有，在 Python 侧，应上移 Rust） |
| `ai_client::classify` LLM 兜底 | 信号 `classifier` | 学习型（已有，低置信才触发） |
| 语义缓存 embedding（all-MiniLM-L6-v2） | 信号 `embedding` + 插件 `response_cache` | 学习型（已有） |
| `cache_feedback` 点赞点踩 + 灰区问 | 信号 `user-feedback` | 学习型（已有） |
| 短间隔重问检测 | 信号 `reask`（隐式不满） | **待新增**：同对话内相似度 >阈值 且间隔 <N 分钟 → 负反馈信号 |
| token 数 / 上下文长度 | 信号 `context` | 启发式（tiktoken 已有） |
| 预算水位 | 决策策略输入 | 已有（待接入） |

插件链思想（response_cache / jailbreak / pii / system_prompt 在同一条请求处理链上，
每个可独立开关）也与 LLooM「安全检查 → 缓存 → 路由」的既有顺序吻合，正规化为
可配置插件即可。

### 3.4 定价与元数据源实测（2026-08-24 复测）

| 数据源 | 覆盖 | 新鲜度 | 本机可达性 | 实测结论 |
|---|---|---|---|---|
| litellm 打包表 `litellm.model_cost` | 2982 条 | 随 pip 包版本 | ✅ 本地 | gpt-4o ✅ / deepseek-chat ✅ / **qwen-plus、qwen-max ❌ MISS** |
| litellm GitHub raw `model_prices_and_context_window.json` | 同上但最新 | 社区 PR，调价后 1–3 天更新 | ⚠️ 远端拉取 SSL 校验失败（既有问题），需镜像 | 同覆盖，作为刷新源 |
| models.dev `api.json` | 75+ 提供商、2000+ 模型，含 cost/1M、context、cache 价 | 社区 PR | ⚠️ 本机直连被重置，可经镜像/代理 | **实测无 dashscope/aliyun 条目**；七牛托管 qwen3-max 等（context 262144）但**无 cost 字段**；国际模型与 context window 可作补充源 |
| OpenRouter `GET /api/v1/models` | 400+ 模型 | **实时**（唯一 live 源） | 未启用 OpenRouter 前不适用 | 若未来走 OpenRouter，可用响应内 `usage.cost` 直接对账 |
| 项目 overlay `model_catalog.json` | 自维护 | 手动 | ✅ | **唯一能覆盖百炼/国产模型的层，必须保留并作为主要维护面** |

结论：
1. **百炼系模型在所有自动源中均无价格**——overlay 不是可选项而是必需品；
2. 动态更新的正确形态是「**快照优先 + 定期刷新 + 人工校准覆盖**」，而不是依赖实时 API
   （唯一实时源 OpenRouter 不在当前供应商组合内）；
3. 本机网络受限 + SSL 问题 → 刷新走镜像（jsdelivr / ghproxy 类，项目已有 GH_MIRROR 基建），
   失败静默保持本地值并标陈旧。

### 3.5 自进化路由研究（新证据）

- **Router-R1（2026）**：用 RL 把路由建模为序列决策，路由器本身是 LLM，交错 think/route 动作，
  奖励 = 格式正确性 + 结果质量 + 成本。**关键突破：以「模型描述符」（价格、延迟、样本表现）
  为条件，路由器能泛化到训练时未见过的模型**——用 10 个模型训练，能正确路由第 11 个没见过的。
  这直接验证了本计划 P0「注册表驱动 + 描述符评分」的路线：**路由决策只看模型属性、不看模型名**。
- **R2-Router**：把查询分解为子任务、跨异构 LLM 分配——正是 LLooM 编排路径的目标形态，
  验证「子任务级分配」优于「整请求单点路由」。
- **BEST-Route（ICML 2025）**：联合优化「选哪个模型 × 采样几次」省 60%——远期可借鉴
  （对低置信请求并行采样两个轻量档 + 裁决，而不是直接上强模型）。

### 3.6 对本项目的六条结论

1. 三层架构（信号→投影→决策）用来**收敛**现有散落的检测逻辑，不是重写；
2. Switchyard 的 stage router / escalation 正好补上 LLooM 编排路径缺失的「执行期智能」；
3. 定价表做成「快照 + 刷新 + overlay + 人工校准」四级体系，百炼模型靠 overlay；
4. 「描述符条件化」（不看模型名只看属性）是自适应任意模型集的正确路线，已有学界验证；
5. 收益预期按负载难度写区间，验收用 AIQ 式指标而不是固定百分比；
6. 一切学习型信号（EWMA、阈值自校准、影子评测）都以 P1.a 真实用量落库为前置。

---

## 四、目标架构：信号—投影—决策管线

### 4.1 总览

```
请求（chat/stream 或 orchestrate/stream）
  │
  ▼
┌─────────────────── 信号层（检测，只答"看到了什么"）───────────────────┐
│ 启发式快路径（<10ms，纯 Rust）：                                      │
│   keyword/structure 正则 · context token 数 · language · budget 水位  │
│ 学习型慢路径（按需触发）：                                            │
│   embedding（复用语义缓存向量）· complexity · pii/jailbreak（已有）    │
│   classifier（LLM 兜底，仅启发式低置信时触发，预算 <100ms）           │
└──────────────────────────────────────────────────────────────────────┘
  │  命名信号集
  ▼
┌─────────────────── 投影层（协调，产出中间事实）─────────────────────┐
│ difficulty = weighted_sum(structure, complexity, context, embedding)  │
│ band = threshold_bands(difficulty) → {easy | medium | hard}           │
│ intent/partition = 多信号竞争时选出唯一 task_type                     │
└──────────────────────────────────────────────────────────────────────┘
  │  task_type + band + 约束（tools/vision/context/预算档）
  ▼
┌─────────────────── 决策层 plan()（门槛 + 加权评分）─────────────────┐
│ 硬门槛：is_active · capability_tier ≥ 档位 · context_window 够 ·      │
│         health ≠ down · supports_tools/vision · est_cost ≤ 上限        │
│ 软评分：q_w·质量 − c_w·阶梯价成本 − l_w·延迟 + 0.05·priority          │
│ 产出：主选 + 按 score 降序的 fallback 链（depth = fallback_depth）     │
│ 产出：审计记录（候选集/各项得分/选中理由）→ routing_decisions 表      │
└──────────────────────────────────────────────────────────────────────┘
  │
  ▼
执行（Python 纯执行器；编排路径见 §4.5）
  │  SSE: usage(tokens/cost/latency) · cache_hit · error
  ▼
┌─────────────────── 反馈闭环 ───────────────────────────────────────┐
│ usage_records（真实燃料）→ model_task_score.EWMA 更新               │
│ user-feedback / reask → 质量信号                                    │
│ 影子评测 + AIQ 式离线重放 → 策略调参                                │
│ 定价刷新 job → price_source/updated_at/stale 维护                   │
└────────────────────────────────────────────────────────────────────┘
```

### 4.2 与 v1 计划的关系

v1 的 `plan()`（门槛 + 评分 + fallback 链）**原样保留为决策层**；v2 新增的是：
信号层把散落的检测正规化为命名信号（`security::check`、正则、`_is_complex`、
embedding 复用各归其位），投影层把「难度」从单点布尔（complex/simple）升级为
加权分 + 分带（easy/medium/hard），使三档能力分层（tier 1/2/3）有了明确对接点。

### 4.3 信号层配置化（借语义路由的契约思想）

信号以命名注册，决策只引用名字（不内联检测逻辑）。落地形态：Rust 侧一个
`signals` 模块 + DB `routing_config` KV 表存阈值（如 reask 判定的相似度/间隔、
classifier 触发的置信下限、difficulty 各信号权重）。WebUI 暴露为「路由策略」页。

### 4.4 执行期 stage 信号与升级（编排路径，Switchyard 借鉴）

LLooM 编排事件流里已有现成的 stage 信号源：
- `task_done.error`（子任务失败）→ **升级重试**：失败子任务换 fallback 链下一档
  （或直接按 `capability_tier+1` 重选）重试一次，重试即记 `escalation_count`；
- 子任务全部成功且质量信号正常 → 后续同类子任务可降档试探（成本优化）；
- 连续失败/循环 → 整体升级到强模型并标记本次编排「高难」回写 `model_task_score`。

### 4.5 Escalation 模式（可选开关，仅编排路径）

开关打开时：子任务先跑评分最高的**轻量档**候选，产出后由裁判信号判断质量
（结构化输出解析成功 / 长度合理 / 无错误标记）——不达标才升级强档。
裁判优先用**零成本信号**（解析成功、JSON schema 校验），不够时才用 LLM judge。
对应 LangChain 实测：74% 成本下降、约 6 点精度代价——所以做成**按 task_type 可关**的旋钮。

---

## 五、优化计划

### 设计原则（v1 保留 + v2 增补）

1. **单一真源**：路由策略下沉到 SQLite + 配置。Rust 是唯一决策者，Python 降级为纯执行器。
2. **注册表驱动 / 描述符条件化**：路由只在「当前 is_active 的模型」里做选择，代码里不出现
   任何具体模型名；决策只依赖模型属性（tier/价格/窗口/实测质量），Router-R1 已验证此路线
   可泛化到未见模型。
3. **决策可解释**：每次路由输出候选集、各项得分、选中理由（审计落库 + SSE 头部）。
4. **量纲统一**：全库统一为 **USD per token**，写入前强制归一。
5. **（v2 新增）路由自身有预算**：启发式快路径 <10ms，含 LLM 分类器 <100ms，
   routing overhead 与模型调用耗时分开统计。
6. **（v2 新增）检测与决策分离**：信号只答「看到了什么」，决策只答「该做什么」，
   两者都可独立增删配置。

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
并在 `Model` 写入路径加断言：单价落在 `[1e-9, 1e-3]` USD/token 之外则拒绝并提示。

#### P0.b　扩展模型元数据（v2 增补定价溯源三列）

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
ALTER TABLE models ADD COLUMN health_state      TEXT    DEFAULT 'unknown';
ALTER TABLE models ADD COLUMN health_checked_at TIMESTAMP;
ALTER TABLE models ADD COLUMN needs_calibration INTEGER DEFAULT 1;
-- v2 新增：定价溯源（供 §P2 动态更新使用）
ALTER TABLE models ADD COLUMN price_source     TEXT DEFAULT 'unknown'; -- manual > overlay > litellm_remote > litellm_packaged > heuristic
ALTER TABLE models ADD COLUMN price_updated_at TIMESTAMP;
ALTER TABLE models ADD COLUMN price_stale      INTEGER DEFAULT 0;  -- 刷新发现 >20% 偏差且非 manual 来源时置 1
```

`price_tiers_json` 形如
`[{"max_input":32768,"in":3.47e-07,"out":1.39e-06},{"max_input":131072,...}]`。

**价格来源优先级**（写入与刷新都遵守）：`manual`（用户在控制台核对过，最高，刷新不覆盖）
> `overlay`（项目 catalog）> `litellm_remote`（GitHub raw 刷新）> `litellm_packaged`（本地快照）
> `heuristic`（名称启发式，标 `needs_calibration=1`）。

#### P0.c　新增策略表

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
    escalation_enabled   INTEGER DEFAULT 0,  -- v2：编排路径升级开关（§4.5）
    updated_at           TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE model_task_score (         -- 自适应学到的实际成效
    model_name        TEXT,
    task_type         TEXT,
    success_count     INTEGER DEFAULT 0,
    fail_count        INTEGER DEFAULT 0,
    escalation_count  INTEGER DEFAULT 0,
    avg_cost          REAL    DEFAULT 0,
    avg_latency_ms    REAL    DEFAULT 0,
    ewma_quality      REAL    DEFAULT 0.6,
    sample_count      INTEGER DEFAULT 0,   -- v2：保守期解除的样本阈值依据
    updated_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (model_name, task_type)
);

CREATE TABLE routing_decisions (        -- v2 新增：Switchyard 式决策审计
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id     TEXT,                -- 串联 cascade 多次调用
    created_at     TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    task_type      TEXT,
    band           TEXT,                -- easy/medium/hard
    signals_json   TEXT,                -- 命名信号快照（难度分、置信度、触发的正则等）
    candidates_json TEXT,               -- 每个候选的门槛通过情况与各项得分
    selected       TEXT,
    fallback_chain TEXT,
    routing_ms     REAL,                -- 路由自身开销
    outcome        TEXT                 -- success/fail/escalated/cache_hit（事后回填）
);
```

#### P0.d　替换决策函数

`router.rs` 删掉 `TASK_MODEL_MAP`、`INFERENCE_MODELS`、`task_model_preference`，换成：

```rust
pub struct Candidate { pub model: Model, pub score: f64, pub reason: String }

pub fn plan(task: &str, band: &str, policy: &RoutingPolicy, models: &[Model],
            est_in_tokens: u32, est_out_tokens: u32) -> Vec<Candidate>
```

硬门槛（gate）：`is_active` / `capability_tier` / `context_window` / `health_state` /
`supports_*` / `est_cost ≤ max_cost_per_request`。
软打分：`score = q_w·quality(m,task) − c_w·norm(est_cost) − l_w·norm(latency) + 0.05·priority`。
`argmax` 为主选，其余按 score 降序取前 `fallback_depth` 个作为降级链。

候选集为空时返回明确错误（"当前无可用模型满足 X 任务的约束"），**不再伪造空 spec**。
`stream` 改读 `supports_stream`。

#### P0.e　增删模型时的自动打标（自适应核心，v2 微调解析链）

`POST /api/models` 之后跑元数据解析，**五级兜底**：

1. **litellm 打包表**（本地必有）：单价、cache 价、`max_input_tokens`、`supports_*`。
2. **litellm 远端刷新**（若 §P2 刷新 job 可用）：GitHub raw / 镜像，覆盖调价。
3. **models.dev**：国际模型与 context window 补充（实测无 DashScope，仅作补充源）。
4. **项目 overlay `model_catalog.json`**：百炼/国产模型——**唯一能覆盖它们的层**。
5. **名称启发式**（最后兜底）：`flash|mini|turbo|small|lite|air|8b|4b|1.5b`→tier 1，
   `max|opus|ultra|pro|reasoner|thinking|r1|o1|o3`→tier 3，其余 tier 2；
   `provider=="ollama"` 或 localhost → `is_local=1`、单价 0。命中即标 `needs_calibration=1`。

`needs_calibration=1` 的模型进**保守期**：只参与低风险任务且限流，真实结果回填
`ewma_quality`，`sample_count` 达阈值（默认 20）后解除。

删除路径：保持软删；若被删模型是 `pinned_model`，置 NULL 并提示恢复自动选择。

#### P0.f　消除双真源

`ai_service.py` 删掉 `TASK_MODEL_PREFERENCE` / `DECOMPOSER_PREFERENCE` / `_select_model`。
`/v1/orchestrate/stream` 请求体携带 Rust 决策好的 `{subtask_id → model_spec}`；
分解器模型由 Rust 按 `task_type="decompose"` 走同一套评分选出。
（对应 Switchyard libsy 的「库产决策、宿主执行」模型，业界已验证。）

#### P0.g　（v2 新增）信号层正规化

把 §4.1 的启发式信号收进 `lloom-core/src/signals.rs`：`rule_classify` 正则、
`_is_complex`/`_is_comparison`（从 Python 上移 Rust）、tiktoken 上下文计数、
预算水位。学习型信号暂不新增模型调用——embedding 复用语义缓存已算出的向量，
classifier 沿用现有 LLM 兜底但加「启发式置信度低于阈值才触发」的闸门。
本阶段**不追求信号齐备**，只求命名、可配置、有单测。

---

### 阶段 P1：打通用量闭环 + 成本×成效落地

#### P1.a　修用量落库（**最高优先级**，一切学习型功能的前置）

`chat_stream` 成功返回后写真实数据（model/tokens/cost/task_type/latency/cache_hit）；
`orchestrate_stream` 按子任务逐条记账；清掉「全 0 + model=default」的旧调用与脏数据。
补 `latency_ms`、`request_id` 字段。

#### P1.b　基于当前模型集的推荐分配（v1 结论保留）

| 任务类型 | 建议主选 | 成本 | 降级链 | 相对现状 |
|---|---|---|---|---|
| `simple_qa` | qwen2.5-local | 0 | qwen-plus → qwen3.6-flash | 云端兜底由 flash 改 plus |
| `general` | qwen-plus | 4.8 | qwen3.6-flash → local | 不变 |
| `decompose` / `classify`（内部） | **qwen-plus** | 4.8 | qwen3.6-flash | **省约 69%** |
| `coding` | deepseek-v3 | 17.0 | qwen-plus → qwen3-max | 升级位由 plus 改 max |
| `math_logic` | deepseek-v3 | 17.0 | qwen3-max → qwen-plus | 升级位改 max |
| `complex_reasoning`（≤32K） | **qwen3-max** | 22.5 | deepseek-v3 → qwen-plus | 主选由 3.6-plus 改 max |
| `complex_reasoning`（>32K） | qwen3.6-plus | 26.0 | qwen3-max | 阶梯价交叉后切换 |

#### P1.c　成效分：冷启动 + 在线修正（v2 具体化信号源）

- **冷启动 `quality_score`**：litellm 元数据 + overlay 里按 task_type 存的榜单折算分
  （公开评测 → 0..1）。**按任务类型分别给分**（一个模型 coding 0.8、math 0.5 是正常状态），
  这正是 RouterBench「模型 × 任务 × 对错 × 成本」数据形态的本地化。
- **在线 `ewma_quality`**：正常完成（+）/ 重生成或切换模型重问（−）/ cascade 升级率（−）/
  点赞点踩（强 ±）/ 结构化解析失败率（−）/ **reask 信号（v2 新增）**：同对话内
  embedding 相似度 >0.85 且间隔 <5 分钟的重复提问记一次隐式不满。`α = 0.1~0.2`。

#### P1.d　分任务阈值 + 影子评测 + AIQ 式指标

- `routing_policy` 三权重按任务给不同预设（`simple_qa` c_w=0.7，`complex_reasoning` q_w=0.8）。
- `POST /api/routing/shadow`：采样流量双跑「路由选择」与「强模型基线」，两份结果落
  `model_task_score`，只返回路由结果。
- **AIQ 式重放评测（v2 新增）**：基于 `usage_records` + `routing_decisions` 历史离线重放，
  与「全弱 / 全强」两条基线比，输出「成本—质量」曲线与 AIQ（填补差距比例）。
  这是调 `routing_policy` 权重的依据，避免凭感觉调参。

---

### 阶段 P2：定价表系统与动态更新（v2 新增独立阶段）

前置：P0.a/b 的量纲修正与 `price_source` 列。

#### P2.a　刷新机制

- Rust 侧后台 job（`lloom-server` 内 tokio 定时任务）：默认每 24h 拉一次
  `model_prices_and_context_window.json`（GitHub raw，失败走 jsdelivr/ghproxy 镜像，
  复用 `GH_MIRROR` 基建），models.dev `api.json` 作为 context window / 国际模型补充源。
- 刷新规则：
  - 只更新 `price_source ∈ {litellm_packaged, litellm_remote, heuristic}` 的条目；
  - `manual` / `overlay` 来源**永不自动覆盖**（用户核对过的价格最高优先）；
  - 新价与现价偏差 >20%：非 manual 直接更新并记 `price_updated_at`；
    manual 则置 `price_stale=1`（WebUI 黄点提示「官方价已变化，点击核对」）；
  - 拉取失败：静默保持本地值，连续失败 7 天才告警（网络受限是常态，不该刷屏）。
- WebUI：「模型与定价」页显示每个模型的 `price_source` 徽标 + 更新时间 + stale 标记 +
  「采纳建议价」按钮（一键把刷新价转正为 manual）。

#### P2.b　overlay 维护

`model_catalog.json` 按 `{provider}/{model}` 键存：单价（USD/token）、阶梯价、
context window、supports_*、按任务类型的冷启动 quality_score。百炼模型在此维护。
版本随仓库更新；刷新 job 不碰它。

#### P2.c　对账（可选，低优先级）

若未来接入 OpenRouter：响应内 `usage.cost` 可直接对账本地计算值，偏差 >5% 记告警。
当前供应商（DashScope/DeepSeek/Ollama）无此能力，跳过。

---

### 阶段 P3：健康感知与故障转移（v1 P2 保留 + v2 增补）

- 被动健康：失败/超时/429 滑窗累计 → `degraded` → `down` + 指数退避探测。
- 主动探测：`down` 每 N 秒最小请求试探。
- 故障转移：按 P0.d fallback 链顺序重试，每次记 `escalation_count`。
- 熔断：单模型连续失败达阈值，临时剔除候选集。
- **（v2）routing overhead 指标**：`routing_decisions.routing_ms` 聚合暴露；
  启发式快路径超 10ms / 全路径超 100ms 视为实现 bug，进验收断言。

---

### 阶段 P4：编排智能升级（v2 新增，多模型协作的核心）

前置：P0.f（Rust 统一决策）、P1.a（子任务记账）。

#### P4.a　子任务级评分分配（R2-Router 思路）

分解产出的每个子任务，按其**自身的** task_type（分解器输出中带类型标注）独立走一遍
`plan()`——而不是像现在把全部 `ModelSpec` 丢给 Python 轮询分配。子任务间无依赖时
可并行（LLooM 已知待办 O6 与此合并实现）。

#### P4.b　Stage 路由：执行期信号升级（Switchyard 核心借鉴）

- `task_done.error` 非空 → 该子任务按 fallback 链降级重试一次（记 escalation）；
- 重试仍失败 → 整体错误如实汇报（沿用现有「失败不美化」原则）；
- 连续 ≥2 个子任务失败 → 剩余子任务整体升一档 capability_tier；
- 子任务全绿且耗时低于该任务 P50 → 同批后续子任务允许降一档试探（成本优化，
  失败立刻回滚档位）。

#### P4.c　Escalation 模式（可选开关）

`routing_policy.escalation_enabled=1` 的任务类型启用 §4.5 流程。默认只对
`decompose`/`classify`/`simple_qa` 这类「零成本质量信号可靠」（解析成功即对错分明）
的任务开启；开放式生成不开启（业界结论：置信度在开放式任务上不可靠，需 judge，成本高）。

#### P4.d　汇总模型也走评分

聚合阶段模型选择按 `task_type="aggregate"` 走 `plan()`，不再写死偏好。
长输入汇总场景天然受 `context_window` 门槛约束（所有子任务结果拼接可能超小窗口）。

---

### 阶段 P5：预算驱动的动态调整（v1 P3 保留）

#### P5.a　预算进入决策链

| 剩余预算 r | 档位 | 行为 |
|---|---|---|
| r > 50% | 正常 | 按 `routing_policy` 原权重 |
| 20% < r ≤ 50% | 节流 | `cost_weight × 1.5` |
| 5% < r ≤ 20% | 紧缩 | `cost_weight × 2.5`；复杂任务降一档；强制开语义缓存 |
| r ≤ 5% | 保护 | 仅 `is_local=1` 或成本低于阈值；超限任务明确提示 |

降级而非硬拒：预算耗尽推给本地 Ollama。`qwen2.5-local` 必须始终留在候选集。

#### P5.b　预算模型扩展

`budgets` 加 `scope_task_type` / `soft_limit_ratio` / `action_on_exceed(degrade|block)`；
`scope` 扩到 `user / model / task_type / global`。

#### P5.c　预估成本前置校验

tiktoken 算输入 token（`tiktoken_cache/` 已有），输出用该 task_type 历史 P50 估计，
得 `est_cost` 参与 `max_cost_per_request` 门槛与档位判断。
（业界通用「先估算、后对账」模式；若只有 token 预算没有价格数据，也可先做 token 硬顶。）

---

## 六、落地顺序与验收（v2 更新）

| 顺序 | 内容 | 验收标准 |
|---|---|---|
| 1 | P1.a 修用量落库 | 一次对话后 `usage_records` 出现正确的 model/tokens/cost/task_type/latency |
| 2 | P0.a 修成本量纲 + 写入断言 | qwen 系列单价降为原 1/10；越界单价写入被拒 |
| 3 | P0.b/c 建表迁移（含 price_source 三列、routing_decisions） | 幂等可重跑，旧数据不丢 |
| 4 | P0.d 评分式 `plan()` + router 单测 | 删任一模型后自动改选；不再返回未注册名字；空候选集明确报错 |
| 5 | P0.e 五级打标 | 新增未知模型 → 元数据自动填充、标 `needs_calibration`、不承接复杂任务 |
| 6 | P0.f+g 消除 Python 真源 + 信号正规化 | `ai_service.py` 无模型名字面量；信号可配置、有单测 |
| 7 | P1.b/c 成效分 + 推荐分配 | 影子评测下内部分解路径成本降 ≥60%，质量无显著回退 |
| 8 | P2 定价刷新 job + WebUI 徽标 | 手动触发刷新成功更新非 manual 来源；断网刷新静默保持本地值 |
| 9 | P3 健康 + fallback + overhead 指标 | 停 Ollama/错 key → 自动降级；routing_ms 快路径 <10ms |
| 10 | P4 编排升级 | 子任务失败自动降级重试成功；`escalation_enabled` 任务成本再降 ≥30%（相对步骤 7） |
| 11 | P5 预算联动 | 预算接近耗尽 → 逐档降级直至只走本地 |
| 12 | P1.d AIQ 重放评测 | 离线重放输出成本—质量曲线；`routing_policy` 调参有数据依据 |

**测试基建**（v1 结论保留）：Rust 侧零单测，`router.rs` / `signals.rs` 纯函数为主最适合先补。
覆盖：空候选集、单模型、删主选后降级、阶梯价交叉点、预算各档位、difficulty 分带边界、
reask 判定、保守期解除阈值。

---

## 七、风险与注意事项（v2 更新）

1. **价格会过期** → 放 DB + overlay + 刷新 job + manual 校准入口，代码里零价格字面量。
   本机 litellm 远端拉取有 SSL 问题，刷新走镜像；`qwen3.6-flash` 存在 1.2/7.2 与
   0.367/2.936 两种公开口径（本计划采信 1.2/7.2，与 DB 反推值逐位吻合）。
2. **models.dev 无 DashScope**（2026-08-24 实测）：百炼价格只能靠 overlay + 人工核对，
   别指望全自动覆盖国产模型。
3. **Switchyard 是 pre-alpha**（v0.2.0，v1.0 前 API 会破坏性变更，Python launcher 已移除）：
   **只借鉴设计，不引入 crate 依赖**。若未来要引入，走 libsy（库路径）而非 server 路径，
   且等 v1.0。
4. **vLLM Semantic Router 是 Envoy ext_proc 控制面**，架构重、面向服务网格场景：
   借它的「信号—投影—决策」分层思想与 YAML 契约形态，不引入 Envoy。
5. **收益预期按区间管理**：路由收益 35–85% 取决于负载难度分布；验收写「随难度可变」
   的 AIQ 式指标，不承诺固定百分比。
6. **级联/升级延迟翻倍** → 只在编排路径（P4）启用；`chat/stream` 保持一次性分类路由。
7. **开放式生成不能靠置信度做级联裁判** → escalation 默认只对结构化/解析可判任务开启。
8. **Router-R1 式 RL 路由是远期可选项**：当前阶段「描述符评分 + EWMA + 影子评测」已覆盖
   其核心收益（泛化到新模型），RL 训练回路等影子评测数据积累到千级样本再评估。
9. **迁移幂等性**：`data/lloom.db` 含真实对话与向量数据，所有 ALTER 先查
   `PRAGMA table_info`，迁移前备份。
10. **改 Python 后必须重启 Rust 服务**才会重拉 AI 服务（既有踩坑记录，P0.f 之后该类问题消失）。
