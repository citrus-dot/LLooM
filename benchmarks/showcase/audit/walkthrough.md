# Walkthrough — 一条 workflow 的完整证据链（demo case）

> 贯穿式演示（34 号 §六 展示 5）：从 root goal 到 workflow completion proxy，
> 每一步标注「**真实生产逻辑** / **离线回放** / **仿真或人工标注**」三种身份。
> demo case：`sb_v02_sem_030`（calibration split，6 节点，search/synthesis/verification 链）。
> 机器可读版：`audit/demo_case_sb_v02_sem_030.json`。

## 链路总览

```text
[1 真实生产逻辑]  root_goal（用户任务）
[2 真实生产逻辑]  decomposition（生产 prompt → 语义节点 DAG）
[3 真实生产逻辑]  annotation（role/atomicity/dependency/artifact contract；LLM 预标 + agent 复核，semantic gold 未人审）
[4 真实生产逻辑]  routing_input / execution_input 构造（R1 headline / R2 diagnostic / 执行注入）
[5 真实生产逻辑]  route decision（bench::route_single → router::plan()，与生产同链路）
[6 离线回放]      node outcome（frozen replay：score/cost 查表，零新推理）
[7 仿真]          schedule（duration-profile：completion_tokens × 2ms，非墙钟）
[8 离线回放]      completion proxy（replay 成功判定：required 节点 succeeded + 依赖满足 + 无非法并行）
[9 真实生产逻辑]  failure attribution（recovery 状态机 + P1-P11 归因；v0.1 failure 10 case 为注入样本）
```

## 逐步明细（sb_v02_sem_030）

### [1-2] root_goal → 语义节点

root_goal（节选）：「…大连三天两夜游玩…需一份能直接执行的方案…」。
生产 decompose prompt（`ai_client.rs` 原文，qwen-plus temp=0）产出 6 节点 DAG；
本 case 的节点均通过 canonical validator。

### [3] 标注身份（重要边界）

每个节点的 `semantic_role` / `artifact_contract.consumes` 由 LLM 预标注 + agent 复核 + 机械校验
（consumes.from 与 edges 投影一致、依赖语义修复幂等归零）。**semantic gold 尚未 100% 人工签收**
（正式 v0.2 gate 项，见 claims_and_limits §禁用 6）——本 demo 不把它描述为 approved gold。

### [4] 输入构造（R1/R2/execution 三段分离）

| 节点 | role | dependency | R1 band | execution_input |
|---|---|---|---|---|
| nC1 | extraction | independent | medium | base_text（root_goal+子任务） |
| nC2 | search | artifact_dependent | hard | final_text（base + 冻结 artifacts） |
| nC3 | reasoning | artifact_dependent | hard | final_text |
| nC4_s1/s2/s3 | verification | artifact_dependent | hard | final_text |

hash 逐节点记录在 demo JSON（routing_input_hash / execution_input_hash；
artifacts 冻结自 stage0 参考执行 qwen3.6-plus）。**R1/R2 是两套 routing 口径，不混算**
（headline=R1 description；Compression=0.1948 见 showcase_report §5.2）。

### [5] route decision

`bench::route_single → router::plan()`（P2 Frozen = `lloom@softgate0`，frozen manifest
policy `P2_softgate0`）——与生产**同一决策链路**，零公式复制。band（R1）如上表；
hard band 在 P2 下仅 strong 合格，medium/easy 全池 eligible。

### [6] node outcome = frozen replay

score/cost 来自 llmrb 冻结矩阵查表（exact binding，prompt_hash 校验），**不产生新推理**。
semantic 层另有 L1 matrix（exploratory，分层审计见 `audit/semantic_l1_comparability.csv`），
与本 v0.1 replay 是两条独立证据，不合并。

### [7] schedule = 仿真

makespan 用 duration-profile（completion_tokens × 2ms）模拟：
B2 seq mean 2184.6 ms（median 2000 / p90 3500）→ B3/B4 par 500 ms。
**这不是线上 wall-clock latency**（claims 禁用 3）。

### [8] completion proxy = replay 判定

全 required 节点 succeeded + 依赖满足 + 无非法并行。v0.1 test 65 workflows：
B2/B3/B4 = 13.9%，O1（事后上界）= 26.2%。**不是真实 artifact 传播后的端到端成功率**。

### [9] failure attribution = 真实生产状态机（注入样本）

F1-F7 注入 + recovery 状态机 + P1-P11 归因（OrchestrationBench 基础设施）。
v0.1 的 failure 10 case 仅 1 例恢复——样本小，只证明状态机 works，不证明 recovery 改善成功率
（34 号 §4.2 同款边界）。

## 一句话总结

这条链上，**[1]-[5][9] 是真实生产代码路径，[6][8] 是冻结离线回放，[7] 是仿真**——
showcase 的全部 headline 数字都产生于这个身份明确的组合，而非 live E2E。
