# 21 · LLooM SystemBench 决策与改进指导 v3

> 日期：2026-10-07  
> 定位：基于 RouterBench 收官、Workflow/Orchestration 离线层完成、SystemBench Phase A/B + Shadow parity 完成后的**下一阶段正式执行入口**。  
> 上游：`17-RouterBench实施总结-收官Day9-15.md`、`18-OrchestrationBench实施总结-O-Day9-14.md`、`19-LLooM-SystemBench-合并与改进实施指导-v2.md`、`20-SystemBench实施总结-合并验收与PhaseAB.md`。  
> 核心结论：**SystemBench v0.1 Frozen Composition 已经成立；下一步不是继续扩大 RouterBench，也不是直接进入大型 Live Agent Benchmark，而是先做 L1 方法学收口 → 极小真实 decomposition calibration → Semantic Workflow v0.2 → Historical Artifact/State Replay → Live External。**

---

# 一、执行摘要

## 1.1 当前项目已经走到哪里

截至 20 号实施总结，SystemBench 已完成：

```text
RouterBench v0.2
        ↓
Workflow / Orchestration Offline Layer
        ↓
SystemCase Builder
        ↓
L1 Frozen Outcome Replay
        ↓
Composition Runner
        ↓
Failure / Recovery propagation
        ↓
Workflow-level Bootstrap
        ↓
SystemBench manifest freeze
        ↓
Production Shadow parity
```

当前事实：

- RouterBench v0.2 已完成 calibration tournament、test freeze、matched random、bootstrap CI、cross-benchmark，并转入 maintenance；Native 当前保持 hold。  
- Workflow/Orchestration Offline 层已经完成 canonical schema、PlanningBench 467/467、PlanBench 1901、deterministic planning evaluator、scheduler、router bridge、F1-F7 failure injection 与 P1-P11 归因。  
- SystemBench 已完成 80 cases / 327 nodes 的 L1 composition set，其中 65 test / 15 calibration，并完成 frozen manifest。  
- SystemBench Shadow 的 284 个可路由节点实现了 production decision path 与 benchmark replay 的零失配。  
- 当前 S-0~S-4、S-7、S-8 已绿；S-5 planning baseline、S-6 完整传播归因仍 partial，原因都不是工程失败，而是真实 decomposition / artifact dependency 尚未进入。  

因此，**当前系统已经不再处于“有没有 SystemBench”的阶段，而是处于“第一版 SystemBench 测到什么、还缺什么现实性”的阶段。**

---

# 二、最终决策

## 2.1 决策一：RouterBench 正式停在 maintenance，不再继续主线优化

RouterBench v0.2 已有完整冻结与统计闭环：

```text
Calibration
→ Pool Tournament
→ Freeze
→ Test
→ Matched Random
→ Budget
→ Bootstrap CI
→ Cross-Benchmark
→ Native decision
```

其最终研究结果应作为 SystemBench 的 frozen routing source，而不是继续成为 SystemBench 的开发分支。

### 不再作为 P0 的项目

```text
继续增加 RouterBench dataset
重新搜索大量模型池
继续调 λ / penalty
训练大型 query router
扩大 RouterBench Native
```

除非出现明确的 SystemBench blocking issue，否则以上全部进入 maintenance。

### 唯一保留的 RouterBench 改动

只有以下情况允许修改：

```text
1. SystemBench 发现 frozen manifest / adapter 有 bug
2. 发现 source outcome 语义存在实质性错误
3. production policy 需要独立的新 benchmark variant
```

若修改会影响 frozen identity，则必须新建 benchmark_id，绝不回写 v0.2。

---

## 2.2 决策二：SystemBench v0.1 不推翻，直接冻结为 L1 Frozen Composition

当前 SystemBench v0.1 应正式定义为：

> **LLooM SystemBench v0.1 / L1 Frozen Composition：在固定 workflow 结构、固定 node outcome、固定调度规则与确定性 replay 条件下，评估 model routing、scheduling、failure propagation 与 workflow-level cost/quality trade-off 的组合控制能力。**

它不是：

```text
完整 agent benchmark
```

也不是：

```text
真实 end-to-end task success benchmark
```

这一区分必须写入 README、论文/立项材料和结果页。

---

## 2.3 决策三：下一阶段先补 zero-cost methodology hardening

在进入真实 LLM decomposition 前，先完成 5 项零成本工作：

```text
S10.1  Scheduling headline
S10.2  SystemBench matched random
S10.3  Recovery ablation
S10.4  Cost validity / coverage audit
S10.5  Baseline + interaction matrix
```

原因不是为了“把 Gate 补漂亮”，而是为了确保下一步得到的系统结论不会混入明显的评价口径问题。

---

## 2.4 决策四：S9b 真实 decomposition pilot 应该做，但严格限制规模

建议：

```text
10–20 PlanningBench instances
1 个固定 decomposition model
1 个固定 system prompt/profile
1 个固定 schema
```

S9b 的唯一目的：

```text
真实 planning calibration
真实 workflow shape
真实 dependency / semantic workflow 样本
```

**S9b 不用于刷新 v0.1 headline。**

它是 v0.2 数据建设与 evaluator calibration 的入口。

---

## 2.5 决策五：真实 decomposition 产生的新 node 不得 fuzzy-bind 到 RouterBench

这是硬规则：

```text
真实 decomposition node
        ↓
不能
        ↓
nearest query / embedding similarity / fuzzy lookup
        ↓
伪造 RouterBench outcome
```

原因：当前 L1 的 strongest methodological property 就是 exact binding。19 号方案已经明确禁止模糊 `lookup`，20 号阶段也已实现 327/327 node exact-bound。保持这一点比“让更多真实 workflow 能跑”更重要。

真实 decomposition 后，如果 node 不存在 frozen outcome：

```text
L1
→ no valid frozen binding
→ 不进入 headline
```

该 node 进入：

```text
L2 historical_artifact
```

或者最终：

```text
L3 live_only
```

---

# 三、为什么当前 L1 已经成立，但还不能称为完整 SystemBench

当前 L1 的科学边界必须明确写清。

## 3.1 当前 L1 的强项

### A. Production parity

Shadow 已确认：

```text
production route_single()
        =
router::plan()
        =
SystemBench replay decision path
```

284 个可路由节点零失配。

这证明 benchmark 并非复制了一套 router 公式，而是在评测真实 production seam。

### B. Exact binding

327 节点均有 frozen source binding，且 prompt hash 校验。

### C. Deterministic replay

相同 manifest / input 下：

```text
same input
→ same routing
→ same schedule
→ same outcome
```

### D. Workflow-level statistics

bootstrap unit 已从 RouterBench 的 query/instance 提升为：

```text
system_case
```

这符合 workflow 中节点并非独立样本的统计结构。

### E. Counterfactual 至少在 independent dependency 下成立

目前 builder 使用：

```text
dependency_semantics = independent
```

因此 L1 中可以严谨比较：

```text
same workflow
same node prompt
same outcome matrix
不同 model assignment
```

这形成了第一版系统级 counterfactual。

---

## 3.2 当前 L1 的根本限制

目前 workflow 虽然有 DAG，但大部分 node outcome 之间实际上是独立 frozen lookup：

```text
A score
B score
C score
```

并不意味着：

```text
A(output)
  ↓
B(input)
  ↓
C(input)
```

真实发生。

因此 L1 当前能够证明：

> routing / scheduling control plane 的组合价值。

不能直接证明：

> 一个真实 decomposition 改变后，前置模型输出变化是否传播到 downstream node，从而改变最终答案。

这正是 L2/L3 存在的意义。

---

# 四、SystemBench 正式采用四级 Fidelity Model

这是后续所有实现的总框架。

```text
L0 · Schedule Replay
        ↓
L1 · Outcome Replay
        ↓
L2 · Artifact / State Replay
        ↓
L3 · Live Execution
```

---

## 4.1 L0 · Schedule Replay

输入：

```text
DAG
dependency
duration
wave
```

不使用模型 quality。

主要回答：

```text
parallel legality
critical path
makespan
sequentialization waste
parallel utilization
```

---

## 4.2 L1 · Outcome Replay

当前已完成。

每个 node 拥有：

```text
success
quality
cost
latency
```

但没有真实 downstream artifact。

允许：

```text
fixed workflow
model reassignment
schedule reassignment
failure injection
```

主要回答：

```text
routing gain
scheduling gain
failure/recovery control gain
workflow-level cost/quality trade-off
```

---

## 4.3 L2 · Artifact / State Replay

未来重点。

每个节点不再只是：

```text
quality = 0.8
```

而是：

```text
input state
model
output artifact
next-state / tool state
```

于是可以真正 replay：

```text
A output
   ↓
B input
   ↓
B output
   ↓
C input
```

并做：

```text
Original trajectory
vs
LLooM counterfactual trajectory
```

这一步应该优先使用已有 historical trajectories，而不是自己重新制造一套 offline pseudo-trace。

τ-bench / τ²-bench 的 evaluation 设计尤其值得借鉴：其 reference actions 可用于重放生成目标环境状态，而 agent 并不必须复制同一条动作路径；只要达到等价最终状态即可。这对于 SystemBench 的 “trajectory 不是唯一正确路径” 原则很重要。citeturn984854search0turn984854search11

---

## 4.4 L3 · Live Execution

真正调用：

```text
models
+
tools
+
stateful environment
```

最终测：

```text
final task success
quality
cost
latency
recovery
realized spend
```

L3 不再把 L1 的 frozen node quality 当成 final result。

---

# 五、SystemBench 应从“Workflow”升级为“SystemCase”作为顶层 canonical object

`Workflow` 描述结构；`SystemCase` 描述一次完整可评测实验单元。

推荐：

```json
{
  "system_case_id": "sb_v02_0001",
  "root_goal": "...",

  "workflow_ref": {
    "source": "workflow_track",
    "workflow_id": "...",
    "workflow_hash": "..."
  },

  "nodes": [
    {
      "node_id": "n1",
      "prompt": "...",
      "role": "research",
      "dependency_semantics": "independent",
      "outcome_binding": {
        "mode": "llmrouterbench_exact",
        "source_sample_id": "...",
        "prompt_hash": "..."
      }
    }
  ],

  "evaluation": {
    "evaluator": "...",
    "success_rule": "..."
  },

  "replay_level": "L1"
}
```

---

# 六、Outcome Binding 必须成为 SystemBench 的一级概念

建议正式封闭枚举：

```text
llmrouterbench_exact
routerbench_exact
historical_artifact
synthetic_fixture
live_only
```

禁止：

```text
lookup
similarity
nearest
embedding_match
guess
```

### Binding 级别解释

| Binding | 可做什么 | 是否进 headline |
|---|---|---|
| exact frozen | L1 counterfactual | ✅ |
| historical artifact | L2 replay | ✅，需单独 track |
| synthetic fixture | framework / failure self-check | ❌ |
| live only | L3 execution | ✅，但独立标注 |
| derived / fuzzy | 研究探索 | ❌ |

其中：

> **fuzzy-derived outcome 永远不能进入正式 headline。**

---

# 七、Dependency Semantics 必须从当前 1 个枚举扩展到三层

当前：

```text
dependency_semantics = independent
```

v0.2 起：

```text
independent
artifact_dependent
state_dependent
```

进一步增加：

```yaml
artifact_contract:
  produces:
    - facts
    - code_patch
    - calculation
  consumes:
    - facts
```

这样 DAG 中：

```text
A → B
```

不再只是“数学意义上的 edge”，而是：

```text
A produces X
B consumes X
```

这会成为 L2 replay 的关键接口。

---

# 八、下一阶段 P0：五项 Zero-cost Hardening

## S10.1 Scheduling Headline

当前已经有 B2/B3：

```text
Strong Sequential
vs
Strong Parallel
```

但主表仍然应增加：

```text
median makespan
p90 makespan
critical path
parallel width
sequentialization waste
parallelization utilization
```

建议主表：

| Policy | Completion | Quality | Cost/WF | Median Time | P90 Time | Strong Share |
|---|---:|---:|---:|---:|---:|---:|
| B2 Strong-Seq | | | | | | 100% |
| B3 Strong-Par | | | | | | 100% |
| B4 Routed-Par | | | | | | |
| O2 Schedule Oracle | | | | | | |

TPS-Bench 本身也是将 task completion 与 scheduling efficiency 同时评价，且报告展示了高完成率与大量 sequential calls 之间的明显 trade-off，因此 SystemBench 不应只展示 success / cost。citeturn984854search12

---

## S10.2 SystemBench Matched Random

新增：

```text
B4 LLooM Routed
R1 Cost-matched Random
R2 Strong-share-matched Random
```

目的：

> 证明 SystemBench 中的 routing benefit 来自 content-aware decision，而不是“恰好选择了更多便宜模型”。

这不会替代 RouterBench 的 matched random，只是让 SystemBench 自己也具备最小因果对照。

---

## S10.3 Recovery Ablation

现有 F1-F7 不应只作为状态机单测。

直接做：

```text
R0 Recovery Off
R1 Recovery On
```

指标：

```text
task success
recovery rate
partial success
abort rate
extra cost
extra latency
```

全部 zero-cost，因为可以直接使用现有 deterministic frozen outcomes + failure injection。

---

## S10.4 Cost Validity / Coverage Audit

建立统一成本规则：

```text
cost > 0
OR
source_explicitly_free
```

否则：

```text
cost = unknown
```

不能默认：

```text
0 = free
```

另外正式输出：

```text
system_cases
routable_nodes
bound_nodes
unbound_nodes
excluded_nodes
reason
coverage_rate
```

当前 Shadow 为 284 节点 parity，而 composition set 为 327 节点；未来必须在 report 中解释剩余节点为何没有进入 production route parity，而不能只给“284/284 zero mismatch”这种局部表述。

---

## S10.5 Baseline + Interaction Matrix

最终不要只做：

```text
Full LLooM vs Strong
```

至少建立：

| Baseline | Planning | Routing | Scheduling | Recovery | 目的 |
|---|---|---|---|---|---|
| B0 Flat Strong | flat | strong | — | off | 单强模型 |
| B1 Flat Best Single | flat | fixed-best | — | off | 最简单强基线 |
| B2 Fixed + Strong + Seq | fixed | strong | seq | off | decomposition/scheduling 基线 |
| B3 Fixed + Strong + Par | fixed | strong | parallel | off | parallel gain |
| B4 Fixed + LLooM Router + Par | fixed | LLooM | parallel | off | routing gain |
| B5 LLooM Workflow + Strong | LLooM | strong | LLooM | off | orchestration gain |
| B6 LLooM Workflow + LLooM Router | LLooM | LLooM | LLooM | off | joint gain |
| B7 Full LLooM + Recovery | LLooM | LLooM | LLooM | on | full system |
| O1 Node Oracle | fixed | node oracle | fixed/ref | off | node 上界 |
| O2 Schedule Oracle | fixed | fixed | schedule oracle | off | schedule 上界 |
| O3 Joint Oracle | fixed | joint | joint | off | 联合上界 |

---

# 九、SystemBench 必须新增“Interaction Gain”

这是 SystemBench 和两个下层 benchmark 最不同的地方。

定义：

```text
Routing Gain
= B3 − B4
```

```text
Orchestration Gain
= B3 − B5
```

```text
Full Gain
= B3 − B6
```

再定义：

```text
Interaction Gain
= Full Gain
  − Routing Gain
  − Orchestration Gain
```

其目标不是强行证明 interaction 为正，而是测量：

> **routing 与 orchestration 放在一起后，是否产生了单独使用两者无法解释的额外系统收益。**

这个结果即使为负，也有研究意义：说明两个机制存在资源竞争、错误传播或重复协调成本。

---

# 十、Oracle 体系调整

SystemBench v0.1 不需要搜索 workflow topology。

先固定 workflow，只增加：

```text
O1 Node Oracle
O2 Schedule Oracle
O3 Joint Router-Schedule Oracle
```

## O1 Node Oracle

每个节点单独选最好模型。

回答：

```text
local model regret
```

## O2 Schedule Oracle

固定 DAG，计算最短合法 makespan。

回答：

```text
scheduler regret
```

## O3 Joint Oracle

固定 workflow，同时优化：

```text
model assignment
+
schedule
```

回答：

```text
system control-plane regret
```

### 暂不做

```text
decomposition topology oracle
```

因为这会把：

```text
workflow search
× model assignment
× schedule
```

合成组合优化问题，明显超出当前 v0.x 的必要范围。

---

# 十一、Coordination Tax 重新规范化

当前定义：

```text
Coordination Tax
= actual workflow cost
  /
  per-node independently-min-cost
  − 1
```

这个定义可以保留，但正式报告不要只写：

```text
Tax = 18.35
```

建议同时输出：

```text
coordination_cost_ratio = 19.35×
coordination_overhead_pct = 1835%
```

另外在 source cost 缺失、0 成本或极端价格比的情况下：

```text
Tax = NA
```

而不是强行给数字。

当前 18.35 很大，并不意味着 orchestrator 一定“很差”，因为分母实际上是极端激进的逐节点最低成本下界；它应该作为：

> **质量优先 routing 相对于理论最低成本所付出的代价**

来解释。

---

# 十二、两个真正应该进入 headline 的系统指标

## 12.1 Decomposition ROI

核心问题：

> 什么时候值得把一个任务拆开？

定义：

```text
Decomposition ROI
= ΔGoalSuccess / ΔCost
```

在不同 complexity bucket 上报告：

```text
1–2 nodes
3–5 nodes
6–8 nodes
```

以及：

```text
ΔQuality / ΔCost
ΔLatency / ΔCost
```

目的不是制造一个综合分数，而是建立：

```text
complexity
→ expected orchestration value
```

---

## 12.2 Coordination Tax

回答：

> 为了获得 workflow control，系统到底支付了多少 coordination cost？

两者组合后形成：

```text
Decomposition ROI
        ↕
Coordination Tax
```

这是判断“拆得更多是不是更好”的核心对照。

---

# 十三、最终不采用一个 Composite System Score

不建议：

```text
SystemScore
= 0.3 Quality
+ 0.2 Cost
+ 0.2 Time
+ ...
```

原因：

1. 权重主观；
2. 很容易为了 leaderboard 调权；
3. 不同应用对 cost / quality / latency 的偏好不同；
4. 无法保留 frontier 信息。

推荐直接报告：

```text
Goal Success
Quality
Cost / Success
Latency
Strong Share
Recovery Rate
```

再给三张 frontier：

```text
Quality × Cost
Quality × Latency
Quality × Strong Budget
```

需要决策时使用显式约束：

```text
Max Quality
subject to Cost ≤ B
```

或：

```text
Min Cost
subject to Quality ≥ Q
```

---

# 十四、Failure Attribution 必须从“错误标签”升级为“错误传播图”

当前已有 P1-P11，保留。

SystemBench 再新增：

```text
root_cause
propagation_edges
terminal_failure
```

例如：

```text
P6 Wrong Routing
        ↓
P7 Execution Failure
        ↓
P9 Recovery Failure
        ↓
P11 Goal Failure
```

每个 case 记录：

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

最终 report 同时输出：

```text
Root cause distribution
Terminal failure distribution
Root → terminal propagation matrix
```

这样系统才能回答：

> 一个局部错误最终造成了多大的全局损失？

---

# 十五、S9b：真实 decomposition pilot 的正确定位

## 15.1 输入

```text
10–20 PlanningBench cases
固定模型
固定 prompt
固定 schema
```

## 15.2 输出

只要求得到：

```text
root goal
workflow
nodes
edges
```

以及：

```text
planner raw output
normalized workflow
planning score
invalid reason
```

## 15.3 不要求

S9b 不要求：

```text
每个 node 都能 exact bind RouterBench
```

也不要求：

```text
产生新的 SystemBench headline
```

## 15.4 S9b 的三个研究目的

### A. 校准 planning evaluator

当前 lexical 2-gram Jaccard / greedy matching 只是 deterministic heuristic，尚未被真实 decomposition 数据校准。

因此 pilot 后决定：

```text
保持 tau=0.45
```

或：

```text
修改 tau
```

一旦修改：

```text
新的 evaluator hash
→ 新 benchmark identity
```

绝不能默默重写 v0.1。

### B. 观察真实 workflow morphology

记录：

```text
node count
edge count
depth
width
dependency density
parallel fraction
branching factor
```

与当前 synthetic morphology 对比。

### C. 建立第一批 Semantic Workflow

只有能明确说明：

```text
A produces X
B consumes X
```

的 workflow，才进入 v0.2 semantic set。

---

# 十六、Semantic Workflow v0.2 的目标

v0.2 不应追求“大”。

建议：

```text
20–40 high-quality semantic cases
```

比：

```text
1000 synthetic DAG
```

更有价值。

建议覆盖：

```text
Research
Coding
Data / Calculation
Planning
Content transformation
Tool-like structured tasks
```

但每个 workflow 必须满足：

```text
root goal
↓
meaningful subtasks
↓
meaningful dependencies
↓
artifact contract
↓
aggregation criterion
```

---

# 十七、真实 Workflow Schema v0.2 建议新增字段

```yaml
workflow_id: wf_...
root_goal: ...

nodes:
  - node_id: n1
    role: research
    prompt: ...
    dependency_semantics:
      type: independent
    artifact_contract:
      produces: [facts]
      consumes: []

  - node_id: n2
    role: verification
    prompt: ...
    dependency_semantics:
      type: artifact_dependent
    artifact_contract:
      produces: [verified_facts]
      consumes: [facts]

edges:
  - from: n1
    to: n2
    contract: facts
```

这样未来 L2 replay 可以验证：

```text
producer artifact
matches consumer contract
```

而不是只验证：

```text
edge exists
```

---

# 十八、Split 必须继续以 SystemCase 为单位

RouterBench 使用 query/instance 级 split；SystemBench 必须坚持：

```text
system_case = split unit
```

例如：

```text
SystemCase A
  ├── n1
  ├── n2
  └── n3
```

必须整个进入 calibration 或 test。

严禁：

```text
n1 → calibration
n2 → test
```

也严禁相同 workflow template 的高度近似变体跨 split 泄漏。

v0.2 应额外检查：

```text
root_goal hash
workflow topology hash
artifact contract hash
```

是否跨 split 重叠。

---

# 十九、SystemBench 的 statistical protocol

## 19.1 Bootstrap unit

固定：

```text
system_case
```

## 19.2 Comparison

优先：

```text
paired bootstrap
```

因为同一个 case 同时运行：

```text
B2
B3
B4
B5
B6
B7
```

## 19.3 Headline CI

至少：

```text
ΔGoalSuccess
ΔQuality
ΔCost / success
ΔLatency
ΔStrongShare
ΔRecoveryRate
```

## 19.4 分层

至少：

```text
node count
workflow depth
workflow width
failure class
binding mode
complexity
```

## 19.5 不要把 node 当独立样本

这条必须成为 SystemBench 的统计硬规则。

同一个 workflow 的 node 共享：

```text
root goal
context
state
budget
dependencies
```

因此：

```text
327 node ≠ 327 independent samples
```

---

# 二十、External Benchmark 的接入路线重新排序

## 第一优先级：自身 Frozen / Shadow

```text
SystemBench L1
```

零成本。

## 第二优先级：真实 decomposition

```text
PlanningBench small pilot
```

只用于 Semantic Workflow 建设与 evaluator calibration。

## 第三优先级：Historical trajectory

优先：

```text
τ² historical
ToolSandbox recorded trajectory
```

因为它们能够引入真实：

```text
state
artifact
trajectory
```

τ-bench 官方仓库已经长期提供 historical trajectories；当前 tau² 系列也支持 trajectory re-evaluation，这说明“记录轨迹 → 冻结 → 重评”是一条成熟的 benchmark 路线。citeturn984854search8turn984854search6

## 第四优先级：Live External

推荐：

```text
τ²
→ ToolSandbox
→ AppWorld
→ WebArena
```

不要四个一起接。

---

# 二十一、为什么必须采用 Static → Replay → Live

2026 年近期 routing benchmark 已明显向 step-level / agentic trajectory evaluation 演进。

TwinRouterBench 将 routing 分成：

```text
Static
+
Dynamic
```

Static 使用离线、确定性评测；Dynamic 才进入真实 agent execution，并直接看 task resolution 与实际 API spend。citeturn266415academia12

近期 AgentRouter 也直接把问题定义为 multi-step agent workflow 中的 step-level model assignment，而不是只做传统单轮 query routing。citeturn266415academia13

这与 LLooM 应采用的：

```text
L1 Frozen
→ L2 Historical Replay
→ L3 Live
```

高度一致。

因此不需要追求一个“超级大 benchmark”，而应该先保证不同 fidelity 之间的语义连续性。

---

# 二十二、对现有 OrchestrationBench 名称的最终处理

外部已经存在 Kakao 的正式 **OrchestrationBench**，并已作为 ICLR 2026 工作公开，目标就是 workflow planning + constraint-aware tool execution，覆盖多个 service domain 与大量 virtual tools。citeturn266415search0turn266415search14

因此 LLooM 不应把内部工作继续作为对外独立品牌：

```text
LLooM OrchestrationBench
```

建议改为：

```text
LLooM SystemBench
├── Routing Track
│   └── RouterBench
│
├── Workflow Track
│   └── LLooM Workflow Track
│
└── System Track
    ├── L1 Frozen
    ├── L2 Replay
    └── L3 Live
```

内部历史目录：

```text
benchmarks/orchestration/
```

可以继续保留，不需要为了命名做大规模重构。

---

# 二十三、SystemBench 最核心的研究问题重新定义

SystemBench 不应该试图证明：

> “LLooM 比其他 agent benchmark 高多少分。”

更适合证明四个问题。

## RQ1 · Routing

> 在一个已经存在的 workflow 中，是否可以把不同节点交给不同成本模型，从而降低成本而基本不损失质量？

来源：RouterBench。

---

## RQ2 · Orchestration

> 是否能把一个复杂目标组织成合法且高效的 workflow？

来源：Workflow Track / planning / scheduling / recovery。

---

## RQ3 · Interaction

> routing 与 orchestration 联合以后，是否产生额外系统收益？

来源：SystemBench B3/B4/B5/B6。

---

## RQ4 · Global Control

> 局部最优模型选择是否等价于全局 workflow 最优？

来源：Node Oracle vs Joint Oracle，最终进入 L2/L3。

---

# 二十四、SystemBench 的主要实验矩阵

## A · Scheduling

```text
B2 Strong-Sequential
vs
B3 Strong-Parallel
```

指标：

```text
makespan
critical path
parallel width
waste
```

## B · Routing

```text
B3 Strong-Parallel
vs
B4 LLooM-Routed-Parallel
```

指标：

```text
quality
cost
strong share
routing regret
```

## C · Orchestration

```text
B3 Fixed Workflow
vs
B5 LLooM Workflow
```

必须等真实 decomposition 后才进入正式 headline。

## D · Joint

```text
B4 Fixed+Router
vs
B6 LLooMWorkflow+Router
```

这才是核心 SystemBench interaction comparison。

## E · Recovery

```text
B6 Recovery Off
vs
B7 Recovery On
```

指标：

```text
success
recovery rate
cost
latency
```

## F · Oracle

```text
B6
vs
O1
vs
O2
vs
O3
```

形成不同层级 regret。

---

# 二十五、当前 13.9% completion 结果的正确表述

现阶段不得把：

```text
B4 completion = 13.9%
```

解释为：

> “LLooM orchestration 只有 13.9% 成功率。”

正确表述是：

> **在当前 L1 frozen、独立 node outcome、固定 workflow composition 口径下，B4 与 B2/B3 的 completion proxy 相同，但 B4 workflow cost 更低；当前结果衡量的是 control-plane routing composition gain，而不是完整语义 workflow 的最终任务成功率。**

20 号报告的现有结果是：

```text
B2 13.9% / $0.002896
B3 13.9% / $0.002896
B4 13.9% / $0.002463
O1 61.5% / $0.001743
```

且 routing cost Δ 为 -14.96%，bootstrap CI 不含 0。这个结果是有价值的，但 headline 必须带 fidelity label。fileciteturn20file0L86-L104

推荐对外写：

```text
L1 Frozen Composition
Routing reduces workflow model cost by ~15%
without changing the current completion proxy.
```

而不是：

```text
LLooM improves task success by X%.
```

---

# 二十六、当前 O1 61.5% 应如何解释

O1 是：

```text
node-level oracle
```

它不是：

```text
真实 workflow oracle
```

所以：

```text
61.5%
```

只能解释为：

> 固定当前 workflow 后，在 node 级模型选择上存在显著优化空间。

不能说：

> LLooM 距离真正最优 workflow 还有 47.6pp。

因为 O1 与 Full LLooM 不是同一 optimization problem。

需要 O3 Joint Oracle 后，才可以谈：

```text
workflow control-plane regret
```

---

# 二十七、Production Policy 与 Benchmark Policy 必须继续分离

RouterBench v0.2 的 P2：

```text
routerbench_v02_p2
```

是 frozen benchmark variant。

生产系统可能是：

```text
production_router_<commit>
```

两者绝不写成同一个：

```text
LLooM Router
```

SystemBench manifest 必须保留：

```yaml
routing_policy:
  benchmark_id: routerbench_v02_p2
  policy_hash: ...

production_policy:
  commit: ...
```

这可以防止研究结果和生产实现互相污染。

---

# 二十八、Manifest / Identity 体系

SystemBench 当前已有 10 项 hash，这是正确方向。

最终建议统一为：

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
```

后续 L2 增加：

```text
artifact_schema_hash
trajectory_source_hash
state_evaluator_hash
```

L3 增加：

```text
provider_config_hash
runtime_env_hash
tool_version_hash
```

任何输入变化：

```text
→ new SystemBench identity
```

不得 overwrite old result。

---

# 二十九、数据库建议

保持下层库独立。

SystemBench 自己至少：

```text
bench_system_cases
bench_system_runs
bench_system_items
bench_system_events
```

### bench_system_cases

保存：

```text
system_case_id
workflow_id
workflow_hash
split
binding_summary
replay_level
evaluator
```

### bench_system_runs

保存：

```text
run_id
benchmark_id
policy_hash
manifest_hash
started_at
mode
summary_json
```

### bench_system_items

保存：

```text
run_id
case_id
success
quality
cost
latency
strong_share
primary_error
recovery_status
```

### bench_system_events

L2/L3 再启用：

```text
node event
model call
tool call
state transition
artifact produced
artifact consumed
retry
fallback
```

这是以后做 propagation analysis 的核心数据。

---

# 三十、Production Code Boundary

继续保持：

```text
SystemBench
      ↓
production seam
      ↓
router.rs / orchestrator.rs
```

benchmark 禁止复制：

```text
routing formula
complexity formula
wave computation
recovery policy
```

如果接口不可测：

```text
add pure function
or
trait
or
test seam
```

但不要在 benchmark 里复制一份逻辑。

20 号阶段的 Shadow parity 已证明这种结构可以工作，因此应继续沿用，而不是为了方便把业务逻辑搬回 Python benchmark。

---

# 三十一、Future L2 Historical Replay 的正式定义

L2 不应被定义成：

```text
“把历史 JSON 再跑一遍”
```

而应定义为：

> **给定一个历史 workflow trajectory，在不重新调用模型的前提下，使用真实历史 artifacts / states 重建决策点，并对 routing / scheduling policy 进行 counterfactual replay。**

标准输入：

```text
historical_task
historical_trajectory
historical_model
historical_outputs
historical_tools
historical_state
```

输出两个世界：

```text
Original World
Counterfactual LLooM World
```

比较：

```text
final outcome
cost
latency
strong share
retries
failures
```

必须记录：

```text
counterfactual_validity
```

当某个 counterfactual node 改变后导致 downstream input 无法重建：

```text
counterfactual_status = invalid
```

不得强行给成功/失败数字。

---

# 三十二、L2 的“Counterfactual Validity”将成为关键指标

建议：

```text
valid_cf_rate
artifact_reconstruction_rate
state_reconstruction_rate
```

例如：

```text
100 historical cases
80 可以完整重建
20 无法重建 downstream state
```

则：

```text
valid_cf_rate = 80%
```

正式 headline 应只在 valid counterfactual subset 上报告，并单独给 coverage。

这比把所有历史 trajectory 强行塞进一个数字更严谨。

---

# 三十三、L3 Live 的进入条件

只有同时满足：

```text
Gate A · L1 methodology
Gate B · Semantic Workflow
Gate C · Historical Replay
```

才进入正式 Live。

第一轮 Live 不要直接 100–1000 cases。

建议：

```text
10–30 sanity cases
↓
50–100 held-out cases
↓
再考虑规模化
```

正式 Live 必须计入：

```text
model input/output cost
judge cost
runtime/tool cost
wall-clock
retries
fallback
```

不能只报模型 API cost。

---

# 三十四、External Benchmark 接入优先级

## Tier 1 · 自有 frozen asset

```text
RouterBench v0.2
Workflow Track v0.1
```

## Tier 2 · Planning / scheduling

```text
PlanningBench
PlanBench
TPS-Bench（如果数据正式可得）
```

当前 TPS-Bench 论文描述 200 个 compounding tasks 并同时强调完成率与 scheduling efficiency；但实际公开仓库数据可获得性必须以当前仓库内容为准，不应按论文摘要自动视为“可零成本复现”。citeturn984854search12

## Tier 3 · Stateful historical

```text
τ² historical
ToolSandbox recorded trajectories
```

## Tier 4 · Live environment

```text
τ²
AppWorld
WebArena
```

## Tier 5 · Large-scale external validation

等自己的 L2/L3 语义稳定后再上。

---

# 三十五、为什么现在不需要继续追求更多 benchmark 名称

SystemBench 当前最大的瓶颈不是：

```text
benchmark count
```

而是：

```text
semantic fidelity
```

如果现在再增加：

```text
5 个 planning benchmark
10 个 tool benchmark
20 个 routing benchmark
```

但全部只能变成：

```text
node score
```

研究价值不会线性增加。

优先把：

```text
workflow
→ node
→ artifact
→ state
→ model
→ schedule
→ final goal
```

这条链做完整。

---

# 三十六、SystemBench 对外展示的建议

最终 UI 不应只有：

```text
Quality
Cost
```

而应该展示一条可追溯的 workflow：

```text
User Goal
   ↓
Complexity
   ↓
Decomposition
   ↓
Workflow Graph
   ↓
Model Assignment
   ↓
Wave Schedule
   ↓
Execution
   ↓
Recovery
   ↓
Aggregation
   ↓
Final Goal
```

点击 node 能看到：

```text
prompt
model
routing decision
source outcome
cost
latency
status
failure
artifact
```

最后显示：

```text
Single Strong
Fixed Strong
Fixed Router
Full LLooM
Oracle
```

并同时显示：

```text
Quality
Cost
Latency
Strong Share
```

这会比“LLooM 分数比 baseline 高”更能解释系统价值。

---

# 三十七、最终研究叙事

项目最后不要变成：

```text
RouterBench
+ OrchestrationBench
+ SystemBench
= 三个 benchmark
```

应变成一套连续的 inference-control research story：

```text
                    LLooM
                      │
        ┌─────────────┴─────────────┐
        │                           │
   Routing Track               Workflow Track
   “选哪个模型”                “怎么组织任务”
        │                           │
        └─────────────┬─────────────┘
                      ↓
                 System Track
                      │
             ┌────────┼────────┐
             ↓        ↓        ↓
            L1       L2       L3
          Frozen   Replay    Live
```

三个核心命题：

### 第一层

> **不是每个 query 都需要最强模型。**

RouterBench 已经有数据证据。

### 第二层

> **不是每个复杂任务都应该由一个模型一次完成。**

Workflow Track 用 planning / scheduling / recovery 证据支持。

### 第三层

> **一个真正的 inference-control system，需要同时解决“怎么组织工作”和“每一步由谁完成”。**

SystemBench 负责证明两者结合后是否产生实际系统收益。

---

# 三十八、下一阶段正式排期

## Phase S10 · L1 Hardening

**目标：不增加任何 API 成本。**

```text
1. Scheduling headline
2. System matched random
3. Recovery ablation
4. Cost validity audit
5. Shadow coverage audit
6. Interaction matrix
7. O2/O3 oracle
```

完成条件：

```text
S-5 remaining L1 baseline = closed as far as frozen semantics permit
S-6 L1 propagation attribution = complete
L1 report = standalone publishable
```

---

## Phase S11 · S9b Tiny Live Calibration

```text
10–20 PlanningBench cases
```

输出：

```text
real decomposition
planning evaluator calibration
semantic workflow seeds
```

不刷新 v0.1 headline。

---

## Phase S12 · Semantic Workflow v0.2

```text
20–40 cases
```

要求：

```text
artifact contracts
real semantic dependencies
workflow-group split
new manifest
```

---

## Phase S13 · L2 Historical Replay

```text
10–30 historical trajectories first
```

目标：

```text
Original
vs
LLooM counterfactual
```

核心指标：

```text
valid_cf_rate
final success
cost
latency
strong share
recovery
```

---

## Phase S14 · SystemBench v0.2 Main Result

正式报告：

```text
B0–B7
O1–O3
paired bootstrap
frontier
failure propagation
```

这一阶段才可以形成一份完整的 SystemBench 主结果。

---

## Phase S15 · L3 Live External

顺序：

```text
10–30 sanity
↓
50–100 held-out
↓
external benchmark expansion
```

---

# 三十九、验收 Gate

## Gate A · L1 Frozen Methodology

```text
✅ exact binding
✅ production parity
✅ deterministic replay
✅ workflow split
✅ bootstrap
✅ manifest freeze
⬜ scheduling headline
⬜ system matched random
⬜ recovery ablation
⬜ cost validity
⬜ coverage explanation
⬜ O2/O3 oracle
```

---

## Gate B · Semantic Workflow

必须：

```text
20–40 semantic cases
real decomposition
real dependency semantics
artifact contract
```

---

## Gate C · Historical Replay

必须：

```text
historical output/state
counterfactual validity
artifact reconstruction
paired original/counterfactual
```

---

## Gate D · Live

必须：

```text
real model
real tool
real state
real cost
real latency
real final success
```

---

# 四十、当前明确“不做”的事情

为了避免研究范围失控，以下暂不进入 P0：

```text
1. 不继续扩大 RouterBench dataset
2. 不继续训练大型 router
3. 不继续调 frozen P2 的 λ / penalty
4. 不重新挑 RouterBench test case
5. 不对不同 benchmark 分别人工调系统 threshold
6. 不把 fuzzy outcome binding 放进 headline
7. 不用 node 数当 bootstrap unit
8. 不把 L1 node score 相加当作最终答案质量
9. 不把 O1 当成完整 workflow oracle
10. 不立刻 full-run WebArena / AppWorld / τ²
11. 不开发复杂 WebUI 先于 benchmark semantics
12. 不继续使用 LLooM OrchestrationBench 作为新的公开 benchmark 品牌
```

---

# 四十一、研究风险清单

| 风险 | 现状 | 控制措施 |
|---|---|---|
| Test leakage | 当前已冻结 | system_case split + manifest |
| Fuzzy binding | 已禁止 | exact binding hard rule |
| Fake zero cost | 已出现过 | cost validity policy |
| Synthetic workflow bias | 当前存在 | Semantic Workflow v0.2 |
| Planning evaluator heuristic | 当前存在 | S9b calibration |
| Node independence assumption | L1 明确存在 | dependency_semantics + L2 |
| Oracle misinterpretation | O1 易被误读 | O1/O2/O3 分层 |
| Production/benchmark policy 混淆 | 已分离 | policy_hash |
| External benchmark reproducibility | 部分不稳定 | source registry + observed availability |
| Live cost drift | 必然存在 | cost_basis + runtime manifest |
| Parallel scheduling overclaim | 当前已有 deterministic simulator | live/trajectory validation |

---

# 四十二、最终技术架构

```text
                       LLooM SystemBench
                              │
             ┌────────────────┼────────────────┐
             │                │                │
       Routing Track    Workflow Track    System Track
             │                │                │
       RouterBench v0.2   Workflow v0.1   SystemCase
             │                │                │
             └──────────┬─────┴───────────────┘
                        │
                 Composition Layer
                        │
            ┌───────────┼────────────┐
            ↓           ↓            ↓
           L1          L2           L3
        Frozen      Historical      Live
        Outcome     Artifact       Execution
        Replay       Replay
            │           │            │
            └───────────┼────────────┘
                        ↓
                System-level Evaluator
                        │
       ┌────────────────┼─────────────────┐
       ↓                ↓                 ↓
    Quality           Cost             Latency
       │                │                 │
       └────────────────┼─────────────────┘
                        ↓
                 Failure Attribution
                        ↓
                   Pareto Frontier
```

---

# 四十三、SystemBench 最终成功条件

SystemBench 真正成功，不是因为：

```text
benchmark 数量很多
```

也不是因为：

```text
有一个很高的 composite score
```

而是因为最终可以从同一个 `SystemCase` 追溯：

```text
root goal
  ↓
workflow
  ↓
subtask
  ↓
model decision
  ↓
schedule
  ↓
execution
  ↓
artifact/state
  ↓
recovery
  ↓
final goal
```

并且对于每一个最终结果，都能回答：

```text
为什么这个模型被选中？
为什么这个节点被并行？
为什么这个任务失败？
失败在哪里开始？
错误如何传播？
如果换一个模型会怎样？
如果不并行会怎样？
如果启用 recovery 会怎样？
系统为此付出了多少 cost / latency？
距离 joint oracle 还有多远？
```

这才是 LLooM SystemBench 与普通 Agent leaderboard 最值得拉开的区别。

---

# 四十四、最终决策表

| 项目 | 决策 | 原因 |
|---|---|---|
| RouterBench | **Maintenance** | v0.2 已完整冻结并统计闭环 |
| Workflow Offline | **Freeze / reuse** | O-0~O-5 已完成 |
| SystemBench L1 | **保留并收口** | 已有 exact binding / parity / replay / bootstrap |
| L1 Hardening | **立即执行** | 低成本，直接提高结果可信度 |
| S9b decomposition | **执行，小规模** | 校准 semantics，不用于刷新 headline |
| Semantic Workflow v0.2 | **下一主阶段** | 解决 synthetic morphology / independent-node 局限 |
| L2 Historical | **随后进入** | 提供真实 artifact/state counterfactual |
| L3 Live | **最后** | 环境、成本、执行噪声最大 |
| WebArena/AppWorld/full τ² | **暂缓** | 目前不是主要科学瓶颈 |
| 大型 router training | **暂缓** | 目前需要的是 system semantics，不是更强 classifier |
| Composite Score | **不做** | 主观权重且掩盖 frontier |
| OrchestrationBench 品牌 | **改为 Workflow Track** | 避免与已有公开 benchmark 同名 |

---

# 四十五、最终执行一句话

> **现在不是再证明“LLooM 会 routing”，也不是再证明“LLooM 有 orchestrator”；现在要证明的是：当一个复杂任务被组织成 workflow 后，LLooM 能否在每个状态下选择合适的模型、合法调度工作、处理失败，并以可解释的 quality / cost / latency frontier 完成整个目标。**

实施顺序固定为：

```text
L1 Hardening
    ↓
S9b Tiny Calibration
    ↓
Semantic Workflow v0.2
    ↓
L2 Historical Counterfactual
    ↓
L3 Live
```

**任何跨越一个 Fidelity Level 的结果，必须有新的 manifest identity、新的 coverage 声明和独立的统计口径。**

---

# 附录 A · 参考资料

## LLooM / 本项目内部资料

- `17-RouterBench实施总结-收官Day9-15.md`
- `18-OrchestrationBench实施总结-O-Day9-14.md`
- `19-LLooM-SystemBench-合并与改进实施指导-v2.md`
- `20-SystemBench实施总结-合并验收与PhaseAB.md`

## 外部资料

- TPS-Bench: https://aclanthology.org/2026.acl-long.1614/
- TwinRouterBench: https://arxiv.org/abs/2605.18859
- AgentRouter: https://arxiv.org/abs/2609.22951
- Kakao OrchestrationBench: https://openreview.net/pdf/ce1efaa8e60fa63f37212961c37f393e875eb03b.pdf
- Kakao announcement: https://www.kakaocorp.com/page/detail/11919
- τ-bench repository: https://github.com/sierra-research/tau-bench
- τ²-bench evaluation docs: https://github.com/sierra-research/tau2-bench/blob/main/docs/evaluation.md
- τ²-bench evaluation implementation: https://github.com/sierra-research/tau2-bench/blob/main/src/tau2/evaluator/evaluator.py

---

# 附录 B · 推荐的下一次提交顺序

```text
S10.1  sched headline
   ↓
S10.2  matched random
   ↓
S10.3  recovery ablation
   ↓
S10.4  cost/coverage audit
   ↓
S10.5  interaction + O2/O3
   ↓
S10.6  freeze v0.1 report revision
   ↓
S11    10–20 real decomposition pilot
   ↓
S12    Semantic Workflow v0.2
   ↓
S13    L2 historical replay
```

每一步均应：

```text
git status
→ path-scoped add
→ commit
→ git pull --rebase origin v2
→ cargo clippy
→ cargo test
→ deterministic self-check
→ report update
```

并继续沿用并行会话协议中的“路径限定 add / commit”规则，避免共享工作树再次发生 staged ownership 混入。
