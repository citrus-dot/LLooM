# 15 · RouterBench 实施总结（Weak Rescue 阶段 Day 6-8 + 存储迁移）

> 日期：2026-10-06 · 执行：ZCode（自主闭环授权范围内） · 范围：commit `cc04f5c` → `9a33bf6`
> 定位：13-v2 号路线校正后的第一段实施（Day 6-8，零 API 成本）+ 存储迁移的**实施存档**。
> 上游：`13-LLooM-RouterBench-下一阶段实施计划-v2`（执行入口）/ `13-参考资料`（证据库）/ `12-RouterBench实施总结-Day1-5`（上一阶段存档）/ `11`（状态板，已同步）。
> 状态：**Day 6-8 完结并推送；下一步 = Day 9-10（Top 池 R4-R7 replay，零成本），见 §八。**

---

## 一、总览与提交链

| commit | 阶段 | 内容概要 | 质量门 |
|---|---|---|---|
| `cc04f5c` | Day 6 | LLMRouterBench 接入 canonical（541,152 outcomes）+ 30 dataset_id 全量 screening | clippy 0 错 + 135 测试 |
| `0e5fa7e` | Day 7 | Model Pool Search：28 dataset 枚举、208 跨 dataset 池、pool_candidates.json | 同上 |
| `f2515d2` | Day 8 | SoftGatePolicy + BandOnly 消融 + weak_diagnosis.py：Weak 0% 根因实锤 | 同上（bench 单测 6/6） |
| `9a33bf6` | 存储迁移 | 大文件迁外置盘 `/Volumes/Files/LLooM-bench/` + symlink + gitignore 适配 | 迁移前后数字逐位一致 |

全程零 API 成本（红线遵守），conventional commits，research/ 未入库。

---

## 二、Day 6 · LLMRouterBench 接入

### 2.1 实测结构与导入

release 实测比 13 号文档口径大一圈：**27 个 dataset 目录、700 个结果 JSON、40 个模型 slug**（解压后 6.5GB）。结构 `bench-release/<dataset>[/setting]/<model>/*.json`，目录深度不齐（arenahard 无 setting 层）→ 以 JSON 内 `dataset_name/split/model_name` 字段为权威，目录仅用于找文件。

`import_llmrouterbench.py` 产出（`normalized/llmrouterbench/`）：

- **27,034 实例 × 541,152 outcomes**，30 个 dataset_id（官方 split 编入 id，如 `llmrb_aime_hybrid_v1`/`llmrb_mmlupro_test_3000_v1`）；
- **raw/normalized 双轨**（13-v2 §6.2）：`raw_score/raw_cost` 与 `score/cost` 并存，v1 归一化策略 normalized=raw；`cost_basis=llmrouterbench_source` 与 RouterBench 分轨；
- sample_id 内容哈希派生（ds+setting+origin_query），与 record index/行序无关；哈希桶切分 20/80 seed=3407（与 Day 1 同法）；
- `raw_output` 不复制进 canonical（避免 6.5GB 重复落盘）：outcomes 带 `source_file/source_record_index` 溯源，原文留在 sha256 门控的 raw 归档；
- 丢弃如实记录：bad_record 4,327（simpleqa 一个坏文件整份丢弃）、dup_outcome 2,580（同模型重复时间戳文件，保留首条）。

完整性门：tar sha256 `b79f8cde…`（与 Day 1 记录一致）校验通过后解压。

### 2.2 首轮 screening（临时池，calibration）

临时池 qwen3-235b-a22b-2507 / deepseek-v3.1-terminus / claude-sonnet-4（frontier 池组的成本单调三元），14 个 dataset 出数（`screening_llmrouterbench.{json,csv}`）：

| 亮点 dataset | n(cal) | oracle_gain | 解读 |
|---|---:|---:|---|
| tau2 | 39 | **0.205** | 全场最大 headroom（tool use） |
| arenahard 三变体 | 46-153 | 0.163-0.188 | judge 分，cheap_safe 0.84+ |
| gpqa | 41 | 0.098 | stress 轨候选 |
| mmlupro_test_3000 | 609 | 0.056 | cheap_safe 0.941 |
| livemathbench / aime | 23 / 10 | **0.000** | qwen3-235b 本身逐题最优，零 headroom |

**数据层发现**：经典 20 组 dataset（bbh/math500/mbpp/winogrande/finqa/medqa 等 16 个）的模型集**全部是 7-9B 小模型阵营**（Llama-3.1-8B、Qwen3-8B、Nemotron-Nano 等），与 frontier 池组（aime/gpqa/livecodebench/arenahard/mmlupro_3000 等，最多 38 模型）天然分层——正好构成 Weak Rescue 的两套候选弹药库。

### 2.3 过程修复

- **潜在 bug（重要）**：`str.splitlines()` 会把 JSON 字符串里**合法的** U+2028/U+2029（LLM 数学文本常见）当行分隔符，把一行截断成非法 JSON——routerbench 没踩到纯属 prompt 碰巧没有。screening.py 的 jsonl 读取全部改 `split("\n")`；
- screening 的 `--tasks` 过滤去掉 `rb0shot_` 前缀硬编码（改 dataset_id 子串/task_family 匹配），跨数据源复用；
- `openrouter`（14,566 条）是聚合路由服务条目而非裸模型 → canonical 保留、pool_search 层 `EXCLUDE_MODELS` 排除。

---

## 三、Day 7 · Model Pool Search

`pool_search.py`（numpy 向量化，28 dataset 全枚举 10.7s）：

- **枚举域**：每个 dataset 的 calibration 实例 + 该 dataset 全覆盖模型宇宙（Gate 1 前置），成本单调三元池（weak<mid<strong 按 calibration 均成本）；
- **PoolScore**（13-v2 §26，仅 screening 排序不进生产）：`0.30×norm(oracle_gain) + 0.25×weak_oracle_share + 0.20×cheap_safe + 0.15×winner_entropy + 0.10×norm(log1p(cost_ratio))`，min-max 在 dataset 内归一；`weak_oracle_share = P(weak ∈ oracle-equivalent tie 集)`（tie-aware）；
- **产出**（`pool_candidates.json`）：208 个跨 dataset 池（≥5 datasets）+ 每 dataset Top-10 与三冠军（max_cost_leverage / max_weak_opportunity / max_diversity）。

**核心发现：Top 池 weak_oracle_share 普遍 0.70-0.83**——weak 成员在七成题目上本就是逐题最优解，Weak Rescue 的天花板远高于当前池（Mistral-7B 仅 43.4% 且被 gate 全挡）。两条候选线：

| 线 | 代表池（weak/mid/strong） | ds | og | weak_oracle_share | cheap_safe | cost× |
|---|---|---:|---:|---:|---:|---:|
| 小模型阵营（经典 20 组） | deephermes_3_llama_3_8b / deepseek_r1_distill_qwen_7b / intern_s1_mini | 13 | 0.150 | 0.70 | 0.77 | 17.4 |
| frontier 线 | deepseek_v3_0324 / glm_4_6 / gpt_5_chat | 7 | 0.119 | 0.76 | 0.84 | 7.1 |

---

## 四、Day 8 · Soft Gate 落地 + Weak 诊断闭环

### 4.1 实现（production 公式零改动）

- **SoftGatePolicy**（13-v2 §10.3 的"candidate eligibility + PlanInput"路径）：weak 的 nominal tier 1→2 使 medium band 门槛放行（hard 的 tier_req=3 天然仍拒），penalty 经 `quality_override` 通道折减质量估计：`q_weak − penalty×gap`（gap=band_req−true_tier，v1 仅 medium 放行 → gap 恒 1）。**语义注意**：penalty 会被 quality_weight 缩放（qw=0.4 时实际 0.4×penalty），已记入 parameter_json；
- **Strategy::BandOnly**（V1 消融）：band→tier→model 直接映射（easy→weak/medium→mid/hard→strong），无评分；
- CLI：`bench replay|sweep --soft-gate <p>`（给值即启用，含 0.0=pure eligibility）；单测 6/6 覆盖 V0/V2/hard 拒绝/penalty 单调/BandOnly 映射。

### 4.2 诊断结论（Core 1000 test，现池 R0-R3）

| 证据 | 数字 | 含义 |
|---|---|---|
| V0 lloom (run 38) ≡ BandOnly (run 39) | 逐题一致（Q=0.7087/C=$0.002826） | band 内评分从未推翻门槛——**plan() 的选择空间被 gate 完全定义** |
| V0 诊断（weak_diagnosis） | blocked_weak **434**（=Oracle weak share 43.4%）、Missed **0**、recall **0** | **Weak 0% = 100% Blocked**，H1 实锤；scoring 层无缺陷 |
| V2 soft gate 0（run 40） | weak share 0→**18%**、recall **0.237**、precision **0.572**、Q −1.7pp、cost −1.2%；blocked 434→331（hard 带按设计保持关闭）；risky_weak 77 | gate 打开 weak 即进场，但质量先验（0.31）分不清哪些 medium 题它能答 |
| penalty sweep 0.05-0.20（run 41-44） | weak 被完全挤出（结果与 V0 逐位同） | **全有全无**——Mistral-7B 结构性不可用，penalty 无操作空间 |
| λ 重扫（V2，run 45-54） | 两档→**三档**（λ≥0.5 weak 进场） | 曲线变 rich，但折中点 saving 仅 +1.2% |

**判定 = 13-v2 §四十 Case B + C 并存**：换池（Day 7 已备 weak_oracle_share 0.7+ 候选）与 query-level suitability（kNN）是下一杠杆；继续调 λ/penalty 已被证明无效。Cost Regret 基线也出来了（V0 mean $0.002062/p50 $0.000719——即便"最优"选择也常花着比 oracle-equivalent 贵 1-3 倍的钱）。

---

## 五、存储迁移（`9a33bf6`）

- **动机**：系统盘仅剩 6.7GB；bench 数据 8.4GB（raw 7.8G + normalized 585M + bench.db 28M）。
- **布局**：实体迁至外置 APFS 卷 `/Volumes/Files/LLooM-bench/routerbench/`；仓内 `raw`/`bench.db` 为 symlink，**normalized 保持本地真目录 + 4 个大 jsonl 文件级 symlink**（整目录 symlink 会让 git 视其为一个文件、已跟踪的 source_manifest.json 失联）；manifests/scripts/reports/.venv 留本地。
- **先验证后切换**：三重校验（源件 sha256 与 Day 1 记录逐位一致；本地/外置逐文件 size+name diff；sqlite integrity_check 54 runs/54,000 items）通过后才删除本地原数据。
- **功能回归**：`bench replay`（外置盘 sqlite WAL 写入正常）、`screening.py`（同参数逐字节复现 Day 2 结果）、`weak_diagnosis`、`pool_search`——全部与迁移前逐位一致。系统盘 6.7GB → **15GB**。
- **git 边角修正**：`benchmarks/**/raw/` 带尾斜杠不匹配 symlink（git 视 symlink 为文件）→ 改无斜杠；llmrouterbench/source_manifest.json 首次入库（此前被 `normalized/*` 规则误挡）。
- **运行注意**：外置盘未挂载时 symlink 悬空，bench 命令明确报错，重挂载即恢复；已写入 AGENTS.md「Bench 数据存储」节。

---

## 六、成果清单

- **数据**：LLMRouterBench canonical 矩阵（541,152 outcomes，双轨+溯源）；pool_candidates.json（208 池）；两套候选池线
- **代码**：import_llmrouterbench.py / pool_search.py / weak_diagnosis.py 三新脚本；bench.rs SoftGatePolicy + Strategy::BandOnly + CLI `--soft-gate`；单测 6/6
- **报告**：screening_llmrouterbench.{json,csv}、pool_candidates.json、weak_diagnosis.json、pareto_points_softgate.json（三档 λ 曲线）
- **基础设施**：存储外置 + symlink 布局（AGENTS.md 已记）；gitignore 适配
- **诊断资产**：Weak 0% 根因链完整存档（100% Blocked → gate 打开 → precision 0.572 → Case B+C）

## 七、诚实边界

1. LLMRouterBench 成本是**采集时点 source 价**（13-v2 §5.1），非当前牌价；与 RouterBench 成本口径分轨，headline 禁止跨源混算；
2. `weak_oracle_share` 是**逐题最优等价的天花板**，不是可实现的 recall；pool 排序公式仅 screening 用；
3. Soft Gate penalty 被 quality_weight 缩放（qw=0.4 → 实际 0.4×penalty），与未来生产侧 soft gate 公式存在语义差，跨 variant 比较需同口径；
4. 首轮 screening 用临时池，池条件性解读约束（12 号 §十一#3）继续适用；judge 类 dataset（hle/arenahard/simpleqa）分量为源量纲，binary/continuous 模式已自动区分但解释成本高；
5. simpleqa 有一个坏文件（4,327 条丢弃）→ 含该模型的池在该 dataset 覆盖率 <100%，由 Gate 1 前置自动排除；
6. 本阶段全部数字仍是**点估计**（bootstrap CI 留 Best Variant 冻结后）。

## 八、下一步（Day 9-10，继续零 API 成本）

1. **R4-R7 实验矩阵**（13-v2 §三十七）：在两条候选线各取 Top 池，跑 新池 × {V0, Soft Gate} × λ sweep——预期 weak_oracle_share 0.7+ 的池同时拉起 Weak Recall 与 Precision（对照 Gate B/C：precision 高、saving>36.1%、retention≥93%）；
2. 若新池下 precision 仍不达标 → **kNN query-level router**（TF-IDF + Top-K cosine，calibration-only，13-v2 Day 10）+ threshold sweep；
3. 达标后**冻结 Best Variant**（新 benchmark_id）→ 补 matched random（cost-matched / strong-share-matched）、budget curve、bootstrap CI（Gate 5/7）→ Day 13 唯一决策点：是否进 Native（届时才重录 qwen3.6 价格 + 2×2×50 sanity）。

## 附录 · 关键命令（仓库根目录）

```bash
# 导入 LLMRouterBench（tar sha256 门控在 raw 目录前置校验）
benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/import_llmrouterbench.py

# 全 dataset screening（指定池）
benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/screening.py \
  --input benchmarks/routerbench/normalized/llmrouterbench \
  --models qwen3-235b-a22b-2507 deepseek-v3.1-terminus claude-sonnet-4 \
  --output benchmarks/routerbench/reports/screening_llmrouterbench.json

# Pool Search（calibration-only 枚举）
benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/pool_search.py

# 消融：V0 全基线 / V1 BandOnly / V2 soft gate 0 / penalty sweep / λ 重扫
cargo run -q -p lloom-cli -- bench replay --strategy all --models …
cargo run -q -p lloom-cli -- bench replay --strategy band_only --models …
cargo run -q -p lloom-cli -- bench replay --strategy lloom --soft-gate 0 --models …
cargo run -q -p lloom-cli -- bench sweep --soft-gate 0 --models … \
  --output benchmarks/routerbench/reports/pareto_points_softgate.json

# Weak 诊断（Blocked/Missed/Recall/Precision/Cost Regret）
benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/weak_diagnosis.py --run 38 40
```
