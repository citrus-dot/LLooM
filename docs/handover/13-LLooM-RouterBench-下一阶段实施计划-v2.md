# 13 · LLooM RouterBench 下一阶段实施计划 v2

> 日期：2026-10-06  
> 基线：LLooM `v2` / RouterBench Public Track v0.1 / Day 1–5 完成态  
> 定位：**零 API 成本优先、快速优化 Weak 利用、尽可能复用现有公开 benchmark 与预计算 model outcomes 的下一阶段实施规格**  
> 上游：`09-LLooM RouterBench 构建与立项演示方案` / `10-LLooM RouterBench 实施规格 v0.1` / `12-RouterBench 实施总结-Day1-5` / `13-LLooM-RouterBench-下一阶段实施计划` / `bench_report.md`

---

## 一、这版方案相对上一版的核心调整

上一版已经把问题定位为：

```text
Oracle：Weak 43.4% / Mid 38.3% / Strong 18.3%
LLooM：Weak  0.0% / Mid 18.0% / Strong 82.0%
```

因此“让 LLooM 学会利用 Weak”仍然是唯一的优化主线。

但上一版仍把 Native 自采放在比较靠前的位置。本版根据当前成果重新收束：

> **下一阶段默认不花钱、不采集新模型结果、不急于跑 Native。先把已经存在的 RouterBench / LLMRouterBench / RouteLLM 预计算结果全部转化成可复用的 routing evaluation backend，并利用它们在零成本条件下搜索模型池、诊断 Weak 0% 的根因、验证 Soft Gate 和轻量 query-level router。只有零成本阶段已经跑出稳定增益后，Native 才作为可选最终验证。**

本版的优先级从：

```text
Error taxonomy
→ Soft Gate
→ Native
```

调整为：

```text
Public Matrix 扩容
→ Pool Search
→ Error Taxonomy
→ Band/Quality/Cost Ablation
→ Soft Weak Gate
→ 轻量 Query-level Router
→ Cross-Benchmark 复现
→ 再决定是否需要 Native
```

---

# 二、阶段目标重新定义

本阶段只有两个硬目标：

### Goal A · 快速解决 Weak 0%

不是追求：

```text
Weak Selection Rate 从 0% 变成 30%
```

而是：

```text
Weak Recall ↑
Weak Precision 保持高
Cost Saving ↑
Quality Retention 不显著下降
```

目标是让 Weak 成为一个**真实可竞争的候选模型**。

### Goal B · 全程尽量零 API 成本

下一阶段主线允许的数据来源：

```text
已有 frozen matrix
已有公开 model outcomes
已有预计算 benchmark cache
本地 CPU / 内存计算
```

禁止作为主线：

```text
新 API 推理
重新让 3/5/10 个模型跑 benchmark
为 screening 花钱
为了 tuning 反复采集模型结果
```

---

# 三、最重要的新判断：不要把“Weak”误解成“最弱模型”

当前 Public Track 中：

```text
Mistral-7B      Q≈0.35
Yi-34B          Q≈0.63
GPT-4           Q≈0.76
```

当前 Weak 之所以 0% 入选，不一定全部是 router 的错。

还有一个可能性：

> **当前所谓 Weak 本身太弱，以至于即使放开竞争资格，也很少有 query 值得选它。**

因此下一阶段必须把：

```text
weak = 参数最小 / 能力最低模型
```

改成：

```text
weak = 低成本候选层
```

即 routing 真正关心的是：

> **“这个模型是不是足够便宜，同时在一部分 query 上已经足够好？”**

而不是：

> “它是不是整个池里参数最少？”

这意味着 **Model Pool Search 本身就是 Weak 优化的一部分。**

---

# 四、零成本数据源策略：优先吃干净现有结果

## 4.1 Source A · LLMRouterBench：下一阶段主数据源

LLMRouterBench 当前公开统一了：

```text
33 models
21+ datasets
400K+ instances
```

并提供预收集 benchmark results，结果字段包含：

```text
origin_query
prompt
prediction
score
ground_truth
prompt_tokens
completion_tokens
cost
```

其 Performance-Cost setting 当前包含 13 个模型、10 个 benchmark；包括 AIME、LiveMathBench、LiveCodeBench、SWE-Bench、GPQA、HLE、MMLU-Pro、SimpleQA、ArenaHard、τ²-Bench。citeturn446241view0turn416699search2

**这应该成为下一阶段零成本优化的第一数据源。**

尤其重要的是：它已经预先付过模型 inference 成本，所以 LLooM 不需要再次调用这些模型。

---

## 4.2 Source B · RouterBench 0-shot：继续保留

当前已经下载并成功导入：

```text
36,497 instances
401,467 outcomes
11 models
```

RouterBench 官方数据本身就是“超过 30,000 prompts + 11 个 LLM response + cost + performance”，覆盖 MMLU、GSM8K、MBPP、Winogrande、HellaSwag、MT-Bench 等。citeturn416699search4turn416699search8

它作为：

```text
Legacy / compatibility track
```

继续保留。

不要因为它是 2024 数据就丢掉：

> 它最大的价值已经从“最终 benchmark”变成“快速验证 router 改动是否有效”。

---

## 4.3 Source C · RouteLLM cached results：2-model sanity track

RouteLLM 自带 MMLU / GSM8K / MT-Bench evaluation，并且官方仓库说明这些 benchmark 的结果已经缓存；默认 strong/weak pair 是 GPT-4-1106-preview 与 Mixtral-8x7B-Instruct。citeturn416699search7turn446241view2

这个数据源只承担：

```text
2-model routing sanity check
```

目的：

```text
验证 LLooM 的 Weak-first / strong-escalation 思路
与已有 RouteLLM 评价口径是否方向一致
```

不作为 LLooM 主 headline。

---

## 4.4 Source D · RouterArena：只做 future external validation

RouterArena 当前提供 evaluation-only benchmark，覆盖 9 个 domains / 44 categories，并有 accuracy、cost、optimality、robustness、latency 等 router-specific metrics。它明确禁止在该数据上训练、fit 或 tune router。citeturn446241view1

因此：

> **RouterArena 不进入本阶段的 tuning 数据。**

如果未来能获得已有的 cached model outcomes，则可以作为 external holdout；否则正式评测需要真实推理，违反本阶段“零 API 成本”的优先原则。

---

# 五、数据源最终优先级

```text
                 ┌─────────────────────────┐
                 │ LLMRouterBench cached   │
                 │ Performance-Cost Track  │
                 └────────────┬────────────┘
                              │
                              ▼
                 ┌─────────────────────────┐
                 │ RouterBench 0-shot      │
                 │ 现有 401k outcomes      │
                 └────────────┬────────────┘
                              │
                              ▼
                 ┌─────────────────────────┐
                 │ RouteLLM cached         │
                 │ 2-model sanity         │
                 └────────────┬────────────┘
                              │
                              ▼
                 ┌─────────────────────────┐
                 │ RouterArena              │
                 │ future external holdout │
                 └─────────────────────────┘
```

Native 自采移出关键路径：

```text
Native = optional validation
```

而不是：

```text
Native = 下一阶段必须做
```

---

# 六、P0-1：把 LLMRouterBench 接成真正可复用的 Frozen Matrix

当前 LLooM 已经有 RouterBench adapter，因此下一步不是再扩一个一次性脚本，而是把 LLMRouterBench 变成统一 backend。

## 6.1 统一 canonical schema

无论来源：

```text
RouterBench
LLMRouterBench
RouteLLM cache
```

都转换成：

```text
BenchmarkInstance
ModelOutcome
```

### BenchmarkInstance

```text
source
source_version
dataset
sample_id
prompt
ground_truth
task_family
task_type
split
meta
```

### ModelOutcome

```text
source
sample_id
model_id
score
cost
prompt_tokens
completion_tokens
latency(optional)
response(optional)
```

---

## 6.2 严禁覆盖 source outcome

Canonical 层必须同时保存：

```text
raw_score
normalized_score
raw_cost
normalized_cost
cost_basis
```

禁止：

```text
导入后直接覆盖原始 score
导入后把 source cost 改成 LLooM cost
```

因为以后必须可以回答：

> 这个数字来自原论文数据，还是 LLooM 的归一化？

---

# 七、P0-2：做 Model Pool Search——先解决“Weak 选谁”

这是本阶段最值得新增的工作。

## 7.1 不再人工指定三模型

原来：

```text
Mistral-7B
Yi-34B
GPT-4
```

直接作为：

```text
Weak / Mid / Strong
```

这一做法现在应该升级。

LLMRouterBench 已经有完整 Performance-Cost pool，因此可以在 calibration split 上离线枚举模型池。它本身也强调不同模型存在互补优势，而不存在一个模型统治所有领域的情况。citeturn957897view0

---

## 7.2 三模型池枚举

对所有候选模型：

```text
cost_i
mean_score_i
```

先筛：

```text
low-cost candidates
mid-cost candidates
high-cost candidates
```

再枚举：

```text
Weak × Mid × Strong
```

组合。

第一版不需要穷举 13³ 的全部排列；因为层级已经按成本单调排序，可以只枚举满足：

```text
cost(Weak) < cost(Mid) < cost(Strong)
```

的组合。

---

## 7.3 Pool scoring

所有指标只在 calibration 上算：

```text
weak_score
mid_score
strong_score
oracle_score
oracle_gain
weak_oracle_share
cheap_safe
upgrade_opportunity
winner_entropy
cost_ratio
```

新增一个最重要指标：

```text
weak_opportunity
=
P(Weak is oracle-equivalent)
```

以及：

```text
weak_safe_ratio
=
P(Weak is oracle-equivalent | Weak selected)
```

注意后者在 pool screening 阶段没有 router selection，因此只能在 replay 后算；pool screening 本身使用：

```text
weak_oracle_share
```

---

## 7.4 Pool Search 的目标不是最大 Oracle Gain

不能简单：

```text
max oracle_gain
```

否则容易选出：

```text
Strong
```

特别强、Weak 特别弱

```

但这种 pool 并不适合 routing。

推荐 screening score：

```text
PoolScore =
  0.30 × normalized_oracle_gain
+ 0.25 × weak_oracle_share
+ 0.20 × cheap_safe
+ 0.15 × winner_entropy
+ 0.10 × normalized_cost_ratio
```

这只是 screening ranking，不是 router 的生产公式。

---

## 7.5 最终选三个候选池，而不是一个

```text
Pool-A · 最大 cost leverage
Pool-B · 最大 Weak opportunity
Pool-C · 最大 winner diversity
```

然后：

```text
在 calibration 上调完参数
        ↓
只选择一个最终 pool
        ↓
锁死 manifest
        ↓
只看 test
```

这样可以避免：

> “我们只是挑了一个结果最好看的模型池。”

---

# 八、P0-3：重做 Weak 失败分析

当前已经有 Error Taxonomy 草案，但本版增加一个关键方向：

> **区分“Weak 不应该被选”和“Weak 根本没有资格被选”。**

## 8.1 新增 Eligibility Error

每一道 test query 记录：

```text
weak_eligible_before_plan
weak_selected
weak_oracle_equivalent
```

因此可以得到四格：

| Weak eligibility | Oracle Weak | 含义 |
|---|---|---|
| false | true | **Blocked Weak** |
| true | true | Missed Weak |
| false | false | Safe exclusion |
| true | false | Risky Weak exposure |

这是当前最关键的诊断。

---

## 8.2 直接计算 Weak Blocking Rate

```text
WeakBlockingRate =
P(Weak oracle-equivalent AND Weak not eligible)
```

如果这个值很高：

```text
→ 主要修 capability gate
```

如果这个值很低，但 Weak Recall 仍很低：

```text
→ 主要修 scoring / query suitability
```

这样下一步不会再靠猜。

---

# 九、P0-4：第一轮只做三个 Ablation

不要一下加入 kNN / embedding / learned router。

先测：

```text
V0 Current Full
V1 Band-only
V2 Quality+Cost without hard gate
```

## V0

当前生产 `plan()`，作为 control。

## V1

只让 band / capability 决定候选集合。

## V2

允许 Weak 进入 medium：

```text
min_capability_tier = 1
```

由现有 `plan()` 继续完成 quality × cost 选择。

hard benchmark 则先保持：

```text
min tier >= 2/3
```

不让第一轮实验把极难样本全部降到 Weak。

---

# 十、P0-5：Soft Capability Gate 是最快的 Weak Rescue

## 10.1 第一阶段不要改生产 Router

只增加 benchmark policy：

```text
soft_gate_v1
```

生产 `router.rs` 的现有公式保持不动。

---

## 10.2 Basic Soft Gate

对于 medium：

```text
当前：tier >= 2
```

改为：

```text
Weak 也进入 candidate
```

但如果 Weak 质量估计明显不足，则自然不会被选。

对于 hard：

```text
保留 strong-only / strong-dominant gate
```

这样首先解决：

```text
“Weak 资格问题”
```

而不是同时解决所有事情。

---

## 10.3 Soft Penalty Sweep

如果 Basic Soft Gate 直接让 Weak 过度选择，则增加：

```text
penalty ∈ {0, .05, .10, .15, .20}
```

形式：

```text
effective_quality
=
quality
-
penalty × tier_gap
```

注意：

> 这只是 benchmark variant 的 policy，不复制 production `score_all()` 公式。

如果 production `plan()` 当前接口无法直接传 penalty，则采用“candidate eligibility + PlanInput”方式实现，而不是在 bench 中复制 score 公式。

---

# 十一、P0-6：先用 Cost Weight 观察 Weak 是否能自然回流

当前 λ sweep：

```text
λ ≤ 0.2 → Strong
λ ≥ 0.3 → Mid/Strong
```

原因之一是 Weak 被 hard gate 排除了。

因此顺序必须是：

```text
先放 Weak 进 candidate set
        ↓
再重新 sweep λ
```

测试：

```text
λ = 0.0 ... 0.9
```

观察：

```text
Weak share
Quality
Cost
Oracle regret
```

如果 Soft Gate 后 λ 曲线仍只有两档，才说明瓶颈已经从 candidate eligibility 转向 quality prior。

---

# 十二、P1：轻量 Query-level Weak Router

只有在：

```text
Soft Gate 后
Weak Recall 仍然明显低
```

才进入本步骤。

而且第一版**不使用 LLM**。

---

## 12.1 为什么不用 LLM classifier

目的就是零成本。

如果为了让 router 学会 Weak：

```text
query
→ LLM judge
→ router
```

那么评测成本又回来了。

所以第一版使用：

```text
TF-IDF / hashed n-gram
+
kNN
```

完全 CPU 离线。

---

## 12.2 输入

只使用：

```text
prompt
```

以及 calibration：

```text
prompt
model score
```

绝不使用：

```text
test score
```

---

## 12.3 最简单版本：Weak kNN

对每条 test query：

```text
找 calibration 中最相似 k 条 query
```

计算：

```text
weak_probability
```

例如：

```text
P(Weak succeeds | nearest calibration queries)
```

然后给 routing 一个 eligibility hint：

```text
P_weak >= threshold
```

才允许 Weak 参与。

---

## 12.4 threshold sweep

```text
0.50
0.60
0.70
0.80
0.90
```

输出：

```text
Weak Recall
Weak Precision
Cost Saving
Quality Retention
```

这样非常容易找到第一个真正有效的 Weak rescue operating point。

---

# 十三、推荐加入“Task-only → kNN → Full”三档诊断

```text
Task-only
  ↓
只知道 task_type / band

kNN
  ↓
知道 query similarity

Full LLooM
  ↓
band + quality + cost
```

如果：

```text
Task-only ≈ Full
```

说明当前 router 仍然主要依赖粗粒度任务分类。

如果：

```text
kNN >> Task-only
```

说明 query-level suitability 是主要缺口。

如果：

```text
kNN ≈ Full
```

说明 LLooM 已经能够吸收大部分 query-level 信息。

---

# 十四、P1：加入“Matched Random”，让收益解释更严格

当前 Random：

```text
Q=0.6080
C=$0.001614
```

与 LLooM：

```text
Q=0.7087
C=$0.002826
```

不在相同成本点上。

所以新增两个：

## Cost-Matched Random

令：

```text
E[C_random] ≈ C_LLooM
```

## Strong-Share-Matched Random

令：

```text
P(Random selects Strong)
=
P(LLooM selects Strong)
```

然后比较：

```text
Quality_LLooM
-
Quality_RandomMatched
```

如果 LLooM 仍明显更高，才能证明：

> 它不是仅仅“把模型比例调得刚好”，而是在识别 query。

---

# 十五、P1：Budget Curve 取代 λ 作为主要展示曲线

λ sweep 继续保留，用来 debug。

但 README / presentation 的主图换成：

```text
Strong-call budget
        ↓
maximum quality
```

例如：

```text
100%
90%
80%
70%
60%
50%
40%
30%
20%
```

同时报告：

```text
Weak share
Mid share
Strong share
```

这样更容易展示：

> 在相同 Strong 使用预算下，LLooM 能保住多少质量。

RouteLLM 本身就以 strong-call threshold / cost-quality tradeoff 进行评估，并提供阈值校准。citeturn446241view2turn416699search7

---

# 十六、P1：把 LLMRouterBench 的 10 个 Performance-Cost datasets 全部纳入 screening

不需要全部成为正式 headline。

先全部做 calibration screening。

LLMRouterBench 当前 Performance-Cost 数据集包括：

```text
AIME
LiveMathBench
LiveCodeBench
SWE-Bench
GPQA
HLE
MMLU-Pro
SimpleQA
ArenaHard
τ²-Bench
```

其中自动评分较干净的优先：

```text
AIME
LiveMathBench
LiveCodeBench
GPQA
MMLU-Pro
τ²-Bench
```

而：

```text
HLE
SimpleQA
ArenaHard
```

因为使用 LLM-as-judge，暂作为 secondary track。citeturn446241view0

SWE-Bench 继续作为 stress track，不参与第一轮 Weak rescue 主实验。

---

# 十七、Benchmark Selection 新规则

当前不再固定：

```text
“就是这五个 benchmark”
```

而改为：

## Step 1 · Dataset Screening

每个 dataset 在 calibration 上计算：

```text
ModelGap
OracleGain
WeakOpportunity
Disagreement
WinnerEntropy
CostRatio
```

## Step 2 · Pool Screening

对候选 model pool 计算同样指标。

## Step 3 · Cross-product Screening

真正筛选的是：

```text
(dataset, model_pool)
```

因为：

> routing headroom 是 workload × pool 的联合属性。

## Step 4 · Freeze

最后形成：

```text
2–3 个最适合 routing 的 workload/pool pair
```

然后才进入 test headline。

---

# 十八、严禁用 Screening Test Split 反向选 Benchmark

整个流程必须：

```text
Raw public result
      ↓
Calibration
      ↓
Dataset / Pool / Variant selection
      ↓
Freeze manifest
      ↓
Test
```

一旦进入 Test：

```text
禁止：
换 pool
换 benchmark
换 threshold
换 Weak
换 cost_weight
```

否则新配置必须：

```text
new benchmark_id
new manifest_hash
```

---

# 十九、最重要的三层“免花钱优化”路线

本阶段优化按成本从低到高：

```text
Level 0
纯 replay / baseline
≈ 0

Level 1
Soft Gate + Pool Search + λ/Budget sweep
≈ 0

Level 2
CPU kNN / TF-IDF query router
≈ 0

Level 3
Native model collection
$ / token cost
```

**必须只有 Level 0–2 全部做完且仍有明确问题，才允许进入 Level 3。**

---

# 二十、第一轮最可能快速产生收益的方案

我建议实际实施时优先试下面这个，而不是一次实现所有东西：

## Weak Rescue v1

```text
1. 使用现有 RouterBench matrix
2. 不改变 production score_all()
3. medium band 放开 Weak candidate
4. hard band 仍保守
5. λ sweep 重新跑
6. 输出 Weak Recall / Precision
```

预计这只需要很小的 benchmark policy 修改。

### 如果 Weak 仍然接近 0

立刻做：

```text
Weak Rescue v2
=
Weak candidate
+
calibration-only kNN Weak eligibility
```

### 如果 v2 有明显收益

再做：

```text
v2 × threshold sweep
```

寻找：

```text
最高 Weak Recall
```

同时保持：

```text
Weak Precision ≥ 90%
```

---

# 二十一、一个更值得尝试的“安全 Weak”策略

为了避免只追求省钱，增加一个非常简单的双阈值策略：

```text
P_weak(query) ≥ τ_safe
AND
P_weak(query) - P_mid(query) ≥ margin
```

才允许 Weak。

例如：

```text
τ_safe = 0.85
margin = 0.05
```

否则：

```text
Weak blocked
```

这样可以把 Weak 定位成：

> **safe downgrade candidate**

而不是：

> **cheap fallback candidate**

这会明显更符合生产 router 的安全性。

---

# 二十二、Weak Error Taxonomy 的最终版本

每个 query 最终应该能落入：

```text
1. Correct Weak
2. Correct Mid
3. Correct Strong
4. Missed Weak
5. Missed Mid
6. Missed Strong
7. Over-route
8. Under-route
9. Blocked Weak
10. Risky Weak
```

其中：

```text
Blocked Weak
= Weak 是 oracle-equivalent
  但 capability gate 不允许
```

```text
Missed Weak
= Weak 是 oracle-equivalent
  Weak 有资格
  但 router 最终没选
```

这两个必须分开。

因为：

```text
Blocked Weak
→ gate 问题

Missed Weak
→ scoring / query suitability 问题
```

这就是下一阶段最重要的 debugging compass。

---

# 二十三、增加一个“Oracle-Equivalent Cost”指标

对于每个 query：

```text
oracle_score = max_m score_m
oracle_equivalent_models = {m | score_m = oracle_score}
```

定义：

```text
oracle_equivalent_cost
=
min(cost_m | m ∈ oracle_equivalent_models)
```

再定义：

```text
Cost Regret
=
selected_cost
-
oracle_equivalent_cost
```

这样：

```text
Q 没掉
但花多了很多钱
```

也会被真正识别。

这比单独看 accuracy 更适合 routing。

---

# 二十四、增加三个 headline diagnostic

除了 Quality / Cost / AIQ，加入：

## Weak Recall

```text
P(select Weak | Weak oracle-equivalent)
```

## Weak Precision

```text
P(Weak oracle-equivalent | select Weak)
```

## Oracle Cost Efficiency

```text
C_router / C_oracle-equivalent
```

其中前两个用于解释 Weak，第三个用于衡量 router 离理论最优成本还有多远。

---

# 二十五、重新定义阶段成功标准

不再预设：

```text
Weak Selection ≥ 30%
```

而定义三层 Gate。

## Gate A · 修复资格问题

至少：

```text
Blocked Weak 明显下降
```

## Gate B · 修复选择问题

至少：

```text
Weak Recall > 0
Weak Precision 高
```

## Gate C · 总体收益

目标区间：

```text
Cost Saving > 36.1%
Quality Retention ≥ 93%
```

更好的目标：

```text
Cost Saving 40–50%+
Quality Retention 93–95%+
```

这些都是目标，不是提前承诺的结果。

---

# 二十六、需要特别关注一个可能发生的反直觉结果

可能出现：

```text
Weak Recall ↑↑
Cost Saving ↑↑
但 Quality Retention ↓
```

这时不要继续压：

```text
threshold ↓
```

而应该：

```text
threshold ↑
margin ↑
```

寻找安全 operating point。

反过来也可能：

```text
Weak Precision 99%
Weak Recall 5%
```

这说明 router 太保守。

所以真正希望的是：

```text
Weak Precision / Recall curve
```

而不是某一个固定 Weak share。

---

# 二十七、加入 Weak Precision–Recall Curve

在 calibration 上 fit：

```text
Weak suitability score
```

在 test 上 sweep：

```text
threshold 0.50 → 0.95
```

画：

```text
Weak Precision
      ↑
1.0 ─────────●
             │
0.9 ─────●───┘
         │
0.8 ─●───┘
      └────────────→ Recall
```

这个图很容易回答：

> “为了安全地把 Weak 放回来，能放回来多少？”

它会成为下一阶段最有价值的诊断图之一。

---

# 二十八、Cross-Benchmark 验证：不要在一个 benchmark 上过拟合

如果在 RouterBench 0-shot 上做出了：

```text
Weak Recall +20pp
Cost Saving +10pp
```

还不能直接认为 router 真有提升。

必须至少在：

```text
RouterBench
LLMRouterBench
```

各跑一次冻结配置。

配置必须完全固定：

```text
same policy
same threshold
same Soft Gate
same query feature extractor
```

这样才能知道：

> Weak rescue 是 router 本身的改进，还是旧 benchmark 特性。

---

# 二十九、第二个强烈建议：做“跨模型池泛化”

同一个 router policy 至少跑：

```text
Pool-A
Pool-B
Pool-C
```

其中：

```text
Pool-A：成本梯度最大
Pool-B：Weak opportunity 最大
Pool-C：winner diversity 最大
```

最终看：

```text
平均 Weak Recall
平均 Cost Saving
平均 Quality Retention
```

如果某个策略只对一套 pool 有效，不能作为 LLooM general routing 的证据。

---

# 三十、RouteLLM 不作为“要击败的系统”，而作为 sanity reference

RouteLLM 公开说明它支持 MMLU / GSM8K / MT-Bench，且 benchmark cache 已存在；其路由主要针对 strong/weak 二模型对并通过 threshold 做 cost-quality tradeoff。citeturn416699search7turn446241view2

因此 LLooM 可以增加：

```text
2-model mode
```

让：

```text
Weak / Strong
```

对齐 RouteLLM 的典型设定。

然后比较：

```text
LLooM
RouteLLM
Random
Always Weak
Always Strong
```

但只在有可复用 cached outcomes 的数据集上进行。

目的不是声称 SOTA，而是：

> **让 LLooM 的最基础 strong/weak routing 行为能与已有研究口径对齐。**

---

# 三十一、什么时候才允许 Native

只有以下条件全部满足，才值得花钱：

```text
Public Pool Search 已完成
AND
Weak Rescue 已完成
AND
Cross-Benchmark 有复现
AND
Matched Random 优于 baseline
AND
至少一个 policy 有稳定 quality/cost frontier
```

否则：

```text
不要采 Native
```

因为如果 Public matrix 上都不能证明 router 改进有效，再采新的真实模型只是在放大成本，不是在解决问题。

---

# 三十二、Native 进入时也不再全量自采

如果以后进入 Native：

第一轮只做：

```text
2 models
×
2 datasets
×
50 samples
```

用于 sanity check。

而不是：

```text
3 models × 5 datasets × 100+
```

只有 Native pilot 复现 Public Track 的：

```text
Weak opportunity
Soft Gate gain
```

才扩大。

---

# 三十三、工程结构调整

建议从当前：

```text
benchmarks/routerbench/
```

升级成：

```text
benchmarks/routerbench/
├── adapters/
│   ├── routerbench.py
│   ├── llmrouterbench.py
│   └── routellm_cache.py
│
├── screening/
│   ├── dataset.py
│   ├── pool.py
│   └── pair.py
│
├── routing/
│   ├── weak_gate.py
│   ├── knn.py
│   └── matched_random.py
│
├── analysis/
│   ├── taxonomy.py
│   ├── bootstrap.py
│   ├── budget.py
│   └── metrics.py
│
├── manifests/
├── normalized/
├── reports/
└── scripts/
```

Rust：

```text
crates/lloom-core/src/bench.rs
```

继续只负责：

```text
PlanInput
→ production plan()
→ frozen lookup
→ run item
```

Benchmark-specific feature engineering尽量留在 benchmark module，不污染 production router。

---

# 三十四、CLI 最小集合

## Dataset / Pool screening

```bash
lloom-cli bench datasets --source llmrouterbench
lloom-cli bench pools --dataset <dataset>
lloom-cli bench pool-search --split calibration
```

## Error diagnosis

```bash
lloom-cli bench diagnose --run <run_id>
```

## Ablation

```bash
lloom-cli bench ablation \
  --variants current,band-only,soft-gate
```

## Weak rescue

```bash
lloom-cli bench weak-sweep \
  --thresholds 0.5,0.6,0.7,0.8,0.9
```

## Budget

```bash
lloom-cli bench budget-sweep \
  --strong-ratios 1.0,0.9,0.8,0.7,0.6,0.5,0.4,0.3
```

## Cross-benchmark

```bash
lloom-cli bench matrix \
  --datasets routerbench,llmrouterbench
```

---

# 三十五、报告最终要从“一个数字”改成“一个诊断闭环”

每个 run 必须能回答：

```text
1. 选了谁？
2. 为什么？
3. Weak 当时有没有资格？
4. Weak 是否其实能答对？
5. 如果没选 Weak，是 gate 问题还是 scoring 问题？
6. 多花的钱是多少？
7. 比 matched random 好多少？
8. 与 Oracle 差多少？
```

因此每一条 `bench_run_item` 建议保存：

```text
selected_model
selected_score
selected_cost
oracle_score
oracle_equivalent_models
oracle_equivalent_cost
regret
cost_regret
weak_eligible
weak_selected
weak_oracle_equivalent
weak_probability(optional)
error_class
decision_json
```

---

# 三十六、WebUI 这阶段也要随之改变

不是立即做一个复杂 dashboard。

第一屏直接回答三个问题：

```text
Quality
Cost Saving
Weak Recall
```

第二屏：

```text
Blocked Weak
Missed Weak
Correct Weak
Missed Strong
Over-route
Under-route
```

第三屏：

```text
Current
Soft Gate
kNN
Best Variant
```

第四屏：

```text
Public Track
LLMRouterBench Track
```

第五屏：

```text
Single Query
```

点一题即可看到：

```text
Weak eligible? YES/NO
Weak actual score
Mid actual score
Strong actual score
LLooM selected
Oracle equivalent
error class
saved cost
```

---

# 三十七、下一阶段真正的实验矩阵

第一轮，不超过以下 8 组：

| Run | Pool | Policy | 目的 |
|---|---|---|---|
| R0 | 当前 Public Pool | Current | control |
| R1 | 当前 Public Pool | Band-only | 拆 band 贡献 |
| R2 | 当前 Public Pool | Soft Gate | 测 Weak eligibility |
| R3 | 当前 Public Pool | Soft Gate + λ sweep | 找最优成本区间 |
| R4 | 最佳新 Pool | Current | 分离 pool 贡献 |
| R5 | 最佳新 Pool | Soft Gate | pool + gate 联合 |
| R6 | 最佳新 Pool | Soft Gate + kNN | query-level rescue |
| R7 | 最佳新 Pool | Best Variant | 最终零成本候选 |

然后在：

```text
RouterBench test
LLMRouterBench test
```

各复跑最终固定 variant。

---

# 三十八、期望得到的关键结果

理想情况不是：

```text
LLooM 选择 Weak 40%
```

而是：

```text
Current
Weak Recall = 0%

Soft Gate
Weak Recall = ↑
Weak Precision = 高

kNN
Weak Recall = ↑↑
Weak Precision = 高

Cost Saving
36.1% → 更高

Quality Retention
92.9% → 保持 / 提升
```

如果最终结果是：

```text
Weak Recall ↑
但 Weak Precision 崩
```

就退回阈值。

如果：

```text
Weak Recall 仍 0
```

则证明问题可能不在 gate，而在 query-level suitability / model pool。

---

# 三十九、当前最值得验证的三个假设

## H1 · Weak 0% 主要由 hard capability gate 导致

验证：

```text
Current
vs
Soft Gate
```

## H2 · 当前 pool 的 Weak 真的过弱

验证：

```text
Pool Search
```

若找到新的低成本模型，使：

```text
weak_oracle_share ↑
```

则说明主要是 pool 问题。

## H3 · query-level suitability 是剩余主要瓶颈

验证：

```text
Soft Gate
vs
Soft Gate+kNN
```

如果 kNN 明显提升 Weak Recall，则 H3 成立。

---

# 四十、三种可能结论及后续路线

## Case A · Soft Gate 就明显有效

说明：

```text
主要问题 = eligibility
```

后续：

```text
soft capability penalty
→ production router experiment
```

## Case B · Pool Search 有效，Soft Gate 一般

说明：

```text
主要问题 = model pool
```

后续：

```text
重新设计 tier
```

## Case C · kNN 才明显有效

说明：

```text
主要问题 = query-level suitability
```

后续：

```text
更强 query router
→ embedding / lightweight learned router
```

这些结论都比“继续微调 λ”有价值。

---

# 四十一、下一阶段明确不做什么

为了保持速度，本阶段暂时不做：

```text
1. 不重新采集 RouterBench / MMLU / GSM8K
2. 不跑全量 Native model outcomes
3. 不训练大型 neural router
4. 不接 LLM judge 做线上 routing classifier
5. 不先开发复杂 WebUI 动画
6. 不把 RouterArena 用作 tuning 数据
7. 不为了 headline 重新挑 test sample
8. 不在多个 benchmark 上分别人工调 threshold
```

---

# 四十二、最终优先级

## P0 · 必须立即做，零成本

```text
[ ] LLMRouterBench adapter
[ ] 10 个 Performance-Cost dataset 全量 screening
[ ] Model Pool Search
[ ] WeakOpportunity / BlockingRate
[ ] tie-aware Error Taxonomy
[ ] Band-only / Soft-Gate ablation
[ ] 重新 λ sweep
```

## P1 · 零成本继续做

```text
[ ] Cost-matched Random
[ ] Strong-share-matched Random
[ ] Budget Curve
[ ] Weak Precision/Recall Curve
[ ] CPU kNN Weak Router
[ ] RouterBench + LLMRouterBench cross-benchmark
```

## P2 · 决定性工作

```text
[ ] 固定 Best Pool
[ ] 固定 Best Variant
[ ] Test-only freeze
[ ] Bootstrap CI
[ ] 正式 AIQ
```

## P3 · 可选

```text
[ ] Native pilot
[ ] RouterArena external validation
[ ] Full WebUI
```

---

# 四十三、建议实际排期

## Day 6 · Public Expansion

```text
LLMRouterBench import
↓
统一 canonical matrix
↓
全 dataset screening
```

产物：

```text
screening_llmrouterbench.csv
screening_llmrouterbench.json
```

## Day 7 · Pool Search

```text
3-model pool enumeration
↓
Pool-A/B/C
↓
calibration freeze
```

产物：

```text
pool_search.csv
pool_candidates.json
```

## Day 8 · Weak Rescue

```text
Error Taxonomy
↓
Blocked Weak
↓
Band-only
↓
Soft Gate
```

产物：

```text
weak_diagnosis.*
ablation_v2.*
```

## Day 9 · Cost / Budget

```text
λ sweep
↓
Budget curve
↓
Matched Random
```

## Day 10 · Query-level

如果 Weak 仍不足：

```text
TF-IDF / hashed n-gram
↓
kNN
↓
Weak threshold sweep
```

## Day 11 · Cross Benchmark

锁死 policy 后：

```text
RouterBench test
LLMRouterBench test
```

不得分别 tuning。

## Day 12 · Statistics

```text
Bootstrap CI
AIQ golden fixtures
```

## Day 13 · Decision

只做一次决策：

```text
值得进 Native？
```

如果否：

```text
继续零成本 router 改进
```

如果是：

```text
Native pilot
```

---

# 四十四、最终验收 Gate

## Gate 1 · Public Data

```text
LLMRouterBench 至少一个 Performance-Cost dataset
完整 model outcomes
```

## Gate 2 · Pool Search

```text
≥3 candidate pools
all selected on calibration
```

## Gate 3 · Weak Diagnosis

必须报告：

```text
Weak Opportunity
Weak Blocking Rate
Weak Recall
Weak Precision
```

## Gate 4 · Ablation

必须有：

```text
Current
Band-only
Soft Gate
```

## Gate 5 · Matched Baseline

必须有：

```text
Cost-matched Random
Strong-share-matched Random
```

## Gate 6 · Cross-Benchmark

最终 variant 至少在：

```text
RouterBench
LLMRouterBench
```

各跑一次。

## Gate 7 · Statistical

headline 必须有：

```text
Bootstrap 95% CI
```

## Gate 8 · Leakage

必须满足：

```text
pool selection → calibration only
threshold tuning → calibration only
query router fitting → calibration only
test → evaluator only
```

---

# 四十五、最终阶段定义

这一阶段结束时，不要求 LLooM 已经达到某个固定百分比。

真正需要得到的是一条已经被实验验证的因果链：

```text
当前 LLooM
    ↓
Weak 0%
    ↓
Blocked Weak / Missed Weak 被拆开
    ↓
Pool Search
    ↓
找到真正有 routing headroom 的低成本候选
    ↓
Soft Gate
    ↓
Weak eligibility 恢复
    ↓
Query-level kNN
    ↓
Weak Recall 进一步提升
    ↓
Matched Random 仍被超过
    ↓
Cross-Benchmark 复现
    ↓
形成新的 LLooM routing policy
```

最终真正希望证明的不是：

> “LLooM 让 Weak 出现了。”

而是：

> **“LLooM 能在无需额外 inference 作为 router 判断成本的前提下，识别出一部分便宜模型可以安全承担的 query，并把更强模型保留给真正需要它们的样本。”**

---

# 四十六、最终实施原则

### 原则 1 · 先吃现有数据，再采新数据

已有 benchmark outcomes 已经足够做大量 routing research。LLMRouterBench 当前明确提供预收集 results 与 performance-cost model pool；RouterBench 也已有完整逐实例结果。citeturn446241view0turn416699search4

### 原则 2 · Weak 的定义是“低成本且有机会”，不是“参数最小”

如果更便宜的模型几乎永远不能正确回答，那它不应该被强行塞进 router。

### 原则 3 · 先解除资格问题，再解决判断问题

```text
candidate eligibility
        ↓
query suitability
        ↓
quality-cost scoring
```

顺序不能反。

### 原则 4 · 只用 calibration 做 tuning

Benchmark selection、pool selection、threshold、kNN 全部遵循同一条规则。

### 原则 5 · 最终结果至少跨两个独立公开数据源

一个 benchmark 能产生漂亮结果，不足以说明 router 泛化。

### 原则 6 · API 花钱是最后一步

只有 Public zero-cost replay 已经证明：

```text
这条改进真的值得验证
```

才值得采 Native。

---

# 四十七、下一次开工的唯一第一步

**不要先改 `router.rs`。**

直接执行：

```text
1. 接入 LLMRouterBench Performance-Cost cached results
2. 在 calibration 上对所有可用模型做 Pool Search
3. 输出 Top-3 model pools
4. 对当前 pool 做 Weak Blocking Rate 分析
5. 在现有 `bench.rs` 增加 Soft-Gate policy variant
6. 重跑 λ sweep
```

完成以上六步后，再决定是否需要写 kNN。

这是当前最短、最省钱、信息增益最高的路径。

---

# 四十八、对本阶段成功状态的判断

如果最终得到：

```text
Weak Recall ↑
Weak Precision 高
Cost Saving ↑
Quality Retention ≈ 当前或更高
LLooM > matched Random
RouterBench 与 LLMRouterBench 同方向
```

则可以认为：

> **LLooM 已经从“基于粗粒度任务难度进行模型分层”，真正开始走向“针对 query 的模型适配与成本感知路由”。**

这时 Native Track 才值得开启。

反之，如果：

```text
Soft Gate
+ Pool Search
+ kNN
```

都无法明显改善 Weak Recall，那么应暂停扩大 benchmark，并回到 production routing 的能力估计模型重新设计，而不是继续堆数据量。
