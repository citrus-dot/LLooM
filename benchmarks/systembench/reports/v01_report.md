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


---

## 附录 · Phase S10 L1 Hardening（21 号 §八）

| policy | completion | Q_node | cost/wf | strong | median ms | p90 ms |
|---|---:|---:|---:|---:|---:|---:|
| B2_workflow_strong_seq | 13.9% | 0.3134 | $0.002896 | 100.0% | 2000 | 3500 |
| B3_workflow_strong_par | 13.9% | 0.3134 | $0.002896 | 100.0% | 500 | 500 |
| B4_workflow_p2_par | 13.9% | 0.3134 | $0.002463 | 91.7% | 500 | 500 |
| R1_cost_matched_random | 12.3% | 0.2993 | $0.001174 | 61.0% | 500 | 500 |
| R2_strong_share_matched | 13.9% | 0.3063 | $0.002772 | 92.4% | 500 | 500 |
| O1_node_oracle_par | 26.2% | 0.4965 | $0.000270 | 12.8% | 500 | 500 |

- **Scheduling headline**：B2 seq median 2000ms → B3/B4 par 500ms（p90 3500→500ms）；critical path/width/waste/utilization 见 json
- **O2 Schedule Oracle**：median 500ms —— scheduler regret=0（reference waves 已达 critical path 下界）
- **O3 Joint Oracle**：median 500ms（≤6 节点精确枚举）——L1 下 makespan 维无 joint 增益
- **Matched Random**：R1 cost-matched completion 12.3%（vs B4 13.9%，+1.6pp；target 截断注）｜R2 strong-share-matched 13.9% 持平、Q +0.71pp——content-aware 信号成立
- **Recovery ablation**：R0/R1 completion 0%→0%（n=10，CI 全 0）；extra_cost $0.00000844/wf
- **Coverage/Cost validity**：327 = test 284 + calibration 43（无未解释缺口）；R7 池 accuracy 类 cost 全 >0

