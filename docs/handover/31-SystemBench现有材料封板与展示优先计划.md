# 31 · SystemBench 现有材料封板与展示优先计划

> 日期：2026-10-09  
> 依据：`30-SystemBench-S12.5.4到S12.6-工作记录与待拍板决策.md`，并承接 RouterBench v0.2、SystemBench v0.1 与 S12.5 的既有记录。  
> 本文性质：当前阶段的**决策文件与有限范围执行计划**。目标不是继续扩大 benchmark，而是先用现有材料形成可信、可复现、能讲清楚边界的展示版本。  
> 核心决策：**暂停新 API 采集与新 benchmark 接入；封存当前实验快照；优先完成证据分级、同质性审计、结果图表与演示叙事。**

---

## 0. 决策摘要

### 当前总决策

1. **不充值、不继续补完 S12.6-B、不扩展到 175-node 全矩阵。** 当前 L2 数据保持 partial，作为探索性材料存档，不纳入主结果。
2. **RouterBench v0.2 与 SystemBench v0.1 保持冻结。** 不修改其数据、评估口径或 manifest；将其作为目前最稳定的定量证据。
3. **S12.6-A 的 198-cell 结果保留，但不能直接称为同质、可独立比较的完整 semantic routing benchmark。** 中途发生了 mid 模型代际替换、judge 模型切换、输出预算敏感性与高截断率；必须分层报告，而不是把所有 cell 简单合并求均值。
4. 建立一个 `SystemBench v0.2-showcase`（或 `v0.2-exploratory-snapshot`）展示快照，而不是宣称正式的、完整可比的 v0.2 benchmark release。
5. 当前主工作是 **benchmark 的收束、审计与展示准备**，不是新内核路由机制开发。生产侧发现的问题单独记 issue；在展示版本完成前，不重构 `router.rs` 或引入新 router。

### 现在已经足以展示什么

- **Routing 定量证据**：RouterBench v0.2 的冻结结果及其统计区间、匹配随机基线与跨 benchmark 边界。
- **System composition 证据**：SystemBench v0.1 在冻结 outcome / 固定 workflow 条件下，证明生产路由路径与 replay parity、workflow cost 有可测变化；调度时间结果须标为模拟 makespan，而不是真实 wall-clock。
- **Semantic prototype 证据**：真实 decomposition 可通过 canonical validator；atomicity 问题、routing input 与 output budget 对结果的影响已在小样本中显露。
- **暂时不能宣称**：已经证明完整端到端 task success 提升、L2 counterfactual 已完成、当前 semantic matrix 得出了不受协议影响的最终模型排名。

---

## 1. 当前状态：完成了什么，尚未完成什么

### 1.1 状态盘点

| 层 | 当前状态 | 可用于展示的内容 | 不能越界声称的内容 |
|---|---|---|---|
| RouterBench v0.2 | Frozen / maintenance | 冻结 test、matched random、bootstrap CI、成本节省与质量保留 | 不代表所有模型代际、所有生产池都能复现相同幅度 |
| Workflow Track v0.1 | Offline gate 完成 | schema、validator、PlanningBench/PlanBench adapter、scheduler、failure simulator | 不等于已验证真实在线 decomposition 和 execution 的端到端表现 |
| SystemBench v0.1 | L1 Frozen Composition 完成 | production/replay parity；固定结构下 routing 的 workflow cost 变化；调度模拟时间 | 不等于语义依赖真实传播后的 system-level quality |
| S11 decomposition pilot | 15 个输出通过 validator | 展示真实 decomposition 输出和 schema 落地；支持发现 atomicity 问题 | 样本量不足以代表所有任务；validator 通过不等于 plan 语义正确 |
| S12.5.4 protocol probe | REVIEW | 1536 与 4096 输出预算会改变模型 winner 的探索性证据 | 6-node probe 不能代表所有 task family |
| S12.6-A L1 semantic matrix | 198 cells 有输出，197 个有 judge 分数，但协议/模型身份存在变化 | 输出预算敏感性、候选模型池存在互补性的探索性分析 | 不能把不同 generation model、不同 judge model 的分数当作完全同质观测，直接汇总为最终 headline |
| S12.6-B L2 conditional matrix | 119/180 有输出，49/119 已评分，partial | 展示 artifact-conditioned 评测管线已开始；作为失败/中断案例存档 | 不得作为完整 L2 结果，不得由 49 个已评分 cell 推导稳定的 L2 排名或 counterfactual 结论 |

### 1.2 当前最大的风险不是工程，而是证据同质性

S12.6 的问题不是“缺少更多数据”这么简单，而是**模型身份、输出协议、judge 身份与采集阶段发生过变化**：

- P1536 与 P4096 的结果显示 winner agreement 在 probe 中为 4/6；更完整的 L1 matrix 中，双 profile winner agreement 为 16/32。输出预算确实是会改变模型排序的实验变量。
- L1 mid 层发生 `qwen3.6-plus` → `qwen3.7-plus` 的代际替换；这可以作为真实 model-pool 更新，但不能再把所有 mid cell 都视为同一个固定模型的结果。
- Judge 先后使用 `qwen3.8-flash` 与 `qwen3.7-max`。rubric 正文相同并不足以证明两个 judge 的尺度完全一致；若没有交叉校准，应按 judge identity 分层，不能默认分数可无偏混合。
- 198 cells 中 178 个 truncated，意味着 outcome 当前测量的是**指定生成协议与预算下的任务完成度**，不是模型在充分输出预算下的通用能力。
- L2 只完成部分采集和评分，而且采集/评分模型接力，必须保留 partial 标签。

因此现在应做的是**把已有材料整理成可诚实展示的证据结构**，不是继续通过更多 API 调用掩盖现有协议变化。

---

## 2. 证据分级与对外口径

所有图表、展示稿和 README 必须使用以下三个证据等级。

### Tier A · 可作主 headline

**A1. RouterBench v0.2 冻结结果**

使用其独立冻结报告中的数字与 CI，清楚注明数据源、模型池、基线与适用边界。不要把 RouterBench 上的 saving 直接外推为所有生产工作流的 saving。

**A2. SystemBench v0.1 L1 composition 结果**

可表述为：在固定 workflow、冻结 node outcome 与当前 L1 completion proxy 下，routing 组合带来 workflow cost 下降；并行调度降低模拟 makespan。必须同时注明：workflow 结构/节点 outcome 并非真实下游 artifact 依赖的完整 end-to-end replay；makespan 使用模拟 duration profile，不是线上墙钟时间。

### Tier B · 可作诊断发现，需显著标注探索性

- Router visible context 不同会导致 band 与 eligible model 份额明显变化。
- P1536 与 P4096 的 winner ranking 不稳定，说明 output budget 是 routing policy 的实际变量。
- 真实 decomposition 约有一部分节点违反 single-responsibility/atomicity 规则；这是当前 planner 的诊断发现，不是已完成的跨任务基准结论。
- L1 semantic matrix 在当前运行协议下观察到多层模型有不同 winner，但该信号需要按 model generation / judge / output budget 分层，不能合并成单一“最佳模型”结论。

### Tier C · 仅存档，不进入 headline

- S12.6-B 未完成的 119/180 outcomes、49 个 judge score。
- 跨 judge 模型的混合均分，除非完成可信的 judge 交叉校准。
- 跨 mid generation 的混合结果，除非明确按实际 model ID 分层并且比较集可配对。
- 任何由 L1 independent outcome 推断出的真实 artifact-dependent workflow success。

---

## 3. 当前要做的核心改进

### P0-1 · 冻结当前语义，不再边采边改

把以下内容写入 `v0.2-showcase` manifest：

- source commit、case IDs、split 与每条 cell 的唯一 ID；
- 每个 cell 的真实 provider model ID / generation，不只写 weak/mid/strong tier；
- temperature、thinking mode、max output、估计/实际 token、finish reason、truncated flag、retry policy；
- execution input hash、routing input hash、reference artifact hash；
- judge model ID、judge prompt/rubric hash、评分时间、评分有效性；
- cost basis、价格核验时间、未知成本标记；
- `evidence_tier` 与 `included_in_headline`。

原始数据只追加、不覆盖。任何清洗和排除都生成派生文件，并保留排除理由。

### P0-2 · 建立“可比性审计表”

对现有 L1 matrix 逐 cell 建立审计列：

```text
case_id
node_id
profile (P1536/P4096)
requested_model_id
actual_model_id
model_generation
judge_model_id
rubric_hash
finish_reason
truncated
score_status
split
```

然后至少产出以下视图：

1. **P1536 / P4096 分开**；
2. **actual_model_id 分开**，特别把 `qwen3.6-plus` 与 `qwen3.7-plus` 分开；
3. **judge_model_id 分开**，不把不同 judge 直接合并；
4. 只在相同 node、同一 profile、相同实际模型身份/评分协议下做 paired comparison；
5. 对 `null`、invalid JSON、truncated、API error 分别报告，禁止混为同一失败类别。

这一步是零 API 成本，且比继续补 61 个 L2 cell 更有价值。

### P0-3 · 对 1536/4096 的结论采用“协议敏感性”，不选最好看的 profile

报告必须并列给出：

- P1536：受限输出预算下的任务完成表现；
- P4096：更宽输出预算下的敏感性对照；
- 两者 winner agreement / rank correlation / truncation rate；
- 哪些 case 或 role 出现 winner inversion。

如果不同 profile 排序变化明显，结论应该是“模型选择依赖预算 profile”，而不是宣称其中一个 profile 是唯一正确答案。暂时不加 P8192 或更多 profile；除非展示目标明确需要，否则会重新引入无限扩展。

### P0-4 · 把 L2 partial 从主结果分离

`semantic_outcomes_l2.jsonl` 保留原样；额外生成只读诊断报告：

- 采集覆盖率：119/180；评分覆盖率：49/119；
- 按 actual model / judge / profile 列出 coverage；
- 说明缺失不是随机缺失，而与额度中断和采集顺序有关，因此不能只对已评分 cells 计算 pooled mean 后代表总体；
- 不将 L2 partial 与 L1 score 合并成一个 system score。

**当前不充值、不继续接力换模型完成 L2。**

### P0-5 · 分清 benchmark 结果与 production finding

单独登记生产侧问题，不在本轮为了解决 benchmark 问题而直接改内核：

- Qwen 3.6 的 `enable_thinking` 默认行为可能导致过长 reasoning 与截断；
- `max_tokens=estimated` 可能低估输出需求。

它们应该成为独立 production issue，按真实 API profile 进行回归；修复生产逻辑之后，必须使用新的 production commit/policy identity 做新 run，而不能悄悄修改已冻结 benchmark 的结果。

---

## 4. 有限范围的展示计划：先完成“Showcase Freeze”，不扩数据

### 交付目标

制作一套足够用于项目介绍、立项演示和下一阶段讨论的材料。交付完成后暂停 benchmark 扩展，除非演示反馈暴露明确缺口。

### 交付件 A · `showcase_report.md`

建议控制在 5–8 页等价篇幅，结构固定为：

1. LLooM 的研究问题：模型路由、workflow orchestration、system composition；
2. RouterBench 的主要定量结果；
3. SystemBench v0.1 composition 路径与可测结果；
4. S11/S12 semantic prototype 的两项诊断发现；
5. 目前的三个明确边界：L1 不是 live、semantic L1 outcome 协议不完全同质、L2 partial；
6. 下一步仅保留一个研究决策：是否值得付费推进完整 artifact/state replay。

### 交付件 B · 三张核心图

**图 1：RouterBench quality-cost frontier**

- 使用已冻结 v0.2 指标；
- 至少包含 Always Strong、Best Single、LLooM P2、matched-random、Oracle；
- Oracle 明确标为上界，不作为可部署策略。

**图 2：SystemBench v0.1 composition matrix**

- quality/completion proxy 与 workflow cost 同表；
- 单独画 simulated makespan：Sequential vs Parallel；
- 标记固定 workflow / frozen outcome / L1 条件。

**图 3：Semantic protocol sensitivity**

- P1536 vs P4096 的 truncation rate、winner agreement、per-tier score/胜出分布；
- 将它定位为“协议敏感性诊断”，而不是最终 routing leaderboard。

### 交付件 C · 一个贯穿全场的 demo case

展示一条从 root goal 到 workflow node 的链：

```text
root goal
  → semantic nodes
  → routing_input / execution_input
  → route decision
  → frozen outcome / cost
  → schedule
  → completion proxy
```

必须明确哪里是真实 production decision path，哪里是 frozen replay，哪里仍是模拟或人工标注。不要用一个漂亮的 workflow 图暗示所有 node 之间已经完成真实 artifact propagation。

### 交付件 D · 复现与限制说明

至少附：

- commit / manifest identity；
- 运行命令与环境要求；
- 价格和 cost basis；
- API 消耗记录；
- 分母、split、缺失 cell、truncation 统计；
- 对外可引用的主结果与仅可内部参考的探索性结果。

---

## 5. 明确停止线：什么情况下不再继续扩展

满足以下条件，就可以宣布“当前 benchmark 足以展示”，转入维护：

- [ ] RouterBench v0.2 的主结果、CI 与 matched random 可由冻结报告复现；
- [ ] SystemBench v0.1 的 cost / completion proxy / simulated makespan 表有清楚定义；
- [ ] L1 semantic matrix 的混合 model/judge/profile 被审计并分层；
- [ ] L2 partial 被标注为 partial，未被混入 headline；
- [ ] 所有主图都有 denominator、split、协议、证据等级；
- [ ] 主报告明确说明 L1 / L2 / Live 的区别；
- [ ] demo 能在不发生新 API 调用的情况下重现主要图表。

完成后停止：

- 不再扩展到 175-node L2 full matrix；
- 不接入新的外部 benchmark；
- 不继续追着模型 pool 或输出预算做无止境 sweep；
- 不为追求更高 headline 指标修改 test 或筛选样本。

---

## 6. 如果展示后确实需要继续，只允许两个有边界的后续方向

### 方向 A · 统一协议确认性实验（优先级较低）

仅当外部评审明确认为 semantic routing headline 不足时再启动。要求先确定**一个实际可用的固定模型池、一个 judge 或 deterministic evaluator、一个输出预算 profile**；采集范围只针对必要的 matched cells，不默认回到 175×3 全量。若无法保证评分同质性，则宁可不作新 headline。

### 方向 B · L2 Artifact Replay（研究升级）

仅当项目确实要主张 artifact-dependent workflow 的 counterfactual 收益时才启动。先冻结 source/trajectory/schema/evaluator，再做小样本 feasibility；不把历史轨迹本身当作 counterfactual。此方向是新的研究阶段，不属于当前“展示准备”的必需条件。

Live / AppWorld / WebArena / 大规模 τ² 继续后置。

---

## 7. 现在是不是该开发新内核机制？

**当前答案：不是。**

当前优先级是：

```text
Evidence Audit
  → Showcase Freeze
  → Presentation / Review
```

而不是：

```text
change router.rs
  → add state-aware policy
  → 再去找 benchmark 证明它有效
```

生产侧 `enable_thinking` / `max_tokens` 问题可独立修复和回归，但它们不是新路由内核机制。

只有在展示完成后，仍然存在清晰、可重现且无法由现有机制解释的问题，才另开机制研究分支。可能的假设包括：

- router 使用的上下文长度会压缩 eligible model 空间；
- 固定 band/quality prior 不能充分利用 mid tier；
- workflow state / artifacts / remaining budget 应该进入 step-level routing feature。

每个机制分支必须先写清：`hypothesis → measurable prediction → frozen test → implementation → ablation`，不能仅因“看起来有创新”就修改生产决策路径。

---

## 8. 推荐的研究叙事

以三层证据讲清楚 LLooM，而不是把所有实验压成一个总分：

1. **RouterBench：** 在冻结的公开模型矩阵与明确基线下，研究低成本模型何时可以替代强模型，以及这种收益的适用边界。
2. **SystemBench v0.1：** 把真实 production routing decision path 接入 workflow composition，验证路由、调度和 frozen outcome replay 能正确组合，并量化其在当前受控口径下的成本/时间影响。
3. **Semantic prototype：** 发现 routing input 与输出预算会改变模型选择；该发现提供下一阶段假设，但当前样本及协议异质性还不足以当作最终性能结论。

不声称“SystemBench 已经证明完整端到端 agent 成功率提升”；应声称“SystemBench 已建立可复现的系统级评测骨架，并获得了初步的 semantic / protocol-sensitivity evidence”。

---

## 9. 后续执行顺序（有限版）

### Phase F1 · Evidence Audit（零成本）

1. 冻结当前数据快照，记录 `origin/v2` 对应 commit、所有 manifest 与文件 hash。
2. 生成 L1 cell-level comparability table；分层统计 actual model、judge model、profile、split、finish reason 与 truncation。
3. 对 L2 生成 coverage-only report，不补采、不把 partial 均分当总体结论。
4. 对关键 headline 重新跑纯离线汇总，确认图表完全由冻结产物生成。

**通过条件：** 不存在未解释的 cell、分母或 protocol identity；混合协议均明确分层。

### Phase F2 · Showcase Freeze（零成本）

1. 生成 `showcase_report.md`；
2. 生成三张核心图和一个 demo case；
3. 生成 `claims_and_limits.md`（哪些数字可引用、哪些不可）；
4. 主要图表离线可重现；不调用 provider。

**通过条件：** 任何主结论都能在 1–2 分钟内说明数据源、分母、基线、协议和限制。

### Phase F3 · Maintenance（默认）

只修影响复现与展示的 bug；不加新数据、不动冻结口径。

### Phase F4 · 有明确理由才解锁研究升级

由外部反馈或产品问题触发，而不是按路线图自动扩展。新实验必须有明确 RQ、停止条件、预算上限与新 benchmark identity。

---

## 10. 最终决策表

| 事项 | 决策 | 理由 |
|---|---|---|
| 继续 RouterBench tuning | **不做** | 已有冻结主结果；边际收益低于证据收束 |
| 完成当前 L2 剩余采集 | **暂停** | 额度耗尽且跨代采集/judge 已使同质性风险增加；并非当前展示必需 |
| 充值以完成 full L2 | **暂不授权** | 先用现有材料做出可展示版本，再根据明确研究问题决定 |
| 把 L1 198 cells 合并为单一 headline | **不允许** | generation / judge / budget identity 不完全同质 |
| 展示 P1536/P4096 差异 | **允许，标注 exploratory** | 是有价值的 protocol sensitivity 发现，但不等于普遍规律 |
| 冻结 SystemBench v0.2 | **只冻结 showcase snapshot，不宣布完整 benchmark release** | L2 partial，L1 semantic matrix 存在可比性限制 |
| 修改 router 内核 | **暂缓** | 先完成 benchmark 展示与证据审计；生产调用问题另立 issue |
| 产出展示包 | **现在优先做** | 现有材料足以讲清研究目标、定量结果、工作流组合和已发现边界 |

---

## 11. 外部方法对照

TwinRouterBench 将 step-level routing 先做静态离线评测，再以独立 held-out 的 live task resolution 与实际花费验证。这支持把当前 RouterBench/SystemBench 的 frozen 结果和未来 live 结果分开报告，而不是要求所有层面一次性完成。参见：

- TwinRouterBench paper: https://arxiv.org/abs/2605.18859
- TwinRouterBench repository: https://github.com/CommonstackAI/TwinRouterBench
- AgentRouter: https://arxiv.org/abs/2609.22951

这份外部参照仅用于支持“分阶段、不同 fidelity 单独报告”的方法选择；不意味着 LLooM 当前已有与这些工作的同等 live benchmark 证据。
