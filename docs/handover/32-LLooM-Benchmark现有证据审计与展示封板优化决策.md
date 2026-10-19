# 32 · LLooM Benchmark 现有证据审计与展示封板优化决策

> 日期：2026-10-09  
> 文档性质：决策记录 + 有限范围执行规格。  
> 决策目标：以现有数据和代码资产，整理出可信、可复现、足以介绍 LLooM 的 benchmark 展示包；不以“把尚未完成的实验补齐”为目标。  
> 依据：`17-RouterBench实施总结-收官Day9-15.md`、`20-SystemBench实施总结-合并验收与PhaseAB.md`、`27-LLooM-SystemBench-S12.5-S15-最终实施决策与指导.md`、`29-LLooM-SystemBench-S12.5终审与后续实施规划-v6.md`、`30-SystemBench-S12.5.4到S12.6-工作记录与待拍板决策.md`、`31-SystemBench现有材料封板与展示优先计划.md`，并对照 LLooM 仓库中现有 SystemBench 说明/报告，以及 RouteLLM、TwinRouterBench、τ²-bench 等项目的公开实现文档。

---

## 0. 最终拍板

### 0.1 一句话决策

**停止扩数据，先完成证据审计、口径修正和展示包封板。现在交付的是“Benchmark Showcase / Research Snapshot”，不是正式宣布 SystemBench v0.2 完成。**

### 0.2 立即执行的决策

1. **不充值，不补完当前 S12.6-B，不扩成 175-node × 3-model 全矩阵。** 现有 L2 数据原样保留，标为 partial/exploratory；不得将它作为完整结果或纳入主 headline。
2. **RouterBench v0.2 与 SystemBench v0.1 继续冻结。** 不改测试集、不调结果口径、不为了让图更漂亮而重新选择策略。对展示数字的修正只限于解释、命名和派生视图，不能静默覆盖旧产物。
3. **不把 S12.6-A 的 198 cells 合并成一个“模型总体排名”。** actual model generation、judge model、output profile 和 truncation 均必须进入可比性审计；现阶段以协议敏感性诊断为主，不发布最终 semantic routing leaderboard。
4. **不将展示版命名为正式 `SystemBench v0.2` release。** 使用 `SystemBench semantic research snapshot` / `SystemBench v0.2-showcase` 这类显式非正式标签，且在封面、README、图注中重复说明其并非满足全部 v0.2 freeze gate 的正式版本。
5. **暂不开发新 router 内核机制。** `enable_thinking`、`max_tokens=estimated` 等生产侧发现独立登记，只有当其影响生产行为的可复现问题得到隔离后，才另开生产 bug 修复；不得混入冻结 benchmark 的重跑。
6. **展示包完成后进入 maintenance。** 任何下一项数据采集都必须由一个具体、可检验、足以改变决策的研究问题触发，并先写预算上限、同质性协议和停止条件。

### 0.3 哪些工作不再是展示包的前置条件

| 待办 | 当前决定 | 原因 | 何时重新打开 |
|---|---|---|---|
| 完成剩余 L2 API 采集和评分 | 延后 | 当前额度已耗尽，且接力采集/评分导致 protocol identity 更复杂；展示主线不依赖它 | 必须回答“artifact 是否改变 routing decision”且现有数据无法回答时 |
| 175 个 semantic node 全量采集 | 不做 | 节点数量达标不是研究完成标准，也不能修复异质协议 | 有正式 semantic benchmark release 的明确需求时重新制定实验设计 |
| 把 SystemBench v0.2 正式 freeze | 暂缓 | 尚有 human sign-off、同质性、oracle 与 manifest 等硬门槛未全部完成 | 用户明确需要可独立引用的正式 v0.2 benchmark 时 |
| O3 全精确枚举 | 展示包暂不阻塞；正式 v0.2 必须处理 | ≤8 nodes 时 3^8=6561，可精确枚举；不可将 approximate 与 exact 混称 | 需要正式发布 O3 / joint-oracle headline 时 |
| 100% headline-node human sign-off | 暂不作为展示包总门槛；demo case 必须人工复核 | 这是正式 semantic gold 的 gate，不是展示一组探索性诊断图的前置条件 | 任何 node-level semantic gold 要进入正式 headline 时 |
| 新 router、embedding router、trajectory-aware policy | 不做 | 当前需要先把现有证据交代清楚；否则机制与评估会同时变化，无法归因 | 展示审查提出具体失败模式，且能写成可证伪假设时 |
| AppWorld/WebArena/τ² 大规模接入 | 不做 | 属于新 benchmark track，不是修复当前展示包的必要工作 | 用户明确要求真实工具/状态/artifact replay 证据时 |

---

## 1. 这次相较 31 号方案的关键优化

31 号方案的主方向正确——先 Evidence Audit，再 Showcase Freeze；本文件不推翻它，而是补上以下必须明确的细节，避免“封板”本身又扩张成新一轮研究。

### 1.1 必须正面呈现 Best Single 的强基线地位

RouterBench v0.2 的主表中，P2 相对 Always-Strong 有成本节省且质量差异不显著，且超过 cost-matched / strong-share-matched random；**但点估计上，Best Single 的质量更高、成本也更低，因而支配 P2。**

因此，不能把图画成“LLooM P2 位于全局 Pareto 前沿”或“LLooM 已优于简单策略”。正确表述是：

- P2 在冻结测试集上相对 Always-Strong 节省约 13.46% 成本，质量 retention 的 CI 包含 1.0；
- P2 的质量点估计高于两种匹配随机基线，且对应 bootstrap CI 不含 0，支持“该路由不是简单随机分派”；
- **当前结果并没有证明 P2 胜过 calibration 选出的 Best Single。** 这个基线必须保留在同一主表/图中，而不能藏进脚注。

这一限制会降低宣传口号的强度，但会提高 benchmark 的可信度，并明确下一阶段真正值得研究的问题：LLooM 能否在同样公平的预算、模型池和测试集下，超越简单但很强的 Best-Single 选择？

### 1.2 把“SystemBench v0.2-showcase”与正式 v0.2 release 分开

正式 v0.2 的原始决策规范包含若干不可省略的条件：headline semantic nodes 100% human-approved、artifact/dependency/input contracts 完整、L1 outcome coverage、control split no-overlap、稳定的 generation/outcome identity、judge 口径、精确 O3、workflow-level paired CI 和可独立复现的 manifest。

当前展示需求不必一次完成所有这些条件。正确做法不是悄悄降低正式 release 的标准，而是明确建立两个不同的目标：

- **Showcase Gate：** 现有主结果可离线重现，分母/协议/限制无歧义，所有探索性材料如实标记。
- **Formal v0.2 Gate：** 满足原始规范；有任何硬条件未满足，就不能宣布正式 v0.2 完成。

因此，展示包可以完成，而正式 v0.2 仍保持 `not frozen / not release-ready`。这并不矛盾。

### 1.3 更严格地区分“输出预算敏感”与“全矩阵因果确认”

当前有两层证据：

- **S12.5.4 六节点 probe：** 1536 与 4096 只改变 max output；truncation 从 94.4% 降至 55.6%，winner agreement 4/6。这个小实验比大矩阵更接近受控的协议敏感性测试，原始判定为 REVIEW。
- **S12.6-A 33-node matrix：** 报告给出双 profile winner agreement 为 16/32，但采集期间发生 `qwen3.6-plus → qwen3.7-plus` 代际替换、judge 从 `qwen3.8-flash` 换为 `qwen3.7-max`，且 198 个 cell 中 178 个 truncated。

所以展示口径应为：

> 小样本受控 probe 已显示 output budget 能改变部分节点的胜出模型；更大的 L1 矩阵提供了同方向的描述性信号，但因模型代际、judge 身份和截断比例等因素，不应视为对单一变量效应的干净确认实验。

不要把“L1 的 16/32 winner agreement”无条件描述为“33-node 上已因果确认 budget effect”。先做离线 identity audit；如无法构造严格匹配子集，就只展示为 descriptive/exploratory。

### 1.4 把“数字修正”限于显示派生，不篡改冻结原始结果

SystemBench v0.1 的报告将 `Coordination Tax` 列为 mean `18.347`，其公式是每个 workflow 的 `B4 workflow cost / Σ per-node min-cost − 1`，随后对 workflow 值求均值。该数值容易被误读为 18.347% 或 18.347×。

展示时应同时呈现：

- 原指标名称：`Coordination Overhead`（或在兼容旧报告时标记为旧名 `Coordination Tax`）；
- 旧公式定义的 mean overhead：`18.347`，无单位的 ratio-minus-one 值；
- 派生 `Coordination Cost Ratio ≈ 19.347×`；
- 派生 `Coordination Overhead ≈ +1834.7%`。

需要在图注中说明这是从既有 per-workflow 值加 1 后的**派生展示值**，不是新采集或新统计。如果之后从原始 per-workflow 数据重新计算，应将 “mean of per-workflow ratios” 与 “ratio of aggregate costs” 分开命名，不要默认二者相等。

### 1.5 公开文档必须和实际状态对齐

核对 LLooM GitHub `v2` 分支时，`benchmarks/systembench/README.md` 仍包含“Phase A 完成、待 Phase B”的旧阶段描述，但同一仓库下的 `reports/v01_report.md` 已记录 80 cases、test 65、327 nodes、CI 和 hardening 结果。这是展示前必须修复的文档一致性问题。

另外，现有材料目录中的 `bench_report.md` 标题仍为 RouterBench v0.1、Core 1000 test 实例；它不能替代 RouterBench v0.2 的 12,345-instance 冻结 headline。对外打包前必须指定唯一的结果来源，防止旧表被误贴进新报告。

---

## 2. 当前证据资产盘点

### 2.1 可作为正式展示主线的冻结证据

| 资产 | 已知状态 | 可支撑的主张 | 不能支撑的主张 | 本轮操作 |
|---|---|---|---|---|
| RouterBench v0.2 | frozen；12,345 test instances；含 bootstrap CI 和 matched-random 对照 | 在指定模型池、协议和测试集上，P2 相对 Always-Strong 有成本节省且质量保持；相对两种匹配随机基线有正向差值 | P2 普遍胜过 Best Single；收益可外推到所有模型代际/生产池；Oracle 可部署 | 原样冻结，复核报告/manifest/hash 后生成展示派生图 |
| Workflow Track v0.1 | offline gate 完成 | 有 schema、validator、dataset adapter、planning evaluator、scheduler simulator、failure injection 等基础设施 | 已证明真实线上 decomposition、真实工具执行或 artifact 传播的端到端表现 | 作为基础设施层简要介绍；避免把它包装成 live agent benchmark |
| SystemBench v0.1 | L1 Frozen Control-plane Composition frozen | 在固定 workflow 与冻结 outcome 下，比较 routing cost；模拟调度时延；生产路由决策路径与 benchmark replay parity 有验证 | 真实 artifact-dependent workflow success 提升；真实线上 wall-clock latency；`Q_node` 等于最终 workflow quality | 重点展示，所有时间注明 simulated；成本与 completion proxy 分开讲 |

### 2.2 适合作为诊断/原型证据的研究快照

| 资产 | 当前观测 | 可以展示 | 必须标注 |
|---|---|---|---|
| S11 decomposition pilot | 15/15 个输出通过 canonical validator；估计 22/126 节点有复合职责（约 17.5%） | validator/schema 落地；atomicity 是实际 planner 诊断问题 | 样本小；validator 通过不等于 plan 语义正确；不是跨任务总体比率 |
| S12.5.4 P1536/P4096 probe | truncation 94.4% → 55.6%；winner agreement 4/6；REVIEW | output budget 能影响完成度/胜出模型的受控小样本证据 | n=6；只能作为 protocol sensitivity probe，不是广泛模型排名 |
| S12.6-A L1 | 198/198 有输出，197/198 有 judge score；178/198 truncated；跨代替换和跨 judge | 采集管线、输出预算敏感性及模型池互补性诊断 | 不发布 pooled overall model rank；所有分层需注明 actual model、judge 和 profile |
| S12.6-B L2 | 119/180 有输出；49/119 已评分 | artifact-conditioned 评测管线已开始；用于覆盖率/中断诊断 | 只存档 partial；缺失与 quota/执行顺序有关，不是可假定随机缺失；不得把 49 个分数当总体排名 |

### 2.3 SystemBench v0.1 主表的精确边界

现有冻结报告记录：80 workflows（test 65）、327 nodes。test 部分的核心结果如下：

| 策略 | completion proxy | Q_node | cost/workflow | strong share | simulated makespan 中位数 / p90 |
|---|---:|---:|---:|---:|---:|
| B2 · Strong + Sequential | 13.9% | 0.3134 | $0.002896 | 100.0% | 2000 / 3500 ms |
| B3 · Strong + Parallel | 13.9% | 0.3134 | $0.002896 | 100.0% | 500 / 500 ms |
| B4 · P2 Router + Parallel | 13.9% | 0.3134 | $0.002463 | 91.7% | 500 / 500 ms |
| O1 · Node Oracle + Parallel | 26.2% | 0.4965 | $0.000270 | 12.8% | 500 / 500 ms |

配套解释：

- `B2 → B3`：在固定 outcome/固定 workflow 下，parallel scheduler 把模拟 makespan 中位数从 2000ms 降至 500ms；这是调度仿真结果，不是线上时延。
- `B3 → B4`：completion proxy 与 Q_node 的点估计相同，workflow cost 下降约 14.96%；这是当前固定组合条件下的 routing-cost 结果，不应宣称质量提升。
- B4 对 B3 的 routing cost delta CI95 为 `[-26.59%, -5.16%]`，应引用冻结报告中的计算口径及同一 bootstrap 设置。
- `O1` 是基于事后节点信息的局部 oracle/上界参照，不是可部署策略，也不证明真实完整 workflow 的成功率可以到 26.2%。
- `Q_node` 为节点分数聚合的 secondary/diagnostic 指标，不等于最终 workflow quality。
- completion proxy 的语义必须附定义：全 required node 满足当前 replay 的成功判定、依赖条件与非法并行约束；它不是由真实下游 artifact 生成的 end-to-end success。

### 2.4 RouterBench v0.2 的展示数字和边界

以下表格应作为图表与摘要的单一来源，不应和 RouterBench v0.1 的旧数字混用。

| 策略 | Quality | Cost / query | 作用 |
|---|---:|---:|---|
| Always Strong | 0.5860 | $0.000948 | 成本/质量参照 |
| Best Single | 0.6053 | $0.000674 | **强基线；点估计支配 P2，必须可见** |
| LLooM P2 frozen | 0.5863 | $0.000821 | 当前 headline router policy |
| Cost-matched Random | 0.5610 | $0.000819 | 相近成本随机对照 |
| Strong-share-matched Random | 0.5749 | $0.000899 | 相近强模型占比对照 |
| Oracle | 0.7122 | $0.000118 | hindsight upper bound；不可部署 |

冻结结果的关键统计：

- 相对 Always-Strong 的 cost saving：`13.46%`，CI95 `[12.20%, 14.73%]`。
- 相对 Always-Strong 的 quality retention：`1.0005`，CI95 `[0.9957, 1.0052]`；该区间含 1.0，不能说质量显著提高。
- 相对 cost-matched random 的质量差：`+2.52pp`，CI95 `[+2.07, +2.97]`。
- 相对 strong-share-matched random 的质量差：`+1.14pp`，CI95 `[+0.75, +1.53]`。

**必须明确写出：Best Single 质量 0.6053 高于 P2 的 0.5863，成本 $0.000674 也低于 P2 的 $0.000821。** 因而当前 RouterBench v0.2 的主结论不是“P2 达到全局最优 cost-quality frontier”，而是一个更窄但有统计支持的结论：P2 能以相对 Always-Strong 更低的成本保留相近质量，并胜过匹配随机策略；但它尚未击败 calibration 选择的 Best Single。

---

## 3. 对照相关项目的实际实现：我们应该学什么、不应该照搬什么

本节把相关项目当作实现与评测设计参照，而不是把它们的结果当作 LLooM 的证据。以下对照基于公开仓库/文档，截至 2026-10-09。

### 3.1 RouteLLM：缓存评测资产，避免每次都重新生成结果

参考：<https://github.com/lm-sys/RouteLLM>

其 README/评测实现说明：已有 benchmark 结果会缓存；MT-Bench 使用预计算 judgments；新增 benchmark 通过统一 Benchmark 抽象接入；尽量预先生成结果，避免每次评测都重新调用模型。

对 LLooM 的可迁移做法：

- 当前 showcase 必须完全离线生成图表与摘要，不应为了“展示完整”再调用 provider。
- 冻结的 RouterBench v0.2 与 SystemBench v0.1 产物是展示的主要输入；报告生成器只读这些产物并输出图表数据，不能隐式重跑模型。
- 评测执行与报告渲染拆开：`collect/run` 产生原始产物，`score/analyze/report` 只读产物。

不应照搬的部分：RouteLLM 的常见设置聚焦两个模型之间的路由与 preference/win-rate 信号；LLooM 当前有 tier、eligibility、budget、workflow composition 等更多维度。二者 benchmark 结果或指标不能直接横向比较。

### 3.2 TwinRouterBench：同一个 suite 内明确分开静态和动态轨道

参考：

- 代码：<https://github.com/CommonstackAI/TwinRouterBench>
- 论文：<https://arxiv.org/abs/2605.18859>

从 README 可见，该项目将 Static 与 Dynamic 放在同一个 suite，但有各自清晰的评测含义：Static 使用固定 supervision bank 和 nominal cost metric；Dynamic 用 SWE-bench Verified、真实工具调用和固定的 held-out instance list。动态评分产物还记录 pool/pricing fingerprint、run ID、失败惩罚、trace cost 等，并提供 `score`、`audit-infra`、`audit-trace-cost`、`render` 等命令；支持通过实例级结果文件断点续跑。

对 LLooM 的可迁移做法：

- 保留清楚的 fidelity 分层，而不是等待 live evaluation 完成才展示：`RouterBench offline → SystemBench frozen composition → semantic exploratory snapshot → 未来 artifact/live track`。
- 把 pool、pricing、prompt/generation profile、evaluator/judge、split 和代码版本写进身份记录；结果必须能追溯到某个不可变 run，而不是只存一个汇总均分。
- 即使本轮不开发完整 CLI，也至少在展示包中有 `manifest + raw results + derived report + audit` 的等价结构。
- 未来若要接真实工具执行，应建立新的 dynamic/live track 和独立 held-out set，不能把它作为当前 frozen 数字的“验证补丁”。

不应照搬的部分：TwinRouterBench 的 SWE 轨道需要 Docker、工具环境、API key、较多磁盘和运行费用。它证明的是分轨设计和审计产物的价值，并不意味着 LLooM 现在就需要接入 SWE-bench。

### 3.3 τ²-bench：评分规则变化会改变分数，必须版本化

参考：

- 代码：<https://github.com/sierra-research/tau2-bench>
- 评分/任务版本变动记录：<https://github.com/sierra-research/tau2-bench/blob/main/CHANGELOG.md>
- 重评分说明：<https://github.com/sierra-research/tau2-bench/releases>

τ²-bench 公开记录过任务数据与 grader 逻辑修复，相关 domain 在版本前后的分数不可直接比较；项目提供对已有 trajectory 重新评分的路径，并允许固定旧 tag 来复现旧行为。

对 LLooM 的可迁移做法：

- 原始 LLM 输出、tool/节点 trace、实际 model ID、生成配置、judge 结果必须分层保存。
- 换 judge、换 rubric、换 validator 或修复 task/reference 后，要生成新的 evaluator/rubric/task identity；旧结果保持可读和可复现。
- 若未来需要用统一 evaluator 重评旧输出，要把它作为新的 `rescoring run`，同时保留原来的原始 judge score，不得覆盖。
- 报告必须注明哪些分数可跨版本比较，哪些由于 scorer/task 变化不可比较。

### 3.4 AgentRouter：可作为下一阶段问题线索，不构成当下开发命令

参考：<https://arxiv.org/abs/2609.22951>

AgentRouter 的研究将 routing 放到 multi-step agent trajectory 内，讨论不同 workflow step 的复杂度差异。它与 LLooM 的 workflow-aware routing 方向有关，但仅凭研究方向相近，并不能推出 LLooM 当前必须开发 trajectory-aware router；更不能在当前 protocol 尚未收束时同时修改 router 和 benchmark。

本轮只记录为 future-work reference。只有当 showcase 审查明确指出“当前 route decision 无法利用 workflow state/artifact 造成可重复损失”时，才把它转换成一条可证伪假设。

### 3.5 从相关实现提炼出的三条工程原则

1. **先缓存、再展示：** 评测数据采集和报告派生分离；当前展示不能依赖任何实时 API 调用。
2. **静态和动态分轨：** frozen replay、semantic prototype、真实 artifact replay、live run 是不同证据等级，绝不拼成一个混合总分。
3. **结果身份版本化：** dataset/split、model pool、generation protocol、judge/evaluator、pricing、router policy、scheduler、failure policy 变化都应可追溯；变化可能要求新 benchmark identity，而不是覆盖旧 headline。

---

## 4. 展示层级与推荐主张

### 4.1 Evidence Tier 定义

| Tier | 定义 | 本项目内容 | 展示方式 |
|---|---|---|---|
| A · Frozen main evidence | 冻结、具备明确分母/口径、离线可重现 | RouterBench v0.2；SystemBench v0.1 L1 Frozen Composition | 主报告、核心图、演示主线 |
| B · Diagnostic evidence | 数据可读但研究限制显著；有助于提出下一问题 | S11 atomicity；S12.5.4 protocol probe；经身份审计后的 L1 profile 分层 | “诊断发现”章节，显式标 exploratory |
| C · Partial evidence | 覆盖不全且缺失机制影响可比性 | S12.6-B L2 partial；异质 judge/model 的 pooled 值 | 附录/coverage table；不进入 headline，不混算 |
| D · Not yet measured | 当前没有相应证据 | 真实 artifact propagation 后的 E2E success；live tool latency/spend；trajectory-aware mechanism 的收益 | 仅列 future work，不展示虚构数值或暗示已验证 |

### 4.2 可以采用的对外叙事

> LLooM 已有两层可复现的路由/组合评测证据：RouterBench v0.2 在冻结测试集上量化了成本—质量取舍，并通过两种匹配随机基线验证策略不是简单随机分派；SystemBench v0.1 则把生产路由决策路径接入固定 workflow 的 frozen outcome composition，分离了路由成本效果与并行调度的模拟时间效果。后续 semantic prototype 显示 output budget、上下文和任务分解质量会影响 routing 研究结果，但当前的模型/judge/protocol 异质性意味着这部分仍属于诊断证据，而不是正式的语义路由 leaderboard。完整 artifact replay 与 live E2E 仍是后续研究，而非已完成结果。

### 4.3 禁用表述

下列表述不得出现在 README、演示稿或图表标题中：

- “LLooM 已经找到全局最优路由策略。”
- “P2 优于所有简单策略 / Pareto optimal。”
- “SystemBench 证明端到端 agent 成功率提升。”
- “并行调度让线上 latency 减少 75%。”（当前是模拟 makespan）
- “L1/L2 证明 artifact 让强模型胜出”或“已证明真实 artifact propagation 的 routing gain”。
- “S12.6 semantic matrix 给出最终 Weak/Mid/Strong 排名。”
- “SystemBench v0.2 complete/released”，除非正式 v0.2 全部 gate 已满足并通过签字。

---

## 5. 最小展示包：交付内容只保留必要件

建议将展示材料放入独立目录，例如 `benchmarks/showcase/`（具体路径按仓库现有布局决定，避免搬动旧产物）。展示包的目标是“任何主结论都能快速追溯并离线重生”，而不是建立新的 benchmark 框架。

### 5.1 必需文件

```text
benchmarks/showcase/
├── README.md                         # 当前展示包入口、证据层级、快速复现
├── showcase_report.md                # 主叙事：控制在可阅读的项目介绍长度
├── claims_and_limits.md              # 可引用的 claims / 禁止越界的 claims
├── manifests/
│   └── showcase_manifest.json        # 输入文件 hash、commit、数据版本、图表版本
├── audit/
│   ├── source_inventory.md           # 每个主数字来自哪个冻结产物
│   ├── routerbench_v02_check.md      # 主指标/CI/基线逐项核对
│   ├── systembench_v01_check.md      # 分母、composition 口径、模拟时间、ratio 命名
│   ├── semantic_l1_comparability.csv  # 逐 cell identity/质量状态审计
│   └── l2_coverage.json               # L2 采集/评分覆盖，明确 partial
├── figure_data/
│   ├── routerbench_v02_frontier.csv
│   ├── systembench_v01_composition.csv
│   └── semantic_protocol_sensitivity.csv
└── figures/
    ├── routerbench_quality_cost.png
    ├── systembench_composition.png
    └── semantic_protocol_sensitivity.png
```

如果仓库目前不适合新增这么完整的目录，不必一次造齐；最小可接受版本至少包含 `showcase_report.md`、`claims_and_limits.md`、`showcase_manifest.json`、三张图对应的 CSV，以及一个包含 demo case 的 Markdown/JSON。不要为目录结构本身再开一轮架构工程。

### 5.2 展示包主报告建议结构

1. 问题定义：模型路由、workflow composition、系统级控制的关系。
2. RouterBench v0.2：冻结结果、匹配随机对照、Best Single 的限制。
3. SystemBench v0.1：固定 workflow/frozen outcome 下的 routing cost 与 simulated scheduling time。
4. Workflow Track：用一小段说明已有 validator/scheduler/failure simulation 基础设施；不独立造 headline。
5. Semantic prototype：S11 atomicity + S12.5.4 protocol sensitivity；L1 identity 异质性说明。
6. 明确限制：L1 frozen outcome 不等于真实 artifact propagation；L2 partial；无 live E2E 结论。
7. 下一研究门槛：只有出现具体外部需求或产品失败模式才重开付费实验/机制研发。

---

## 6. 三张核心图：图要回答问题，而不是制造胜利感

### 图 1 · RouterBench v0.2 quality–cost comparison

**目标问题：** P2 相对 Always-Strong 和两个 matched-random control 的取舍是什么？它有没有击败 Best Single？

要求：

- 必须同时包含 Always-Strong、Best Single、P2、cost-matched random、strong-share-matched random、Oracle。
- 显示质量和成本两个维度；P2 不得被标成 global Pareto winner。
- Best Single 应显眼可见，并通过注释明确：它在点估计上比 P2 质量更高、成本更低。
- Oracle 标成 `hindsight upper bound / not deployable`。
- 图注注明 `n=12,345`、冻结模型池与策略、CI 区间的来源，且不能把 calibration 结果当 test 结果。
- 如为了展示统计显著性在旁边画差值 forest plot，所有差值必须保持原来的比较对象和 bootstrap CI，不得混用不同分母。

不推荐单独以“Pareto frontier”作为标题；更合适的标题是“Frozen test: quality–cost trade-offs and matched controls”。

### 图 2 · SystemBench v0.1 composition matrix

**目标问题：** 在固定 workflow 与 frozen node outcomes 的条件下，routing 和 scheduling 分别改变了什么？

建议拆成两块而不是塞进一根多指标柱图：

- A 块：B2/B3/B4 的 completion proxy、Q_node、workflow cost、strong share。
- B 块：B2/B3/B4 的 median/p90 simulated makespan。

说明：

- `B2 → B3` 主要体现 scheduler 的模拟并行效果。
- `B3 → B4` 主要体现当前组合下的成本变化；完成率与 Q_node 点估计并未提高。
- `O1` 可放在虚线/单独区域标作 upper bound，避免和实际 policy 争胜。
- Coordination 指标使用正确名称与单位，不能写成“18.35%”。
- 图注固定出现 `L1 Frozen Composition`、`frozen outcomes`、`simulated duration profile`。

### 图 3 · Semantic protocol sensitivity

**目标问题：** 现有结果为什么不能被简化成一个统一的 model ranking？

优先显示：

- S12.5.4 probe 的 P1536/P4096 truncation rate（94.4%/55.6%）与 winner agreement（4/6，REVIEW）；
- L1 matrix 的 16/32 profile winner agreement 作为描述性附加信息，不当作干净因果确认；
- 逐 profile 的 winner 分布和各 cell truncation 必须由 `semantic_outcomes_l1.jsonl` 离线重新统计，分母需显式显示；
- 在同一张图或图注报告不同 actual model ID 与 judge identity，至少注明 `qwen3.6-plus → qwen3.7-plus`、`qwen3.8-flash → qwen3.7-max`；
- 明确 198 cells 中 178 truncated、197 scored。

不允许：

- 选出看起来更好看的 P1536 或 P4096 单独当最终 headline；
- 把跨 judge 分数简单平均之后排列 Weak/Mid/Strong；
- 仅凭总体 winner count 推断模型本征能力排序；
- 将 quota 中断导致的 L2 缺失当作 MCAR（完全随机缺失）。

---

## 7. 可比性审计规格（纯离线，不调用 API）

### 7.1 每个 semantic cell 必须有的审计列

最低字段：

```text
benchmark_snapshot_id
case_id
node_id
split
layer (L1/L2)
profile (P1536/P4096)
requested_model_id
actual_model_id
model_generation
judge_model_id
judge_rubric_hash
prompt_or_input_hash
execution_input_hash
reference_artifact_hash
max_output
thinking_mode
temperature
finish_reason
truncated
output_status
score_status
score_value
error_class
cost_basis
included_in_headline
exclusion_reason
```

如果某个字段从现有文件中无法恢复，填 `unknown/not_recorded`，不要根据预期配置补写为事实。缺失的 identity 本身就是 audit finding。

### 7.2 先分状态，再计算均值

对每一 profile、actual model、judge 和 split 产出如下计数：

- planned cells；
- 有输出 cells；
- 有效评分 cells；
- `finish_reason=length` / truncated cells；
- invalid JSON/null；
- API error/quota interruption；
- 其他错误；
- 进入某项统计的 paired cells / paired nodes。

**禁止把以下状态都编码成 0 分：** API error、无输出、截断、invalid JSON、合法但任务失败、judge 缺失。若 benchmark 规范本身将某种生成失败定义为失败结果，须在对应指标中明示；同时保留状态字段以便分层审计。

### 7.3 允许做哪些统计

可以：

- 按 profile 报告 truncation/coverage；
- 按实际模型、judge、split 分层给出样本计数；
- 对身份和评分协议相同、case/node 对齐的子集做 paired comparison；
- 记录 S12.5.4 的 controlled probe 作为最强 protocol-sensitivity evidence；
- 报告 L2 的 coverage 与中断原因。

不可以：

- 对跨 judge 的原始分数直接 pooled mean；
- 把 `qwen3.6-plus` 和 `qwen3.7-plus` 直接都压成单一 `mid` 后声称模型 identity 恒定；
- 把 L1 independent nodes 的分数位移解释为 L2 artifact-dependent workflow 的成功率提升；
- 对 L2 已评分的 49 cells 计算总均分并称之为 L2 model ranking；
- 对没有输出的 cell 作无证据的均值填补。

### 7.4 L2 partial 必须给出的 coverage 摘要

以当前工作记录的分母：

- 有输出：119/180 = 66.1%；
- 已评分：49/119 = 41.2%（相对已生成输出）；
- 已评分占计划矩阵：49/180 = 27.2%。

缺失与 quota interruption、调用/评分顺序和 model/judge 接力相关，不可当作随机缺失。对外展示最多放 coverage table + 明显的 `PARTIAL / NOT FOR HEADLINE` 标签，不把它融入 L1 分数。

---

## 8. 版本和身份管理：不建立新系统，只把必要的字段落进 manifest

### 8.1 Showcase manifest 最低字段

```json
{
  "showcase_id": "lloom-benchmark-showcase-2026-10-09",
  "status": "showcase_snapshot_not_formal_v02_release",
  "created_at": "2026-10-09",
  "source_commit": "<actual_commit_sha>",
  "routerbench_id": "<frozen_routerbench_v02_id>",
  "systembench_v01_id": "lloom-systembench-v0.1",
  "semantic_snapshot_id": "<explicit_exploratory_snapshot_id>",
  "inputs": [
    {"path": "<frozen_result_path>", "sha256": "<sha256>"}
  ],
  "figure_data_version": "1",
  "report_version": "1",
  "api_calls_during_render": 0,
  "notes": [
    "RouterBench v0.2 and SystemBench v0.1 remain frozen",
    "S12.6-A is exploratory due to mixed model/judge identities and truncation",
    "S12.6-B is partial and excluded from headline"
  ]
}
```

占位符必须在实际执行时填真实值；不要把示例中的 placeholder 当成实际 manifest。若现有 manifest 已有同义字段，优先复用，不需要再造第二种命名。

### 8.2 变更什么会要求新 identity

以下内容变化后，至少应产生新的 run/report identity；若改变了 benchmark 测量含义，则新建 benchmark identity：

- case set、split、reference/gold；
- model pool 或实际 model generation；
- router policy/权重/eligibility/gate；
- `thinking_mode`、max output、temperature、prompt/template、structured-output、retry behavior；
- judge model、rubric、validator 或任务评分语义；
- pricing source/cost basis；
- scheduler、failure/recovery policy、workflow schema；
- 任何会改变计分结果的 bug fix。

仅改变图表颜色、图标题、布局等呈现样式，不需要改变 benchmark identity，但必须递增 report/figure version，保证截图和 CSV 对应。

### 8.3 原始数据与派生结果规则

- 原始 JSONL/JSON 不覆盖；任何清洗、过滤、排除都生成派生文件。
- 派生 CSV 中每个排除项必须有 `exclusion_reason`。
- `collect`（模型调用）和 `score/analyze/render`（纯离线）分开；当前 showcase 只执行后者。
- 重评分如果未来发生，应新建 run，保留原 judge output/score；不以新评分覆盖旧结果。

---

## 9. 执行顺序：四个有限阶段，全部以离线工作为主

### Phase 0 · 冻结来源与清点事实（P0）

目标：先确保“图里每个数字来自哪里”没有歧义。

- [ ] 确认实际展示基于哪个 LLooM commit/branch；记录 `git rev-parse HEAD`、working tree 状态和来源 commit。
- [ ] 列出 RouterBench v0.2、SystemBench v0.1、S11、S12.5.4、S12.6-A/B 的权威结果文件及 SHA-256。
- [ ] 把 RouterBench v0.1 的 `bench_report.md` 和 v0.2 frozen report 明确区分，避免旧结果混入新图。
- [ ] 检查仓库 README、冻结报告、manifest、代码 docstring 的状态是否一致；至少修正对外入口中“Phase B 仍待做”的陈旧描述，或在新 showcase README 中明确指出权威状态来源。
- [ ] 统计每项主结果的 dataset/test split、denominator、缺失与排除规则。
- [ ] 记录所有外部参考及访问日期；外部项目只能作为方法参照，不能作为 LLooM 性能证据。

完成标准：任何主图可追溯到明确的源文件、commit 与 split；找不到来源的数字不进入主报告。

### Phase 1 · 离线审计和口径修正（P0）

- [ ] 建立 `semantic_l1_comparability.csv`；字段至少满足第 7 节。
- [ ] 产出 per-profile、per-actual-model、per-judge、per-split 的 coverage/truncation/state table。
- [ ] 对 16/32 winner agreement 的分母与 paired cell 明确标注；若无法证明同 node、同模型身份和可接受的评分协议对齐，就只用作 descriptive diagnostic。
- [ ] 产出 L2 coverage-only 报告，复核 119/180、49/119、49/180 三种口径，所有比例都配分母。
- [ ] 检查 SystemBench v0.1 B2/B3/B4 指标名称和定义；将 simulated makespan 与 cost/completion 分开。
- [ ] 从冻结的 per-workflow 数据计算 Coordination ratio 的派生展示值，并解释旧字段 `18.347` 与 `19.347×` / `+1834.7%` 的关系；不覆盖原始 frozen report。
- [ ] 确认所有图表生成命令不触发 provider，不需要 API key。

完成标准：没有未解释的 denominator、identity、missing cell 或指标单位；所有不兼容观测都显式分层。

### Phase 2 · 制作 Showcase（P0）

- [ ] 写 `showcase_report.md` 和 `claims_and_limits.md`。
- [ ] 产出 RouterBench v0.2 quality–cost comparison 图（含 Best Single 与两种 matched random）。
- [ ] 产出 SystemBench v0.1 composition 图，清楚拆开 routing cost 与 simulated schedule。
- [ ] 产出 semantic protocol-sensitivity 图，不做 model leaderboard。
- [ ] 选择一个小型 demo workflow；人工检查 root goal、node role/atomicity、依赖、routing input、execution input 和回放结果。若其 semantic gold 尚未人审，不把它描述为 approved gold。
- [ ] 在 demo 中标出 production decision path、frozen outcome replay、simulation/proxy 各自边界。
- [ ] 所有主图附分母、协议、身份与证据等级，且都能从 `figure_data/*.csv` 离线再生成。

完成标准：不新增 API 花费；演示流程能在 1–2 分钟内解释“数据是什么、支持什么、不能说明什么”。

### Phase 3 · 公开/分享前一致性检查（P1）

- [ ] 入口 README 和主报告引用相同的结果文件/版本号。
- [ ] 主图不从过时的 v0.1 `bench_report.md` 抄取 v0.2 数字，也不把 private research snapshot 说成正式 release。
- [ ] 任何显示 `SystemBench v0.2` 的地方都改成 `SystemBench semantic research snapshot (not formal v0.2 release)`，或在上下文清楚处使用同等明确的称谓。
- [ ] 所有 external references 有链接；外部 benchmark 的结果不混进 LLooM 图表。
- [ ] 执行离线再生成/校验，确认图上的数字与表格一致。
- [ ] 加入最后一页“Limitations & Next Gate”，明确 L2 partial、没有 live E2E、Best Single 限制。

完成标准：外部读者只看展示包，不会误以为 L2 已完成、semantic leaderboard 已稳定或真实 artifact replay 已通过。

---

## 10. Formal SystemBench v0.2 的门槛：完整保留，但不绑架本轮展示

如果未来决定正式发布 `SystemBench v0.2 Semantic Workflow Composition`，必须重新开启正式 release checklist。至少核对：

- [ ] semantic workflow set 与 split 冻结；control sample no-overlap 100%；
- [ ] headline node 的 semantic role、atomicity、dependency semantics、artifact contract、completion relevance 100% human-approved；
- [ ] `routing_input_hash` 与 `execution_input_hash` 完整且保持双轨，不可合成一个模糊 `prompt`；
- [ ] generation profile、outcome protocol、model pool、judge/rubric、cost basis、scheduler/failure policy identity 完整；
- [ ] model ranking 的 headline 使用同一 actual model generation、同一生成协议和可比较的评分协议，或明确限制到同质子集；
- [ ] L1 coverage 与错误类别完整；L2 要么满足预先定义的正式 coverage/validity gate，要么在版本定义中明确不作为 v0.2 headline；不能将 partial 伪装成 complete；
- [ ] O3 对不超过 8 nodes 的 case 全量枚举 3^n assignments，并对每个 assignment 计算合法调度及 cost/latency/quality；approximate oracle 只能另标；
- [ ] workflow-level paired CI 可由冻结输入独立重现；节点级结果不得取代 workflow-level inference；
- [ ] manifest hash、运行命令、输出与报告互相可追溯；
- [ ] 对外 README 与版本标记一致，并有显式 sign-off。

**此 checklist 不属于当前 Showcase Gate。** 未完成它不妨碍如实展示 RouterBench v0.2、SystemBench v0.1 和语义原型诊断；但它明确阻止当前快照被误称为正式 v0.2 release。

---

## 11. 停止条件与下一阶段触发器

### 11.1 Showcase Stop Gate

以下条件全部满足后，当前 benchmark 工作转为 maintenance。不要再按既有路线图自动进入 S12.7/S13/S14。

- [ ] RouterBench v0.2 的主表、CI、Best Single、matched random 和 Oracle 边界可离线重现。
- [ ] SystemBench v0.1 的 80/65/327 分母、B2/B3/B4 指标和 CI 可追溯；simulated makespan 明确标注。
- [ ] Coordination ratio 的旧字段与新展示单位解释清楚。
- [ ] S12.6-A 的模型/judge/profile identity 已审计；不兼容数据没有 pooled headline。
- [ ] S12.6-B 明确为 partial，coverage 数字正确，未混入主要质量比较。
- [ ] 主报告明确 Best Single 支配 P2 的点估计这一事实。
- [ ] 三张主图的分母、数据来源、协议、证据等级和限制完整。
- [ ] demo case 的真实决策路径、frozen replay 和 simulation/proxy 边界标注清楚。
- [ ] 所有图表可在无 provider 调用的情况下再生成。
- [ ] 仓库入口文档不再与已完成报告相冲突。

### 11.2 哪些反馈才足以重开实验

| 触发情形 | 推荐反应 | 不应自动触发 |
|---|---|---|
| 观众问“P2 为什么没超过 Best Single？” | 把它记录为清晰 research question；先从 frozen traces 分析当前模型池/策略限制 | 不立即开发新 router 或付费跑完整新 benchmark |
| 观众需要可信的 semantic model ranking | 重新写预注册式确认实验：固定 model generation、judge/rubric、profile，只变指定变量，先定义最小样本/预算/停止条件 | 不把当前异质 198 cells 直接改标题后发版 |
| 产品实际出现 qwen `thinking`/token budget 截断 | 独立 production bug，建立受控 API regression 和新的 policy/run identity | 不回写冻结 RouterBench/SystemBench 结果 |
| 需要证明 artifact 会改变模型选择 | 另开 L2/historical artifact replay RQ；优先 source audit 和已有 traces，再决定最小实验 | 不为“补完 180 格矩阵”而补采 |
| 需要证明真实端到端工具成功率/延迟/花费 | 另建 dynamic/live track，锁定 environment、test IDs、模型池/价格，并记录完整 traces | 不将当前 simulated makespan 当 live latency |
| 只是希望 benchmark 更大或图更多 | 拒绝扩张；展示包完成后维护 | 不添加新数据集、新 profile 或新 metrics 作为默认任务 |

### 11.3 新机制开发的进入条件

必须先写一页机制提案，至少包括：

```text
Observed failure mode
→ Hypothesis
→ Measurable prediction
→ Frozen test / baseline
→ Budget cap + stop condition
→ Implementation change
→ Ablation
```

如果现象可由现有的模型池结构、output budget、context compression、generation failure 或 judge variation 解释，就先解决评测/配置问题，不要跳到新算法。

---

## 12. 建议后续提交的最小任务切片

为避免工作又变成“所有东西都做”，按以下顺序开任务，每一项完成后都可独立停下。

| 顺序 | 任务 | 预期产物 | API 成本 | 必须不做的事 |
|---:|---|---|---:|---|
| 1 | Source-of-truth audit | 结果清单、SHA、commit、README 状态一致性 | 0 | 不更改冻结数据 |
| 2 | L1/L2 audit scripts or tables | 可比性 CSV、分层 coverage、L2 partial 报告 | 0 | 不重评、不补采、不算 pooled headline |
| 3 | SystemBench report wording fix | cost/ratio/simulation/proxy 解释和派生值 | 0 | 不篡改旧 `v01_report.json` |
| 4 | Three figures + CSV | 图、底层绘图数据、caption | 0 | 不增加新 benchmark case 或新 metric |
| 5 | Showcase report + claims ledger | 主报告、声明与限制 | 0 | 不宣称正式 v0.2 / live E2E |
| 6 | Final offline replay check | 可重复生成记录、最终 manifest | 0 | 不接 provider，不改 router core |
| 7 | Maintenance handoff | 维护范围和研究触发器 | 0 | 不按路线图自动继续 S13/S14 |

推荐将 1–6 作为当前唯一 active milestone。第 7 项只需在展示包交付时形成短记录，不需要额外建设流程工具。

---

## 13. 对项目介绍的最终定位

### 13.1 一句话定位

**LLooM 的 benchmark 阶段已建立冻结的模型路由定量比较和 workflow 组合评测基础；当前正在把已有证据整理成可复现的展示快照，并明确 semantic routing、artifact replay 与 live E2E 的证据边界。**

### 13.2 三层叙事

1. **RouterBench — Router quantitative track**：回答固定测试集和模型池中，路由策略相对 Always-Strong 与匹配随机基线的质量/成本表现；同时坦诚 Best Single 仍然是更强的简单基线。
2. **Workflow/SystemBench v0.1 — Frozen composition track**：在固定 workflow 和 frozen outcomes 下，把 routing 与 scheduler 的影响拆开测量；结果可离线复现，但不等同于真实下游 artifact 传播或 online agent completion。
3. **Semantic research snapshot — Diagnostic track**：说明分解原子性、input/context 和 output budget 如何影响实验设计与模型选择；当前 L1/L2 尚不足以作为正式 semantic model leaderboard。

### 13.3 当前阶段名

建议使用：

> **Benchmark evidence audit & showcase freeze**

不使用：

> “SystemBench v0.2 全面完成”或“开始新 routing core research”。

---

## 14. 参考资料与核验说明

### LLooM 当前资产

- LLooM repo（`v2`）：<https://github.com/citrus-dot/LLooM/tree/v2>
- SystemBench README：<https://github.com/citrus-dot/LLooM/blob/v2/benchmarks/systembench/README.md>
- SystemBench v0.1 frozen report：<https://github.com/citrus-dot/LLooM/blob/v2/benchmarks/systembench/reports/v01_report.md>
- RouterBench 收官总结与 SystemBench S12.5/S12.6 工作记录：以项目研究文档中的对应文件为准，并在 showcase manifest 里记录确切 commit/hash。

### 相关项目实现

- RouteLLM：<https://github.com/lm-sys/RouteLLM>（评测缓存、预计算 judgements、可扩展 benchmark 接口）
- TwinRouterBench：<https://github.com/CommonstackAI/TwinRouterBench>（静态/动态两轨、locked pool/pricing、held-out IDs、run trace、score/audit/render）
- TwinRouterBench paper：<https://arxiv.org/abs/2605.18859>
- τ²-bench：<https://github.com/sierra-research/tau2-bench>（任务/评分版本、旧轨迹重新评分、可比性说明）
- τ²-bench changelog：<https://github.com/sierra-research/tau2-bench/blob/main/CHANGELOG.md>
- AgentRouter paper：<https://arxiv.org/abs/2609.22951>（仅作为 multi-step/trajectory-level routing 研究方向参照）

### 核验边界

- 本决策将相关项目的公开 README、代码入口和 release/changelog 作为**方法与工程实现**参照，不将它们的报告指标作为 LLooM 的结果。
- 现有工作记录是本项目状态与数字的主要来源。若 GitHub 当前内容与研究工作记录不一致，以实际用于展示的 commit 下可复现产物为准；在发布前必须解决这种差异，不能默默选取更有利的一方。
- 所有日期按 2026-10-09 记录。外部仓库会继续变化，正式发布时应再次固定引用的 commit/tag，而不是只记 `main` 分支 URL。

---

## 最终决策复述

**当前不再以更多数据换取“完整感”。先完成源文件与结果身份审计、修正易误读的统计表述、明确 Best Single 这一强基线、离线生成三张核心图，并公开说明 L1/L2/live 的证据边界。**

当这些工作完成，展示包即可以封板并进入 maintenance。正式 SystemBench v0.2、L2 artifact replay、真实 live evaluation 和新 router 机制，都留在独立的后续研究门后；它们不再自动成为当前任务的延长线。
