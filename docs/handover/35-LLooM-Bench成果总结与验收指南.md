# 35 — LLooM Bench 成果总结与验收指南

> 汇总日期：2026-10-12。范围：2026-10-05 ～ 10-12 全部 bench 工作（含 34 号核验 P0/P1/P2 闭环）。
> 与 33 号的分工：33 号是**工作过程**全景；本文以**验收人视角**组织——你如何亲手验证每一项成果。
> 本文所有命令与预期输出均为 2026-10-12 实测记录，非预期值。

---

## 一、一页速览

**完成了什么**：三条 benchmark track（RouterBench v0.2 冻结 / OrchestrationBench v0.1 基础层 /
SystemBench v0.1 冻结）+ S12 语义探索快照（L1 完整 / L2 partial）+ 证据审计与展示封板
（Showcase Freeze）+ 34 号外部核验的 P0/P1/P2 全部闭环。

**核心成果（30 秒版）**：

| 成果 | 一句话 |
|---|---|
| 冻结定量证据 ×2 | RouterBench v0.2（n=12,345）：P2 省 13.46% 成本、胜过匹配随机；SystemBench v0.1（65 workflows）：路由省 14.95% workflow cost |
| 诚实边界入档 | Best Single 点估计支配 P2（同图呈现不隐藏）；completion 是 replay 判定；makespan 是仿真 |
| 研究发现 ×3 | weak 被 gate 结构性拦截→换池解锁；输出预算改变模型胜出（双 profile）；更多上下文≠更多可选模型（Compression 0.1948） |
| 可复核性 | 远程 v2 一条命令验全部数字：`python3 benchmarks/showcase/audit/replay_check.py` → 31/31 PASS |

**验收成本**：15 分钟（速验 3 条命令）/ 1 小时（完整八步）。

---

## 二、完成清单（commit 链）

| 阶段 | 内容 | commits |
|---|---|---|
| RouterBench Day 1-15 | 数据层/回放引擎/v0.1→v0.2 冻结/tournament/test replay | `0a45b01`…`e2d9299`（已含于早期推送） |
| OrchestrationBench O-Day 9-14 | schema/adapter/调度器/failure/归因 | `0d0b5e4`…`62d095b` |
| SystemBench Phase A/B + S9-S11 | composition 层/v0.1 冻结/shadow/hardening/decomposition pilot | `422d695`…`246e86e` |
| S12.0-S12.4 | 标注 spec/semantic set/标注拆分/band 双口径 | `1746cf5`、`c97d688` |
| S12.5（29 号落地） | Provider Probe/Contract/Probe/Audit/Protocol Stability/Re-audit | `13c26dc`…`ef5df8c` |
| S12.6（b 方案） | L1 Matrix 完整/L2 partial | `74b524f`、`af9684d`、`42216b7` |
| Showcase Freeze（31/32 号） | 审计+三图+claims+manifest | `746c3d7` |
| 34 号闭环 | P0-2 勘误/同步/P1 收口/P2 验收 | `18a5e35`、`f4b5eea`、`138a6d6` |

远程同步状态：`v2` 与 `origin/v2` 一致（`138a6d6`），34 号 §8 的「远程不可见」缺口已关闭。

---

## 三、成果明细

### 3.1 RouterBench v0.2（冻结，Tier A）

test 12,345 实例，实例级 paired bootstrap（B=10k，seed=3407），policy=`P2_softgate0`：

| 策略 | Quality | Cost/query | 备注 |
|---|---:|---:|---|
| Always Strong | 0.5860 | $0.000948 | 基线 |
| **Best Single** | **0.6053** | **$0.000674** | 点估计支配 P2（必须同场） |
| **LLooM P2 Frozen** | **0.5863** | **$0.000821** | = `lloom@softgate0` |
| Cost-matched Random | 0.5610 | $0.000819 | 与 P2 成本匹配 |
| Strong-share-matched Random | 0.5749 | $0.000899 | |
| Oracle | 0.7122 | $0.000118 | 不可部署上界 |

- P2 vs Always-Strong：saving **13.46% [12.20, 14.73]**；retention 1.0005 [0.9957, 1.0052]（含 1.0）
- P2 vs 匹配随机：**+2.52pp [2.07, 2.97]** / **+1.14pp [0.75, 1.53]**（content-aware 实证）
- weak precision（frozen test）0.786 [0.7629, 0.8086]（≠calibration 阶段 0.809，口径已分注）

### 3.2 SystemBench v0.1（冻结，Tier A）

80 cases（test 65）/ 327 节点，frozen outcome replay + duration-profile 仿真调度：

- 路由效果（B3→B4）：workflow cost **−14.95% [−26.59, −5.16]**，completion/Q_node 点估计不变
- 调度效果（B2→B3）：simulated makespan mean 2184.6→500 ms（median 2000→500 / p90 3500→500）
- Coordination：frozen 字段 18.347（ratio-minus-one）→ 派生 **19.35× / +1834.7%**（median 5.11 右偏；诊断附录位，非首页）
- O1 Node Oracle 26.2% 为事后上界

### 3.3 S12 Semantic Snapshot（exploratory，Tier B/C）

- 结构与协议冻结：30 workflows/175 节点、Contract v1（R1 headline/R2 diagnostic）、双 profile（P1536 Budget / P4096 Sensitivity）
- L1 Matrix：33 independent × 3 模型 × 2 profile = **198 cells 全采**，judge 197/198；双 profile winner agreement 0.500（budget 效应复核）；cheap-safe 68-73%
- L2 partial：119/180 输出 / 49 已评（`PARTIAL / NOT FOR HEADLINE`，非 MCAR）
- 发现：Compression 0.1948；atomicity 17.5%；协议异质性已逐 cell 审计分层（198×26 字段 CSV）

### 3.4 展示与治理资产

- `benchmarks/showcase/`：主报告 + claims（C1-C7）/禁用清单 + 三图 + figure_data CSV + audit（可比性/coverage/coordination 派生/walkthrough/replay_check.py）+ manifest（13 输入 SHA + snapshot identity）
- 文档一致性：SystemBench README、LLooMprogress.md、OrchestrationBench README 均已对齐实际状态（34 号 §8.3 清零）

---

## 四、验收指南（你要怎么验）

### 4.0 前置

```bash
git pull                      # 本地 v2 与 origin 一致（138a6d6）
# 复核环境：Python 3.3+（纯 stdlib，无第三方依赖）；看报告用任意 Markdown 阅读器
```

### 4.1 速验（3 条命令，~5 分钟）

```bash
# ① 离线复核：31 项数字级校验（CSV↔冻结报告逐项 + P2 真源 + manifest hash）
python3 benchmarks/showcase/audit/replay_check.py
#    预期输出：检查 31 项：PASS 31 / FAIL 0

# ② P2 主数字真源核对（勘误后的权威行）
grep "P2 Frozen" benchmarks/routerbench/reports/v02_test_report.md
#    预期：| P2 Frozen ⭐ | 0.5863 | $0.000821 |
#    注意：runs 里 0.5867/$0.000849 是 P0/BandOnly（34 号勘误点，showcase 已修）

# ③ 展示入口打开即读
open benchmarks/showcase/README.md        # 或任意编辑器
#    预期：第一屏可见三 track、Evidence Tier A-D、一句话结论（含 Best Single 支配声明）
```

### 4.2 完整验收（八步，~1 小时）

| 步 | 命令/动作 | 预期 | 验的是什么 |
|---|---|---|---|
| 1 | `git log --oneline -10` | 见 §二 commit 链尾部（…138a6d6） | 版本链完整、34 号三项闭环在案 |
| 2 | `python3 benchmarks/showcase/audit/replay_check.py` | 31/31 PASS | 全部展示数字与冻结报告一致、P2 未再错标 |
| 3 | 主表肉眼核对：`showcase_report.md` §2 vs `v02_test_report.md` | 六策略数字逐一相等；Best Single 支配有显式声明 | 主表单一权威来源 |
| 4 | `grep -rn "0.5867" benchmarks/showcase/ --include="*.md" --include="*.csv"` | 唯一命中 = claims C1 的身份区分注（勘误说明本身），无其他混入 | P0/BandOnly 数字未污染展示层 |
| 5 | `grep -o "PARTIAL / NOT FOR HEADLINE" benchmarks/showcase/audit/l2_coverage.json` | 命中 | L2 partial 未冒充完整结果 |
| 6 | 打开三图（`figures/*.png`）与 `figure_data/*.csv` 对照 | 图 1：Best Single 支配箭头 + Oracle 标 not deployable；图 2：mean 与 median/p90 分开标注；图 3：exploratory 标注 | 图不制造胜利感、分母与协议在图注 |
| 7 | `claims_and_limits.md` 通读 | C1-C7 每条带数据源与分母；7 条禁用表述 | 对外口径受控 |
| 8 | （可选）质量门：`cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` | 0 warning；**138 passed 0 failed**（实测 2026-10-12） | Rust 侧健康 |

### 4.3 34 号 P2 验收清单对照（八项，全部 ✅）

| # | 34 号要求 | 状态 | 验收锚点 |
|---|---|---|---|
| 1 | RouterBench 主数字与官方报告一致 | ✅ | replay_check 31 项（P2 对照源=`lloom@softgate0`） |
| 2 | makespan mean/median/p90 与 simulated 分开 | ✅ | CSV 三列分列 + 图 2 分开标注 |
| 3 | Coordination 公式/ratio-minus-one/分布，不作首页 | ✅ | `audit/coordination_ratio_derivation.json` |
| 4 | Semantic 逐 cell 可追溯、分层、无 pooled ranking | ✅ | `audit/semantic_l1_comparability.csv`（198×26） |
| 5 | L2 标 partial，不与 L1 合并 | ✅ | l2_coverage.json |
| 6 | claims 全部有来源；未测边界明确 | ✅ | claims_and_limits.md |
| 7 | 离线 replay/图再生/audit 通过 | ✅ | replay_check.py（零 API） |
| 8 | manifest 与 snapshot commit hash 记录 | ✅ | README 直书 + source_inventory（13 SHA + erratum commits） |

### 4.4 验收中「不应该存在」的东西（反向抽查）

- 任何把 P2 标为「Pareto 最优/优于所有简单策略」的句子 → 应为零命中
- 任何「SystemBench 证明端到端成功率提升」「线上 latency 减少 75%」类表述 → 应为零命中
- 任何把 `v0.2` 称为 complete/released 的表述 → 应为零命中（status=`showcase_snapshot_not_formal_v02_release`）

```bash
# 排除禁用清单文件本身（claims_and_limits.md 列举禁用话术属正常）
grep -rn "Pareto 最优\|证明端到端\|latency 减少 75\|v0.2 complete\|v0.2 released" \
  benchmarks/showcase/ --include="*.md" --include="*.json" --include="*.csv" \
  | grep -v "claims_and_limits.md" | grep -v "not_formal"
# 预期（实测 2026-10-12）：无命中
```

---

## 五、已知边界（验收时的判读框架）

1. **L1 不是 live**：frozen replay 证明控制面与归因链路，非真实 artifact 传播后的 E2E。
2. **semantic L1 协议异质**（已审计分层）：mid 代际替换（qwen3.6-plus→qwen3.7-plus）、judge 换档（flash 113 cells / max 84 cells）、178/198 truncated——分层视图可查，无 pooled headline。
3. **L2 partial**：缺失与额度中断相关（非 MCAR），仅 coverage 报告。
4. **Best Single 支配**：当前结果支持「content-aware 路由收益」，不支持「已找到最优路由策略」——这是最重的诚实边界，也是下一阶段研究问题的来源。
5. **统计口径**：所有 CI 为实例/workflow 级 paired bootstrap（B=10k seed=3407）；calibration 与 frozen test 数字全程分注。

---

## 六、后续计划

### 6.1 当前默认态：maintenance

只修影响复现与展示的 bug。不加新数据、不动冻结口径、不按路线图自动扩展。

### 6.2 挂起待办（等你指令）

| 项 | 说明 |
|---|---|
| Production issue P-01 | 生产 ai_client 对 qwen3.6 系列不传 `enable_thinking`（子任务截断+分钟级 latency）+ `max_tokens=estimated` 截断风险——独立立 issue + 受控回归，不回写 benchmark |
| 对外材料 | 立项/答辩稿可直接引用 C1-C7 与 showcase 三图 |

### 6.3 重开实验的唯一入口（32 号 §11.2 触发器）

| 触发 | 动作 | 红线 |
|---|---|---|
| 评审问「P2 为什么没超过 Best Single」 | 离线分析 frozen traces → 写成 RQ | 不立即改 router、不扫 λ |
| 需要可信 semantic ranking | 预注册同质实验（固定 generation/judge/profile，先定样本/预算/停止条件） | 不把异质 198 cells 改标题发版 |
| 生产出现 thinking/截断问题 | 独立 production bug + 新 policy identity | 不回写冻结结果 |
| 需证明 artifact 改变模型选择 | L2 replay RQ：source audit → feasibility | 不为填满 180 cells 补采 |
| 需真实 E2E 成功率/延迟/花费 | 新建 dynamic/live track（独立 held-out + 完整 traces） | 不把 simulated 当 live |

### 6.4 远期研究门（全部有前置条件）

正式 SystemBench v0.2 release（32 号 §十 checklist：100% human sign-off、O3 exact 全枚举、同质 headline 等）→ L2 artifact replay → State-aware Router v1（独立分支）→ live track。均不自动启动。

---

## 七、验收后动作

验收通过 → benchmark 阶段正式收束（maintenance）。若发现问题，按层回退：
数字错 → 改 showcase 层（展示派生）并走勘误 commit 流程（参照 18a5e35 先例，不动冻结报告）；
脚本错 → 改 replay_check.py；
口径争议 → 停下写 RQ，不静默改图。

---

## 八、文件索引（验收涉及）

| 文件 | 作用 |
|---|---|
| `benchmarks/showcase/README.md` | 展示入口（Tier 分级 + 快速复现 + snapshot identity） |
| `benchmarks/showcase/showcase_report.md` | 主叙事（8 节） |
| `benchmarks/showcase/claims_and_limits.md` | C1-C7 可引用 / 7 禁用 |
| `benchmarks/showcase/audit/replay_check.py` | **一键离线复核（31 项）** |
| `benchmarks/showcase/audit/walkthrough.md` | 贯穿式 demo（身份标注） |
| `benchmarks/showcase/audit/*.json/csv` | 可比性审计/L2 coverage/coordination 派生/检查结果 |
| `benchmarks/showcase/manifests/` | source_inventory（13 SHA）+ showcase_manifest（v3） |
| `benchmarks/routerbench/reports/v02_test_report.md` | RouterBench 权威报告（P2 Frozen ⭐ 行） |
| `benchmarks/systembench/reports/v01_report.{json,md}` + `v01_hardening.json` | SystemBench 权威报告 + 分位数 |
| `research/手记规划-20260928/24-34 号` | 决策与工作记录族 |
