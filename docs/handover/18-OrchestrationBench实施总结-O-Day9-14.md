# 17 · OrchestrationBench v0.1 Offline 基础层实施总结（O-Day 9-14）

> 日期：2026-10-06
> 状态：已完成，全部 commit 已推送 origin/v2（0d0b5e4 → 62d095b）
> 车道：OrchestrationBench 分叉会话（与 RouterBench 主线并行，协作协议见 `并行会话协作协议.md`）
> 方案真源：`16-LLooM-OrchestrationBench-方案.md`（§二十九 Phase 0-4）；排期：`16-v3` §二十

---

## 一、结论先行

OrchestrationBench v0.1 **Offline 基础层全部建成**：canonical workflow schema + validator、
两个外部 benchmark adapter（PlanningBench 467 实例全量 + PlanBench 1901 实例含 reference DAG）、
确定性 planning evaluator、四策略调度模拟器（方案 §十指标族全量）、Router bridge 接口层、
F1-F7 failure injection + recovery 状态机、P1-P11 case-level 归因报告 + 独立库落库。

质量门终态：**Rust 138 测试全绿（基线 135 → 138，+3 = RouterBench 会话 +1、本车道 parity 缝 +2）、
clippy -D warnings 0 错、Python 54 测试全绿；Mimosa deep scan 0 findings**。
零 API 成本红线全程无违反（无任何网络/LLM 调用代码路径）。

Gate 状态：**O-0 ~ O-5 全绿，O-6 后置（按设计），O-7 offline 半边完成**。

---

## 二、任务原话逐字存档（2026-10-06 会话任务书，只摘关键约束段）

> 「**任务（照 16-OrchestrationBench-方案 §二十九 Phase 0-4 + 16-v3 §二十排期）**
> - O-Day 9：canonical workflow schema（workflow.schema.json）+ validator + fixture examples
> - O-Day 10：PlanningBench adapter + PlanBench adapter + planning evaluator（确定性、零 LLM）
> - O-Day 11：DAG scheduler + wave simulator + scheduling 指标（makespan/illegal parallelism/sequentialization waste/parallelization utilization）
> - O-Day 12：Router bridge（workflow node → subtask → RouterBench 产出的模型分派；先只做 schema 与接口，不改 router.rs）
> - O-Day 13：failure injection（F1-F7）+ recovery 状态机（确定性、可重复）
> - O-Day 14：offline report（case-level attribution）+ 总结」

> 「**红线（违反即返工）**
> - 零 API 成本：本阶段 0 次 LLM/judge/provider 调用；APB online、τ² live、ToolSandbox live、AppWorld、WebArena 全部后置
> - 决策真源：bench 层只调用 orchestrator.rs / router.rs 现有公开函数，禁止复制核心逻辑；生产接口不合适就加纯函数/trait/test seam，不改行为
> - 防泄漏：planning/scheduling 评测若涉及拆分，一律哈希桶（sha256("lloom-split:3407:"+id) 模式，参考 RouterBench 导入器），禁 random 模块；test 只读
> - 独立库：benchmark 数据不碰 data/lloom.db
> - 提交纪律：每次提交前 git pull --rebase origin v2 + cargo clippy --workspace --all-targets -- -D warnings + cargo test --workspace 全绿（基线 135 测试只增不减）；conventional commits」

> 「若 PlanningBench/TPS-Bench 数据获取失败或形态与文档严重不符：降级为「schema + simulator + failure injection + 自建 fixture」，把外部 adapter 列为 P1，同样视为完成本阶段」

（其余原文：文件域协议、环境坑清单、汇报节奏，见任务书全文，此处不重复。）

---

## 三、产出清单（commit 时间线）

| O-Day | Commit | 产出 |
|---|---|---|
| 9 | `0d0b5e4` + `da2886d` | `manifests/workflow.schema.json`（canonical 规范：nodes/edges/waves/aggregation，task_type 对齐生产词表）；`validate.py`（stdlib 零依赖，错误码=归因键：SCHEMA_*/DUP_NODE/SELF_EDGE/DUP_EDGE/UNKNOWN_NODE_REF/DEPENDS_EDGE_MISMATCH/CYCLE/WAVE_PARTITION/WAVE_ORDER/EMPTY_NODES/UNSOLVABLE_WITH_NODES/BAD_MODEL_REF）；7 fixtures（4 合法 + 3 非法）；`scripts/db_init.py`（§十八三表 DDL，幂等 + 拒绝 data/lloom.db）；单测 15 |
| 10 | `9a00070` | `adapters/planningbench.py`：467/467 导入（CC-BY-4.0 实测确认），checklist 行首编号 → obligation 节点（3379 项，mean 7.24/实例），aggregation=none + 全量 test（源声明 evaluation-only）；`adapters/planbench.py`：PDDL parser（objects/init/goal 确定性切片）→ goal 原子节点，blocksworld 堆叠链依赖（on X Y ← on Y Z）= §九 9.4 reference DAG 真源，10 变体 × cap 200 = 1901 行落库（哈希桶 397 cal / 1504 test）；`adapters/planning_eval.py`：字符 2-gram Jaccard 贪心一对一对齐（确定性禁 random），recall/precision/redundancy/dependency P/R/invalid plan rate/unsolvable recognition；单测 +9 |
| 11 | `70ab712` | `schedule.py`：deterministic reference scheduler（§二十），四策略 reference/sequential/greedy_ready_queue(cap 可调)/lloom_waves（消费生产 waves 预存输出）；§十 10.1-10.7 指标全量（wave_count/serial_baseline/ideal_legal_makespan=critical path/actual/waste/utilization/illegal_parallelism_rate/parallel_width）；**orchestrator.rs `orchbench_parity` 缝**（协议 §3：仅 #[cfg(test)]，零生产改动）——生产 `dependency_waves` 输出与 canonical fixture.waves 一致断言；单测 +10 |
| 12 | `dd4c080` + `90afd9b` | `router_bridge.py`（方案 §十四/§十八 O4，接口层）：forward（workflow → instances.jsonl 同构查询行，prompt=root_goal+node 最小上下文，split 恒 test）；backward（assign_from_decisions / assign_from_bench_db 只读逐节点查询 / assign_fixed_tier=§十五 baseline 生成器）；assignment_report（routing regret + global workflow cost，注入式 lookup 纯聚合）；sample_id 跨车道契约 `obx__{wf}__{node}`；单测 +10 |
| 13 | `a8e1e56` | `diagnose.py`：F1-F7 全族（§二十一）+ 节点级状态机 READY→RUNNING→FAILED→{RETRYING\|FALLBACK\|PARTIAL\|ABORTED}→{SUCCEEDED_RECOVERED\|FAILED_FINAL}（§十八 O5 收束形态）；F4 按上游终态二遍裁决（replan 语义）；F7 按声明序 ABORT；耗时口径 retry/fallback 成功=2×duration（确定性 clock）；哈希注入 sha256("lloom-obfail:{seed}:{wf}:{node}") 禁 random；工作流级 task_success/partial/failed（§十一）；单测 +9 |
| 14 | `62d095b` | `report.py`：三层评估 → per-case P1-P11 归因（primary=最早层 O1→O2→O3→goal；P3 含漏边=dependency_recall<1）；save_run 落 bench_orchestration_runs/items；manifest_hash=六组件 sha256 聚合（§十九 identity 治理）；demo 4 case 全链路 + `reports/offline_v0.1.{md,json}`；单测 +6 |

数据资产终态（`benchmarks/orchestration/orchbench.db`，独立库）：
`bench_workflows` 467（PlanningBench）+ 1901（PlanBench）；`bench_orchestration_runs` 1 run / `items` 4 case。
raw/ 三源实体全部在外置盘 `/Volumes/Files/LLooM-bench/orchestration-raw/`，仓内 symlink，
commit hash 记录于 `manifests/datasets.yaml`。

---

## 四、Gate 对照（方案 §二十七）

| Gate | 状态 | 证据 |
|---|---|---|
| O-0 Schema | ✅ | 467+1901 workflow 全部过 validator（strict_meta 模式）；7 fixture 双向覆盖（15 单测）；错误码即报告归因键 |
| O-1 Dataset | ✅（部分） | PlanningBench：license CC-BY-4.0 实测、467/467 coverage、全量 test（evaluation-only 声明）；PlanBench：MIT、10 变体、哈希桶切分可复现；TPS-Bench 缺数据如实 deferred（见 §五）；source commit hash + 重建路径记录 datasets.yaml；manifest_hash 六组件 |
| O-2 Planning | ✅ | evaluator 确定性（8 场景 self-check + 12 单测）；obligation 节点=§九 9.1 required obligations 真源；PlanBench blocksworld=§九 9.4 ref DAG 真源 |
| O-3 Scheduling | ✅（fixture 口径） | simulator deterministic（12 场景 + 双跑 identical 断言）；TPS 外部参照缺数据 → 自建 fixture（任务书允许的降级路径） |
| O-4 Router parity | ✅（波次缝 + 接口级） | orchestrator.rs parity 缝 2 测试（Rust 侧读 fixture 断言 dependency_waves 一致）；bridge 接口不复制评分逻辑；**subtask 级 plan() 真实数据 parity 未跑**（等 O-6 真实查询） |
| O-5 Failure | ✅ | F1-F7 全族状态转移可重复（18 场景 self-check + 9 单测 + 双跑 identical + 哈希注入可重复断言） |
| O-6 Online | ❌ 后置 | 零成本红线（方案 §二十二：RouterBench freeze + offline gate 全绿后才进 pilot） |
| O-7 Report | ✅（offline 半边） | case-level attribution P1-P11 + 落库 + md/json 报告；成本/CI 维度属 online 侧，后置 |

---

## 五、诚实边界（缺口与限制，逐条写实）

1. **TPS-Bench 数据缺失**（O-Day 10 实测）：`hanwenxu1/mcp-agent` 仓库只含 Gemini eval runner 脚本 + 8 个 MCP tool server，**未发布 200 compounding tasks 数据文件**（README 目录树与实际不符，arxiv 链接是占位符 `xxxx.xxxxx`），且无 LICENSE 文件。已登记 `datasets.yaml` status=deferred；scheduling 车道按任务书降级自建 fixture。方案文档对其「200 实例两级难度」的描述与仓库现状不符——**实测优先**。
2. **APB / τ² / ToolSandbox / AppWorld / WebArena 全部后置**（方案 §五/§十九 设计如此，非缺口）。
3. **planning evaluator 是 lexical 近似**：字符 2-gram Jaccard + 贪心一对一，阈值 tau=0.45 未经过 online 数据校准；中文 obligation vs 英文 LLM plan 的对齐质量要等 O-6 才知道。evaluate_hash 已纳入 manifest_hash，调阈值 = 新 benchmark identity（§十九）。
4. **obligation 节点无依赖边**：PlanningBench 源数据只有 checklist 无依赖 → dependency P/R 指标只对 PlanBench（blocksworld 链）有意义；PlanningBench 侧任何 predicted 边都计 spurious。
5. **PDDL parser 是正则级**：只切 :objects/:init/:goal 三段原子，不支持 :constraints/:metric（PlanBench generated 域实测未使用）；1 个实例无 goal 被跳过（scanned 3321 / imported 3320）。
6. **bridge readback 未接真实数据**：RouterBench bench.db 的 sample_id 是 `rb0shot_*` 族，bridge 查询是 `obx__*` 族，现在跑必然空匹配——接口预留给 SystemBench 时代注入真实 outcome；fixed-tier 分派是 baseline 生成器不是被测路由器。
7. **report demo 的 predicted = 手工扰动 fixture**（缺节点/漏边/注入失败），不是 LLM 输出；正式 planning 归因数据等 O-6。
8. **Mimosa 钩子 4 轮拦截与收敛**（按「改写候选本身，不绕行」原则）：① validate.py CLI glob 模式展开 → iterdir+后缀过滤 → 再升级为路径收口（resolve + relative_to 仓内拒收）；② planningbench.py 三轮（隐式字符串拼接 SQL → 模块常量 → 单行字面量+局部变量+参数元组，通过）；③ router_bridge.py 的 IN 列表动态占位符 → 单占位符逐节点查询（只读 mode=ro）；④ report.py INSERT 行内联化。终态：**deep scan 0 findings**。期间 raw/ 下第三方仓库源码（mcp-agent 自带的 SQL/路径问题）被误扫，通过三源实体全部外置盘化解决。
9. **并行会话事故存档**：RouterBench 会话 commit `8519149`（Day 11-13）把当时共享 staging 里本车道的 O-Day 10 文件（5 个）一并带入——共享工作目录的副作用。文件完整无丢失、内容即本车道最终版本，未 rewrite 对方历史；`git log` 可见 `70ab712`（O-Day 11）实际只新增 schedule.py 等剩余文件。**教训**（供协议修订）：并行窗口内 add 应显式列文件路径而非目录级 `git add`，或各自提交前 `git status` 核对 staged 清单归属。
10. **fixture duration profile 是手造值**（500-1500ms），只用于验证指标口径与确定性，不代表真实模型耗时。

---

## 六、下一步建议（P1/P2 优先级）

1. **O-6 Online pilot**（等 RouterBench 主线 Freeze+CI 完成的资源窗口，16-v3 §二十三）：PlanningBench 30-50 实例真实 LLM decomposition → canonical graph → bridge → scheduler → evaluator 全链；这是 planning evaluator 阈值校准（诚实边界 3）的唯一途径。
2. **SystemBench 接口冻结**：workflow.schema.json v1 与 sample_id 契约 `obx__{wf}__{node}` 从此冻结（变更=新 benchmark identity）；SystemBench 设计方案可直接引用本文 §三的数据资产。
3. **PlanBench obfuscated 变体 robustness sanity**：数据已在库（6 个 logistics obfuscated 变体 ×200），可作为 O-2 扩展（对比 obfuscated vs clean 的 dependency recall 差异）。
4. **调度策略空间扩展**：v0.1 只有 4 个离线策略；生产 orchestrator 的实际 waves 输出（lloom_waves 消费路径）需要一次真实在线 run 的 workflow 快照才能回放。
5. **协议修订建议**：把「staged 清单归属核对」写入并行会话协议 §1（见 §五.9）。

---

## 七、验证方式（复现命令）

```bash
cd benchmarks/orchestration
python3 validate.py manifests/fixtures/fixture_chain.json   # PASS（非法 fixture 换名即报对应错误码）
python3 -m unittest discover -s tests                        # 54 tests OK
python3 adapters/planningbench.py                            # 467/467，mean obligations 7.24
python3 adapters/planbench.py                                # 10 变体，0 rejected
python3 adapters/planning_eval.py --self-check               # 8 PASS
python3 schedule.py --self-check                             # 12 PASS
python3 router_bridge.py --self-check                        # 14 PASS
python3 diagnose.py --self-check                             # 18 PASS
python3 report.py --demo                                     # 8 PASS + run 落库
cargo test -p lloom-core orchestrator                        # 含 orchbench_parity 2 缝
```

外置盘未挂载时：adapter 会明确报错（symlink 悬空），重新挂载 `/Volumes/Files` 即恢复。
