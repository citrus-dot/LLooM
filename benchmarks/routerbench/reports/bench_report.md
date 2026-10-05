# RouterBench v0.1 · Public Track 报告

> 生成于 2026-10-05T07:12:41+00:00 · 数据源 bench.db（各策略最新 run）

## 全量（Core 1000 test 实例）

| Strategy | Quality | Cost / query | Regret P50 | 选择分布 |
|---|---:|---:|---:|---|
| always_weak | 0.3475 | $0.000045 | 0.50 | chat:1000 |
| always_mid | 0.6268 | $0.000190 | 0.00 | chat:1000 |
| always_strong | 0.7628 | $0.004423 | 0.00 | preview:1000 |
| cheapest | 0.3475 | $0.000045 | 0.50 | chat:1000 |
| random | 0.6080 | $0.001614 | 0.00 | preview:345, chat:313, chat:342 |
| best_single | 0.7628 | $0.004423 | 0.00 | preview:1000 |
| oracle | 0.8393 | $0.001258 | 0.00 | preview:183, chat:434, chat:383 |
| lloom | 0.7087 | $0.002826 | 0.00 | preview:820, chat:180 |

**LLooM（λ=0.5，run 25）**：saving 36.1% · retention 92.9% · gap_recovery 87.0%

**AIQ** = 0.7036（3 点前沿）

## Pareto 前沿（cost ↑ 排序）

- always_weak: C=$0.000045 Q=0.3475
- always_mid: C=$0.000190 Q=0.6268
- oracle: C=$0.001258 Q=0.8393

## λ sweep（LLooM cost_weight 曲线）

| λ | Quality | Cost/query |
|---:|---:|---:|
| 0.0 | 0.7628 | $0.004423 |
| 0.1 | 0.7628 | $0.004423 |
| 0.2 | 0.7628 | $0.004423 |
| 0.3 | 0.7087 | $0.002826 |
| 0.4 | 0.7087 | $0.002826 |
| 0.5 | 0.7087 | $0.002826 |
| 0.6 | 0.7087 | $0.002826 |
| 0.7 | 0.7087 | $0.002826 |
| 0.8 | 0.7087 | $0.002826 |
| 0.9 | 0.7087 | $0.002826 |

## Per-dataset（Q / C per query）

### rb0shot_gsm8k_v1

| Strategy | Quality | Cost |
|---|---:|---:|
| always_weak | 0.4175 | $0.000094 |
| always_mid | 0.5637 | $0.000388 |
| always_strong | 0.6538 | $0.008447 |
| cheapest | 0.4175 | $0.000094 |
| random | 0.5550 | $0.002883 |
| best_single | 0.6538 | $0.008447 |
| oracle | 0.6963 | $0.002543 |
| lloom | 0.6538 | $0.008447 |

### rb0shot_hellaswag_v1

| Strategy | Quality | Cost |
|---|---:|---:|
| always_weak | 0.2550 | $0.000045 |
| always_mid | 0.7850 | $0.000178 |
| always_strong | 0.8400 | $0.002257 |
| cheapest | 0.2550 | $0.000045 |
| random | 0.6600 | $0.000729 |
| best_single | 0.8400 | $0.002257 |
| oracle | 0.9400 | $0.000379 |
| lloom | 0.8400 | $0.002257 |

### rb0shot_mbpp_v1

| Strategy | Quality | Cost |
|---|---:|---:|
| always_weak | 0.3050 | $0.000048 |
| always_mid | 0.3850 | $0.000232 |
| always_strong | 0.6950 | $0.009449 |
| cheapest | 0.3050 | $0.000048 |
| random | 0.4950 | $0.003712 |
| best_single | 0.6950 | $0.009449 |
| oracle | 0.7350 | $0.003026 |
| lloom | 0.4250 | $0.001463 |

### rb0shot_mmlu_v1

| Strategy | Quality | Cost |
|---|---:|---:|
| always_weak | 0.2100 | $0.000028 |
| always_mid | 0.6900 | $0.000112 |
| always_strong | 0.8000 | $0.001421 |
| cheapest | 0.2100 | $0.000028 |
| random | 0.6050 | $0.000542 |
| best_single | 0.8000 | $0.001421 |
| oracle | 0.8900 | $0.000252 |
| lloom | 0.8000 | $0.001421 |

### rb0shot_winogrande_v1

| Strategy | Quality | Cost |
|---|---:|---:|
| always_weak | 0.5500 | $0.000010 |
| always_mid | 0.7100 | $0.000040 |
| always_strong | 0.8250 | $0.000540 |
| cheapest | 0.5500 | $0.000010 |
| random | 0.7250 | $0.000204 |
| best_single | 0.8250 | $0.000540 |
| oracle | 0.9350 | $0.000092 |
| lloom | 0.8250 | $0.000540 |

