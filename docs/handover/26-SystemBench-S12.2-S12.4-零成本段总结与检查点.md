# 26 · SystemBench S12.2–S12.4 零成本段总结与检查点

> 日期：2026-10-07 · 执行：ZCode（主会话，自主闭环授权内） · 基线：origin/v2 @ `c97d688`
> 定位：S12.2–S12.4 完成存档 + **进入花钱段（S12.5/S12.6）前的检查点**——headline 口径决策 + 抽检确认 + 放行审批。
> 上游：`24-决策文档`（D1/D2/D3 已按 25 号 §四十八 定案 D1-c'/D2-b'/D3-b' 执行）/ `23-v4` / `25-v5`。
> 状态：**S12.2–S12.4 完成并推送；S12.5 起暂停，等本报告三个决策。**

---

## 〇、用户原话存档（交接消息逐字，2026-10-07）

> 「S12.2：扩 30 semantic workflows（按 D1 组合：真 decomposition 采新 root_goal
> ~$0.04 + llmrb 拼装对照）」
> 「S12.3：artifact_contract/dependency_semantics/atomicity 标注（按 D2：LLM 辅助
> 预标注 + 我抽检 20%，分歧>30% 回退人工）」
> 「S12.4：Band Coverage Gate（生产 route-batch 实测 band 分布；全 strong-only case
> 进 diagnostic 不进 routing headline）」
> 「汇报节奏：S12.2-S12.4（零成本段）完成回报一次；S12.5 Pool Audit 结果（决定是否
> 续采）单独回报；S12.7 freeze 后交完整报告」
> 「遇 23 号未覆盖的执行决策：实测优先、记录进报告、不阻塞；影响 headline 口径的
> 决策必须停下问我」

开工消息未含「D1-x D2-x D3-x」显式 token；按 25 号 §四十八 已定案（D1-c'/D2-b'/D3-b'）执行，未阻塞。

---

## 一、执行摘要

| 阶段 | 产出 | 状态 |
|---|---|---|
| S12.2 | 15 个新 decomposition（ob_pb_0032 validator 拒收，stride 补采 ob_pb_0465）→ semantic candidate pool = 15 S11 seeds + 15 新 = **30**；llmrb 拼装 control **10** | ✅ commit `c97d688` |
| S12.3 | qwen-plus 预标注（126 节点 role 全覆盖，0 unclassified）→ **22 复合节点按 spec §atomicity 拆分**（25 组，round2 再拆 3 组）→ agent 两轮复核 → **175 semantic 节点标注完成** | ✅ |
| S12.4 | 生产 route-batch seam 实测 band 分布，**双口径**：full_prompt vs description_only——headline 口径成为本次检查点核心决策 | ✅ |
| 质量门 | clippy -D warnings 0 错；cargo test **138/138**（基线保持）；binding 哈希全量校验通过 | ✅ |

**实际花费**：qwen-plus 共 ~115.7k in + ~47.7k out tokens（decomposition 32.7k/9.2k + 标注 72.8k/23.0k + 拆分 r1 8.9k/13.7k + 拆分 r2 1.2k/1.8k）。以账单为准量级 **≤$0.10**（ decomposition 授权 ~$0.04 + 标注 ~$0.01；拆分为 spec §atomicity on_violation 的强制处置，超出处已如实记录）。

---

## 二、S12.2 · v0.2 Case Set 构建

- `cases/system_cases_v02.jsonl`：**40 cases / 207 nodes**（30 headline_candidate + 10 control）；split 新算（sha256 lloom-sbsplit:3407:{case_id}，31 test / 9 calibration）——v0.1 Frozen 资产零接触，control 用 v0.1 未消费的 llmrb sample（无重叠）。
- semantic node **frozen prompt** = `root_goal + "\n\n---\n## 子任务\n" + description`（prompt_hash 已冻结）。
- node 级 dependency_semantics：independent 63 → 拆分后 **independent 65 / artifact_dependent 110**（拆分子节点默认线性链）。
- control 节点保持 `llmrouterbench_exact` 逐字绑定 + L1_strict；semantic 节点 binding mode = `semantic_matrix_exact`（source_id = case/node，matrix 于 S12.6 采集）。

### 执行决策记录（23 号未覆盖，实测优先）

1. **semantic node prompt 嵌入 root_goal**：生产 `ai_client.rs:526` 子任务 prompt 现状**只发 description，不嵌根目标数据**——真实 decomposition 的子任务描述普遍引用根目标数据（如「解析所有任务与车辆基础数据」），不嵌则节点不可执行。benchmark 层嵌入 root_goal 是使 outcome 矩阵有意义的必要适配。**已记为 production observation**：是否回修生产（把 root_goal 随发子任务）是独立决策，不属本阶段。
2. **artifact 注入后置 S12.6 stage 0**：artifact_dependent 节点的上游产物按生产「前置任务结果：」格式注入冻结参考产物（qwen-plus 参考执行），届时 prompt_hash 重算。S12.2 不采（控制在授权内）。
3. **拆分 provenance**：拆分子节点 `split_provenance.from_node` 指回原复合节点；`s12_composite_split.json` 25 组记录（**诚实边界**：前 22 组 raw_output 因报告覆盖缺陷未存档，已改为累积合并并自 case 状态重建，3 组再拆分带 raw_output）。

---

## 三、S12.3 · 标注（workflow_annotation_v1）

### 3.1 流程与数字

```text
qwen-plus 预标注（temp=0，每 workflow 一次调用，spec 全文 few-shot）
  → 126 节点 semantic_role 全覆盖（reasoning 51 / calculation 20 / extraction 17 /
     verification 15 / synthesis 12 / search 5 / transformation 3 / classification 2 / retrieval 1）
  → 确定性复核：词表封闭 ✓ / consumes.from==edges 投影（权威）✓ / contract 补齐 ✓
  → 22 节点 atomic_ok=false（16/30 workflow 含复合节点）→ spec §atomicity 强制拆分
  → 拆分 25 组（22 首轮 + 3 再拆）→ 175 节点
  → agent 两轮复核（verdict 存档 s12_agent_review.json）
```

### 3.2 复合节点发现（真实 decomposition 质量数据）

生产 planner 把「提取+计算+初筛」（sem_001/nsubtask_1）这类**多个独立动作打包进一个子任务**的比例：22/126 节点（17.5%），波及 16/30 workflow。这是 RQ4（workflow morphology）之外的第一条 **decomposition 质量发现**：生产 decompose prompt 要求「每个子任务独立、可执行」，但实际产出 ~1/5 节点违反单一职责。

### 3.3 Agent 复核结论（两轮，verdict 全存档）

- 首轮 22 组拆分：**20 组放行**；3 节点争议（残留复合 / 非法 role `allocation_and_verification`）→ 按 spec 循环再拆分。
- round2 再拆 3 组 → 9 子节点：7 放行；**2 节点「过拆」争议**（sem_023/n3_s3_s2、s3——段落级重排 vs 句内级重排可合并为一个 transformation 职责；非复合、不触发再拆，升级人工仲裁）。
- **atomicity 政策**：仅含并列连接词而 LLM 判单一职责 → 连接词不构成复合证据，降级信息性（59 节点，抽检可推翻）。
- 最终分层：**必审 2 / 信息性 128 / 干净 45；抽检包 36 节点**（20% 哈希确定性样本 ∪ 必审）。

### 3.4 你的抽检（D2-b，预计 ~10 分钟）

抽检包在 `s12_annotation_report.json` → `human_spotcheck_pack`（36 节点：description / role / atomic_ok / flags）。重点：
1. **2 个仲裁节点**（sem_023/n3_s3_s2、s3）：裁决「维持拆分」或「合并回一个 transformation 节点」；
2. 20% 样本 role/atomicity 抽查；不一致率 >30% → 回退人工全量（24 号 D2-b 条款）。

---

## 四、S12.4 · Band Coverage Gate（核心决策点）

生产 seam（`bench::route_single` → `router::plan()`，policy_id=routerbench_v02_p2，soft-gate 0）实测 175 semantic 节点 + 32 control 节点：

| 指标 | full_prompt（root_goal+description 进 router） | description_only（仅 description 进 router） |
|---|---|---|
| band 分布 | hard **91.4%** / medium 8.6% / easy 0 | hard 56.0% / medium 36.6% / **easy 7.4%** |
| multi_model_eligible_rate | **8.6%**（<20% → diagnostic 阈） | **44.0%**（20–50% → routing candidate 区间） |
| 选择分布 | strong 160 / weak 15 / mid 0 | strong 98 / weak 77 / mid 0 |
| winner_entropy | 0.422 bits | 0.990 bits |
| Case gate（semantic） | **11/30 routing-relevant**，19 全 strong-only→diagnostic | **29/30 routing-relevant**，1 diagnostic |

这是「长 prompt → hard band 压缩 routing 空间」的**第四次独立复现**，且首次量化了「router 看到什么」对 routing 空间的因果影响。mid 模型（deepseek_r1_distill）两口径下均 0 选中——P2 质量先验下 medium 带被 strong/weak 分食，System Pool Audit（S12.5）需要重点关注 mid 的定位。

---

## 五、决策请求（三个，按 a/b/c 回复即可）

### 决策 A · v0.2 headline 的 routing 口径（影响 S12.6 采集与全部 headline 数字）

**决策点一句话**：router 的 routing input 用「完整 frozen prompt」还是「节点 description」？

| 方案 | 机制 | 优点 | 缺点 |
|---|---|---|---|
| **A-a full_prompt** | routing input = node prompt（与 outcome 采集同文本，v0.1 同构） | 语义最干净：router 看到执行器看到的一切 | hard 91.4% → 19/30 case diagnostic，headline 缩到 11 case 且 multi_model 8.6%，RQ3（routing gain）基本不可答 |
| **A-b description_only** | routing input = description（task_type 不变；execution prompt 仍为 full prompt） | 29/30 rele、multi_model 44%——headline 有实质 routing 空间；**贴近生产现状**（plan_decision 子任务路由连 prompt 都不看，desc 已是更强信号） | router 输入 ≠ 执行输入（seam split 需在 case schema 显式冻结 `routing_input` 字段）；可被质疑「router 信息少于执行器」 |
| **A-c 双轨**（推荐） | headline = description_only；full_prompt 作 sensitivity diagnostic 同报 | headline 可答 + 塌缩现象本身是发现（「router 上下文越全，band 多样性越塌」——23 号 §二十五 complexity-gate 研究的直接证据）；两口径数字都有出处 | 报表口径多一层，需要明确标注哪轨是 headline |

**推荐 A-c**。依据：headline 的存在意义是回答 RQ1–RQ3（routing 可行性/互补性/收益），A-a 下 headline 近乎空转；而 full_prompt 塌缩数据作为 diagnostic 恰好支撑 25 号 §二十五「complexity detector 应单独报告」的路线。若你更看重口径纯度，A-a 亦可——代价是 v0.2 routing headline 退化为 11 case 的弱证据（此时 S12.6 采集规模可相应缩到 11 case）。

**取决于你的追问点**：若选 A-b/A-c，case schema 增加冻结字段 `routing_input: description`（hash 记录），S12.6 stage 0 注入 artifact 后 description 不变 → **无需重测 desc 口径 band**（full_prompt 口径需重测，已列入 stage 0 零成本步骤）。

### 决策 B · 抽检确认（D2-b 收尾）

36 节点抽检包 + 2 个过拆仲裁（§3.4）。回复「抽检通过」或指出不一致节点；不一致率 >30% 我回退人工全量重标。

### 决策 C · S12.5/S12.6 放行（花钱审批）

D3-b 阶梯（25 号 §九）第一批：

| 项 | 规模 | token 估算 | 成本估算 |
|---|---|---|---|
| stage 0 参考执行（artifact 冻结） | 首批 5 workflow × ~6.6 节点（拆分后均值）≈ 33 节点 ×1 模型 | ~50k in / ~15k out | ~$0.03 |
| D3-0 Provider Probe | 3 模型 × 2 节点 | ~6k in / ~2k out | ~$0.005 |
| D3-1 5-workflow outcome 采集 | 33 节点 × 3 模型 ≈ 99 点 | ~150k in / ~40k out | ~$0.10–0.20 |
| Judge 评分（qwen-plus，frozen rubric，temp=0） | 99 点 | ~180k in / ~10k out | ~$0.10 |
| **首批合计** | | | **≈ $0.25–0.35**（授权 $0.3–0.5 内） |

→ System Pool Audit（S12.5，零成本）→ 审计通过才放量全量 harvest（≤$1.5 上限，届时再按实节点数报精确预算）。**注意**：拆分后 semantic 节点数 175（原 126），全量 harvest+judge+参考执行估算 **~$1.2–1.6**，贴着 $1.5 上限——若审计通过，我会在放量前按 A 决策后的 headline-capable 节点数（预计 desc 口径 rele case 的节点 ≈ 170）重报一次精确数字再动手。

**采集模型映射**（24 号 D3 追问点，按其建议执行）：R7 档位 → DashScope 映射 deephermes(weak)→qwen-turbo、deepseek_r1(mid)→qwen-plus、intern_s1_mini(strong)→qwen-max；单价按官方页重录进 cost_basis（S12.7 freeze 项）。

---

## 六、诚实边界（本段）

1. 拆分报告前 22 组 raw_output 未存档（工具缺陷，已修复为累积合并；拆分结果自 case 状态重建可追溯）。
2. atomicity「连接词≠复合」政策是 agent 判定，未经理由你确认（已记 verdict，抽检可推翻）。
3. control 节点标注为 `control_fixture`（不参与 semantic 标注与人工签收，diagnostic 身份）。
4. 全部 LLM 预标注未经人工逐条确认（review_status=agent_reviewed / pending_human_spotcheck，freeze gate 要求 100% human-approved 于 S12.7 兑现）。
5. 实际花费 ~≤$0.10 略超分解授权合计（~$0.05）——超出部分全部来自 spec 强制的复合节点拆分（三轮 LLM 调用），机制上不可免。
6. Mimosa commit/push 门禁 medium 提示（s12_semantic_expand.py:99 sqlite3.connect(args.db) 跨文件污点嫌疑）：与 S11 同模式，DB 路径为 argparse 常量默认值，benchmark 内部工具无外部输入面；按兼容策略放行，如实记录。

## 七、下一步（决策后）

```text
A/B/C 定 → S12.6 stage 0（参考执行 + Provider Probe + band 复测）
         → D3-1 首批 5 workflow 采集 + judge
         → S12.5 System Pool Audit（oracle_gain / cheap_safe / weak_oracle_share / winner_entropy）
         → 审计单独回报 → 通过则放量 harvest（≤$1.5）→ S12.7 v0.2 candidate freeze（新 systembench_id + manifest）→ 完整报告 → S13
```
