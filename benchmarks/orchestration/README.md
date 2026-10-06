# LLooM OrchestrationBench v0.1 · Offline 基础层

> 方案真源：`research/手记规划-20260928/16-LLooM-OrchestrationBench-方案.md`
> 排期真源：`16-LLooM-RouterBench-OrchestrationBench-联合实施计划-v3.md` §二十（O-Day 9-14）
> 工程约束真源：`并行会话协作协议.md`（本目录与 RouterBench 主线并行，文件域独占）

**定位**：LLooM 编排层（复杂度判断 → 拆解 → 子任务路由 → 波次调度 → 并行执行 → 聚合/恢复）
的诊断型 benchmark。v0.1 只做 **Offline / Replay Track（零 API 成本）**：无 LLM 调用、无
judge、无 provider 推理；APB online / τ² live / ToolSandbox live / AppWorld / WebArena 全部后置。

**核心原则**（方案 §三十）：不是"拆得越多越好、并行越多越好"，而是"只拆必要的、依赖正确的、
尽可能合法并行的、模型选择合理的、失败可恢复的、最终值得付出成本的工作流"。

## 结构

```text
benchmarks/orchestration/
├── README.md                  本文件
├── orchbench.db               独立 SQLite（三表 DDL=方案 §十八；生产 data/lloom.db 零接触）
├── manifests/
│   ├── workflow.schema.json   canonical workflow 规范（JSON Schema 形态文档）
│   ├── datasets.yaml          外部数据源清单/接入状态
│   └── fixtures/*.json        7 个 fixture（4 合法 + 3 非法），O-Day 9/11/13 单测共用
├── validate.py                validator（O-Day 9）：结构+图语义+DAG+波次，stdlib 零依赖
├── adapters/                  O-Day 10：planningbench.py / planbench.py + planning evaluator
├── schedule.py                O-Day 11：DAG scheduler + wave simulator + 调度指标
├── router_bridge.py           O-Day 12：workflow node → subtask → RouterBench 分派（schema+接口）
├── diagnose.py                O-Day 13：failure injection（F1-F7）+ recovery 状态机
├── reports/                   O-Day 14：offline report（case-level attribution）
├── scripts/                   db_init.py 等工具脚本
└── tests/                     stdlib unittest（`python3 -m unittest discover -s tests`）
```

## 质量门（本目录）

```bash
cd benchmarks/orchestration
python3 validate.py manifests/fixtures/         # 4 PASS；3 个非法 fixture 预期 FAIL（看错误码）
python3 -m unittest discover -s tests           # 全绿只增不减
python3 scripts/db_init.py                      # 幂等
```

与仓库整体门（提交前必须）：`cargo clippy --workspace --all-targets -- -D warnings`
+ `cargo test --workspace`（基线 135 只增不减）。

## 红线（协议 §4 + 任务书）

1. **零 API 成本**：本目录任何代码路径不得发起网络请求（adapter 只处理已下载的本地文件）。
2. **决策单一真源**：生产波次语义 = `crates/lloom-core/src/orchestrator.rs::dependency_waves`；
   Python 侧不复制该逻辑，parity 由 orchestrator.rs 的 `#[cfg(test)]` 缝 + 共享 fixture 断言。
3. **哈希桶**：一切切分/抽样用 `sha256("lloom-obsplit:3407:"+id)`，禁 `random` 模块。
4. **独立库**：只写 orchbench.db；Mimosa 钩子被拦时改写候选本身不绕行。
5. **jsonl 读写**：一律 `split("\n")`（U+2028/U+2029 坑，RouterBench Day 6 实测）。

## 指标族速览（细节见方案 §九-§十二）

| 层 | 指标 |
|---|---|
| O1 Planning | subtask recall/precision、redundancy rate、dependency P/R、invalid plan rate、unsolvable recognition |
| O2 Scheduling | wave count、serial baseline、ideal legal makespan、actual makespan、sequentialization waste、illegal parallelism rate、parallelization utilization |
| O3 Execution/Recovery | task/milestone success、recovery rate、fallback success、abort、partial success |
| O4 End-to-End | final goal completion + cost/latency 归因 |

失败分类 P1-P11（方案 §十三）+ 注入族 F1-F7（方案 §二十一）。
