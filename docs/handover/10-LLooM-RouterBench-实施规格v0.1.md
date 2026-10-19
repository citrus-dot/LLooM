# 10 · LLooM RouterBench 构建与实施规格 v0.1

> 日期：2026-10-04  
> 目标：在现有 LLooM `v2` 路由机制之上，建立一套可重复、可解释、零推理成本回放的 **LLooM RouterBench**。  
> 本文是 09 评审稿之后的实施版：已经把数据来源、模型池、冻结矩阵、筛选、校准、replay、baseline、指标、CLI、WebUI、验收和第一批实际命令细化到可以直接开工。

---

## 0. 本阶段结论先行

### 0.1 唯一主线

本阶段不是“给 LLooM 找一个模型 benchmark”，而是构造：

```text
LLooM RouterBench
=
固定模型池
+
固定 query set
+
冻结的 query × model outcome matrix
+
LLooM 真实 plan()
+
zero-inference-cost replay
+
quality / cost / Pareto / AIQ / Oracle 对照
```

LLooM 当前公开仓库已经明确将 score-based routing、两层分类、registry-gated scoring、成本/质量加权 `plan()`、shadow evaluation 和 AIQ replay 作为核心能力，因此 RouterBench 应直接验证其 routing decision layer，而不是另造一套 router 公式。citehttps://github.com/citrus-dot/LLooM

### 0.2 v0.1 不追求“大而全”

第一目标是 **7 天内形成第一张可信 Pareto 图**，而不是一次性支持 20+ benchmark。

推荐顺序：

```text
Phase 0
RouterBench 0-shot public matrix
        ↓
完成 screening
        ↓
选出真正有 routing headroom 的数据集/样本
        ↓
Phase 1
LLooM replay MVP
        ↓
Phase 2
Native Pool frozen matrix
        ↓
Phase 3
正式结果 + WebUI
```

### 0.3 两种结果必须分开

```text
Public Track
  数据来自公开 routing benchmark 的历史模型结果
  → 用于快速验证、研究对照、基础设施 smoke test

Native Track
  数据来自 LLooM 当前固定真实模型池的自采结果
  → 用于 LLooM 正式 headline
```

两条路线可以共享 query / scorer / replay / UI，但不能混合 cost basis、model metadata 或 headline。

---

# 一、已完成的信息收集与可立即采用的外部资料

## 1.1 RouterBench：最适合做第一天的 bootstrap

RouterBench 原始项目是专门为 multi-LLM routing 建立的 benchmark，论文报告超过 405k inference outcomes；GitHub 项目提供 convert → evaluate → visualize 的完整 pipeline。citehttps://arxiv.org/abs/2403.12031

其 Hugging Face 数据集明确说明：包含 3 万余 prompts、11 个 LLM 的响应、estimated cost 与 performance，并同时提供 0-shot 和 5-shot 数据。当前数据仓库文件中 `routerbench_0shot.pkl` 约 99.6 MB，`routerbench_raw.pkl` 约 1.2 GB。citehttps://huggingface.co/datasets/withmartian/routerbench

**实施结论：**

第一轮 bootstrap 不要下载完整 1.47 GB。优先使用：

```text
routerbench_0shot.pkl
```

因为它足以验证：

```text
query
×
model
→ score + cost
```

整个数据管线。

### 第一批下载命令

```bash
mkdir -p benchmarks/routerbench/raw/routerbench
cd benchmarks/routerbench/raw/routerbench

curl -L \
  -o routerbench_0shot.pkl \
  https://huggingface.co/datasets/withmartian/routerbench/resolve/main/routerbench_0shot.pkl
```

如果当前网络环境不能直连 HF，则使用浏览器/HF CLI 下载后，把文件放入上述目录；**不要让 LLooM 代码依赖下载链路本身。**

---

## 1.2 LLMRouterBench：正式 Public Track 的优先来源

LLMRouterBench 已在 2026 年进入 ACL Findings，并公开整理了：

```text
33 models
21+ datasets
400K+ instances
~1.8B tokens
10 routing algorithms
```

其数据以统一 JSON 记录保存，单条记录已经包含：

```text
origin_query
prompt
prediction
raw_output
ground_truth
score
prompt_tokens
completion_tokens
cost
```

同时提供 performance-only 与 performance-cost 两种设置。citehttps://github.com/ynulihao/LLMRouterBench/blob/main/README.md

它的 performance setting 包括：

```text
AIME          60
MATH500       500
MATHBench     150
HumanEval     164
MBPP          974
LiveCodeBench 1055
BBH           1080
KORBench      1250
Knights&Knaves 700
MMLU-Pro      1000
GPQA          198
FinQA         1147
MedQA         1273
EmoryNLP      697
MELD          1232
```

performance-cost setting 则包含：

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

官方项目同时公开了 pre-collected `bench-release.tar.gz`，当前 HF 镜像提供约 1.28 GB 的 archive，并明确给出了 `hf download` 与 `curl` 下载方式。citehttps://github.com/ynulihao/LLMRouterBench/blob/main/results/download.md

### 推荐下载

```bash
mkdir -p benchmarks/routerbench/raw/llmrouterbench
cd benchmarks/routerbench/raw/llmrouterbench

hf download \
  hf://datasets/NPULH/LLMRouterBench/bench-release.tar.gz \
  --local-dir .
```

或：

```bash
curl -L \
  -o bench-release.tar.gz \
  https://huggingface.co/datasets/NPULH/LLMRouterBench/resolve/main/bench-release.tar.gz
```

然后：

```bash
tar xzf bench-release.tar.gz
```

**第一阶段不必立即解析全部 21 个 dataset。**

优先抽取：

```text
mmlu_pro
math500
mbpp
humaneval
bbh
```

必要时再加入：

```text
gpqa
ifeval / arena-hard 类 instruction-following track
```

---

## 1.3 RouteLLM：作为 showcase/reference，而不是 LLooM 的复制对象

RouteLLM 已公开使用固定 strong/weak model pair 评估 MT-Bench、MMLU、GSM8K，并且代码明确支持切换 strong/weak model。MT-Bench 直接使用 precomputed judgements，MMLU/GSM8K 则可以重新计算指定模型对。citehttps://github.com/lm-sys/RouteLLM

其项目 README 宣称，在维持约 95% GPT-4 performance 时可获得最高 85% 的成本降低，因此 MT-Bench 非常适合作为 LLooM RouterBench 的 showcase 参照。citehttps://github.com/lm-sys/RouteLLM

**实施结论：**

```text
MT-Bench = Showcase
不是 v0.1 Core 的唯一科学依据
```

原因是它的主要评价链路使用 LLM-as-a-judge，和 MMLU-Pro / GSM8K / MBPP 这样的自动评分任务不完全同质。FastChat 也明确将 MT-Bench 定义为 LLM-as-a-judge 的 multi-turn open-ended evaluation。citehttps://github.com/lm-sys/FastChat/blob/main/fastchat/llm_judge/README.md

---

## 1.4 RouterArena：提供两个必须吸收的设计思想

RouterArena 当前公开为 router evaluation-only 平台，强调：

```text
9 domains
44 categories
3 difficulty levels
accuracy / cost / optimality / robustness / latency
```

并明确禁止在 evaluation data 上训练/拟合 router。citehttps://github.com/RouteWorks/RouterArena

它还采用 `sub_10 / full / robustness` 分层，并对 model cost 做显式配置；robustness track 可以只做 model-selection flip ratio，不需要额外 LLM inference。citehttps://github.com/RouteWorks/RouterArena/blob/main/README.md

**实施结论：** LLooM RouterBench 必须吸收：

```text
evaluation-only test split
显式 cost manifest
robustness / flip ratio 预留
```

但 v0.1 不直接依赖 RouterArena 的真实 inference pipeline。

---

## 1.5 LLMRouterBench 的一个非常重要的研究事实

LLMRouterBench 的公开分析明确指出：

> 不存在一个模型统治所有 domain；不同模型在数学、代码等领域存在互补性。

其 performance-cost setting 进一步用 PerfGain、CostSave、Pareto frontier 与 ParetoDist 衡量 routing，并报告最佳方法可以在维持 Best Single 精度的情况下获得明显成本节省。citehttps://github.com/ynulihao/LLMRouterBench/blob/main/README.md

这正好支持 LLooM 的实验假设，但也提醒我们：

> **必须测“逐题互补性”，不能只测模型平均分。**

---

# 二、LLooM RouterBench 的正式定义

## 2.1 核心对象

```text
Instance q

ModelPool M = {m1, m2, ..., mk}

FrozenOutcome(q, m)
  = {
      score,
      cost,
      tokens,
      optional latency,
      provenance
    }
```

Router 的输入：

```text
q
+ task metadata
+ model manifest
+ calibration state
+ budget/policy configuration
```

Router 的输出：

```text
m*
+ decision trace
```

Evaluator 的输入：

```text
(q, m*)
```

然后从 FrozenOutcome 查：

```text
score(q,m*)
cost(q,m*)
```

所以 replay 阶段没有：

```text
LLM API call
embedding API call
LLM judge call
```

**“Zero-cost” 的定义就是 replay 阶段 0 个外部 LLM inference。**

---

# 三、v0.1 任务分层与模型池

## 3.1 Track A：Public Matrix Track

目的：

```text
验证 import
验证 screening
验证 replay
验证 baseline
验证 AIQ/Pareto
验证 UI
```

不是最终 headline。

推荐优先数据源：

```text
P0: RouterBench 0-shot
P1: LLMRouterBench performance-cost
```

---

## 3.2 Track B：Native Pool Track（正式结果）

### 推荐第一候选池：三层同体系池

```text
Weak   = qwen3.6-flash
Mid    = qwen3.6-plus
Strong = qwen3.6-max-preview
```

这个选择的优点是：

1. 同一模型家族，减少 provider/chat-template 等额外变量；
2. 价格梯度清晰；
3. 都可以通过 Model Studio API 获取；
4. `qwen3.6-flash` 和 `qwen3.6-plus` 的上下文窗口为 1M，`qwen3.6-max-preview` 为 256K，因此还可以单独测试 context gate；
5. 官方价格页面当前给出的国际价格在 <=128K 范围内分别大致为 Flash 输入 $0.165/M、输出 $0.99/M；Plus 输入 $0.276/M、输出 $1.651/M；Max Preview 输入 $1.3/M、输出 $7.8/M，存在足够大的 cost gradient。citehttps://www.alibabacloud.com/help/en/model-studio/qwen3-6-flash citehttps://www.alibabacloud.com/help/en/model-studio/qwen3-6-plus citehttps://www.alibabacloud.com/help/en/model-studio/qwen3-6-max

**注意：这只是候选池，不是拍板结果。**

正式 Native Pool 必须先经过 screening：

```text
固定模型池
      ↓
跑少量 screening sample
      ↓
计算逐题互补性
      ↓
若 Mid 被 Strong 大面积支配
   或 Weak 基本无法处理
      ↓
替换 Mid/Weak
```

### 第二候选池：异构三模型

用于 v0.2：

```text
cheap   = qwen3.6-flash
reason  = deepseek-flash
strong  = qwen3.6-max-preview
```

DeepSeek 当前官方 API 将 `deepseek-flash` 映射为 DeepSeek-V4.1-Flash，并提供 1M context；当前价格体系还引入 peak/off-peak 区分，因此如果采用该池，`cost_basis` 必须额外冻结采样时间段或统一换算口径。citehttps://api-docs.deepseek.com/quick_start/pricing/

第一版不建议把峰谷电价混入同一 headline。

---

## 3.3 为什么第一版不是 5–7 个模型

第一版必须先验证：

```text
routing mechanism 是否有效
```

而不是：

```text
model catalog 是否丰富
```

如果同时加入 7 个模型，其中 3 个被其他模型 Pareto-dominated，最后的选择分布会很难解释。

因此：

```text
v0.1 = 3 models
v0.2 = 5–7 models
```

---

# 四、Benchmark 选择：先 screening，后决定 headline

## 4.1 候选集合

第一轮只测试：

```text
MMLU-Pro
MATH500
GSM8K
MBPP
HumanEval
BBH
GPQA
```

MT-Bench 单独进入 Showcase，不参与 Core 的唯一排序。

IFEval 可以作为 Native Track 第二阶段补充，但不要求第一轮立即跑。

---

## 4.2 每个数据集都计算 7 个 screening signal

### S1 · Best-Single Gap

```text
BestSingle = max_m mean(score(q,m))
```

与最弱模型差距：

```text
ModelGap = BestSingle - min_m mean(score(q,m))
```

用于判断基础模型差异。

---

### S2 · Oracle Gain

```text
Oracle(q) = max_m score(q,m)
OracleGain = mean(Oracle(q)) - BestSingle
```

**这是 v0.1 最重要的 screening 指标。**

它直接回答：

> 即使知道每一道题应该选哪个模型，理论上还能比“永远选最强的单模型”提升多少？

如果 OracleGain 几乎为 0，这个 benchmark 就没有 routing headroom。

---

### S3 · Strong-only Opportunity

对于 Weak/Strong：

```text
UpgradeOpportunity
= P(score_weak < score_strong)
```

如果是 binary scorer：

```text
= P(weak=0 ∧ strong=1)
```

它衡量“确实存在需要升级的题”。

---

### S4 · Cheap-safe Opportunity

如果 Weak 是最便宜模型：

```text
CheapSafe
= P(score_weak >= score_strong)
```

binary 情况可简化为：

```text
P(weak=1 ∧ strong=1)
```

它衡量“其实没必要花 Strong 钱”的题。

---

### S5 · Disagreement

```text
Disagreement
= P(∃ i,j : score_i != score_j)
```

如果 score 是连续值，则使用：

```text
mean(max(score)-min(score))
```

---

### S6 · Winner Diversity

定义：

```text
winner(q) = argmax_m score(q,m)
```

统计每个模型的 winner share，以及 winner entropy：

```text
H = -Σ p_m log(p_m)
```

如果 95% 以上 query 的 winner 都是同一个模型，说明模型池缺乏互补性。

---

### S7 · Cost Gradient

```text
CostRatio = cost_strong / cost_weak
```

至少要求存在明显 cost gradient。

第一版建议：

```text
>= 3x
```

作为工程 screening heuristic；这不是论文普适阈值，只用于避免“模型能力差异存在但成本差几乎没有”的 workload。

---

# 五、自动筛选规则

## 5.1 Dataset Gate

候选数据集进入 Core 的条件：

```text
1. 所有候选模型 outcome 覆盖率 = 100%
2. OracleGain > 0
3. UpgradeOpportunity 非零
4. CheapSafe 非零
5. CostRatio >= 3x
```

然后按照：

```text
OracleGain
+ UpgradeOpportunity
+ CheapSafe
+ WinnerEntropy
```

进行 rank。

不要硬设一个“必须 > 10%”的研究结论阈值。

---

## 5.2 Sample-level mining

在通过 dataset gate 后，再从数据集中挖出真正值得用于 showcase 的样本：

### A 类：Perfect downgrade

```text
Weak correct
Mid correct
Strong correct
```

这是最直接的节省成本案例。

### B 类：Correct escalation

```text
Weak wrong
Strong correct
```

这是证明 router 不会盲目省钱。

### C 类：Mid sweet spot

```text
Weak wrong
Mid correct
Strong correct
```

这是三层模型池最漂亮的案例。

### D 类：Complementary winner

```text
Mid/Strong/Weak
中的非 strongest 模型独占 winner
```

用于证明模型互补，而不是单纯 difficulty routing。

---

# 六、Canonical Frozen Matrix：不要再把所有内容塞进 SQLite

## 6.1 重新采用“两份文件 + SQLite metadata”

不建议把 30k×11 的 response 文本全塞进 SQLite。

### A. instances.jsonl

每个 query 一行：

```json
{
  "dataset_id": "mmlu_pro_v1",
  "sample_id": "mmlu_pro_000123",
  "task_family": "mmlu_pro",
  "task_type": "general",
  "prompt": "...",
  "ground_truth": "...",
  "split": "test",
  "meta": {}
}
```

### B. outcomes.jsonl

每个 query×model 一行：

```json
{
  "dataset_id": "mmlu_pro_v1",
  "sample_id": "mmlu_pro_000123",
  "model_id": "weak",
  "score": 1.0,
  "cost": 0.00042,
  "prompt_tokens": 71,
  "completion_tokens": 18,
  "latency_ms": 812,
  "response_sha256": "...",
  "source": "llmrouterbench",
  "source_model": "Qwen3-8B"
}
```

response 原文可作为可选 audit artifact：

```text
responses/
  <dataset>/<model>/<sample_id>.txt
```

正式 replay 不需要读取 response。

---

## 6.2 Frozen Matrix Manifest

```yaml
benchmark_id: lloom-routerbench-v0.1
created_at: 2026-10-04T00:00:00Z
schema_version: 1

source:
  type: llmrouterbench
  revision: "<git-or-release-id>"
  source_url: "<url>"

scoring:
  metric: accuracy
  score_range: [0.0, 1.0]

prompt_profile:
  mode: zero_shot
  temperature: 0
  top_p: 1
  tools: false
  cache: false

split:
  calibration_ratio: 0.2
  test_ratio: 0.8
  seed: 3407

models:
  - id: weak
    source_name: "..."
    capability_tier: 1
    input_price_per_million: 0.0
    output_price_per_million: 0.0
    cost_basis: source

  - id: mid
    source_name: "..."
    capability_tier: 2

  - id: strong
    source_name: "..."
    capability_tier: 3
```

**Manifest 是实验不可变配置的一部分。**

任何以下字段改变都必须生成新的 `benchmark_id`：

```text
model pool
price
prompt format
sampling params
scorer
dataset revision
split seed
```

---

# 七、Calibration/Test：严格禁止 test leakage

## 7.1 唯一允许进入 Router 状态的来源

```text
calibration outcomes
```

禁止：

```text
test score
→ quality_override
```

---

## 7.2 推荐 split

```text
20% calibration
80% test
seed = 3407
```

如果源 benchmark 本身已有官方 train/test split，优先保持官方边界；内部 calibration/test 只在 evaluation/test 内部再做随机切分。

---

## 7.3 更进一步：避免“选择 benchmark”本身污染 test

正式 benchmark 选择过程使用：

```text
screening split
```

或公共矩阵的独立分析副本。

最终 headline test 一旦冻结，不再因为“哪个曲线更漂亮”调整 query composition。

### 最终数据冻结流程

```text
Raw benchmark
      ↓
Screening copy
      ↓
选择 dataset + pool + sample policy
      ↓
生成 frozen candidate
      ↓
锁 manifest hash
      ↓
生成 calibration/test
      ↓
禁止修改 test
```

---

# 八、Task Mapping：只做 metadata adapter，不制造第二套路由规则

建议 `task_mapping.yaml`：

```yaml
mmlu_pro:
  task_type: general
  family: knowledge

gsm8k:
  task_type: math_logic
  family: math

math500:
  task_type: math_logic
  family: math

mbpp:
  task_type: coding
  family: code

humaneval:
  task_type: coding
  family: code

bbh:
  task_type: complex_reasoning
  family: logic

gpqa:
  task_type: complex_reasoning
  family: knowledge

ifeval:
  task_type: general
  family: instruction_following
```

这里的 `task_type` 是 **Plan Benchmark 的 metadata input**，不是一个新的模型预测结论。

如果生产 `band_for()` 需要更多输入，就在 benchmark adapter 中提供这些已有字段；禁止重新写一套 `if difficulty > x => hard` 的 benchmark 专用分类器。

---

# 九、Replay Engine：工程上只允许一个 decision source

## 9.1 正确依赖图

```text
Production route()
      ↓
    plan()
      ↓
 score_all()

Benchmark replay
      ↓
 construct PlanInput
      ↓
    plan()
      ↓
 score_all()
```

**不得出现第二份 selection formula。**

---

## 9.2 Benchmark replay 的五步

```text
for sample in test:

  1. load instance
  2. construct PlanInput with frozen pool + calibration state
  3. selected_model = plan(input)
  4. outcome = lookup(sample, selected_model)
  5. write bench_run_item
```

---

## 9.3 Zero-cost 红线

Replay 过程中：

```text
ai_client = forbidden
classifier LLM = forbidden
usage_records = read-only
production models = read-only
routing_calibration = read-only
```

只允许：

```text
bench_* tables
frozen matrix
memory-only PlanInput
```

---

# 十、不要一开始就直接复刻完整 route()

## 10.1 Track B0：Plan Benchmark

第一版正式 routing benchmark 先直接测试：

```text
plan()
```

所需要的 `task_type` 来自 benchmark adapter。

这样能够回答：

> **LLooM 的模型选择器本身能不能正确做 quality-cost tradeoff？**

---

## 10.2 Track B1：Offline End-to-End Route

在 B0 稳定之后再测试：

```text
route()
```

但 classifier 必须使用：

```text
rule-only
或
cached classifier result
```

这样不会产生额外 LLM 成本。

最终可以做：

```text
Plan-only
vs
End-to-End rule-only
```

直接量化“分类错误损失了多少 routing performance”。

这是很有价值的研究诊断。

---

# 十一、Baseline：v0.1 固定 8 条

```text
1. Always Weak
2. Always Mid
3. Always Strong
4. Cheapest
5. Random
6. Best Single
7. Oracle
8. LLooM
```

其中：

```text
Always Strong
```

是质量参考；

```text
Cheapest
```

是最低成本参考；

```text
Best Single
```

是合理的 non-routing baseline；

```text
Oracle
```

是 hindsight upper bound。

v0.2 再加入：

```text
kNN
RouteLLM
Dataset-Oracle
```

---

# 十二、Baseline 的严格定义

## 12.1 Best Single

```text
m_best = argmax_m mean_calibration(score(q,m))
```

注意：如果用于 test benchmark，`Best Single` 的模型必须只由 calibration split 决定。

不能直接用全 test accuracy 选 Best Single。

---

## 12.2 Cheapest

```text
argmin_m expected_cost(m)
```

成本只来自 manifest，不看 test score。

---

## 12.3 Random

固定 seed：

```text
seed = 3407
```

均匀选择模型。

为了减少随机方差，最终报告建议：

```text
5 random seeds
```

但 LLooM 自身不因为这件事重复运行五次。

---

## 12.4 Oracle

```text
oracle(q) = argmax_m score(q,m)
```

仅用于上界。

若存在成本约束版本：

```text
oracle(q; budget)
```

只选满足 budget constraint 的最优模型。

---

# 十三、指标：不要只看 Accuracy

## 13.1 Quality

```text
Q = mean(score_i)
```

---

## 13.2 Cost

```text
C = mean(cost_i)
```

Native Track 的 cost 来自真实采集。

Public Track 的 cost 只用于 public matrix 内部比较。

---

## 13.3 Cost Saving

相对 Always Strong：

```text
1 - C_router / C_strong
```

---

## 13.4 Quality Retention

```text
Q_router / Q_strong
```

---

## 13.5 Gap Recovery

```text
(Q_router - Q_weak)
-------------------
(Q_strong - Q_weak)
```

这是展示 router “把弱模型提升到强模型附近多少”的核心指标。

---

## 13.6 Oracle Regret

```text
regret_i = oracle_score_i - router_score_i
```

报告：

```text
mean regret
P50
P95
```

---

## 13.7 Strong Call Ratio

```text
count(router == strong) / N
```

非常适合产品演示。

---

## 13.8 Selection Distribution

```text
share(weak)
share(mid)
share(strong)
```

按 task family 和 difficulty/band 分组。

---

## 13.9 AIQ

保留 RouterBench 的 quality-cost integration 思路；现有 `scripts/aiq_replay.py` 作为实现参考，但改成独立纯函数：

```text
points
→ nondominated set
→ monotonic / convex hull
→ common cost interval
→ integrate normalized quality
```

AIQ 不作为唯一 headline。

---

## 13.10 Bootstrap CI

所有 headline metric 用 test instances bootstrap：

```text
B = 10,000
seed = 3407
95% CI
```

先对 `sample_id` 做 bootstrap，再重新计算：

```text
quality
cost
saving
retention
gap_recovery
AIQ
```

这样不会人为把单模型输出行当成独立样本。

---

# 十四、最重要的实验：Cost-Quality Pareto Curve

对于 LLooM，不要只跑一个 policy。

需要对 routing policy 的成本权重做 sweep：

```text
λ = [0.0, 0.1, 0.2, ..., 1.0]
```

或直接 sweep 当前 policy 的 cost/quality tradeoff 参数。

每个 λ：

```text
Plan
→ test outcomes
→ Quality / Cost
```

最终得到：

```text
LLooM λ=0.0
LLooM λ=0.1
...
LLooM λ=1.0
```

再和：

```text
Cheapest
Best Single
Always Strong
Oracle
```

一起画 Pareto。

**这张图才是 RouterBench 的核心产物。**

---

# 十五、Willingness-to-Pay / cost threshold 不要与 router formula 耦合

RouterBench 原项目本身会使用 willingness-to-pay 等参数探索不同 cost-performance operating points。其公开代码支持解析 routing algorithm parameters 与 cost-related configuration。citehttps://github.com/withmartian/routerbench/blob/main/utils.py

LLooM v0.1 不需要完全复制这个 API。

只要保证：

```text
一个 policy 参数
→ 一个确定性 routing configuration
→ 一个可重放结果
```

之后可以自由画：

```text
policy parameter
vs
cost
vs
quality
```

---

# 十六、数据导入器：第一批真正要写的代码

目录：

```text
benchmarks/routerbench/
├── raw/
├── manifests/
├── normalized/
├── scripts/
│   ├── import_routerbench.py
│   ├── import_llmrouterbench.py
│   ├── screen.py
│   ├── split.py
│   └── validate.py
└── reports/
```

## 16.1 `import_routerbench.py`

输入：

```text
routerbench_0shot.pkl
```

输出：

```text
normalized/instances.jsonl
normalized/outcomes.jsonl
normalized/source_manifest.json
```

### 最小流程

```python
load pkl
→ enumerate sample_id
→ enumerate models
→ extract score
→ extract cost
→ normalize task_family
→ write long-format outcome rows
```

---

## 16.2 `import_llmrouterbench.py`

输入：

```text
results/bench/<dataset>/<split>/<model>/<timestamp>.json
```

输出统一到相同 canonical schema。

必须保留：

```text
source_model
source_timestamp
raw_cost
raw_tokens
```

---

# 十七、Screening Script：不依赖 LLooM Rust，今天就可以写

文件：

```text
benchmarks/routerbench/scripts/screen.py
```

命令：

```bash
python benchmarks/routerbench/scripts/screen.py \
  --input benchmarks/routerbench/raw/routerbench/routerbench_0shot.pkl \
  --models <weak> <mid> <strong> \
  --tasks mmlu gsm8k mbpp hellaswag mt_bench \
  --output benchmarks/routerbench/reports/screening.json
```

输出：

```json
{
  "dataset": "gsm8k",
  "n": 1319,
  "coverage": 1.0,
  "best_single": 0.81,
  "weak": 0.42,
  "oracle": 0.89,
  "oracle_gain": 0.08,
  "upgrade_opportunity": 0.47,
  "cheap_safe": 0.31,
  "disagreement": 0.52,
  "winner_entropy": 0.97,
  "cost_ratio": 6.4,
  "screening_score": 0.88
}
```

**数值只是格式示例，不是预期结果。**

---

# 十八、数据筛选后的第一个正式工作集

第一版不要直接把全部样本拿来跑。

推荐：

```text
每 dataset
↓
screening 后
↓
最多 500 test samples
```

即：

```text
MMLU-Pro       500
GSM8K          500
MATH500        500
MBPP           400
HumanEval      164
BBH            500
GPQA           198
```

理论上上限约：

```text
2,762 samples
```

这已经足以生成可靠的第一版图表。

实际第一轮可以更小：

```text
MMLU-Pro 200
GSM8K 200
MATH500 200
MBPP 150
BBH 150
```

总计：

```text
900 samples
```

先验证整个 pipeline。

---

# 十九、Native Pool 自采：什么时候开始花钱

**只有完成以下条件才开始：**

```text
Public screening 已完成
RouterBench replay 已跑通
Plan parity test 已通过
Outcome schema 已冻结
model_pool.yaml 已冻结
```

然后使用 `lm-eval-harness`。

其官方 README 明确支持 `local-chat-completions` / OpenAI-compatible server，并支持 `generate_until`；没有 logprobs 的 chat API 不适用于需要 loglikelihood/multiple-choice 的任务。citehttps://github.com/EleutherAI/lm-evaluation-harness/blob/main/README.md

因此 Native v0.1 优先：

```text
GSM8K
MBPP
HumanEval
IFEval
```

MMLU-Pro 如果通过当前 chat endpoint 做多选时存在 scoring/answer extraction 兼容问题，不要为了 benchmark 硬改生产 API；改用：

```text
predict_only
+
官方 scorer
```

或单独写 adapter。

官方也明确建议闭源 chat API 先使用少量 `--limit 10` 抽查输出与 scoring compatibility。citehttps://github.com/EleutherAI/lm-evaluation-harness/blob/main/README.md

---

# 二十、Native Pool 的采集配置

## 20.1 固定请求 profile

```yaml
generation:
  temperature: 0
  top_p: 1
  max_tokens: 4096
  stream: false
  tools: false
  cache: false
```

不同模型如果必须使用不同 `max_tokens`，则必须进入 manifest。

---

## 20.2 每模型独立直连

原则：

```text
query set fixed
system prompt fixed
sampling fixed
model direct
```

不是：

```text
query
→ LLooM
→ selected model
```

Native matrix 阶段必须测裸模型。

---

# 二十一、Native cost：按真实 token 计算，不使用平均价格乘样本数

每个 outcome 保存：

```text
prompt_tokens
completion_tokens
input_price
output_price
```

cost：

```text
cost
= prompt_tokens / 1e6 * input_price
+ completion_tokens / 1e6 * output_price
```

如果 provider 有：

```text
cache read
cache write
reasoning token
peak/off-peak
```

则这些都必须作为 manifest / outcome 的显式字段。

**不能在不同模型之间混用“估算 cost”和“真实 cost”。**

---

# 二十二、SQLite：只保存运行状态和索引

SQLite 保留：

```sql
bench_datasets
bench_runs
bench_run_items
bench_calibration_states
bench_model_pools
```

不建议：

```text
全量 response text
```

## 22.1 `bench_model_pools`

```sql
CREATE TABLE IF NOT EXISTS bench_model_pools (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  pool_id TEXT UNIQUE NOT NULL,
  manifest_json TEXT NOT NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

## 22.2 `bench_calibration_states`

```sql
CREATE TABLE IF NOT EXISTS bench_calibration_states (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  dataset_id INTEGER NOT NULL,
  pool_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  task_type TEXT NOT NULL,
  sample_count INTEGER NOT NULL,
  quality_ewma REAL NOT NULL,
  state_json TEXT,
  UNIQUE(dataset_id, pool_id, model_id, task_type)
);
```

## 22.3 `bench_runs`

```sql
CREATE TABLE IF NOT EXISTS bench_runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  dataset_id INTEGER NOT NULL,
  pool_id TEXT NOT NULL,
  strategy TEXT NOT NULL,
  parameter_json TEXT,
  split TEXT NOT NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  summary_json TEXT
);
```

## 22.4 `bench_run_items`

```sql
CREATE TABLE IF NOT EXISTS bench_run_items (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id INTEGER NOT NULL,
  sample_id TEXT NOT NULL,
  selected_model TEXT NOT NULL,
  score REAL NOT NULL,
  cost REAL NOT NULL,
  oracle_model TEXT,
  oracle_score REAL,
  regret REAL,
  decision_json TEXT,
  UNIQUE(run_id, sample_id)
);
```

---

# 二十三、Decision Trace：让单题 demo 有“证据”

每一个 `bench_run_item.decision_json` 至少保存：

```json
{
  "task_type": "math_logic",
  "band": "hard",
  "candidates": [
    {
      "model": "weak",
      "eligible": true,
      "quality_estimate": 0.62,
      "cost_norm": 0.08,
      "score": 0.51
    },
    {
      "model": "mid",
      "eligible": true,
      "quality_estimate": 0.78,
      "cost_norm": 0.34,
      "score": 0.72
    },
    {
      "model": "strong",
      "eligible": true,
      "quality_estimate": 0.89,
      "cost_norm": 1.0,
      "score": 0.67
    }
  ],
  "selected_model": "mid",
  "selection_probability": 1.0,
  "fallback_reason": null
}
```

这里的字段名以现有 C10 snapshot contract 为准；benchmark 只扩展保存，不另造生产日志 schema。

---

# 二十四、Decision Parity Test：正式开工的第一道 Rust Gate

写一个固定 `PlanInput` fixture：

```text
input_A
input_B
input_C
```

生产调用：

```text
plan(input_X)
```

benchmark adapter 调用：

```text
bench_plan(input_X)
```

验证：

```text
selected_model_A == selected_model_B
score_all outputs equal
```

如果这个 test 失败，**禁止继续做 benchmark report/UI。**

---

# 二十五、CLI：最终接口

## 25.1 数据

```bash
lloom-cli bench list

lloom-cli bench import \
  --source routerbench \
  --path benchmarks/routerbench/raw/routerbench/routerbench_0shot.pkl

lloom-cli bench import \
  --source llmrouterbench \
  --path benchmarks/routerbench/raw/llmrouterbench/results
```

## 25.2 Screen

```bash
lloom-cli bench screen \
  --dataset routerbench_0shot \
  --pool routerbench_top3
```

## 25.3 Calibration

```bash
lloom-cli bench calibrate \
  --dataset lloom_core_v1 \
  --pool native_qwen_v1
```

## 25.4 Replay

```bash
lloom-cli bench replay \
  --dataset lloom_core_v1 \
  --pool native_qwen_v1 \
  --strategy all \
  --split test
```

## 25.5 Report

```bash
lloom-cli bench report \
  --run <run_id> \
  --format json
```

```bash
lloom-cli bench report \
  --run <run_id> \
  --format markdown
```

---

# 二十六、Python / Rust 的职责边界

## Python

负责：

```text
原始 benchmark 解析
数据格式转换
screening
bootstrap
AIQ math/report helper
lm-eval adapter
```

## Rust

负责：

```text
PlanInput 构造
plan()
score_all()
calibration state
replay orchestration
run/item persistence
CLI
HTTP API
```

这样可以避免引入 Arrow/Polars 到 Rust 核心。

---

# 二十七、第一版目录结构

```text
LLooM/
├── benchmarks/
│   └── routerbench/
│       ├── README.md
│       ├── manifests/
│       │   ├── model_pool_public.yaml
│       │   ├── model_pool_native.yaml
│       │   ├── datasets.yaml
│       │   └── task_mapping.yaml
│       ├── raw/
│       │   ├── routerbench/
│       │   └── llmrouterbench/
│       ├── normalized/
│       ├── reports/
│       └── scripts/
│           ├── import_routerbench.py
│           ├── import_llmrouterbench.py
│           ├── screen.py
│           ├── split.py
│           ├── validate.py
│           └── bootstrap.py
│
├── crates/
│   └── lloom-core/
│       └── src/
│           └── bench.rs
│
├── scripts/
│   └── aiq_replay.py      # 保留，后续迁移纯函数
│
└── webui/
    └── src/pages/
        └── RouterBenchPage.tsx
```

---

# 二十八、API

## 28.1 Import

```http
POST /api/bench/import
```

body：

```json
{
  "source": "routerbench",
  "path": "...",
  "dataset": "gsm8k"
}
```

---

## 28.2 List

```http
GET /api/bench/datasets
GET /api/bench/pools
GET /api/bench/runs
```

---

## 28.3 Replay

```http
POST /api/bench/replay
```

body：

```json
{
  "dataset_id": "...",
  "pool_id": "...",
  "strategy": "lloom",
  "split": "test"
}
```

---

## 28.4 Results

```http
GET /api/bench/results?run_id=123
```

返回：

```text
summary
metrics
selection_distribution
pareto_points
samples
```

---

# 二十九、WebUI：四个页面组件足够

## 29.1 Overview

显示：

```text
Quality
Cost
Saving
Retention
AIQ
Strong Call Ratio
```

---

## 29.2 Pareto Chart

横轴：

```text
cost / 1K queries
```

纵轴：

```text
quality
```

点：

```text
Always Weak
Always Mid
Always Strong
Cheapest
Best Single
LLooM policy points
Oracle
```

---

## 29.3 Selection Distribution

按：

```text
dataset
family
task_type
band
```

显示：

```text
weak / mid / strong share
```

---

## 29.4 Per-instance Explorer

每题展示：

```text
Prompt
LLooM selected model
actual score
actual cost
Strong baseline cost
saved cost
Decision trace
```

这部分是最重要的“产品演示弹药”。

---

# 三十、必须支持的一个“Single Query Replay”

API：

```http
POST /api/bench/replay/one
```

输入：

```json
{
  "run_id": 42,
  "sample_id": "gsm8k_000142"
}
```

输出：

```text
query
→ PlanInput
→ candidates
→ LLooM decision
→ frozen score
→ frozen cost
→ baseline cost
→ savings
```

现场演示时不需要跑全 benchmark。

---

# 三十一、展示型样本的自动挑选

不要人工从几千题里找。

在 report 阶段自动选三类：

## Case A · Cheap Win

最大化：

```text
strong_cost - lloom_cost
```

约束：

```text
lloom_score == strong_score
```

---

## Case B · Smart Escalation

寻找：

```text
lloom = strong
weak wrong
strong correct
```

---

## Case C · Mid Sweet Spot

寻找：

```text
lloom = mid
weak wrong
mid correct
strong correct
```

这三个 case 一定会比随机 10 道题更有展示价值。

---

# 三十二、研究对照：从 v0.2 开始加入 Router baseline

v0.1 不需要一次实现所有 router。

优先顺序：

```text
v0.1
LLooM
Random
Cheapest
Best Single
Oracle

v0.2
kNN

v0.3
RouteLLM / EmbedLLM / GraphRouter
```

原因：

- LLMRouterBench 已经把 10 个主流 routing algorithm 纳入统一框架；
- RouteLLM 已经是一个成熟 strong/weak routing reference；
- GraphRouter / EmbedLLM 更适合等 frozen matrix schema 稳定后再适配。

LLMRouterBench 当前列出的代表性 router 包括 RouterDC、EmbedLLM、MODEL-SAT、Avengers、HybridLLM、FrugalGPT、RouteLLM、GraphRouter、Avengers-Pro 和 OpenRouter。citehttps://github.com/ynulihao/LLMRouterBench/blob/main/README.md

---

# 三十三、统计与重复实验策略

## v0.1

所有 deterministic replay：

```text
seed = 3407
```

随机 baseline：

```text
5 seeds
```

bootstrap：

```text
10,000 resamples
95% CI
```

## v0.2

加入：

```text
3 split seeds
```

即：

```text
3407
42
2026
```

最终报告平均值 + CI。

---

# 三十四、Robustness Track：v0.2 只做决策稳定性

借鉴 RouterArena 的 robustness 思路，构造：

```text
original prompt
      ↓
light paraphrase / whitespace / punctuation perturbation
      ↓
LLooM plan()
```

计算：

```text
flip_ratio
```

不重新调用任何模型。

目的：

> router 是否因为轻微表述变化而频繁跳模型？

这尤其适合分析当前 regex/task classification 对 routing 的影响。

---

# 三十五、Context Track：不要混入 Core

当前 Qwen3.6 Flash / Plus 都具备 1M context，而 qwen3.6-max-preview 的 context window 为 256K；因此长上下文天然适合做单独 Track，而不是混入一般 quality-cost benchmark。citehttps://www.alibabacloud.com/help/en/model-studio/text-generation-model

构造：

```text
8K
32K
64K
128K
256K
512K
1M
```

观察：

```text
context requirement
→ capability gate
→ selected model
```

这会成为 LLooM `context_window` gate 的专门证据。

---

# 三十六、第一阶段不做的事情

明确冻结：

```text
× 不把 latency 加进 v0.1 headline
× 不把 semantic cache 算进 routing quality
× 不把 shadow double-run 结果混入 public matrix
× 不把 test outcomes 写回 production calibration
× 不直接拿 aggregate benchmark score 填缺失 test outcome
× 不为了漂亮曲线手工删除失败样本
× 不把 MT-Bench 作为唯一 Core benchmark
× 不把 SWE-Bench / AIME 当 v0.1 主战场
× 不实现 10 个 router
× 不把 7 个模型一次全部塞进模型池
```

---

# 三十七、验收标准

## Gate 0 · 数据获取

```text
RouterBench 0-shot 可读取
OR
LLMRouterBench release 可读取
```

---

## Gate 1 · Frozen Matrix

```text
selected test samples × selected model pool
coverage = 100%
```

缺失 outcome：

```text
拒绝进入 headline
```

---

## Gate 2 · Screening

至少输出：

```text
ModelGap
OracleGain
UpgradeOpportunity
CheapSafe
Disagreement
WinnerEntropy
CostRatio
```

---

## Gate 3 · Plan parity

固定 fixture：

```text
production plan == bench replay plan
```

---

## Gate 4 · Zero cost

一次完整 replay：

```text
API requests = 0
usage_records mutation = 0
routing_calibration mutation = 0
```

---

## Gate 5 · Baseline

```text
Weak
Mid
Strong
Cheapest
Best Single
Random
Oracle
LLooM
```

全部出结果。

---

## Gate 6 · 统计

```text
Quality
Cost
Saving
Retention
Gap Recovery
AIQ
Oracle Regret
95% CI
```

全部可导出。

---

## Gate 7 · UI

四组件均来自真实 run：

```text
Pareto
Selection Distribution
Baseline Table
Per-instance Explorer
```

无 mock 数据。

---

# 三十八、第一周详细执行计划

## Day 1 上午 · 环境与数据

```bash
mkdir -p benchmarks/routerbench/raw/routerbench
mkdir -p benchmarks/routerbench/raw/llmrouterbench
mkdir -p benchmarks/routerbench/normalized
mkdir -p benchmarks/routerbench/manifests
mkdir -p benchmarks/routerbench/reports
```

下载：

```text
routerbench_0shot.pkl
```

目标：文件可读取。

---

## Day 1 下午 · 写 importer

实现：

```text
import_routerbench.py
```

验收：

```text
sample_id 去重
model list 正确
score 非空
cost 非空
```

---

## Day 2 · Screening

实现：

```text
screen.py
```

第一轮跑：

```text
mmlu
hellaswag
arc
winogrande
gsm8k
mbpp
mt_bench
```

然后输出：

```text
screening.json
screening.csv
```

---

## Day 3 · 选 workload + manifest freeze

产出：

```text
model_pool_public.yaml
datasets.yaml
task_mapping.yaml
frozen_manifest.json
```

确定：

```text
Core 3–5 datasets
~900–1800 samples
```

---

## Day 4 · Rust bench.rs

只实现：

```text
BenchInstance
FrozenOutcomeProvider
BenchPlanInputBuilder
BenchReplay
```

不做 UI。

---

## Day 5 · Baseline + metrics

实现：

```text
8 baselines
AIQ
Pareto
bootstrap CI
```

目标：第一次完整跑通：

```bash
lloom-cli bench replay --strategy all
```

---

## Day 6 · Native Pool 小规模采集

先跑：

```text
3 models
3 datasets
50–100 samples/dataset
```

先验证：

```text
API compatibility
answer extraction
cost accounting
outcome coverage
```

不立即跑完整集。

---

## Day 7 · Native frozen matrix + UI skeleton

完成：

```text
native matrix
calibration/test
LLooM replay
Pareto
single query explorer
```

此时就已经具备立项展示所需的第一版 evidence。

---

# 三十九、实际第一批命令清单

### 39.1 下载 RouterBench

```bash
mkdir -p benchmarks/routerbench/raw/routerbench
curl -L \
  -o benchmarks/routerbench/raw/routerbench/routerbench_0shot.pkl \
  https://huggingface.co/datasets/withmartian/routerbench/resolve/main/routerbench_0shot.pkl
```

### 39.2 运行 Python 健康检查

```bash
python benchmarks/routerbench/scripts/inspect.py \
  --input benchmarks/routerbench/raw/routerbench/routerbench_0shot.pkl
```

### 39.3 Screen

```bash
python benchmarks/routerbench/scripts/screen.py \
  --input benchmarks/routerbench/raw/routerbench/routerbench_0shot.pkl \
  --output benchmarks/routerbench/reports/screening.json
```

### 39.4 Import

```bash
lloom-cli bench import \
  --source routerbench \
  --path benchmarks/routerbench/raw/routerbench/routerbench_0shot.pkl
```

### 39.5 Replay

```bash
lloom-cli bench replay \
  --dataset routerbench_bootstrap_v1 \
  --pool public_top3_v1 \
  --strategy all
```

### 39.6 Report

```bash
lloom-cli bench report \
  --run latest \
  --format markdown
```

---

# 四十、建议的 manifest 初版模板

## `model_pool_native.yaml`

```yaml
pool_id: native_qwen_v1
version: 1
cost_basis: provider_list_price
region: international

models:
  - id: weak
    api_model: qwen3.6-flash
    capability_tier: 1
    context_window: 1000000

  - id: mid
    api_model: qwen3.6-plus
    capability_tier: 2
    context_window: 1000000

  - id: strong
    api_model: qwen3.6-max-preview
    capability_tier: 3
    context_window: 262144
```

真正价格不要手写到 benchmark code；统一从 LLooM pricing registry / frozen snapshot 导出，再存入 manifest。

---

# 四十一、正式结果表的最终形状

## Per Dataset

| Strategy | Quality | Cost / 1K | Saving | Retention | Gap Recovery | AIQ |
|---|---:|---:|---:|---:|---:|---:|
| Always Weak | | | | | | |
| Always Mid | | | | | | |
| Always Strong | | | | | | |
| Cheapest | | | | | | |
| Best Single | | | | | | |
| Random | | | | | | |
| Oracle | | | | | | |
| **LLooM** | | | | | | |

---

# 四十二、最终演示故事线

## 第一幕：为什么需要 Router

```text
同一个 benchmark
不同模型
不同题目 winner 不一样
```

展示：

```text
model average score
+
OracleGain
+
cost ratio
```

---

## 第二幕：LLooM 如何做决定

展示一题：

```text
GSM8K #142
```

```text
LLooM
→ Mid
→ score = correct
→ cost = $X
→ Strong baseline = $Y
→ saving = Z%
```

---

## 第三幕：另一题必须升级

```text
MATH #88
```

```text
LLooM
→ Strong
→ Weak/ Mid history insufficient
```

这一步证明 router 不是“无脑省钱”。

---

## 第四幕：宏观 Pareto

```text
Quality
↑
│                         Oracle
│                LLooM ●
│            ● BestSingle
│      ● Mid
│  ● Cheapest
└──────────────────────────→ Cost
```

---

## 第五幕：证明零成本

现场输入：

```text
sample_id
```

然后实时：

```text
plan()
→ selected model
→ frozen outcome lookup
```

同时显示：

```text
API calls = 0
```

---

# 四十三、项目成功的最低条件

只需要满足下面这组事实，立项展示已经成立：

```text
1. 有固定模型池
2. 有冻结 query × model matrix
3. 有 calibration/test split
4. LLooM 使用真实 plan()
5. replay = 0 API calls
6. 有 Cheapest / Best Single / Strong / Oracle
7. LLooM 产生非 trivial model selection distribution
8. 至少一个 workload 出现明显 quality-cost Pareto improvement
9. 单题可解释
10. 所有数字都有可追溯 provenance
```

并不要求第一版已经：

```text
支持所有 benchmark
支持所有 router
支持 latency-aware routing
支持 online learning
支持 multimodal
```

---

# 四十四、最终优先级：从现在开始按这个顺序写代码

```text
P0
下载 + inspect RouterBench 0-shot

P1
import_routerbench.py

P2
screen.py

P3
冻结第一版 public workload

P4
bench.rs + FrozenOutcomeProvider

P5
Plan parity test

P6
8 baseline + AIQ/Pareto

P7
Native model pool screening

P8
Native frozen matrix

P9
Calibration/test replay

P10
RouterBench WebUI

P11
MT-Bench showcase

P12
kNN / RouteLLM / GraphRouter baseline
```

**其中 P0–P6 完成之前，不应该把大量时间投入 UI polish。**

---

# 四十五、实施后的最终工程闭环

```text
                    ┌────────────────────┐
                    │ Benchmark Source   │
                    │ RouterBench        │
                    │ LLMRouterBench     │
                    │ lm-eval Native     │
                    └─────────┬──────────┘
                              │
                              ▼
                  ┌────────────────────────┐
                  │ Normalize + Screening  │
                  │ Gap / OracleGain       │
                  │ Complementarity        │
                  │ Cost Gradient          │
                  └──────────┬─────────────┘
                             │ freeze
                             ▼
                  ┌────────────────────────┐
                  │ Frozen Outcome Matrix  │
                  │ query × model          │
                  │ score + cost           │
                  └──────────┬─────────────┘
                             │
                   ┌─────────┴─────────┐
                   ▼                   ▼
             Calibration             Test
                   │                   │
                   ▼                   │
             Quality EWMA              │
                   │                   │
                   └─────────┐         │
                             ▼         │
                          PlanInput ◄──┘
                             │
                             ▼
                         LLooM plan()
                             │
                             ▼
                        selected_model
                             │
                             ▼
                       outcome lookup
                             │
                             ▼
            ┌────────────────────────────────┐
            │ Quality / Cost / AIQ / CI      │
            │ Baseline / Oracle / Selection  │
            └────────────────┬───────────────┘
                             │
                             ▼
                       WebUI / Report
```

这就是本阶段真正要交付的 **LLooM RouterBench v0.1**。

---

## 二十三、v0.1 默认实施参数（评审通过后可直接执行）

本节把前文原则收束成第一轮实验的默认配置。除非出现明确技术阻塞，不再为 v0.1 重新讨论参数。

### 23.1 默认实验对象

```text
Formal Track A · Public Screening
  数据源：RouterBench 0-shot（优先）
  作用：验证 importer / screening / replay / metric pipeline

Formal Track B · Native Pool
  数据源：LLooM 当前真实 API 模型池自采
  作用：正式 headline 结果

Showcase Track
  数据源：MT-Bench frozen outcomes
  作用：演示 Pareto 与 smart routing
```

RouterBench HF 当前公开的 0-shot 文件约 99.6 MB，原始文件另有更大的 raw 版本；它是最适合先打通离线回放链路的小型入口。citeturn971073view2

LLMRouterBench 当前发布包则约 1.28 GB，包含更现代模型和更广数据集覆盖；第一轮不以它作为代码联调的第一数据源，待 replay MVP 稳定后再接入。citeturn827654search0turn913629view3

### 23.2 Native Pool 默认候选

首轮先采用**同系列三档池**，减少模型风格差异带来的额外变量：

```text
weak   = qwen3.6-flash
mid    = qwen3.6-plus
strong = qwen3.6-max-preview
```

原因不是“参数越大越好”，而是：三者均属于当前 LLooM 已经关注的 Qwen 3.6 系列，能力/价格/上下文层级清晰，便于把第一版结果解释成“同一家族不同成本档的 routing”。当前官方信息显示：

- `qwen3.6-flash`：上下文 1M，<=256K 输入约 $0.165/M tokens，输出约 $0.99/M tokens；
- `qwen3.6-plus`：上下文 1M，<=256K 输入约 $0.276/M tokens，输出约 $1.651/M tokens；
- `qwen3.6-max-preview`：上下文 262,144，<=128K 输入约 $1.3/M tokens，输出约 $7.8/M tokens。citeturn987265search4turn829513search2turn829513search6

因此首轮理论上具有明显成本梯度：

```text
Weak → Mid   ≈ 1.7× input / 1.7× output
Mid  → Strong ≈ 4.7× input / 4.7× output
```

这个价格梯度足以在 benchmark 中形成可观察的 cost-saving 空间。

注意：这些价格只用于**当前 Native Track 的 cost basis**；最终运行前由脚本从官方页面/配置文件重新记录一次，并写入 manifest，避免价格漂移。LLooM 当前代码中的价格配置不得被 benchmark 运行时偷偷改写。

### 23.3 第二池：异构模型互补实验

v0.1 Native Pool 首轮通过后再做：

```text
weak   = qwen3.6-flash
mid    = deepseek-flash
strong = qwen3.6-max-preview
```

DeepSeek 当前官方 API 将 `deepseek-flash` 定义为 V4.1-Flash，1M context；其 off-peak 输入/输出价格约为 $0.15/$0.60 每百万 tokens，peak 时更高。citeturn443929search0

这个池的意义不是“更便宜”，而是测试**不同模型家族的互补性**。如果第一池能取得收益、第二池进一步增加 winner diversity，则可以形成很好的研究结论：

```text
同系列 scale routing
          vs
异构 model-family routing
```

### 23.4 Core 数据集首轮固定样本量

Native Pool 第一轮只跑：

```text
MMLU-Pro    300
MATH500     300
GSM8K       300
MBPP        300
IFEval      200
----------------
总计       1,400 instances
```

其中：

```text
Calibration = 20%
Test        = 80%
```

若某数据集没有完整的 20/80 切分或不能稳定自动评分，则先不进入 headline；宁可减少数据集，也不做数据填补。

### 23.5 Screening 阈值

在 Public Track 或 Native calibration split 上先计算：

```text
Coverage
ModelGap
Disagreement
UpgradeOpportunity
CheapSafe
WinnerEntropy
CostRatio
OracleGain
```

v0.1 默认通过门槛：

```text
Coverage        = 100%
OracleGain      > 0
UpgradeOpp      > 5%
CheapSafe       > 10%
CostRatio       >= 3×   （Weak ↔ Strong）
```

这里的阈值是**工程筛选 heuristic，不是论文定理**。目的只是快速识别“值得继续投入 API 成本”的 workload。

若满足门槛的数据集超过 3 个，优先保留：

```text
1. OracleGain 最大
2. WinnerEntropy 较高
3. CostRatio 较高
4. scorer 最稳定
```

### 23.6 三类关键样本必须各抽至少 20 条

正式 demo 数据从 test split 单独固定抽样：

```text
Cheap Win
  Weak 对 / Strong 对

Smart Escalation
  Weak 错 / Strong 对

Mid Sweet Spot
  Mid 对 / Weak 错 / Strong 无额外收益
```

再加：

```text
Failure Case
  LLooM 选择错误且 Oracle 不同
```

最终至少形成：

```text
20 + 20 + 20 + 20 = 80 条 demo evidence set
```

这些 sample_id 在 `demo_manifest.json` 中永久冻结，不允许每次随机变化。

---

## 二十四、v0.1 精确运行顺序

### Step 0 · 环境与数据准备

```bash
mkdir -p benchmarks/routerbench/data/raw
mkdir -p benchmarks/routerbench/data/frozen
mkdir -p benchmarks/routerbench/manifests
mkdir -p benchmarks/routerbench/reports
```

先获取 RouterBench 0-shot；网络可用时优先使用官方/公开发布源。不要直接覆盖仓库中的静态数据目录，原始下载放 `data/raw/`，解包后再导入 frozen schema。

LLMRouterBench 第二阶段按其公开发布包下载命令获取：

```bash
hf download hf://datasets/NPULH/LLMRouterBench/bench-release.tar.gz
```

其公开仓库给出的发布结构是 `results/bench/<dataset>/<split>/<model>/<timestamp>.json`，可以直接编写通用 adaptor。citeturn827654search0turn913629view3

### Step 1 · Public Screening

```bash
python benchmarks/routerbench/import_frozen.py \
  --source routerbench \
  --input benchmarks/routerbench/data/raw/routerbench_0shot.pkl \
  --dataset-id routerbench-public-0shot-v1

python benchmarks/routerbench/screening.py \
  --dataset routerbench-public-0shot-v1 \
  --pool public-weak-mid-strong \
  --split calibration \
  --report benchmarks/routerbench/reports/public-screening.json
```

第一阶段只回答：

```text
哪些 dataset 真正存在 routing headroom？
```

不是追求 benchmark 分数。

### Step 2 · 冻结 v0.1 Dataset Manifest

screening 完成后生成：

```text
v0.1-core.yaml
v0.1-showcase.yaml
v0.1-model-pool.yaml
```

manifest 一旦进入正式 test，就不再修改。

### Step 3 · 建立 Native Pool

先检查三个模型在相同 prompt profile 下能否全部调用；然后逐模型、逐 dataset 固定采集。

采集不是：

```text
router → model
```

而是：

```text
model A → full query set
model B → full query set
model C → full query set
```

确保最终形成完整 `N × M` outcome matrix。

### Step 4 · Native Outcome Scoring

保存：

```text
sample_id
model_name
raw_response
score
input_tokens
output_tokens
cost
latency
```

其中 `score` 必须由官方 scorer / 明确可复现 scorer 生成。

### Step 5 · 冻结 Matrix

运行：

```bash
python benchmarks/routerbench/freeze.py \
  --dataset native-core-v1 \
  --output benchmarks/routerbench/data/frozen/native-core-v1
```

freeze 完成后生成 manifest hash：

```text
benchmark_hash
model_pool_hash
prompt_profile_hash
scorer_hash
split_hash
cost_basis_hash
```

任何 hash 改变都视为新 benchmark version。

### Step 6 · Offline Calibration

```bash
lloom-cli bench calibrate \
  --dataset native-core-v1 \
  --pool lloom-native-3tier
```

这个命令只能消费 calibration split。

必须把最终产生的 quality state 导出为：

```text
calibration_state.json
```

便于重复 replay。

### Step 7 · Offline Replay

```bash
lloom-cli bench replay \
  --dataset native-core-v1 \
  --pool lloom-native-3tier \
  --split test \
  --strategy all
```

执行期间：

```text
API calls = 0
usage_records writes = 0
routing_calibration writes = 0
production model registry writes = 0
```

### Step 8 · 统计与置信区间

```bash
lloom-cli bench compare \
  --run <run_id> \
  --bootstrap 10000
```

默认 bootstrap：

```text
10,000 resamples
seed = 42
95% percentile CI
```

### Step 9 · 生成报告

```bash
lloom-cli bench export \
  --run <run_id> \
  --format json

python benchmarks/routerbench/report.py \
  --run <run_id> \
  --output benchmarks/routerbench/reports/native-v1
```

---

## 二十五、首轮实验必须记录的“黄金表”

无论 UI 怎么变化，最终都要生成一张标准 CSV/JSON：

| 字段 | 含义 |
|---|---|
| benchmark | 数据集名 |
| split | calibration/test |
| sample_id | 样本 ID |
| task_family | 数据集任务族 |
| weak_score | Weak frozen score |
| mid_score | Mid frozen score |
| strong_score | Strong frozen score |
| selected_model | LLooM 选择 |
| selected_score | LLooM 实际 score |
| selected_cost | LLooM 实际 cost |
| oracle_model | hindsight best |
| oracle_score | hindsight score |
| regret | oracle - selected |
| decision_band | LLooM band |
| quality_estimate_weak | 路由时看到的 EWMA |
| quality_estimate_mid | 路由时看到的 EWMA |
| quality_estimate_strong | 路由时看到的 EWMA |
| selection_score_* | 各模型最终 score |
| selection_reason | 选择原因 |

这样未来无论改成 ECharts、CSV、论文图，底层都不用重新跑实验。

---

## 二十六、关键研究实验矩阵

v0.1 最少做以下 6 个 experiment：

```text
E1  Always Strong vs Always Weak
    证明“模型差异确实存在”

E2  Cheapest vs LLooM
    证明 router 不是简单的最低成本策略

E3  Always Strong vs LLooM
    测 Cost Saving / Retention

E4  Oracle vs LLooM
    测 Oracle Regret / Gap Recovery

E5  LLooM w/o calibration vs LLooM calibrated
    证明质量历史确实提供了增益

E6  Qwen-only pool vs heterogeneous pool
    测 model diversity 对 routing 的帮助
```

其中 E5 非常重要。否则一个评审很容易问：

> “你的 router 只是 `task_type + band` 的固定规则，EWMA 到底有没有价值？”

E5 可以直接回答这个问题。

---

## 二十七、v0.1 结果判读规则

不要预先承诺具体数字。

采用如下判读：

### 情况 A · 高质量 + 明显低成本

```text
Retention 高
Cost Saving 高
Regret 低
```

→ **主成功结果**。

### 情况 B · 成本省很多但质量掉很多

```text
Cost Saving 高
Retention 低
```

→ router 过度 downgrade；优先检查 capability gate / band / quality weight。

### 情况 C · 质量很好但成本几乎没降

```text
Retention ≈ Strong
Saving ≈ 0
```

→ router 过度 conservative；重点检查 cost penalty、band 升级阈值。

### 情况 D · LLooM 不如 Cheapest

→ 不视为 benchmark 失败，而视为非常有价值的 diagnostic；检查 Strong/Mid 是否真正被选到正确的 hard cases。

### 情况 E · LLooM 不如 task-family rule baseline

→ 说明当前逐题 router 没有从历史质量中得到额外价值；这会直接指导 v0.2 增加 embedding / kNN / preference predictor。

### 情况 F · OracleGain 很小

→ **先换 benchmark，不急着改 router。**

这点非常重要：没有 routing headroom 时，继续调权重很容易把 benchmark overfit。

---

## 二十八、v0.1 最终交付物清单

### A. 数据

```text
[ ] RouterBench public raw snapshot
[ ] RouterBench normalized frozen matrix
[ ] Native Pool frozen matrix
[ ] MT-Bench showcase matrix（可选但推荐）
```

### B. Manifest

```text
[ ] model_pool.yaml
[ ] datasets.yaml
[ ] task_mapping.yaml
[ ] prompt_profile.yaml
[ ] demo_manifest.json
```

### C. Engine

```text
[ ] bench.rs
[ ] public importer
[ ] native importer
[ ] screening.py
[ ] calibration state builder
[ ] replay CLI
[ ] baseline evaluator
[ ] AIQ scorer
[ ] bootstrap CI
```

### D. Evidence

```text
[ ] benchmark screening report
[ ] baseline table
[ ] Pareto / AIQ chart data
[ ] per-instance evidence table
[ ] failure cases
[ ] zero-cost replay log
```

### E. Presentation

```text
[ ] RouterBench overview
[ ] Pareto chart
[ ] selection distribution
[ ] decision explorer
[ ] 80-instance demo evidence set
```

---

## 二十九、当前信息收集状态与网络限制

截至 2026-10-04，本轮可以确认的公开信息已经足以确定 schema 与实施路线：

1. RouterBench 公开矩阵存在，并且其数据结构天然适合 frozen outcome replay；
2. RouterBench 项目本身已有 train/eval dataset split 工具，可作为“不得 sample overlap”的参考实现；
3. LLMRouterBench 已公开更大的现代 benchmark release，提供统一的 performance/performance-cost 结果结构；
4. RouterArena 已明确将“evaluation-only / no tuning on evaluation data”作为 router benchmark 的方法学要求，可作为 LLooM 的泄漏防护参考；
5. 当前 Qwen / DeepSeek 官方页面可确认 Native Pool 的 context 与价格层级。citeturn787626view0turn913629view3turn868237search0turn987265search4turn829513search6turn443929search0

本环境本轮仍无法可靠直接下载 GitHub/Hugging Face 的大体积二进制数据包，因此**已完成“数据结构/字段/下载入口/版本状态”的信息确认，但尚未在当前环境成功落地完整 99.6 MB / 1.28 GB 数据文件**。这不阻塞工程设计，但 Step 0 必须在开发机上执行真实下载并保存 checksum。

---

## 三十、从这里开始，不再改方案：第一批代码提交顺序

正式开工时建议直接按以下 commit 粒度推进：

```text
commit 1
bench: add frozen matrix schema + manifests

commit 2
bench: add RouterBench importer

commit 3
bench: add dataset screening metrics

commit 4
bench: add offline PlanInput builder

commit 5
bench: add zero-cost replay engine

commit 6
bench: add baseline evaluators + oracle

commit 7
bench: add AIQ + bootstrap CI

commit 8
bench: add native lm-eval importer

commit 9
bench: add calibration state replay

commit 10
webui: add RouterBench overview + Pareto

commit 11
webui: add per-instance decision explorer

commit 12
bench: add heterogeneous pool experiment
```

每个 commit 都应满足：

```text
cargo test
cargo clippy
```

并尽量保持 benchmark data / UI / production routing 三者解耦。

---

## 三十一、实施原则（最终冻结）

最后把本项目压缩成 8 条不可变原则：

```text
1. RouterBench 评测的是“选模型”，不是“模型本身”。

2. Frozen Outcome Matrix 是 benchmark 的核心资产。

3. Test split 的任何 score 都不能进入路由状态。

4. Benchmark replay 必须复用 production plan()/score_all()。

5. Zero-cost = replay 阶段 0 LLM/API calls，不是“不需要事先采集数据”。

6. 先 screening 再花 API 钱；没有 routing headroom 就不进入 headline。

7. 正式结果优先 Native Pool，Public Matrix 主要负责复现与对照。

8. 不预设“LLooM 必赢”；输给 baseline 时，报告原因而不是调整 benchmark。
```

**到这里，LLooM RouterBench v0.1 已经从“一个评测想法”变成可以直接执行的实验协议。**
