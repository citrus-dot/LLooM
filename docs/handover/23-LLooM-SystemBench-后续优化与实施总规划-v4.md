# 23 · LLooM SystemBench 后续优化与实施总规划 v4

> 日期：2026-10-07
> 定位：**SystemBench 当前阶段的正式决策与下一阶段执行入口**
> 上游：`20-SystemBench实施总结-合并验收与PhaseAB.md`、`21-LLooM-SystemBench-决策与改进指导-v3.md`、`22-SystemBench实施总结-S10-S11.md`
> 当前版本：`SystemBench v0.1` Frozen Composition 已建立；S10 Hardening 已完成；S11 真实 decomposition pilot 已完成；下一阶段进入 `S12 Semantic Workflow v0.2`
> 核心原则：**不推翻已经成立的 L1；先把语义与因果链做真，再进入 historical artifact/state replay，最后才做 live。**

---

## 一、执行摘要

### 1. 当前正式判断

LLooM 当前不需要再回头扩大 RouterBench，也不需要继续对 RouterBench v0.2 做常规 tuning。

RouterBench v0.2 已经完成：

- calibration tournament；
- policy / pool freeze；
- frozen test replay；
- cost-matched / strong-share-matched random；
- budget analysis；
- workflow-independent bootstrap；
- cross-benchmark validation；
- Native hold 决策。

因此 RouterBench 进入 maintenance 状态，继续实验必须由新的研究问题触发，而不是为了继续堆数字。

SystemBench 当前也已经跨过了“能不能把两条 benchmark 线路接起来”的阶段：

```text
RouterBench v0.2
       +
Workflow Track v0.1
       ↓
SystemBench v0.1
       ↓
L1 Frozen Composition
       ↓
Production routing parity
```

SystemBench v0.1 已经具备：

- canonical SystemCase；
- exact node binding；
- workflow-group split；
- L1 outcome replay；
- production `route_single` / `router::plan()` seam；
- scheduling composition；
- failure / recovery 基础传播；
- workflow-level paired bootstrap；
- Shadow parity；
- manifest / identity 治理。

### 2. 但是 v0.1 的证据边界必须明确

当前 v0.1 **不是完整的 end-to-end agent benchmark**。

它最准确的定义是：

> **SystemBench v0.1 = Workflow-aware L1 Frozen Composition Benchmark。**

当前已经可以严谨支持：

```text
固定 workflow
+
frozen node outcomes
+
LLooM routing
+
合法 scheduling
+
确定性 replay
```

从而测量：

```text
routing cost gain
scheduling latency gain
local model-choice regret
system-level cost / quality trade-off
```

但目前还不能严谨声称已经证明：

```text
真实 semantic decomposition quality
artifact propagation
state-dependent routing
真实 failure recovery gain
真实 end-to-end task success
```

这些应该成为 v0.2 / L2 / Live 的后续证据。

### 3. 总路线正式收束

```text
RouterBench v0.2
        ↓
     frozen
        ↓
SystemBench v0.1
L1 Frozen Composition
        ↓
S12 Semantic Workflow
        ↓
S13 L2 Historical / Artifact Replay
        ↓
S14 SystemBench v0.2 Main Result
        ↓
S15 L3 Live External
```

核心思想不是“把 benchmark 做得更大”，而是：

> **让 benchmark 从结构正确 → 语义正确 → 状态正确 → 真实执行正确。**

---

# 二、当前工作状态的最终盘点

## 2.1 RouterBench：冻结，不再主开发

RouterBench v0.2 的研究结果已经完成研究闭环。

核心结论：

- R7 stability pool 在 calibration tournament 胜出；
- P2 frozen policy 在 llmrouterbench test 上相对 Always-Strong saving = 13.46%；
- retention 约 100.05%；
- cost-matched random 与 strong-share-matched random 均被超过；
- bootstrap CI 已完成；
- cross-benchmark 证明机制方向一致，但模型代际导致 rescue 幅度不同；
- Native 决策为 hold。

因此：

```text
RouterBench
status = frozen / maintenance
```

后续除非出现新的研究问题，否则不重新：

- 搜模型池；
- 调 λ；
- 调 penalty；
- 重挑 test；
- 修改 v0.2 benchmark identity。

RouterBench 继续作为 SystemBench 的 **frozen source benchmark**。

---

## 2.2 Workflow Track：离线基础层已经成立

原内部 `OrchestrationBench` 方案已经实现：

- workflow schema；
- validator；
- PlanningBench adapter；
- PlanBench adapter；
- planning evaluator；
- scheduling simulator；
- router bridge；
- F1-F7 failure injection；
- P1-P11 attribution。

但后续对外命名建议统一使用：

```text
LLooM SystemBench
├── Routing Track
│   └── RouterBench
│
├── Workflow Track
│   └── LLooM Workflow Track
│
└── System Track
    ├── Frozen
    ├── Shadow
    ├── Historical Replay
    └── Live
```

内部目录 `benchmarks/orchestration/` 可保留，避免工程迁移成本。

原因：公开社区已经存在名为 **OrchestrationBench / OrchBench** 的独立研究工作，未来对外论文、README、搜索和引用应避免名称歧义。

---

## 2.3 SystemBench：v0.1 当前真实成果

Phase A/B + S10 + S11 已完成。

目前已经实现的关键能力：

```text
SystemCase
  ↓
Exact Binding
  ↓
Frozen Outcome
  ↓
LLooM Router
  ↓
Scheduler
  ↓
Replay
  ↓
Workflow metrics
```

S10 已补齐：

- scheduling headline；
- matched random；
- recovery ablation；
- cost validity；
- coverage audit；
- O2/O3 oracle；
- Gate A。

S11 已完成真实 decomposition pilot：

- 15 PlanningBench 实例；
- qwen-plus；
- temperature=0；
- 15/15 canonical validator 通过；
- 真实 workflow morphology 已获得第一批样本；
- 总成本约 $0.02。

---

# 三、S11 的真正意义：不是“tau 校准失败”，而是发现了错误的 gold semantics

这是下一阶段设计必须吸收的最重要发现之一。

S11 证明：

```text
PlanningBench checklist
        ≠
LLM decomposition subtask reference
```

实际 lexical similarity max 仅约 0.049，而当前 tau=0.45。

因此：

```text
planning recall = 0
```

不能解释成：

```text
LLM 不会 decomposition
```

而应该解释为：

```text
evaluator target 与模型输出属于不同语义层
```

所以当前 tau **保持不变**。

### 3.1 正式重新定义 PlanningBench 的角色

PlanningBench 后续只作为：

```text
verification artifact
```

而不是：

```text
decomposition gold reference
```

### 3.2 真正的 planning evaluator 校准对象

后续只能使用：

```text
PlanBench goal atom
```

或：

```text
S12 人工标注 semantic reference
```

进行校准。

### 3.3 严禁的“错误修复”

不要为了得到非零 recall：

```text
把 tau 从 0.45 调到 0.05
```

因为这只是让 evaluator 接受错误的语义匹配，不是提高 evaluator 正确性。

---

# 四、SystemBench v0.1 的正式能力边界

必须把目前 SystemBench 拆成四个 fidelity level。

## L0 · Schedule Replay

输入：

```text
DAG
+ dependency
+ duration
+ waves
```

输出：

```text
wave_count
critical_path
makespan
parallel_width
illegal_parallelism
sequentialization_waste
```

回答：

> scheduler 有没有把一个已经确定的 workflow 正确安排起来？

---

## L1 · Outcome Replay

输入：

```text
workflow
+
node prompt
+
model outcome
+
quality / success
+
cost
+
latency
```

LLooM 只负责：

```text
routing
scheduling
recovery policy
composition
```

这是当前 v0.1 的主 track。

---

## L2 · Artifact / Historical Replay

输入升级为：

```text
historical trajectory
+
actual output
+
artifact
+
state transition
```

允许真正模拟：

```text
A output
  ↓
B input
  ↓
B output
  ↓
C input
```

回答：

> 一个节点的结果改变以后，后续节点真的会怎样？

---

## L3 · Live Execution

真实：

```text
LLM
+ tools
+ environment
+ state
+ wall-clock
+ provider cost
```

回答：

> LLooM 在真实系统里是否真的完成得更好、更便宜、更快、更可靠？

---

# 五、最重要的数据原则：SystemBench 不再只是 Workflow Matrix，而是 SystemCase + Binding Ledger

SystemCase 是顶层对象。

推荐最终 canonical：

```json
{
  "system_case_id": "sb_v02_0001",
  "root_goal": "...",
  "workflow_ref": {
    "workflow_id": "...",
    "workflow_hash": "..."
  },
  "nodes": [
    {
      "node_id": "n1",
      "prompt": "...",
      "semantic_role": "retrieval",
      "dependency_semantics": "independent",
      "artifact_contract": {
        "produces": [],
        "consumes": []
      },
      "outcome_bindings": [
        {
          "mode": "llmrouterbench_exact",
          "source_sample_id": "...",
          "prompt_hash": "..."
        }
      ]
    }
  ],
  "target": {
    "evaluator": "...",
    "success_rule": "..."
  },
  "replay_level": "L1"
}
```

### 5.1 Binding mode 封闭枚举

只能允许：

```text
routerbench_exact
llmrouterbench_exact
historical_artifact
synthetic_fixture
live_only
```

禁止：

```text
lookup
nearest
similar
embedding_match
guess
```

### 5.2 headline 的 binding 要求

Headline system result 中：

```text
binding_exactness = 100%
```

Derived binding 可以存在，但必须：

```text
diagnostic_only = true
```

不得进入主要 headline。

---

# 六、S12：Semantic Workflow v0.2

S12 是当前最重要的下一阶段。

它不是简单：

```text
15 → 40
```

而是要把 workflow 从“结构合法”升级成“语义真实”。

建议分成四个子阶段。

---

## S12-A · Annotation Specification

先写规范，再开始大量标注。

每个 node 至少包含：

### 1. semantic_role

建议初版词表：

```text
retrieval
search
extraction
classification
calculation
transformation
reasoning
verification
synthesis
execution
```

词表后续允许扩展，但必须 versioned。

### 2. atomicity

判断一个 node 是否只承担一个明确职责。

例如：

```text
搜索资料并分析并总结
```

通常不应作为一个 node。

### 3. dependency_semantics

正式采用：

```text
independent
artifact_dependent
state_dependent
```

### 4. artifact_contract

建议：

```json
{
  "produces": [
    {
      "type": "facts",
      "required": true
    }
  ],
  "consumes": [
    {
      "type": "facts",
      "from": ["n1"],
      "required": true
    }
  ]
}
```

### 5. completion_condition

明确：

```text
required_nodes
optional_nodes
success_condition
aggregation_condition
```

不要默认所有 workflow 都采用：

```text
all nodes succeeded
```

---

## S12-B · Workflow Set 重建

建议总量先控制在：

```text
30 semantic workflows
```

不要立即扩大到几百。

建议：

```text
10 simple
10 medium
10 complex
```

但“simple/medium/complex”不再只按 node 数定义，还需要结合：

```text
node_count
depth
width
dependency_density
artifact_dependency_ratio
state_dependency_ratio
```

### 推荐最小覆盖

必须出现：

```text
chain
fan-out
fan-in
parallel branches
diamond
mixed DAG
```

同时避免所有 case 都是：

```text
hard → strong
```

---

## S12-C · Band Coverage Gate

当前 Shadow 已第三次证明：长 prompt 会大量进入 hard band。

因此 S12 必须增加：

```text
Band Coverage Gate
```

至少报告：

```text
band distribution
eligible model count distribution
strong-only node rate
weak-eligible node rate
mid-eligible node rate
```

建议目标不是“人为制造 easy”，而是：

> **让真实 semantic workflow 中确实出现多种模型可行性。**

如果所有节点都天然 strong-only，则该 case 对 routing 研究价值很低，应进入 diagnostic，不进入 routing headline。

---

## S12-D · Semantic Outcome Matrix

这是 S12 必须新增的核心数据资产。

真实 decomposition node 往往不是旧 RouterBench query，因此：

```text
semantic node
    ≠
old RouterBench query
```

不要 fuzzy matching。

对挑选出的高价值 semantic workflows 建立：

```text
semantic node
×
locked model pool
→
quality / success / cost / latency
```

得到：

```text
Semantic Outcome Matrix
```

这样才真正能做：

```text
semantic workflow
×
subtask routing
```

的研究。

---

# 七、System Pool：从“继承 R7”改成“继承优先、重新审计”

当前 SystemBench 使用 RouterBench v0.2 的 R7 pool 是合理的第一版决策。

但不能假定：

```text
RouterBench Best Pool
=
SystemBench Best Pool
```

因为：

```text
RouterBench query distribution
≠
semantic subtask distribution
```

所以 S12 对 semantic outcome 完成后，应做一个轻量的：

```text
System Pool Audit
```

至少看：

```text
oracle_gain
cheap_safe
winner_entropy
weak_oracle_share
model-cost ratio
band eligibility
cross-workflow stability
```

注意：

```text
System Pool Audit
```

产生：

```text
system_pool_v02
```

不修改 RouterBench v0.2。

---

# 八、Oracle 体系升级

当前已经有 O0/O1/O2/O3，但建议在 S12 直接把 O3 做成真正的 exact joint oracle。

当前 workflow 最大约 8 nodes，三模型 assignment 的空间为：

```text
3^8 = 6561
```

这个规模可以直接精确枚举。

### O0 · Node Min-Cost

只回答：

> 每个节点理论上最便宜要多少钱？

用于 cost lower bound。

### O1 · Node Oracle

每个节点独立选择最优模型。

回答：

> 如果每个节点都单独选最好模型，能做到什么？

### O2 · Schedule Oracle

固定 workflow + assignment，采用：

```text
critical path
```

作为合法调度下界。

### O3 · Joint Oracle

对固定 workflow：

```text
all model assignments
×
optimal legal schedule
```

做精确搜索。

回答：

> 如果 router + scheduler 联合最优，当前 workflow 最多能做到什么？

### 暂不做

暂时不要加入：

```text
topology oracle
```

也就是不要同时搜索：

```text
decomposition
model assignment
schedule
```

否则会把 v0.2 变成组合爆炸问题。

---

# 九、Recovery：从“状态机正确”升级为“可量化收益”

当前 recovery ablation 结果：

```text
n = 10
completion 0% → 0%
recovered workflow = 1
```

只能作为工程验证，不能作为研究结论。

S12/S13 应补一个：

```text
Recovery Challenge Set
```

建议：

```text
30–40 failure cases
```

覆盖：

```text
F1-F7
```

并尽量包含：

```text
recoverable
partially recoverable
non-recoverable
```

正式比较：

```text
Recovery Off
vs
Recovery On
```

指标：

```text
Goal Success
Recovered Rate
Partial Success
Retry Count
Fallback Count
Extra Cost
Extra Latency
Abort Rate
```

这样才能出现真正可报告的：

```text
Recovery Gain
```

---

# 十、Coordination Tax 重新规范

当前：

```text
Coordination Tax = B4 cost / local-min-cost - 1
```

数值可能非常大，也容易误读。

以后同时报告：

```text
coordination_cost_ratio
coordination_overhead_pct
```

例如：

```text
ratio = 19.35×
overhead = +1835%
```

并明确：

```text
unknown cost ≠ free cost
```

统一规则：

```text
cost > 0
→ observed

cost = 0
→ unknown

source explicitly declares free
→ free
```

未知成本不得进入：

```text
Oracle
cost lower bound
Coordination Tax
headline frontier
```

---

# 十一、SystemBench 的 Completion / Quality 需要正式拆层

不能继续让：

```text
Q_node
```

承担 workflow final quality 的含义。

正式采用三个层次：

```text
Node Quality
Workflow Completion
Final Goal Quality
```

### Node Quality

节点级模型 outcome。

### Workflow Completion

例如：

```text
required nodes succeeded
AND
required dependencies satisfied
AND
no illegal parallelism
```

适合 L1。

### Final Goal Quality

真正评估：

```text
最终答案
或
最终环境状态
```

必须在 L2/L3 才逐步进入 headline。

---

# 十二、SystemBench 应采用因果隔离实验矩阵

建议正式冻结下面这组 baseline：

| ID | Planning | Routing | Scheduling | Recovery | 研究问题 |
|---|---|---|---|---|---|
| B0 | flat | strong | — | off | 单强模型基线 |
| B1 | flat | best-single | — | off | 简单 routing 基线 |
| B2 | fixed | strong | sequential | off | workflow + 串行 |
| B3 | fixed | strong | parallel | off | scheduling gain |
| B4 | fixed | LLooM | parallel | off | routing gain |
| B5 | LLooM | strong | LLooM | off | orchestration gain |
| B6 | LLooM | LLooM | LLooM | off | joint gain |
| B7 | LLooM | LLooM | LLooM | on | full system |
| O0 | fixed | node-min-cost | reference | off | cost lower bound |
| O1 | fixed | node oracle | reference | off | local model upper bound |
| O2 | fixed | fixed | schedule oracle | off | scheduling upper bound |
| O3 | fixed | joint oracle | joint oracle | off | combined upper bound |

核心差分：

```text
B2 → B3 = Scheduling Gain
B3 → B4 = Routing Gain
B3 → B5 = Orchestration Gain
B4 → B6 = Routing × Orchestration Interaction
B6 → B7 = Recovery Gain
```

其中：

```text
Interaction Gain
=
Full Gain
− Routing Gain
− Orchestration Gain
```

具体符号方向应在实现时统一为“改善量为正”。

---

# 十三、SystemBench 必须增加 matched random

RouterBench 已证明 matched-random 能把“便宜”与“会挑”区分开。

SystemBench 也应保留这个能力。

### R1 · Cost-matched Random

匹配：

```text
expected workflow cost
```

### R2 · Strong-share-matched Random

匹配：

```text
strong-call share
```

但注意：

如果 target 低于模型混合的可达成本下界，应标记：

```text
partial_match
```

而不能把它描述成“完美 cost matched”。

最终应报告：

```text
target
actual
relative gap
```

---

# 十四、SystemBench 的统计必须统一到 workflow-level paired bootstrap

当前做法正确，应继续保持：

```text
bootstrap unit = SystemCase
```

不能：

```text
node = independent sample
```

因为一个 workflow 内：

```text
nodes
share
context
state
budget
workflow structure
```

### Headline CI

至少：

```text
Δ Goal Success
Δ Final Quality
Δ Cost / successful workflow
Δ Latency
Δ Strong Share
Δ Recovery Success
```

采用：

```text
paired bootstrap
B=10,000
seed=3407
```

并保持同一 cases、同一 ordering、同一 evaluator。

---

# 十五、SystemBench 的 split 需要升级为“workflow isolation”

当前已经采用 workflow-group hash split，这是正确方向。

未来必须进一步保证：

```text
同一 root goal family
同一 workflow template
同一 artifact template
同一 trajectory
```

不跨 calibration/test 泄漏。

推荐 split unit：

```text
system_case
```

并禁止：

```text
node-level split
```

此外，S12 Semantic Workflow 如果由同一个 planning prompt template 生成多个变体，应建立：

```text
group_id
```

按 group 整组切分。

---

# 十六、L1 Counterfactual 的硬科学边界

只有：

```text
dependency_semantics = independent
```

的 workflow，才能做严格的：

```text
model assignment counterfactual
```

例如：

```text
A → Weak
B → Strong
```

只能在 B 不依赖 A output 的情况下，安全解释为：

```text
改变 A 的模型
```

否则：

```text
A artifact
→
B input
```

B 的 outcome 可能已经改变。

所以：

```text
independent
→ L1
artifact_dependent
→ L2
state_dependent
→ L3
```

这是 SystemBench 的核心方法学红线。

---

# 十七、S12 Semantic Outcome Matrix 的成本控制

不建议为全部 30 workflow × 全部节点 × 全部模型做大规模采集。

推荐两阶段：

### S12-1

```text
30 semantic workflows
```

先做 annotation 和结构检查。

### S12-2

从中筛：

```text
10–15 high-value workflows
```

重点要求：

```text
多模型可选
存在明显 cost difference
存在一定 oracle headroom
```

然后采集：

```text
node × locked pool
```

### 选 pool 的原则

先继承：

```text
RouterBench R7
```

再做 System Pool Audit。

如果发现 semantic node 上 R7 失去互补性，再另行建立 system pool。

---

# 十八、S13：L2 Historical / Artifact Replay

S13 不应该一开始就等价于：

```text
下载 τ²
→ 跑 benchmark
```

应该分成三个阶段。

## S13-A · Source Feasibility Audit

首先检查：

```text
trajectory 是否完整
model identity 是否完整
input 是否完整
output 是否完整
tool event 是否完整
state 是否完整
evaluator 是否可复现
license 是否允许使用
version 是否可以 pin
```

任何一项缺失，都可能导致无法做严格 counterfactual。

---

## S13-B · Historical Replay

先不做 counterfactual。

只证明：

```text
历史 trajectory
→ replay
→ identical outcome
```

至少验证：

```text
artifact
state
milestone
termination
```

---

## S13-C · Counterfactual Replay

只有当数据满足：

```text
同一 task
同一步骤
存在多个 model outcome
或存在可验证替代轨迹
```

才允许做：

```text
Original policy
vs
LLooM policy
```

并显式记录：

```text
counterfactual_valid = true/false
```

不能把所有 historical trajectory 都叫 counterfactual evidence。

---

# 十九、S13 优先数据源建议

优先级不再单纯按照“最有名”排序，而按：

```text
可获得性
×
轨迹完整性
×
状态完整性
×
可重放性
×
license
×
可验证性
```

建议顺序：

```text
1. LLooM 自己已有 historical trace
2. τ² historical trajectory
3. ToolSandbox recorded trajectory
4. AppWorld state / trace
5. WebArena / other long-horizon logs
```

不要因为某个 benchmark 名气大，就强行作为 L2 source。

---

# 二十、S14：SystemBench v0.2 Main Result

只有完成以下条件后，才正式出 v0.2 main result：

```text
Semantic Workflow >= 20
Exact / audited bindings
Band coverage acceptable
System pool frozen
O3 exact joint oracle
Recovery challenge set
L2 至少一小部分可复现
```

然后正式运行：

```text
B0-B7
O0-O3
R1-R2
```

并输出：

```text
Goal Success
Final Quality
Workflow Cost
Cost / success
Median Latency
P90 Latency
Strong Share
Recovery Rate
Oracle Gap
Routing Gain
Orchestration Gain
Interaction Gain
```

---

# 二十一、System-level Pareto

不要只给一个 System Score。

正式保留三张 frontier：

```text
1. Goal Success × Cost
2. Goal Success × Latency
3. Goal Success × Strong-call Budget
```

同时报告：

```text
Cost / successful workflow
Latency / successful workflow
```

在存在预算约束时，用约束优化表达：

```text
Max Quality
subject to Cost <= B
```

或：

```text
Min Cost
subject to Quality >= Q
```

避免人为定义一个固定权重 composite score。

---

# 二十二、SystemBench 的核心新指标

## 22.1 Decomposition ROI

用于衡量：

```text
增加多少 orchestration 成本
换来多少 final success / quality
```

建议：

```text
Decomposition ROI
=
ΔFinal Goal Quality / ΔWorkflow Cost
```

或者对于二元任务：

```text
ΔGoal Success / ΔCost
```

由于 denominator 可能接近 0，headline 同时报告原始差值，不只报告 ratio。

---

## 22.2 Coordination Tax

用于衡量：

```text
实际 workflow cost
vs
局部 cost lower bound
```

保持：

```text
ratio
+
overhead%
```

---

## 22.3 Routing Interaction Gain

用于衡量：

```text
Routing
×
Orchestration
```

是否存在超出简单加和的收益。

---

## 22.4 Recovery Efficiency

建议同时报告：

```text
recovered success / extra cost
```

即：

> 为恢复一个原本失败的 workflow，平均多花多少成本？

这比单纯：

```text
recovery rate
```

更有产品意义。

---

# 二十三、Failure Attribution 从“错误类型”升级为“传播链”

当前 Workflow Track 已经拥有 P1-P11。

SystemBench 不需要推翻，而是在其上增加：

```text
root_cause
    ↓
propagation_edges
    ↓
terminal_failure
```

例如：

```text
Wrong Routing
    ↓
Bad Artifact
    ↓
Downstream Failure
    ↓
Recovery Trigger
    ↓
Partial Success
    ↓
Final Goal Failure
```

每个 case 允许：

```text
primary_error
secondary_errors[]
propagation_trace[]
```

最终做：

```text
failure incidence
+
failure impact
```

而不是只有百分比。

---

# 二十四、Shadow 的目标重新定义

Shadow 不应该被理解为“接下来就跑 live”。

它主要回答：

```text
Production Decision Path
       =
Benchmark Decision Path
```

当前已有 284 个 test nodes parity zero mismatch。

下一阶段应把 coverage 做成正式指标：

```text
shadow_total_nodes
shadow_routable_nodes
shadow_excluded_nodes
shadow_coverage_rate
exclusion_reason
```

以后每次 production code / benchmark 代码变化，都必须：

```text
shadow parity
→ zero mismatch
```

才能修改 benchmark。

---

# 二十五、Production Code 与 Benchmark Code 的边界

继续坚持：

```text
Benchmark
   ↓
production function / seam
```

而不是：

```text
production logic
复制到 benchmark
```

允许的改动：

```text
pure function
trait
read-only seam
public test hook
```

禁止：

```text
benchmark-only routing formula
benchmark-only scheduler semantics
hidden policy override
```

任何为了 benchmark 新增的生产 seam 都要：

```text
production behavior unchanged
```

并由 parity test 覆盖。

---

# 二十六、Manifest / Identity 治理

SystemBench identity 必须继承下层 identity，同时增加系统层 hash。

建议至少包含：

```text
routerbench_ref_hash
workflow_schema_hash
workflow_cases_hash
model_pool_hash
routing_policy_hash
scheduler_policy_hash
failure_policy_hash
evaluator_hash
split_hash
cost_basis_hash
system_manifest_hash
```

任何核心输入改变，都产生新的：

```text
systembench_id
```

版本建议：

```text
v0.1
= L1 Frozen Composition

v0.2
= Semantic Workflow

v0.3
= Artifact / State Replay

v1.0
= Live validated benchmark
```

版本定义以“证据能力”而不是“提交次数”划分。

---

# 二十七、对外研究叙事建议重新收束

最终不要介绍成：

```text
我们做了三个 benchmark
```

而应该：

```text
LLooM
│
├── Routing Track
│   └── RouterBench
│
├── Workflow Track
│   └── Workflow evaluation
│
└── SystemBench
    ├── Frozen
    ├── Shadow
    ├── Replay
    └── Live
```

三个层次分别回答：

### Routing

> 不是所有 query 都需要最强模型。

### Workflow

> 不是所有复杂目标都应该由单个模型一次性完成。

### System

> 真正重要的是：复杂任务能否被合理组织，并把正确模型分配给正确工作，在时间、成本和最终质量之间形成更优的系统 frontier。

SystemBench 的差异点应该强调：

```text
cost-aware routing
+
workflow orchestration
+
subtask-level model assignment
+
parallel scheduling
+
recovery
```

而不是声称 LLooM 发明了 DAG、多 agent 或 parallel execution。

---

# 二十八、与近期公开研究的对齐

近期 agent routing / orchestration 研究已经明显从传统：

```text
one query → one model
```

向：

```text
trajectory step
→
model decision
```

发展。

TwinRouterBench 将评测拆成静态与动态两条轨道：静态轨道做 execution-verified、deterministic 的 step-level routing 评估，动态轨道再进入真实 agent execution，并直接用最终任务解决率与实际 API spend 评估。这个范式与本项目：

```text
L1 Frozen
→
L2 Replay
→
L3 Live
```

高度一致。

同时，近期 OrchBench 一类工作尝试用 deterministic simulation 隔离 orchestration plan 的质量，把 workflow DAG、parallelism、context / budget constraints 与最终 outcome 进行可解释的离线模拟。这进一步说明：**离线 composition / simulation 并不是 live benchmark 的低配版，而是独立且有价值的研究层。**

因此 LLooM 的路线应该继续强调：

```text
offline causal isolation
→
historical state replay
→
live validation
```

而不是直接把所有东西压进一次 live score。

---

# 二十九、S12–S15 具体排期

## S12 · Semantic Workflow v0.2

### S12.0

```text
Annotation Spec
```

产物：

```text
workflow_annotation_spec.md
```

### S12.1

```text
15 seeds re-audit
```

产物：

```text
semantic_seed_audit.json
```

### S12.2

```text
扩到 30 semantic workflows
```

产物：

```text
system_cases_v02.jsonl
```

### S12.3

```text
artifact_contract
dependency semantics
completion condition
band coverage
```

### S12.4

```text
System Pool Audit
```

### S12.5

从 30 workflows 中筛出：

```text
10–15 high-value workflows
```

### S12.6

建立：

```text
Semantic Outcome Matrix
```

### S12.7

冻结 candidate：

```text
lloom-systembench-v0.2-candidate
```

---

## S13 · L2 Historical Replay

### S13.0

```text
source feasibility audit
```

### S13.1

```text
trajectory parser
artifact/state schema
```

### S13.2

```text
historical replay
```

要求：

```text
identical
```

### S13.3

```text
counterfactual validity audit
```

### S13.4

```text
10–30 valid historical cases
```

---

## S14 · SystemBench v0.2 Main Result

固定：

```text
System pool
Routing policy
Workflow set
Evaluator
Oracle
```

运行：

```text
B0-B7
O0-O3
R1-R2
```

输出：

```text
paired bootstrap
frontier
failure propagation
oracle gap
interaction gain
```

---

## S15 · L3 Live External

第一轮：

```text
10–30 cases
```

第二轮：

```text
50–100 held-out cases
```

之后才考虑：

```text
external leaderboard
```

---

# 三十、每个阶段的停止条件

## Stop S12

满足：

```text
semantic workflows >= 20
artifact contracts complete
band coverage audited
system pool frozen
semantic outcome matrix available
```

否则不要进入 S13。

---

## Stop S13

满足：

```text
historical trajectory replay identical
artifact/state replay reproducible
counterfactual validity explicitly audited
>=10 valid cases
```

否则不要把 L2 结果当 headline。

---

## Stop S14

满足：

```text
B0-B7 complete
O0-O3 complete
matched random complete
paired bootstrap complete
failure propagation complete
```

才形成正式 SystemBench v0.2 main result。

---

## Stop S15

满足：

```text
live sanity pass
cost accounting pass
latency accounting pass
final evaluator pass
held-out test pass
```

才考虑 production-facing benchmark。

---

# 三十一、必须保留的“不做清单”

接下来暂时不做：

```text
1. 不继续扩大 RouterBench v0.2
2. 不修改 RouterBench frozen test
3. 不用 tau 调参制造 planning score
4. 不对 semantic node 做 fuzzy outcome matching
5. 不把 synthetic DAG 当 semantic planning evidence
6. 不把 historical trajectory 自动称为 counterfactual
7. 不做大规模 L2 before feasibility audit
8. 不直接 full AppWorld / WebArena / τ² live
9. 不创建 composite System Score
10. 不把 benchmark P2 policy 自动当生产 policy
11. 不为了 headline 人工挑 test
12. 不让 benchmark 自己复制 production router / orchestrator 逻辑
```

---

# 三十二、风险矩阵

| 风险 | 概率 | 影响 | 处理 |
|---|---:|---:|---|
| Semantic workflow 难以定义 | 中 | 高 | 先做 annotation spec + 小样本停点 |
| 模型 pool 在 semantic task 上失去互补性 | 中 | 高 | System Pool Audit |
| semantic node 无现成 frozen outcome | 高 | 高 | 建立 Semantic Outcome Matrix |
| Planning evaluator 无合适 gold | 高 | 中 | 人工 semantic reference / PlanBench |
| historical trajectory 不可 counterfactual | 中 | 高 | validity audit，分离 replay 与 CF |
| recovery effect 太小 | 中 | 中 | 扩 failure case，不调 evaluator |
| scheduler 已接近 oracle | 高 | 低 | 把 scheduling 作为 baseline / regression，不强求新 gain |
| cost source 缺定价 | 中 | 中 | unknown/free 明确分离 |
| benchmark / production seam 漂移 | 低 | 高 | Shadow parity gate |
| 共享工作树误提交 | 中 | 中 | path-qualified add/commit + staged ownership check |

---

# 三十三、最终实验图谱

SystemBench 最终应形成：

```text
                         User Goal
                             │
                             ▼
                    Complexity Detection
                             │
                    ┌────────┴────────┐
                    │                 │
                  Flat             Complex
                    │                 │
                    ▼                 ▼
                 Router          Decomposition
                                      │
                                      ▼
                                Workflow Graph
                                      │
                         ┌────────────┼────────────┐
                         ▼            ▼            ▼
                       Router       Router       Router
                         │            │            │
                         ▼            ▼            ▼
                        M1           M2           M3
                         └────────────┼────────────┘
                                      ▼
                                  Scheduler
                                      │
                         ┌────────────┴────────────┐
                         ▼                         ▼
                      Parallel                  Sequential
                         │                         │
                         └────────────┬────────────┘
                                      ▼
                                  Execution
                                      │
                              ┌───────┴───────┐
                              ▼               ▼
                           Success         Failure
                                               │
                                           Recovery
                                               │
                                               ▼
                                          Aggregation
                                               │
                                               ▼
                                           Final Goal
```

每一层都要能回答：

```text
为什么这样决定？
相比什么更好？
距离 Oracle 多远？
如果失败，错在哪里？
这个结果是否可以重放？
这个结果是否可以归因？
```

---

# 三十四、最终工程架构建议

保持：

```text
benchmarks/
├── routerbench/
├── orchestration/
└── systembench/
    ├── manifests/
    ├── cases/
    ├── bindings/
    ├── adapters/
    ├── replay/
    │   ├── l0_schedule.py
    │   ├── l1_outcome.py
    │   ├── l2_artifact.py
    │   └── l3_live.py
    ├── runner/
    ├── evaluator/
    ├── oracle/
    ├── reports/
    └── tests/
```

其中：

```text
adapters
```

负责 source access；

```text
bindings
```

负责 source → node exact mapping；

```text
replay
```

负责 fidelity；

```text
runner
```

负责策略执行；

```text
evaluator
```

负责指标；

```text
oracle
```

负责理论上界。

避免把所有逻辑继续堆进一个 `composition.py`。

---

# 三十五、最终建议的产物清单

S12 至少新增：

```text
manifests/
├── workflow_annotation_v1.yaml
├── system_pool_v02.yaml
└── semantic_system_manifest_v02.json

cases/
├── system_cases_v02.jsonl
├── semantic_bindings_v02.jsonl
└── artifact_contracts_v02.jsonl

reports/
├── semantic_workflow_audit.json
├── band_coverage_v02.json
├── system_pool_audit_v02.json
└── semantic_outcome_matrix_v02.json
```

S13 新增：

```text
replay/
├── l2_artifact.py
└── l2_counterfactual.py

reports/
├── trajectory_replay_v01.json
└── counterfactual_validity_v01.json
```

S14 新增：

```text
reports/
├── systembench_v02_main.json
├── systembench_v02_main.md
├── oracle_gap_v02.json
├── interaction_gain_v02.json
├── frontier_v02.json
└── failure_propagation_v02.json
```

---

# 三十六、最终决策表

| 事项 | 决策 |
|---|---|
| RouterBench v0.2 | 冻结 / maintenance |
| Workflow Track | 保留，作为 SystemBench 下层能力 |
| `OrchestrationBench` 对外命名 | 不再作为 LLooM 独立对外 benchmark 名 |
| SystemBench v0.1 | 保留为 L1 Frozen baseline |
| S12 Semantic Workflow | **立即开始** |
| S11 tau | **保持 0.45，不再调** |
| L1 fuzzy matching | **禁止** |
| System Pool | R7 作为起点，S12 重新 audit |
| O3 | 升级为 exact joint oracle |
| Recovery | 扩 failure challenge，再做定量收益 |
| S13 τ² / history | 先 feasibility audit |
| Counterfactual | 必须显式 validity 标记 |
| System score | 不做 composite score |
| L2 | S12 之后进入 |
| Live | S14 后再进入 |
| Native RouterBench | 继续 hold |

---

# 三十七、最终研究路线

整个项目最终应该收束成：

```text
               LLooM
                 │
      ┌──────────┴──────────┐
      │                     │
   Routing               Workflow
      │                     │
 RouterBench          Workflow Track
      │                     │
      └──────────┬──────────┘
                 ▼
            SystemBench
                 │
        ┌────────┼────────┐
        ▼        ▼        ▼
       L1       L2       L3
     Frozen   Replay    Live
        │        │        │
        ▼        ▼        ▼
   Causal      State     Real
 Composition  Aware    Validation
```

对应研究问题：

```text
Routing
“哪个模型更适合这个工作？”

Workflow
“这个目标应该怎样被组织？”

System
“组织以后，怎样把合适模型放到合适节点，
并以合理的成本、延迟和可靠性完成最终目标？”
```

最终真正希望得到的不是一个漂亮的单数字，而是一条可以复现、可以拆解、可以反事实分析、可以最终 live 验证的证据链。

---

# 三十八、参考资料与外部对齐

以下外部资料只用于方法论与 benchmark 设计对齐，不替代项目自身数据：

1. **TwinRouterBench** — step-level routing；Static + Dynamic 双轨；强调 router-visible trajectory prefix、执行验证与实际 spend。
   - https://arxiv.org/abs/2605.18859
   - https://github.com/CommonstackAI/TwinRouterBench

2. **LLMRouterBench** — 大规模 routing benchmark；performance / cost-aware routing；强调简单基线、model recall、模型池策划与 latency-aware 分析。
   - https://arxiv.org/abs/2601.07206
   - https://aclanthology.org/2026.findings-acl.1881/

3. **APB / Agent Planning Benchmark** — 将 planning 与 downstream execution diagnostic 地拆开，并覆盖 long-horizon planning、tool noise、refinement 等维度。
   - https://arxiv.org/abs/2606.04874
   - https://github.com/Mikivishy/AgentPlanningBenchmark

4. **OrchBench** — 采用 deterministic simulation 评估多 agent orchestration plan，强调 DAG、并行性、上下文/预算约束和可解释的离线评测。
   - https://arxiv.org/abs/2607.25656

这些工作共同支持本方案的核心方法论：

```text
先做可重复的离线因果隔离
        ↓
再做历史状态 / artifact replay
        ↓
最后做真实在线执行
```

而 LLooM 的差异化研究重点应继续放在：

```text
routing
+
workflow orchestration
+
subtask model assignment
+
parallel scheduling
+
recovery
+
workflow-level cost / quality / latency optimization
```

---

# 三十九、最终执行口令

从本文件开始，默认下一阶段执行顺序固定为：

```text
S12.0 Annotation Spec
        ↓
S12.1 Semantic Seed Audit
        ↓
S12.2 30 Semantic Workflows
        ↓
S12.3 Contract + Dependency + Completion
        ↓
S12.4 Band Coverage
        ↓
S12.5 System Pool Audit
        ↓
S12.6 Semantic Outcome Matrix
        ↓
S12.7 v0.2 Candidate Freeze
        ↓
S13 L2 Feasibility + Replay
        ↓
S14 Main Result
        ↓
S15 Live
```

除非出现新的实证证据，否则不改变上述阶段顺序。

---

# 四十、最终判断

**当前 SystemBench 不缺“规模”，缺的是“语义真实性和因果闭环”。**

S10 已经把 L1 的工程问题基本收干净；S11 又成功发现了一个非常有价值的方法学问题：错误不在 tau，而在 PlanningBench gold 与 decomposition target 的语义错位。

因此下一步最合理的动作不是重新调 evaluator，也不是继续增加 benchmark 数量，而是：

> **建立真正的 Semantic Workflow，并为这些新 workflow 建立真实、可审计、可冻结的 model outcome matrix。**

一旦这一层成立，后面的 L2 artifact/state replay、trajectory-aware routing、workflow-level oracle 和最终 live evaluation 都会顺着同一个 canonical SystemCase 自然接上。

这才是 LLooM 从：

```text
query → model
```

真正升级到：

```text
task state
→ workflow
→ node state
→ model
→ schedule
→ artifact/state
→ next decision
→ final goal
```

的关键一步。
