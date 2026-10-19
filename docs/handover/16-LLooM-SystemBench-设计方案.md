# 15 · LLooM SystemBench 设计方案

> 日期：2026-10-06
> 状态：架构设计稿；**等待 OrchestrationBench 实施稳定后再进入工程化**
> 定位：RouterBench + OrchestrationBench 上层的系统级组合评测，不重复实现两个下层 benchmark 的核心 evaluator。
> 核心问题：LLooM 是否能把“复杂用户目标”转化为合适的 workflow，并在 workflow 内继续进行子任务级模型选择、依赖调度、并行执行、失败恢复和结果聚合，在质量、成本、时间之间取得更好的整体结果。

---

# 一、SystemBench 的最终产品命题

LLooM 当前两条亮点线分别是：

```text
RouterBench
→ 不是所有 query 都需要最强模型

OrchestrationBench
→ 不是所有复杂任务都应该由一个模型一次性完成
```

SystemBench 把两者串起来：

```text
User Goal
  ↓
Complexity Detection
  ↓
Should Decompose?
  ↓
Task Decomposition
  ↓
Workflow Graph
  ↓
Subtask Routing
  ↓
Dependency / Wave Scheduling
  ↓
Parallel Execution
  ↓
Fallback / Recovery
  ↓
Aggregation
  ↓
Final Result / Final State
```

因此 SystemBench 的最终命题是：

> **LLooM 是否能在正确组织工作之后，把合适的模型放到合适的工作节点上，并以合理的时间与成本完成整个目标。**

---

# 二、为什么必须等两个子 benchmark 成熟之后再做

如果直接做 end-to-end benchmark，最终只会看到：

```text
Task Success = 63%
```

但不知道失败原因：

```text
Planning ?
Routing ?
Scheduling ?
Execution ?
Aggregation ?
```

而 RouterBench 与 OrchestrationBench 可以先提供独立的诊断层：

```text
RouterBench
→ subtask → model 能不能选对

OrchestrationBench
→ graph / schedule / recovery 能不能做对

SystemBench
→ 两者联动后是否真正提升最终系统目标
```

所以 SystemBench **不是第三套独立 benchmark**，而是两个 benchmark 的 composition layer。

---

# 三、SystemBench 三轨结构

## S1 · Shadow / Offline

不发生新的 LLM inference：

```text
frozen workflow
+
frozen subtask outcomes
+
reference schedule
```

让 LLooM 只负责：

```text
routing
scheduling
policy
aggregation logic
```

主要用于回归测试与超低成本 experiment。

## S2 · Frozen Replay

使用历史 trajectory / frozen model responses：

```text
真实 workflow structure
+
历史模型行为
```

可以观察真实输出带来的 propagation effect，但仍不需要重新调用 provider。

## S3 · Live / External

真实运行：

```text
LLM
+ tools
+ models
+ execution environment
```

用于最终外部验证。

---

# 四、SystemBench 的数据核心：Workflow Outcome Matrix

RouterBench 是：

```text
query × model
```

SystemBench 应升级为：

```text
workflow
 ×
subtask
 ×
model
 ×
execution policy
```

每一个 workflow 最终需要：

```text
root goal
subtask graph
model outcomes
schedule
execution trace
final state
```

推荐数据资产：

```text
Workflow Frozen Matrix
```

它不替代 RouterBench，而是在 RouterBench outcome 之上增加 workflow structure。

---

# 五、SystemBench 如何复用 RouterBench

对于每个 subtask：

```text
subtask prompt
   ↓
router::plan()
   ↓
selected model
```

然后从已有 frozen outcome 查询：

```text
selected_model
→ score / cost
```

因此：

```text
SystemBench
├── Task A
│    ├── Subtask A1 → RouterBench
│    ├── Subtask A2 → RouterBench
│    └── Subtask A3 → RouterBench
└── Task B
     └── ...
```

RouterBench 中已经建立的：

```text
Weak Recall
Oracle Regret
Model Pool
Calibration
Cost
```

可以直接向下传递到 subtask level。

---

# 六、SystemBench 如何复用 OrchestrationBench

OrchestrationBench 输出 canonical workflow：

```text
root_goal
nodes
edges
waves
execution trace
planning score
scheduling score
recovery state
```

SystemBench 只负责把：

```text
workflow
+
router decision
+
model outcome
```

组合成最终任务结果。

因此不会重复实现：

```text
planner
scheduler
evaluator
```

---

# 七、标准 SystemBench 流程

```text
                     User Task
                         │
                         ▼
               Complexity Detector
                         │
              ┌──────────┴──────────┐
              │                     │
           Simple                Complex
              │                     │
              ▼                     ▼
           Router              Decomposer
              │                     │
              ▼                     ▼
           Model A              Workflow DAG
                                    │
                        ┌───────────┼───────────┐
                        ▼           ▼           ▼
                     Router      Router      Router
                        │           │           │
                        ▼           ▼           ▼
                        M1          M2          M3
                        └───────────┼───────────┘
                                    ▼
                                Scheduler
                                    │
                           ┌────────┴────────┐
                           ▼                 ▼
                         Wave 1            Wave 2
                           │                 │
                           └────────┬────────┘
                                    ▼
                                Recovery
                                    ▼
                                Aggregate
                                    ▼
                                Final Goal
```

---

# 八、SystemBench baseline

至少保留：

```text
B0 Single Weak
B1 Single Mid
B2 Single Strong
B3 Static Sequential Strong
B4 Static Parallel Strong
B5 Orchestrator + Fixed Strong
B6 Orchestrator + Cheapest
B7 Orchestrator + LLooM Router
B8 Oracle Router + Reference Schedule
B9 Full LLooM
```

它们分别回答：

```text
单模型基线
拆解收益
并行收益
路由收益
联合收益
理论上界
```

---

# 九、最重要的四组实验

## S-A · Single vs Orchestrated

```text
Single Strong
vs
Orchestrated Strong
```

证明：

> decomposition 本身是否创造收益？

## S-B · Fixed vs Routed Subtasks

```text
Orchestrated Fixed
vs
Orchestrated + Router
```

证明：

> subtask-level model routing 是否产生额外收益？

## S-C · Sequential vs Parallel

```text
Orchestrated Sequential
vs
Orchestrated Parallel
```

证明：

> LLooM wave scheduling 是否真正降低时间？

## S-D · Full vs Oracle

```text
Full LLooM
vs
Oracle Assignment + Oracle Schedule
```

证明：

> 系统距离理想 workflow-aware inference 还有多远？

---

# 十、SystemBench 指标体系

## 10.1 Final Quality

最终答案质量。

## 10.2 Goal Success

环境型 benchmark 中：

```text
final state == target state
```

## 10.3 Planning Validity

```text
workflow valid
```

## 10.4 Routing Quality

```text
subtask model recall
oracle regret
weak recall
```

## 10.5 Scheduling Quality

```text
wave count
critical path
illegal parallelism
sequentialization waste
```

## 10.6 Recovery

```text
initial failure
→ successful recovery
```

## 10.7 Cost

```text
model cost
+ tool cost
+ judge cost（如有）
```

## 10.8 Wall-clock Time

从 root task 开始到 final aggregation。

---

# 十一、System-level Pareto

SystemBench 最终不应只给一个 success rate。

应该提供：

```text
Quality × Cost
Quality × Time
Quality × Strong-call budget
```

例如：

```text
Quality
  ↑
  │                   ● Oracle
  │              ● Full LLooM
  │          ● Orchestrated Fixed
  │     ● Single Strong
  │
  └──────────────────────────→ Cost
```

这张图能够同时体现：

```text
orchestration gain
+
routing gain
```

---

# 十二、最重要的新指标：Workflow-level Model Efficiency

RouterBench 的单位是 query；SystemBench 的单位是 workflow。

因此定义：

```text
Workflow Cost
= Σ subtask model costs
```

```text
Workflow Strong Share
= strong subtask calls / all subtask calls
```

```text
Workflow Quality
= final goal quality
```

随后观察：

```text
same final quality
→ lower workflow cost
```

这是 SystemBench 最核心的系统价值。

---

# 十三、Global Optimality vs Local Optimality

必须避免一个误区：

```text
每个 subtask 独立选最优 model
```

不一定等于：

```text
整个 workflow 成本 / 质量最优
```

例如：

```text
A → Weak
B → Strong
```

如果 B 高度依赖 A 的上下文质量，那么 A 的弱输出可能降低 B 的最终收益。

因此 SystemBench 应最终支持：

```text
Local Oracle
```

和：

```text
Workflow Oracle
```

两种理论上界。

---

# 十四、Workflow Oracle 的第一版定义

完整的组合优化可能非常昂贵，因此 v0.1 不要求搜索所有可能 decomposition。

先定义近似 Oracle：

```text
Reference Workflow
+
Oracle Subtask Model
+
Ideal Legal Schedule
```

这样可以计算：

```text
LLooM gap to workflow oracle
```

后续再增加更完整的 joint oracle。

---

# 十五、SystemBench 错误层级

每次失败只保留一个：

```text
success = false
```

是不够的。

必须记录：

```text
Layer 1 · Complexity / Planning
Layer 2 · Decomposition
Layer 3 · Routing
Layer 4 · Scheduling
Layer 5 · Execution
Layer 6 · Recovery
Layer 7 · Aggregation
```

并允许：

```text
primary_error
secondary_errors[]
```

例如：

```text
Missing Subtask
→ Wrong Routing
→ Partial Execution
→ Aggregation Failure
```

最终输出：

```text
Failure Attribution
Planning       XX%
Decomposition  XX%
Routing        XX%
Scheduling     XX%
Execution      XX%
Recovery       XX%
Aggregation    XX%
```

---

# 十六、一个很重要的 SystemBench 实验：Router-only vs Full System

因为 LLooM 当前已经有：

```text
Router
Orchestrator
```

所以至少做：

```text
Router-only
```

和：

```text
Full System
```

对比。

这能回答：

> orchestration 是否让 Router 获得了新的应用空间？

例如原本：

```text
query
→ Strong
```

拆解后：

```text
Research → Strong
Extraction → Weak
Calculation → Mid
Synthesis → Strong
```

这时 router 的 Weak / Mid 优化就不再作用于单一 query，而作用于整个 workflow。

---

# 十七、SystemBench 与当前 LLooM 产品的强结合点

当前产品公开定位已经把：

```text
smart routing
+
task orchestration
```

并列为核心能力。citehttps://github.com/citrus-dot/LLooM

因此 SystemBench 最终应该重点测的不是通用 agent 的所有能力，而是 LLooM 独有的组合：

```text
cost-aware model routing
+
complexity-aware decomposition
+
subtask-level routing
+
parallel execution
+
fallback / budget
```

这比直接复制一个 AutoGen / AppWorld leaderboard 更有产品意义。

---

# 十八、与现有多-agent系统的关系

AutoGen 的 GraphFlow 已经支持 sequential、parallel fan-out、conditional branch、join 和 loop；CrewAI Flows 也把结构化 workflow、条件、循环和状态作为核心；Anthropic 的生产 research 系统则采用 orchestrator-worker + parallel subagents。citehttps://microsoft.github.io/autogen/stable/reference/python/autogen_agentchat.teams.html citehttps://docs.crewai.com/core-concepts/Agents citehttps://www.anthropic.com/engineering/multi-agent-research-system

LLooM 不需要声称自己发明了这些模式。

真正可以强调的是：

> **LLooM 把 workflow orchestration 与 cost/quality-aware model routing 结合在同一个 inference control plane 里。**

这才是 SystemBench 应该证明的差异点。

---

# 十九、SystemBench 数据来源策略

优先级：

```text
1. OrchestrationBench frozen workflows
2. historical trajectories
3. public stateful environments
4. live external environments
```

优先使用：

```text
PlanningBench
TPS-Bench
APB
τ² historical
ToolSandbox
AppWorld
WebArena
```

其中：

```text
PlanningBench / TPS-Bench
→ workflow structure

τ² historical / ToolSandbox / AppWorld
→ execution state

WebArena
→ external long-horizon validation
```

---

# 二十、SystemBench v0.1 Shadow Track

这是最值得先实现的版本。

输入：

```text
frozen workflow
+
frozen per-model subtask outcomes
```

运行：

```text
LLooM decomposition decision
或 reference decomposition
        ↓
Router
        ↓
Scheduler
        ↓
Outcome lookup
        ↓
Aggregation
```

完全 zero inference。

目的：

```text
验证组合逻辑
验证成本计算
验证 routing / scheduling 交互
```

---

# 二十一、SystemBench v0.2 Frozen Replay

输入历史 trajectory：

```text
user task
→ actual decomposition
→ model calls
→ outputs
→ tool events
```

让 LLooM 重放其中的决策点：

```text
重新选 model
重新安排 schedule
```

但不再次执行 LLM。

这样可以研究：

> 如果同一条历史工作流改用 LLooM 路由策略，会发生什么？

这是很有价值的 counterfactual evaluation。

---

# 二十二、SystemBench v0.3 Live

等 OrchestrationBench 和 RouterBench 都稳定后，再真实执行：

```text
LLM
+ tools
+ providers
```

优先：

```text
τ²
ToolSandbox
AppWorld
```

再：

```text
WebArena
GAIA
```

正式报告必须把：

```text
model cost
judge cost
tool/runtime cost
```

全部计入。

---

# 二十三、工程结构

```text
benchmarks/
├── routerbench/
├── orchestration/
└── systembench/
    ├── README.md
    ├── manifests/
    │   ├── datasets.yaml
    │   ├── workflow_schema.json
    │   ├── model_pool.yaml
    │   └── system_policy.yaml
    ├── adapters/
    │   ├── routerbench.rs
    │   ├── orchestration.rs
    │   └── external.rs
    ├── replay/
    │   ├── shadow.rs
    │   └── frozen.rs
    ├── evaluator/
    │   ├── workflow.rs
    │   ├── routing.rs
    │   ├── scheduling.rs
    │   ├── quality.rs
    │   └── errors.rs
    ├── scripts/
    └── reports/
```

SystemBench 不重复实现下层 benchmark 的 source adapter。

---

# 二十四、生产代码边界

唯一决策真源继续是：

```text
router::plan()
```

编排部分同样必须让 benchmark 调用生产 orchestrator，而不是复制一份：

```text
Production Orchestrator
        ↑
        │
SystemBench
```

如果某个 production interface 不适合测试，应先增加纯函数 / trait / test seam，而不是在 benchmark 中复制逻辑。

---

# 二十五、Hash / Manifest

继承 RouterBench：

```text
benchmark_hash
pool_hash
split_hash
scorer_hash
cost_basis_hash
```

新增：

```text
workflow_schema_hash
orchestrator_policy_hash
scheduler_policy_hash
evaluator_hash
```

任何改变都生成新的 SystemBench identity。

---

# 二十六、WebUI 最终形态

## Overview

```text
Final Quality
Success
Cost
Time
Strong Share
```

## Workflow Graph

```text
root
↓
nodes
↓
model
↓
waves
```

## Router Details

点击 subtask：

```text
RouterBench decision
```

## Scheduling

```text
Sequential
Parallel
Oracle
```

## Failure Attribution

```text
Planning
Routing
Scheduling
Execution
Recovery
Aggregation
```

这样 UI 不再只是展示“LLooM 很智能”，而是可以追溯每一层决策。

---

# 二十七、最终展示方式

一个完整复杂任务贯穿全场：

```text
User Goal
   ↓
Complexity
   ↓
Decomposition
   ↓
Task Graph
   ↓
Model Assignment
   ↓
Wave Scheduling
   ↓
Parallel Execution
   ↓
Recovery
   ↓
Aggregation
   ↓
Final Result
```

旁边实时显示：

```text
Model
Cost
Time
Status
```

最后再切到：

```text
Single Strong
vs
Full LLooM
vs
Oracle
```

这会是整个 LLooM 最完整的 demonstration。

---

# 二十八、最终研究叙事

```text
RouterBench
“不是所有 query 都需要最强模型。”
        ↓

OrchestrationBench
“不是所有 complex task 都应该由一个模型完成。”
        ↓

SystemBench
“复杂任务可以被组织成 workflow，
每一个工作节点再由合适模型完成，
最后以合理的成本与时间完成整个目标。”
```

这三层关系应保持清晰：

```text
RouterBench
= Local Model Decision

OrchestrationBench
= Workflow Organization

SystemBench
= End-to-End Inference Control
```

---

# 二十九、实施顺序

### 当前

```text
RouterBench
→ Weak rescue / model recall
```

### 下一阶段

```text
OrchestrationBench
→ Planning
→ Scheduling
→ Recovery
```

### 两者 Gate 全绿之后

```text
SystemBench Shadow
```

### 再之后

```text
SystemBench Frozen Replay
```

### 最后

```text
SystemBench Live External
```

---

# 三十、SystemBench 的最终定义

> **LLooM SystemBench v0.1 是 RouterBench 与 OrchestrationBench 之上的系统级组合 benchmark。它以统一 workflow schema 将复杂度检测、任务拆解、子任务级模型路由、依赖调度、并行执行、失败恢复与结果聚合串成一个完整的 inference-control loop，并在 frozen / replay / live 三种运行模式下，以最终质量、成功率、成本、时间和失败归因衡量 LLooM 是否真正实现“正确的任务由正确的模型在正确的时机完成”。**

---

## 参考资料

- LLooM：https://github.com/citrus-dot/LLooM
- LLooM v2 Architecture：https://raw.githubusercontent.com/citrus-dot/LLooM/v2/ARCHITECTURE.md
- Agent Planning Benchmark：https://arxiv.org/abs/2606.04874
- PlanningBench：https://arxiv.org/abs/2605.20873
- TPS-Bench：https://aclanthology.org/2026.acl-long.1614/
- PlanBench：https://arxiv.org/abs/2206.10498
- ToolSandbox：https://aclanthology.org/2025.findings-naacl.65/
- τ-bench / τ²-bench：https://arxiv.org/abs/2406.12045
- AppWorld：https://arxiv.org/abs/2407.18901
- WebArena：https://arxiv.org/abs/2307.13854
- AssistantBench：https://arxiv.org/abs/2407.15711
- GAIA：https://arxiv.org/abs/2311.12983
- MultiAgentBench：https://arxiv.org/abs/2503.01935
- Magentic-One：https://arxiv.org/abs/2411.04468
- Anthropic Multi-Agent Research：https://www.anthropic.com/engineering/multi-agent-research-system
- AutoGen：https://microsoft.github.io/autogen/stable/reference/python/autogen_agentchat.teams.html
- CrewAI：https://docs.crewai.com/core-concepts/Agents
