# LLooM Benchmark Showcase Report — Research Snapshot

> 版本 v1（2026-10-09）。数据源 commit 与全部输入 hash 见 `manifests/source_inventory.json`。
> 本报告为 **showcase snapshot**，不宣布 SystemBench v0.2 正式完成（门槛见 32 号 §十）。

## 1. 研究问题

LLooM 关注智能 LLM 路由的三层问题：

1. **Routing（query 级）**：同一个 query，便宜的模型够不够？何时必须上强模型？
2. **Scheduling（workflow 级）**：子任务并行化能省多少时间？
3. **Composition（系统级）**：路由 + 编排组合后，成本/时间/质量的净效果是什么？

三个 benchmark track 各自回答一层，**不合并成一个总分**。

## 2. Tier A · RouterBench v0.2（冻结主证据）

**设置**：`lloom-routerbench-v0.2` 冻结（R7 池 deephermes/r1-distill/intern-s1-mini + P2 policy，
soft-gate 0），test 12,345 实例，实例级 paired bootstrap B=10k seed=3407。

| 策略 | Quality | Cost/query | 说明 |
|---|---:|---:|---|
| Always Strong | 0.5860 | $0.000948 | 成本/质量参照 |
| **Best Single** | **0.6053** | **$0.000674** | 强基线：点估计质量更高且成本更低 |
| **LLooM P2 (frozen)** | 0.5863 | $0.000849 | headline router policy |
| Cost-matched Random | 0.5610 | $0.000819 | 近成本随机对照 |
| Strong-share-matched Random | 0.5749 | $0.000899 | 近强模型占比随机对照 |
| Oracle | 0.7122 | $0.000118 | hindsight 上界，不可部署 |

统计结论（bootstrap CI）：

- 相对 Always-Strong：cost saving **13.46% [12.20, 14.73]**；quality retention **1.0005 [0.9957, 1.0052]**（含 1.0，质量差异不显著）
- 相对 cost-matched random：**+2.52pp [2.07, 2.97]**；相对 strong-share-matched random：**+1.14pp [0.75, 1.53]** —— 路由不是简单随机分派（content-aware 实证）
- **Best Single 支配 P2 的点估计**（质量 0.6053 > 0.5863，成本 $0.000674 < $0.000849）。

> 图：`figures/routerbench_quality_cost.png`（数据：`figure_data/routerbench_v02_frontier.csv`）

**适用边界**：结论限于冻结的 llmrb test split、该模型池与该 policy；不外推到所有模型代际与生产池。

## 3. Tier A · SystemBench v0.1 L1 Frozen Composition（冻结主证据）

**设置**：80 cases（test 65）/ 327 节点，节点 prompt 为 llmrb 真实 query（exact binding），
node outcome 全部 frozen replay（零新推理），调度为 duration-profile 仿真。

| 策略 | completion proxy | Q_node | cost/workflow | strong share | sim. makespan mean |
|---|---:|---:|---:|---:|---:|
| B2 Strong+Sequential | 13.9% | 0.3134 | $0.002896 | 100% | 2184.6 ms |
| B3 Strong+Parallel | 13.9% | 0.3134 | $0.002896 | 100% | 500 ms |
| B4 P2 Router+Parallel | 13.9% | 0.3134 | **$0.002463** | 91.7% | 500 ms |
| O1 Node Oracle | 26.2% | 0.4965 | $0.000270 | 12.8% | 500 ms |

- **B2→B3（调度效果）**：模拟 makespan 2184.6→500 ms（duration-profile 仿真，非线上墙钟）。
- **B3→B4（路由效果）**：workflow cost **−14.95%**（bootstrap CI [−26.59%, −5.16%]），
  completion proxy 与 Q_node 点估计不变——当前口径下路由改变的是成本，不是质量。
- **O1 为事后上界参照**，不是可部署策略。
- **Coordination 指标**（B4 相对逐节点独立最优的开销）：frozen 报告原字段
  `coordination_tax.mean = 18.347`（无单位 ratio-minus-one）→ 派生展示值
  **Cost Ratio ≈ 19.35×** / **Overhead ≈ +1834.7%**（换算关系见 `audit/coordination_ratio_derivation.json`；
  median 5.11，右偏分布，均值受少数 workflow 主导）。

> 图：`figures/systembench_composition.png`（数据：`figure_data/systembench_v01_composition.csv`）

**适用边界**：completion proxy 是 replay 成功判定（全 required 节点 succeeded + 依赖满足 +
无非法并行），**不是**真实 artifact 传播后的端到端 success；makespan 是仿真值。

## 4. Tier B · Workflow Track 基础设施（简述）

OrchestrationBench v0.1 offline 基础层完成：canonical schema + validator（错误码=归因键）、
PlanningBench 467 + PlanBench 1901 适配、planning evaluator（tau=0.45）、四策略调度器、
failure 注入 F1-F7 + recovery 状态机、P1-P11 归因。定位为基础设施，不独立造 headline。

## 5. Tier B · Semantic prototype（诊断证据，exploratory）

### 5.1 真实 decomposition 的 atomicity 发现（S11/S12）

生产 decompose prompt（qwen-plus temp=0）15/15 通过 canonical validator；
126 节点中 22 个（17.5%，16/30 workflow）违反 single-responsibility，按 spec 拆分为 25 组。
这是 planner 的诊断发现，样本小，不是跨任务总体比率。

### 5.2 Routing input 口径差异（S12.4）

同一批节点，R2 full_prompt 口径 91.4% hard / multi-model eligible 8.6%；
R1 description-only 56% hard / eligible 44%。**Routing Context Compression = 0.1948**——
router 看到更多上下文不一定有更多可选模型；这是下一阶段 step-level routing 的核心假设来源。

### 5.3 Output budget 是实质性 routing variable（S12.5.4 + S12.6-A）

- **受控 probe（n=6，REVIEW）**：只变 max_output（1536→4096），truncation 94.4%→55.6%，
  winner agreement 4/6；tier 排序翻转（weak>mid>strong → mid>strong>weak）。
- **L1 matrix（33 节点 × 3 模型 × 双 profile，exploratory）**：双 profile winner agreement
  16/32（描述性）；truncated 178/198；**存在 mid 代际替换（qwen3.6-plus→qwen3.7-plus）与
  judge 换档（qwen3.8-flash 113 cells + qwen3.7-max 84 cells）**。
- 逐 cell 身份审计：`audit/semantic_l1_comparability.csv`（198 行全字段）+ 分层状态
  `audit/l1_stratified_status.json`。

> 图：`figures/semantic_protocol_sensitivity.png`（数据：`figure_data/semantic_protocol_sensitivity.csv`）

**不允许的读法**：跨 judge/model generation 混合 pooled mean、单一 semantic leaderboard、
把协议效应当模型本征能力。

## 6. Tier C · L2 partial（仅存档）

`semantic_outcomes_l2.jsonl`：180 计划 cells，119 有输出（66.1%），49 已评分
（41.2% of 输出 / 27.2% of 计划）。缺失与 quota 中断/采集顺序相关（非 MCAR）。
仅作 coverage 报告（`audit/l2_coverage.json`），不进任何 headline、不与 L1 合并。
初步方向性观察（49 cells，不足以下结论）：artifact 注入条件下 strong 均值占优
（0.75 vs weak 0.51 / mid 0.39），与 L1 的 budget 效应方向相反——这是未来
artifact-conditioned routing 的假设来源，不是结论。

## 7. 明确边界（三个）

1. **L1 不是 live**：frozen outcome replay 证明控制面与归因链路，不证明真实 artifact 传播后的端到端效果。
2. **semantic L1 outcome 协议不完全同质**：model generation / judge / profile 三层身份分层后方可比，当前未做跨层合并。
3. **L2 partial**：管线已打通（stage0 冻结 artifacts → conditional cells → judge），数据未完整。

## 8. 下一研究门槛

唯一保留的研究决策：**是否值得付费推进完整 artifact/state replay（方向 B）**——
仅在项目需要主张 artifact-dependent counterfactual 收益时启动。其余扩展
（175-node 全矩阵、新 benchmark 接入、无止境 sweep）按 31/32 号停止线不再自动进行；
生产侧 `enable_thinking` / `max_tokens=estimated` 问题独立立 issue（P-01），不混入 benchmark。
