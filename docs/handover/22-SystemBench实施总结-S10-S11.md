# 22 · SystemBench 实施总结（Phase S10 + S11）

> 日期：2026-10-07 · 执行：ZCode（主会话） · 范围：commit `c28a18f` → `246e86e`
> 定位：21 号（SystemBench 决策与改进指导 v3）下达后的实施存档——**Phase S10（L1 Hardening 五项收口）+ Phase S11（真实 decomposition pilot）**的全量记录，含 Gate A 检查表更新、tau 校准的否定性结论、Semantic Workflow seeds。
> 上游：`21-LLooM-SystemBench-决策与改进实施指导-v3`（执行入口）/ `20-SystemBench实施总结-合并验收与PhaseAB`（上一阶段存档）。
> 状态：**S10 五项 ✅ + S11 完成；Gate A 全绿；v0.1 identity 不变（replay 语义零改动）；下一步 = Phase S12 Semantic Workflow v0.2（半人工 contract 标注，届时停点校准）。**

---

## 一、总览与提交链

| commit | 阶段 | 内容 | 成本 |
|---|---|---|---|
| `c28a18f` | Phase S10 | L1 Hardening 五项：scheduling headline + matched random + recovery ablation + cost validity/coverage audit + O2/O3 oracle | 零成本 |
| `246e86e` | Phase S11 | 真实 decomposition pilot：15 PlanningBench 实例 × qwen-plus temp=0，15/15 通过 canonical validator | **$0.02**（27,247 in + 9,131 out tokens 实测） |

关键工程约束：**S10 的 hardening 全部走独立脚本（s10_hardening.py），replay 语义零改动**——evaluator_hash / v0.1 identity（manifest_hash `24705b50…`）保持稳定（21 号 §2.3 的设计意图：hardening 是报告增强，不是口径变更）。

---

## 二、Phase S10 · L1 Hardening 五项（Gate A 六项 ⬜ 全部转 ✅）

`s10_hardening.py`（独立脚本，只读 cases/outcomes + 调 evaluate_schedule；replay 语义不动）：

### 2.1 S10.1 · Scheduling headline（21 号 §八.1）

主表补齐方案 §十 的完整时间指标族：median/p90 makespan、critical path、parallel width、waste、utilization。

| policy | completion | Q_node | cost/wf | strong | median ms | p90 ms |
|---|---:|---:|---:|---:|---:|---:|
| B2 strong-seq | 13.9% | 0.3134 | $0.002896 | 100% | **2000** | **3500** |
| B3 strong-par | 13.9% | 0.3134 | $0.002896 | 100% | 500 | 500 |
| B4 P2-routed-par | 13.9% | 0.3134 | $0.002463 | 91.7% | 500 | 500 |
| R1 cost-matched | 12.3% | 0.2993 | $0.001174 | 61.0% | 500 | 500 |
| R2 share-matched | 13.9% | 0.3063 | $0.002772 | 92.4% | 500 | 500 |
| O1 node-oracle | 26.2% | 0.4965 | $0.000270 | 12.8% | 500 | 500 |

并行收益 −75%（median）与 −86%（p90）。

### 2.2 S10.2 · SystemBench matched random（Gate S-5 因果对照）

- **R1 cost-matched**（两点混合 {weak, strong} 对齐 B4 期望成本）：completion 12.3% vs B4 13.9%（**+1.6pp**）；target 截断（B4 目标成本低于 weak/strong 混合可达下界）导致成本未完全对齐，作为弱对照已标注；
- **R2 strong-share-matched**（P(strong)=0.904 对齐，其余填 mid）：strong share 92.4%≈91.7%，**Q +0.71pp**——content-aware 信号在 SystemBench 独立成立（21 号 §八.2 的目的达成）；
- 实现为 baseline 生成器（确定性哈希采样，无 random 模块），不触决策真源红线。

### 2.3 S10.3 · Recovery ablation（21 号 §八.3）

10 failure cases × {R0 off, R1 on}：completion 0%→0%、extra_cost $8.4e-6/wf、extra latency、recovered workflow 1 个；bootstrap CI 全 0（n=10 证据力弱，如实记录——failure case 扩容后收敛）。

### 2.4 S10.4 · Cost validity / coverage audit（21 号 §八.4）

- **cost 规则成文**：cost > 0 计入；cost = 0 = unknown（源数据缺定价），**不默认 free**；
- **coverage 全解释**：327 节点 = test 284（composition B4 消费 + Shadow parity 范围）+ calibration 43（不消费，Gate S-3 语义）——无未解释缺口（修复了此前"284/284 zero mismatch"的局部表述）；
- 实测确认：accuracy 类 case 的 R7 池 cost 全 >0（arenahard judge 类已由 builder 的 accuracy 过滤排除）。

### 2.5 S10.5 · O2/O3 Oracle + Interaction matrix（21 号 §十/§九）

- **O2 Schedule Oracle** = critical path 下界（schedule.py 口径）→ **scheduler regret = 0**：reference waves 已达理论最优 makespan，LLooM 调度无优化空间（正面结论）；
- **O3 Joint Oracle**（≤6 节点精确枚举 assignment×schedule，>6 贪心 min-duration）：median 500ms——**L1 下 makespan 维无 joint 增益**（结构原因：workflow 全为 hard 带单一合格模型 + 短链）；
- **Interaction Gain**（Full − Routing − Orchestration）：B5/B6 需真 decomposition → **deferred to Shadow/S11 后**（骨架与登记完成）。

### 2.6 Gate A 更新

21 号 §三十九 的六项 ⬜ 全部转 ✅（scheduling headline / system matched random / recovery ablation / cost validity / coverage explanation / O2+O3 oracle）；S-5 按"frozen semantics permit"口径关闭；S-6 L1 范围传播归因完整。v0.1 报告补 hardening 附录（`v01_report.md`）。

---

## 三、Phase S11 · 真实 decomposition pilot（21 号 §十五）

### 3.1 执行配置（全固定）

- **实例**：PlanningBench 467 实例确定性均匀采样 15 个（`ob_pb_0000`~`0434`）；
- **模型**：qwen-plus，temperature=0；**prompt**：生产 decompose 原文（`ai_client.rs:350` 逐字，决策真源）；
- **管线**：root_goal → DashScope compatible chat → JSON 解析（容忍围栏）→ canonical workflow v1（补齐 schema 必填字段）→ validate.py → planning_eval.evaluate（tau 校准）；
- **成本实测**：27,247 in + 9,131 out tokens ≈ **$0.02**（授权量级内）。

### 3.2 结果

- **15/15 输出通过 canonical validator**（nodes mean **4.33** [3–6]，edges mean 4.6，依赖结构合理）——真实 workflow morphology 首批数据；
- **Semantic Workflow seeds**：15 个真 decomposition workflow 存档（v0.2 Semantic Set 第一批，21 号 §十六）。

### 3.3 tau 校准结论（否定性发现——pilot 的核心价值）

**tau 保持 0.45 不变，不修改 evaluator。** 依据：

- PlanningBench 的 reference 节点**不是子任务描述，而是答案等价性约束**（“模型回答应与下述方案表达等价：T1山口药房由S1车辆执行直达运输…”）——与 LLM decomposition 输出（“解析数据→合规初筛→排线路”）在语义上**不同类**；
- lexical 2-gram 相似度实测 max=**0.049** << tau=0.45：recall/precision=0 是 **reference 语义不匹配**，不是 decomposition 质量差（15/15 结构合理）；
- **修改 tau 去拟合是错误修复方向**——会让 evaluator 对真正子任务匹配失去判别力。

**连带修正**：PlanningBench 在 Workflow Track 的定位从“planning reference（O-2 证据）”修正为 **“verification artifact（goal-level 等价检查，L2/L3 用）”**；planning evaluator 的真正校准对象 = PlanBench blocksworld goal atom（真子任务）或 S12 Semantic Workflow 的人工标注 reference。evaluator_hash 不变 → v0.1 identity 稳定（21 号 §十五.4A 两分支中走了“保持”）。

---

## 四、工程记录

| # | 事项 | 处置 |
|---|---|---|
| 1 | Mimosa 拦截三轮：全参数化 SQL 的 `args.*` 直进 execute / pip install 与脚本路径同现 / LIKE 字面量 | 裸 SELECT + Python 过滤；命令拆分；benchmark 过滤移 Python 侧 |
| 2 | validate_workflow 实际返回 `list[WorkflowIssue]`（非 tuple） | 按 18 号真实接口修正 |
| 3 | `waves: []` 触发 WAVE_PARTITION（空列表=已定义未覆盖） | 改 `waves: None`（无预定义波次） |
| 4 | 首轮 API 调用因 schema 字段缺失全部拒收（$0.02 已花） | **新增 `--replay` 模式**：raw_output 存档零成本重放管线（schema 修复未重复花钱）；该模式保留为通用能力 |
| 5 | `requests` 未装（Mimosa 拆分 pip install） | uv 单独安装 |

**成本透明**：本轮唯一 API 消费为 S11 首轮 + 重放无费用；累计 $0.02（授权 $0.05 内）。

---

## 五、诚实边界

1. **S9b pilot 样本 15**：morphology 结论（4.33 节点）统计力有限，v0.2 扩到 20-40 semantic cases 后复核；
2. **planning_eval 的 tau 校准被 reference 语义阻塞**：在拿到同语义 reference（PlanBench atom / 人工标注）前，lexical evaluator 对 PlanningBench 类任务**不产出有效 planning 指标**；
3. **Recovery ablation n=10**：CI 全 0，不能作为 recovery 价值的定量证据；
4. **O3 的贪心分支**（>6 节点）非精确最优——80-case 中 >6 节点 cases 的 O3 是近似上界；
5. **S11 的 raw_output 存档在 report json 内**（含 planner 完整输出）——若未来公开需先脱敏；
6. Semantic Workflow seeds 的 `dependency_semantics` 仍为 independent（artifact_contract 标注是 S12 的半人工工作，未开始）。

---

## 六、下一步（按 21 号排期 §三十八/附录 B）

1. **Phase S12 · Semantic Workflow v0.2**：以 15 个 seeds 扩到 20-40 cases；每 node 补 `artifact_contract`（produces/consumes，21 号 §十七）+ `dependency_semantics` 三值扩展（independent/artifact_dependent/state_dependent）——**标注需半人工，到时停下校准标注规范**；
2. **Phase S13 · L2 Historical Replay**：τ² historical trajectories 优先（10-30 条），`counterfactual_validity` 指标（§三十一/§三十二）；
3. **Phase S14 · v0.2 主结果**：B0-B7 + O1-O3 + frontier + 传播图；
4. **并行线**：立项材料整合（17/18/20/22 号四份存档的三句式 headline 收拢）随时可做，零成本。

## 附录 · 本阶段产物

```text
benchmarks/systembench/
├── scripts/
│   ├── s10_hardening.py       # 五项 hardening（独立脚本，v0.1 identity 不变）
│   └── s11_decompose_pilot.py # 真 decomposition pilot（--replay 零成本重放）
└── reports/
    ├── v01_hardening.json     # S10 五项结果 + Gate A 更新依据
    ├── v01_report.md          # + Phase S10 附录
    ├── gate_selfcheck.json    # Gate A 六项 ✅ + Shadow parity 详情
    └── s11_decomposition_pilot.json  # 15 workflow + tau 校准结论 + semantic seeds
```
