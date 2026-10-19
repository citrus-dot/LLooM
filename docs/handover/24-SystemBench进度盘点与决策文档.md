# 24 · SystemBench 进度盘点与决策文档（S12.0-S12.1 停点）

> 日期：2026-10-07 · 执行：ZCode（主会话） · 基线：origin/v2 @ `1746cf5`
> 定位：SystemBench S12 停点的**决策支持文档**——当前进度全景 + 三个需要 Nironta 决策的问题（详细展开方案/利弊/推荐）。
> 上游：`23-后续优化与实施总规划-v4`（路线权威）/ `22-SystemBench实施总结-S10-S11` / `21-v3` / `20`。
> 状态：**S12.0（Annotation Spec）与 S12.1（Seed Audit）已完成并推送；S12.2 起暂停，等本文档三个决策。**

---

## 一、进度全景（自 19 号方案起，一页看全）

### 1.1 已完成（按 21/23 号排期核对）

| 阶段 | 内容 | commit | 状态 |
|---|---|---|---|
| 合并验收 | OrchestrationBench O-Day 9-14 交接验收（Rust 138/Python 54/红线 grep） | — | ✅ |
| Phase A（S1-S5） | SystemBench 骨架：namespace/registry/builder/L1 引擎/route-batch seam | `422d695` | ✅ |
| Phase B（S6-S8） | 80-case + failure 传播 + Coordination Tax + paired bootstrap + v0.1 freeze | `6c92f9a` | ✅ |
| S9a | Shadow parity：284 节点 production==replay 零失配 + band 语义验证 | `539eed5` | ✅ |
| S10 | L1 Hardening 五项：scheduling headline（scheduler regret=0）/ matched random 双对照 / recovery ablation / cost validity / coverage audit / O2+O3 | `c28a18f` | ✅ |
| S11 | 真 decomposition pilot：15/15 canonical 通过 + tau 校准否定性结论 + 15 semantic seeds | `246e86e` | ✅ |
| **S12.0** | **Annotation Spec v1**（`workflow_annotation_v1.yaml`） | `1746cf5` | ✅ |
| **S12.1** | **Seed Audit**（15 seeds/65 节点预标注，43% 待人工） | `1746cf5` | ✅ |

### 1.2 核心数字速览（对外引用口径）

- **RouterBench v0.2**（maintenance）：llmrb test saving 13.46% [12.20, 14.73]、retention 1.0005、weak precision 78.6%、matched random 双杀 CI 不含 0；RouterBench 0-shot 侧 rescue 不可行（模型代际边界）。
- **Workflow Track v0.1**：O-0~O-5 全绿；PlanningBench 467 + PlanBench 1901 落库；scheduler regret = 0。
- **SystemBench v0.1**：80 case / 327 节点 exact-bound 100%；routing gain（workflow 级）cost −14.95% CI [−26.59, −5.16]；Coordination Tax 18.35（质量优先 vs 独立成本最优的代价）；O1 oracle completion +12.31pp [4.62, 20.00]。
- **S11 pilot**：真 decomposition 15/15 通过（4.33 节点均值），成本实测 $0.02。

### 1.3 S12 剩余步骤（23 号 §三十九）

```text
S12.2 扩 30 semantic workflows          ← 需决策 D1（binding 来源）
S12.3 artifact_contract/dependency/completion 人工标注  ← 需决策 D2（标注执行方式）
S12.4 Band Coverage Gate（生产 seam 实测）
S12.5 System Pool Audit（R7 在 semantic 节点上是否仍互补）
S12.6 Semantic Outcome Matrix           ← 需决策 D3（花钱采集）
S12.7 v0.2 candidate freeze
→ S13 L2 Historical → S14 v0.2 Main → S15 Live
```

---

## 二、S12.0-S12.1 产出明细

**Annotation Spec v1**（`workflow_annotation_v1.yaml`）五项契约：

1. **semantic_role 十词表**：calculation / verification / extraction / classification / retrieval / transformation / synthesis / reasoning / execution / search；每节点恰一主 role，扩展必须 versioned；
2. **atomicity**：描述出现并列动词（且/并/再/然后）即拆分，复合节点禁入 v0.2 headline；
3. **dependency_semantics 三值**：independent（L1 counterfactual）/ artifact_dependent（L2）/ state_dependent（L3）；
4. **artifact_contract**：`produces/consumes` 条目（type + required；consumes 带 from），一致性校验（consumes.from == edges 投影、consumes.type 可由上游 produces 满足）；
5. **completion_condition**：required/optional 显式化 + Band Coverage Gate（routing-relevant case 需 ≥1 weak/mid-eligible 节点）。

**Seed Audit**（`semantic_seed_audit.json`）：15 seeds / 65 节点关键词启发式预标注——calculation 13 / verification 7 / execution 5 / search 5 / extraction 4 / reasoning 3 / **unclassified 28**；13/15 workflow 含待人工节点；dependency_semantics 全部 artifact_dependent（planner 输出全有 depends_on）；structure（depth/width/density）已统计。

---

## 三、需要决策的三个问题（详细）

### D1 · S12.2 的节点 sample 来源（决定 workflow 的语义真实性上限）

**决策点一句话**：扩 30 semantic workflows 时，新增节点的 prompt 从哪里来？

| 方案 | 机制 | 优点 | 缺点 |
|---|---|---|---|
| **D1-a 再采 decomposition** | 对 30 个新 root_goal（PlanningBench 未用实例）再跑生产 decompose（qwen-plus，~$0.04），节点 prompt = 子任务 description（真实但无 frozen outcome） | prompt 语义真实（真 decomposition 产物）；workflow 形态真实；S11 管线直接复用 | **节点无 frozen outcome** → binding mode 只能 `synthetic_fixture`/`live_only`，Semantic Outcome Matrix（S12.6）必须真实采集（花钱，D3 联动） |
| **D1-b 继续用 llmrb query 拼装** | 节点 prompt 继续从 llmrb 真实 query 选（builder 现路径），workflow 结构按 semantic 模板重排 | 每节点有 frozen outcome（零采集成本，L1 counterfactual 立即可用） | **结构 synthetic 未变**——23 号 §六 明确批评"结构合法≠语义真实"，v0.2 的核心升级点落空 |
| **D1-c 混合**（推荐） | 30 workflows 分两组：15 个 D1-a 真 decomposition（outcome 真实采集，headline 候选）+ 15 个 D1-b llmrb 拼装（diagnostic，验证 builder/管线扩展性） | 兼顾语义真实性与成本；两组对照本身可量化"真 decomposition vs 拼装"的形态差异 | 管线两条路径，工程量约 +30% |

**推荐 D1-c**。理由：23 号 §六 把"语义真实"定为 v0.2 的核心升级点，纯 D1-b 会让 S12 失去意义；纯 D1-a 则把全部筹码压在 S12.6 的花钱采集上（若 pool 在 semantic 节点上失去互补性——23 号 §七风险——成本沉没）。混合方案用 15 个拼装 case 保底验证管线，15 个真 decomposition case 作为 headline 候选，风险分摊。成本增量：D1-a 部分 ~$0.04（decomposition only，不含 outcome 采集）。

**追问点**：若选 D1-c/D1-a，S12.6 的 outcome 采集规模（节点数 × 3 模型）在 D3 一起定。

---

### D2 · 43% unclassified + atomicity 人工标注的执行方式

**决策点一句话**：65 节点中 28 个 unclassified + 全部节点的 atomicity 判定，谁来标、按什么流程标？

背景：关键词启发式对中文 planning 描述覆盖率仅 57%（unclassified 28/65）；PlanningBench 参考语义已证实与 decomposition 不同类（S11 结论），**无 gold 可自动对齐**。

| 方案 | 机制 | 优点 | 缺点 |
|---|---|---|---|
| **D2-a 人工全量标注** | 你（或指定人）按 spec 对 28 unclassified + 全节点 atomicity 逐个标注（预计 30-60 分钟） | 质量最高；spec 的人机分歧样本可反哺词表 v1.1 | 占用你的时间；spec 首版可能需返工 |
| **D2-b LLM 辅助标注 + 人工抽检** | 先用 qwen-plus 按 spec few-shot 预标注（零成本红线例外，~$0.01），你只抽检 20% + 复核分歧 | 你的时间成本降到 ~10 分钟；标注一致性可量化 | 引入标注模型偏差；spec v1 未经校准就大规模应用 |
| **D2-c 先降级** | v0.2 先只收 unclassified=0 的 workflow（37 节点/约 8 case），unclassified 节点多的 workflow 进 diagnostic | 零人工零成本 | 30 workflow 目标缩水到 ~8-10 个，v0.2 规模不足（23 号 Stop S12 要求 ≥20） |

**推荐 D2-b**。理由：spec 首版的价值恰恰需要通过"LLM 标注 vs 人工抽检"的分歧率来验证——这本身就是 S12 的标注质量数据；你的介入点收敛到抽检与分歧仲裁，符合"标注半人工"的原设计（19 号 §六 原话）。D2-a 作为 D2-b 分歧过大（抽检不一致率 >30%）时的回退。

**追问点**：LLM 辅助标注用哪个模型（建议沿用 qwen-plus 保持与 decomposition 同源）；抽检比例 20% 是否合适。

---

### D3 · Semantic Outcome Matrix 的采集规模与时机（花钱决策）

**决策点一句话**：S12.6 需要为 semantic 节点采集真实 model outcome（节点 × R7 三模型），这是本阶段唯一大头成本——规模与时机怎么定？

背景：D1-a/D1-c 的真 decomposition 节点**没有 frozen outcome**（23 号 §六 S12-D 明确禁止 fuzzy matching），必须真实采集才能让 semantic workflow 进入 routing headline。成本模型：节点数 × 3 模型 × （平均 ~600 in + ~900 out tokens）× qwen-plus/mini 单价。

| 方案 | 机制 | 成本估算 | 优点 | 缺点 |
|---|---|---|---|---|
| **D3-a 全量采集** | 15 真 decomposition workflows × ~4.3 节点 × 3 模型 ≈ **194 采集点** | ≈ **$0.5-1.5**（取决于节点 token 长度；PlanningBench 规划类输出较长） | Semantic Outcome Matrix 完整；Gate Stop S12 全条件满足；v0.2 routing headline 有充分数据 | 一次花钱最多（但绝对量小） |
| **D3-b 阶梯采集** | 先采 5 workflows（~65 点，≈$0.3-0.5）做 System Pool Audit 前置检查——若 R7 在 semantic 节点上互补性消失（oracle_gain≈0）即止损，避免沉没 | ≈ **$0.3-0.5** 起步，按审计结果决定是否续采 | 直接吸收 23 号 §七风险（"RouterBench Best Pool ≠ SystemBench Best Pool"）；成本风险最小 | 两阶段拉长排期；audit 本身也花小钱 |
| **D3-c 延后** | v0.2 先只做 annotation+结构（D1-b 路线），outcome 采集整体推迟 | $0 | 零成本 | semantic workflow 无 outcome = routing headline 缺位，v0.2 退化为形态研究 |

**推荐 D3-b 阶梯采集**（与 D1-c 配套：只对 15 个真 decomposition workflows 采集）。理由：23 号 §七 明确把"R7 在 semantic 节点上失去互补性"列为中概率高风险——先用 5 个 workflow 的采集验证 pool 假设，成立再放量，是对"确定性优先"原则的成本侧对齐；且 23 号 §十七 本身就建议 S12 分两阶段（S12-1 结构 / S12-2 筛选后采集）。绝对成本上限 $1.5，在既有授权模式内。

**追问点**：采集用 qwen-plus 一档还是 R7 三模型按其 llmrb 档位映射（deephermes/mini 档用 qwen-turbo、intern 档用 qwen-plus）——**建议后者**（保持三模型成本/能力梯度真实），但单价需按官方页重录（23 号 §十 unknown cost 规则）。

---

## 四、三个决策的依赖关系与执行顺序

```text
D1（sample 来源）─┐
                  ├─→ D2（标注执行）→ S12.2-S12.3 零成本推进
D3（采集规模）────┘
        │
        └─→ S12.4 Band Coverage（生产 seam 实测，零成本）
        └─→ S12.5 System Pool Audit（依赖 D3-b 首批采集）
        └─→ S12.6 Outcome Matrix（依赖 D3 全量决策）
        └─→ S12.7 v0.2 candidate freeze → 汇报 → S13
```

- **D1 和 D3 必须一起决策**（D1 决定需要采集的节点量，D3 决定采集规模）；
- **D2 独立**（标注对象在 D1 定案后才完全确定，但 spec 校准可先行）；
- 建议回复格式：`D1-c / D2-b / D3-b`（即我的推荐组合）或逐项改。

## 五、决策后的推进预告（按所选组合）

以推荐组合（D1-c + D2-b + D3-b）为例：

1. **零成本段**：S12.2 建 15 llmrb 拼装 case（builder 扩展）→ S12.3 LLM 辅助标注 + 你抽检 → S12.4 Band Coverage（route-batch 实测 band 分布）；
2. **首次花钱**：S11 扩采 15 个新 decomposition（~$0.04）→ D3-b 首批 5 workflows outcome 采集（~$0.3-0.5）→ System Pool Audit → 审计通过则续采至 15 workflows 全量（≤$1.5）；
3. **S12.7**：v0.2 candidate freeze（新 systembench_id `lloom-systembench-v0.2-candidate` + manifest）→ 汇报后进 S13。

全程预计新增成本上限 **$2.0**，每个花钱点前按协作惯例单独通报。

---

## 附录 · 当前待人工工作量明细（D2 参考）

- 28 个 unclassified 节点（D2-b 下由 LLM 预标注、你抽检）；
- 65 个节点的 atomicity 判定（`atomic_ok` 字段，逐节点 true/false + 理由）；
- 15 个 workflow 的 completion_condition 复核（required/optional 是否需要非默认值）；
- artifact_contract 类型抽检（role_to_artifact 映射的准确性）。

预计 D2-b 下你的净介入时间 ≈ 10-15 分钟（抽检 13 个节点 + 仲裁分歧）。
