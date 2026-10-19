# 16 · LLooM RouterBench + OrchestrationBench 联合实施计划 v3

> 日期：2026-10-06  
> 基线：RouterBench Day 6–8 完成态 / LLooM v2  / OrchestrationBench v0.1 设计态  
> 定位：**RouterBench 继续作为当前研究主线；OrchestrationBench 并行开工，但仅推进零成本、可复现、不会改变 RouterBench 关键路径的基础层。**  
> 上游：`13-LLooM-RouterBench-下一阶段实施计划-v2` / `15-RouterBench实施总结-Day6-8` / `14-LLooM-OrchestrationBench-方案` / `15-LLooM-SystemBench-设计方案`

---

# 一、结论先行

当前工作已经不处于“要不要继续做 RouterBench”的阶段，而是进入：

```text
RouterBench
Weak Rescue → Model Pool / Query Suitability → Freeze
```

同时，OrchestrationBench 已经具备独立开工的理论与工程入口：

```text
OrchestrationBench
Schema → Planning → Scheduling → Failure Injection
```

因此不建议二选一，也不建议把两个项目串成：

```text
RouterBench 全部做完
        ↓
OrchestrationBench 才开始
```

更合理的是：

```text
                    LLooM Benchmark Program
                             │
                ┌────────────┴────────────┐
                │                         │
          RouterBench                 OrchestrationBench
          主研究车道                    并行基础车道
          ≈ 70%                         ≈ 30%
                │                         │
                │                         ├─ Canonical Workflow
                │                         ├─ Planning evaluator
                │                         ├─ Scheduling simulator
                │                         └─ Failure injection
                │
                ├─ R4-R7 Pool Tournament
                ├─ Query-level feasibility
                ├─ Frozen Test
                ├─ Matched Random
                ├─ Budget / CI
                └─ Cross-benchmark
                          │
                          └──────────────┬──────────────┘
                                         ↓
                                  SystemBench Shadow
```

核心原则：

> **RouterBench 解决“LLooM 是否真的会选对模型”；OrchestrationBench 解决“LLooM 是否真的会组织任务”；两者可以同时建设，但不要同时把两个系统都推入昂贵的在线推理。**

---

# 二、先检查当前工作状况：Day 6–8 实际完成了什么

`15-RouterBench实施总结-Day6-8` 已明确记录 Day 6–8 完结并推送，下一步原计划是 Day 9–10 的 R4–R7 replay。fileciteturn9file0L3-L6

当前已经完成：

```text
LLMRouterBench 接入
↓
30 dataset_id canonical
↓
541,152 outcomes
↓
28 dataset Pool Search
↓
208 个候选池
↓
SoftGatePolicy
↓
BandOnly 消融
↓
Weak Diagnosis
↓
存储迁移 + 完整回归
```

尤其重要的是，Day 8 已经把 Weak 0% 的根因从“可能是 scoring 不好”收束成：

```text
V0 ≡ BandOnly
        ↓
Weak 0%
        ↓
blocked_weak = 434
        ↓
blocked = Oracle weak share
        ↓
100% 被 gate 挡住
```

而打开 Weak eligibility 后：

```text
weak share = 18%
weak recall = 0.237
weak precision = 0.572
quality = -1.7pp
cost = -1.2%
```

并且 penalty sweep 基本退化回 V0，说明继续微调 penalty / λ 不是主要杠杆。fileciteturn9file0L88-L96

同时，Pool Search 已发现两条明显不同的候选路线：经典小模型池的 `weak_oracle_share ≈ 0.70`，frontier 池约 `0.76`；这直接证明“换池”比继续调当前 Weak 更值得实验。fileciteturn9file0L63-L74

所以当前状态可以简化为：

> **RouterBench 的问题已经从“找 bug”转成“找正确的候选池 + 判断 query-level suitability”。**

---

# 三、对 13-v2 原计划的核心修正

13-v2 的方向仍然正确，但 Day 6–8 已经产生了新的实验事实，因此现在需要把执行顺序进一步收紧。原方案在 Day 9–10 仍计划 `R4-R7 × {V0, Soft Gate} × λ sweep`，如果新池效果不够再进入 kNN；达标后再 freeze、matched random、budget curve、bootstrap CI。fileciteturn10file6L1-L25

现在建议改成下面这条更严格的链：

```text
Calibration Tournament
        ↓
候选池 + policy 一起筛
        ↓
只保留少量 finalists
        ↓
必要时 calibration-only kNN
        ↓
Freeze
        ↓
Test 一次性 replay
        ↓
Matched Random / Budget / CI
        ↓
Cross-Benchmark
        ↓
Native Decision
```

而不是：

```text
Test
↓
看结果
↓
再改 pool / λ / kNN
↓
再 Test
```

后者会迅速把 test 变成 tuning 数据。

---

# 四、RouterBench：下一阶段真正的 P0

## R0 · 维护当前基线，不再修改

当前 R0 已经固定：

```text
Always Weak
Always Mid
Always Strong
Random
Best Single
Oracle
LLooM V0
BandOnly
SoftGate V2
```

已有 Core 1000 结果继续保留，不需要为了新实验反复重算或改口径。

原则：

> **基线是 reference，不是新的 tuning space。**

---

# 五、R1 · R4–R7 不再简单做“4 个池 × 全套 λ sweep”

## 5.1 正确做法：Calibration Tournament

R4–R7 继续保留，但它们的含义升级成四种**池型候选**，而不是四个随意手挑模型组合。

推荐：

```text
R4 = Weak Opportunity Champion
     weak_oracle_share 最大

R5 = Cost Leverage Champion
     cheap_safe / cost leverage 最大

R6 = Diversity Champion
     winner entropy / 跨题 winner 分布最好

R7 = Stability Champion
     多 dataset 上表现稳定，而不是只在单一 dataset 极优
```

其中 R7 最好新增一个 screening 指标：

```text
cross_dataset_stability
= median(normalized PoolScore across eligible datasets)
```

目的：防止选择一个只在单个 benchmark 具有漂亮 headroom 的池。

---

## 5.2 R4–R7 必须全部由 calibration 选出

允许：

```text
Pool Search → calibration
policy search → calibration
threshold search → calibration
kNN fitting → calibration
```

禁止：

```text
根据 test 哪个池最好
→ 再决定最终池
```

最终冻结记录必须包含：

```text
pool_id
weak_model
mid_model
strong_model
selection_reason
calibration_metric_snapshot
model_pool_hash
```

---

# 六、R2 · 新池实验不要再把“λ sweep”作为主问题

Day 8 已经证明当前实现的 λ 曲线只是有限几个 regime，且折中点的 saving 改善很小。fileciteturn9file0L90-L94

因此新池阶段应该从：

```text
“哪个 λ 最好？”
```

转为：

```text
“哪个 Pool × Policy Family 最值得继续？”
```

只保留三个 policy family：

```text
P0 · Current LLooM

P1 · BandOnly

P2 · Soft Eligibility
```

必要时再增加：

```text
P3 · Soft Eligibility + Query-level suitability
```

而不是对每个池做几十个 λ 点。

---

# 七、R3 · Soft Gate 的下一步不是继续调 penalty，而是验证“候选池是否救得回来”

当前 penalty sweep 已表现出明显的 all-or-nothing：penalty 稍微变大，Weak 再次完全被挤出去。fileciteturn9file0L92-L94

因此：

```text
penalty fine-tuning
```

降级为次要工作。

真正要观察的是：

```text
新池
↓
Weak eligibility 打开
↓
Weak Precision 是否提升
↓
Weak Recall 是否提升
↓
总质量是否保持
↓
成本是否明显下降
```

如果一个候选池在 `SoftGate(0)` 下已经明显优于当前池，那么继续研究 penalty 才有意义。

---

# 八、R4 · 增加一个比 weak_oracle_share 更重要的 screening 维度

目前：

```text
weak_oracle_share
```

是很好的“天花板”指标，但它只表示：

> Weak 在多少题上属于 pool 内逐题最优等价集合。

它不能说明：

> Weak 被选以后，会不会经常犯错。

因此下一阶段新增 calibration 指标：

## Weak Regret Potential

定义为：

```text
weak_regret(q)
= best_pool_score(q) - weak_score(q)
```

然后统计：

```text
mean weak regret
median weak regret
p90 weak regret
```

以及：

```text
P(weak_regret <= ε)
```

这里的 ε 必须与 dataset 的 score semantics 一致，不允许在不同数据集之间直接拿一个未经解释的统一数值当“安全线”。

意义：

```text
weak_oracle_share
= “Weak 有机会赢多少题”

weak_regret distribution
= “Weak 输的时候输得多惨”
```

这两个维度比单独追求 Weak share 更有研究价值。

---

# 九、R5 · kNN 现在应该是“条件触发”，不是固定 Day 10 任务

13-v2 把 TF-IDF / hashed n-gram + kNN 放在 Day 10。现在建议改成：

```text
先跑 R4–R7
        ↓
若 calibration 已经找到稳定高 precision 的 Weak
        ↓
不做 kNN
```

只有出现：

```text
Pool 有 headroom
但
SoftGate 仍无法区分“适合 Weak / 不适合 Weak”
```

才进入：

```text
TF-IDF / hashed n-gram
↓
Top-K cosine
↓
Weak suitability estimator
↓
threshold sweep
```

原因很简单：

> **如果换池已经解决主要问题，就没有必要为了“更复杂”而加一个 query-level router。**

---

# 十、kNN 的验收方式也需要改

不能只看：

```text
Weak share ↑
```

必须看：

```text
Weak Precision ↑
Weak Recall ↑
Weak Regret ↓
Quality Retention ≥ baseline guardrail
Cost Saving ↑
```

另外增加：

```text
Strong Escalation Recall
```

即：

> 真正需要强模型的 query，有多少最终仍然被送到强模型。

这能防止 router 为了省钱把 Strong 过度挤掉。

---

# 十一、RouterBench 的真正主目标：从“选模型率”改成“成本-质量前沿”

当前 headline 不能只写：

```text
Weak = 18%
```

也不能只写：

```text
Cost Saving = XX%
```

建议正式报告统一展示：

```text
Quality
Cost
Quality Retention
Cost Saving
Weak Recall
Weak Precision
Strong Escalation Recall
```

然后画：

```text
Quality vs Cost Pareto Frontier
```

并区分：

```text
Deployable Frontier
Oracle Upper Bound
```

不要把 Oracle 与真实 router 画在同一个“可部署前沿”图例里。

---

# 十二、R6 · Matched Random 要改成“冻结后”基线

原方案保留：

```text
Cost-matched Random
Strong-share-matched Random
```

这一点正确，但顺序需要调整：

```text
Calibration
↓
Freeze Policy
↓
Test
↓
Matched Random
```

而不是边调 router 边调 random baseline。

推荐至少有：

### Cost-matched Random

随机 router 的期望成本与 LLooM test policy 接近。

### Strong-share-matched Random

随机 router 使用与 LLooM 相同的 Strong 选择比例。

这样可以回答：

> LLooM 的收益究竟来自“恰好用了这么多便宜模型”，还是来自“真的选得更准”。

---

# 十三、R7 · Budget Curve 取代 Lambda Curve 成为主图

lambda sweep 保留作实现诊断，但正式 headline 改成：

```text
Budget / Cost Constraint
        ↓
Best attainable quality
```

展示：

```text
预算 100%
预算 80%
预算 60%
预算 40%
预算 20%
```

在每个成本约束下比较：

```text
Always Strong
Best Single
Random
LLooM
Oracle
```

这比：

```text
λ=0.1 / 0.2 / 0.3 / ...
```

更接近真正 routing system 的工程价值。

---

# 十四、R8 · Freeze 的标准进一步收紧

Best Variant 必须在 calibration 上先确定：

```text
Best Pool
Best Policy Family
Best SoftGate threshold
Best kNN threshold（若启用）
```

然后生成一个新的：

```text
benchmark_id
manifest_hash
policy_hash
pool_hash
split_hash
scorer_hash
cost_hash
```

之后：

```text
TEST = read-only
```

不再修改：

```text
pool
threshold
λ
kNN
quality prior
```

除非开启全新的 benchmark version。

---

# 十五、Cross-Benchmark 不再允许“两个 benchmark 各自调一套”

原方案要求 RouterBench + LLMRouterBench 复现，但这里再加一条：

> **必须把“router policy”与“source adapter”分开。**

允许 source adapter 各自提供：

```text
model availability
source cost
score normalization
benchmark-specific metadata
```

但 routing policy 的结构参数必须统一冻结。

理想结构：

```text
                  Common Router Policy
                         │
              ┌──────────┴──────────┐
              │                     │
       RouterBench Adapter   LLMRouterBench Adapter
              │                     │
          models/cost             models/cost
```

而不是：

```text
RouterBench → 单独 tuning
LLMRouterBench → 再单独 tuning
```

否则所谓 cross-benchmark validation 其实只是两个独立 benchmark 的 tuning。

---

# 十六、RouterBench 推荐的新排期

## Day 9 · Calibration Pool Tournament

```text
208 pool
↓
候选过滤
↓
R4-R7 四种池型
↓
Current / BandOnly / SoftGate
↓
calibration 全量比较
```

产物：

```text
pool_tournament_calibration.json
pool_tournament_calibration.csv
```

---

## Day 10 · Conditional Query Router

仅在 Day 9 仍无法获得稳定 Weak Precision 时启动：

```text
TF-IDF / hashed n-gram
↓
kNN
↓
Weak suitability
↓
threshold sweep
```

产物：

```text
knn_calibration.json
weak_precision_recall_curve.json
```

---

## Day 11 · Freeze

冻结：

```text
Best Pool
Best Policy
Best Threshold
```

生成：

```text
benchmark_id v0.2
manifest
policy snapshot
```

---

## Day 12 · Test + Matched Random

只读 Test：

```text
RouterBench
LLMRouterBench
```

并生成：

```text
Cost-matched Random
Strong-share-matched Random
Budget Curve
```

---

## Day 13 · Statistics

```text
Bootstrap 95% CI
per-query paired deltas
confidence intervals for cost saving
confidence intervals for quality retention
```

AIQ 若需要，重新定义为正式可复现版本；旧的 0.7036 只保留为 historical diagnostic，不作为最终 headline。当前 Day 1–5 已明确其为简化三点实现。fileciteturn9file0L121-L126

---

## Day 14 · Cross-Benchmark Final Validation

```text
Frozen Policy
↓
RouterBench Test
LLMRouterBench Test
```

禁止在 Test 上继续改参数。

---

## Day 15 · Native Decision

只有满足以下条件才值得进入 Native：

```text
1. 至少一个新池在 frozen test 上明显优于 V0
2. quality retention 达到 guardrail
3. cost saving 有实质改善
4. matched random 被超过
5. 至少一个结果在第二数据源方向一致
6. bootstrap CI 不与“无收益”结论严重冲突
```

否则继续零成本 routing research。

---

# 十七、OrchestrationBench：答案是“要并行，但只并行到正确的深度”

## 17.1 为什么现在就要开始

OrchestrationBench 不是 RouterBench 的后续附属功能，而是另一条能力轴：

```text
RouterBench
query → model

OrchestrationBench
task → workflow

SystemBench
task → workflow → models → execution → result
```

已有 OrchestrationBench 方案已经把 canonical workflow、Planning / Scheduling / Execution 分层，并明确建议先走 Offline / Replay Track，再进入 Online / Execution Track。fileciteturn10file0L1-L35

因此现在开工最合适。

但不应该现在就做：

```text
大规模 APB live
大规模 AppWorld
WebArena
τ² live
大量 LLM Judge
```

因为这些会把成本、环境稳定性和 benchmark engineering 同时带进来，反而干扰 RouterBench 主线。

---

# 十八、OrchestrationBench 并行车道的明确范围

## O1 · Canonical Workflow Schema

先统一：

```text
Workflow
├── root_goal
├── nodes
├── edges
├── waves
├── model_assignment
├── execution_trace
└── aggregation
```

输出：

```text
workflow.schema.json
validator
fixture examples
```

这一项与 RouterBench 完全不冲突，可以立即做。

---

## O2 · Planning Track

优先接：

```text
PlanningBench
PlanBench
```

第一阶段只做：

```text
dataset adapter
reference plan parser
deterministic evaluator
```

不要求真实 LLM generation。

所以仍然是零 API 成本。

---

## O3 · Scheduling Track

优先构建 deterministic scheduling simulator：

```text
DAG
↓
legal schedule
↓
wave count
↓
makespan
↓
parallel utilization
```

TPS-Bench 作为主要外部参考。

先验证 scheduler，而不是马上验证“模型会不会自己设计最优 schedule”。

---

## O4 · Router Bridge

把 RouterBench 已经成熟的：

```text
subtask → model
```

接入 workflow node：

```text
workflow node
      ↓
subtask query
      ↓
RouterBench policy
      ↓
model assignment
```

这是以后 SystemBench 最重要的接口。

---

## O5 · Failure Injection

先做确定性的：

```text
timeout
model unavailable
malformed output
dependency missing
parallel branch failure
budget exhausted
```

每种 failure 都进入统一状态机：

```text
RUNNING
↓
FAILED
↓
RETRY / FALLBACK / REPLAN / ABORT
↓
RECOVERED / PARTIAL / FAILED
```

仍然不需要 LLM API。

---

# 十九、OrchestrationBench 现在“不做”的东西

本阶段明确暂缓：

```text
APB full online
τ² live
ToolSandbox live
AppWorld full
WebArena full
大型 LLM judge
真实多 agent cost study
```

这些全部后置到：

```text
OrchestrationBench Offline Gate 全绿
```

之后再选最值得的一项做小规模 online pilot。

---

# 二十、OrchestrationBench 的并行排期

## O-Day 9

```text
Workflow schema
validator
fixtures
```

## O-Day 10

```text
PlanningBench adapter
PlanBench adapter
planning evaluator
```

## O-Day 11

```text
DAG scheduler
wave simulator
scheduling metrics
```

## O-Day 12

```text
Router bridge
subtask model assignment schema
```

## O-Day 13

```text
Failure injection
recovery state machine
```

## O-Day 14

```text
Offline report
case-level attribution
WebUI skeleton / graph viewer
```

## O-Day 15+

只有 RouterBench 已完成 Freeze + CI 后，才决定：

```text
Planning online pilot
或
Scheduling online pilot
或
τ² historical trajectory analysis
```

不默认三者全部做。

---

# 二十一、两条车道之间如何共享资产

这是并行推进最重要的部分。

## 共享 1 · ModelOutcome

RouterBench 已有：

```text
query × model → score + cost
```

未来 OrchestrationBench 的 node 直接复用这个 outcome backend。

---

## 共享 2 · Cost Ledger

统一记录：

```text
model cost
router cost
workflow execution cost
judge cost
```

但 source cost 必须分轨，不得把 RouterBench 的 source cost 与 LLMRouterBench 的 source cost 混成一个绝对 headline。Day 6–8 已明确指出这点。fileciteturn9file0L121-L126

---

## 共享 3 · Hash / Manifest

两套 benchmark 最终都需要：

```text
dataset_hash
split_hash
model_pool_hash
policy_hash
scorer_hash
cost_hash
```

以后 SystemBench 只允许引用冻结对象。

---

## 共享 4 · Benchmark DB

建议逐步把：

```text
router_runs
orchestration_runs
workflow_items
```

统一成 benchmark schema，而不是两个孤立 SQLite 体系。

---

# 二十二、资源分配：为什么建议 70 / 30

当前 RouterBench 已经接近“能否形成第一条可信研究结论”的临界点，而 OrchestrationBench 还处于 benchmark infrastructure 阶段。

因此：

```text
RouterBench       70%
OrchestrationBench 30%
```

不是因为 Orchestration 不重要，而是因为：

```text
RouterBench
已经有真实实验数据 + baseline + failure evidence

OrchestrationBench
目前主要是 schema / evaluator / simulator 建设
```

这能避免：

```text
两个项目同时进入实验 tuning
```

导致注意力分散。

---

# 二十三、何时暂停 RouterBench，把资源切给 OrchestrationBench

只允许在 RouterBench 发生以下状态时临时切换资源：

```text
A. Best Pool 已冻结
B. Test 已跑完
C. Matched Random 已完成
D. Bootstrap CI 已完成
E. Cross-Benchmark 已完成
```

达到这五项后，RouterBench 从：

```text
active research
```

进入：

```text
maintenance / new-idea queue
```

此时可把：

```text
70 / 30
```

切换成：

```text
30 / 70
```

正式冲 OrchestrationBench。

---

# 二十四、什么时候真正进入 SystemBench

不要现在开始跑 SystemBench。

SystemBench 的正确位置是：

```text
RouterBench Freeze
        +
OrchestrationBench Offline Gate
        ↓
SystemBench Shadow
```

原因：

SystemBench 会同时包含：

```text
planning
routing
scheduling
execution
recovery
aggregation
```

如果 RouterBench 和 OrchestrationBench 本身还在改，SystemBench 会变成一个无法定位错误的大黑箱。

---

# 二十五、最终三层研究叙事

未来最终展示应收束成：

```text
第一层 · RouterBench

“不是每个 query 都需要最强模型。”
```

↓

```text
第二层 · OrchestrationBench

“不是每个 complex task 都应该由一个模型完成。”
```

↓

```text
第三层 · SystemBench

“复杂任务可以被组织成 workflow，
不同节点由不同模型完成，
并通过依赖、并行与恢复机制完成最终目标。”
```

这三层不要互相抢 headline。

RouterBench 给出最强的定量证据。

OrchestrationBench 给出最强的流程与故障归因证据。

SystemBench 最后把两者组合成系统级证据。

---

# 二十六、下一阶段总看板

## RouterBench · P0

```text
[x] LLMRouterBench canonical
[x] Dataset screening
[x] Model Pool Search
[x] Weak diagnosis
[x] Soft Gate

[ ] R4-R7 calibration tournament
[ ] stability score
[ ] weak regret potential
[ ] conditional kNN
[ ] freeze
[ ] frozen test
[ ] matched random
[ ] budget curve
[ ] bootstrap CI
[ ] cross-benchmark
[ ] Native decision
```

## OrchestrationBench · P0.5

```text
[ ] Workflow schema
[ ] validator
[ ] PlanningBench adapter
[ ] PlanBench adapter
[ ] scheduling simulator
[ ] Router bridge
[ ] failure injection
[ ] offline report
```

## OrchestrationBench · P1

```text
[ ] historical trajectory reuse
[ ] small online pilot
[ ] stateful execution benchmark
```

## SystemBench · P2

```text
[ ] Shadow
[ ] Frozen Replay
[ ] Live External
```

---

# 二十七、最终决策

### RouterBench

**继续做，而且下一步就是 R4–R7，但必须从“实验枚举”升级成“Calibration Tournament → Freeze → Test”的严格流程。**

最值得新增的两个东西不是更多 λ，而是：

```text
1. cross-dataset stable pool selection
2. weak regret / strong escalation safety
```

kNN 只有在换池后仍然出现明显的 query-level suitability 问题时才启动。

### OrchestrationBench

**现在就并行开始。**

但只做：

```text
schema
planning offline
scheduling offline
router bridge
failure injection
```

不要马上投入大量 API / Judge / browser / stateful environment。

### SystemBench

**现在不开始。**

等：

```text
RouterBench Freeze
+
OrchestrationBench Offline Gate
```

再进入 Shadow。

---

# 二十八、最终原则

> **先把 RouterBench 做成可信的 routing evidence，再同时把 OrchestrationBench 的骨架搭起来；不要为了并行而并行，更不要让两个 benchmark 同时进入昂贵的 live tuning。**

整个计划最终收束为：

```text
                    LLooM
                      │
          ┌───────────┴───────────┐
          │                       │
    RouterBench              OrchestrationBench
    “选哪个模型”              “怎么组织任务”
          │                       │
          └───────────┬───────────┘
                      ↓
                SystemBench
              “完整 inference loop”
```

当前真正的下一步不是增加更多 benchmark 名单，而是把已有证据变成一条干净的因果链：

```text
Current LLooM
    ↓
Weak 0%
    ↓
100% blocked
    ↓
Soft Gate proves eligibility is causal
    ↓
Pool Search finds high-headroom Weak candidates
    ↓
Calibration Tournament selects stable pool
    ↓
Conditional query router only if necessary
    ↓
Frozen Test
    ↓
Matched Random + Budget + CI
    ↓
Cross-Benchmark reproduction
    ↓
RouterBench research result

与此同时：

Workflow Schema
    ↓
Planning
    ↓
Scheduling
    ↓
Router Bridge
    ↓
Failure Injection
    ↓
OrchestrationBench Offline result

最后：

RouterBench
    +
OrchestrationBench
    ↓
SystemBench
```

这比单纯继续堆数据集、堆模型或堆参数 sweep 更接近一个真正可以对外解释、复现和继续扩展的研究路线。
