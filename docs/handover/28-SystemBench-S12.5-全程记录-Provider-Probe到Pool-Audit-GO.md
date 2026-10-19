# 28 — SystemBench S12.5 全程记录：Provider Probe → Contract Freeze → Semantic Probe → Pool Audit（GO）

> 执行窗口：2026-10-07 ～ 2026-10-08。执行者：ZCode（Nironta 授权自主闭环）。
> 上游文档：27 号（S12.5-S15 最终实施决策与指导，本报告的执行权威）。
> 本报告定位：27 号 §42 四步序列（S12.5.0 → S12.5.3）的完整工作过程、成果、数据、与后续规划。
> 交付状态：**S12.5 四步全部完成，Pool Audit verdict = GO**；S12.6 Full Harvest 放量预算待 Nironta 拍板（§六）。

---

## 一、执行链总览

```text
27 号 §42 指令链（「在这四步完成以前，不做 175-node 全量采集，不做 S13，不做 Live」）：

S12.5.0 Provider Probe          PASS（7 calls，$0.168）
        ↓
[前置] 依赖语义一致性修复        18 sem 升级 + 14 dangling 重映射，幂等归零
        ↓
S12.5.1 Contract Freeze         routing_input_contract_v1 + sidecar 175 节点 + R1/R2 重命名
        ↓                        + Routing Context Compression 0.1948 + P2 价格重录 v2
S12.5.2 5→3-workflow Probe      72 cells，$0.5426（judge 额度中断→换档缩减）
        ↓
S12.5.3 System Pool Audit       §11.1 全指标 + §12 GO 五条件全过 / STOP 三信号全无
        ↓
      GO（S12.6 gate 已过，放量预算待拍板）
```

commits（分支 v2，基线 origin/v2 @ `1746cf5`）：

| commit | 内容 |
|---|---|
| `13c26dc` | S12.5.0-S12.5.1：Provider Probe PASS + Contract Freeze + 依赖修复 + P2 重录 |
| `a443aee` | S12.5.2-S12.5.3：Semantic Probe 72 cells + Pool Audit GO |
| `c97d688` | （S12.2-S12.4，26 号报告已记） |

质量门：`cargo clippy --workspace --all-targets -- -D warnings` 0 错；`cargo test --workspace` **138 passed / 0 failed**（基线只增不减 ✓）。

---

## 二、前置：依赖语义一致性修复（花钱前必修）

**发现时机**：S12.5.0 开跑前抽查 `sb_v02_sem_006/023` 的 `artifact_contract.consumes` 与 edges 投影，发现两类 `s12_composite_split.py` 拆分遗留 bug：

1. **拆分首子节点 sem 未升级**：`*_s1` 节点的上游依赖被重接到自身（有入边、consumes 非空），但节点级 `dependency_semantics` 仍为 `independent`——违反 spec v1「sem 是 artifact_contract 的投影」。共 **18 个**。
2. **dangling consumes 引用**：拆分后节点 consumes.from 仍引用拆分前的父 id（如 023 的 n2 consumes `n1`，n1 已拆为 n1_s1/s2/s3）。共 **14 个**。重映射规则：父组最终产物 = 拆分链末子节点（`{parent}_s{n}`，拓扑序最后一个孩子）。

**处理**：新脚本 `benchmarks/systembench/scripts/s12_5_dep_consistency.py`（--audit 只读 / --fix 写回）。修复写回 `system_cases_v02.jsonl` + `annotations_v02.jsonl` + `builder_manifest_v02.json`（dependency_fix 记录），复审计幂等归零。

**影响**：修复后全库 175 semantic 节点 = **33 independent（→ L1 Matrix）+ 142 artifact_dependent（→ L2 Conditional Matrix）**，L1/L2 分建（27 号决策 ⑤）的 cell 分类依据由此干净。若不修，18 个节点会被错分进 L1，L2 conditional 统计被污染。

**诚实边界**：22 组拆分记录 raw_output 未存档（26 号已知），本次修复未触碰该历史；edge_no_consumes 计数为 0（有入边必 consumes，投影自洽）。

---

## 三、S12.5.0 Provider Probe（27 号 §九）— PASS

**规模**：3 candidate models（native_qwen36_3tier_v1）× 2 代表节点（sb_v02_sem_006 第一个 independent + 第一个 artifact_dependent）+ 1 次 judge JSON validity。`non_matrix=true`（probe 数据不入正式矩阵）。

**七项检查结果**（`reports/s12_5_provider_probe.json`）：

| 检查项 | 结果 | 备注 |
|---|---|---|
| provider reachability | ✓（含瞬时故障重试 1 次） | plus 首跑一次瞬时失败，重试即过 |
| model identity | ✓ | 三模型 response.model echo == 请求 id |
| schema stability | ✓ | content:str + finish_reason + usage 三字段齐 |
| JSON validity | ✓ | judge 协议输出可解析，score∈[0,1] |
| latency | 记录：44.5-144.2s | thinking-on 口径（见下） |
| cost recording | ✓ | usage × 官方单价 > 0 |
| cost=0/unknown 路径 | ✓（验证存在） | unknown 不进 Oracle/Tax/frontier（红线） |

**关键实测发现（影响后续所有协议）**：

- qwen3.6 三层**默认开 thinking**（`reasoning_content` 字段，17-20k reasoning chars/调用）；reasoning tokens **不受 max_tokens 约束** → content 被系统性截断（三模型 finish_reason=length），输出即"截断协议"。
- latency 44-144s、completion tokens 7.4k-9.4k/调用（reasoning 占 ~90%），probe 实花 **$0.168**（原预估 $0.006 的 28 倍——预算模型被实测推翻的第一课）。
- `enable_thinking=false` 实测完全有效：latency 3-5s、输出 216-233 tokens、finish=stop 无截断。
- **定案**：headline 执行协议 = `enable_thinking: false`；thinking-on 保留为 sensitivity 记录不采正式矩阵（如需重评全 cells 重采 ~$3）。

**production finding（候选回修，另行决策）**：生产 `ai_client` 对 qwen3.6 系列不传 `enable_thinking`（DashScope 默认开）→ 生产用户配该系列做编排子任务会拿到**截断输出 + 分钟级 latency**。此项与 max_tokens 问题合并，见 §四。

**P2 价格重录（同日完成）**：官方页 2026-10-07 华北2（北京）逐模型核验——flash ¥1.2/¥7.2、plus ¥2/¥12（≤256k 档）、max-preview ¥9/¥54（≤128k 档）每百万 tokens。`model_pool_native.yaml` → **v2**（`price_status: official_verified_2026-10-07`，fx 7.25 与原转述隐含率一致）。diff：flash/plus <0.5%（换算舍入）、**max-preview -4.5%**（原转述 $1.30/$7.80 偏高）。按 freeze_rules：价格变化 = 新 cost_basis_hash = 新 benchmark_version（S12.7 freeze 时快照）。

---

## 四、S12.5.1 Routing/Execution Input Contract Freeze（27 号决策 ①）— 零成本

**落盘三件**：

1. **`manifests/routing_input_contract_v1.yaml`**（contract 权威）：
   - **R1** = `routing_input{mode: description}` —— **headline 路由口径**，依据 production-faithful（生产子任务执行 `ai_client.rs:526` 只发 description）
   - **R2** = `routing_input{mode: root_goal_plus_description}` —— diagnostic，生产当前不可见 root_goal
   - **execution_input** 分离：independent = base_text；artifact_dependent = base_text + 冻结 artifacts（`[子任务{node_id}] {artifact_text}`，拓扑序）
   - 执行协议三项 + 两项**显式偏差**（均带实测依据入档）：`enable_thinking: false`（§三）；`max_tokens = max(estimated, 1536)`（stage0 首跑 35/35 finish=length，planner estimated 系统性低于真实输出需求，质量信号被截断伪影污染）
   - production finding 合并记录：生产 `max_tokens=estimated` 机制本身在所有模型上有同款截断风险
   - judge 协议、split 纪律（Pool Audit 只读 calibration，test read-only）
2. **`cases/routing_inputs_v02.jsonl`** sidecar：175 节点 × {routing_input R1 hash、routing_input_r2_diagnostic hash、execution_input{base_text/base_text_hash/artifact_refs/final_text/final_hash/final_frozen}}。`s12_5_contract.py` 生成，幂等；完整性校验 base_text vs R2 零漂移。
3. **band 报告 R1/R2 正式命名**（`s12_band_coverage.py --collect` 重跑）：views 改名 `R1_description_only_headline` / `R2_full_prompt_diagnostic`；headline 口径注由「留 Nironta」更新为「27 号决策 ① 定案」。

**Routing Context Compression**（新增，仅 diagnostic）= Eligible(R2)/Eligible(R1) = **8.6% / 44.0% = 0.1948**，与 27 号预估 ≈19.5% 吻合。

---

## 五、S12.5.2 Semantic Probe（27 号 §十）— $0.5426 实花

### 5.1 额度中断与缩减处置（2026-10-08，Nironta 指示「qwen-plus 额度几乎耗尽，缩减测试量」）

首跑 collect（5 case × 35 节点 × 3 模型，judge=qwen-plus）跑到 ~50% 时 Nironta 通报 qwen-plus 额度告急，立即 TaskStop。实测探测：**qwen-plus 403 Free quota exhausted（Arrearage）**；qwen-turbo 200 OK 且 JSON 输出正常；qwen3.6-flash 200 但在采集池内（rubric 独立性不允许）。

三项调整（已入档）：

| 调整 | 内容 | 依据 |
|---|---|---|
| judge 换档 | qwen-plus → **qwen-turbo**，rubric **v1→v2**（文件改名 `judge_rubric_v2.md`；正文/锚点/附加规则与 v1 逐字一致，仅 judge 模型与版本号变化） | 不在采集池内、无自评偏差、200 OK |
| probe 5→3 | 确定性贪心序前 3：**023 / 030 / 006**（24 节点；7 种 semantic_role 覆盖不变：calculation/extraction/reasoning/search/synthesis/transformation/verification） | `select_probe_cases()` 贪心序天然给出，非手工挑 |
| 断点续采 | collect 每 cell 落盘 + 启动时跳过已有 (case,node,tier) | 中断最多丢当前 cell |

judge 计价（诚实修正）：初版常数笔误（¥/k 当 USD/M），实为 qwen-turbo 官方 ¥0.3/¥0.6 每百万 tokens → $0.0414/$0.0828；本轮 judge 实际花费 <$0.001 量级，误差不影响任何结论，已在 commit message 注明。

### 5.2 stage0 参考执行

qwen3.6-plus 拓扑序执行 probe 全部 35 节点（5 case，早期方案产物；缩减后 collect 只用前 3 case 的冻结值），协议 = contract v1。**首跑发现 35/35 finish=length**（estimated 过低）→ 协议改 `max_tokens=max(est,1536)` 重跑（$0.06 重花，授权内）。重跑后仍多数节点顶到 1536（qwen3.6-plus 报告式长输出倾向）——**协议定案：1536 = 三层一致的固定输出预算约束（「固定预算内任务完成度」口径），truncated 分桶如实标注，不再放宽**。产出：`matrices/probe_reference_artifacts_v02.json`（35 参考输出，median 2227 chars，无空壳）+ sidecar final_hash 回填 35 行。

### 5.3 采集 + judge 结果

72/72 cells（24 节点 × 3 模型），`validity_fail=0`，**truncated 65/72（90.3%）**（1536 预算协议下如实分桶）。rubric v2 sha256 `8b8fc8a2…` 入 summary。

**judge 分数分布（rubric v2，qwen-turbo，temp=0）**：

| tier | n | mean | dist |
|---|---|---|---|
| weak (qwen3.6-flash) | 24 | **0.883** | 0.5×2, 0.8×9, 1.0×13 |
| mid (qwen3.6-plus) | 24 | 0.875 | 0.5×4, 0.8×5, 1.0×15 |
| strong (qwen3.6-max-preview) | 24 | **0.846** | 0.5×5, 0.8×6, 1.0×13 |

- score 未饱和（0.5/0.8/1.0 三档都有量），judge 能区分
- 节点级 headroom（max-min）：**mean 0.179，非零 13/24，max 0.5**
- truncated cells mean 0.857 vs ok cells 0.971——judge 对截断伪影敏感（协议有效）
- **反直觉信号：strong 均分最低**——固定 1536 输出预算下大模型长报告式输出反被截断吃亏；需全量验证

---

## 六、S12.5.3 System Pool Audit（27 号 §11.1/§12）— **GO**

**口径**：R1 desc band + P2 policy（`routerbench_v02_p2`，soft-gate 0）assignments 经 `bench::route_single → router::plan()` 现算（35 queries，零成本）；oracle 规则 = 节点内 argmax(judge_score)，平手取低成本；只读 calibration（3 probe case 全在 calibration split ✓）。

### §11.1 全指标（`reports/s12_5_pool_audit.json`）

| 指标 | 实测 | 解读 |
|---|---|---|
| n_nodes_scored | 24/24 | judge 失效/缺 cell 的节点剔除数为 0 |
| quality headroom | mean **0.179**，非零 **54.2%**，max 0.5 | quality gap 非零 ✓ |
| cost ratio strong/weak | **7.68×** | cost leverage 非零 ✓（门槛 1.5） |
| cost ratio mid/weak | 1.55× | mid 价差温和 |
| mean score by tier | weak 0.883 / mid 0.875 / strong 0.846 | strong 最低（见 §五） |
| cheap-safe rate（ε=0.1） | **79.2%**（19/24） | 近八成节点 weak 已达 strong-ε |
| weak opportunity | eligible 45.8%，**eligible_oracle_share 72.7%**，P2_selected 100% | oracle 里 weak 机会大且 router 已吃到 |
| mid opportunity | eligible 45.8%，**eligible_oracle_share 18.2%**，**P2_selected 0%** | **「mid 可用但 router 不选」实锤**（27 号 §11.1 要求区分的两类，本池为第二类） |
| winner 分布 | weak 18 / mid 3 / strong 3 | entropy **1.061 bits** |
| cross-workflow dominant | 006: weak、023: weak、030: mixed | 跨 workflow 有差异 ✓ |
| gain top20% 集中度 | **0.4651** | GO 但贴线（<0.5 过）：增益部分来自少数节点 |

### §12 判定

- **GO 五条件**：two_tier_complement ✓ / quality_gap_nonzero ✓ / cost_leverage_nonzero ✓ / winner_diversity_across_workflows ✓ / gain_not_outlier_driven ✓（0.4651 < 0.5）
- **STOP 三信号**：models_homogeneous ✗ / outlier_driven ✗ / eligibility_only_source ✗
- **verdict = GO**，S12.6 Full Harvest gate 通过。

### 诚实边界（六条）

1. probe 仅 3 case / 24 节点 / 105→72 cells（额度缩减），GO 的统计基础薄；gain 集中度 0.4651 贴线。
2. judge 换 qwen-turbo（rubric v2）——与 27 号原文（qwen-plus）不同档位；正文逐字一致但 judge 强度下降，跨口径对比需注意。
3. truncated 90.3%（1536 预算协议）：分数测的是「固定预算内任务完成度」，非无限预算能力；strong 吃亏与此协议直接相关。
4. stage0 冻结了 5 case 的 final_hash（早期方案产物），collect 只消费前 3——sidecar 中 007/017 的 final 已冻结未使用（无害冗余，可复用）。
5. R1 band assignments 为现算值（band_for 语义，S9a parity 同源），非 llmrb 冻结 outcome。
6. Mimosa commit 门禁三次报 medium（`s12_semantic_expand.py:99` sqlite3 跨文件污点嫌疑，S11 存量同模式，benchmark 内部工具无外部输入面）——按兼容策略放行并如实记录，不代表安全结论。

---

## 七、S12.6 Full Harvest 规划（待拍板）

**gate 状态**：GO 已确认；单价已官方核验（v2）；断点续采已就位；judge=turbo 每节点仅 1 次调用（$0.01 量级总额，不碰 qwen-plus 额度）。

**实测口径重算预算**（thinking-off + max_tokens=max(est,1536)）：

| 项 | 估算 | 依据 |
|---|---|---|
| stage0 全量参考执行 | ~$0.9 | 175 节点 qwen3.6-plus，1536 预算（probe 35 节点 $0.12 外推） |
| 采集 175×3 | ~$3.5 | probe 72 cells $0.54 外推（strong 7.68× 价差主导） |
| judge 175×3 | ~$0.01 | qwen-turbo 官方价 |
| **合计** | **~$4.4** | **超原授权 $1.5**（原授权按早期成本模型估算） |

**待拍板三选项**：

- **a. 全量 175 节点（推荐）**：$4.0-4.5 一次到位；L1 33 + L2 142 分层矩阵完整；headline 数据量最扎实；断点续采保中断安全
- **b. calibration 全量 + test 抽样**：~$2.5；先出方向性结论，test 少量验证泛化
- **c. 缩减矩阵**：仅 independent 33 节点（L1 headline 必需）+ L2 抽样 50 ≈ $1.6；最省但 L2 conditional 分析力受损

**执行序列（拍板 a/b/c 后）**：S12.6 stage0 全量（回填 sidecar final_hash 142 行）→ 3 模型采集（断点续采）→ judge → L1/L2 分层矩阵 → S12.6 报告 → **S12.7 v0.2 candidate freeze**（headline 节点 100% human sign-off 打包 + manifest hashes + O3 exact ≤8[10] 节点全枚举 3^8=6561 + 完整报告编号 29 号）。S12.7 freeze gate 新增项（27 号决策 ②）：headline 节点 gold = **100% human sign-off**（LLM 只预标，20% 抽检降为 QA 统计量）。

**风险预告**：strong mean 最低的反直觉信号若在全量复现，headline 的 Oracle/Tax 结论将「cheap-safe 主导」——这正是 routing 的价值形态（weak 足够时省 7.68×），不是坏消息，但叙事要与 RouterBench 口径分开写。

---

## 八、工程备忘（新坑与通过写法）

- **`enable_thinking` 参数**：DashScope compatible-mode 对 qwen3.6 系列必传 `false`（默认开 thinking → reasoning tokens 不受 max_tokens 约束 → content 截断 + 44-144s latency）。生产 ai_client 候选回修点。
- **max_tokens=estimated 陷阱**：planner 估的 estimated_output_tokens 系统性偏低，benchmark 协议用 `max(est, 1536)`；生产同机制有截断风险。
- **judge 额度单点**：qwen-plus 免费额度耗尽（403 Arrearage）——judge 类角色选型时先探测可用性再冻结 rubric；rubric 变更 = 新 rubric_id（v2），正文可逐字保留。
- **route-batch CLI**：子命令层级 `bench route-batch`，参数 `--input/--output`（非 --queries/--out）。
- **consumes 数据形态**：`[{"type": str, "from": [node_id...], "required": bool}]`——dict 项，from 是列表；拍平/重映射须保持 dict 结构。
- **case_class ≠ semantic/control**：`case_class` 是难度标签（simple/medium/complex），semantic/control 区分在 `case_role`（headline_candidate/control）。
- **sidecar 冻结状态**：`routing_inputs_v02.jsonl` 中 35 行 final_frozen=true（5 case），007/017 两 case 冻结值未消费（可复用于 S12.6 若选 b/c）。

---

## 九、Nironta 原话存档（逐字）

> 「注意到刚刚上下文突然激增后触发了压缩，完整梳理、检查一遍工作保证没有上下文超额引起的错误。
> 检查完毕后把27号指导开始到现在的工作过程、成果、以及规划，详细记录为md文档」

> 「检查断点，继续」/「检查并继续 注意qwen-plus的额度几乎耗尽，可能需要缩减测试量」（S12.5.2 期间的断点续跑指示与额度约束）

### 上下文压缩后完整性检查结果（2026-10-08 执行）

压缩发生在大体 S12.5.2 collect 完成后。全量核验清单（均通过）：

| # | 检查项 | 结果 |
|---|---|---|
| 1 | git log（13c26dc / a443aee 在位）+ 工作树干净（仅 .ai-memory.toml/.zcodeignore 两个未跟踪会话文件） | ✓ |
| 2 | 四份数据文件行数：cases 40 / annotations 175 / routing_inputs 175 / outcomes 72 | ✓ 全对 |
| 3 | outcomes cells：24 节点 × {weak,mid,strong} 全齐，case 集合 = {006,023,030} | ✓ |
| 4 | rubric v2 sha256 文件实测 == summary 记录（8b8fc8a2…） | ✓ |
| 5 | outcomes.execution_input_hash == sidecar.final_hash 逐 cell 对齐（0 不一致）；final_hash 冻结 35 行归属 5 case（预期状态，007/017 未消费） | ✓ |
| 6 | 依赖一致性复审计（18+14 修复持久，幂等归零） | ✓ |
| 7 | Provider Probe PASS / Pool Audit GO (5/5 go, 0/3 stop) / Compression 0.1948 / manifest v2 official_verified / contract 两显式偏差在位 | ✓ |

结论：**无上下文压缩引起的错误**；所有成果与 commit 完整、可复算。
