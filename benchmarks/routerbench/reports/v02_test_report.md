# RouterBench v0.2 · 冻结 test 报告（llmrb，n=12,345）

> 2026-10-06T13:29:23+00:00 · benchmark_id `lloom-routerbench-v0.2` · 池 R7_stability · policy P2（softgate0）
> 基线口径：saving/retention 相对池内 Always-Strong；accuracy-only 为自动评分 dataset 子集

| Strategy | Quality | Cost/query |
|---|---:|---:|
| Always Weak | 0.4058 | $0.000073 |
| Always Mid | 0.4767 | $0.000438 |
| Always Strong | 0.5860 | $0.000948 |
| Cheapest | 0.4058 | $0.000073 |
| Random | 0.4853 | $0.000483 |
| Best Single | 0.6053 | $0.000674 |
| Oracle | 0.7122 | $0.000118 |
| P0 Current | 0.5867 | $0.000849 |
| BandOnly | 0.5867 | $0.000849 |
| P2 Frozen ⭐ | 0.5863 | $0.000821 |
| Cost-matched Random | 0.5610 | $0.000819 |
| Strong-share-matched | 0.5749 | $0.000899 |

## Bootstrap 95% CI（B=10,000，seed=3407）

| 指标 | point | CI95 |
|---|---:|---|
| saving_vs_strong | +0.1346 | [+0.1220, +0.1473] |
| retention_vs_strong | +1.0005 | [+0.9957, +1.0052] |
| quality_delta_vs_cost_matched | +0.0252 | [+0.0207, +0.0297] |
| quality_delta_vs_strong_share_matched | +0.0114 | [+0.0075, +0.0153] |
| weak_precision | +0.7860 | [+0.7629, +0.8086] |

**accuracy-only 子集**（n=9705）：Q=0.6086 · saving=14.1% · retention=101.4%

## 发现

1. P2 对两个 matched random 均显著占优（CI 不含 0）→ weak 选择是 content-aware，非比例调参
2. retention CI 含 1.0 → 质量与 Always-Strong 无显著差，成本省 13.5%
3. budget curve 平坦：R7 池 strong 调用全部来自 hard 带（唯一合格模型），strong 预算无交换空间（结构属性）

## P2 per-dataset（Q / C per query）

| dataset | n | Q | C |
|---|---:|---:|---:|
| llmrb_aime_hybrid_v1 | 50 | 0.7400 | $0.002856 |
| llmrb_arcc_test_v1 | 917 | 0.8866 | $0.000348 |
| llmrb_arenahard_coding_test_v1 | 207 | 0.1280 | $0.000000 |
| llmrb_arenahard_creative_writing_test_v1 | 196 | 0.1658 | $0.000000 |
| llmrb_arenahard_math_test_v1 | 191 | 0.0785 | $0.000000 |
| llmrb_arenahard_test_v1 | 598 | 0.1296 | $0.000000 |
| llmrb_bbh_test_v1 | 886 | 0.8318 | $0.000250 |
| llmrb_emorynlp_test_v1 | 572 | 0.3741 | $0.000148 |
| llmrb_finqa_test_v1 | 912 | 0.7072 | $0.000600 |
| llmrb_gpqa_test_v1 | 157 | 0.5032 | $0.001711 |
| llmrb_humaneval_test_v1 | 122 | 0.4672 | $0.001922 |
| llmrb_kandk_test_v1 | 562 | 0.7883 | $0.000545 |
| llmrb_korbench_test_v1 | 954 | 0.5734 | $0.000904 |
| llmrb_livecodebench_test_v1 | 823 | 0.2564 | $0.003374 |
| llmrb_livemathbench_test_v1 | 98 | 0.7245 | $0.001517 |
| llmrb_math500_test_v1 | 397 | 0.9118 | $0.000873 |
| llmrb_mathbench_test_v1 | 124 | 0.6452 | $0.001796 |
| llmrb_mbpp_test_v1 | 774 | 0.5310 | $0.000311 |
| llmrb_medqa_test_v1 | 1031 | 0.6867 | $0.001116 |
| llmrb_meld_test_v1 | 964 | 0.5093 | $0.000171 |
| llmrb_mmlupro_test_1000_v1 | 805 | 0.5615 | $0.001606 |
| llmrb_winogrande_valid_v1 | 1005 | 0.7244 | $0.000800 |
