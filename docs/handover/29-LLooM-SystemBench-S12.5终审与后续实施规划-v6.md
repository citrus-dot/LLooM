# 29 · LLooM SystemBench S12.5 终审与后续实施规划 v6

> 日期：2026-10-08  
> 定位：基于 28 号 S12.5 全程记录的**终审、纠偏与后续执行入口**。  
> 当前阶段：**SystemBench v0.2 Semantic Workflow 数据与协议定型期**，尚未进入新路由内核机制研发。  
> 上游：27 · S12.5-S15 最终实施决策与指导；28 · S12.5 全程记录；25/26 · Semantic Workflow 与 Pool Audit 路线。  
> 本文目的：重新检查 S12.5 的方法学正确性，明确哪些结论已经成立、哪些仍不足以 freeze，并给出 S12.5.4 → S12.7 → S13 → S15 的执行顺序。

---

# 一、结论先行

当前工作总体**正确，不需要回滚，也不需要推翻 S12.2-S12.5**。

但是，28 号报告中的：

```text
Pool Audit = GO
        ↓
S12.6 Full Harvest
```

应修正为：

```text
Pool Audit = CONDITIONAL GO
        ↓
Outcome Protocol Stability Probe
        ↓
GO / REVIEW / STOP
        ↓
S12.6 Full Harvest
```

原因只有一个，但非常重要：

> 当前 72 个 semantic outcome cells 中，65 个（90.3%）在固定 1536 输出预算下 `finish_reason=length`；而当前模型排序又出现 `weak > mid > strong` 的反直觉结果。此时无法把“模型能力差异”和“输出预算/截断效应”完全分离。

因此现在最大风险已经不是：

```text
Pool 是否互补？
```

而是：

```text
模型排名是否被 generation protocol 过度主导？
```

本文件的决策是：

> **先验证 protocol stability，再决定是否全量采集。**

这一小步的成本远低于在 175×3 个模型结果采完之后才发现排名主要来自截断协议。

---

# 二、当前总体阶段判断

## 2.1 已完成的主线

```text
RouterBench v0.2
        ↓
Frozen / Maintenance

Workflow Track v0.1
        ↓
Offline 基础层完成

SystemBench v0.1
        ↓
L1 Frozen Composition

S11
        ↓
真实 decomposition pilot

S12.0
        ↓
Annotation Spec

S12.1
        ↓
Seed Audit

S12.2
        ↓
Semantic candidate set

S12.3
        ↓
Semantic annotation + atomicity 修复

S12.4
        ↓
Routing input 双口径实测

S12.5
        ↓
Provider / Contract / Semantic Probe / Pool Audit
```

## 2.2 当前尚未进入的事情

以下均不是当前主线：

```text
❌ 新的大型 routing algorithm
❌ 新 neural router
❌ embedding router
❌ trajectory-aware router 的生产实现
❌ Full agent live benchmark
❌ 大规模 τ² / AppWorld / WebArena
```

目前仍处在：

```text
Benchmark / Evaluation Infrastructure
                ↓
Semantic Outcome Matrix
                ↓
SystemBench v0.2 Freeze
```

只有 SystemBench v0.2 的 measurement protocol 与 semantic matrix 稳定后，才进入下一阶段的：

```text
Core Mechanism Research
```

---

# 三、S12.5 当前状态复核

28 号记录的工作链是：

```text
S12.5.0 Provider Probe
        ↓
依赖语义一致性修复
        ↓
S12.5.1 Contract Freeze
        ↓
S12.5.2 Semantic Probe
        ↓
S12.5.3 Pool Audit
```

四步全部完成，工程质量门保持：

```text
cargo clippy = 0 error
cargo test = 138 / 138
binding integrity = PASS
```

因此工程层面：

> **PASS。**

真正需要重新审查的是实验协议层。

---

# 四、已经成立的几个强结论

## 4.1 Semantic workflow 不应该直接复用 PlanningBench checklist 作为 decomposition gold

S11 已经实测证明：PlanningBench checklist 与真实 decomposition node 的语义类别不同，lexical similarity 极低，因此不应该通过调低 tau 强行拟合。

正确处理是：

```text
PlanningBench
→ verification artifact / goal-level validity

PlanBench / 人工 semantic gold
→ decomposition evaluation
```

这个决定保持不动。

---

## 4.2 Dependency consistency 已修复

当前：

```text
33 independent
142 artifact_dependent
```

因此：

```text
33
→ L1 candidate

142
→ L2 conditional candidate
```

之前发现的 18 个 dependency_semantics 错标与 14 个 dangling consumes 均已修复并幂等复核归零。

这一块不用再动。

---

## 4.3 Routing / Execution input 必须继续双轨

当前正确结构：

```text
R1
routing_input = description
→ production-faithful headline

R2
routing_input = root_goal + description
→ context-rich diagnostic
```

执行输入：

```text
independent
→ base_text

artifact_dependent
→ base_text + frozen upstream artifacts
```

未来 schema 继续明确保存：

```text
routing_input_hash
execution_input_hash
```

不能合并成单一 `prompt` 字段。

---

# 五、S12.4 的真正研究发现：Context 本身已经成为 routing control variable

当前实测：

| Input | Hard | Medium | Easy | Multi-model Eligible |
|---|---:|---:|---:|---:|
| R2 full prompt | 91.4% | 8.6% | 0% | 8.6% |
| R1 description | 56.0% | 36.6% | 7.4% | 44.0% |

因此：

```text
Eligible(R2) / Eligible(R1)
=
8.6% / 44.0%
≈ 0.1948
```

这个值应继续作为 diagnostic：

```text
Routing Context Compression
```

它目前不是 headline performance metric，但非常值得保留。

研究含义：

> router 看到更多信息，不一定拥有更多可用选择；如果复杂度 gate 与输入长度耦合，额外上下文反而可能把大量 query 推入 hard tier，从而压缩 routing search space。

这与近期 step-level / trajectory-level routing 研究方向是相容的：路由器不再只是看到原始 query，而是在不同 workflow step 上根据当时可见的 context 做决策。TwinRouterBench 将 Static 与 Dynamic routing 分层，AgentRouter 则直接把 multi-step trajectory 的逐步模型选择作为问题。相关设计强调 router-visible prefix 与真实 downstream outcome 的区别。

---

# 六、S12.5 最大的问题：Generation Protocol

## 6.1 当前异常

当前 Probe：

```text
72 cells
65 truncated
truncation = 90.3%
```

模型均分：

```text
weak = 0.883
mid  = 0.875
strong = 0.846
```

也就是：

```text
weak > mid > strong
```

这个结果本身没有必要被否定，但是不能马上解释成：

> weak 在 semantic task 上比 strong 更强。

更可能是三种因素共同作用：

```text
真实能力差异
+
回答长度 / generation style
+
1536 输出预算截断
```

因此目前 Pool Audit 的结论只能解释为：

> **在 P1536 generation regime 下，三模型表现存在可路由差异。**

还不能解释为：

> **三模型一般能力排序就是这样。**

---

# 七、不要废掉 P1536，而是把它正式定义成 Benchmark Profile

1536 可以保留，甚至值得保留。

但必须明确它是：

```text
P1536 · Budget-Constrained Profile
```

定义：

```yaml
thinking: false
max_output_tokens: 1536
```

其研究问题是：

> 在固定输出预算下，哪个模型更可能完成该子任务？

它是一个有效的 routing condition。

官方 DashScope 文档确认 Qwen 系列可以关闭 thinking，并支持通过 `max_completion_tokens` / `max_tokens` 等参数控制输出长度；对于 thinking 模式，完整输出长度的定义与 answer-only 的限制不同，因此 benchmark 必须把 generation protocol 固定进 manifest，而不能把不同模式的 token budget 混为一谈。citeturn121104search0turn121104search2

---

# 八、新增 P4096 不是为了找“更漂亮的数字”

建议增加第二个 protocol：

```text
P4096 · Capability-Sanity Profile

thinking = false
max_output = 4096
```

它只承担一个任务：

```text
检查模型排名是否稳定
```

不直接替换 P1536。

最终可以保留：

```text
P1536
→ Budget Track

P4096
→ Capability Sensitivity Track
```

如果以后需要更高的动态环境 fidelity，再根据任务长度决定 adaptive budget，而不是现在就引入第三种 protocol。

---

# 九、下一步必须做：S12.5.4 Protocol Stability Probe

## 9.1 规模

不要做 175 节点。

只做：

```text
6 semantic nodes
×
3 models
×
2 output profiles
=
36 cells
```

节点来自现有 3 个 probe workflow，确保 role diversity：

```text
calculation
extraction
reasoning
search
verification
synthesis / transformation
```

不需要再扩 dataset。

---

## 9.2 Profile A

```text
P1536
thinking=false
max_output=1536
```

与当前已测结果同协议。

---

## 9.3 Profile B

```text
P4096
thinking=false
max_output=4096
```

推荐原因：先保持简单、可解释，只改变一个主要变量：

```text
output budget
```

不要同时改变：

```text
prompt
model
judge
sampling
```

否则无法解释差异。

---

# 十、Protocol Stability Gate

S12.5.4 输出：

```text
truncation_rate
mean_score
headroom
pairwise_winner
winner_agreement
rank_correlation
```

尤其计算：

```text
Weak vs Mid
Weak vs Strong
Mid vs Strong
```

建议使用 Spearman rank correlation 或简单 pairwise winner agreement，不需要引入复杂统计。

## GO

满足大致：

```text
winner/rank agreement ≥ 0.70
```

且：

```text
P4096 truncation 显著下降
```

则：

```text
P1536
= headline Budget Track

P4096
= sensitivity / capability diagnostic
```

可以进入 S12.6。

## REVIEW

如果：

```text
0.40 ≤ agreement < 0.70
```

则说明：

> generation budget 是一个实质性 routing variable。

此时 headline 必须同时报告两个 profile，不能只选一个。

## STOP / REVISE

如果：

```text
agreement < 0.40
```

或者：

```text
大量 winner inversion
```

则暂停 Full Harvest，重新设计 generation protocol。

这些数字是 operational thresholds，不是理论定理；重要的是提前把 decision rule 冻结，而不是看完结果后再定门槛。

---

# 十一、S12.5.4 还要顺手检查一个更基础的问题：模型输出是否真的“完成任务”

当前 judge 使用 0.5 / 0.8 / 1.0 三档，已经能产生区分。

但对于 semantic subtask，评价最好进一步采用：

```text
Task Completion
+
Required Information
+
Constraint Satisfaction
+
Format Validity
```

优先级：

```text
Deterministic evaluator
>
Exact/reference evaluator
>
LLM judge
```

原因：

> routing 关心的是“这个模型够不够完成这个 node”，而不是“哪个模型写得最漂亮”。

例如：

```text
任务：提取三个数字
```

返回三个正确数字的短答案，应该优于一大段漂亮但漏掉一个数字的说明。

因此 S12.6 应把：

```text
completion_score
```

作为 principal node outcome；

```text
general_quality
```

作为 secondary / diagnostic。

---

# 十二、Judge 本身必须进入冻结 identity

当前已经有：

```text
judge_rubric_v2
qwen-turbo
```

后续 manifest 必须固定：

```yaml
judge:
  model: qwen-turbo
  rubric_id: v2
  rubric_hash: ...
  temperature: 0
```

同时：

```text
judge_cost
model_cost
```

必须分开统计。

不能出现：

```text
system cost
```

里面包含 judge 成本，但 headline 却把它写成纯 model inference cost。

---

# 十三、S12.6 不要一次性采满 175×3

即使 Protocol Stability Gate 通过，也建议保留分阶段采集。

当前：

```text
175 semantic nodes
```

应该分为：

```text
33 L1 independent
142 L2 artifact-dependent
```

因此第一轮只做：

```text
L1
33 × 3 models
```

原因：

1. 这是严格 L1 counterfactual-capable 部分；
2. 不依赖上游 artifact；
3. outcome 可以直接绑定；
4. 最适合作为 v0.2 routing headline。

然后：

```text
L2
先做 30 nodes
```

如果 artifact-conditioned routing 确实有额外信息，再扩到 142。

这会显著减少沉没成本。

---

# 十四、S12.6 采集前增加 Provider → L1 → L2 三道小 Gate

```text
Provider Probe
      ↓
L1 Protocol Probe
      ↓
L1 Full Harvest
      ↓
L2 Conditional Probe
      ↓
L2 Full Harvest
```

不能反过来一口气跑满。

---

# 十五、System Pool Audit 应采用“可路由性”而不是“平均分”作为核心

池评估至少看：

```text
quality_headroom
cost_leverage
cheap_safe_rate
weak_oracle_share
mid_oracle_share
winner_entropy
multi_model_eligible_rate
cross_workflow_gain
```

特别是：

```text
mid_eligible
mid_oracle_share
mid_selected
```

必须单独看。

当前：

```text
mid eligible = 45.8%
mid oracle share = 18.2%
mid selected = 0%
```

这已经证明：

> “mid 没有被选中”不等于“mid 没有价值”。

应该进一步拆成：

```text
Mid is ineligible
vs
Mid is eligible but rejected
```

第二类才真正指向 routing policy。

近期 LLMRouterBench 研究也强调 careful model curation、模型互补性和 latency/cost trade-off，而不是单纯扩大模型集合；这支持当前“先做 Pool Audit，再决定 full matrix”的路线。citeturn121104academia14

---

# 十六、Pooling 之后不要立刻写死 R7

R7 只应被记录为：

```text
RouterBench-derived candidate pool
```

SystemBench v0.2 最终应生成自己的：

```text
system_pool_v01
```

因为：

```text
RouterBench query distribution
≠
Semantic Workflow node distribution
```

这也是当前 system-level benchmark 独立存在的意义。

---

# 十七、当前 Coordination Tax 应继续保留，但改变展示方式

当前：

```text
Coordination Tax = 18.35
```

更推荐最终报告：

```text
Coordination Cost Ratio = 19.35×
Coordination Overhead = +1835%
```

同时保留绝对成本。

原因：18.35 单独展示很容易被读成“18.35%”，实际不是。

还必须继续遵守：

```text
cost = 0
→ unknown
→ 不可当 free
```

这条规则已经在 S10 确立，继续冻结。

---

# 十八、O3 Joint Oracle 应全量 exact ≤ 8 nodes

当前最大 workflow：8 nodes。

三模型：

```text
3^8 = 6561
```

完全可枚举。

因此：

```text
O3
≤ 8 nodes
→ exact assignment enumeration
```

每个 assignment 再计算：

```text
model cost
model quality
legal schedule
critical path
```

不要保留：

```text
>6 nodes = greedy
```

如果未来 workflow >8 nodes，再重新讨论 approximate oracle。

这样 SystemBench v0.2 的：

```text
Joint Oracle
```

才是真正的 joint upper bound，而不是 approximate upper bound。

---

# 十九、Recovery 不需要阻塞 S12，但应在 S14 前扩大

当前 recovery：

```text
10 cases
0% → 0%
```

能证明：

```text
state machine works
```

不能证明：

```text
recovery improves success
```

所以：

```text
S12
→ keep as infrastructure

S14
→ 30–40 failure cases
```

并划分：

```text
recoverable
partial-recoverable
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

---

# 二十、S12 的 semantic case 不应只以“数量达标”为 freeze 条件

v0.2 candidate freeze 应满足：

```text
Semantic workflows ≥ 20
Headline nodes human-approved = 100%
Artifact contracts = 100%
Dependency semantics = 100%
Routing input hash = 100%
Execution input hash = 100%
L1 outcome coverage = 100%
Control sample no-overlap = 100%
```

同时：

```text
multi_model_eligible_rate
```

不能被极少数 case 垄断。

建议输出：

```text
case-level eligible rate distribution
```

而非只给全局平均。

---

# 二十一、S12.3 的 atomicity 发现要保留为研究结果

目前：

```text
22 / 126 = 17.5%
```

planner 输出节点实际上是复合职责。

建议正式记录：

```text
Composite Node Rate
= composite nodes / raw planner nodes
```

以及：

```text
Atomicity Rate
= atomic nodes / raw planner nodes
```

这是未来比较 planner 的有价值指标，不要把它从报告里删掉。

---

# 二十二、必须把“Production Bug”和“Benchmark Decision”分离

当前发现：

```text
qwen3.6
+ thinking default
+ max_tokens / output budget 处理不当
```

造成：

```text
截断
+
高 latency
```

这一发现非常值得进入 production backlog。

但是：

```text
Production Fix
```

不能直接修改：

```text
SystemBench v0.2
```

正确做法：

```text
Issue P-01
ai_client generation config

Issue B-01
SystemBench generation profile
```

两条独立提交、独立测试、独立 identity。

否则会出现：

```text
production bug fix
→ benchmark behavior changed
→ historical benchmark 不可复现
```

这是必须避免的。

---

# 二十三、当前不是开发“新内核机制”的时候

需要明确区分两个阶段。

## 阶段 A：SystemBench 完善

当前就是这里。

任务是：

```text
定义任务
定义数据
定义 outcome
定义 oracle
定义 experiment
定义统计
定义 replay
```

本阶段不应该修改：

```text
router core
orchestrator behavior
policy architecture
```

除非是为了增加：

```text
pure function
trait
observability seam
```

且不改变生产行为。

---

## 阶段 B：新内核机制开发

SystemBench v0.2 / L2 至少有一版稳定结果以后，才进入。

最值得研究的新机制不是“再做一个 router”，而是：

```text
State-aware Routing
```

即：

```text
Current Node
+
Visible Context
+
Previous Artifacts
+
Workflow Position
+
Remaining Budget
+
Failure State
        ↓
Model Selection
```

这是从当前 SystemBench 实验自然推出来的新机制方向。

近期 AgentRouter 已明确把 multi-step agent trajectory 的 step-level model assignment 作为研究问题；TwinRouterBench 则把 router-visible prefix、offline static routing 与 dynamic execution 分离。citeturn121104academia12turn121104academia13

因此未来新内核可以自然定义为：

```text
LLooM State-aware Router v1
```

但现在不要提前做。

---

# 二十四、S13 L2 的正确定位

S13 不是“再找一个 benchmark”。

它是：

```text
从 node outcome
→
artifact-aware outcome
→
state-aware outcome
```

推荐拆三段：

```text
S13.0 Source Audit
S13.1 Artifact Replay
S13.2 Counterfactual Validity
```

---

# 二十五、Historical Replay 的硬性定义

只有同时存在：

```text
original task
+
original trajectory
+
original artifact
```

才能进入 L2 artifact replay。

只有存在：

```text
multiple model outcomes
```

才允许做：

```text
routing counterfactual
```

绝对禁止：

```text
historical trajectory
→
假设另一个 model 会产生什么
```

然后把它当真实 counterfactual。

---

# 二十六、Counterfactual Validity 应成为 S13 Gate

每次 counterfactual 至少记录：

```text
original_model
counterfactual_model
original_artifact_hash
counterfactual_artifact_hash
downstream_input_hash
state_hash
```

必须满足：

```text
model changed
↓
artifact changed / validated equivalent
↓
downstream input correctly updated
↓
state propagation valid
```

否则只能标成：

```text
diagnostic / invalid counterfactual
```

不能进入 headline。

---

# 二十七、SystemBench v0.2 最终主结果应该回答什么

不要回答：

> LLooM 的 SystemScore 是多少？

应该回答五个独立问题：

### RQ1 · Routing

```text
固定 workflow 下
LLooM routing 是否降低 cost？
```

### RQ2 · Scheduling

```text
parallel / wave planning 是否降低 makespan？
```

### RQ3 · Orchestration

```text
decomposition 是否比 single-model 更有价值？
```

### RQ4 · Interaction

```text
routing + orchestration
是否产生额外协同收益？
```

### RQ5 · Recovery

```text
recovery 是否提升最终 goal success？
```

---

# 二十八、固定 Baseline Matrix

| Baseline | Workflow | Routing | Schedule | Recovery |
|---|---|---|---|---|
| B0 Flat-Strong | Flat | Strong | — | Off |
| B1 Flat-BestSingle | Flat | BestSingle | — | Off |
| B2 Fixed-Strong-Seq | Fixed | Strong | Sequential | Off |
| B3 Fixed-Strong-Par | Fixed | Strong | Parallel | Off |
| B4 Fixed-LLooMRouter | Fixed | LLooM | Parallel | Off |
| B5 LLooMWorkflow-Strong | LLooM | Strong | LLooM | Off |
| B6 Full-LLooM | LLooM | LLooM | LLooM | Off |
| B7 Full-LLooM-Recovery | LLooM | LLooM | LLooM | On |
| O1 Node Oracle | Fixed | Node Oracle | Reference | Off |
| O2 Schedule Oracle | Fixed | Fixed | Oracle | Off |
| O3 Joint Oracle | Fixed | Joint | Oracle | Off |

核心差分：

```text
B2 → B3 = Scheduling Gain
B3 → B4 = Routing Gain
B3 → B5 = Orchestration Gain
B4 → B6 = Routing × Orchestration Interaction
B6 → B7 = Recovery Gain
```

定义：

```text
Interaction Gain
=
Full Gain
− Routing Gain
− Orchestration Gain
```

这是 SystemBench 应该拥有的核心独有指标之一。

---

# 二十九、System-level headline 指标

## Primary

```text
Goal Success
Final Goal Quality
Cost / Successful Workflow
Makespan / Successful Workflow
```

## Secondary

```text
Node Completion
Strong Share
Routing Regret
Recovery Rate
```

## Diagnostic

```text
Planning Validity
Atomicity Rate
Dependency P/R
Parallel Width
Sequentialization Waste
Coordination Cost Ratio
Context Compression
Failure Attribution
```

不要把：

```text
Q_node
```

直接叫做：

```text
Final Quality
```

---

# 三十、统计规范

SystemBench 所有 policy 使用同一 case set。

bootstrap unit：

```text
system_case
```

而不是：

```text
node
```

使用：

```text
workflow-group paired bootstrap
B = 10,000
seed = 3407
```

headline 至少报告：

```text
ΔGoalSuccess + CI95
ΔQuality + CI95
ΔCost + CI95
ΔLatency + CI95
ΔStrongShare + CI95
```

对于 L1：

```text
paired node outcome
```

可以作为 diagnostic，但最终 workflow-level inference 仍以 SystemCase 为重采样单位。

---

# 三十一、数据 Identity 必须新增 outcome protocol hash

v0.2 manifest 至少冻结：

```text
systembench_id
workflow_schema_hash
case_hash
routing_policy_hash
model_pool_hash
split_hash
scheduler_hash
failure_policy_hash
evaluator_hash
judge_rubric_hash
generation_profile_hash
outcome_protocol_hash
cost_basis_hash
```

其中：

```text
thinking mode
max output
temperature
system prompt
user prompt template
structured-output setting
retry behavior
```

都应该进入 generation/outcome identity。

因为现在已经明确证明：generation protocol 可以影响 model ranking。

---

# 三十二、版本边界

## SystemBench v0.1

```text
Frozen Control Composition
```

不改。

## SystemBench v0.2

```text
Semantic Workflow Composition
```

主要增加：

```text
semantic gold
semantic node outcomes
input contracts
exact joint oracle
```

## SystemBench v0.3

```text
Artifact / Historical Replay
```

主要增加：

```text
real artifacts
trajectory
counterfactual
```

## SystemBench v0.4

```text
Live External
```

主要增加：

```text
real models
real tools
real state
real latency
real spend
```

不要因为已经开始 S13 就立刻把版本号升到 v1。

---

# 三十三、执行路线重新定稿

## S12.5.4 · Protocol Stability Probe

```text
6 nodes
× 3 models
× 2 profiles
```

目标：

```text
检查 ranking stability
```

不是：

```text
刷新 benchmark headline
```

---

## S12.5.5 · Pool Re-audit

如果 protocol stable：

```text
重新计算
weak / mid / strong
```

并加入：

```text
per-workflow gain
mid eligibility
```

---

## S12.6-A · L1 Full Harvest

只采：

```text
33 independent
×
3 models
```

得到第一版 semantic routing matrix。

---

## S12.6-B · L2 Conditional Probe

先：

```text
30 artifact-dependent nodes
```

验证：

```text
artifact 是否真正改变 winner
```

有效才继续扩。

---

## S12.7 · v0.2 Freeze

冻结：

```text
semantic case set
routing input
execution input
generation profile
model pool
judge
cost
oracle
split
```

并生成：

```text
systembench-v0.2
```

---

# 三十四、S13 · L2 Historical / Artifact Replay

优先顺序：

```text
LLooM own traces
↓
τ² historical
↓
ToolSandbox traces
↓
AppWorld
↓
WebArena
```

第一阶段只做：

```text
source audit
schema normalization
artifact validity
```

再决定 replay。

---

# 三十五、S14 · SystemBench Main Result

第一版正式主结果建议：

```text
30–50 semantic cases
+
10–30 L2 cases
```

输出：

```text
Baseline Matrix
Oracle Matrix
Cost Frontier
Latency Frontier
Strong Budget Frontier
Interaction Gain
Failure Attribution
```

---

# 三十六、S15 · Live

只在 S14 成立以后进行：

```text
10–30 sanity
↓
50–100 held-out
↓
external benchmark
```

优先：

```text
τ²
ToolSandbox
AppWorld
```

最后才考虑：

```text
WebArena
```

---

# 三十七、当前需要避免的事情

```text
1. 不直接 Full Harvest 175×3
2. 不因为 strong 排名最低就立即否定 pool
3. 不因为 truncation 就直接废掉 P1536
4. 不调 threshold 去“修” PlanningBench
5. 不让 fuzzy matching 进入 headline
6. 不把 artifact-dependent 当 L1
7. 不把 Q_node 当 Final Quality
8. 不把 historical trajectory 当 counterfactual
9. 不把 production bug fix 混入 benchmark identity
10. 不在 v0.2 未冻结前开发新 routing kernel
```

---

# 三十八、当前真正的研究机会

从现有数据看，SystemBench 已经把下一代机制问题暴露出来了：

```text
Context
    ↓
Complexity Gate
    ↓
Eligible Models
    ↓
Model Selection
    ↓
Output Budget
    ↓
Artifact
    ↓
Next Step
```

当前 router 主要还是：

```text
prompt
→
model
```

未来值得研究：

```text
workflow state
→
model
```

进一步：

```text
workflow state
+
remaining budget
+
artifact context
+
failure state
→
model
```

这才是新内核机制的自然起点。

---

# 三十九、什么时候正式转入“新内核机制开发”

必须满足：

```text
SystemBench v0.2 frozen
+
Semantic Outcome Matrix stable
+
L1 baseline complete
+
至少一组 L2 artifact evidence
+
Router / Orchestration interaction 已有主结果
```

之后再创建独立研究分支：

```text
research/state-aware-router
```

不要把新机制直接写进当前 production `router.rs`。

建议先以 benchmark-only prototype 开始：

```text
state-aware router prototype
↓
offline replay
↓
compare against frozen P2
↓
Shadow
↓
Live
```

只有证据成立后才进入 production。

---

# 四十、推荐最终研究叙事

## 第一层：RouterBench

> **不是所有 query 都需要最强模型。**

已经由 RouterBench v0.2 提供 frozen evidence。

## 第二层：Workflow Track

> **不是所有复杂任务都应该由一个模型一次完成。**

提供 planning / scheduling / recovery 的独立诊断。

## 第三层：SystemBench

> **真正困难的不是单独选择模型或单独拆分任务，而是让模型选择、任务结构、依赖、预算与执行状态共同决定下一步 inference。**

这就是 SystemBench 的长期研究空间。

---

# 四十一、最终决策表

| 项目 | 决策 |
|---|---|
| S12.5 Pool Audit | **Conditional GO** |
| Full Harvest | **暂缓一个小 Probe** |
| P1536 | 保留为 Budget Track |
| P4096 | 新增 Stability Diagnostic |
| Routing Input | R1 description headline / R2 full prompt diagnostic |
| L1/L2 | 严格分层 |
| L1 Full Harvest | 33 nodes × 3 models，Protocol Gate 后执行 |
| L2 | 先 30-node probe，再决定放量 |
| Pool | R7 仅 candidate，不直接冻结 |
| O3 | ≤8 nodes 全量 exact |
| Recovery | S14 前扩容，不阻塞 S12 |
| PlanningBench tau | 保持，不再调整 |
| Production Qwen config | 独立 issue，不混入 benchmark |
| Router core | 当前不改机制 |
| 新 routing kernel | S14 后再开研究分支 |
| Live benchmark | S14 后 |

---

# 四十二、当前直接执行命令序列

```text
S12.5.4
Protocol Stability Probe
        ↓
S12.5.5
Pool Re-audit
        ↓
[Gate]
Protocol stable?
        ↓ yes
S12.6-A
33 L1 nodes × 3 models
        ↓
S12.6-B
30 L2 nodes × 3 models
        ↓
S12.7
v0.2 Freeze
        ↓
S13
Historical / Artifact Replay
        ↓
S14
Main Result
        ↓
S15
Live
```

---

# 四十三、完成标准

本阶段真正完成，不是“175 个节点都采完”。

而是：

```text
1. generation protocol 对 model ranking 的影响被验证
2. SystemBench headline 的 input / execution contract 被冻结
3. semantic L1 outcome matrix 可复现
4. model pool 在 semantic node 上具有稳定互补性
5. oracle 与 baseline 定义清楚
6. workflow-level paired CI 可复现
7. L1 与 L2 不混算
8. v0.2 manifest 可独立复现
```

如果这八项都成立，则 SystemBench v0.2 才真正完成。

---

# 四十四、最终状态定义

截至本报告：

```text
SystemBench v0.1
    ✅ Frozen

S12 Semantic Workflow
    ✅ structure
    ✅ annotation
    ✅ dependency repair
    ✅ routing contract
    ✅ provider probe
    ⚠ outcome protocol stability
    ⚠ semantic matrix

下一步
    ↓
Protocol Stability Probe
    ↓
L1 Matrix
    ↓
L2 Matrix
    ↓
v0.2 Freeze
```

**因此当前 LLooM 项目阶段的正确描述是：**

> **SystemBench 评测体系完善与语义化阶段。**

而不是：

> **新内核路由机制开发阶段。**

新内核机制的研究入口已经被 SystemBench 提前暴露出来了，但现在应该把它当成**下一阶段的研究 backlog**，而不是当前执行主线。

---

# 四十五、参考资料

- LLMRouterBench：<https://arxiv.org/abs/2601.07206>
- AgentRouter：<https://arxiv.org/abs/2609.22951>
- TwinRouterBench：<https://arxiv.org/abs/2605.18859>
- LLMRouter：<https://arxiv.org/abs/2608.06867>
- DashScope Qwen API Reference：<https://www.alibabacloud.com/help/en/model-studio/qwen-api-via-dashscope>
- DashScope Qwen Responses API：<https://www.alibabacloud.com/help/en/model-studio/qwen-api-via-openai-responses>

