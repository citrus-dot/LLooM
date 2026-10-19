# 14 · LLooM OrchestrationBench 方案

> 日期：2026-10-06
> 状态：生产级实施设计；可在 RouterBench v0.1 基础上开工
> 核心目标：用尽可能多的公开 benchmark、公开 evaluator、历史轨迹和确定性检查器，系统评价 LLooM 的“复杂度判断 → 任务拆解 → 子任务路由 → 依赖/波次调度 → 并行执行 → 聚合/恢复”能力。

---

## 一、结论先行

LLooM 的任务编排已经是当前产品能力，而不是概念功能。当前公开仓库把 `/api/orchestrate/stream` 定义为完整编排入口，把 `/api/routing/plan-subtask` 定义为子任务级路由规划入口；v2 roadmap 还将独立子任务并行执行、按顺序聚合标记为 shipped。citehttps://github.com/citrus-dot/LLooM citehttps://raw.githubusercontent.com/citrus-dot/LLooM/v2/ARCHITECTURE.md

本地 Day 1–5 实施记录也确认：Rust 化后已有 `orchestrator.rs`，涉及复杂度、分解、波次编排；因此本阶段应正式给它建立独立 benchmark，而不是只把它当成聊天演示功能。fileciteturn7file1L28-L35

**核心判断：**

```text
RouterBench
= query → model

OrchestrationBench
= complex task → workflow

SystemBench
= complex task → workflow → models → execution → result
```

OrchestrationBench 不应该成为另一个“从零造大型 Agent Arena”的项目；应优先复用 PlanningBench、TPS-Bench、APB、PlanBench、ToolSandbox、τ²-Bench 等已有资源。

---

# 二、为什么 OrchestrationBench 必须独立于 RouterBench

RouterBench 当前的基本单位是：

```text
query × model → score + cost
```

因此它能回答：

> 对一个已确定的 query，哪个模型更适合？

但不能回答：

> 这个复杂目标是否应该被拆开？
> 应该拆成哪些工作单元？
> 哪些工作可以并行？
> 哪些必须串行？
> 如何在中途失败后重新规划？

所以需要新的 canonical object：

```text
Workflow
├── root goal
├── subtasks
├── dependencies
├── waves
├── model assignment
├── execution trace
└── aggregation
```

---

# 三、LLooM 当前能力的 benchmark 映射

| 当前 LLooM 能力 | 当前证据 | 应用什么 benchmark 测 |
|---|---|---|
| Complexity detection | 编排入口 / orchestrator | APB planning regime、复杂度分布 |
| LLM decomposition | `/api/orchestrate/stream`、orchestrator | APB、PlanningBench |
| Subtask-level routing | `/api/routing/plan-subtask` | 直接复用 RouterBench 逻辑 |
| Dependency / wave | Day1–5 `orchestrator.rs`；parallel roadmap | TPS-Bench |
| Parallel execution | roadmap shipped | TPS-Bench scheduling |
| Aggregation | orchestration endpoint | milestone / final-goal evaluator |
| Failure / fallback | LLooM routing / health / fallback 体系 | ToolSandbox、τ²-Bench 后续 |

注意：公开 README 中仍有旧的“sequential execution”描述，而 roadmap 已标记 parallel subtask execution shipped。citehttps://github.com/citrus-dot/LLooM

因此 benchmark 以**实际代码行为和最新实现状态为准**，并把 README 文案同步作为一个独立维护项。

---

# 四、benchmark 调研后的最终分工

## 4.1 PlanningBench：零成本主力 Planning Track

PlanningBench 当前公开 467 个 evaluation instances，每个实例提供 planning question 和 verification checklist，可按约束满足与目标质量做验证；任务 taxonomy 覆盖 6 个 planning family 和 30+ task/subtask/constraint/difficulty 类型。citehttps://github.com/Tencent-Hunyuan/PlanningBench

优势：

```text
公开
小
可重复
有 checklist
适合确定性 evaluator
```

定位：

```text
OrchestrationBench Planning Core
```

限制：它测试的是结构化 planning，不天然等同于 LLooM 的现实工作流 decomposition。

---

## 4.2 TPS-Bench：Scheduling 主力

TPS-Bench 有 200 个 compounding tasks、两级难度、数百 MCP tools；每项任务由多个 subtasks 构成，并明确把 task completion 和 scheduling efficiency 分开评价。citehttps://aclanthology.org/2026.acl-long.1614/

这是和 LLooM 当前“subtask + wave + parallel”最贴近的公开 benchmark。

核心使用价值：

```text
reference subtask structure
↓
合法依赖
↓
合法并行
↓
实际 schedule
```

定位：

```text
OrchestrationBench Scheduling Core
```

---

## 4.3 APB：Planning Diagnostic / Online Track

APB 有 4,209 个多模态 cases、22 domains，覆盖 holistic、step-wise、tool-extraneous、tool-broken、unsolvable 等 planning settings，明确试图区分 planning 与 downstream execution。citehttps://arxiv.org/abs/2606.04874

但官方 runner 当前要求 tested model 和 judge model API，因此**不能把 APB 原始流程定义成 zero-cost**。citehttps://github.com/Mikivishy/AgentPlanningBenchmark/blob/main/README.md

定位：

```text
Online Planning Diagnostic
```

只有取得公开历史 predictions/judgements 时，才能把相应子集纳入 Offline Replay。

---

## 4.4 PlanBench：Structural Sanity Track

PlanBench 使用 Blocksworld、Logistics 等自动规划领域，并提供自然语言、obfuscated、randomized 等版本；公开仓库已经提供自动生成与验证工具。citehttps://arxiv.org/abs/2206.10498 citehttps://github.com/harshakokel/PlanBench

定位：

```text
结构化 planning sanity
鲁棒性 / obfuscation sanity
```

不要把它当作现实 orchestration benchmark 的唯一代表。

---

# 五、Execution / Recovery benchmark 的后置接入

## 5.1 ToolSandbox

ToolSandbox 支持 stateful execution、隐式 state dependency、用户模拟、多轨迹动态 milestone evaluation；重点不是“最后一句话像不像答案”，而是最终与中间状态是否达到目标。citehttps://aclanthology.org/2025.findings-naacl.65/

适合作为：

```text
Execution / Recovery Track
```

但第一阶段不强制运行在线环境。

---

## 5.2 τ²-Bench

τ-bench / τ²-bench 通过动态 user-agent interaction 和最终数据库状态比较进行评估，并提供 pass^k 一类可靠性指标；官方仓库明确推荐最新的 τ²-bench，并保留 historical trajectories。citehttps://arxiv.org/abs/2406.12045 citehttps://github.com/sierra-research/tau-bench

这是最值得优先研究的**历史轨迹复用**资源之一。

可先做：

```text
historical trajectory
→ failure / recovery analyzer
```

再考虑真实执行。

---

## 5.3 AppWorld

AppWorld 提供 9 个日常 app、457 APIs、750 个任务，并用 state-based tests 检查任务完成与 collateral damage。citehttps://arxiv.org/abs/2407.18901

定位：

```text
Future End-to-End External Track
```

不作为 v0.1 zero-cost 主线。

---

## 5.4 WebArena / AssistantBench / GAIA

WebArena 提供真实感网站和长 horizon tasks；AssistantBench 包含 214 个 realistic、time-consuming web tasks；GAIA 以 466 个现实问题综合考察 reasoning、web、multimodality 和 tool use。citehttps://arxiv.org/abs/2307.13854 citehttps://arxiv.org/abs/2407.15711 citehttps://arxiv.org/abs/2311.12983

它们更像：

```text
“整个 Agent 行不行？”
```

而不是：

```text
“LLooM 的 orchestrator 是否行？”
```

因此全部后置。

---

# 六、编排 benchmark 的双轨制度

## 6.1 Offline / Replay Track

默认主线，目标是**零 API 成本**。

允许：

```text
公开 benchmark data
公开 reference/checklist
历史 trajectories
已有 LLooM traces
静态 execution simulator
failure injection
```

禁止：

```text
新 LLM generation
新 LLM judge
新 provider inference
```

主要回答：

```text
workflow 是否合法？
schedule 是否合理？
parallel 是否安全？
故障状态机是否正确？
```

## 6.2 Online / Execution Track

只有 Offline diagnostic 后才进入。

允许：

```text
real LLM decomposition
real subtask routing
real tools
real execution
judge
```

但必须单独计：

```text
model cost
judge cost
tool/runtime cost
wall time
```

---

# 七、Canonical Workflow Schema

外部 benchmark 不直接进入 production orchestrator。

先统一为：

```json
{
  "workflow_id": "...",
  "root_goal": "...",
  "nodes": [
    {
      "id": "t1",
      "description": "...",
      "inputs": [],
      "outputs": ["x"],
      "constraints": [],
      "model": null
    }
  ],
  "edges": [
    {"from": "t1", "to": "t3", "type": "data"}
  ],
  "waves": [],
  "aggregation": {"type": "final_synthesis"}
}
```

原则：

```text
benchmark adapter
→ canonical workflow
→ production orchestrator / evaluator
```

而不是：

```text
benchmark A format
→ benchmark 专用 orchestrator
```

---

# 八、最重要的评测层：Planning / Scheduling / Execution 分离

## O1 · Planning / Decomposition

回答：

```text
有没有漏掉必要工作？
有没有拆出无用工作？
依赖有没有表达清楚？
```

## O2 · Scheduling

回答：

```text
独立任务有没有并行？
依赖任务有没有被提前？
有没有不合法并行？
```

## O3 · Execution / Recovery

回答：

```text
模型 / tool 出错后能不能继续？
是否选择正确 fallback？
是否出现状态污染？
```

## O4 · End-to-End

回答：

```text
最终 goal 是否完成？
```

这样才能避免“最终 success=失败，所以不知道哪层坏了”。APB 的设计动机就是解决这一类 evaluation attribution 问题。citehttps://arxiv.org/abs/2606.04874

---

# 九、Planning 指标

### 9.1 Subtask Recall

```text
required atomic obligations covered
─────────────────────────────────
required atomic obligations
```

### 9.2 Subtask Precision

```text
useful subtasks
───────────────
proposed subtasks
```

### 9.3 Redundancy Rate

```text
redundant / proposed
```

### 9.4 Dependency Edge Precision / Recall

只针对有 reference graph 的 benchmark。

### 9.5 Invalid Plan Rate

违反硬 constraint 的计划比例。

### 9.6 Unsolvable Recognition

对于明确不可解任务：

```text
正确拒绝 / 正确报告不可解
```

APB 的 unsolvable setting 对此有直接参考价值。citehttps://arxiv.org/abs/2606.04874

---

# 十、Scheduling 指标

### 10.1 Wave Count

实际执行波次数。

### 10.2 Serial Baseline

如果全部顺序执行，需要多久。

### 10.3 Ideal Legal Schedule

在 reference DAG 下只满足依赖约束的理论最短 makespan。

### 10.4 Actual Makespan

真实 orchestrator schedule 的 makespan。

### 10.5 Sequentialization Waste

```text
actual makespan
−
ideal legal makespan
```

### 10.6 Illegal Parallelism Rate

依赖尚未满足却发生并行执行的比例。

### 10.7 Parallelization Utilization

```text
actual useful parallel work
───────────────────────────
legal parallel opportunity
```

不要只展示“parallel tasks 数量”。

---

# 十一、Execution / Recovery 指标

```text
Task Success
Milestone Success
Failed Subtasks
Recovery Rate
Fallback Success
Abort Rate
Partial Success
```

对于环境型 benchmark：

```text
final state == target state
```

优先于 LLM judge。

ToolSandbox、τ²-Bench、AppWorld 的 state-based / milestone-based evaluation 是这一层的主要参考。citehttps://aclanthology.org/2025.findings-naacl.65/ citehttps://arxiv.org/abs/2407.18901

---

# 十二、Cost / Time 指标

编排天然会增加调用量，因此必须同时记录：

```text
LLM Calls
Input Tokens
Output Tokens
LLM Cost
Wall-clock Time
Critical Path
Cost / Successful Task
Time / Successful Task
```

Anthropic 在其生产 multi-agent research 系统中明确指出 multi-agent 系统的 token 消耗显著高于普通 chat，同时并行 subagents 对 breadth-first research 有明显价值。因此“性能增加是否值得额外成本”是编排 benchmark 的必要维度。citehttps://www.anthropic.com/engineering/multi-agent-research-system

---

# 十三、LLooM 特有的失败分类

每个 case 允许：

```text
primary_error
secondary_errors[]
```

第一版 taxonomy：

```text
P1 Missing Subtask
P2 Spurious / Redundant Subtask
P3 Wrong Dependency
P4 Invalid Parallelism
P5 Unnecessary Serialization
P6 Wrong Model Assignment
P7 Execution Failure
P8 Fallback Failure
P9 Recovery Failure
P10 Aggregation Failure
P11 Final Goal Failure
```

一个典型链条：

```text
P1 Missing Subtask
   ↓
P3 Wrong Dependency
   ↓
P7 Execution Failure
```

因此不能只记录最终失败。

---

# 十四、RouterBench 与 OrchestrationBench 的联动

LLooM 的一个独特优势是：subtask assignment 不需要另造模型选择系统。

```text
Orchestrator
    ↓
subtask
    ↓
router::plan()
    ↓
model
```

因此在 OrchestrationBench 中，对每个 subtask 都可以复用 RouterBench 的：

```text
tie-aware oracle
weak recall
oracle regret
cost-matched random
model diagnostics
```

形成新的：

### Subtask Model Recall

```text
oracle-best model
vs
LLooM-selected model
```

### Orchestration Routing Regret

```text
subtask oracle score
−
selected model score
```

### Global Workflow Cost

```text
sum(subtask costs)
```

这样 RouterBench → OrchestrationBench 不是两条独立代码线。

---

# 十五、Baseline 体系

## Planning

```text
Reference Plan
Single LLM Plan
LLooM Plan
```

## Scheduling

```text
Static Sequential
Static Parallel
Greedy Ready-Queue
Oracle Schedule
LLooM Scheduler
```

## Model assignment

```text
Fixed Strong
Fixed Mid
Fixed Weak
Cheapest
LLooM Router
Oracle Router
```

## End-to-End

```text
Single Strong
Static Sequential Strong
Static Parallel Strong
Orchestrator + Fixed Strong
Orchestrator + Cheapest
Orchestrator + Router
Full LLooM
Oracle Schedule + Oracle Router
```

这些 baseline 能回答“到底是哪一层产生收益”。

---

# 十六、零成本阶段的推荐 benchmark 顺序

```text
1. PlanningBench
   ↓
   canonical planning + validator

2. TPS-Bench
   ↓
   scheduling metadata + offline scheduler

3. PlanBench
   ↓
   planning sanity / robustness

4. τ² historical trajectories
   ↓
   offline recovery analysis

5. APB
   ↓
   online planning diagnostic

6. ToolSandbox
   ↓
   stateful execution

7. AppWorld / WebArena
   ↓
   external end-to-end
```

**不要倒过来。**

---

# 十七、第一阶段工程结构

```text
benchmarks/
└── orchestration/
    ├── README.md
    ├── manifests/
    │   ├── datasets.yaml
    │   ├── workflow_schema.json
    │   └── evaluator.yaml
    ├── adapters/
    │   ├── planningbench.py
    │   ├── tpsbench.py
    │   ├── planbench.py
    │   ├── apb.py
    │   └── tau2.py
    ├── normalize.py
    ├── validate.py
    ├── schedule.py
    ├── diagnose.py
    ├── bootstrap.py
    └── reports/
```

生产代码：

```text
crates/lloom-core/src/orchestrator.rs
crates/lloom-core/src/router.rs
```

原则：benchmark 不复制 orchestrator / router 的核心逻辑。

---

# 十八、数据库设计

继续使用 benchmark 独立数据库，不污染 production。

```sql
CREATE TABLE bench_workflows (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  benchmark_id TEXT NOT NULL,
  sample_id TEXT NOT NULL,
  root_goal TEXT NOT NULL,
  workflow_json TEXT NOT NULL,
  reference_graph_json TEXT,
  verification_json TEXT,
  split TEXT NOT NULL,
  meta_json TEXT,
  UNIQUE(benchmark_id, sample_id)
);
```

```sql
CREATE TABLE bench_orchestration_runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  benchmark_id TEXT NOT NULL,
  manifest_hash TEXT NOT NULL,
  strategy TEXT NOT NULL,
  variant TEXT NOT NULL,
  parameter_json TEXT,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  summary_json TEXT
);
```

```sql
CREATE TABLE bench_orchestration_items (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id INTEGER NOT NULL,
  sample_id TEXT NOT NULL,
  predicted_workflow_json TEXT,
  execution_trace_json TEXT,
  task_success INTEGER NOT NULL,
  planning_score REAL,
  scheduling_score REAL,
  execution_score REAL,
  cost REAL,
  makespan_ms REAL,
  primary_error TEXT,
  secondary_errors_json TEXT,
  decision_json TEXT,
  UNIQUE(run_id, sample_id)
);
```

---

# 十九、Manifest / Freeze 原则

沿用 RouterBench 已验证的：

```text
source hash
benchmark hash
scorer hash
split hash
manifest hash
```

特别增加：

```text
workflow_schema_hash
evaluator_hash
schedule_policy_hash
```

任一变化都必须产生新的 benchmark identity。

RouterBench 当前已经使用六级 hash / manifest freeze；OrchestrationBench 应直接继承同一治理模式。fileciteturn7file8L176-L191

---

# 二十、Offline Scheduler 的生产级要求

Offline scheduler 不是 mock，而是 deterministic reference implementation，用于验证生产 scheduling policy。

输入：

```text
DAG
node duration profile
availability
failure scenario
max concurrency
```

输出：

```text
waves
start/end
critical path
parallel width
violations
```

固定：

```text
seed
clock
node ordering
failure schedule
```

同一输入必须得到相同 schedule。

---

# 二十一、Failure Injection

v0.1 不需要真实 provider。

可在 offline scheduler / executor simulator 注入：

```text
F1 timeout
F2 selected model unavailable
F3 malformed output
F4 missing dependency output
F5 parallel branch failure
F6 partial aggregator input
F7 budget exhausted
```

并验证：

```text
retry
fallback
abort
partial success
replan
```

这部分是 LLooM 最值得做成差异化的 benchmark 内容之一，因为产品本身已经有 health / fallback / budget / routing 基础设施。

---

# 二十二、Online Pilot

Online 第一轮只做极小规模：

```text
PlanningBench   30–50
TPS-Bench       20–30
APB             20–30
```

目的不是论文 headline，只验证：

```text
LLM
 ↓
decomposition
 ↓
canonical graph
 ↓
router
 ↓
scheduler
 ↓
execution
 ↓
evaluator
```

链路通了以后才扩大。

---

# 二十三、统计设计

正式 Online headline：

```text
Bootstrap 95% CI
```

至少报告：

```text
Plan validity
Subtask recall
Dependency accuracy
Task success
Recovery rate
Cost
Wall time
Parallelization efficiency
```

bootstrap 以 task instance 为单位，不在 test 数据上重新调 router。

---

# 二十四、WebUI 展示设计

## Page 1 · Overview

```text
Task Success
Plan Validity
Cost
Time
```

## Page 2 · Task Graph

真实显示：

```text
Node
→ dependency
→ model
→ wave
→ status
```

## Page 3 · Scheduling

```text
Sequential
vs
LLooM
vs
Oracle
```

显示：

```text
wall time
waves
parallel width
```

## Page 4 · Error Taxonomy

```text
Missing
Redundant
Dependency
Parallelism
Routing
Execution
Recovery
Aggregation
```

## Page 5 · RouterBridge

点击 subtask 可以直接进入：

```text
RouterBench decision snapshot
```

---

# 二十五、最终展示叙事

## 第一层：任务太复杂

```text
User Goal
```

## 第二层：LLooM 发现复杂度

```text
Complex
```

## 第三层：拆解

```text
Research A
Research B
Calculation
Synthesis
```

## 第四层：每个子任务再路由

```text
A → Strong
B → Weak
C → Mid
D → Strong
```

## 第五层：波次

```text
Wave 1
A B C

Wave 2
D
```

## 第六层：结果

```text
Quality
Cost
Latency
Success
```

因此现场看到的不是：

> “LLooM 有多 agent。”

而是：

> **“LLooM 会把一个复杂目标变成一个可执行工作流，并在工作流内部继续进行模型级路由。”**

---

# 二十六、何时进入外部 Execution Track

只有以下条件全部满足：

```text
Planning validator 稳定
Scheduling validator 稳定
Router parity 稳定
Error taxonomy 稳定
Offline failure injection 稳定
Online pilot 链路稳定
```

才进入：

```text
τ²
ToolSandbox
AppWorld
WebArena
```

否则外部 benchmark 的失败无法定位。

---

# 二十七、验收 Gate

### O-0 Schema

Canonical workflow 可稳定解析。

### O-1 Dataset

source hash / coverage / license / scorer 完整。

### O-2 Planning

PlanningBench / PlanBench static evaluator 全绿。

### O-3 Scheduling

TPS-Bench scheduling simulator deterministic。

### O-4 Router parity

subtask `plan()` 与 production 一致。

### O-5 Failure

F1–F7 状态转移可重复。

### O-6 Online

小规模真实链路可跑通。

### O-7 Report

完整误差、成本、时间、CI。

---

# 二十八、与 RouterBench 的最终关系

```text
                  LLooM
                    │
          ┌─────────┴─────────┐
          │                   │
      RouterBench       OrchestrationBench
          │                   │
     query → model       task → workflow
          │                   │
          │             subtask → model
          │                   │
          └─────────┬─────────┘
                    ▼
                SystemBench
```

RouterBench 测“局部模型选择”。

OrchestrationBench 测“全局任务组织”。

SystemBench 测“两者串起来之后的系统效果”。

---

# 二十九、最终实施路线

```text
Phase 0
Canonical Schema

Phase 1
PlanningBench + PlanBench
→ zero-cost planning validation

Phase 2
TPS-Bench
→ zero-cost scheduling validation

Phase 3
Router bridge
→ subtask-level model assignment

Phase 4
Failure injection
→ recovery / fallback

Phase 5
APB / small online pilot
→ real decomposition

Phase 6
τ² / ToolSandbox
→ stateful execution

Phase 7
AppWorld / WebArena
→ external end-to-end validation
```

---

# 三十、最终定义

> **LLooM OrchestrationBench v0.1 是一个面向 LLooM 编排层的诊断型 benchmark suite：它不重新发明通用 Agent benchmark，而是复用现有 planning、scheduling、tool-use、stateful execution 数据与 evaluator，把复杂度识别、任务拆解、依赖调度、并行执行、子任务路由、故障恢复和最终完成度拆开评测，并最终组合回 LLooM 的完整编排链。**

核心原则：

> **不是“拆得越多越好、并行越多越好”，而是“只拆必要的、依赖正确的、尽可能合法并行的、模型选择合理的、失败可恢复的、最终值得付出成本的工作流”。**

---

## 参考资料

- LLooM：https://github.com/citrus-dot/LLooM
- LLooM v2 Architecture：https://raw.githubusercontent.com/citrus-dot/LLooM/v2/ARCHITECTURE.md
- Agent Planning Benchmark：https://arxiv.org/abs/2606.04874
- APB code：https://github.com/Mikivishy/AgentPlanningBenchmark
- PlanningBench：https://arxiv.org/abs/2605.20873
- PlanningBench code：https://github.com/Tencent-Hunyuan/PlanningBench
- TPS-Bench：https://aclanthology.org/2026.acl-long.1614/
- TPS-Bench code：https://github.com/hanwenxu1/mcp-agent
- PlanBench：https://arxiv.org/abs/2206.10498
- PlanBench code：https://github.com/harshakokel/PlanBench
- ToolSandbox：https://aclanthology.org/2025.findings-naacl.65/
- τ-bench / τ²-bench：https://arxiv.org/abs/2406.12045
- τ²-bench code：https://github.com/sierra-research/tau-bench
- AppWorld：https://arxiv.org/abs/2407.18901
- WebArena：https://arxiv.org/abs/2307.13854
- AssistantBench：https://arxiv.org/abs/2407.15711
- GAIA：https://arxiv.org/abs/2311.12983
- MultiAgentBench：https://arxiv.org/abs/2503.01935
- Magentic-One：https://arxiv.org/abs/2411.04468
- Anthropic Multi-Agent Research：https://www.anthropic.com/engineering/multi-agent-research-system
- AutoGen：https://microsoft.github.io/autogen/stable/reference/python/autogen_agentchat.teams.html
- CrewAI：https://docs.crewai.com/core-concepts/Agents
- PlanBench-XL：https://github.com/JiayuJeff/PlanBench-XL
