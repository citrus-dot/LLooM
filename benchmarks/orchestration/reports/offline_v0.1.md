# OrchestrationBench v0.1 · Offline Report（demo）

- benchmark_id: lloom-orchbench-v0.1
- manifest_hash: 1d613721…
- cases: 4（task_success 3 / partial 1 / failed 0）
- primary_error 分布: {'none': 1, 'P1': 1, 'P4': 1, 'P8': 1}

| case | final | planning recall | makespan ms | primary | secondary |
|---|---|---|---|---|---|
| ob_fixture_chain_v1__clean | task_success | 1.0 | 2400 | None | - |
| ob_fixture_chain_v1__missing_node | task_success | 0.6666666666666666 | 2400 | P1 | P3 |
| ob_fixture_bad_waves_v1__illegal_parallelism | task_success | 1.0 | 200 | P4 | - |
| ob_fixture_chain_v1__f2_no_fallback | partial_success | 1.0 | 2400 | P8 | P9,P7,P11 |

> demo 口径：predicted = 手工扰动 fixture（planning 侧演示），失败注入显式 schedule；
> 正式数据源等 O-6 online pilot 的真实 predicted plan（零成本红线，本阶段不跑）。
