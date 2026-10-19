# 34 — LLooM Bench 工作总结核验、成果解读与后续行动

> 核验日期：2026-10-10  
> 输入：用户提供的《33-LLooM-Bench工作全景总结-成果与后续计划.md》；当前可访问的 `citrus-dot/LLooM` GitHub `v2` 分支文件、报告、manifest 与提交记录。  
> 目的：说明到目前为止 Bench 工作真正完成了什么、有哪些能展示的证据、哪些结论不能越界，以及当前最值得投入的下一步。  
> **重要口径：**“33 号总结记载的本地进展”与“当前远程 `v2` 分支可直接验证的内容”不是同一概念。本文会显式区分两者。

---

## 0. 先说结论

Bench 已经不是“刚搭框架、等着做实验”的阶段。按 33 号工作总结，项目已完成 RouterBench v0.2 冻结、OrchestrationBench v0.1 离线基础设施、SystemBench v0.1 Frozen Composition，以及 S12 语义工作流探索与 Showcase Freeze。它们组成了一条从 query 级路由、workflow 级评估，到系统组合分析的证据链。

但现在有两个必须正视的核验问题：

1. **一处关键数字写错了基线归属。** 当前仓库 `benchmarks/routerbench/reports/v02_test_report.md` 把 P0 Current / BandOnly 报为 Q=0.5867、成本 $0.000849；把真正的 P2 Frozen 报为 Q=0.5863、成本 $0.000821。33 号总结把前一组数填到了“LLooM P2 (frozen)”行。这需要先修正。
2. **远程仓库没有同步到 33 号总结声称的 Showcase Freeze。** 当前可访问的 GitHub `v2` 分支能看到 S12.2–S12.4、L1 v0.1 冻结报告等资产，但找不到 `benchmarks/showcase/`、`semantic_outcomes_l1.jsonl`、`semantic_outcomes_l2.jsonl`、S12.6 报告，也无法解析总结所写的 Showcase Freeze commit `746c3d7`。这不代表这些文件不在本机；它只表示它们目前不能从当前远程分支直接复现。

因此，**下一阶段不应该继续扩实验，而应先做一次短而严格的“仓库同步与数字校正”，然后整理一个别人能从 GitHub 打开、运行、复核的展示入口。** 在这些事项完成之前，不宜把“Showcase Freeze 已对外可复现”作为远程仓库现状来描述。

---

## 1. 工作全貌：三条 Track 各自解决什么问题

| Track | 核心问题 | 当前证据等级 | 应如何定位 |
|---|---|---|---|
| RouterBench v0.2 | 单个 query 应选哪个模型，能否在质量可接受时降低成本？ | **A：冻结主证据** | 当前最适合作为主展示的定量结果 |
| OrchestrationBench v0.1 | workflow 的 schema、依赖、规划、调度、失败恢复如何被确定性验证与归因？ | **基础设施 / 离线诊断** | 证明评测框架和诊断能力，不独立宣称真实端到端成功率 |
| SystemBench v0.1 | 把 routing、scheduling 放进同一 workflow 后，成本、完成代理指标和调度时间如何变化？ | **A：冻结的 composition/replay 证据** | 展示系统组合的效果与边界，不能冒充 live E2E |
| SystemBench S12 semantic snapshot | 当子任务更接近真实分解、上下文/输出预算变化时，候选模型与选择是否变化？ | **B：诊断探索；L2 为 C：partial** | 用于说明研究发现、实验协议和下一步假设，不做异质矩阵的 pooled ranking |
| Live / dynamic E2E | 真正执行 workflow 时，最终答案质量、墙钟延迟、真实花费有何变化？ | **D：尚未测** | 不得暗示当前已验证 |

三条 track 应分别报告，不应把它们拼成一个“总分”。核心价值是证据分层、输入与决策可追溯，而不是表格数量或 benchmark 名称数量。

---

## 2. RouterBench：目前最硬的定量成果

### 2.1 已经做了什么

- 导入和整理公开的 LLMRouterBench 数据与模型输出矩阵；原始大文件移至外置盘 `/Volumes/Files/LLooM-bench/`，仓内通过 symlink 引用，避免把数 GB 数据塞进 Git。
- 建立 Rust 侧的 benchmark 路由入口，核心是 `bench::route_single → router::plan()`；尽量复用生产决策真源，避免 Python benchmark 再复制一套路由公式。
- 建立 run、pool、policy、split、cost basis 和 manifest 的冻结身份；策略选择在 calibration split 内完成，冻结 test 只读。
- 完成 R7 池选择、P2 soft-gate 方案、baseline、cost-matched random、strong-share-matched random，以及实例级 paired bootstrap（B=10,000，seed=3407）。
- 做了 Weak Rescue / eligibility gate 诊断，并在不同模型代际上交叉检查机制边界。

### 2.2 冻结 test 应采用的结果

依据当前仓库的 [RouterBench v0.2 冻结报告](https://github.com/citrus-dot/LLooM/blob/v2/benchmarks/routerbench/reports/v02_test_report.md)：

| 策略 | Quality | Cost/query | 解释 |
|---|---:|---:|---|
| Always Strong | 0.5860 | $0.000948 | 固定强模型基线 |
| **P2 Frozen** | **0.5863** | **$0.000821** | 冻结的路由策略 |
| Best Single | **0.6053** | **$0.000674** | calibration 选出的最佳单模型；点估计支配 P2 |
| Cost-matched Random | 0.5610 | $0.000819 | 与 P2 成本近似匹配 |
| Strong-share-matched Random | 0.5749 | $0.000899 | 强模型占比近似匹配 |
| Oracle | 0.7122 | $0.000118 | 事后知道每题结果的不可部署上界 |

冻结 P2 的核心统计结果：

- 相对 Always Strong，成本节省 **13.46%**，95% CI **[12.20%, 14.73%]**。
- 质量保留率 **1.0005**，95% CI **[0.9957, 1.0052]**，区间包含 1.0；严谨表达应为“未观察到显著质量损失”，不应说“质量严格不变”。
- 相对 cost-matched random，质量差 **+2.52 个百分点**，95% CI **[+2.07, +2.97]**。
- 相对 strong-share-matched random，质量差 **+1.14 个百分点**，95% CI **[+0.75, +1.53]**。
- 冻结 test 的 weak precision 为 **0.786**，95% CI **[0.7629, 0.8086]**。

### 2.3 这组结果可以说什么、不能说什么

**可以说：**在选定的 R7 模型池、LLMRouterBench 冻结 test 和当前 cost/quality 定义下，P2 相比 Always Strong 降低了成本；在近似匹配成本或强模型使用占比的对照下，P2 取得更高质量。这支持“路由具有 content-aware 信号”，而不只是按固定比例抽模型。

**不能说：**P2 找到了最优成本/质量前沿，或者优于所有可部署策略。Best Single 的质量更高、成本更低，是必须同场展示的负面约束。Oracle 是不可部署的事后上界，也不能作为工程基线。

### 2.4 需要修正的数字问题

33 号总结第 2.2 节将 `Q=0.5867 / $0.000849` 写在“LLooM P2 (frozen)”一行。这组数字在当前仓库报告中对应 **P0 Current / BandOnly（`lloom` run）**；真正的 **P2 Frozen** 是 **Q=0.5863 / $0.000821**（`lloom@softgate0` run）。

这是策略身份与指标错配，不是四舍五入问题。所有展示图、摘要、对外文字都应以 frozen manifest 指向的策略和 `v02_test_report.md` 中的 P2 Frozen 行为准。也要避免把 calibration 阶段 weak precision 0.809 与 frozen test 的 0.786 混在一起。

### 2.5 值得展示的研究过程

- **Blocked Weak 诊断：**早期弱模型使用率为 0，并不是路由器“看过弱模型后认为它不好”，而是 eligibility/band gate 将它拦在候选集之外。该诊断把问题从调权重转向候选资格与模型池。
- **Soft Gate / R7 池结果：**在较适合弱模型的模型池中，允许部分弱模型参与后，弱模型使用变得可观察，并取得对成本/质量有意义的变化；但幅度受模型代际和任务分布影响。
- **跨 benchmark 的边界：**机制层面存在 gate 限制，但不同代际模型池的可路由收益不一致。RouterBench 2024 模型矩阵中模型能力差距过大，调权重难以“救活”弱模型；因此没有理由继续无止境扫描 λ 或强行宣称弱模型普遍可救。
- **E5 负结果：**静态 outcome 矩阵里 EWMA quality override 没带来增益。它更适合未来真实分布/能力漂移场景，而非用静态回放硬造结果。

这些发现能构成一条可信的工程研究叙事：先定位 gate 造成的结构性阻塞，再通过候选池与软门槛验证可行边界，最后明确机制不能跨模型代际无条件外推。

---

## 3. OrchestrationBench：基础设施已建成，主价值是“可诊断”

### 3.1 已建成的部分

当前仓库的 [OrchestrationBench README](https://github.com/citrus-dot/LLooM/blob/v2/benchmarks/orchestration/README.md) 可以核对主要组件：

- Canonical workflow schema 与确定性 validator：节点、依赖边、DAG、waves、非法并行等结构错误都有明确归因码。
- PlanningBench adapter：导入 467 个实例，并拆出 3,379 个 obligation 节点。
- PlanBench adapter：PDDL 解析与 1,901 个 workflow 记录；对 blocksworld 等依赖结构做确定性抽取。
- 确定性 planning evaluator：子任务匹配、冗余、依赖边 precision/recall、非法计划等指标。当前文本相似度阈值仍属于工程校准，不等价于真实在线任务成功率。
- 四类调度策略与模拟指标：reference/Kahn 分层、sequential、greedy ready queue、生产 waves 口径；计算 makespan、critical path、利用率、浪费与非法并行。
- F1–F7 failure injection 与确定性 recovery 状态机；P1–P11 case-level diagnosis。
- Router bridge 使用明确的 `obx__{workflow_id}__{node_id}` sample_id 契约，并把分派决策保留给 Rust 生产路由器，不在 Python 侧重写评分逻辑。
- 独立 `orchbench.db`、manifest hash、fixtures 与 Python 单元测试；仓库记录的 offline 质量门已完成。

### 3.2 可以展示的成果

- 给定 workflow JSON，展示 validator 如何发现 cycle、dangling dependency、bad waves 等错误，并返回可定位的归因码。
- 用 clean / missing node / illegal parallelism / fallback failure 等 fixture，演示报告怎样从终态追到 P1–P11 的原因链。
- 展示 scheduling 的关键路径与可并行宽度，比较“模拟的串行执行”和“模拟的并行执行”。
- 展示相同 workflow 输入可以确定性重复，且 benchmark 与生产波次逻辑之间有 parity 检查。

目前的 [offline demo 报告](https://github.com/citrus-dot/LLooM/blob/v2/benchmarks/orchestration/reports/offline_v0.1.md) 明确使用手工扰动 fixture；它是演示/基础设施证据，不是从真实在线预测计划得到的总体结果。

### 3.3 仍未验证的事

- OrchestrationBench v0.1 不是对真实 agent 的端到端评测；当前没有把所有规划、路由、执行、产物传递、聚合与真实成本串成一个 live headline。
- PlanningBench reference 的语义与真实 decomposition 子任务不完全同类，不能靠降低阈值把文本匹配变成有效的 gold decomposition。
- 在线工具环境、τ²、AppWorld、WebArena 等仍是明确后置项，不属于已完成成果。

因此，不应单独拿 OrchestrationBench 当前的 offline demo 声称“LLooM 的复杂任务规划成功率已被验证”。它的亮点是评测基础设施、确定性诊断与可复用的 workflow 数据模型。

---

## 4. SystemBench v0.1：已经形成可量化的系统组合结果

### 4.1 评测是怎样构成的

当前远程仓库有 [SystemBench v0.1 报告](https://github.com/citrus-dot/LLooM/blob/v2/benchmarks/systembench/reports/v01_report.md) 与 [冻结 manifest](https://github.com/citrus-dot/LLooM/blob/v2/benchmarks/systembench/manifests/frozen_manifest_v01.json)。冻结设置为：

- 80 个 workflow cases，其中 test 65 个；总计 327 个节点。
- 节点 prompt 绑定到真实 LLMRouterBench query，使用 exact binding 与 hash 校验，禁止 nearest/fuzzy 绑定进入 headline。
- 节点质量与成本来自 frozen outcome replay，不产生新模型推理。
- 调度耗时使用合成 duration profile（completion tokens × 2 ms）模拟，不是实测墙钟延迟。
- workflow-group paired bootstrap，以 `system_case` 为统计单元，B=10,000，seed=3407。
- `O1 Node Oracle` 只是在固定 workflow 与 frozen outcome 前提下的事后上界，不是全局最优 agent。

### 4.2 主结果

| 策略 | Workflow completion proxy | Q_node | Cost/workflow | Strong share | 平均模拟 makespan |
|---|---:|---:|---:|---:|---:|
| B2 Strong + Sequential | 13.9% | 0.3134 | $0.002896 | 100.0% | 2184.6 ms |
| B3 Strong + Parallel | 13.9% | 0.3134 | $0.002896 | 100.0% | 500 ms |
| **B4 P2 + Parallel** | **13.9%** | **0.3134** | **$0.002463** | **91.7%** | **500 ms** |
| O1 Node Oracle + Parallel | 26.2% | 0.4965 | $0.000270 | 12.8% | 500 ms |

结果解读：

- **路由效果（B3 → B4）：**在该 frozen replay 条件下，workflow 平均成本下降 **14.95%**；95% CI **[−26.59%, −5.16%]**。completion proxy 与 Q_node 的点估计不变，强模型占比下降 8.31 个百分点。
- **调度效果（B2 → B3）：**平均模拟 makespan 从 2184.6 ms 降到 500 ms。S10 的分位数报告对应 median 2000 → 500 ms、p90 3500 → 500 ms。必须保留“simulated / 模拟”限定。
- **Oracle 差距：**O1 completion proxy 26.2%，高于 B4 的 13.9%；这表示在固定 workflow 的 outcome lookup 下，当前路由离逐节点事后选优还有空间，但不代表真实生产中能达到该上界。
- **Matched random：**S10 额外做了 cost-matched 与 strong-share-matched 的对照，帮助把“用了更少强模型”与“路由确实利用了内容信号”分开观察。应在附表中保留对照限制，尤其 cost-matched 的 target 截断问题。
- **Failure/recovery：**10 个注入样本中仅观察到 1 个 workflow 在 recovery 中恢复；总体 completion 仍为 0%。样本太小，不能将其包装成恢复能力的总体结论。

### 4.3 Coordination Tax 指标必须解释清楚

冻结 JSON 中的 `coordination_tax.mean = 18.347` 定义为：

`B4 workflow cost / Σ(per-node independently-min-cost) − 1`

因此 18.347 是“ratio minus one”，不是直接的成本倍率。按此定义换算，均值对应约 **19.347×** 的实际比值、即相对分母高约 **1834.7%**；中位数 5.1057 对应约 **6.1057×**、即高约 **510.6%**。该分布右偏明显，且分母是逐节点独立最小成本和，不是可直接实现的 workflow 策略。建议将它放在诊断附录，明确公式和分布，**不要让这个极端值成为首页 headline**。

### 4.4 SystemBench 的核心价值与边界

它能回答：“在固定 workflow 结构、固定节点 outcome、固定成本口径和模拟调度时间下，把 P2 路由放进 workflow 后，成本与调度代理指标如何变化？”

它目前不能回答：“LLooM 在线执行真实复杂任务时，最终答案成功率提升了多少、真实延迟下降了多少？” 这属于不同的动态/live track，需要真实执行轨迹、实际成本、产物传递和 held-out tasks，不能通过改写 L1 的描述来补上。

---

## 5. S12 Semantic Workflow：值得保留的研究线索，但不能把它当作已经完成的模型排名

### 5.1 已完成的结构性工作

根据 33 号总结以及当前远程 `band_coverage_v02.json` 可见内容：

- 使用生产 decomposition prompt 生成真实 workflow seeds；初始 pilot 15/15 通过 canonical validator；后续再增加 15 个 workflow seed。
- 构建 30 个 semantic workflows，并与 10 个 LLMRouterBench exact-bound 控制 case 组成 40-case / 207-node 的候选集；其中 175 个为 semantic nodes。
- 建立 `semantic_role`、atomicity、dependency semantics 与 artifact contract 的 annotation spec。已记录 22/126（17.5%）复合节点需要按单一职责拆分，并修正依赖语义与 dangling references。
- 明确分开 Routing Input 与 Execution Input：R1 description 作为路由口径，R2 full prompt 作为上下文诊断口径。
- 远程已存的 `band_coverage_v02.json` 显示输入口径对模型可选空间影响很大：在 175 semantic nodes 上，`full_prompt` 的 multi-model eligible rate 为 8.6%，而 `description_only` 为 44.0%。这与生产路径当前对子任务只发送 description 的行为相关，因此应作为**输入协议/候选可用性诊断**呈现，而不是将完整上下文与描述输入结果混成同一条件。
- Routing Context Compression 指标与双 profile 方案（P1536 Budget Track、P4096 Capability Sensitivity Track）在本地工作总结中已形成探索性记录。

### 5.2 S12 语义矩阵的发现

按上传的 33 号总结，S12.5.2 做了 72-cell probe；S12.5.4 检查 protocol stability；S12.6-A 完成 198-cell L1 matrix；S12.6-B 形成 119/180 outputs、49 个已评分的 L2 partial matrix。

这些内容有研究价值：

- Output budget 会改变模型胜出排序；P1536 与 P4096 不是一个可互换的“同条件”配置。
- L1 双 profile winner agreement 只有 0.500（描述性口径），与早期 probe 的 4/6 agreement 一起表明预算可能是 routing 的实质性输入变量。
- 但 L1 的 mid-tier 发生模型代际替换；judge 从 qwen3.8-flash 切换到 qwen3.7-max；178/198 cell 有截断。它不是同质的模型比较矩阵，不能做 pooled semantic headline。
- L2 仅部分输出和评分，且方向上出现 strong 占优；这适合生成下一步假设，不足以得出 artifact-conditioned routing 已经有效/无效的结论。

### 5.3 关键的远程可复现缺口

虽然上述 L1/L2 细节记录在 33 号总结里，但当前远程 `v2` 分支无法打开以下预期产物：

- `benchmarks/systembench/matrices/semantic_outcomes_l1.jsonl`
- `benchmarks/systembench/matrices/semantic_outcomes_l2.jsonl`
- S12.6 L1/L2 报告
- `benchmarks/systembench/manifests/routing_input_contract_v1.yaml`
- Showcase 的来源清单、可比性审计 CSV 与 claims/limits 入口

因此，正确说法是：“本地工作总结记载这些实验已完成到某个程度；但当前远程 `v2` 还没有可供第三方复核的对应资产。”在相关文件同步前，不应将这些结果当作已在公开仓库中交付的证据。

---

## 6. 可展示的成果：建议按叙事组织，而非按时间线堆数据

### 展示 1：RouterBench 的质量—成本平面图（首屏）

展示 Always Strong、P2 Frozen、Best Single、两种 matched random 和 Oracle。把 Best Single 的 Pareto 支配关系明确画出来；P2 的亮点是比 Always Strong 节省成本、优于匹配随机，而不是成为最优策略。

图注必须写明：测试样本量 12,345；固定 R7 模型池；冻结 test；成本单位为每 query；质量与成本定义；95% CI 的 bootstrap 单位及设置。

### 展示 2：SystemBench 的“路由收益 + 调度收益”两面板图

左图比较 B3 与 B4 的 workflow cost 与 CI；右图比较 B2/B3/B4 的 simulated makespan。成本和时间分开画，避免把调度仿真时间与 API 延迟混为一个复合分数。

在同一页加一句 limitation：node outcome replay，completion 是 proxy，makespan 是 simulated，不是 online E2E。

### 展示 3：OrchestrationBench 的可诊断 demo

用一个工作流 JSON 展示：合法 DAG → validator → scheduler → failure injection → recovery transition → P1–P11 归因。该演示展示系统对问题的定位能力，而不是伪装成真实 benchmark leaderboard。

### 展示 4：Semantic protocol sensitivity（探索性页面）

只有在 matrix files、逐 cell provenance、分层审计 CSV 同步并可重生后才展示。图标题需明确 `Exploratory / non-pooled`。按 profile、model ID/generation、judge、finish reason / truncation 分层，不画一个混合平均排名。

### 展示 5：一个贯穿式 walkthrough

用一条固定案例走完：root goal → semantic nodes → R1/R2 输入构造 → route decision → frozen outcome/cost → schedule simulation → workflow completion proxy → failure attribution。旁边标出每一步的“真实生产逻辑”“离线回放”“仿真”身份，让观众知道证据从哪里来。

---

## 7. 相关成果能形成怎样的项目叙事

比较好的叙事不是“LLooM 已经全面胜过其它路由器”，而是：

1. **先把评测做得可信。** 统一决策真源、exact binding、manifest identity、hash、calibration/test 隔离与 paired bootstrap。
2. **发现了机制边界。** Weak 模型曾被 gate 结构性拦截；soft gate 与合适模型池能释放一部分选择空间，但收益由池内能力差距决定。
3. **把路由提升到 workflow 层看净效果。** 在冻结 outcome 下，P2 在 SystemBench 组合里节省约 15% workflow cost；并行调度显著降低模拟 makespan，但没有提高 completion proxy 点估计。
4. **继续探索更接近真实任务的 decomposition。** 它暴露出 atomicity、上下文口径和输出预算都会改变评测；正因为发现协议敏感，所以主动降低语义矩阵结论强度，而不是把异质数据硬合并。
5. **下一步以可复现交付为目标。** 先让现有证据从远程仓库可读、可验证，再由明确研究问题驱动新实验。

这是一条有可信度的工程研究故事：既有成功结果，也有负结果、基线限制和清楚的未测边界。

---

## 8. 当前远程仓库核验：需要马上处理的同步问题

### 8.1 当前看得到什么

当前 GitHub `v2` 分支能访问：

- [RouterBench v0.2 报告](https://github.com/citrus-dot/LLooM/blob/v2/benchmarks/routerbench/reports/v02_test_report.md)
- [RouterBench frozen manifest](https://github.com/citrus-dot/LLooM/blob/v2/benchmarks/routerbench/manifests/frozen_manifest_v02.json)
- [SystemBench v0.1 报告](https://github.com/citrus-dot/LLooM/blob/v2/benchmarks/systembench/reports/v01_report.md)
- [SystemBench v0.1 frozen manifest](https://github.com/citrus-dot/LLooM/blob/v2/benchmarks/systembench/manifests/frozen_manifest_v01.json)
- [SystemBench band coverage 结果](https://github.com/citrus-dot/LLooM/blob/v2/benchmarks/systembench/reports/band_coverage_v02.json)
- [OrchestrationBench README](https://github.com/citrus-dot/LLooM/blob/v2/benchmarks/orchestration/README.md) 与 [offline demo report](https://github.com/citrus-dot/LLooM/blob/v2/benchmarks/orchestration/reports/offline_v0.1.md)

可见的最新 bench commit 是 `c97d688`（S12.2–S12.4，semantic set build + annotation + Band Coverage Gate，2026-10-07）。

### 8.2 当前看不到什么

当前远程分支上，Showcase Freeze 总结所列 `benchmarks/showcase/`、S12.6 L1/L2 矩阵、L1/L2 报告、routing input contract、可比性审计 CSV 与 `32` 号决策文件均未找到；GitHub 也无法解析总结中提及的 `746c3d7` commit。

这通常意味着“本地完成但还没有推送/提交”“记录来自另一个分支”，或“路径/提交号记载有误”。不应猜是哪一种；下一步应直接在本机检查 git status、分支、commit log 和文件跟踪情况。

建议核对命令：

```bash
git status --short --branch
git branch --show-current
git log --oneline --decorate -20 --all
git show --stat --oneline 746c3d7

git ls-files benchmarks/showcase \
  benchmarks/systembench/matrices \
  benchmarks/systembench/reports \
  benchmarks/systembench/manifests
```

如果文件存在且已经通过离线校验：建立一个清楚的 commit，把小型 matrix/CSV/报告/manifest 纳入版本控制，原始大数据仍保持外置盘策略；确认后推送到 `v2` 或明确的 benchmark release branch。若文件未被跟踪，就先检查产物完整性与敏感内容，再决定纳入哪些文件。不要盲目 `git add .`。

### 8.3 文档入口也过时

- 当前 [`LLooMprogress.md`](https://github.com/citrus-dot/LLooM/blob/v2/LLooMprogress.md) 头部标注最后更新为 2026-10-04，尚未反映 RouterBench / SystemBench 后续成果。
- 当前 [`benchmarks/systembench/README.md`](https://github.com/citrus-dot/LLooM/blob/v2/benchmarks/systembench/README.md) 仍写 Phase A 完成、Phase B 和 v0.1 freeze 待办；但仓库已经有 v0.1 frozen report 与 manifest。
- OrchestrationBench README 仍是基础设施说明；这本身并非错误，但需要显式链接 offline report 与 SystemBench/Showcase 的关系。

因此，这一轮最大的实际缺口不是算法或实验，而是**仓库状态、文档状态和工作总结状态不一致**。

---

## 9. 下一步该做什么：有顺序、有停止线

### P0 — 同步与纠错（必须先做）

1. 核对本地 branch/commit，确认 33 号总结所说的 `746c3d7` 以及 Showcase 产物究竟在哪里。
2. 修正 RouterBench P2 数字归属：P2 Frozen 为 0.5863 / $0.000821；0.5867 / $0.000849 属于 P0 Current/BandOnly。
3. 校验所有展示图引用的 CSV 与表格都来自报告/manifest 的同一个 benchmark identity；检查 calibration 数据与 frozen test 是否被混在同一段话。
4. 把 S12.6 matrix、逐 cell provenance、分层 CSV 和审计说明纳入受控版本或提供稳定的 artifact 位置。外置盘原始数据不用复制进 Git。

**P0 完成条件：**远程仓库可以打开展示入口和所有引用文件；manifest hash 可对上；P2 主数字只保留一个权威版本；每个结果都能指出来源文件。

### P1 — 文档与展示入口收口（不跑 API）

1. 建立一个简单的 `benchmarks/showcase/README.md`，第一屏明确三条 track、Evidence Tier A–D 和“能证明/不能证明”的内容。
2. 放入 3 张核心图 + figure-data CSV：RouterBench cost-quality、SystemBench cost/makespan 两面板、semantic protocol sensitivity（探索性）。
3. 放入一个 demo case 和短 walkthrough；用 `claims_and_limits` 管住可引用句式与禁用说法。
4. 更新 SystemBench README 的阶段状态；更新 `LLooMprogress.md` 的 benchmark 里程碑与入口链接。
5. 提供完全离线的复核命令：数据与报告再生成、不联网、不产生 API 费用。

**P1 完成条件：**一位不了解内部工作记录的读者，10 分钟内能从 README 找到主结论、图表、原始报告、manifest 和限制条件。

### P2 — 一次性展示验收，然后冻结

按以下清单做一次，不转成新的研究计划：

- [ ] RouterBench 的 P2 / Best Single / Always Strong / matched random 数字与官方报告一致。
- [ ] SystemBench 把平均 makespan、median/p90 和 simulated 标注分开，不混写。
- [ ] Coordination Tax 显示公式、ratio-minus-one 定义与分布，不作为首页主结论。
- [ ] Semantic 图逐 cell 可追溯，模型代际、judge、profile、截断状态分层；不存在不受支持的 pooled mean/ranking。
- [ ] L2 标 `partial / exploratory`，不与 L1 合并。
- [ ] 所有 claims 都有来源文件；所有“未测”边界明确。
- [ ] 离线 replay / figure regeneration / audit checks 通过。
- [ ] manifest 与 snapshot commit hash 记录到 README。

验收通过后进入 maintenance。除非发生复现 bug 或出现具体评审/产品问题，不继续增加工作流数量、矩阵规模、benchmark 名称或图表数量。

---

## 10. 未来研究只按问题触发，不按路线图自动扩张

| 明确问题 / 触发器 | 下一步动作 | 暂时不做什么 |
|---|---|---|
| 评审问为什么 P2 没超过 Best Single | 先离线分析 frozen traces：任务族、候选资格、quality prior、strong 调用结构、成本估计误差；形成 Research Question 后再定实验 | 不立刻换权重、不把 test 当调参集、不再扫一轮任意 λ |
| 必须证明 semantic model ranking | 预注册式确认实验：固定 model generation、judge/rubric、profile 与输入合同，明确样本量、预算、截断处理与停止规则 | 不把当前异质 198 cells 改标题成 benchmark leaderboard |
| 必须证明 artifact context 改变模型选择 | 先核对 L2 raw/provenance，再做小样本 feasibility 和预定效应指标 | 不为填满 180 cells 而采集，不把 incomplete L2 当主结果 |
| 需要真实 E2E 成功率/延迟/花费 | 新建 dynamic/live track：held-out tasks、真实执行、完整 traces、真实成本，单独 identity 与报告 | 不将 frozen replay completion 或 simulated makespan 改称 live |
| 生产用户遇到 thinking / max_tokens 截断 | 单独立 production bug，固定模型和输入做受控回归；修复后创建新 policy identity | 不回写旧 frozen benchmark，不把生产修复和研究数据混作一版 |
| 未来要研发 State-aware Router | 等 SystemBench release 门槛通过，再有 L1 baseline + 同质 L2 证据和明确 interaction 主结果后，在独立 research branch 做 prototype / replay / shadow | 不在 Showcase 封板期间开发新内核 |

### 暂缓项

当前不建议投入：充值完成 L2；175-node 全矩阵补采；τ² / AppWorld / WebArena 全套 live 评测；继续扩充 orchestration 数据源；或把 State-aware Router 提前写成下一项主线。它们都不是修好当前交付所必需的。

---

## 11. 建议的最终对外主张（可直接做展示文案底稿）

> LLooM 建立了分层的路由与 workflow 评测体系。在冻结的 LLMRouterBench test（12,345 个实例）上，P2 相比 Always Strong 节省 13.46% 成本，质量保留率的 95% CI 包含 1.0，并显著优于成本匹配与强模型占比匹配的随机基线；但 Best Single 在该实验中的点估计更优，因此当前结果支持“有内容感知的路由收益”，而不是“已找到最优路由策略”。
>
> 在 SystemBench v0.1 的 frozen outcome replay 中，P2 将 workflow 平均成本降低 14.95%，而 completion proxy 点估计不变；并行调度把模拟 makespan 从约 2.18 秒降到 0.5 秒。由于节点结果为离线冻结查表、耗时为合成调度仿真，这些结果是可复现的组合层证据，不是实时端到端成功率或延迟声明。
>
> 进一步的 semantic workflow 探索表明，输入上下文和输出预算会实质性改变模型可选空间与胜出排序；目前矩阵存在模型代际、judge 和截断异质性，L2 又尚未完整评分，因此将其保留为探索性证据，并以同质协议确认实验作为未来的条件性研究方向。

对外发布前，必须先完成第 8 节的仓库同步和第 9 节的 P0/P1/P2 验收，确保这段文案链接到实际可访问的产物。

---

## 12. 收束判断

Bench 的核心成果不是“每一条假设都被证实”，而是三层工作互相衔接：

- RouterBench 给出了冻结的 query-level 定量证据和对照；
- OrchestrationBench 建立了确定性 workflow 验证、调度和失败归因基础设施；
- SystemBench 把路由与调度放到 workflow 级组合中，展示成本收益与 replay/simulation 边界；
- S12 则把输入合同、上下文压缩、输出预算敏感性与数据异质性显式暴露出来。

下一步应把这些成果做成一个能复核的交付物，而不是继续证明“还可以再测更多东西”。先对齐本地与远程状态、修正 P2 行、同步 Showcase 资产；通过一次离线验收后，进入 maintenance。新的研究应由具体问题触发，而不是由尚未填满的表格触发。
