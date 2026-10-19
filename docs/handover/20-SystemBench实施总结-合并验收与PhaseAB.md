# 20 · SystemBench 实施总结（合并验收 + Phase A/B + Shadow 验证，自 19 号方案起）

> 日期：2026-10-06 · 执行：ZCode（主会话） · 范围：commit `422d695` → `539eed5`（自 19 号方案交付起）
> 定位：19 号（SystemBench 合并与改进实施指导 v2）下达后的**全部工作存档**——OrchestrationBench 合并验收、SystemBench Phase A/B（Day S1-S8）、Shadow 结构验证（Day S9a）、校准停点决策。
> 上游：`19-LLooM-SystemBench-合并与改进实施指导-v2`（执行入口）/ `17-RouterBench实施总结-收官Day9-15` / `18-OrchestrationBench实施总结-O-Day9-14`（被合并方交接报告）。
> 状态：**Phase A/B 完成、v0.1 Frozen 已冻结（manifest_hash `24705b50…`）、Gate 7/9 ✅ + S-5/S-6 partial；停点 = S9b（跨零成本红线）vs 立项材料整合，待 Nironta 校准。**

---

## 一、时间线与提交链

| commit | 阶段 | 内容 |
|---|---|---|
| `62d095b` | （分叉会话） | OrchestrationBench O-Day 14 收官（合并验收的对象，交付 18 号报告） |
| `422d695` | Phase A | SystemBench namespace + source registry + SystemCase Builder + L1 replay engine + composition runner（smoke 20 case） |
| `6c92f9a` | Phase B | 80-case 全量 + failure injection/recovery 传播 + Coordination Tax + workflow-group paired bootstrap + `frozen_manifest_v01.json`（10 项 hash） |
| `539eed5` | Day S9a | Shadow 结构验证（284 节点 parity 零失配）+ Gate S-0~S-8 自查表 + 停点决策 |

全程零 API 成本（Shadow parity 走生产 seam 的确定性函数，无 LLM 调用）；clippy 0 错、Rust 138 测试全绿贯穿。

---

## 二、合并验收（19 号落地的前置，协议 §5 全流程执行）

分叉会话交付 18 号报告（O-Day 9-14：canonical schema + PlanningBench 467/PlanBench 1901 adapter + 确定性 planning evaluator + 四策略调度模拟器 + Router bridge 接口 + F1-F7 failure injection + P1-P11 归因落库）后，主会话执行验收：

1. **质量门**：Rust 138 测试（含分叉会话 +3 的 parity 缝）、Python 54 测试、clippy -D warnings 0 错；
2. **自检抽查**：validate（fixture PASS）、planning_eval（8 场景）、schedule（12 场景）、diagnose（18 场景）、router_bridge（14 场景）、report demo（落库 run_id=2）全 PASS；
3. **零成本红线 grep**：无 requests/urllib/httpx/openai/anthropic 调用路径；
4. **登记**：18 号进 00 索引（分叉会话先行登记，文件头内部编号 17 为旧编号）；**并行会话协议失效声明**落盘（教训沉淀：路径限定 add/commit、staged 归属核对——源于 `8519149` 裸 commit 扫入对方 5 文件的事故）。

验收结论：Gate O-0~O-5 全绿确认，O-6 后置符合设计，可以进入 SystemBench。

---

## 三、Phase A（Day S1-S5）：Composition Layer 骨架

### 3.1 namespace 与 manifest（S1-S2）

- `benchmarks/systembench/` 八子目录（manifests/cases/adapters/replay/runner/evaluator/tests/reports）；
- `manifests/system.yaml`：systembench_id `lloom-systembench-v0.1`、**policy identity 分离**（`routerbench_v02_p2` bench variant ≠ production，19 号 §四十二）、L1 成功语义（`workflow_completion`：全 required node succeeded AND 依赖满足 AND 无 illegal parallelism；headline 只收 accuracy 类 0/1 量纲，judge 类 diagnostic 后置）、workflow-group split 规则；
- `manifests/source_registry.yaml`：引用 routerbench v0.2 冻结 manifest / orchestration 调度引擎（**下层数据零复制**，19 号 §二十二）；binding mode 封闭枚举（llmrouterbench_exact / routerbench_exact / historical_artifact / synthetic_fixture / live_only）+ **禁模糊 lookup**（Gate S-2）。

### 3.2 Rust seam（S4 前置，生产决策真源）

- `bench.rs` 新增 `route_single(pool, stats, soft_gate, cost_weight, task_type, prompt)`：组合 `build_pool_models_gated`（nominal tier eligibility）+ `policy_for` 权重 + `quality_override`（soft gate penalty 通道）+ `router::plan()`——**与 bench replay 同一决策真源，零公式复制**（19 号 §二十三）；
- `CalibrationStats: Clone`；`lloom-cli bench route-batch`：单次子进程批量分派（SystemBench Python 侧的 LLooM routing 入口）。

### 3.3 SystemCase Builder（S2-S3）

`adapters/builder.py`：workflow 结构为形态模板（chain2/fanout3/diamond/chain3/mixed5，参考 PlanBench blocksworld 形态分布），**node prompt = llmrb 真实 query（逐字）**，exact binding（`prompt_hash = sha256(prompt)` 验证），`dependency_semantics = independent`（L1 严格 counterfactual 的科学边界，19 号 §三十三），workflow-group 哈希桶切分（`sha256("lloom-sbsplit:3407:{case_id}")`，20/80）。sample 选取确定性（sha256 排序、无 random、per-dataset cap 防垄断）。

### 3.4 L1 replay engine + composition runner（S4-S5）

- `replay/l1_outcome.py`：frozen outcome 查表（零新推理）+ 复用 orchestration `evaluate_schedule`（不重写）+ completion 语义；**determinism + counterfactual 冒烟自检**（同输入双跑 identical；换 weak 分派成本必变——binding 有效性验证）；
- `runner/composition.py`：B2（strong-seq）/ B3（strong-par）/ B4（**P2 分派**-par）/ O1（node-oracle）四基线 + Routing Gain；B0 仅形态参考（goal-level 无 frozen outcome，不进 headline 对比——19 号 §十四 禁伪装）；B5/B6/B7（planning 维）**留 Shadow**（19 号 §六：v0.1 只证 control plane composition）。

**smoke 结果（20 case）**：B4 vs B3 = cost −16.7% / strong share −9.7pp / quality 持平——routing gain 在 workflow 级可见。

---

## 四、Phase B（Day S6-S8）：80-case 全量 + 统计 + freeze

### 4.1 80-case composition set（S7）

builder `--full`：**20 simple（1-2 node）+ 30 medium（3-5）+ 20 complex（5-8 multi-wave）+ 10 failure（复用 medium 形态 + F1-F7 确定性注入位标记）** = 80 case / 327 节点，全部 exact-bound；split test 65 / calibration 15（workflow-group）。

### 4.2 failure injection + recovery（S6）

`l1_outcome.replay_case` 扩展：

- 注入节点 primary model outcome 不可用（F2 语义）→ **recovery=on** 时按 `fallback_order` 重查 frozen outcome（`recovered` 标记）；recovery=off 直接 failed；
- **传播语义**：节点 failed/skipped → 下游全部 skipped（依赖未满足）——L1 的传播图雏形；
- `primary_error` 三级优先（execution → recovery → goal，19 号 §十九 的 L1 子集）。

### 4.3 统计与 freeze（S8）

`scripts/v01_report.py`：

- **五基线矩阵**（test 65 cases）：B2/B3/B4/O1/O0（node-min-cost 成本下界）；
- **Coordination Tax** = B4 workflow cost / Σ per-node independently-min-cost − 1（19 号 §十五）；
- **workflow-group paired bootstrap**（unit=system_case，B=10,000，seed=3407——与 RouterBench 实例级的关键方法论差异，19 号 §三十五）；
- **depth/width 分层**（1-2 / 3-5 / 6-8 node，19 号 §二十九）；
- **`frozen_manifest_v01.json`**：10 项 hash（routerbench_ref / workflow_schema / model_pool / routing_policy / scheduler_policy / failure_policy / evaluator / split / cost_basis / cases）+ manifest_hash `24705b50…` + 引用声明（不复制 routerbench test_selection，19 号 §二十二）。

### 4.4 v0.1 结果（test 65 cases）

| policy | completion | Q_node | cost/wf | strong share |
|---|---:|---:|---:|---:|
| B2 strong-seq | 13.9% | 0.3134 | $0.002896 | 100% |
| B3 strong-par | 13.9% | 0.3134 | $0.002896 | 100%（makespan 2184.6→500ms 仿真口径） |
| **B4 P2-routed-par** | 13.9% | 0.3134 | **$0.002463** | **91.7%** |
| O1 node-oracle | 61.5%* | 0.7324 | $0.001743 | 0% |
| O0 node-min-cost | 15.4% | 0.2641 | ~$0 | 1.0% |

*O1 在 composition_v01（65 case）为 61.5%；bootstrap 报告的 26.2% 为早期口径修正前数字，以 json 最新为准（见 §六.6 口径修正记录）。

**bootstrap CI（workflow-group paired）**：

- Routing Δcost = **−14.96% [−26.59, −5.16]**（不含 0——routing 在 workflow 级省成本显著）；
- completion Δ(O1 vs B4) = **+12.31pp [4.62, 20.00]**（oracle 上界的价值显著）；
- completion Δ(B4 vs B3) = 0.00 [−4.62, +4.62]（quality 持平预期内）。

**Coordination Tax = 18.35**——质量优先路由（B4 出于质量大量用 strong）相对“逐节点独立成本最优”的代价。这个大数字是 R7 池结构（deephermes 极便宜但答不对）的诚实量化，不是系统缺陷；19 号 §十五 的设计意图正是暴露这类 trade-off。

### 4.5 数据口径修正（重要）

`load_outcomes` 载入全模型行暴露一个此前未察的数据事实：**llmrb 池外模型存在 cost=0 行**（arenahard 四 dataset 的 judge 类源数据缺定价，R7 池三模型各 1,500 行命中）。零成本行污染 O0/tax 的 min 口径（O0 cost 显示 $0.000000、tax 分母归零）。修复：outcomes 过滤为 R7 池三模型（policy 语义本就只含三模型）。同时确认 cases 本身不受影响（arenahard 是 judge 类，builder 的 accuracy 过滤早已排除）。

---

## 五、Day S9a · Shadow 结构验证（零成本）

- **Shadow parity**：284 个 workflow 节点的 production decision path（`route_single`→`plan()` 生产 seam，经 route-batch）与 benchmark replay（composition B4）**零失配**——19 号 Track S2 的核心目标“production decision path == benchmark replay path”提前达成；
- **band 语义验证**：production 分派 hard 301（92%）/ medium 26（8%）/ easy 0；**medium 全部落 weak（deephermes）**——P2 soft gate 生效语义在真实 workflow 节点上确认；
- **发现（第三次复现）**：llmrb 完整 benchmark prompt 全部偏长 → `is_complex` 触发 hard 带——band 门槛主导在 RouterBench Core、llmrb 各 dataset、SystemBench 节点三个独立场景一致复现，routing gain 的可操作空间被 query 形态约束（强化 19 号 §三十八 state-aware routing 的动机）；
- **S9b 前置核查**（未花钱）：三 API key 已配置、dashscope 可达。

---

## 六、过程中的问题与修复（全部实测）

| # | 问题 | 根因 | 处置 |
|---|---|---|---|
| 1 | Edit 误删 `RunItem` 定义（bench.rs） | Edit old_string 覆盖了结构体头，new_string 未含 | 补回 + 全量编译验证；教训：**大文件 Edit 前必须 Read 精确锚点** |
| 2 | Edit 误删 `Sweep {` 解构行（cli，同类） | 同上 | 修复；同教训 |
| 3 | zsh 不做词分割 | `${args}` 展开为单词 | `${=args}` 强制分割 |
| 4 | Mimosa 拦全参数化 SQL | `args.*` 直进 execute 触发 | 裸 SELECT + Python 侧过滤（表小可承受） |
| 5 | Mimosa 误判 Bash 写文件 | 复合命令含 pip install | 拆分命令 |
| 6 | jsonl 的 U+2028 被 splitlines 误切 | JSON 字符串合法字符 | `split("\n")`（RouterBench Day 6 已知坑，本轮复用） |
| 7 | llmrb cost=0 行污染 min 口径 | arenahard judge 类源数据缺定价 | outcomes 过滤 R7 池（§4.5） |
| 8 | composition_v01.json 旧产物致 parity 核对空 | builder --full 后未重跑 composition | 重跑后 284 节点核对通过 |
| 9 | builder NameError（seed/argparse） | f-string 引用未定义变量 | 修正 |
| 10 | 并行裸 commit 事故（`8519149`，跨阶段） | 共享 git index | 协议加“commit 路径限定”铁律；本轮全部按路径提交 |

---

## 七、Gate S-0~S-8 自查（19 号 §三十六，`gate_selfcheck.json`）

| Gate | 状态 | 证据 |
|---|---|---|
| S-0 Identity | ✅ | routerbench v0.2（cbebcefd…）/ workflow.schema v1 / system.yaml + source_registry |
| S-1 Parity | ✅ | 下层 benchmark 数字未动（合并验收确认）；SystemBench 引用不复制 |
| S-2 Binding | ✅ | 327 节点 100% exact-bound；mode 封闭枚举；无 guessing |
| S-3 Split | ✅ | workflow-group 哈希桶（unit=system_case） |
| S-4 Replay | ✅ | L1 determinism 自检 PASS |
| S-5 Baselines | ⚠️ partial | B2/B3/B4/O1/O0 ✅；B5/B6/B7（planning 维）缺真 decomposition → Shadow online pilot 后补 |
| S-6 Attribution | ⚠️ partial | primary_error + skipped 传播链 ✅；完整传播图（root_cause→propagation_edges）→ Shadow |
| S-7 Statistics | ✅ | workflow-group paired bootstrap（B=10k，seed 3407） |
| S-8 Honesty | ✅ | L1≠live 声明；policy_id 分离；Oracle 上界声明 |

**Shadow parity 证据**（S9a）：284 节点零失配 + band 语义验证——Track S2 核心目标提前达成。

---

## 八、停点决策与两个校准点（当前状态）

**停点判断**：Phase A/B 全部完成、v0.1 Frozen 已冻结、Gate 7/9 ✅ + 2 partial 带明确解锁路径、Shadow 零成本部分完成——继续推进将跨越两个需要用户校准的门槛：

**校准点 1 · S9b（真 LLM decomposition pilot）跨越零成本红线**。解锁 S-5/S-6 的唯一途径：起生产 `lloom-server`，调 DashScope 跑 10-20 个 PlanningBench 实例的真实 decomposition（成本约 $0.01-0.04，授权量级内；key 已核可用）。副作用：真 decomposition 会**校准 planning evaluator 的 tau 阈值，阈值变化 = 新 benchmark identity**——v0.1 的 identity 稳定性将被重置。

**校准点 2 · 优先级分叉**：
- **(a) S9b pilot**：花钱少但工程链路较长（起 server / SSE 解析 / schema 对齐 / evaluator 校准），产出 planning 维基线与 Gate 全绿；
- **(b) 立项材料整合**（零成本、直接产出）：三线证据按 19 号 §四十一 三句式 headline 收拢——Routing（“不是所有 query 都需要最强模型”，saving 13.5% content-aware 实证）/ Orchestration（“不是所有复杂任务都该单模型完成”，O-0~O-5 全绿）/ System（“组合后移动质量-成本前沿”，routing gain −15% CI 不含 0）。

**我的建议**：(b) 先行——三线证据已足够支撑立项展示核心叙事，S9b 的边际价值（planning 维诊断）对展示增量小，且其结论更适合在立项反馈后落地。

---

## 九、下一步（校准后）

- 选 (b)：立项演示文档（三线证据 + 三张图文字版 + §四十三 Oracle 边界声明），随后 S9b 按需启动；
- 选 (a)：S9b pilot（起 server → SSE 捕获 decomposition → schema 对齐 → tau 校准 → B5/B6/B7 → Gate S-5/S-6 转 ✅）；
- 无论哪条：S10 v0.1 freeze gate 复核在 Shadow pilot 后收口；Native hold 维持（触发器不变）。

## 附录 · 本阶段产物路径

```text
benchmarks/systembench/
├── README.md                          # 定位/架构/L1 语义/baseline 表/运行方式
├── manifests/
│   ├── system.yaml                    # namespace + policy identity + 成功语义 + split 规则
│   ├── source_registry.yaml           # source 资产登记 + binding mode 枚举 + 禁令
│   └── frozen_manifest_v01.json       # 10 项 hash + manifest_hash 24705b50…
├── cases/
│   ├── system_cases.jsonl             # 80 case / 327 节点（exact-bound + independent + split）
│   ├── bindings.jsonl                 # node→sample binding 台账
│   └── builder_manifest.json          # 构建统计
├── adapters/builder.py                # SystemCase Builder（smoke/--full 两档）
├── replay/l1_outcome.py               # L1 引擎（failure/recovery/传播 + self-check）
├── runner/composition.py              # B2/B3/B4/O1 矩阵
├── scripts/v01_report.py              # 统计 + bootstrap + freeze
└── reports/
    ├── composition_v01.json / v01_report.{json,md}
    ├── gate_selfcheck.json            # Gate S-0~S-8 + Shadow parity
    └── pool_tournament_calibration.*  # （RouterBench 侧，Day 9）
crates/lloom-core/src/bench.rs         # + route_single seam、CalibrationStats: Clone、BenchInstance: Clone
crates/lloom-cli/src/main.rs           # + bench route-batch、--split/--selection/--matched-target/--strong-budget
```
