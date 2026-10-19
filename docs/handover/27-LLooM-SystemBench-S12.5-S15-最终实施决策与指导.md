# 27 · LLooM SystemBench S12.5–S15 最终实施决策与指导

> 日期：2026-10-07
> 状态：执行版总指导
> 基线：`origin/v2 @ c97d688`
> 上游：`24-SystemBench进度盘点与决策文档` / `25-LLooM-SystemBench-S12-S15-实施决策与执行指导-v5` / `26-SystemBench-S12.2-S12.4-零成本段总结与检查点`
> 目的：在 S12.4 完成、进入 S12.5/S12.6 花钱段之前，重新冻结 SystemBench v0.2 的实验语义、数据契约、成本 Gate、Oracle、统计和后续 L2/L3 路线。

---

# 一、结论先行

当前 SystemBench 已完成从“工程骨架”到“可运行 Frozen Composition”的跃迁，S12.2–S12.4 又完成了第一批真实 decomposition 的 semantic 化与 routing-context 检验。

下一步不应回头修改 RouterBench v0.2，也不应直接扩大采集规模或跳入 Live benchmark。

正式路线固定为：

```text
RouterBench v0.2
    │
    └── Frozen / Maintenance

SystemBench v0.1
    │
    └── Frozen Control Composition

S12.5
    │
    ├── Provider Probe
    ├── Routing Input Contract Freeze
    ├── 5-workflow Semantic Outcome Probe
    └── System Pool Audit

        ↓ GO

S12.6
    ├── L1 Semantic Outcome Matrix
    └── L2 Conditional Outcome Matrix

        ↓

S12.7
    └── SystemBench v0.2 Freeze

        ↓

S13
    ├── Historical Source Audit
    ├── Artifact Replay
    ├── State Replay
    └── Counterfactual Validity

        ↓

S14
    └── SystemBench v0.2 Main Result

        ↓

S15
    └── L3 Live External Validation
```

核心原则：

> **S12 的目标不是把 case 做“大”，而是把 SystemBench 从“结构上成立”升级成“语义上成立”；S13 才解决真实 artifact/state 传播；S14 才形成完整 system-level quantitative result；S15 最后验证真实环境。**

---

# 二、当前状态判定

## 2.1 RouterBench

RouterBench v0.2 已完成：

```text
Calibration Tournament
Freeze
Frozen Test
Matched Random
Budget Curve
Bootstrap CI
Cross-Benchmark
Native decision
```

当前定位：

```text
RouterBench v0.2 = Frozen Research Result
RouterBench     = Maintenance
```

原则：

```text
不再重新调 λ
不重新搜索 pool
不为了 SystemBench 修改 RouterBench v0.2
```

---

## 2.2 SystemBench v0.1

已完成：

```text
80 cases / 327 nodes
Exact binding 100%
L1 replay
Production route seam
Scheduling headline
Matched Random
Recovery ablation infrastructure
O2 Schedule Oracle
O3 Joint Oracle
Workflow-group paired bootstrap
Frozen manifest
Shadow parity
```

当前结论：

> **SystemBench v0.1 已足够作为冻结的 control-plane baseline，不需要推翻。**

正式定义：

```text
SystemBench v0.1
= L1 Frozen Control Composition
```

而不是：

```text
End-to-End Agent Benchmark
```

---

## 2.3 S11

15 个真实 decomposition 已全部通过 canonical validator，平均 4.33 节点，总 API 消费约 `$0.02`。

最重要发现不是节点数量，而是：

```text
PlanningBench reference
≠
LLM decomposition reference
```

因此 lexical evaluator 的 `tau=0.45` 不应为了拟合 PlanningBench 而修改。

PlanningBench 在当前体系中的角色正式调整为：

```text
verification artifact
```

而不是：

```text
decomposition gold reference
```

---

## 2.4 S12.2–S12.4

当前实际数据：

```text
30 semantic workflows
10 exact-bound control workflows
40 total cases
207 nodes
175 semantic nodes after atomicity split
32 control nodes in routing check
```

S12.3 发现：

```text
22 / 126 original nodes
≈ 17.5%
```

存在复合职责，触发 atomicity 拆分；这应作为 decomposition quality finding 保留，而不只是脏数据修复。

S12.4 最关键发现：

| Routing Input | Hard | Medium | Easy | Multi-model Eligible |
|---|---:|---:|---:|---:|
| Full Prompt | 91.4% | 8.6% | 0% | 8.6% |
| Description Only | 56.0% | 36.6% | 7.4% | 44.0% |

这说明：

> **routing input 本身就是 routing search-space 的控制变量。**

这是当前 SystemBench 最重要的新研究观察之一。

---

---

# 三、决策 D1：采用双 Routing Input Track，但正式化命名

## 3.1 三种 input 必须分开

一个 system node 后续必须明确保存：

```text
root_goal
routing_input
execution_input
```

建议 schema：

```yaml
routing_input:
  mode: description
  text: "..."
  hash: "..."

execution_input:
  mode: root_goal_plus_description
  text: "..."
  hash: "..."
```

L2 之后：

```yaml
execution_input:
  mode: root_goal_plus_description_plus_artifacts
  artifact_refs:
    - n1
    - n2
```

---

## 3.2 R1 · Production-Visible = headline

```text
routing_input = description
```

这是当前 production-faithful track，因为当前子任务路由决策实际可见的是 description。

因此 S12.4 的：

```text
56.0% hard
36.6% medium
7.4% easy
44.0% multi-model eligible
```

应该成为后续 v0.2 routing experiment 的主要口径。

---

## 3.3 R2 · Context-Rich = diagnostic

```text
routing_input = root_goal + description
```

用于研究：

```text
additional context
↓
complexity band
↓
model eligibility
↓
routing search-space
```

当前观察：

```text
multi-model eligible
44.0% → 8.6%
```

建议增加诊断指标：

```text
Routing Context Compression
=
Eligible(R2) / Eligible(R1)
≈ 8.6 / 44.0
≈ 19.5%
```

该指标当前仅作为 diagnostic，不作为优化目标。

---

## 3.4 核心纪律

不能因为 R1 数字更漂亮就把它作为 headline。

唯一依据是：

```text
production router 当前实际看到什么
```

如果生产代码未来变更 routing input：

```text
new policy
→
new manifest
→
new benchmark identity
```

不能污染旧 v0.2。

---

# 四、决策 D2：LLM 辅助标注 + Headline 100% Human Sign-off

S12.3 已证明关键词启发式不适合直接承担 semantic gold。

因此：

```text
LLM pre-label
↓
Schema validator
↓
Human review
↓
Headline gold
```

允许：

```text
LLM
→ role suggestion
→ atomicity suggestion
→ artifact type suggestion
```

但最终：

```text
review_status = human_approved
```

才可进入 headline。

20% spot-check 仍保留为：

```text
annotation QA statistic
```

而不是 gold 的替代品。

---

# 五、D2 的标注对象

每个 headline node 必须人工最终确认：

```text
semantic_role
atomic_ok
dependency_semantics
artifact_contract
completion relevance
```

并记录：

```text
annotation_version
annotator
review_status
review_timestamp
```

---

# 六、S12.3 已发现的 Atomicity Finding 应保留为正式 Diagnostic

目前：

```text
22 / 126
≈ 17.5%
```

原 planner nodes 被判断为 composite。

因此以后报告：

```text
atomicity_rate
composite_node_rate
mean_nodes_before_normalization
mean_nodes_after_normalization
```

同时必须区分：

```text
raw_planner_workflow
```

与：

```text
normalized_canonical_workflow
```

不能让读者误解 SystemBench headline 直接使用 planner 原始输出。

---

# 七、增加 Normalization Rate

定义：

```text
Normalization Rate
=
被 benchmark normalization 修改的 raw node 数
/
raw planner node 总数
```

修改包括：

```text
atomicity split
missing required field repair
schema normalization
future dependency repair
```

它是 workflow provenance 的核心指标。

---

# 八、S12.5：严格采用 Provider Probe → Pool Probe → Audit

不要直接执行 175 semantic nodes × 3 models。

正式顺序：

```text
S12.5.0 Provider Probe
        ↓
S12.5.1 Routing / Execution Contract Freeze
        ↓
S12.5.2 5-workflow Semantic Probe
        ↓
S12.5.3 System Pool Audit
        ↓
      GO / STOP
```

只有 GO：

```text
S12.6 Full Harvest
```

---

# 九、S12.5.0 Provider Probe

规模：

```text
3 candidate models
×
2 representative nodes
```

只检查：

```text
provider reachability
model identity
schema stability
JSON validity
latency
cost recording
cost > 0 / unknown handling
```

Provider Probe 任何关键项失败：

```text
STOP
```

---

# 十、S12.5.2 Semantic Probe

优先选择：

```text
multi_model_eligible ≥ 20%
```

优先级更高：

```text
≥ 50%
```

同时控制 semantic role 覆盖：

```text
retrieval
reasoning
calculation
verification
synthesis
```

不要让 probe 被单一 reasoning 节点垄断。

---

# 十一、S12.5.3 System Pool Audit

R7：

```text
deephermes
+
deepseek_r1_distill
+
intern_s1_mini
```

只能作为：

```text
candidate pool
```

不能默认成为：

```text
SystemBench v0.2 Best Pool
```

因为 RouterBench 与 Semantic Workflow 的 node distribution 已经不同。

---

## 11.1 Pool Audit 指标

至少重新计算：

```text
quality headroom
cost ratio
cheap-safe
weak opportunity
mid opportunity
winner entropy
cross-workflow stability
multi-model eligibility
```

特别增加：

```text
eligible_mid_rate
eligible_mid_oracle_share
eligible_mid_selected_share
eligible_weak_rate
eligible_weak_oracle_share
eligible_weak_selected_share
```

目的：

```text
mid 不可用
```

与：

```text
mid 可用但 router 不选
```

必须被区分。

---

# 十二、Pool Audit 的 GO / STOP

## GO

满足：

```text
至少两个 model tiers 在 Semantic Nodes 上稳定互补
```

且：

```text
quality gap 非零
cost leverage 非零
winner diversity 跨 workflow 存在
routing gain 不是少数异常 node 驱动
```

则：

```text
→ Full Harvest
```

## STOP

发现：

```text
models 高度同质
```

或者：

```text
gain 全来自少数异常 node
```

或者：

```text
band eligibility 是唯一来源
```

则停止采集，优先修改：

```text
pool
semantic set
routing contract
```

---

# 十三、S12.6：Semantic Outcome Matrix 必须分 L1 / L2

```text
Semantic Outcome Matrix
│
├── L1 Matrix
│   └── independent node × model
│
└── L2 Conditional Matrix
    └── artifact-dependent node × model | artifact
```

---

## 13.1 L1 Matrix

允许进入：

```text
routing headline
workflow cost
routing regret
node oracle
```

每个 cell 至少：

```json
{
  "score": 0.0,
  "success": true,
  "cost": 0.0,
  "latency_ms": 0,
  "model_id": "...",
  "evaluator_id": "...",
  "prompt_hash": "..."
}
```

---

## 13.2 L2 Conditional Matrix

需要额外：

```text
upstream_artifact_hashes
artifact_schema_hash
execution_input_hash
state_hash
```

只用于：

```text
conditional quality
artifact-conditioned routing
L2 replay
```

不能把 L2 frozen artifact 结果伪装成真正 end-to-end live result。

---

---

# 十四、Evaluator 优先级

semantic node outcome 的 evaluator 按以下顺序：

```text
1. deterministic evaluator
2. exact/reference evaluator
3. LLM judge
```

只有无法稳定确定性评分的节点才进入 judge。

如果使用 judge，必须 freeze：

```text
judge_model
judge_prompt_hash
rubric_hash
evaluator_version
```

并分开核算：

```text
model_cost
judge_cost
```

最终 SystemBench frontier 默认同时报告：

```text
model cost
judge cost
combined cost
```

避免“模型省钱、judge 把钱花回去”却只报告模型成本。

---

# 十五、Cost 口径正式统一

统一规则：

```text
cost > 0
→ observed cost

cost = 0
→ unknown
```

除非数据源明确声明 free。

因此：

```text
unknown ≠ free
```

所有 oracle / frontier / cost-per-success 默认只使用 known-cost rows。

另外单独报告：

```text
unknown_cost_rows
unknown_cost_rate
```

---

# 十六、S12.7 · SystemBench v0.2 Freeze Gate

不能以“case ≥ 30”作为唯一条件。

必须满足：

```text
[ ] semantic workflows ≥ 20
[ ] headline nodes 100% human-approved
[ ] artifact contracts 100% complete
[ ] dependency semantics 100% complete
[ ] routing_input frozen
[ ] execution_input frozen
[ ] L1 exact outcome binding complete
[ ] L2 conditional data separately identified
[ ] System Pool frozen
[ ] evaluator frozen
[ ] cost basis frozen
[ ] system-case split frozen
[ ] control / headline no unintended overlap
[ ] routing relevance coverage adequate
[ ] manifest hashes complete
```

Freeze 后任一以下变化：

```text
semantic annotation
routing policy
routing input
execution context
evaluator
cost basis
pool
split
```

都必须生成：

```text
new systembench_id
```

---

# 十七、O3 Joint Oracle：改为 ≤8 Nodes 全精确

当前最大 workflow 为 8 nodes，三模型 assignment 数量：

```text
3^8 = 6561
```

这个规模完全可以 exhaustive search。

因此正式规则：

```text
≤8 nodes
→
all assignment exhaustive
```

每个 assignment：

```text
model assignment
↓
legal schedule
↓
cost / latency / quality
```

然后求 joint optimum。

正式 headline 不再使用：

```text
greedy approximate O3
```

近似算法可以继续作为 scalability experiment，但不得与 exact O3 使用同一个 label。

---

# 十八、SystemBench Baseline 因果隔离矩阵

最终固定：

| Baseline | Planning | Routing | Scheduling | Recovery | 目的 |
|---|---|---|---|---|---|
| B0 Flat Strong | Flat | Strong | N/A | Off | 单强模型 |
| B1 Flat BestSingle | Flat | BestSingle | N/A | Off | 简单竞争基线 |
| B2 Fixed + Strong Seq | Fixed | Strong | Sequential | Off | baseline |
| B3 Fixed + Strong Par | Fixed | Strong | Parallel | Off | scheduling gain |
| B4 Fixed + LLooM Router | Fixed | LLooM | Parallel | Off | routing gain |
| B5 LLooM Workflow + Strong | LLooM | Strong | LLooM | Off | orchestration gain |
| B6 Full LLooM | LLooM | LLooM | LLooM | Off |联合控制 |
| B7 Full + Recovery | LLooM | LLooM | LLooM | On | recovery gain |
| O1 Node Oracle | Fixed | Node Oracle | Reference | Off | local upper bound |
| O2 Schedule Oracle | Fixed | Fixed | Oracle | Off | schedule upper bound |
| O3 Joint Oracle | Fixed | Joint | Oracle | Off | joint upper bound |

---

# 十九、Gain 分解

正式定义：

```text
Scheduling Gain
= B3 − B2
```

```text
Routing Gain
= B4 − B3
```

```text
Orchestration Gain
= B5 − B3
```

```text
Full Gain
= B6 − B3
```

对于 benefit-oriented metric，统一成：

```text
higher = better
```

再计算：

```text
Interaction Gain
=
Full Gain
− Routing Gain
− Orchestration Gain
```

Cost / latency 等 lower-is-better metric 必须先统一方向，禁止直接使用 raw delta 混算。

---

# 二十、SystemBench Primary / Secondary / Diagnostic

## Primary

```text
Goal Success
Final Goal Quality
Cost / Successful Workflow
Makespan
```

## Secondary

```text
Node Quality
Strong Share
Routing Regret
Recovery Rate
```

## Diagnostic

```text
Planning Validity
Atomicity Rate
Normalization Rate
Dependency P/R
Parallel Width
Sequentialization Waste
Coordination Cost Ratio
Failure Attribution
Routing Context Compression
```

当前：

```text
Q_node
```

只能是 secondary / diagnostic，不得描述成 final workflow quality。

---

# 二十一、Coordination Tax 的正式表达

旧值：

```text
18.35
```

正式同时报告：

```text
Coordination Cost Ratio = 19.35×
Coordination Overhead = +1835%
```

再提供绝对：

```text
actual workflow cost
local-min baseline cost
absolute delta
```

避免 18.35 被误解为 18.35%。

---

# 二十二、Recovery 实验升级

当前 n=10 只证明状态机与传播，不足以证明 gain。

后续建议：

```text
30–40 failure cases
```

覆盖：

```text
recoverable
partially recoverable
non-recoverable
```

比较：

```text
Recovery Off
vs
Recovery On
```

指标：

```text
Goal Success
Recovery Rate
Partial Success
Extra Cost
Extra Latency
Abort Rate
```

只有 bootstrap CI 足够稳定后，才对外使用：

```text
Recovery Gain
```

---

# 二十三、Workflow-Level Paired Bootstrap

bootstrap unit：

```text
system_case
```

不能：

```text
node
```

建议：

```text
B = 10,000
seed = 3407
workflow-group paired bootstrap
```

至少输出：

```text
ΔGoalSuccess
ΔFinalQuality
ΔCost/Success
ΔLatency
ΔStrongShare
```

所有 policy 在相同 system cases 上配对比较。

---

# 二十四、Split / Leakage

split unit：

```text
system_case
```

hash：

```text
sha256("lloom-sbsplit:3407:" + system_case_id)
```

禁止：

```text
same root goal
same workflow family
same source prompt
```

以变体形式跨 calibration/test。

任何：

```text
pool selection
threshold tuning
annotation-derived normalization rule
evaluator tuning
```

必须只读取 calibration。

Test：

```text
read-only
```

---

# 二十五、S13 · L2 Feasibility Pipeline

S13 不直接定义成“τ² benchmark”。

正式拆成：

```text
S13.0 Source Audit
        ↓
S13.1 Trajectory Schema
        ↓
S13.2 Artifact Audit
        ↓
S13.3 Counterfactual Validity
        ↓
S13.4 L2 Replay
```

---

# 二十六、S13.0 · Source Audit

候选数据源必须记录：

```text
repository commit
dataset version
task hash
trajectory hash
evaluator version
license
model identity
tool state availability
artifact availability
```

不同 evaluator / grading version：

```text
禁止直接混合 headline
```

---

# 二十七、S13.1 · Trajectory Schema

至少：

```json
{
  "task_id": "...",
  "steps": [
    {
      "step_id": "...",
      "input_state_hash": "...",
      "model_id": "...",
      "output_hash": "...",
      "artifact_refs": [],
      "tool_events": [],
      "output_state_hash": "..."
    }
  ],
  "final_state_hash": "..."
}
```

---

# 二十八、S13.2 · Artifact Audit

确认：

```text
producer output
↓
consumer input
```

真实存在数据依赖。

至少验证：

```text
artifact type
artifact hash
producer
consumer
schema compatibility
```

---

# 二十九、S13.3 · Counterfactual Validity

只有：

```text
model changes
↓
artifact changes
↓
downstream input changes
↓
state changes
```

才称：

```text
counterfactual replay
```

否则只能是：

```text
decision replay
```

必须记录：

```text
original_artifact_hash
counterfactual_artifact_hash
downstream_input_hash
state_hash
```

---

# 三十、S14 · Main Result

SystemBench v0.2 Main Result 固定使用：

```text
B0–B7
O1–O3
```

同时报告：

```text
Routing Gain
Orchestration Gain
Scheduling Gain
Interaction Gain
Recovery Gain
```

和：

```text
Goal Success
Final Quality
Cost / Success
Makespan
Strong Share
```

再给：

```text
Quality × Cost
Quality × Latency
Quality × Strong Budget
```

以及 failure propagation：

```text
root cause
↓
propagation
↓
terminal failure
```

---

# 三十一、S15 · Live External

Live 必须在以下 Gate 全部满足之后进行：

```text
[ ] RouterBench frozen
[ ] SystemBench v0.2 frozen
[ ] semantic gold complete
[ ] outcome matrix complete
[ ] system pool frozen
[ ] evaluator frozen
[ ] L2 feasibility passed
[ ] counterfactual leakage = 0
[ ] recovery evidence adequate
[ ] paired statistics stable
[ ] cost accounting complete
```

第一阶段：

```text
10–30 sanity
```

第二阶段：

```text
50–100 held-out
```

最后才考虑：

```text
τ²
AppWorld
WebArena
```

---

# 三十二、当前明确不做

```text
❌ 不修改 RouterBench v0.2
❌ 不重新调 P2 λ / penalty
❌ 不预设 R7 为 SystemBench 最佳 pool
❌ 不直接 Full Harvest 175 semantic nodes
❌ 不用 fuzzy binding / nearest query
❌ 不把 PlanningBench checklist 当 decomposition gold
❌ 不为错误 reference 调 tau
❌ 不把 Q_node 当 final workflow quality
❌ 不把 cost=0 当 free
❌ 不把 historical trajectory 自动称作 counterfactual
❌ 不直接 full τ² / AppWorld / WebArena
❌ 不建立 composite SystemScore
❌ 不以 case 数作为唯一 freeze 条件
```

---

# 三十三、当前最重要的研究问题

SystemBench v0.2 固定回答六个问题：

## RQ1 · Routing

> 子任务级 model routing 能否在真实 semantic workflow 上降低模型成本而保持目标质量？

## RQ2 · Scheduling

> workflow-aware scheduling 能否降低 makespan，而不是只减少理论 wave 数？

## RQ3 · Orchestration

> decomposition 本身能否创造独立于 routing 的 system gain？

## RQ4 · Interaction

> routing 与 orchestration 联合后是否产生额外协同收益？

## RQ5 · Recovery

> failure recovery 是否能把局部节点失败转化成更高的 workflow success？

## RQ6 · Global Optimality

> node-level local optimum 是否等于 workflow-level optimum？

---

# 三十四、增加一个研究问题：什么时候不应该拆

定义：

```text
Decomposition ROI
```

比较：

```text
Single Strong
vs
Orchestrated Strong
```

观察：

```text
ΔGoalSuccess
ΔFinalQuality
ΔCost
ΔLatency
```

最终识别：

```text
Decomposition Benefit
```

与：

```text
Decomposition Tax
```

这会直接反馈 production complexity detector。

---

# 三十五、未来方向：Context Budget

S12.4 已证明 routing context 会显著改变 search space。

后续可形成：

```text
C0 = description
C1 = description + root goal
C2 = description + compressed state
C3 = full trajectory context
```

研究：

```text
context amount
↓
routing eligibility
↓
model selection
↓
quality / cost
```

当前不实现为 production policy，仅作为 v0.3+ 研究方向。

---

# 三十六、未来方向：Trajectory-Level Routing

当前 RouterBench：

```text
query → model
```

SystemBench 后续：

```text
workflow state
→ node
→ model
```

最终 routing input 可包括：

```text
node description
root goal
completed artifacts
remaining budget
failure state
workflow position
```

这会把 LLooM 的 routing 从 one-shot decision 升级为 state-aware inference control。

---

# 三十七、版本边界

| Version | 定位 | 核心能力 |
|---|---|---|
| SystemBench v0.1 | Frozen Control Composition | router + schedule + replay |
| SystemBench v0.2 | Semantic Workflow Composition | semantic nodes + L1/L2 outcome |
| SystemBench v0.3 | Artifact / Historical Replay | trajectory + artifact + counterfactual |
| SystemBench v0.4 | Live External | real model + tool + state |

原则：

```text
v0.1
= benchmark machinery correctness

v0.2
= semantic validity + routing usefulness

v0.3
= downstream propagation validity

v0.4
= real-world validity
```

---

# 三十八、最终实施时间线

## Phase S12.5

```text
Provider Probe
↓
Input Contract Freeze
↓
5 Workflow Semantic Probe
↓
Eligibility-conditioned Pool Audit
↓
GO / STOP
```

## Phase S12.6

```text
L1 Semantic Matrix
+
L2 Conditional Matrix
↓
Evaluator / Cost Freeze
```

## Phase S12.7

```text
Human Sign-off
↓
Manifest
↓
Bootstrap
↓
Coverage Audit
↓
v0.2 Freeze
```

## Phase S13

```text
Source Audit
↓
Trajectory Schema
↓
Artifact Audit
↓
Counterfactual Validity
↓
L2 Replay
```

## Phase S14

```text
B0–B7
O1–O3
Gain Decomposition
Frontiers
Failure Propagation
CI
```

## Phase S15

```text
10–30 Live Sanity
↓
50–100 Held-out
↓
External Validation
```

---

# 三十九、最终 Go / No-Go Matrix

| 项目 | 当前 | 下一步 | Go 条件 |
|---|---|---|---|
| RouterBench v0.2 | ✅ | maintenance | 不修改 |
| SystemBench v0.1 | ✅ | frozen baseline | 不修改 |
| Semantic cases | ✅ 30 | S12.5 | annotation complete |
| Annotation | ⚠️ pending human | review | headline 100% approved |
| Routing Input | ⚠️ 双口径 | contract freeze | R1/R2 固定 |
| System Pool | ⚠️ R7 candidate | Probe | 至少两个 tier 真正互补 |
| L1 Matrix | ⬜ | Harvest | pool audit GO |
| L2 Matrix | ⬜ | conditional harvest | contracts complete |
| O3 | ⚠️ partly approximate | exact ≤8 | exhaustive |
| Recovery | ⚠️ n=10 | expand | 30–40 failure cases |
| S13 | ⬜ | source audit | artifact/state 可用 |
| S14 | ⬜ | main result | v0.2 frozen |
| S15 | ⬜ | live | all live gates green |

---

# 四十、实施纪律

1. **先冻结语义，再采 outcome。**
2. **先做 routing-space / pool audit，再花钱。**
3. **L1 / L2 / L3 严格分层。**
4. **Exact binding 永远优先。**
5. **Benchmark policy 与 production policy 分离。**
6. **Oracle 必须明确 exact / approximate。**
7. **System-level bootstrap 必须以 workflow 为 unit。**
8. **Evaluator / cost / input / policy 任何变化都生成新 identity。**
9. **Failure 必须保留 root cause → propagation → terminal outcome。**
10. **任何仅仅改善数字、却没有改善 measurement validity 的修改，应停止。**

---

# 四十一、最终决策摘要

### 决策 1

**采用 production-faithful description-only 作为 R1 headline，full prompt 作为 R2 diagnostic。**

### 决策 2

**Semantic headline node 必须 100% human-approved；LLM 只负责预标。**

### 决策 3

**S12.5 严格采用 Provider Probe → 5-workflow Probe → Pool Audit → GO/STOP。**

### 决策 4

**R7 不直接冻结为 SystemBench Best Pool；必须在 semantic nodes 上重新验证。**

### 决策 5

**L1 independent 与 L2 artifact-dependent outcome 分开建矩阵。**

### 决策 6

**O3 ≤8 nodes 改为全精确枚举。**

### 决策 7

**S13 先做 L2 feasibility，再谈 counterfactual；historical trajectory 本身不等于 counterfactual。**

### 决策 8

**RouterBench v0.2 彻底进入 maintenance，不再为了 SystemBench 回头修改。**

---

# 四十二、现在真正应该执行什么

当前唯一推荐的下一组动作：

```text
S12.5.0
Provider Probe

↓

S12.5.1
Routing / Execution Input Contract Freeze

↓

S12.5.2
5-workflow Semantic Outcome Probe

↓

S12.5.3
Eligibility-conditioned System Pool Audit

↓

STOP / GO
```

**在这四步完成以前，不做 175-node 全量采集，不做 S13，不做 Live。**

如果 GO：

```text
S12.6 Full Harvest
→
S12.7 v0.2 Freeze
```

如果 STOP：

```text
修正 pool / semantic set / routing contract
```

而不是继续消耗 API 预算。

---

# 四十三、最终研究叙事

整个 LLooM 最终不应被讲成：

```text
RouterBench
+
另一个 Agent Benchmark
```

而应成为一条逐层增加 fidelity 的证据链：

```text
Layer 1 · RouterBench

不是所有 query 都需要最强模型。

        ↓

Layer 2 · Workflow Track

不是所有复杂任务都应该一次性完成。

        ↓

Layer 3 · SystemBench v0.1

模型选择、任务调度和失败传播可以在统一控制面正确组合。

        ↓

Layer 4 · SystemBench v0.2

真正的 semantic workflow 中，routing 与 orchestration 能否产生系统收益。

        ↓

Layer 5 · L2 Replay

上游 artifact / state 的变化能否真实传播到 downstream decision。

        ↓

Layer 6 · Live

这些控制策略能否在真实模型、工具和环境中继续成立。
```

最终希望证明的不是：

> “LLooM 用了更多便宜模型。”

而是：

> **LLooM 能理解任务结构，把合适的模型放到合适的工作节点，在满足依赖和时间约束的情况下完成整个目标，并在质量、成本、延迟和可靠性之间形成更好的系统级 frontier。**

---

# 四十四、Current Working State

截至本指导文档：

```text
SystemBench S12.2–S12.4
✅ complete

S12.5
⏸ waiting for execution

S12.6
⏸ GO/STOP dependent

S12.7
⏸ pending harvest

S13
⏸ post-freeze

S14
⏸ post-L2 evidence

S15
⏸ post-system gate
```

**下一执行入口：`S12.5.0 Provider Probe`。**
