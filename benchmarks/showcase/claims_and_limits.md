# Claims & Limits — 可引用声明与禁止越界清单

> showcase snapshot v1（2026-10-09）。每条 claim 标注数据源与分母。

## ✅ 可对外引用的 claims（附条件）

| # | Claim | 数据源 | 分母/条件 |
|---|---|---|---|
| C1 | RouterBench v0.2 冻结 test 上，**LLooM P2 Frozen（`lloom@softgate0`，frozen manifest policy `P2_softgate0`）** 相对 Always-Strong 成本节省 13.46%（CI95 [12.20, 14.73]），质量保留 1.0005（CI [0.9957, 1.0052]，含 1.0）。注意区分：runs 里 `lloom`/`band_only`（0.5867/$0.000849）是 P0 Current/BandOnly，不是 P2 | `routerbench/reports/v02_test_report.json`（P2 Frozen = `lloom@softgate0`） | n=12,345；该模型池+policy；实例级 bootstrap B=10k seed=3407 |
| C2 | P2 显著优于 cost-matched random（+2.52pp，CI [2.07, 2.97]）与 strong-share-matched random（+1.14pp，CI [0.75, 1.53]）——content-aware 路由实证 | 同上 | 同上 |
| C3 | SystemBench v0.1 固定 workflow + frozen outcome 下，P2 路由使 workflow cost 下降 14.95%（CI [−26.59, −5.16]），completion/Q_node 点估计不变 | `systembench/reports/v01_report.json` | test 65 workflows / 327 nodes；workflow-group paired bootstrap |
| C4 | 并行调度使模拟 makespan 2184.6→500 ms | 同上 | duration-profile 仿真，**必须说 simulated** |
| C5 | 真实 decomposition 输出 15/15 通过 canonical validator；22/126（17.5%）节点违反 atomicity（已按 spec 拆分） | `s11_decomposition_pilot.json` + `s12_composite_split.json` | 样本小，diagnostic |
| C6 | Output budget 是实质性 routing variable：受控 probe（n=6）truncation 94.4%→55.6%、winner agreement 4/6（REVIEW）；L1 matrix 同方向（16/32，descriptive） | `s12_5_protocol_stability.json` + L1 matrix | **必须标 exploratory**；probe 受控、L1 混身份分层 |
| C7 | Routing Context Compression = 0.1948（R2/R1 eligible 比）——更多上下文≠更多可选模型 | `band_coverage_v02.json` | P2 band 口径，calibration+test 全 175 节点 |

## ❌ 禁止的表述

1. 「LLooM 已找到全局最优路由策略」/「P2 优于所有简单策略 / Pareto 最优」
   —— **Best Single 点估计支配 P2**（质量更高、成本更低），必须同图呈现。
2. 「SystemBench 证明端到端 agent 成功率提升」—— completion proxy 是 replay 判定，非 E2E。
3. 「并行调度让线上 latency 减少 75%」—— 是 simulated makespan，非墙钟。
4. 「semantic matrix 给出最终 Weak/Mid/Strong 排名」—— 身份异质（mid 代际替换 + judge 换档 + 90% 截断），无 pooled leaderboard。
5. 「L1/L2 证明 artifact 让强模型胜出」—— L2 仅 49 cells partial，方向性观察而已。
6. 「SystemBench v0.2 complete / released」—— 正式 v0.2 gate（100% human sign-off、O3 exact、同质 headline 等）未满足。
7. 把 Oracle 当可部署策略引用；把 calibration 数字当 test 数字；把 RouterBench saving 外推为生产工作流 saving。

## 边界速查

| 维度 | 已有证据 | 未有证据 |
|---|---|---|
| Routing 质量/成本 | v0.2 冻结（C1/C2） | 生产池实时性、新模型代际泛化 |
| Workflow 组合 | v0.1 frozen composition（C3/C4） | 真实 artifact 传播、线上墙钟 |
| Semantic 层 | 协议敏感性诊断（C5/C6/C7） | 完整同质矩阵、L2 完整、正式 v0.2 |
| Live E2E | 无 | 工具执行、真实花费/延迟 |
