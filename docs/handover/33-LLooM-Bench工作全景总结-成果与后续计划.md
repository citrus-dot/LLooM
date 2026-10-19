# 33 — LLooM Bench 工作全景总结：成果、证据与后续计划

> 汇总日期：2026-10-10。范围：2026-10-05 开工至今的全部 benchmark 工作（RouterBench / OrchestrationBench / SystemBench / Showcase）。
> 依据：git 历史 35 个 bench commits、27-32 号决策与工作记录、冻结报告与 showcase 包。
> 当前状态：**Benchmark evidence audit & showcase freeze 完成 → maintenance**。

---

## 一、三条 Track 一览

```text
RouterBench v0.2（routing track，冻结）
    回答：query 级路由的成本/质量取舍与统计强度
OrchestrationBench v0.1（workflow track，offline 基础层）
    回答：planning/scheduling/failure 的离线评测基础设施
SystemBench v0.1（composition track，冻结）+ S12 semantic snapshot（exploratory）
    回答：路由+调度+编排组合后的系统级净效果；语义层原型发现
        ↓
Showcase Freeze（2026-10-09，commit 746c3d7）
    现有证据的审计、封板与展示包 → maintenance
```

三条 track 各自独立报告、不合并总分；fidelity 分层（offline frozen → semantic exploratory → 未来 live）贯穿全部口径。

---

## 二、RouterBench：v0.1 → v0.2（冻结主证据）

### 2.1 建成的基础设施

- **数据层**：LLMRouterBench 27,034 实例 × 541,152 outcomes 双轨导入（raw/normalized，大文件外置盘 `/Volumes/Files/LLooM-bench/`，仓内 symlink）；RouterBench 0-shot 36,497×11=401,467 outcomes；哈希桶 split（20/80，seed 3407）导入时生成。
- **Rust 决策真源**：`bench.rs`（lloom-core）——8 策略回放引擎 + 决策单源 = 生产 `router::plan()` 零公式复制；`lloom-cli bench replay|sweep|route-batch`；production/benchmark parity test。
- **冻结 identity**：`lloom-routerbench-v0.2`（manifest_hash 七 hash，含 policy_hash）。

### 2.2 主结果（test 12,345 实例，实例级 paired bootstrap B=10k seed=3407）

| 策略 | Quality | Cost/query |
|---|---:|---:|
| Always Strong | 0.5860 | $0.000948 |
| **Best Single** | **0.6053** | **$0.000674** |
| **LLooM P2 Frozen** | 0.5863 | $0.000821 |
| Cost-matched Random | 0.5610 | $0.000819 |
| Strong-share-matched Random | 0.5749 | $0.000899 |
| Oracle（不可部署） | 0.7122 | $0.000118 |

> **勘误（2026-10-10，34 号 §2.4）**：本表初版曾把 P2 Frozen 误写为 0.5867/$0.000849——那是 `lloom`/`band_only` run（P0 Current/BandOnly，selection=deepseek+intern）的数字。真正的 P2 Frozen 是 **`lloom@softgate0` = 0.5863/$0.000821**（selection=deephermes 1182 + intern 11163；frozen manifest policy=`P2_softgate0`；cost-matched random 的 $0.000819 与之匹配亦为佐证）。34 号核验发现，已修正本表与 showcase 图/CSV/报告。

- P2 vs Always-Strong：**saving 13.46% [12.20, 14.73]**；retention 1.0005 [0.9957, 1.0052]（含 1.0）
- P2 vs 匹配随机：**+2.52pp [2.07, 2.97]** / **+1.14pp [0.75, 1.53]** —— content-aware 实证
- **诚实边界：Best Single 点估计支配 P2**（质量更高且成本更低）——P2 未击败 calibration 选出的最强单模型，这是下一阶段真正的研究问题

### 2.3 路线上的关键研究发现

- **Weak Rescue 诊断**：V0 下 Weak share 0% = 100% 被 band gate 挡住（Blocked Weak）；Soft Gate（P2）后 weak 可被选择。**注意区分口径**：calibration tournament 阶段 weak precision 0.809（Day 9，run 8-27）≠ frozen test 的 0.786（CI [0.7629, 0.8086]）——前者是 calibration 池选择依据，后者才是 v0.2 headline 统计。
- **换池实锤（Case B）**：R7 池（deephermes/r1-distill/intern-s1-mini）上 P2 达成 Pareto 改进；RouterBench 2024 世代矩阵 weak rescue 不可行（代际差距 0.35，cost 权重翻不动）。
- **E5 负结果**：静态矩阵下 EWMA quality_override 零增益——该机制的舞台是 Native 漂移场景。
- **Cross-benchmark 边界**：机制一致（gate 因果）、幅度由模型代际决定——「机制存在且可解释边界」的两源对照叙事。
- **Native hold**：生产池自采挂起至立项需要。

---

## 三、OrchestrationBench：offline 基础层（v0.1）

- **全链建成**（O-Day 9-14，7 commits）：canonical workflow schema + validator（错误码=归因键）；PlanningBench 467 全量（CC-BY-4.0，3379 obligation 节点）+ PlanBench 1901（PDDL parser）；确定性 planning evaluator（2-gram Jaccard，tau=0.45 校准）；四策略调度模拟器 + §十指标族；failure 注入 F1-F7 + 确定性 recovery 状态机；P1-P11 case-level 归因；`orchbench.db` 落库。
- **Router bridge**：`obx__{wf}__{node}` sample_id 契约，决策真源留 Rust 侧。
- **Gate O-0~O-5 全绿**（O-6 后置）；Python 54 测试。
- **诚实边界**：evaluator 阈值未经 online 校准；基础设施层，不独立造 headline。

---

## 四、SystemBench：v0.1 Frozen + S12 semantic snapshot

### 4.1 v0.1 Frozen Composition（主证据，Tier A）

- **设置**：80 cases（test 65）/ 327 节点；节点 prompt = llmrb 真实 query exact binding；node outcome 全 frozen replay（零新推理）；调度 = duration-profile 仿真；workflow-group paired bootstrap（unit=system_case）。
- **结果**：

| 策略 | completion proxy | Q_node | cost/workflow | strong share | sim. makespan |
|---|---:|---:|---:|---:|---:|
| B2 Strong+Seq | 13.9% | 0.3134 | $0.002896 | 100% | 2184.6 ms |
| B3 Strong+Par | 13.9% | 0.3134 | $0.002896 | 100% | 500 ms |
| B4 P2+Par | 13.9% | 0.3134 | **$0.002463** | 91.7% | 500 ms |
| O1 Node Oracle | 26.2% | 0.4965 | $0.000270 | 12.8% | 500 ms |

  - **路由效果（B3→B4）**：cost **−14.95% CI [−26.59, −5.16]**，completion/Q_node 点估计不变
  - **调度效果（B2→B3）**：模拟 makespan 2184.6→500 ms（非墙钟）
  - **Coordination 指标**：frozen 字段 18.347（ratio-minus-one）→ 派生展示 **19.35× / +1834.7%**（median 5.11，右偏）
  - S10 Hardening：scheduler regret=0（O2=critical path）、matched random 双对照、cost validity 审计
- **边界**：completion proxy 是 replay 判定，非真实 artifact 传播的 E2E success。

### 4.2 S12 semantic snapshot（exploratory，Tier B/C）

**结构与协议（已冻结，可复用）**：
- 真实 decomposition seeds：生产 prompt 15+15 实例 → 30 semantic workflows / 175 节点（`system_cases_v02.jsonl`）
- annotation spec v1（semantic_role 十词表 + atomicity + dependency_semantics 三值 + artifact_contract）
- 22/126（17.5%）复合节点按 spec 拆分；依赖语义修复（18 sem 升级 + 14 dangling 重映射）幂等归零
- Routing/Execution Input Contract v1：**R1 description（headline）/ R2 full_prompt（diagnostic）分离**；175 节点三段 hash sidecar
- **Routing Context Compression = 0.1948**（R2/R1 eligible 比 8.6%/44.0%——更多上下文≠更多可选模型）
- 双 generation profile 定案：**P1536 = Budget Track / P4096 = Capability Sensitivity Track**（29 号 REVIEW 条款）
- Provider Probe PASS + P2 官方价格重录（manifest v2）

**数据矩阵（exploratory）**：
- **S12.5.2 Semantic Probe**：72 cells（24 节点 × 3 模型），$0.54，headroom 0.179 / cheap-safe 79.2%
- **S12.5.4 Protocol Stability**：REVIEW（winner agreement 4/6；truncation 94.4%→55.6%；tier 排序翻转）——budget 是实质性 routing variable
- **S12.6-A L1 Matrix**：33 independent 节点 × 3 模型 × 双 profile = 198 cells 全采；judge 197/198；双 profile winner agreement **0.500**（budget 效应复核成立）；pool 互补性两口径均在（entropy 1.43/1.51）；cheap-safe 68-73%
- **S12.6-B L2 partial**：119/180 输出 / 49 已评；方向性信号：artifact 注入下 strong 占优（与 L1 相反）——artifact-conditioned routing 假设来源
- **同质性限制（已审计分层）**：mid 代际替换（qwen3.6-plus→qwen3.7-plus）、judge 换档（flash 113 / max 84）、178/198 truncated——逐 cell 身份审计 CSV（198×26 字段）+ 11 视图分层，**无 pooled headline**

### 4.3 Showcase Freeze（2026-10-09，Tier 分级封板）

`benchmarks/showcase/`（19 文件，零 API 生成）：
- 主报告（三层叙事）+ claims_and_limits（C1-C7 可引用 / 7 条禁用）
- 三张核心图（quality-cost 含 Best Single 支配关系 / composition 拆 cost 与 simulated makespan / protocol sensitivity）+ figure_data CSV 真源
- audit/：source inventory（13 文件 SHA）、comparability CSV、L2 coverage、Coordination 派生、demo case、离线再生校验 **30/30 PASS**
- **Stop Gate 达成 → maintenance**

---

## 五、方法学与工程资产

1. **统计纪律（全 track 统一）**：workflow-group paired bootstrap、unit=system_case、B=10k、seed=3407；cost=0 → unknown 不进 Oracle/Tax/frontier；exact binding 硬规则（禁止 fuzzy/nearest 进 headline）；split 泄漏纪律（pool selection/阈值只读 calibration）。
2. **决策真源零复制**：全部 routing 决策调 `bench::route_single → router::plan()`，生产与 benchmark 永远同链路。
3. **Evidence Tier 制度**：A 冻结主证据 / B 诊断（exploratory）/ C partial 存档 / D 未测——每图每表标注。
4. **Identity 版本化**：manifest hash 族（case/policy/pool/split/scheduler/judge/generation profile/cost basis）；换 judge = 新 rubric id；换池 = 新 pool_id；原始数据只追加，清洗生成派生文件。
5. **断点续采模式**：全部采集脚本每 cell 落盘 + 启动跳过已有 cells——额度中断不废数据（本轮三次验证）。
6. **生产 finding（候选回修，独立 issue P-01 待确认）**：生产 ai_client 对 qwen3.6 系列不传 `enable_thinking`（默认开 → 子任务截断 + 分钟级 latency）；`max_tokens=estimated` 有同款截断风险。
7. **实花合计约 $3.6**（S11 $0.02 + S12.5 $0.71 + S12.6 $2.8 + probe $0.17），全部免费 tier 消耗，无计费。

---

## 六、对外可引用的主结论（claims C1-C7 摘录）

| # | 结论 | 强度 |
|---|---|---|
| C1 | RouterBench v0.2 冻结 test：P2 省 13.46% 成本（CI [12.20,14.73]），质量保留 1.0005（含 1.0） | Tier A 冻结 |
| C2 | P2 显著优于两种匹配随机（+2.52pp / +1.14pp，CI 不含 0）——content-aware 实证 | Tier A 冻结 |
| C3 | SystemBench v0.1：路由使 workflow cost −14.95%（CI [−26.59,−5.16]），completion 点估计不变 | Tier A 冻结 |
| C4 | 并行调度使模拟 makespan 2184.6→500 ms（必须说 simulated） | Tier A 冻结 |
| C5 | 真实 decomposition 15/15 过 validator；17.5% 节点违反 atomicity | Tier B 诊断 |
| C6 | Output budget 是实质性 routing variable（probe 4/6 REVIEW；L1 16/32 descriptive） | Tier B 诊断 |
| C7 | Routing Context Compression = 0.1948：更多上下文≠更多可选模型 | Tier B 诊断 |

**必须同场呈现的限制**：Best Single 点估计支配 P2；L1 非 live；semantic matrix 协议异质；L2 partial；无 live E2E。

---

## 七、后续计划

### 7.1 Maintenance（当前默认态，32 号停止线）

只修影响复现与展示的 bug；不加新数据、不动冻结口径、不按路线图自动扩展。

### 7.2 待办（小项，等确认）

| 项 | 说明 | 等谁 |
|---|---|---|
| Production issue P-01 | enable_thinking/max_tokens 截断问题独立立 issue + 受控回归 | Nironta 确认 |
| 对外叙事材料 | showcase 包已是底稿；立项/答辩稿可从 claims C1-C7 直接引用 | 需要时事 |

### 7.3 重开实验的唯一入口（32 号 §11.2 触发器）

| 触发 | 动作 | 红线 |
|---|---|---|
| 评审问「P2 为什么没超过 Best Single」 | 先从 frozen traces 分析池/策略限制，写成 RQ | 不立即改 router、不付费跑新 benchmark |
| 需要可信 semantic model ranking | 预注册式确认实验：固定 model generation/judge/profile，只变指定变量，先定样本/预算/停止条件 | 不把异质 198 cells 改标题发版 |
| 产品出现 thinking/token 截断 | 独立 production bug + 受控 regression + 新 policy identity | 不回写冻结结果 |
| 需证明 artifact 改变模型选择 | L2 artifact replay RQ：source audit → 小样本 feasibility | 不为补完矩阵而补采 |
| 需证明真实 E2E 成功率/延迟/花费 | 另建 dynamic/live track（独立 held-out + 完整 traces） | 不把 simulated makespan 当 live latency |

### 7.4 更远期的研究门（全部有前置条件）

- **方向 A · 统一协议确认实验**：仅在评审认为 semantic headline 不足时；先冻结一个池/judge/profile。
- **方向 B · L2 Artifact Replay（研究升级）**：需主张 artifact-dependent counterfactual 收益时；source audit → feasibility → 小样本。
- **正式 SystemBench v0.2 release**：checklist 见 32 号 §十（100% human sign-off、O3 exact 全枚举、同质 headline、L2 完整 gate、显式 sign-off）。
- **State-aware Router v1（新内核）**：SystemBench v0.2 frozen + L1 baseline 完整 + 一组 L2 证据 + interaction 主结果之后；独立分支 `research/state-aware-router`，benchmark-only prototype → offline replay → Shadow → Live。
- **Live / τ² / AppWorld / WebArena**：S14 主结果成立后才考虑。

---

## 八、关键文件索引

| 资产 | 路径 |
|---|---|
| RouterBench v0.2 冻结报告 | `benchmarks/routerbench/reports/v02_test_report.json` |
| SystemBench v0.1 冻结报告 | `benchmarks/systembench/reports/v01_report.json`（+ `.md`） |
| v0.1 frozen manifest | `benchmarks/systembench/manifests/frozen_manifest_v01.json` |
| Semantic cases + contract | `benchmarks/systembench/cases/system_cases_v02.jsonl`、`manifests/routing_input_contract_v1.yaml` |
| L1/L2 矩阵 | `benchmarks/systembench/matrices/semantic_outcomes_{l1,l2}.jsonl` |
| 展示包 | `benchmarks/showcase/`（README → 入口） |
| 决策与工作记录 | `research/手记规划-20260928/`（17/18/20/22/24-32 号） |
| 外置盘原始数据 | `/Volumes/Files/LLooM-bench/`（未挂载时 bench 命令报错，重挂即恢复） |
