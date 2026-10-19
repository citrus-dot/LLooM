# 17 · RouterBench 实施总结（收官阶段 Day 9-15）

> 日期：2026-10-06 · 执行：ZCode（主会话，自主闭环授权范围内） · 范围：commit `e72a671` → `e2d9299`
> 定位：RouterBench 主线**收官阶段**的实施存档——Calibration Tournament、v0.2 冻结、冻结 test、matched random、bootstrap CI、cross-benchmark、Native 决策的全量记录；含与 OrchestrationBench 分叉会话的并行协作实践。
> 上游：`13-v2`（Weak Rescue 阶段入口）/ `16-v3`（联合调度层，本阶段的排期权威）/ `15-RouterBench实施总结-Day6-8`（上一阶段存档）/ `并行会话协作协议.md`（分叉窗口工程约束）。
> 状态：**RouterBench 主线研究收官（Day 15 决策 = Native hold），转入 maintenance；本文与 12/15 号共同构成 RouterBench 全程存档。**

---

## 一、总览与提交链

| commit | 阶段 | 内容概要 | 质量门 |
|---|---|---|---|
| `e72a671` | Day 9 | Calibration Tournament：R4-R7 四池型冠军 + 20 runs（5 entrant × 4 policy × llmrb calibration） | clippy 0 错 + 136 测试 |
| `8519149` | Day 11-13 | v0.2 冻结（七 hash）+ test replay + matched random ×2 + budget curve + bootstrap CI | 同上 |
| `e2d9299` | Day 14-15 | RouterBench 侧 tournament（反发现）+ cross-benchmark 分析 + Native 决策 | 同上 |

并行发生：OrchestrationBench 分叉会话完成 O-Day 9-11（`9a00070`/`70ab712`，schema/adapter/调度模拟器），与本会话共享工作树并行提交，协议执行良好（见 §八）。

---

## 二、Day 9 · Calibration Tournament（16-v3 §5）

### 2.1 四池型冠军挑选（calibration-only）

前置改造：`pool_search.py --dump-full` 导出**全量 733 池 × dataset 指标矩阵**（R7 stability 的计算前提；此前 JSON 只存每 dataset Top-10）。`tournament_select.py` 按 16-v3 §5.1 挑选（候选 142 个 ≥5 datasets 池，同名池去重顺延）：

| entrant | 池（weak/mid/strong） | ds | 选择依据 |
|---|---|---:|---|
| R4_weak_opportunity | deepseek_r1_0528_qwen3_8b / deepseek_r1_distill_qwen_7b / intern_s1_mini | 6 | max mean(weak_oracle_share)=**0.91** |
| R5_cost_leverage | deepseek_r1_0528_qwen3_8b / gemini_2_5_flash / gemini_2_5_pro | 5 | max geo(cost_ratio)=**80.2**（cheap_safe≥0.70 守卫） |
| R6_diversity | deepseek_v3_1_terminus / intern_s1_new / qwen3_235b_a22b_thinking_2507 | 5 | max mean(winner_entropy) |
| R7_stability ⭐ | deephermes_3_llama_3_8b_preview / deepseek_r1_distill_qwen_7b / intern_s1_mini | **13** | max median(PoolScore)=0.794（cross-dataset stability） |
| REF_global_top | llama_3_1_8b_instruct / llama_3_1_8b_ultramedical / qwen3_8b | 5 | max mean(pool_score) |

R5 的新增守卫：cheap_safe_mean≥0.70——防止选出"便宜但几乎不会做"的池（16-v3 §五.1 的 Cost Leverage 语义）。

### 2.2 Rust 扩展与 20 runs

- `bench replay --split calibration` + `--selection` 可选化 + manifest 容错（llmrb 未冻结矩阵回退占位 identity）；
- **池覆盖 Gate 1 过滤**：llmrb 各 dataset 模型宇宙不一，跨池仿真必须过滤（R7 池 5,460→3,136 实例）；RouterBench Core 100% 覆盖不受影响（回归 run 57 逐位一致）；`BenchInstance: Clone`；
- 20 runs = 5 entrant × {P0 Current / P1 BandOnly / P2 SoftGate(0) / BASE Always-Strong} × llmrb calibration（test 未触碰）。

### 2.3 Tournament 结论（`pool_tournament_calibration.{json,csv}`）

| entrant | 最优 policy | weak share | w_recall | w_precision | retention | saving vs BASE |
|---|---|---:|---:|---:|---:|---:|
| **R7_stability** | **P2** | 0→**9.7%** | 0.115 | **0.809** | **100.4%** | **12.6%** ⭐ |
| R4_weak_opp | P2 | 9.7% | 0.083 | 0.747 | 99.4% | 9.3% |
| R5_cost_lev | P0 | 0% | – | – | 94.3% | 6.9% |
| R6_diversity | 全部相同 | 0% | – | – | 100% | 0% |
| REF_global_top | P0≡P2 | 0% | – | – | 99.3% | 10.2% |

三条结论：**① Case B 换池实锤**——同套 P2，旧池 precision 0.572/Q −1.7pp，R7 池 precision 0.809/Q +0.4pp；**② P0≡P1 全池成立**——band 门槛主导是策略族普遍现象；**③ R6 反直觉**——winner_entropy 最高的池零杠杆，"矩阵多样性≠可路由性"，池筛选必须跑 policy 仿真。

---

## 三、Day 10 · kNN 条件判定（无代码，决策记录）

16-v3 §九 的触发条件："换池后 SoftGate 仍无法区分适合 Weak"。R7+P2 的 precision 已达 0.809 且质量不掉——**条件不成立，kNN 缓行**（移入备胎位：Freeze 后 test 复现精度塌陷再启）。理由：换池已基本解决问题，不应为"更复杂"加 query-level router（16-v3 §九 原文精神）。

---

## 四、Day 11 · Freeze（benchmark_id v0.2）

`freeze_v02.py` 产出（`manifests/`）：

- **benchmark_id = `lloom-routerbench-v0.2`**，manifest_hash `cbebcefd…`；
- **七 hash**（v0.1 六 hash + 新增 `policy_hash`）：benchmark/pool/policy/split/scorer/cost_basis——policy 结构正式进入冻结面（16-v3 §十四要求）；
- **Best Pool** = R7_stability（`model_pool_v02.yaml`，pool_id `llmrb_tournament_r7_v1`，含选择依据与 calibration 指标快照）；
- **Best Policy** = P2 Soft Eligibility（weak nominal tier 2 / penalty=0 / 生产 default 权重 / 质量先验=calibration 全局均分 / hit_rate 全零 / est 固定 profile）——快照写死进 manifest；
- **Test 名单** = llmrb test split × R7 池三模型全覆盖（覆盖是矩阵属性非抽样，确定性）→ 12,345 实例 × 22 datasets（`test_selection_v02.jsonl`）；
- **泄漏声明**：池/policy 选择全部发生在 calibration（Day 9 tournament），test 在冻结前未被任何策略读取（Day 6-9 全程 calibration-only）。

冻结规则：任一 hash 输入变化 = 新 benchmark_id；此后 test 只读。

---

## 五、Day 12 · 冻结 test replay + matched random + budget curve

### 5.1 Rust 扩展（Day 12 前置）

- `CostMatchedRandom` / `StrongShareMatchedRandom`：确定性加权采样（xorshift seed 3407），期望成本/P(strong) 对齐 LLooM（`--matched-target`）；cost-matched 用两点混合 {weak,strong}（成本杠杆最大），strong-share-matched 其余填 mid（隔离"weak 使用是否真有收益"）；
- **Budgeted-LlooM**（`--strong-budget`）：strong 调用预算占比，耗尽时按 `fallback_chain` 降级到首个非 strong 合格模型，hard 带无替代则如实超支计数（`budget_violation` 进 decision_json）；
- pool_id 从 manifest 读取（v0.2 起不再硬编码）。

### 5.2 test 结果（n=12,345，只读一次）

| 策略 | Quality | Cost/query |
|---|---:|---:|
| Always Strong | 0.5860 | $0.000948 |
| **Best Single**（逐 dataset calibration 选） | **0.6053** | $0.000674 |
| P0 Current | 0.5867 | $0.000849 |
| **P2 冻结（headline）** | **0.5863** | **$0.000821** |
| Cost-matched Random | 0.5610 | $0.000819 |
| Strong-share-matched Random | 0.5749 | $0.000899 |
| Oracle | 0.7122 | $0.000118 |

- **P2 vs BASE**：saving **13.5%** / retention **100.05%**（vs Always-Strong）；
- **matched random 双杀**：同成本 +2.52pp、同 strong 占比 +1.14pp——weak 选择是 **content-aware** 的（Gate 5 核心问题正面回答）；
- **新认知：llmrb 口径下 Best Single 是强基线**（0.6053 > Always-Strong 且更便宜）——"逐 dataset calibration 选单一模型"本身就是轻量路由；headline 对照系必须含它（16-v3 §三十二"是否超过简单基线放第一层"）。

### 5.3 budget curve 的结构性发现

λ→预算 0.1-0.9 全程平坦：**R7 池 P2 下 strong 调用 100% 来自 hard 带（唯一合格模型），medium 已全在 weak 上**——strong 预算旋钮在该池无可交换空间。这是池结构属性而非实现缺陷；budget-aware 路由的真实验场在中层竞争充分的池（cross-benchmark 未观察到）。

---

## 六、Day 13 · Bootstrap CI（Gate 7 统计）

`bootstrap_report.py`（B=10,000，seed=3407，**实例级重采样**——10 号 §13.10 口径，不把单模型输出行当独立样本）：

| 指标 | point | CI95 |
|---|---:|---|
| saving vs Always-Strong | **+13.46%** | [12.20, 14.73] |
| retention vs Always-Strong | +1.0005 | [0.9957, 1.0052]（含 1.0 = 质量无显著差） |
| Δquality vs cost-matched random | **+2.52pp** | [2.07, 2.97] |
| Δquality vs strong-share-matched | **+1.14pp** | [0.75, 1.53] |
| weak precision | 78.60% | [76.29, 80.86] |

accuracy-only 子集（自动评分 datasets，n=9,870）：Q=0.6086、saving **14.1%**。全部 headline CI 干净（Gate 7 ✅）。产物 `v02_test_report.{json,md}`。

---

## 七、Day 14-15 · Cross-benchmark 与 Native 决策

### 7.1 RouterBench 侧同法验证（反发现）

RouterBench 11 模型宇宙 Pool Search（9 跨 dataset 池）+ tournament（run 100-119）：**全部 entrant P0≡BASE、P2 仅 99 道 easy 带变化**——RouterBench 2024 世代矩阵上 weak rescue **不可行**。结构性原因：mistral-7b 质量先验 0.31 与 mid（Yi-34B）0.66 差距 0.35，等效单价差 ~4× 翻不动质量项——weak 放行了也不被选。

### 7.2 方向判定（`cross_benchmark_v02.json`）

- **机制一致** ✅：gate 因果链（RouterBench 100% Blocked / llmrb 同构）、P0≡P1（两源多池）、"可行性由弱模型质量先验与 mid 的差距决定"（池属性非 router 属性）——两源同构；
- **幅度不同**：模型代际决定 rescue 成本——llmrb 新世代高效小模型 rescue 近乎免费（retention 100.05%），RouterBench 旧世代代价大（Day 8：saving 36.9% 但 retention 90.7%、precision 0.572）；
- 定性：**边界条件，不是反证**。这比"两源都涨"更有研究价值——Weak Rescue 的适用边界被划清并可解释。

### 7.3 Day 15 Native 五条件核对与决策

| 条件（16-v3 §三十一） | 结果 |
|---|---|
| 1 冻结 test 优于 V0 | ✅ weak share 0→9.7%、precision 0.572→0.809、cost −3.3% |
| 2 retention guardrail | ✅ 100.05% [0.9957, 1.0052] |
| 3 saving 实质 | ✅ 13.46% [12.20, 14.73] |
| 4 matched random 被超 | ✅ 双对照 CI 不含 0 |
| 5 第二源方向一致 | ⚠️ 机制一致/幅度不同（partial，严格字面不满足） |

**决策（自主授权）**：**Native hold**——挂起至立项材料需要生产池数字时再采（2×2×50 sanity 起步，届时重录 qwen3.6 价格）。理由：v0.2 headline 已足够支撑立项；Native 是确认性实验（验证"llmrb 结构可迁移到生产池"）而非新结论；花钱方向不可逆，保守侧优先。**可被用户推翻**。RouterBench 同步转入 maintenance（16-v3 §二十三）。

---

## 八、并行会话协作（分叉窗口实践）

- **协议修订一次**：Day 9 开工时把"crates 全冻结"修订为按文件划分（Calibration Tournament 需要跑 plan() 仿真，bench.rs/main.rs bench 区归 RouterBench 会话）；
- **共享工作树裸 commit 事故一起**：`8519149` 无路径 commit 扫入分叉会话已暂存的 4 个 O-Day 10 文件（PlanBench adapter/evaluator/tests，990 行）——内容完整不回滚；协议补**「commit 必须路径限定」**铁律，此后按路径提交（`e2d9299` 实践验证）；
- 分叉会话进度：O-Day 9-11 完成（schema/validator、PlanningBench+PlanBench adapter、确定性 planning evaluator、schedule.py 调度模拟器 + orchestrator.rs **test-only** parity 缝），O-Day 12-14 进行中；
- 共享工作树下 cargo 文件锁自动串行、git push 曾出现远端先行（分叉会话先推），本会话 push 快进成功（共享本地仓库天然包含对方提交）。

---

## 九、诚实边界

1. **两套 headline 不同基线不可互比**：v0.2（llmrb）saving 13.5% 相对池内 Always-Strong；RouterBench（Day 8）36.9% 相对 v0.1 池 Always-Strong 且质量代价大——立项展示必须分源陈述；
2. **Best Single 在 llmrb 上强于 P2**（0.6053 vs 0.5863）——P2 的价值主张是"相对 Always-Strong 省钱不掉质量 + content-aware"，不是"全局最优"；headline 表必须含 Best Single 行；
3. **policy_hash 冻结的 P2 是 bench 变体**：penalty 语义（被 quality_weight 缩放）与未来生产侧 soft gate 公式存在差异，生产化需独立设计评审；
4. **weak regret potential 在 accuracy 类 dataset 上是 wos 的镜像**（regret∈{0,1}），独立信息量有限；judge 类 dataset 才显分布；
5. **matched random 是确定性单种子实现**（16-v3 §12.3 的"5 seeds 建议报告侧做"未执行——CI 已覆盖采样方差的主要部分）；
6. tournament calibration 数字（如 precision 0.809）与 test 数字（78.6%）有正常泛化落差，对外引用一律用 test CI；
7. RouterBench 侧 tournament 的 20 runs 未做 matched random（P2≡P0 无 weak 占比可对齐）——该侧 matched random 检验不可构造，属结构限制。

---

## 十、下一步

1. **OrchestrationBench 分叉会话支持**：O-Day 12-14 期间响应 Rust 协议问题/RouterBench 资产对接（Router Bridge 的 plan() 接线可复用 bench.rs ReplayCtx 模式）；O-Day 14 交接报告后执行**合并验收**（rebase → 质量门 → 确定性抽查 → Gate O-0~O-5 核对 → 文档登记）；
2. **Native hold 触发器**：立项材料需要生产池数字时重启（前置：qwen3.6 官方页价格重录、lm-eval `--limit 10` 抽查）；
3. **maintenance 池**（RouterBench 剩余低优先项）：v02 报告 WebUI 化、Single Query Replay API、AIQ golden fixtures——全部按需触发；
4. 分叉会话合并完成后：SystemBench Shadow 的进入条件检查（16-v3 §二十四：RouterBench Freeze ✅ + OrchestrationBench Offline Gate）。

---

## 附录 A · 产物路径（本阶段新增）

```text
benchmarks/routerbench/
├── manifests/
│   ├── model_pool_v02.yaml            # R7 池冻结（含选择依据 + calibration 快照）
│   ├── test_selection_v02.jsonl       # 12,345 test 实例冻结名单
│   └── frozen_manifest_v02.json       # benchmark_id v0.2 + 七 hash + 泄漏声明
├── reports/
│   ├── pool_search_full.json          # 733 池 × dataset 全指标矩阵（llmrb）
│   ├── pool_search_full_routerbench.json  # RouterBench 侧同构
│   ├── tournament_entrants{,_routerbench}.json
│   ├── pool_tournament_calibration.{json,csv}
│   ├── pareto_points_softgate.json
│   ├── weak_diagnosis.json
│   ├── v02_test_report.{json,md}      # Day 12-13 headline + CI
│   └── cross_benchmark_v02.json       # Day 14-15 两源分析 + 决策
└── scripts/
    ├── pool_search.py（--dump-full 扩展）
    ├── tournament_select.py / tournament_report.py
    ├── weak_diagnosis.py（per-dataset 扩展）
    ├── freeze_v02.py / bootstrap_report.py / cross_benchmark_v02.py
crates/lloom-core/src/bench.rs        # +2 matched random 策略、budgeted-LlooM、BenchInstance: Clone
crates/lloom-cli/src/main.rs          # --split/--selection/--matched-target/--strong-budget、pool_id from manifest
```

## 附录 B · 关键命令（仓库根目录）

```bash
# 全量池矩阵导出 + 四池型冠军
benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/pool_search.py \
  --dump-full benchmarks/routerbench/reports/pool_search_full.json
benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/tournament_select.py

# Calibration Tournament 单条（--split calibration；池覆盖自动过滤）
cargo run -q -p lloom-cli -- bench replay --strategy lloom --soft-gate 0 --split calibration \
  --normalized benchmarks/routerbench/normalized/llmrouterbench \
  --manifest benchmarks/routerbench/normalized/llmrouterbench/source_manifest.json \
  --models <weak> <mid> <strong>

# v0.2 冻结
benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/freeze_v02.py

# 冻结 test replay（P2 headline / matched random / budget）
cargo run -q -p lloom-cli -- bench replay --strategy lloom --soft-gate 0 \
  --manifest benchmarks/routerbench/manifests/frozen_manifest_v02.json \
  --selection benchmarks/routerbench/manifests/test_selection_v02.jsonl \
  --normalized benchmarks/routerbench/normalized/llmrouterbench \
  --models deephermes_3_llama_3_8b_preview deepseek_r1_distill_qwen_7b intern_s1_mini
cargo run -q -p lloom-cli -- bench replay --strategy cost_matched_random --matched-target 0.0008215 …

# Bootstrap CI + cross 分析
benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/bootstrap_report.py
benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/cross_benchmark_v02.py
```
