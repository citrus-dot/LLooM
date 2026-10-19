# 09 · LLooM RouterBench 构建与立项演示方案（重写评审稿）

> 日期：2026-10-04  
> 代码基线：`v2` @ `412ea10`  
> 性质：**评审稿，未实施**——评审通过后再开工。  
> 核心定位：以 **LLooM RouterBench** 为本阶段唯一主线，把「固定模型池 + 冻结模型×样本结果矩阵 + LLooM 路由决策 + 零 API 回放」做成可重复、可解释、可展示的路由评测基础设施；产品优化、WebUI 与真实池自采都围绕这一主线服务。

---

## 一、目标重新定义：这次不是“做一个模型 Benchmark”，而是“做一个 Router Benchmark”

### 1.1 本阶段真正要回答的问题

LLooM 已经具备以 `plan()` 为核心的模型选择机制：任务类型 / band、能力门槛、质量 EWMA、成本、预算与健康状态等信号共同参与模型选择。因此本阶段不再以“某模型在某 benchmark 上得多少分”为主要目标，而要验证：

> **给定同一批 query、同一组候选模型和固定的模型成本，LLooM 能否让简单任务更多落到便宜模型，让真正需要更强能力的任务升级到强模型，从而在质量接近 Always-Strong 的情况下显著降低平均成本。**

这一定义决定了 benchmark 的基本单位必须是：

```text
query
 ×
model
 ↓
(score, cost [, latency, response])
```

即每个 query 都要知道多个模型分别“答得怎样、花多少钱”，而不是只知道一个模型的 aggregate accuracy。

### 1.2 “大小模型差异大”还不是充分条件

原规划把“模型平均分差大”作为主要选型标准，这需要修正。

一个 benchmark 即使满足：

```text
Strong = 90
Weak   = 50
```

也可能完全不适合 routing。比如 Strong 在几乎所有样本上都赢，Weak 只有极少数样本能处理；此时 LLooM 的最优策略仍接近“全部 Strong”，没有明显 routing headroom。

因此，RouterBench 的数据集筛选必须同时看：

| 指标 | 含义 | 为什么重要 |
|---|---|---|
| `ΔQ` | Strong 与 Weak 的总体质量差 | 判断大小模型差异是否足够大 |
| `disagreement` | 不同模型逐题答案/得分不一致比例 | 判断是否存在逐题路由空间 |
| `upgrade_opportunity` | Weak 错、Strong 对的比例 | 衡量“该升级但不能升级错”的机会 |
| `downgrade_opportunity` | Weak 对、Strong 也对/更强模型无额外收益的比例 | 衡量“可以安全下放”的机会 |
| `winner_diversity` | 不同模型在逐题上的最优比例 | 判断模型池是否存在互补性 |
| `cost_gap` | 模型间真实/冻结成本差 | 决定路由节省是否可见 |

**第一原则：优先选择“模型差异大 + 逐题互补性强 + 成本差明显”的 benchmark，而不是单纯选择最难的 benchmark。**

---

## 二、最终 Benchmark 架构：三层组成，而不是一个数据集包打天下

LLooM RouterBench v0.1 采用以下结构：

```text
LLooM RouterBench
│
├── Track A · Public Matrix Track
│   ├── LLMRouterBench（优先）
│   └── RouterBench（兼容/回退）
│
├── Track B · LLooM Native Pool Track  ← 正式主结果
│   ├── MMLU-Pro
│   ├── GSM8K
│   ├── MATH
│   ├── MBPP
│   └── IFEval
│
└── Track C · Showcase / Stress Track
    ├── MT-Bench       ← 展示质量-成本 Pareto
    ├── GPQA Diamond   ← 强模型升级场景
    └── Long Context   ← 后续验证 context-window gate
```

### 2.1 Track A：Public Matrix Track

用途不是替代正式 LLooM 结果，而是：

1. **零 API 成本快速验证 RouterBench 基建**；
2. 对照已有 routing 研究的评测口径；
3. 在正式真实池数据完成前先打通 UI、AIQ、Pareto、逐实例回放。

RouterBench 原始工作提供超过 405k inference outcomes，并覆盖 MMLU、GSM8K、MBPP、HellaSwag、Winogrande、ARC、MT-Bench、RAG 等来源 benchmark；逐实例数据包含模型预测、performance、cost 等字段，非常适合作为 frozen outcome matrix 的底座。citeturn857815academia12

但它有一个重要限制：模型池和价格口径属于较早期的公开实验条件，因此**不得直接把 RouterBench 的历史价格和 LLooM 当前模型池结果混成一张正式排行榜**。Public Track 应保留独立的 `dataset_id`、`model_pool_id` 和 `cost_basis`。

LLMRouterBench 是更值得优先适配的新一代公共来源：目前公开框架整理了 33 个模型、21+ 数据集、40 万+ 实例，并同时覆盖 performance-only 与 performance-cost routing，逐实例字段也已经标准化为 query / prediction / ground truth / score / token / cost。citeturn453050search1turn453050search6

因此数据源优先级改为：

```text
LLMRouterBench public matrix
        ↓ 失败/无法精确映射
RouterBench public matrix
        ↓ 再失败
LLooM Native Pool 自采
```

这里“优先”指**优先作为公共数据适配层**，不意味着一定成为 LLooM 的最终主结果。

---

## 三、Track B：LLooM Native Pool Track 是正式主结果

### 3.1 为什么要做 Native Pool

Public Matrix 最大的问题是：

```text
数据里的模型
≠
LLooM 当前真实候选模型
```

如果最终 README 写成“LLooM 路由节省 38% 成本”，却是拿旧模型、旧价格、旧能力注册表跑出来的，这个数字容易被质疑。

因此正式结果必须存在一条：

```text
当前 LLooM 模型池
        ×
固定 benchmark
        ×
固定 query set
        ↓
frozen outcome matrix
        ↓
LLooM offline replay
```

真实池只需要**首次采集一次**，采集完成后所有 router、参数、UI、对照实验均在 frozen matrix 上离线进行，不再重复消耗 API。

### 3.2 正式 Core Benchmark

第一版建议：

| Dataset | 主要能力 | 自动评分 | 路由价值 | 角色 |
|---|---|---:|---:|---|
| **MMLU-Pro** | 综合知识 + reasoning | ✓ | ★★★★★ | Core 主力 |
| **GSM8K** | 数学推理 | ✓ | ★★★★☆ | Core 主力 |
| **MATH** | 高难数学推理 | ✓ | ★★★★☆ | Core 主力 |
| **MBPP** | 代码生成 | ✓ | ★★★★☆ | Core 主力 |
| **IFEval** | instruction following | ✓ | ★★★★☆ | Core 主力 |

MMLU-Pro 超过 12,000 道题、覆盖 14 个领域，并以十选一和更强 reasoning 要求提高区分度，适合作为综合路由任务；IFEval 则以可验证 instruction 为主，约 500+ prompts、25 类指令约束，可程序化评分、不需要 LLM judge。citeturn453050search5turn453050search3

但这里不要求“全部数据一次跑完”。v0.1 先做一个**可完整复现的小型标准集**：

```text
MMLU-Pro    400
GSM8K       400
MATH        400
MBPP        300
IFEval      300
----------------
总计       1,800
```

如资源允许，再扩至约 2,500–3,000 条。

### 3.3 Showcase / Stress Track

#### MT-Bench：专门做展示

MT-Bench 是最适合做“质量-成本权衡”可视化的 benchmark 之一；RouteLLM 已经直接使用 MT-Bench、MMLU、GSM8K 验证强/弱模型 routing，并报告在保持约 95% strong-model 表现时显著降低调用 strong model 的比例，其中 MT-Bench 的节省幅度尤其突出。citeturn857815search0

因此 MT-Bench 不必承担全部科学严谨性，而要承担：

```text
第一眼让评审看到：
“LLooM 没有把所有请求都交给最强模型。”
```

#### GPQA Diamond：强模型升级场景

它适合作为“何时必须升级”的 hard track。它不一定适合成为成本节省主战场，因为极难问题如果普遍需要 strong model，router 很容易退化成大量升级。

#### Long Context：后续独立 Track

用于专门验证当前 router 的 `context_window` / 长上下文能力门槛。不要在 v0.1 Core 中混入，否则会把“能力路由”和“上下文资格门控”搅在一起。

---

## 四、固定模型池：先做三模型，再扩展多模型

### 4.1 v0.1 主模型池

第一版正式结果使用固定三层池：

```text
Weak   → 低成本、基础能力
Mid    → 中成本、中高能力
Strong → 高成本、最高能力
```

要求不是机械地按参数大小选择，而是同时满足：

1. 三者成本梯度清晰；
2. Strong 相对 Weak 有明显能力增益；
3. Mid 不能完全被 Strong 在所有样本上支配；
4. 逐题存在可观模型互补；
5. 每个模型在 Core benchmark 上都有完整 frozen outcomes。

### 4.2 为什么不直接把当前所有模型全部放进去

LLooM 当前 registry 已经包含多个不同价格和能力层级模型。如果把所有模型一口气加入，可能产生两类问题：

- 某些模型被其他模型完全支配，route 根本没有选择它们的合理场景；
- 成本归一化、模型池 median cost、capability tier 等现有评分项的相互作用变复杂，难以解释第一版结果。

因此：

> **v0.1 用一个经过筛选的 3-model pool 验证 routing 核心；v0.2 再做 5–7 model pool，研究模型池规模与模型多样性。**

### 4.3 Public Matrix 中的虚拟模型

Public Track 可以用：

```text
virtual:<source-model>
```

避免污染 LLooM 真实 `models` 表。

必须把以下信息冻结到 `model_pool_manifest`：

```text
model_name
capability_tier
context_window
cost_basis
price_spec
source_dataset
source_model_name
```

虚拟模型是**benchmark 适配层**，不是生产模型。

---

## 五、数据核心：Frozen Outcome Matrix

### 5.1 数据模型

RouterBench 的真正核心不是“某数据集有多少题”，而是下面这个矩阵：

```text
                 query_001   query_002   query_003   ...
Weak                1/$$       1/$$       0/$$
Mid                 1/$$       0/$$       1/$$
Strong              1/$$       1/$$       1/$$
```

即：

```text
(sample_id, model_name)
        ↓
score + cost + optional response/latency
```

### 5.2 推荐数据表

不再把 dataset、model outcome、run summary 全塞进一个 `bench_results`，而采用四表结构：

```sql
CREATE TABLE IF NOT EXISTS bench_datasets (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT UNIQUE NOT NULL,
  source TEXT NOT NULL,                 -- lmmrouterbench | routerbench | lm_eval
  version TEXT,
  cost_basis TEXT NOT NULL,             -- source_price | lloom_current_price
  meta_json TEXT,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS bench_instances (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  dataset_id INTEGER NOT NULL REFERENCES bench_datasets(id),
  sample_id TEXT NOT NULL,
  task_family TEXT NOT NULL,
  task_type TEXT NOT NULL,
  band_hint TEXT,
  prompt TEXT NOT NULL,
  ground_truth TEXT,
  meta_json TEXT,
  UNIQUE(dataset_id, sample_id)
);

CREATE TABLE IF NOT EXISTS bench_model_results (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  dataset_id INTEGER NOT NULL REFERENCES bench_datasets(id),
  sample_id TEXT NOT NULL,
  model_name TEXT NOT NULL,
  split TEXT NOT NULL,                  -- calibration | test
  score REAL NOT NULL,
  cost REAL NOT NULL,
  latency_ms REAL,
  response TEXT,
  tokens_in INTEGER DEFAULT 0,
  tokens_out INTEGER DEFAULT 0,
  meta_json TEXT,
  UNIQUE(dataset_id, sample_id, model_name)
);

CREATE TABLE IF NOT EXISTS bench_runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  dataset_id INTEGER NOT NULL,
  model_pool_id TEXT NOT NULL,
  strategy TEXT NOT NULL,
  split TEXT NOT NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  summary_json TEXT
);

CREATE TABLE IF NOT EXISTS bench_run_items (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id INTEGER NOT NULL REFERENCES bench_runs(id),
  sample_id TEXT NOT NULL,
  selected_model TEXT NOT NULL,
  score REAL NOT NULL,
  cost REAL NOT NULL,
  oracle_model TEXT,
  oracle_score REAL,
  regret REAL,
  decision_json TEXT
);
```

### 5.3 必须新增的“选择证据”字段

`decision_json` 至少保存：

```text
candidate_models
capability_gate
cost_gate
band
quality_estimate
selection_score
selection_probability
selected_model
fallback_reason
```

这样 C10 决策快照和 benchmark 的逐题分析天然打通。

---

## 六、最重要的可信度设计：Calibration/Test 严格隔离

### 6.1 不能把测试集答案用于路由决策

这是本方案最重要的修正之一。

错误方式：

```text
整个 benchmark
 ↓
计算各模型真实 test accuracy
 ↓
灌进 quality_override
 ↓
测试 router
```

这样相当于把答案泄露给 router。

正确方式：

```text
benchmark
├── calibration 20%
│      ↓
│  产生质量历史/quality EWMA
│
└── test 80%
       ↓
   LLooM plan()
       ↓
   选择模型
       ↓
   查冻结结果
       ↓
   evaluator
```

### 6.2 Calibration 应模拟生产系统

不能直接“把 calibration accuracy 写进 quality_override”。更推荐新增 offline calibration adapter：

```text
bench calibration outcomes
        ↓
RoutingFeedbackStore / 离线等价层
        ↓
EWMA / sample_count
        ↓
quality_override
        ↓
PlanInput
```

这样 benchmark 学到的是 LLooM 真实会使用的统计状态，而不是一个 benchmark 专用作弊接口。

### 6.3 Calibration/Test 推荐比例

v0.1：

```text
20% calibration
80% test
```

并且所有 headline 结果只能来自 `test`。

数据集筛选本身也尽量只用 calibration/screening split：

> **先看模型互补性选数据集，再把 test split 完全冻结。**

---

## 七、零成本回放：让 LLooM 真正“介入”而不是重新跑模型

### 7.1 核心流程

```text
               Frozen Outcome Matrix
                       │
                       │ lookup
                       ▼
Query ──→ LLooM routing ──→ selected_model ──→ (score, cost)
              │
              ├─ task_type
              ├─ band
              ├─ capability gate
              ├─ cost gate
              ├─ quality EWMA
              └─ plan()
```

整个 replay 过程：

```text
禁止调用 ai_client
禁止产生真实 usage_records
禁止修改 production models
禁止污染 calibration history
```

**“零成本”精确定义为：回放阶段没有任何 LLM/API 调用；模型响应来自预先冻结的 outcome matrix。**

### 7.2 为什么主入口应优先是 `plan()`，而不是直接调用完整 `route()`

当前 `route()` 包含：

```text
classify
→ band_for
→ policy
→ PlanInput
→ plan
```

而 classifier 在某些情况下可以调用真实模型。因此第一版 RouterBench 主评测定义：

> **Core = Plan Benchmark**：用 benchmark 已知的 task metadata / deterministic mapping 提供 `task_type` 与必要的 `band context`，重点验证 LLooM 的模型选择机制。

然后再增加：

> **System = End-to-End Offline Route Benchmark**：使用 rule-only 或已缓存 classifier decision 回放完整 route 链，测分类误差对最终 routing 的影响。

这样可以清楚区分：

```text
Router 本身选择得好不好？
vs
完整系统从 query → model 的整个链路表现如何？
```

---

## 八、Benchmark Baselines：不是只和“最强模型”比较

v0.1 至少包含以下基线：

| Baseline | 策略 | 用途 |
|---|---|---|
| Always-Weak | 永远选 Weak | 低成本下界 |
| Always-Mid | 永远选 Mid | 中间基线 |
| Always-Strong | 永远选 Strong | 质量参考上界、成本上限 |
| Cheapest | 每题只考虑最低成本模型 | 极简成本策略 |
| Random | 固定随机概率 | 无智能 routing 基线 |
| kNN / 相似度路由 | 基于 calibration 样本预测模型偏好 | 传统 router baseline |
| Oracle | 知道 test 中各模型真实 score 后逐题选择 | hindsight 理论上界 |
| **LLooM** | 当前真实 `plan()` | 主结果 |

### 8.1 Oracle 必须明确标注

Oracle 是 hindsight upper bound，不属于可学习/可部署策略。

它的意义只有一个：

```text
LLooM 与理论最优逐题路由还有多远？
```

### 8.2 不再设置“LLooM 必须位于 Cheapest 与 Best-Single 之间”这种硬 gate

这是一个需要主动修正的地方。

路由本质上可能出现：

```text
LLooM 更便宜但质量低
LLooM 更贵但质量更高
LLooM 与某 fixed model 在某数据集上持平
```

不能为了“让图好看”人为定义成功形状。

正确 gate 应该是：

1. 所有 baseline 都完整出数；
2. LLooM 的 decision 可解释；
3. LLooM 的质量 / 成本 Pareto 点真实存在；
4. 若不如某 baseline，必须能定位原因；
5. 在具有 routing headroom 的 benchmark 上，再观察是否取得统计显著的改善。

---

## 九、核心评价指标

### 9.1 Quality

```text
Q = mean(score_i)
```

不同 benchmark 使用各自官方 scorer，最终归一化到 `[0,1]`。

### 9.2 Average Cost

```text
C = mean(cost_i)
```

Public Track 与 Native Track 不混用 cost basis。

### 9.3 Cost Saving

相对 Always-Strong：

```text
Saving = 1 - C_router / C_strong
```

### 9.4 Quality Retention

```text
Retention = Q_router / Q_strong
```

### 9.5 Gap Recovery

这是展示 routing 价值的核心指标之一：

```text
GapRecovery =
(Q_router - Q_weak)
──────────────────
(Q_strong - Q_weak)
```

它回答：

> 相比一直使用 Weak，router 恢复了 Strong 与 Weak 能力差距中的多少？

### 9.6 Oracle Regret

```text
Regret_i = OracleScore_i - RouterScore_i
```

再统计：

```text
Mean Regret
95th percentile Regret
```

### 9.7 Strong-call Ratio

```text
StrongRatio = strong_selected / total
```

对现场展示非常直观。

### 9.8 AIQ

保留现有 `aiq_replay.py` 的 AIQ 定义与凸包思路，但把实现重构成纯函数，并以**共同成本区间**进行比较。

AIQ 不作为唯一 headline，而作为 RouterBench 的综合质量-成本指标。RouterBench 的原始工作已经把 routing 质量与成本的 Pareto/AIQ 作为核心评价框架，因此 LLooM 继续采用这一口径，便于横向对照。citeturn857815academia12

### 9.9 Confidence Interval

每个核心数字至少提供：

```text
mean
95% bootstrap CI
```

特别是 AIQ、Gap Recovery、Cost Saving。

这样可以满足学术展示的严谨性要求，又不影响 WebUI 的直观性。

---

## 十、Benchmark 选型与筛选流程

不再先拍脑袋决定“用 MMLU-Pro / MT-Bench”，而是增加一个 screening 阶段。

### 10.1 Candidate Pool

先收集：

```text
MMLU-Pro
MMLU
GSM8K
MATH
MBPP
HumanEval
IFEval
BBH
GPQA Diamond
MT-Bench
Arena-Hard
SimpleQA
LiveCodeBench
```

然后按以下维度统计候选数据集：

```text
model_gap
error_disagreement
upgrade_opportunity
winner_diversity
cost_gap
scorer_reliability
inference_cost
```

### 10.2 选出三个等级

```text
Tier S · Showcase
    最容易产生漂亮 Pareto / cost saving

Tier A · Formal Core
    综合、可自动评分、可稳定复现

Tier B · Stress
    极难 / 长上下文 / agent 化场景
```

目前建议：

```text
Showcase → MT-Bench
Core     → MMLU-Pro + GSM8K + MATH + MBPP + IFEval
Stress   → GPQA / Long Context
```

但在真实池第一次采集后，允许依据**独立 screening split** 调整具体样本数量和最终排序。

### 10.3 最核心的“Routing Headroom”筛选

对二模型候选池，先计算：

```text
H = P(Weak=0, Strong=1)
  + λ · P(Weak=1, Strong=0)
```

其中第二项代表模型互补性，`λ` 由实验设置确定。

对三模型以上，可统计：

```text
P(model_i is best)
```

的分布熵，防止某个模型在几乎所有样本上都成为唯一 winner。

**这一步是 LLooM RouterBench 与普通模型 benchmark 最大的区别。**

---

## 十一、两条数据路线

### 11.1 路线 A：Public Matrix Import

目标：尽快让 RouterBench 跑起来。

建议目录：

```text
benchmarks/routerbench/
├── adapters/
│   ├── lmmrouterbench.rs
│   └── routerbench.rs
├── manifests/
│   ├── public_llmrouterbench.yaml
│   └── public_routerbench.yaml
├── import/
│   └── import_frozen_matrix.py
└── data/
```

导入器职责：

```text
source output
→ canonical schema
→ dataset/model manifest
→ calibration/test assignment
→ SQLite
```

**禁止在导入阶段修改原始 score；需要换算时保留 raw + normalized 两列。**

### 11.2 路线 B：Native Pool 自采

采用 `lm-eval-harness`，原因是它支持 `--log_samples`，可把逐实例预测保存下来，并能够通过 OpenAI-compatible endpoint 连接 LLooM /v1。用户现有规划中已经验证这条接入路径。fileciteturn0file0L43-L48

采集方式：

```text
对每个 model 固定直连
        ↓
跑同一 dataset / same prompt set
        ↓
保存 response
        ↓
官方 scorer
        ↓
score
        ↓
LLooM 当前价格体系换算 cost
        ↓
写入 frozen matrix
```

注意：

> **自采阶段测的是裸模型，不经过 LLooM router。**

只有完成所有模型的 frozen matrix 之后，才把 LLooM 接进来。

### 11.3 真实端到端演示只做少量样本

可以额外准备：

```text
LLooM /v1 + model=auto
```

但它属于“现场闭环演示”，不属于正式 zero-cost RouterBench 结果。

正式结果必须来自 frozen replay。

---

## 十二、工程结构

建议把 benchmark 从 `scripts/aiq_replay.py` 的孤立脚本，提升为独立模块：

```text
benchmarks/
└── routerbench/
    ├── README.md
    ├── manifests/
    │   ├── model_pool.yaml
    │   ├── task_mapping.yaml
    │   └── datasets.yaml
    │
    ├── adapters/
    │   ├── lmmrouterbench.py
    │   ├── routerbench.py
    │   └── lm_eval.py
    │
    ├── import_frozen.py
    ├── screening.py
    ├── score.py
    ├── baselines.py
    ├── report.py
    └── data/

crates/lloom-core/src/
└── bench.rs
```

### 12.1 `bench.rs` 的职责

只做四件事：

```text
1. 构造与生产一致的 PlanInput
2. 调用现有 plan()/score_all() 决策层
3. 查 frozen outcome
4. 记录 bench_run / bench_run_item
```

不要把 benchmark 专用逻辑塞进 `router.rs`。

### 12.2 决策逻辑唯一来源

这是工程红线：

```text
production route()
        └── plan()
              └── score_all()

benchmark replay
        └── plan()
              └── score_all()
```

而不是：

```text
route()
      ↓
一套逻辑

benchmark
      ↓
复制另一套路由公式
```

Benchmark 允许构造离线 `PlanInput`，但**不能复制 selection formula**。

---

## 十三、LLooM RouterBench CLI

建议固定为：

```bash
lloom-cli bench list
lloom-cli bench import --source llmrouterbench --dataset <name>
lloom-cli bench import --source routerbench --dataset <name>
lloom-cli bench screen --dataset <name> --pool <pool>
lloom-cli bench calibrate --dataset <name> --pool <pool>
lloom-cli bench replay --dataset <name> --pool <pool> --strategy all
lloom-cli bench compare --run <run_id>
lloom-cli bench export --run <run_id> --format json
```

其中：

```text
bench screen
```

负责判断 benchmark 是否有 routing headroom；

```text
bench calibrate
```

负责构造 calibration 状态；

```text
bench replay
```

负责真正的 zero-cost evaluation。

---

## 十四、WebUI：Benchmark 页面只是 RouterBench 的可视化出口

WebUI 不再作为独立“漂亮页面”开发，而以 RouterBench 数据模型为唯一数据源。

页面结构建议：

```text
┌────────────────────────────────────────────┐
│ LLooM RouterBench                          │
│ Dataset · Model Pool · Split · Run        │
├────────────────────────────────────────────┤
│ Quality  Cost Saving  Retention  AIQ      │
├────────────────────────────────────────────┤
│ Quality × Cost Pareto                     │
├────────────────────────────────────────────┤
│ Model Selection Distribution               │
├────────────────────────────────────────────┤
│ Baseline Comparison                        │
├────────────────────────────────────────────┤
│ Per-instance Decision Explorer              │
└────────────────────────────────────────────┘
```

### 14.1 第一屏

只出现：

```text
Always-Strong
Always-Weak
Cheapest
LLooM
Oracle
```

以及：

```text
Quality
Cost
Saving
Retention
AIQ
```

### 14.2 第二屏：解释 LLooM 如何做选择

例如：

```text
题目：GSM8K #142

LLooM → Weak
score estimate = 0.84
cost = $0.0004
实际结果 = correct

如果 Always-Strong：$0.0047
本题节省：91.5%
```

然后：

```text
题目：MATH #88

LLooM → Strong
Weak 历史质量不足
upgrade reason = band=hard
```

### 14.3 第三屏：决策快照

显示：

```text
candidate models
↓
capability gate
↓
quality
↓
cost
↓
selection score
↓
selected model
```

这直接将 C10 从“日志特性”转化成 routing 的可解释证据。

---

## 十五、演示动线：先证明“为什么应该路由”，再证明“LLooM 怎么路由”

### Step 1 · 先看模型池

展示三个模型：

```text
Weak   → cheap
Mid    → medium
Strong → expensive
```

随后出现：

```text
同一个 benchmark 上，模型表现并不相同。
```

### Step 2 · 展示 RouterBench headroom

先给出：

```text
Weak vs Mid vs Strong
逐题 performance disagreement
```

让评审看到：

> 不是为了 benchmark 而做 routing；数据本身确实存在“有些题用小模型就够，有些题必须升级”的空间。

### Step 3 · 展示 Pareto

```text
Always Weak
Always Mid
Always Strong
Cheapest
LLooM
Oracle
```

重点讲：

> LLooM 不追求所有题都跑 Strong，而是在质量下降可接受的范围内，把更多请求放到便宜模型。

### Step 4 · 点开单题

让评审看到真实 frozen outcome：

```text
LLooM 选择谁
为什么
实际 score
实际 cost
相比 Strong 节省多少
```

### Step 5 · 实时 replay

随机抽 10 条 query，让 `plan()` 连续产生选择；但仍不调用 LLM。

这一步特别适合现场演示：

```text
query
→ LLooM decision
→ frozen outcome lookup
→ chart update
```

真正做到“看起来像在线路由，实际上零 API 成本”。

### Step 6 · 附录讲严谨性

最后再讲：

```text
Calibration/Test split
Frozen model pool
No test leakage
Bootstrap CI
Oracle upper bound
Cost basis
```

---

## 十六、A 类快速优化：哪些应该做，哪些暂时不要做

本阶段产品优化不是独立目标，只做对 RouterBench 有直接贡献的功能。

### A1 · C10 决策快照：必须做

原因：

```text
Benchmark 单题解释
        ↓
需要知道 LLooM 当时为什么选这个模型
```

同时让 bench replay 的 decision_json 与生产日志共用同一结构。

### A2 · 计价不变量修复：必须做

原因：

```text
RouterBench 的 headline 有成本数字
```

任何 cost inconsistency 都会直接破坏可信度。

### A3 · C12 classifier 配置：次优先

有助于线上产品展示，但**不是 v0.1 RouterBench 核心**。

### A4 · E1 latency：延后到 v0.2

当前 `score_all()` 的 latency 信号仍未作为第一版核心路由指标。先避免把 latency 与 quality-cost 问题混在一起。

### A5 · WebUI RouterBench：必须做

但先做数据驱动的四张图，不做复杂动画：

1. Pareto / AIQ；
2. Baseline comparison；
3. Selection distribution；
4. Per-instance explorer。

---

## 十七、验收 Gate：以“可信 + 可复现 + 能解释”为第一标准

### Gate 1 · 数据完整性

```text
每个 test sample
×
每个 model
都有 frozen outcome
```

缺项不允许进入 headline 结果。

### Gate 2 · 无数据泄漏

单测确认：

```text
test score
不能进入
quality_override / calibration state
```

### Gate 3 · 决策一致性

同一 `PlanInput`：

```text
production plan()
===
benchmark replay plan()
```

模型选择结果必须一致。

### Gate 4 · Baseline 完整

至少：

```text
Weak / Mid / Strong / Cheapest / Random / Oracle / LLooM
```

### Gate 5 · Headroom 先验成立

正式 benchmark 在进入 headline 前，应先满足：

```text
ΔQ 明显
AND
逐题 disagreement 足够
AND
cost gap 明显
```

否则直接作为 sanity benchmark，而不是主战场。

### Gate 6 · 统计报告完整

至少提供：

```text
Quality
Average Cost
Cost Saving
Quality Retention
Gap Recovery
AIQ
Oracle Regret
95% CI
```

### Gate 7 · WebUI 真数据

页面不能出现 mock 曲线；所有图均从 `bench_runs + bench_run_items` 生成。

### Gate 8 · 零成本回放

```text
replay run
→ 0 API calls
→ 0 usage_records mutation
→ 0 production model mutation
```

---

## 十八、排期：重新按 RouterBench 主线排，而不是先把产品细节全部做完

### Phase 0 · Benchmark Spec / Screening（1–2 天）

```text
确定 frozen schema
确定 model pool candidate
确定 task mapping
确定 calibration/test split
对候选 benchmark 做 headroom screening
```

产物：

```text
RouterBench v0.1 spec
model_pool.yaml
datasets.yaml
task_mapping.yaml
```

### Phase 1 · Public Matrix Replay MVP（2–3 天）

```text
B1 schema
B2 public importer
B3 bench.rs
baselines
AIQ evaluator
CLI
```

目标：

```text
RouterBench subset
→ LLooM replay
→ 完整 baseline
→ 第一张 Pareto 图
```

### Phase 2 · Native Pool Frozen Matrix（2–3 天）

```text
lm-eval
→ 3 models
→ Core 5 datasets
→ log_samples
→ official scoring
→ import
```

目标：建立真正属于 LLooM 的正式数据源。

### Phase 3 · Calibration/Test + Leakage Tests（1–2 天）

```text
offline calibration
quality EWMA
plan parity
no leakage
bootstrap CI
```

这是正式 benchmark 的可信度关口。

### Phase 4 · WebUI / Presentation（2 天）

```text
Pareto
AIQ
selection distribution
per-instance explorer
C10 snapshot
```

### Phase 5 · E2E Offline + Showcase（1–2 天）

```text
rule-only / cached classifier
MT-Bench showcase
10-query live replay
```

### Phase 6 · Optional（后续）

```text
5–7 model pool
kNN / RouteLLM baseline
latency-aware routing
long-context track
LLMRouterBench full adapter
```

整体仍可控制在原规划约 10–13 个工作日，但**核心产出顺序变为“数据 → replay → 验证 → 展示”**，而不是“产品 polish → 再找 benchmark”。

---

## 十九、风险与处理原则

| 风险 | 处理 |
|---|---|
| Public Matrix 拉取失败 | LLMRouterBench → RouterBench → Native Pool 自采三级回退 |
| Public 模型与 LLooM 当前池不一致 | Public Track 与 Native Track 分开，不混 headline |
| 平均分差大但 routing headroom 小 | screening 阶段淘汰，不作为 Showcase 主数据集 |
| 虚拟模型 quality 失真 | 只用于 public track；正式结论优先 Native Pool |
| 某模型没有完整逐题结果 | 不进入正式 test matrix；不得用 aggregate score 填补 test outcome |
| classifier 调用真实 LLM | Core 走 `plan()`；E2E Offline 只允许 rule-only/cached decision |
| benchmark replay 与 route() 漂移 | `PlanInput` / `plan()` 共用；增加 decision parity test |
| AIQ 与现有 Python 口径不一致 | 统一纯函数 scorer，并加入 golden fixtures |
| cost basis 混乱 | dataset manifest 强制声明 `cost_basis` |
| LLooM 某 benchmark 输给 baseline | 不隐藏，使用逐题 selection / regret 定位原因 |
| UI 为追求视觉效果误导结果 | 所有数字从 frozen matrix + run 明细实时聚合，不手写展示数据 |

---

## 二十、与现有文档族的关系

本文件将原 00–08 文档中的多个方向重新收束到 RouterBench 主线，但不改变其基础设计。

```text
00–08
  │
  ├── routing core / plan()
  ├── C10 decision snapshot
  ├── cost invariants
  ├── classifier config
  └── existing AIQ replay
         │
         ▼
09 · LLooM RouterBench
  ├── frozen matrix
  ├── calibration/test
  ├── offline replay
  ├── baselines
  ├── AIQ / Pareto
  └── WebUI
```

其中：

- 原 `aiq_replay.py` 不再承担“完整 benchmark 系统”，而降级为 AIQ 计算实现参考；
- 原 `routing_calibration` 继续服务生产质量反馈，但 benchmark 使用独立 `bench_*` 表，避免相互污染；
- 原 C10 由“生产日志”扩展为“生产 + benchmark 共用决策证据”；
- 原 B1–B5 保留，但重新排序为“数据源 → frozen matrix → replay → evaluation → UI”；
- 原计划中的真实池自采不再只是补充演示，而是**正式 Native Track 的数据生成阶段**。

---

## 二十一、最终版本定义：LLooM RouterBench v0.1 应该是什么

一句话定义：

> **LLooM RouterBench v0.1 是一个固定模型池、冻结逐实例模型结果、严格 calibration/test 隔离、复用 LLooM `plan()` 决策层的 zero-inference-cost LLM routing evaluation framework。**

它最终形成如下闭环：

```text
               ┌─────────────────────┐
               │  Benchmark Dataset   │
               └──────────┬──────────┘
                          │
                          ▼
              ┌───────────────────────┐
              │ Frozen Outcome Matrix │
              │ query × model         │
              │ score + cost          │
              └──────────┬────────────┘
                         │
             ┌───────────┴───────────┐
             │                       │
             ▼                       ▼
      Calibration Split         Test Split
             │                       │
             ▼                       │
       Quality EWMA                 │
             │                       │
             └──────────┐            │
                        ▼            │
                  LLooM plan() ◄────┘
                        │
                        ▼
                 selected_model
                        │
                        ▼
                frozen result lookup
                        │
                        ▼
             ┌─────────────────────┐
             │ Quality / Cost / AIQ│
             │ Baseline / Oracle  │
             │ CI / Per-instance  │
             └──────────┬──────────┘
                        │
                        ▼
                 WebUI / Report
```

最终不是证明：

```text
“LLooM 能调用不同模型。”
```

而是证明：

```text
“在存在真实模型互补性的 workload 上，
LLooM 能用更便宜的模型处理更多本来不需要强模型的请求，
并把有限的 Strong 调用集中到真正有价值的样本上。”
```

这才是 LLooM 当前阶段最值得展示、也最容易继续发展成正式 routing research 的核心证据。

---

## 二十二、评审后第一开工项

评审通过后，第一步不直接写 WebUI，也不先跑大规模 API。

**先完成 `RouterBench v0.1 Spec + Screening Tool + Frozen Outcome Schema`。**

具体顺序：

```text
1. 建 model_pool.yaml
2. 建 datasets.yaml
3. 建 task_mapping.yaml
4. 导入一个 Public Matrix 子集
5. 计算 ΔQ / disagreement / upgrade opportunity / winner diversity
6. 选出正式 Core + Showcase
7. 打通 bench replay → plan() → frozen lookup
8. 再开始 Native Pool 自采
```

只有当这一步已经证明：

```text
“这个 workload 确实存在 routing headroom”
```

才值得投入后面的真实池 API 成本与 WebUI 展示打磨。
