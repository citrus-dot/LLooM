# 19 · LLooM SystemBench 合并与改进实施指导 v2

> 日期：2026-10-06
> 
> 基于：RouterBench 收官 Day 9-15、OrchestrationBench Offline O-Day 9-14、原 SystemBench 设计稿，以及截至 2026-10 的相关公开 benchmark / 方法资料。
> 
> 目标：把已经完成的 RouterBench 与 OrchestrationBench 合并成一个**可复现、可归因、可扩展、最终可进入 Live External 的 SystemBench**，同时避免在现阶段引入无法验证的“伪 end-to-end”。

---

# 一、先给结论

## 1. 现在可以开始 SystemBench，但不是“把两个 benchmark 文件夹拼起来”

目前两条下层 benchmark 已经到达可以进入 composition layer 的状态：

- RouterBench：`lloom-routerbench-v0.2` 已冻结，test 只读，matched random、budget、bootstrap CI、cross-benchmark 都完成；最终决策为 Native hold，进入 maintenance。
- OrchestrationBench：Offline O-0~O-5 全绿，canonical workflow、PlanningBench / PlanBench adapter、planning evaluator、schedule simulator、router bridge、failure injection、P1-P11 attribution 已完成；O-6 Online 仍后置。

因此现在最合理的动作不是继续在两个下层 benchmark 上扩张，而是：

```text
RouterBench v0.2        ─────┐
                            ├──> SystemBench v0.1 Composition / Shadow
OrchestrationBench v0.1 ───┘
```

SystemBench 的第一版应首先证明：

> **同一个 workflow 中，LLooM 的 planning、routing、scheduling、recovery 决策能够被统一记录、统一重放、统一归因，并在一个 workflow-level 结果上形成完整因果链。**

不是一开始就宣称已经证明“完整 agent 的最终任务能力”。

---

# 二、当前真实状态：两个下层 benchmark 已经分别收官，但 SystemBench 仍有三个关键缺口

## 缺口 A · Router bridge 只有接口，还没有真正的 outcome binding

OrchestrationBench 目前 `router_bridge.py` 已完成 forward/backward 接口，但其 `obx__{wf}__{node}` sample_id 与 RouterBench 既有 `rb0shot_*` 不同，因此直接查询现在必然无法得到真实 frozen outcome；`assign_fixed_tier` 只是 baseline generator，不是实际 router。

这意味着：

```text
Workflow node
   ↓
router bridge
   ↓
?? frozen model outcome
```

还没有真正闭环。

**这是进入 SystemBench 工程化前的 P0。**

---

## 缺口 B · OrchestrationBench 的 planning evaluator 还没有在线校准

当前 planning evaluator 使用字符 2-gram Jaccard + 贪心一对一匹配，阈值 `tau=0.45`。这在离线自检和 PlanBench 结构实验中可用，但目前尚无真实 LLooM decomposition 输出对该阈值进行校准。

因此：

```text
planning_score
```

在 SystemBench v0.1 中应该作为：

```text
Diagnostic metric
```

而不是直接成为：

```text
Final System Success 的唯一裁判
```

对 PlanBench 的 DAG 可验证部分，可以继续使用结构真值；对 PlanningBench，优先把它当作 obligation coverage / planning diagnostic。

---

## 缺口 C · 现在的 SystemBench 设计里“frozen subtask outcome”概念还不够严格

如果只有：

```text
node → quality + cost
```

那么只能重放 cost / score，却不能真实模拟：

```text
A 的输出 → B 的输入 → B 的输出 → Aggregation
```

也就是说，不能直接把一串节点分数简单相加当成 workflow final quality。

SystemBench 必须把 replay 分成至少四个 fidelity level：

```text
L0 · Schedule Replay
    只有 DAG / duration / dependency

L1 · Outcome Replay
    node 有 success / quality / cost / latency

L2 · Artifact Replay
    node 有真实 historical output / artifact，可供 downstream consumption

L3 · Live Execution
    真实模型 + tools + environment
```

**SystemBench v0.1 应以 L1 为主，L2/L3 后置。**

---

# 三、最重要的概念调整：SystemBench 不是“第三个 benchmark”，而是 Benchmark Suite / Composition Layer

建议正式架构改为：

```text
LLooM SystemBench
│
├── Routing Track
│   └── RouterBench v0.2
│
├── Workflow Track
│   └── LLooM-Orch / Workflow Track v0.1
│
├── System Composition Track
│   ├── Frozen Composition
│   ├── Shadow
│   └── Live
│
└── Cross-cutting
    ├── Cost
    ├── Latency
    ├── Reliability
    └── Failure Attribution
```

这样做的好处是：

1. RouterBench 和 Workflow Track 的 benchmark identity 仍然独立，原始结果可以继续复现。
2. SystemBench 不需要复制两个 evaluator。
3. 外部用户可以单独看 routing、workflow、system 三层，也可以只看完整 system。
4. 后续增加新的 routing benchmark 或新的 agent benchmark 时，不需要重写 SystemBench。

---

# 四、一个必须立即处理的研究命名问题

公开社区已经存在一个 **OrchestrationBench**，其 ICLR 2026 版本是一个针对多域 service orchestration 的 bilingual benchmark，包含 DAG workflow planning、工具调用、约 17 个领域和近 100 个虚拟工具；公开仓库也直接命名为 `kakao/OrchestrationBench`。citeturn846449search5turn644878view0

因此不建议继续把 LLooM 自己实现的 benchmark 对外直接称为“OrchestrationBench”。

建议：

```text
内部仓库：LLooM-Orchestration / workflow track

SystemBench 对外：
SystemBench · Workflow Track
```

或者：

```text
LLooM WorkflowBench
```

这样以后引用外部 `OrchestrationBench` 时不会发生 benchmark identity 混淆。

---

# 五、从最新研究得到的 SystemBench 核心设计原则

## 5.1 学 TwinRouterBench：必须同时有 Static/Frozen 与 Dynamic 两层

2026 年的新 TwinRouterBench 已经明确把真实 agentic routing 拆成：

```text
Static
→ fast offline routing supervision

Dynamic
→ held-out end-to-end agent execution
```

它的核心思想是：现实 agent 在一个 trajectory 内会进行很多次模型调用，因此 routing benchmark 不能只看 one-shot query；最终还必须让真实 agent execution 验证“便宜模型选择是否真的保持任务成功”。其 static track 采用 execution-verified tier labels，dynamic track 则在 held-out SWE-bench 上直接测 task resolution 与真实 API spend。citeturn118756academia3turn281825search0

SystemBench 应直接继承这个思想：

```text
S1 Frozen Composition
        ↓
S2 Shadow
        ↓
S3 Live
```

而不是直接跳到 Live。

---

## 5.2 学 APB：必须把 planning 与 execution 分开

APB 的重要价值正是：只看最终 success 很难知道失败到底源于 planning 还是 execution，因此它专门把 holistic planning、step-wise planning、tool-extraneous、tool-broken、unsolvable 等情况分离。citeturn118756academia1

SystemBench 因此必须保留：

```text
Planning Failure
≠
Routing Failure
≠
Scheduling Failure
≠
Execution Failure
≠
Recovery Failure
≠
Aggregation Failure
```

最终每个 case 仍然应该只有一个 `primary_error`，但必须保留 `secondary_errors[]`。

---

## 5.3 学 τ² / ToolSandbox：最终系统必须支持 stateful replay

τ²-bench 当前已经支持历史 trajectory 的重新评分与 replay，而且强调 fresh environment、task definition 版本、trajectory 文件版本等可复现性；ToolSandbox 则强调 state dependency、user simulator、intermediate/final milestones 和任意 trajectory 的动态评估。citeturn844883search0turn844883search1turn844883search6

SystemBench 的长期方向因此不应该是：

```text
node score 相加
```

而应该逐步演进到：

```text
state_0
  ↓
node action
  ↓
state_1
  ↓
node action
  ↓
state_2
  ↓
...
  ↓
final state
```

---

# 六、SystemBench v0.1 的正确定位

## v0.1 只证明“control plane composition”

建议把 SystemBench v0.1 定义成：

> **Workflow-aware inference-control benchmark**

主要测：

```text
workflow validity
+ model assignment
+ scheduling
+ recovery policy
+ cost/latency
+ failure attribution
```

不把下面这些东西强行塞进 v0.1：

```text
大规模 browser benchmark
复杂环境 state mutation
大规模 LLM judge
开放世界 research agent
```

这些全部是 v0.2/v0.3。

---

# 七、建议的 SystemBench 四轨，而不是原来的三轨

原设计的 Shadow / Frozen Replay / Live 方向是对的，但实际实施时建议拆得更清楚：

## Track S0 · Component Parity

目的：确认两个 benchmark 接入后没有改变下层语义。

必须验证：

```text
RouterBench v0.2 replay
==
独立运行的 RouterBench v0.2
```

以及：

```text
Orchestration Track replay
==
独立运行的 O-v0.1
```

S0 是 merge gate，不是对外 headline。

---

## Track S1 · Frozen Composition / L1

输入：

```text
frozen workflow DAG
+
frozen per-node outcomes
+
frozen durations
```

不做新的模型 inference。

SystemBench 负责：

```text
route
→ schedule
→ replay
→ aggregate status
```

这个 track 用来做最大的因果实验矩阵。

### S1 的作用

直接回答：

```text
Routing 是否真的改变 workflow cost？
Scheduling 是否真的改变 workflow latency？
Recovery 是否真的改变 final success？
```

---

## Track S2 · Shadow / L1-L2

让生产 LLooM 真正生成：

```text
complexity
→ decomposition
→ DAG
→ route
→ waves
```

但模型结果优先使用 frozen outcomes / historical artifacts。

这一步的价值不是“便宜”，而是：

> **确认 production decision path 与 benchmark replay schema 真正一致。**

---

## Track S3 · Historical Replay / L2

如果拿到 τ²、ToolSandbox 或其他公开 trajectory：

```text
historical task
→ historical trace
→ historical artifact
```

然后做 counterfactual：

```text
原始 model assignment
vs
LLooM assignment
```

注意：只有存在真实历史输出 / 状态数据时，才能称为 artifact replay。

---

## Track S4 · Live / L3

最后才是真正：

```text
LLM
+ tools
+ models
+ environment
```

先小规模 10-30 case sanity，再 50-100 held-out case，最后再考虑大规模 leaderboard。

---

# 八、SystemBench 最关键的数据对象：不要只做 Workflow Matrix，要做“四层 binding”

推荐最终 canonical object：

```json
{
  "system_case_id": "sb_v01_xxx",
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
      "outcome_binding": {
        "mode": "routerbench_exact",
        "source_benchmark": "llmrouterbench",
        "source_sample_id": "...",
        "prompt_hash": "..."
      }
    }
  ],
  "target": {
    "evaluator": "...",
    "success_rule": "..."
  },
  "replay_level": "L1"
}
```

关键字段：

```text
outcome_binding.mode
```

必须明确只能是：

```text
routerbench_exact
llmrouterbench_exact
historical_artifact
synthetic_fixture
live_only
```

**禁止出现一个模糊的 `lookup`。**

尤其禁止：

```text
nearest query
embedding similarity
guessing model outcome
```

出现在 headline evaluation 中。

---

# 九、如何解决当前 Router Bridge 的真正问题

现在不要做：

```text
obx__wf__node
→ 猜一个最相似的 RouterBench sample
```

正确方法是新增：

```text
SystemCase Builder
```

负责建立：

```text
system_case_id
  ↓
node_id
  ↓
outcome_binding
  ↓
benchmark sample
```

并把关系固化为 manifest。

### 两类 binding

#### Exact binding

```text
node.prompt
==
source sample.prompt
```

通过 `prompt_hash` / canonical query hash 验证。

#### Derived binding

允许研究实验里存在，但必须标成：

```text
derived / diagnostic
```

不能进入 headline。

---

# 十、SystemBench v0.1 最值得做的不是“所有 workflow”，而是一个高质量 Composition Set

建议第一版只做 **50-100 个 System Cases**。

不要直接把：

```text
467 PlanningBench
+
1901 PlanBench
```

全部塞入 SystemBench。

它们继续作为 Workflow Track 数据资产。

SystemBench 自己应只挑选可以真正形成：

```text
workflow
+
node outcome
+
execution semantics
```

的 case。

---

# 十一、推荐的 80-case System Set

## 20 个结构简单

```text
1-2 node
low dependency
```

用途：

```text
bridge parity
baseline
```

## 30 个中等 DAG

```text
3-5 nodes
mixed sequential / parallel
```

用途：

```text
routing + scheduling
```

## 20 个复杂 DAG

```text
5-8 nodes
multiple waves
```

用途：

```text
parallelism
critical path
```

## 10 个 failure cases

```text
timeout
model failure
malformed output
dependency failure
budget exhaustion
```

用途：

```text
recovery / fallback
```

第一版重点不是规模，而是**每一个 case 都能真正闭环**。

---

# 十二、SystemBench baseline 必须改成“因果隔离矩阵”

原方案的 B0-B9 方向正确，但建议重新整理：

| Baseline | Planning | Routing | Scheduling | Recovery | 目的 |
|---|---|---|---|---|---|
| B0 Flat-Strong | flat | strong | N/A | off | 单模型基准 |
| B1 Flat-BestSingle | flat | fixed best | N/A | off | 简单基线 |
| B2 FixedWorkflow-Strong-Seq | fixed | strong | sequential | off | 拆解成本 |
| B3 FixedWorkflow-Strong-Par | fixed | strong | parallel | off | 并行收益 |
| B4 FixedWorkflow-LLooMRouter | fixed | LLooM | parallel | off | routing 增益 |
| B5 LLooMPlanner-Strong | LLooM | strong | LLooM | off | planning 增益 |
| B6 LLooMPlanner-Router | LLooM | LLooM | LLooM | off | 联合决策 |
| B7 Full-LLooM-Recovery | LLooM | LLooM | LLooM | on | 完整系统 |
| B8 Local Oracle | fixed | per-node oracle | ref schedule | off | 局部上界 |
| B9 Joint Oracle | fixed | joint | ideal legal | optional | 组合上界 |

其中最重要的是：

```text
B4
B5
B6
B7
```

因为它们能够直接把：

```text
routing gain
planning gain
coordination gain
```

拆出来。

---

# 十三、不要只用一个 Oracle，要分三层 Oracle

## O1 · Node Oracle

每个 node 单独选最优模型。

```text
argmin_model cost / argmax quality
```

## O2 · Schedule Oracle

固定 DAG，只找最优合法 wave / schedule。

## O3 · Joint Oracle

固定 workflow，在所有允许的模型 assignment + legal schedule 中寻找最优组合。

SystemBench v0.1 暂时不要做完整：

```text
workflow topology search × model assignment × schedule
```

因为搜索空间过大，也很难公平。

先固定 Reference Workflow。

---

# 十四、质量指标必须从“node score”升级为“workflow outcome”

这是合并时最值得修正的一点。

不能定义：

```text
Workflow Quality
=
mean(node_quality)
```

除非 benchmark 明确声明它只是在做 component diagnostic。

推荐优先级：

```text
Primary
→ Goal Success / Final State

Secondary
→ Final Quality
→ Milestone Success

Diagnostic
→ node quality
→ routing regret
```

### L1 Outcome Replay 的最终成功

如果还没有真实 artifact，则必须明确成功语义，例如：

```text
all required nodes succeeded
AND
all required dependencies satisfied
AND
aggregation_valid
```

并称：

```text
workflow completion
```

而不要包装成：

```text
semantic final answer quality
```

---

# 十五、加入一个新的系统核心指标：Coordination Tax

定义：

```text
Coordination Tax
=
(System cost under orchestration)
/
(sum of independently optimal node costs)
- 1
```

或者用 cost-constrained frontier 表达同一个概念。

它能够回答：

> 为了获得 workflow coordination，系统额外付出了多少成本？

这样可以识别：

```text
“编排成功但过于昂贵”
```

这一类系统。

---

# 十六、加入三个真正有研究价值的“增益”指标

## Routing Gain

固定 workflow 与 schedule：

```text
Fixed-Strong
vs
LLooM-Routed
```

关注：

```text
Δquality
Δcost
Δstrong_share
```

## Orchestration Gain

固定模型策略：

```text
Flat Strong
vs
Workflow Strong
```

关注：

```text
Δquality
Δlatency
Δcompletion
```

## Coordination Gain

完整系统：

```text
Full LLooM
vs
best isolated component composition
```

这才是真正的：

```text
SystemBench headline
```

---

# 十七、时间指标不要只看 wave_count

当前 O-D11 已有：

```text
wave_count
critical_path
actual
sequentialization waste
parallelization utilization
illegal parallelism
```

SystemBench 中继续保留，但增加：

```text
planning overhead
routing overhead
queue / wait time
execution time
recovery overhead
aggregation overhead
```

最终：

```text
wall_time
=
planning
+ routing
+ scheduling
+ execution
+ recovery
+ aggregation
```

如果没有在线 wall-clock 数据，则离线 track 只报告：

```text
model latency
critical path
simulated makespan
```

不能伪装成真实端到端 latency。

---

# 十八、Failure Attribution 应升级为“第一失败 + 传播图”

现在 O-v0.1 的 P1-P11 单主错误归因已经很好，但 SystemBench 可以进一步增加：

```text
root_cause
↓
propagation_edges
↓
downstream_effects
```

例如：

```text
P6 Wrong Model Assignment
        ↓
P7 Execution Failure
        ↓
P11 Final Goal Failure
```

这样最终能画出：

```text
Failure Origin
→ Failure Propagation
→ Final Loss
```

这是比单纯“失败率”更能体现 orchestration system engineering 的数据。

---

# 十九、SystemBench 的 primary error priority 应固定

建议：

```text
O1 Complexity / Decomposition
O2 Workflow Validity
O3 Routing
O4 Scheduling
O5 Execution
O6 Recovery
O7 Aggregation
O8 Final Goal
```

每个 case：

```json
{
  "primary_error": "routing",
  "secondary_errors": ["execution", "aggregation"],
  "root_cause_layer": "routing",
  "terminal_layer": "aggregation"
}
```

这样可以区分：

```text
“系统在哪一层犯错”
```

和：

```text
“系统最终在哪里爆炸”
```

---

# 二十、SystemBench split 必须升级为“workflow-group split”

RouterBench 目前按 sample_id 做 hash split；SystemBench 不应该照搬到 node 层。

正确：

```text
system_case / workflow_id
    ↓
hash
    ↓
calibration / test
```

同一个 workflow 的：

```text
root
nodes
historical trajectories
outcomes
```

必须全部留在同一个 split。

否则很容易产生：

```text
Node A → calibration
Node B → test
```

从而共享 workflow template / semantics 导致泄漏。

---

# 二十一、SystemBench 的 test freeze 必须同时冻结 10 项

建议 manifest：

```text
systembench_id
routerbench_id
workflow_track_id
workflow_schema_hash
model_pool_hash
routing_policy_hash
scheduler_policy_hash
failure_policy_hash
evaluator_hash
split_hash
cost_basis_hash
```

再额外记录：

```text
runtime_version
orchestrator_commit
router_commit
source_dataset_hashes
```

任何输入变化：

```text
=> new SystemBench identity
```

---

# 二十二、SystemBench 不应该重新 freeze RouterBench 的 test

SystemBench 应引用：

```text
routerbench_manifest_v02
```

而不是复制：

```text
routerbench test_selection
```

否则将来会出现：

```text
RouterBench v0.2
≠
SystemBench 中的“RouterBench”
```

正确关系：

```text
SystemBench manifest
   │
   ├── routerbench_manifest_ref = cbeb...
   ├── workflow_manifest_ref = ...
   └── system_case_manifest = ...
```

---

# 二十三、最重要的生产代码边界

SystemBench 永远不要复制：

```text
router.rs
orchestrator.rs
```

Benchmark 只能：

```text
调用生产函数
```

如果接口不足：

```text
production code
→ test seam / pure function / trait
→ benchmark
```

而不是：

```text
benchmark 内复制一份 router / orchestrator
```

目前 O-Day 11 的 `orchbench_parity` test-only seam 是正确方向，应继续沿用。Stateful / live 阶段也必须保持这个原则。

---

# 二十四、建议的代码结构

建议不要立刻把所有文件搬到一个巨型 `systembench/`。

保留：

```text
benchmarks/
├── routerbench/
│   └── frozen v0.2
│
├── orchestration/
│   └── offline v0.1
│
└── systembench/
    ├── README.md
    ├── manifests/
    │   ├── system.yaml
    │   ├── source_registry.yaml
    │   ├── split.json
    │   └── frozen_manifest_v01.json
    │
    ├── cases/
    │   ├── system_cases.jsonl
    │   └── bindings.jsonl
    │
    ├── adapters/
    │   ├── routerbench.py
    │   ├── workflow.py
    │   └── historical.py
    │
    ├── replay/
    │   ├── l0_schedule.py
    │   ├── l1_outcome.py
    │   └── l2_artifact.py
    │
    ├── runner/
    │   ├── composition.py
    │   ├── shadow.py
    │   └── live.py
    │
    ├── evaluator/
    │   ├── goal.py
    │   ├── routing.py
    │   ├── scheduling.py
    │   ├── recovery.py
    │   ├── failure.py
    │   └── system.py
    │
    ├── scripts/
    └── reports/
```

下层数据不复制。

---

# 二十五、数据库建议

新增四张核心表：

## `system_cases`

```text
case_id
root_goal
workflow_ref
replay_level
target_ref
system_split
manifest_hash
```

## `system_node_bindings`

```text
case_id
node_id
source_benchmark
source_sample_id
prompt_hash
binding_mode
```

## `system_runs`

```text
run_id
case_id
policy_hash
orchestrator_commit
router_commit
status
final_success
final_quality
workflow_cost
wall_time
primary_error
```

## `system_decisions`

```text
run_id
node_id
selected_model
planned_wave
actual_wave
fallback_model
routing_score
outcome_source
status
```

如果将来进入 live，再增加：

```text
system_trace_events
```

记录：

```text
PLAN
DECOMPOSE
ROUTE
SCHEDULE
START
FINISH
FAIL
RETRY
FALLBACK
AGGREGATE
END
```

---

# 二十六、SystemBench 的实验矩阵建议

第一阶段不要一次跑几十个变量。

先固定：

```text
workflow set
model pool
cost basis
scheduler reference
```

只改变：

```text
planning
routing
scheduling
recovery
```

形成 2×2×2×2 的 factorial matrix：

```text
Planning:     Flat / LLooM
Routing:      Strong / LLooM
Scheduling:   Sequential / Parallel
Recovery:     Off / On
```

总计 16 个配置。

但实际对外只需要先报告最有解释力的 8 个：

```text
Flat-Strong-Sequential
Flat-Strong-Parallel
Fixed-Workflow-Strong
Fixed-Workflow-Routed
LLooM-Workflow-Strong
LLooM-Workflow-Routed
LLooM-Workflow-Routed-Recovery
Oracle
```

这样既控制工作量，也能做 factorial attribution。

---

# 二十七、不要为 SystemBench 再造一个大 Composite Score

RouterBench 已经存在 quality / cost / retention / regret 等指标。

SystemBench 第一版最好避免再创造一个：

```text
System Score = 0.4Q + 0.2Cost + 0.2Time ...
```

因为权重高度任意，而且容易遮蔽 trade-off。

更推荐：

```text
Primary:
Goal Success

Secondary:
Quality
Cost / success
P50/P90 latency
Strong share

Frontier:
Quality × Cost
Quality × Time
Quality × Strong Budget
```

真正需要单分数时，只在明确约束下计算：

```text
Max Quality | Cost <= B

or

Min Cost | Quality >= Q
```

这比固定 composite score 更容易解释。

---

# 二十八、SystemBench 最终要做“预算曲线”，而不是一个 budget point

定义：

```text
x = strong-call budget
```

然后报告：

```text
success / quality / cost
```

随着：

```text
0%
10%
20%
...
100%
```

变化的曲线。

原因：RouterBench Day 12 已经发现某些 pool 的 strong budget 具有结构性平坦区；因此 SystemBench 应主动把 budget curve 当作 pool/workflow 属性来解释，而不是只给一个“节省 13.5%”数字。

---

# 二十九、加入“workflow depth / width”分层

SystemBench 最有可能产生一个新的研究结果：

> **routing gain 可能随着 workflow depth / branching factor 改变。**

因此每个 case 增加：

```text
node_count
edge_count
depth
max_width
critical_path_length
parallel_fraction
failure_surface
```

报告：

```text
1-2 node
3-5 node
6-8 node
```

以及：

```text
low parallelism
medium parallelism
high parallelism
```

这样可以回答：

```text
什么时候 orchestration 真正值得？
```

而不是只报告平均值。

---

# 三十、加入“decomposition threshold”实验

LLooM 当前有 complexity detection，因此 SystemBench 最值得测试的一件事情其实是：

```text
什么时候应该拆？
```

不要默认：

```text
complex → always decompose
```

建议生成：

```text
Simple
Medium
Complex
```

三类任务，并测：

```text
Flat success
vs
Orchestrated success
```

最后得到：

```text
Decomposition Benefit Curve
```

这可能比“平均 orchestration success”更有产品价值。

---

# 三十一、一个新的核心指标：Decomposition ROI

定义为：

```text
Decomposition ROI
=
ΔGoalSuccess / ΔCost
```

更稳妥的展示方式仍然是：

```text
GoalSuccess(Cost)
```

即在不同成本水平上画出：

```text
Flat
vs
Decomposed
```

它回答：

> 为了获得 decomposition 带来的额外成功率，增加了多少成本？

---

# 三十二、SystemBench 的 replay 应支持 counterfactual

非常建议把这一能力作为核心特色，而不是附属功能。

例如一条历史 workflow：

```text
A → Strong
B → Strong
C → Strong
```

SystemBench 可以重放：

```text
A → Weak
B → Mid
C → Strong
```

但要求：

```text
same workflow
same task
same evaluator
same historical outcome source
```

因此可以直接问：

> **如果这个 workflow 当时使用 LLooM routing，而不是固定 Strong，结果会怎样？**

这正是 composition benchmark 最有研究价值的地方。

---

# 三十三、但 counterfactual 有一个硬约束

不能把：

```text
独立 node outcome
```

当成：

```text
真正 downstream-independent outcome
```

如果 B 的 prompt 依赖 A 的实际输出，则：

```text
A(Weak)
```

之后的 B outcome 不一定等于：

```text
B(Strong)
```

所以 SystemBench 中必须记录：

```text
dependency_semantics:
  independent
  artifact_dependent
  state_dependent
```

只有：

```text
independent
```

的 case 可以在 L1 outcome replay 中做严格 counterfactual。

`artifact_dependent` 要进入 L2，`state_dependent` 进入 L3。

这是整个 SystemBench 的一个关键科学边界。

---

# 三十四、SystemBench 的历史轨迹优先级

建议按以下顺序寻找可复用轨迹：

```text
P0
LLooM 自己已有 historical trace

P1
τ² historical trajectories

P1
ToolSandbox recorded trajectories

P2
AppWorld logs / state traces

P3
WebArena / other external agent traces
```

τ² 当前已经明确支持对历史 trajectories 进行重新评分，说明“record → freeze → re-grade”这一范式很成熟。citeturn844883search0turn844883search3

---

# 三十五、系统成功率必须采用 workflow-level paired bootstrap

RouterBench Day 13 已经使用实例级 bootstrap，这是正确的。

SystemBench 必须更严格：

```text
bootstrap unit = system_case
```

不能：

```text
node = independent sample
```

同一个 workflow 的 8 个 node 共享：

```text
planning
context
state
cost budget
```

所以必须按 workflow case 成组重采样。

### headline CI

至少报告：

```text
ΔGoalSuccess
ΔQuality
ΔCost / success
ΔLatency
ΔStrongShare
```

并优先采用 paired bootstrap，因为所有 policy 应在同一组 cases 上比较。

---

# 三十六、Merge Gate：真正合并前必须通过 8 个门

## Gate S-0 · Identity

```text
RouterBench manifest frozen
Workflow manifest frozen
System schema frozen
```

## Gate S-1 · Parity

```text
RouterBench numbers unchanged
Orchestration offline numbers unchanged
```

## Gate S-2 · Binding

```text
100% headline nodes exact-bound
```

## Gate S-3 · Split

```text
workflow-group split
```

## Gate S-4 · Replay

```text
L1 deterministic
same input → same output
```

## Gate S-5 · Baselines

至少：

```text
Flat Strong
Fixed Strong
Fixed Routed
Full LLooM
Oracle
```

## Gate S-6 · Attribution

```text
primary error
secondary propagation
```

## Gate S-7 · Statistics

```text
paired bootstrap
95% CI
```

## Gate S-8 · Research honesty

明确区分：

```text
component result
composition result
live result
```

不得把 L1 Replay 结果写成 live agent result。

---

# 三十七、推荐实施顺序

## Phase A · Merge Foundation

先做：

```text
1. 建 systembench/
2. source registry
3. manifest references
4. system schema
5. exact outcome binding
6. workflow-group split
7. parity tests
```

这一阶段不跑任何新的 live LLM。

---

## Phase B · Frozen Composition

然后：

```text
1. 50-100 cases
2. L1 outcome matrix
3. 8 baselines
4. local oracle
5. schedule oracle
6. joint oracle（固定 workflow）
7. paired bootstrap
```

输出第一版：

```text
SystemBench v0.1 Frozen
```

这应该是第一份对外可展示的 SystemBench 结果。

---

## Phase C · Shadow

下一阶段：

```text
真实 orchestrator
真实 router
真实 wave output
↓
仍然消费 frozen outcome
```

重点验证：

```text
production decision path
==
benchmark replay path
```

而不是追求大规模数量。

---

## Phase D · Historical Replay

接入：

```text
τ²
ToolSandbox
LLooM trace
```

开始做：

```text
artifact-aware counterfactual
```

---

## Phase E · Live Pilot

先：

```text
10 cases
```

再：

```text
30 cases
```

最后才：

```text
50-100 held-out
```

严格冻结：

```text
system policy
model pool
pricing
evaluator
```

Live test 期间不改策略。

---

# 三十八、一个值得增加的“生产现实度”指标

TwinRouterBench 强调 router 实际面对的是 trajectory prefix，而不是原始 one-shot prompt。citeturn118756academia3

SystemBench 因此建议给每个 node 记录：

```text
routing_context_length
prior_node_count
prior_success_count
prior_failure_count
available_models
remaining_budget
```

这样以后可以研究：

```text
相同 subtask
在不同 workflow state 下
router 是否应该选择不同模型？
```

这会让 SystemBench 真正从：

```text
query router benchmark
```

升级到：

```text
state-aware system routing benchmark
```

---

# 三十九、不要把“并行越多”当成目标

TPS-Bench、ToolSandbox 以及现有多 agent 系统研究都说明，workflow performance 不是简单地追求最大 parallelism。

SystemBench 必须同时记录：

```text
illegal parallelism
unnecessary serialization
coordination overhead
critical-path gain
```

最终要证明的是：

> **在 dependency constraints 下尽可能安全并行，而不是无脑并行。**

---

# 四十、SystemBench 最终展示图建议改成“三张图”，而不是一张

## 图 1 · Architecture

```text
User Goal
 ↓
Complexity
 ↓
Workflow
 ↓
Router
 ↓
Scheduler
 ↓
Execution
 ↓
Recovery
 ↓
Aggregation
```

## 图 2 · Pareto

```text
Quality
  ↑
  │        Oracle
  │     Full LLooM
  │   Fixed Routed
  │ Flat Strong
  └────────────────→ Cost
```

## 图 3 · Failure Attribution

```text
Planning
  ↓
Routing
  ↓
Scheduling
  ↓
Execution
  ↓
Recovery
  ↓
Aggregation
```

旁边显示每层失败比例与传播损失。

这三张图分别回答：

```text
系统怎么工作？
系统是否更有效率？
系统到底哪里出错？
```

---

# 四十一、最终 headline 应该是什么

不要直接写：

> “LLooM SystemBench 得分 XX%。”

建议最终 headline 分成三句：

### Routing

> LLooM identifies queries/subtasks that can be safely served by cheaper models while preserving quality under a fixed model pool.

### Orchestration

> LLooM decomposes complex tasks into dependency-aware workflows and exploits legal parallelism while preserving task completion.

### System

> Combining routing and orchestration, LLooM moves the quality-cost-latency frontier of workflow execution rather than optimizing either component in isolation.

这样 SystemBench 真正成为 RouterBench + Workflow Track 的自然上层，而不是另一个“综合评分榜”。

---

# 四十二、当前 RouterBench v0.2 应如何进入 SystemBench

必须注意：RouterBench Day 11 冻结的 P2 Soft Eligibility 是一个 benchmark variant，报告已经明确指出它与未来生产侧 soft gate 公式存在语义差异，因此不能把：

```text
P2 frozen benchmark policy
```

直接写成：

```text
production LLooM policy
```

正确做法：

```text
SystemBench
├── Production Policy
└── RouterBench-v0.2 Policy Variant
```

第一版 SystemBench headline 如果是在 offline replay 中，应明确使用：

```text
policy_id = routerbench_v02_p2
```

而 Live 使用：

```text
policy_id = production_v2_<commit>
```

这两个 identity 绝对不能混。

---

# 四十三、对“SystemBench Oracle”的最终建议

v0.1 不做：

```text
Global Workflow Search
```

只做：

```text
Reference Workflow
+
Node Oracle
+
Schedule Oracle
```

如果以后拿到了 artifact-level state replay，再扩展：

```text
Joint Workflow Oracle
```

因此第一版报告必须写：

```text
Oracle = upper bound under fixed workflow and replay assumptions
```

而不是：

```text
globally optimal agent
```

---

# 四十四、SystemBench v0.1 的最小交付物

最终建议锁为：

```text
benchmarks/systembench/

README.md
workflow.schema.json
system.schema.json
source_registry.yaml
frozen_manifest_v01.json
system_cases.jsonl
bindings.jsonl

replay/
  l1_outcome.py

runner/
  composition.py

evaluator/
  system.py
  failure.py

reports/
  baseline.json
  system_v01_report.md
  bootstrap.json
```

以及：

```text
cargo test
python tests
```

所有 deterministic checks 全绿。

---

# 四十五、最终建议的开发顺序（真正落地）

### Day S1

```text
SystemBench namespace
source registry
manifest ref
```

### Day S2

```text
system.schema.json
bindings schema
workflow-group split
```

### Day S3

```text
exact RouterBench binding
20 smoke cases
```

### Day S4

```text
L1 replay engine
parity
```

### Day S5

```text
8 baselines
```

### Day S6

```text
oracle
failure propagation
```

### Day S7

```text
80-case composition set
```

### Day S8

```text
bootstrap
Pareto
budget curve
```

### Day S9

```text
production Shadow
10-20 cases
```

### Day S10

```text
SystemBench v0.1 freeze
```

这十天的核心目标不是“把 SystemBench 做得很大”，而是：

> **让第一版 SystemBench 的每一个数字都能沿着 `case → workflow → node → model → outcome → final result` 完整追溯。**

---

# 四十六、最终系统关系

最终应该收束成：

```text
                         LLooM
                           │
               ┌───────────┴───────────┐
               │                       │
         Routing Track           Workflow Track
         RouterBench v0.2        LLooM-Orch v0.1
               │                       │
               └───────────┬───────────┘
                           ↓
                  SystemBench v0.1
                           │
             ┌─────────────┼─────────────┐
             │             │             │
          Frozen         Shadow        Live
          L1              L1/L2         L3
```

核心研究叙事最终从：

```text
“我们有一个 router + 一个 orchestrator”
```

升级成：

```text
“LLooM 是一个 inference control plane：
它先决定应该怎样组织任务，
再决定每个工作节点由哪个模型完成，
然后决定何时并行、何时串行、如何恢复，
最后以 workflow-level quality / cost / latency 衡量整体结果。”
```

---

# 四十七、最终判断

**现在进入 SystemBench 是对的。**

但要做的是：

```text
合并“接口与数据契约”
```

而不是：

```text
合并“两个 benchmark 的所有代码”
```

其中真正的 P0 只有三件事：

```text
1. Exact Outcome Binding
2. Workflow-level Split + Manifest Freeze
3. L1 Frozen Composition Replay
```

这三件事情完成以后，才真正拥有 SystemBench。

之后：

```text
Shadow
→ Historical Replay
→ Live
```

逐层增加 fidelity。

这样做的好处是：RouterBench 目前已经形成的强定量证据不会被 end-to-end 黑箱稀释；OrchestrationBench 已完成的 planning/scheduling/recovery 诊断也不会被一个最终 success rate 吞掉；最终 SystemBench 则真正承担“两者组合以后是否带来系统级收益”的任务。

---

# 参考资料（本版本新增重点）

- LLMRouterBench — https://arxiv.org/abs/2601.07206
- RouterBench — https://arxiv.org/abs/2403.12031
- TwinRouterBench — https://arxiv.org/abs/2605.18859
- Agent Planning Benchmark (APB) — https://arxiv.org/abs/2606.04874
- PlanningBench — https://arxiv.org/abs/2605.20873
- PlanBench — https://arxiv.org/abs/2206.10498
- ToolSandbox — https://github.com/apple-aiml-research/ToolSandbox
- τ² / τ-Bench — https://github.com/sierra-research/tau2-bench
- AppWorld — https://github.com/bwookwak/appworld_dev
- Public OrchestrationBench (Kakao, ICLR 2026) — https://github.com/kakao/OrchestrationBench
- EPOB — End-to-End Project Orchestration Benchmark
- AgentRouter — https://arxiv.org/abs/2609.22951
