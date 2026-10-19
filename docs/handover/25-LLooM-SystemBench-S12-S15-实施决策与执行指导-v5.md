# 25 · LLooM SystemBench S12–S15 实施决策与执行指导 v5

> 日期：2026-10-07
> 状态：**下一阶段正式执行入口**
> 基线：`SystemBench v0.1` Frozen，`RouterBench v0.2` Frozen / maintenance
> 上游：`20-SystemBench实施总结-合并验收与PhaseAB` / `21-决策与改进指导-v3` / `22-SystemBench实施总结-S10-S11` / `23-后续优化与实施总规划-v4` / `24-SystemBench进度盘点与决策文档`
> 核心目的：在不破坏既有 Frozen 证据的前提下，把 SystemBench 从 **L1 Frozen Composition** 稳步推进到 **Semantic Workflow → Artifact/State Replay → Live**，形成第一条真正可对外解释的系统级证据链。

---

# 一、最终决策摘要

## 1.1 当前状态判断

当前 SystemBench 已经完成：

```text
合并验收
↓
Phase A/B
↓
L1 Frozen Composition
↓
S10 Hardening
↓
S11 Real Decomposition Pilot
↓
S12.0 Annotation Spec
↓
S12.1 Seed Audit
```

因此，当前**不是继续补基础设施**，也不是回头继续优化 RouterBench。

当前真正进入的是：

```text
Semanticization
```

即把已经“结构上合法”的 workflow，升级成“语义上可解释、依赖上可验证、节点上可路由、结果上可复现”的 workflow。

---

## 1.2 RouterBench 决策

**RouterBench v0.2 继续冻结，进入 maintenance。**

不再把以下工作列为主线：

```text
重新搜索 R7
继续调 λ
继续调 SoftGate penalty
扩大 RouterBench 数据
重新寻找 headline test
```

RouterBench 已经完成自身研究闭环：

```text
LLMRouterBench
→ pool tournament
→ policy freeze
→ frozen test
→ matched random
→ bootstrap CI
→ cross-benchmark
→ Native hold
```

SystemBench 后续只把 RouterBench 作为：

```text
frozen routing reference
```

而不是持续变化的下游依赖。

---

## 1.3 SystemBench v0.1 决策

**SystemBench v0.1 保持冻结，不重写。**

v0.1 的正确定位是：

> **L1 Frozen Control-plane Composition Benchmark**

它已经可以可信地回答：

```text
生产 router
+
workflow graph
+
parallel scheduler
+
frozen per-node outcome
```

能否稳定组合，以及 routing 是否可以在 workflow 级产生成本收益。

但 v0.1 不应被包装成完整的 end-to-end planning / stateful agent benchmark。

---

# 二、当前证据到底成立到哪一层

## 2.1 当前已经成立的证据

SystemBench v0.1 已具备：

```text
[x] Exact Binding
[x] Production Router Parity
[x] Deterministic Replay
[x] Workflow-group Split
[x] Matched Random
[x] Scheduling Headline
[x] Recovery Mechanism
[x] O2 Schedule Oracle
[x] O3 Joint Oracle 基础
[x] Paired Bootstrap
[x] Manifest Freeze
```

S9a 已完成 284 个 production-routable nodes 的 decision-path parity，benchmark replay 与生产 route seam 零失配；S10 又补齐了 scheduling headline、matched random、recovery ablation、cost validity、coverage audit 与 oracle。fileciteturn22file0L21-L30 fileciteturn22file0L112-L116

---

## 2.2 当前不能声称成立的证据

现在尚不能把以下结论写成 SystemBench 的最终 headline：

```text
“LLooM 能正确拆解复杂任务”
“LLooM 的 workflow 真实提升了最终答案质量”
“LLooM 的 recovery 显著提高 task success”
“Weak model 的低成本选择在真实 downstream artifact 上安全”
“SystemBench 已完成真正的 end-to-end agent validation”
```

原因是当前 L1：

```text
node outcome
```

是 frozen 的，而真实 decomposition 的 node 往往具有：

```text
artifact dependency
state dependency
```

真正的下游输入可能依赖上游输出。

因此当前必须严格区分：

```text
L1 · Outcome Replay
L2 · Artifact Replay
L3 · Live Execution
```

---

# 三、SystemBench 正式版本与 Fidelity 分层

## 3.1 L0 · Schedule Replay

输入：

```text
DAG
edges
waves
node duration
```

不关心模型质量。

用途：

```text
scheduler correctness
critical path
parallelism
```

---

## 3.2 L1 · Outcome Replay

输入：

```text
workflow
+
node-level success / quality / cost / latency
```

节点结果来自 frozen outcome matrix。

允许：

```text
routing counterfactual
cost counterfactual
schedule counterfactual
```

前提：

```text
dependency_semantics = independent
```

或节点不需要未冻结的上游 artifact。

---

## 3.3 L2 · Artifact / Historical Replay

输入：

```text
真实历史 workflow
+
真实 node output
+
真实 intermediate artifact
+
真实 trajectory / state snapshot
```

允许研究：

```text
A output
→ B input
→ B output
→ C input
```

以及：

```text
original assignment
vs
counterfactual assignment
```

但只有真正存在 alternative outcome / replay material 时，才可以称为 counterfactual。

---

## 3.4 L3 · Live Execution

真实：

```text
model
+
tool
+
state
+
latency
+
cost
+
failure
```

用于最终外部验证。

---

# 四、S12 的真正目标：从 Structural Workflow 升级为 Semantic Workflow

## 4.1 不再以“扩充 case 数量”为第一目标

当前 15 个 real decomposition seed 已证明：

```text
真实 decomposition
→
canonical workflow
```

技术链路成立。

S11 更重要的否定性结果是：PlanningBench checklist 不是 decomposition gold reference；lexical similarity max 仅 0.049，不能通过调 `tau=0.45` 修复。fileciteturn22file0L77-L90

所以 S12 的重点不是继续调 planning evaluator，而是建立自己的：

```text
semantic annotation
+
artifact contract
+
completion semantics
```

---

# 五、D1 最终决策：采用 D1-c' 分层混合，而不是简单 15+15

## 5.1 目标构成

推荐最终 S12 Semantic Set：

```text
20 个 Semantic Workflows
+
10 个 Exact-Bound Control Workflows
```

总计：

```text
30 System Cases
```

但两者身份完全不同。

---

## 5.2 Semantic Workflows

来源：

```text
真实 LLM decomposition
```

用途：

```text
semantic structure
semantic dependency
artifact contract
workflow morphology
L1/L2/L3 fidelity classification
```

Semantic Set 是未来 v0.2 headline 的主要候选来源。

---

## 5.3 Exact-Bound Control Workflows

继续使用：

```text
LLMRouterBench exact query
```

再赋予 workflow structure。

但必须标记：

```text
role = control / diagnostic
```

不能与真实 semantic workflow 混合平均。

用途：

```text
router parity
binding regression
counterfactual machinery
scheduler regression
```

---

## 5.4 为什么不能继续使用“15 real + 15 assembled”直接作为 headline

因为 assembled cases 如果数量占到 50%，最终结果很难解释为：

> semantic workflow 上的 system-level routing result。

新设计明确：

```text
Primary = Semantic Set
Secondary = Control Set
```

这是 v0.2 的身份边界。

---

# 六、D2 最终决策：LLM 只做预标注，100% Headline Node 人工签收

## 6.1 标注流程

```text
LLM pre-label
      ↓
Human review
      ↓
Disagreement arbitration
      ↓
Freeze annotation
```

LLM 可以建议：

```text
semantic_role
atomicity
artifact type
completion candidate
```

但最终 gold 不由 LLM 单独决定。

---

## 6.2 必须记录 annotation provenance

每个 annotation 至少记录：

```yaml
annotation_version: v1
annotator: human
prelabel_model: qwen-plus
review_status: approved
reviewed_at: ...
```

如果人工修改 LLM 建议：

```yaml
source: llm_prelabel
final: human_corrected
```

这样以后可以量化：

```text
prelabel agreement
human correction rate
spec disagreement rate
```

---

# 七、S12 Annotation Spec v1 正式字段

## 7.1 semantic_role

当前十词表继续冻结为：

```text
calculation
verification
extraction
classification
retrieval
transformation
synthesis
reasoning
execution
search
```

扩展词表必须增加：

```text
annotation_version
```

不得静默修改。

---

## 7.2 atomicity

每个 node 必须回答：

> 这个 node 是否只有一个可以单独验证的职责？

发现：

```text
且
并
然后
再
同时
```

等多个独立动作时，应优先拆分。

headline 禁止 composite node。

---

## 7.3 dependency_semantics

正式采用：

```text
independent
artifact_dependent
state_dependent
```

对应：

```text
independent
→ L1

artifact_dependent
→ L2

state_dependent
→ L3
```

这是 SystemBench 的关键 fidelity contract。

---

## 7.4 artifact_contract

从：

```json
{"produces": ["facts"]}
```

升级为：

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

必须满足：

```text
consumes.from
⊆ upstream nodes
```

并且：

```text
consumes.type
```

必须能由 upstream `produces.type` 提供。

---

## 7.5 completion_condition

从：

```text
all nodes succeed
```

升级为：

```yaml
required_nodes: [...] 
optional_nodes: [...]
terminal_condition: ...
aggregation_condition: ...
```

真实 workflow 不一定要求所有 node 都成功。

---

# 八、S12.4 必须提前成为花钱前 Gate：Routing-Relevance Gate

当前 Shadow 已第三次观察到：

```text
hard ≈ 主导
medium 很少
easy ≈ 0
```

完整 llmrb prompt 和 SystemBench 节点都会被大量送入 hard band，导致可操作的 routing 空间被压缩。fileciteturn22file0L112-L116

因此在任何 Semantic Outcome Matrix 采集前，必须先跑：

```text
Band Coverage
```

---

## 8.1 至少报告

```text
hard_rate
medium_rate
easy_rate
eligible_weak_rate
eligible_mid_rate
multi_model_eligible_rate
winner_entropy
```

其中最关键：

```text
multi_model_eligible_rate
```

---

## 8.2 Operational Gate

建议：

```text
< 20%
→ diagnostic only

20%–50%
→ routing candidate

> 50%
→ routing headline candidate
```

这些是工程性判断阈值，不是统计定理；最终以 probe 数据和 case composition 决定。

---

# 九、D3 最终决策：阶梯式 Semantic Outcome Matrix

## 9.1 D3-0 Provider Probe

先对候选模型做极小 probe：

```text
model × 1-2 representative nodes
```

只验证：

```text
provider reachable
model identity correct
response schema valid
cost observable
latency observable
```

任何模型出现：

```text
unavailable
unstable
unpriced
schema failure
```

不得直接进入主 matrix。

---

## 9.2 D3-1 5-workflow Probe

然后：

```text
5 semantic workflows
×
L1-capable nodes
×
3 candidate models
```

得到第一批：

```text
Semantic Pool Matrix
```

---

## 9.3 D3-2 System Pool Audit

不能默认：

```text
RouterBench R7
=
SystemBench Best Pool
```

必须重新检查：

```text
quality headroom
cost ratio
cheap-safe
oracle-equivalent share
winner entropy
cross-workflow stability
multi-model eligibility
```

得到：

```text
SystemBench Candidate Pool
```

---

## 9.4 D3-3 Full Harvest

只有当 Probe 证明：

```text
模型可用
+
pool 有实际互补性
+
至少有足够 routing-relevant node
```

才扩大到：

```text
20 semantic workflows
×
全部 headline-capable nodes
×
3 models
```

---

# 十、Semantic Outcome Matrix 必须与 RouterBench Frozen Outcome 分开

未来数据库中明确区分：

```text
RouterBench Outcome Matrix

SystemBench Semantic Outcome Matrix
```

不能混成：

```text
unified outcomes
```

原因：

```text
prompt distribution
model pool
cost basis
evaluator semantics
```

都可能不同。

SystemBench 的 semantic matrix 应有自己的：

```text
matrix_hash
pool_hash
scorer_hash
cost_basis_hash
collection_manifest
```

---

# 十一、SystemCase 的 canonical identity

建议正式结构：

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
      "atomic": true,
      "dependency_semantics": "independent",
      "artifact_contract": {
        "produces": [],
        "consumes": []
      },
      "outcome_binding": {
        "mode": "semantic_matrix_exact",
        "source_id": "...",
        "prompt_hash": "..."
      }
    }
  ],
  "completion_condition": {
    "required_nodes": ["n1"],
    "optional_nodes": []
  },
  "fidelity": "L1"
}
```

重要规则：

```text
headline node
→ outcome_binding 必须 exact
```

禁止：

```text
nearest query
embedding fuzzy match
LLM guessed outcome
```

---

# 十二、S12 的 O3 Joint Oracle 应升级成真正精确版本

当前最大 workflow 仅 8 node。

三模型 assignment 最多：

```text
3^8 = 6561
```

因此无需继续对 >6 node 使用 greedy approximation。

统一改成：

```text
≤8 node
→ exhaustive assignment
→ legal schedule optimization
```

每个 assignment：

```text
model assignment
→ node duration / cost
→ legal schedule
→ objective
```

得到真正的：

```text
O3 Joint Oracle
```

同时保存：

```text
oracle_objective
oracle_assignment
oracle_schedule
oracle_search_space
```

这样以后 headline 不再需要“approximate oracle”解释。

---

# 十三、Recovery：从状态机验证升级成真正的收益实验

当前只有 10 failure cases：

```text
completion 0% → 0%
recovered workflow = 1
```

只能证明机制运行，不足以证明 recovery value。fileciteturn22file0L46-L48

下一阶段建议增加到：

```text
30–40 failure cases
```

并分成：

```text
recoverable
partially_recoverable
non_recoverable
```

实验：

```text
Recovery Off
vs
Recovery On
```

报告：

```text
goal success
recovery rate
partial success
extra cost
extra latency
abort rate
```

只有出现稳定 effect 才可以称：

```text
Recovery Gain
```

否则保持：

```text
Recovery Mechanism Validated
```

---

# 十四、S12 的主要研究问题

S12 不只是造数据，应围绕以下问题组织：

### RQ1 · Routing feasibility

> 在真实 semantic subtasks 上，是否存在多个可选模型？

### RQ2 · Pool complementarity

> RouterBench 上的最佳 pool 是否仍然适用于 semantic workflow？

### RQ3 · Routing gain

> semantic workflow 内，LLooM 是否可以在保持 workflow completion / quality 的情况下减少成本？

### RQ4 · Workflow morphology

> 真实 decomposition 的 depth / width / dependency density 如何？

### RQ5 · Fidelity boundary

> 哪些 workflow 可以进行 L1 replay，哪些必须进入 L2 / L3？

---

# 十五、S14 的正式因果实验矩阵

最终 v0.2 主结果固定为：

| Policy | Workflow | Routing | Scheduling | Recovery | 目的 |
|---|---|---|---|---|---|
| B0 Flat-Strong | Flat | Strong | — | Off | 单强模型基线 |
| B1 Flat-BestSingle | Flat | Best Single | — | Off | 简单 route 基线 |
| B2 Fixed-Strong-Seq | Fixed | Strong | Sequential | Off | 固定 workflow 串行 |
| B3 Fixed-Strong-Par | Fixed | Strong | Parallel | Off | 并行收益 |
| B4 Fixed-LLooMRouter-Par | Fixed | LLooM | Parallel | Off | routing 收益 |
| B5 LLooMWorkflow-Strong | LLooM | Strong | LLooM | Off | orchestration 收益 |
| B6 Full-LLooM | LLooM | LLooM | LLooM | Off | routing × orchestration |
| B7 Full-LLooM-Recovery | LLooM | LLooM | LLooM | On | recovery 增益 |
| O1 Node Oracle | Fixed | Node Oracle | Ref | Off | 局部上界 |
| O2 Schedule Oracle | Fixed | Fixed | Oracle | Off | 调度上界 |
| O3 Joint Oracle | Fixed | Joint | Oracle | Optional | 联合上界 |

---

# 十六、必须正式引入 Interaction Gain

定义：

```text
Routing Gain
= B3 - B4
```

```text
Orchestration Gain
= B3 - B5
```

```text
Full Gain
= B3 - B6
```

然后：

```text
Interaction Gain
=
Full Gain
- Routing Gain
- Orchestration Gain
```

它是 SystemBench 真正区别于 RouterBench / Workflow Track 的核心指标之一。

它回答：

> Routing 与 orchestration 放在同一个 inference-control loop 后，是否产生了超出简单相加的收益？

---

# 十七、指标体系重新分层

## Primary

```text
Goal Success
Final Goal Quality
Cost / Successful Workflow
Makespan / Successful Workflow
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
Dependency Precision / Recall
Parallelization Utilization
Sequentialization Waste
Coordination Cost Ratio
Failure Attribution
```

当前 `Q_node` 应继续保留，但不能直接写成“workflow final quality”。

---

# 十八、Coordination Tax 正式改口径

当前：

```text
Coordination Tax = 18.35
```

容易造成误读。

正式报告改成同时给：

```text
Coordination Cost Ratio = 19.35×
Coordination Overhead = +1835%
```

并保留绝对：

```text
workflow cost
local-min cost
```

另外：

```text
cost = 0
```

统一解释为：

```text
unknown
```

除非 source 明确声明 free。

不得默认：

```text
0 cost = free model
```

S10 已完成该规则修复。fileciteturn22file0L50-L54

---

# 十九、SystemBench 的统计方法正式冻结

## 19.1 Bootstrap Unit

必须：

```text
system_case
```

不能：

```text
node
```

---

## 19.2 Bootstrap 类型

优先：

```text
paired bootstrap
```

同一 system case 的多个 policy 必须共同重采样。

---

## 19.3 Headline CI

至少：

```text
ΔGoalSuccess
ΔFinalQuality
ΔCost / Success
ΔLatency
ΔStrongShare
ΔRecovery
```

---

# 二十、SystemBench Split 必须按 workflow group

正确单位：

```text
SystemCase / Workflow Group
```

不能：

```text
Node
```

禁止：

```text
同一 workflow template
→ calibration
又→ test
```

需要继承：

```text
sha256("lloom-sbsplit:3407:{system_case_id}")
```

任何 split rule 改动：

```text
new benchmark identity
```

---

# 二十一、S13 L2 Historical Replay 的真正入口

S13 不应该一开场就“跑 τ²”。

正确顺序：

```text
S13.0 Source Audit
↓
S13.1 Trajectory Schema
↓
S13.2 Artifact Audit
↓
S13.3 Counterfactual Validity
↓
S13.4 Small Replay
```

---

## S13.0 Source Audit

对每个外部 trajectory source 检查：

```text
license
source commit
trajectory availability
tool calls
state snapshots
artifacts
model identity
evaluator version
cost availability
```

推荐优先级：

```text
LLooM own historical trace
↓
τ² historical
↓
ToolSandbox historical / recorded trajectory
↓
AppWorld logs/state
↓
WebArena / others
```

---

## S13.1 Trajectory Schema

统一：

```text
trajectory_id
step_id
parent_step
model
prompt_hash
input_state_hash
output_hash
tool_event
artifact_hash
state_after_hash
duration
cost
```

---

## S13.2 Artifact Audit

每条历史 trajectory 必须能够回答：

```text
node A 输出了什么？
node B 消费了什么？
A → B 的依赖是否真实？
B 的输入能否由历史 artifact 重建？
```

---

## S13.3 Counterfactual Validity

只有当：

```text
alternative outcome exists
```

或者：

```text
alternative outcome can be deterministically replayed
```

才允许：

```text
counterfactual routing
```

必须检查：

```text
model change
→ artifact change
→ downstream input change
→ state change
```

防止：

```text
换 A
但 B 仍偷偷读取原始 A artifact
```

这种 counterfactual leakage。

---

# 二十二、L2 最核心的新指标：Counterfactual Validity

建议记录：

```text
cf_valid
artifact_propagated
state_propagated
original_artifact_hash
counterfactual_artifact_hash
downstream_input_hash
```

最终：

```text
valid_counterfactual_rate
```

低于预设阈值的 source：

```text
只能作为 trajectory diagnostic
```

不能进入 routing headline。

---

# 二十三、从 L1 到 L2 的核心科学问题

L1 问：

> “如果每个 node 独立有 frozen outcome，routing 是否更省？”

L2 问：

> “当上游模型改变时，它产生的 artifact 是否改变下游结果，从而改变最佳全局模型分派？”

这个差别非常重要。

L2 才开始真正测试：

```text
Local Oracle
≠
Workflow Oracle
```

---

# 二十四、Local Oracle vs Workflow Oracle

## O1 · Node Oracle

逐节点选择最佳模型：

```text
argmax quality
```

---

## O2 · Schedule Oracle

固定 workflow：

```text
min legal makespan
```

---

## O3 · Joint Oracle

固定 workflow：

```text
joint(model assignment, schedule)
```

---

## O4 · Workflow Oracle（后置）

未来再考虑：

```text
workflow topology
+
model assignment
+
schedule
```

第一版坚决不做 O4。

否则问题会变成：

```text
joint decomposition
× routing
× scheduling
```

搜索空间过大，也很难解释。

---

# 二十五、Complexity Detector 的位置要重新认识

当前 LLooM：

```text
complexity detection
→ band
```

已经被多轮实验发现是很强的 policy gate。

因此未来 SystemBench 不应该把 complexity detector 仅仅当作一个内部 heuristic。

应把它单独报告：

```text
Band Accuracy / Calibration
Band Coverage
Routing Opportunity Coverage
```

真正想研究的是：

```text
complexity
→ should decompose?
```

而不是：

```text
long prompt
→ hard
```

---

# 二十六、SystemBench 应新增 Decomposition ROI

定义：

```text
Decomposition ROI
=
ΔFinal Success / ΔSystem Cost
```

更实际地，报告：

```text
Single Strong
vs
Orchestrated Strong
```

在：

```text
quality
success
cost
latency
```

上的变化。

核心问题：

> **什么时候拆任务真的值得？**

而不是：

> 拆任务后平均节点数是多少？

---

# 二十七、SystemBench 应新增 Coordination / Orchestration Cost

除了模型成本，还要区分：

```text
model cost
tool/runtime cost
orchestration overhead
recovery cost
```

最终至少报告：

```text
raw workflow cost
cost excluding orchestration
coordination overhead
cost / successful workflow
```

这样可以避免：

```text
系统成功率高
但协调成本巨大
```

被误判为优秀系统。

---

# 二十八、S12 / S13 / S14 版本边界

## SystemBench v0.1

```text
Frozen Control Composition
```

输入：

```text
synthetic structural workflow
+
frozen exact outcomes
```

---

## SystemBench v0.2

```text
Semantic Workflow Composition
```

输入：

```text
real decomposition
+
semantic annotation
+
semantic outcome matrix
```

---

## SystemBench v0.3

```text
Artifact / Historical Replay
```

输入：

```text
real trajectory
+
real artifact
+
state
```

---

## SystemBench v0.4

```text
Live External
```

输入：

```text
real model
+
real tool
+
real environment
```

版本升级必须满足：

```text
new fidelity
```

而不是：

```text
更多 cases
```

---

# 二十九、S12 实际执行顺序

```text
S12.2
Semantic Case Expansion
        ↓
S12.3
LLM Pre-label
        ↓
Human Sign-off
        ↓
S12.4
Band Coverage Gate
        ↓
S12.5
Provider Probe
        ↓
S12.6
5-workflow Semantic Outcome Probe
        ↓
S12.7
System Pool Audit
        ↓
S12.8
Full Semantic Outcome Matrix
        ↓
S12.9
Exact O3 Joint Oracle
        ↓
S12.10
v0.2 Candidate Freeze
```

---

# 三十、S12 每一步的停线条件

## Stop S12-1 · Semantic quality

如果：

```text
>20% workflow
无法获得明确 semantic node
```

暂停扩充，先修 annotation spec。

---

## Stop S12-2 · Routing feasibility

如果：

```text
multi_model_eligible_rate < 20%
```

则该 case 进入 diagnostic，不投入完整 outcome matrix。

---

## Stop S12-3 · Pool usefulness

如果 probe 发现：

```text
oracle_gain ≈ 0
cheap-safe ≈ 0
```

则停止 full harvest，重新进行 System Pool Audit。

---

## Stop S12-4 · Outcome quality

如果模型：

```text
不稳定
无价格
schema failure 高
```

立即移出 candidate pool。

---

# 三十一、S12 v0.2 Freeze Gate

只有满足：

```text
[x] ≥20 semantic workflows
[x] 100% headline annotation human-approved
[x] dependency semantics complete
[x] artifact contracts complete
[x] completion conditions complete
[x] routing relevance gate passed
[x] semantic outcome matrix exact-bound
[x] System Pool frozen
[x] O3 exact
[x] workflow-group split frozen
[x] cost basis frozen
[x] evaluator frozen
```

才允许：

```text
benchmark_id
→
lloom-systembench-v0.2
```

否则维持：

```text
candidate
```

---

# 三十二、S14 主结果的实验原则

## 32.1 一个 test 只能看一次

正式 test：

```text
冻结后
→
一次性运行
```

禁止：

```text
看 test
→ 改 policy
→ 再看 test
```

---

## 32.2 不允许按 benchmark 分别调 threshold

所有：

```text
pool selection
threshold
routing policy
```

必须只由：

```text
calibration
```

决定。

---

## 32.3 SystemCase 是统计独立单位

workflow 内：

```text
node
edge
wave
```

都不是 bootstrap independent sample。

---

# 三十三、SystemBench 最终 headline 应是一组 frontier，而不是一个分数

至少：

```text
Goal Success × Cost
Goal Success × Latency
Goal Success × Strong-call Budget
```

同时展示：

```text
Single Strong
Best Single
Full LLooM
Oracle
```

---

# 三十四、Failure Attribution 最终升级为传播图

不要只报告：

```text
routing error = 15%
```

未来应报告：

```text
Root Cause
↓
Propagation
↓
Terminal Error
```

例如：

```text
Wrong Routing
      ↓
Bad Artifact
      ↓
Downstream Failure
      ↓
Recovery Failure
      ↓
Goal Failure
```

因此 SystemCase 最终保留：

```json
{
  "primary_error": "P6",
  "secondary_errors": ["P7", "P9", "P11"],
  "propagation_edges": [
    ["P6", "P7"],
    ["P7", "P9"],
    ["P9", "P11"]
  ]
}
```

---

# 三十五、S15 Live 的进入条件

必须同时满足：

```text
RouterBench Frozen
SystemBench v0.2 Frozen
L2 feasibility proven
Counterfactual leakage = 0
Recovery evidence available
```

然后：

```text
10–30 live sanity
↓
50–100 held-out
↓
external benchmark
```

不要直接 full benchmark。

---

# 三十六、Live 外部 benchmark 的推荐顺序

建议：

```text
第一优先
τ²

第二优先
ToolSandbox

第三优先
AppWorld

第四优先
WebArena
```

理由不是 benchmark 名气，而是 Fidelity：

```text
historical trajectory
→ stateful interaction
→ application state
→ browser / long-horizon
```

一步一步增加环境复杂度。

---

# 三十七、SystemBench 工程结构

```text
benchmarks/
├── routerbench/
│   └── frozen v0.2
│
├── orchestration/
│   └── workflow track
│
└── systembench/
    ├── manifests/
    │   ├── system.yaml
    │   ├── source_registry.yaml
    │   ├── split.yaml
    │   ├── model_pool.yaml
    │   └── frozen_manifest_v02.json
    │
    ├── cases/
    │   ├── system_cases.jsonl
    │   ├── bindings.jsonl
    │   ├── annotations.jsonl
    │   └── trajectory_bindings.jsonl
    │
    ├── matrices/
    │   ├── routerbench_ref.jsonl
    │   └── semantic_outcomes.jsonl
    │
    ├── replay/
    │   ├── l0_schedule.py
    │   ├── l1_outcome.py
    │   └── l2_artifact.py
    │
    ├── runner/
    │   ├── composition.py
    │   ├── shadow.py
    │   ├── historical.py
    │   └── live.py
    │
    ├── evaluator/
    │   ├── goal.py
    │   ├── routing.py
    │   ├── scheduling.py
    │   ├── recovery.py
    │   ├── attribution.py
    │   └── system.py
    │
    └── reports/
```

核心原则：

```text
下层 benchmark 不复制
SystemBench 只 composition
```

---

# 三十八、Manifest Identity

SystemBench v0.2 至少冻结：

```text
routerbench_ref_hash
workflow_schema_hash
semantic_case_hash
model_pool_hash
routing_policy_hash
scheduler_policy_hash
failure_policy_hash
annotation_hash
outcome_matrix_hash
evaluator_hash
split_hash
cost_basis_hash
```

任何改变：

```text
new benchmark identity
```

尤其是：

```text
tau
semantic_role vocabulary
annotation rule
cost basis
policy
```

不能只改文件不改 identity。

---

# 三十九、推荐的数据库结构

## `bench_system_cases`

```text
system_case_id
workflow_id
root_goal
fidelity
split
annotation_hash
case_hash
```

## `bench_system_nodes`

```text
system_case_id
node_id
prompt_hash
semantic_role
atomic
 dependency_semantics
artifact_contract_json
completion_role
fidelity
```

## `bench_outcome_bindings`

```text
system_case_id
node_id
binding_mode
source_benchmark
source_id
prompt_hash
matrix_hash
```

## `bench_system_runs`

```text
run_id
system_case_id
policy_id
replay_level
seed
manifest_hash
```

## `bench_system_items`

```text
run_id
node_id
selected_model
quality
cost
latency
status
primary_error
```

## `bench_propagation_edges`

```text
run_id
from_error
to_error
artifact_hash_before
artifact_hash_after
```

---

# 四十、实验记录与成本控制

## Zero-cost

以下保持零成本：

```text
annotation schema
builder
validator
band coverage
pool search over existing matrices
scheduler
L1 replay
oracle
bootstrap
reporting
```

## Low-cost

只允许小额度：

```text
S11/S12 decomposition
provider probe
semantic outcome probe
historical source audit
```

## High-cost

必须后置：

```text
large semantic matrix
τ² live
AppWorld
WebArena
large model pool
```

每次跨成本层级必须重新记录：

```text
purpose
budget
expected evidence
stop condition
```

---

# 四十一、当前最推荐的执行排期

## S12-A · Semantic Foundation

```text
1. 30 cases 构建
2. annotation pre-label
3. 100% human sign-off
4. artifact contract
5. fidelity classification
6. completion condition
```

目标：

```text
30 cases
→
20+ qualified semantic cases
```

---

## S12-B · Routing Feasibility

```text
7. Band Coverage
8. Provider Probe
9. 5-workflow outcome probe
10. System Pool Audit
```

目标：

```text
证明 semantic node 上真的存在 model trade-off
```

---

## S12-C · Frozen Matrix

```text
11. Full semantic outcome harvest
12. exact binding
13. exact O3
14. v0.2 freeze
```

---

## S13 · L2

```text
15. trajectory source audit
16. schema
17. artifact audit
18. counterfactual validity
19. 10–30 replay
```

---

## S14 · Main Result

```text
20. B0-B7
21. O1-O3
22. Routing Gain
23. Orchestration Gain
24. Interaction Gain
25. Recovery Gain
26. frontier
27. paired bootstrap
28. failure propagation
```

---

## S15 · Live

```text
29. 10–30 sanity
30. 50–100 held-out
31. external benchmark
32. production validation
```

---

# 四十二、当前不做什么

```text
1. 不回头优化 RouterBench v0.2
2. 不把 PlanningBench checklist 当 decomposition gold
3. 不调 tau 去拟合错误 reference
4. 不做 semantic node → old query 的 fuzzy matching
5. 不默认 R7 是 SystemBench 最佳 pool
6. 不把 L1 Q_node 当 final quality
7. 不把 recovery mechanism test 当 recovery gain
8. 不用 cost=0 当 free
9. 不做 composite SystemScore
10. 不直接冲 full live
11. 不同时优化 workflow topology + routing + schedule
12. 不为了 headline 扩大量低 routing relevance case
```

---

# 四十三、研究叙事的最终收束

## Layer 1 · RouterBench

> **不是所有 query 都需要最强模型。**

证据：

```text
cost saving
+
quality retention
+
matched random
+
bootstrap CI
```

---

## Layer 2 · Workflow Track

> **不是所有 complex task 都应该由一个模型一次完成。**

证据：

```text
planning
scheduling
parallelism
recovery mechanism
```

---

## Layer 3 · SystemBench

> **复杂目标可以被组织成 workflow，并在节点级继续做模型决策，从而形成新的质量—成本—延迟 frontier。**

这层最终需要：

```text
semantic workflow
+
routing
+
scheduling
+
artifact/state
+
recovery
+
final goal
```

---

# 四十四、真正属于 SystemBench 的核心科学问题

SystemBench 最终不应该只是回答：

> LLooM 分数高不高？

而应该回答四个更强的问题：

### Q1

> **什么时候值得拆？**

### Q2

> **拆开以后，什么节点应该交给什么模型？**

### Q3

> **局部最优模型选择什么时候会偏离 workflow-level 最优？**

### Q4

> **routing + orchestration + scheduling + recovery 是否存在真正的协同收益？**

其中 Q4 是 SystemBench 相对于 RouterBench / Workflow Track 最核心的新问题。

---

# 四十五、最终路线图

```text
RouterBench v0.2
      │
      │ frozen reference
      ▼
SystemBench v0.1
L1 Frozen Composition
      │
      ├── S12 Semanticization
      │      ├── Annotation
      │      ├── Band Gate
      │      ├── Pool Probe
      │      └── Semantic Outcome Matrix
      │
      ▼
SystemBench v0.2
Semantic Workflow
      │
      ├── S13 Historical Audit
      ├── Artifact Replay
      └── Counterfactual Validity
      │
      ▼
SystemBench v0.3
L2 Artifact / State Replay
      │
      ├── S14 Main Result
      ├── Interaction Gain
      └── Failure Propagation
      │
      ▼
SystemBench v0.4
L3 Live
      │
      ├── 10–30 sanity
      ├── 50–100 held-out
      └── external benchmark
```

---

# 四十六、最终执行原则

## 原则 1 · 先提高 Fidelity，再提高规模

```text
semantic > more cases
artifact > more cases
live > more synthetic
```

---

## 原则 2 · 先证明可测，再花钱

任何新模型、新 benchmark、新 trajectory source：

```text
probe
→ audit
→ freeze
→ scale
```

---

## 原则 3 · 下层 benchmark 冻结，上层 benchmark 组合

```text
RouterBench
Workflow Track
      ↓
SystemBench
```

不要让 SystemBench 偷改下层定义。

---

## 原则 4 · 不把 structural evidence 包装成 semantic evidence

```text
L1 ≠ L2
L2 ≠ L3
```

这是对外叙事最重要的 honesty boundary。

---

## 原则 5 · 所有 headline 必须可追溯

```text
result
→ run
→ manifest
→ case
→ node
→ binding
→ outcome
→ evaluator
```

---

# 四十七、立即执行清单

### P0 · 现在执行

```text
[ ] S12.2：构建 30 System Cases
[ ] S12.3：LLM pre-label + 100% human sign-off
[ ] S12.4：Band Coverage Gate
[ ] S12.5：Provider Probe
[ ] S12.6：5-workflow Semantic Outcome Probe
[ ] S12.7：System Pool Audit
```

### P1 · Probe 通过后执行

```text
[ ] Full Semantic Outcome Matrix
[ ] Exact O3
[ ] v0.2 Freeze
```

### P2 · v0.2 冻结后

```text
[ ] Historical Source Audit
[ ] L2 Artifact Replay
[ ] Counterfactual Validity
```

### P3 · 主结果

```text
[ ] B0-B7
[ ] O1-O3
[ ] Interaction Gain
[ ] Recovery Gain
[ ] Frontier
[ ] Paired Bootstrap
```

### P4 · 最后

```text
[ ] Live sanity
[ ] External benchmark
[ ] Production validation
```

---

# 四十八、最终决策

当前执行方向正式确定为：

```text
RouterBench
→ freeze / maintenance

SystemBench v0.1
→ freeze / regression baseline

S12
→ Semantic Workflow v0.2

S13
→ L2 Historical / Artifact Replay

S14
→ SystemBench Main Result

S15
→ Live External
```

其中最重要的具体决定：

```text
D1 = D1-c'
20 Semantic + 10 Control

D2 = D2-b'
LLM pre-label + 100% human sign-off

D3 = D3-b'
Provider Probe → 5-case Probe → Pool Audit → Full Harvest
```

**不要因为已经具备大量数据，就急着把 SystemBench 做大；当前最有价值的工作是把真实 semantic workflow、routing opportunity、artifact dependency 和 system-level counterfactual 的边界做干净。**

---

# 四十九、外部研究对齐（用于路线验证，不作为 SystemBench 数据依赖）

近期 routing / agent evaluation 研究越来越明显地采用：

```text
offline/static evaluation
→
trajectory/state-aware evaluation
→
live execution
```

并把模型路由从一次性 query 选择推进到 multi-step trajectory step 的模型分配。这与本方案的：

```text
L1 Frozen
→ L2 Artifact
→ L3 Live
```

路线一致。

外部参考：

- TwinRouterBench / static-dynamic routing：https://arxiv.org/abs/2605.18859
- AgentRouter / multi-step agent routing：https://arxiv.org/abs/2609.22951
- APB / Agent Planning Benchmark：https://arxiv.org/abs/2606.04874
- PlanningBench：https://arxiv.org/abs/2605.20873
- τ²-bench：https://github.com/sierra-research/tau2-bench
- ToolSandbox：https://github.com/apple/ToolSandbox
- AppWorld：https://arxiv.org/abs/2407.18901
- WebArena：https://arxiv.org/abs/2307.13854

---

# 五十、单句执行口令

> **先把 S12 做成真正的 Semantic Benchmark，再把 S13 做成真正的 Artifact Replay，最后才让 S14/S15 回答 LLooM 是否在完整复杂任务上形成可重复、可解释、可验证的 system-level frontier。**
