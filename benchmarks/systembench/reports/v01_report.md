# SystemBench v0.1 · Frozen Composition 报告（L1）

> 2026-10-07T07:10:25+00:00 · `lloom-systembench-v0.1` · manifest_hash `24705b50118fbd64…`
> 80 cases（test 65）· 327 nodes · policy `routerbench_v02_p2`

| policy | completion | Q_node | cost/wf | strong | makespan |
|---|---:|---:|---:|---:|---:|
| B2_workflow_strong_seq | 13.9% | 0.3134 | $0.002896 | 100.0% | 2184.6ms |
| B3_workflow_strong_par | 13.9% | 0.3134 | $0.002896 | 100.0% | 500.0ms |
| B4_workflow_p2_par | 13.9% | 0.3134 | $0.002463 | 91.7% | 500.0ms |
| O1_node_oracle_par | 26.2% | 0.4965 | $0.000270 | 12.8% | 500.0ms |
| O0_node_min_cost | 9.2% | 0.2641 | $0.000080 | 1.0% | 500.0ms |

**Routing Gain（B4 vs B3）**：cost -14.95% · quality +0.00pp · strong share -8.31pp

**Coordination Tax**：mean 18.347（B4 相对逐节点独立最优成本和）

## Bootstrap CI（workflow-group paired，B=10k）

- routing_d_cost: -0.1496 [-0.2659, -0.0516]
- completion_delta_b4_vs_b3: +0.0000 [-0.0462, +0.0462]
- completion_delta_o1_vs_b4: +0.1231 [+0.0462, +0.2000]

## depth 分层（B4）

| nodes | n | completion | cost | routing gain |
|---|---:|---:|---:|---:|
| 1-2 | 12 | 50.0% | $0.001046 | -87.07% |
| 3-5 | 35 | 8.6% | $0.002831 | -65.04% |
| 6-8 | 18 | 0.0% | $0.002693 | -66.74% |

**Failure/Recovery**（10 cases）：recovery_off completion 0.0% → recovery_on 0.0%（1 workflow 恢复）；primary_error {'recovery': 5, 'execution': 5}

