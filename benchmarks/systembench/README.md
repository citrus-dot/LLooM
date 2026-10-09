# SystemBench v0.1（Composition Layer，Phase A）

> 依据 19 号文档（`research/手记规划-20260928/19-...指导-v2.md`）。定位：**workflow-aware inference-control benchmark** 的 Frozen Composition 轨（Track S1，L1 Outcome Replay）——证明 LLooM 的 routing/scheduling/recovery 决策能在 workflow 级统一记录、重放、归因。

## 架构关系

```
RouterBench v0.2（routing track，冻结引用）──┐
                                            ├──> SystemBench v0.1 Frozen Composition（本目录）
OrchestrationBench v0.1（workflow track）──┘
```

- **不复制下层资产**：routing 策略与 node outcome 引用 `routerbench/manifests/frozen_manifest_v02.json`（R7 池 + P2）；调度引擎 import `orchestration/schedule.py`；workflow validator 复用 `orchestration/validate.py`。
- **命名**：对外统一 `LLooM WorkflowBench` / `SystemBench · Workflow Track`（19 号 §四，避让 Kakao OrchestrationBench）。

## 核心对象

- `cases/system_cases.jsonl`：SystemCase = workflow DAG（形态模板）+ 节点（prompt 为 llmrb **真实 query**，exact binding：`prompt_hash` 验证）+ `dependency_semantics: independent` + split（workflow-group 哈希桶 20/80 seed 3407）。
- `cases/bindings.jsonl`：node → source sample 的 binding 台账（mode 封闭枚举，headline 只准 `llmrouterbench_exact`；禁止 nearest/similarity/guessing——Gate S-2）。

## L1 Replay 语义（19 号 §十四）

- node outcome = frozen (score, cost, completion_tokens)，**零新推理**；
- 调度 = `evaluate_schedule`（四策略，duration = completion_tokens × 2ms 仿真口径）；
- 成功 = `workflow_completion`（全 required node succeeded（accuracy 0/1）+ 依赖满足 + 无 illegal parallelism）——**不包装成 semantic final quality**；
- 节点只收 accuracy 类 dataset；judge 类 diagnostic 后置。

## Baseline（v0.1 实装收缩版，§十二/§二十六）

| id | routing | schedule | 说明 |
|---|---|---|---|
| B0 | flat strong | – | 形态参考（goal-level 无 frozen outcome，不进 headline 对比） |
| B2 | flat strong | sequential | — |
| B3 | flat strong | parallel(reference) | — |
| B4 | **LLooM P2**（route-batch 分派） | parallel | **headline**；policy_id=routerbench_v02_p2 |
| O1 | per-node oracle | parallel | hindsight 上界（fixed workflow 前提下，§四十三） |

planning 维（B5/B6/B7）留 Shadow track——v0.1 无真实 LLM decomposition，不伪装（§六）。

## 运行

```bash
# 1. 构建 cases（smoke 20 / Phase B --full 80）
benchmarks/routerbench/.venv/bin/python benchmarks/systembench/adapters/builder.py --smoke 20

# 2. LLooM P2 分派（Rust seam 一次子进程调用）
cargo run -q -p lloom-cli -- bench route-batch \
  --input /tmp/sb_queries.jsonl --output /tmp/sb_assignments.jsonl \
  --models deephermes_3_llama_3_8b_preview deepseek_r1_distill_qwen_7b intern_s1_mini \
  --soft-gate 0

# 3. L1 自检 + baseline 矩阵
benchmarks/routerbench/.venv/bin/python benchmarks/systembench/replay/l1_outcome.py --self-check
benchmarks/routerbench/.venv/bin/python benchmarks/systembench/runner/composition.py \
  --assignments /tmp/sb_assignments.jsonl
```

## 状态

**v0.1 Frozen**（2026-10-06，Phase B 完成：80 cases / test 65 / 327 nodes、workflow-group paired bootstrap、frozen_manifest_v01.json；S9a Shadow 结构验证 + S10 L1 Hardening 六项收口见 `reports/v01_hardening.json`、`reports/gate_selfcheck.json`、`reports/v01_report.md`）。权威结果来源：`reports/v01_report.json`。

S12 Semantic Workflow 扩展（v0.2 候选）进行中：30 semantic workflows / 175 节点已构建（`cases/system_cases_v02.jsonl`）；S12.5 Provider Probe / Contract Freeze / Protocol Stability Probe（REVIEW）/ Pool Re-audit 完成；S12.6-A L1 matrix（33 independent × 3 模型 × 双 profile，198 cells）完成——存在 mid 代际替换与 judge 换档，作 exploratory 分层报告；S12.6-B L2 partial（119/180）。**当前为 semantic research snapshot（非正式 v0.2 release）**，见 `../showcase/README.md` 与 30/31/32 号工作记录。
