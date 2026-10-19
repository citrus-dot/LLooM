# 11 · RouterBench 实施总计划（调度执行层）

> 日期：2026-10-04 · 代码基线 `v2` @ `412ea10`
> 定位：**本阶段实施的唯一入口调度文档**。方案原理与完整规格在 09（重写稿）、10（实施规格 v0.1），本文只做三件事：存档开工决策原话、维护方案状态板、把 Day 1-7 排期与 09/10 的章节锚点 + 本轮代码实测结论缝合成可照做的执行清单。
> 状态：**Day 1-5 已完成（2026-10-05，commits `0a45b01`/`58e036c`/`9212d13`/`561e807` 已推送），四项待定全部拍板（1a/2a/3a/4a），Day 6 Native 自采等 Nironta 通报后开跑**。细节冲突时以 10 > 09 > 本文为准（10 是实施规格）。

---

## 一、开工决策原话（逐字存档，2026-10-04）

> Day 1
> RouterBench 0-shot 下载
>         ↓
> import_frozen.py
>         ↓
> screening.py
>
> Day 2
> 算出所有候选 benchmark 的
> ModelGap / OracleGain / CheapSafe / UpgradeOpportunity
>         ↓
> 选正式 workload
>
> Day 3
> 冻结 model_pool.yaml
> 冻结 datasets.yaml
> 生成 benchmark manifest hash
>
> Day 4
> 实现 bench.rs
>         ↓
> 复用 production plan()/score_all()
>
> Day 5
> 完成 Cheapest / Weak / Mid / Strong / Random / Oracle
>         ↓
> 第一张 Pareto 图
>
> Day 6+
> 开始 Native Pool 自采
>         ↓
> lm-eval
>         ↓
> 1400-instance frozen matrix
>
> 之后
> Calibration
> → Replay
> → CI
> → WebUI

（配套补充指令：「相关文件我已经复制进 research/nowdoing，你可以进行整理，做一份总的计划指导之后的实施，然后将 09、10 两份方案放入 research/手记规划-20260928/，整理方案状态方便开始进行」。）

---

## 二、方案状态板

> ### 🏁 Day 14-15 完成（2026-10-06，commit `e2d9299`）——**RouterBench 主线研究收官，转入 maintenance**
>
> **Day 14 cross-benchmark**：RouterBench 侧同法 Pool Search（9 跨 dataset 池）+ tournament（run 100-119）。**反发现**：全部 entrant P0≡BASE、P2 仅 99 道 easy 带变化——RouterBench 2024 世代矩阵上 weak rescue 不可行（mistral-7b 质量先验 0.31 vs mid 0.66 差距 0.35，放行也不被选）。
> **方向判定**（cross_benchmark_v02.json）：**机制一致**（gate 因果链/P0≡P1/可行性由池决定，两源同构）；**幅度不同**（模型代际：llmrb 新世代小模型 rescue 近乎免费 vs RouterBench 代价大 retention 90.7%）——是边界条件不是反证。
> **Day 15 Native 五条件**：4/5 全过（frozen test 改善 ✅ / retention guardrail ✅ / saving 实质 ✅ / matched random 双杀 ✅）+ 第 5 项（第二源一致）partial。
> **决策（自行决策授权）**：**Native hold**——挂起至立项材料需要生产池数字时再采（2×2×50 sanity 起步）；RouterBench 转 maintenance（16-v3 §二十三），本会话资源切向 OrchestrationBench 合并验收与支持。推翻此决策请直接说。

> ### ✅ Day 11-13 完成（2026-10-06，commit `8519149`）——**RouterBench 研究结论成形**
>
> ①**v0.2 冻结**：R7_stability+P2 → `lloom-routerbench-v0.2`（七 hash 含 policy_hash；test 名单 12,345×22 datasets，泄漏声明=Day 6-9 全程 calibration-only）；②**test replay**：P2 Q=0.5863，saving 13.5%/retention 100.05% vs Always-Strong；llmrb 口径下 Best Single（0.6053，逐 dataset 混合）反超 Always-Strong——是强基线；③**matched random 双双被显著压过**（同成本 +2.52pp CI[2.07,2.97] / 同 strong 占比 +1.14pp CI[0.75,1.53]）——content-aware 实证（Gate 5 ✅）；④**budget curve 结构性平坦**（strong 调用全部来自 hard 带，预算无交换空间）；⑤**bootstrap CI**（Gate 7 统计 ✅）：saving 13.46% [12.20,14.73]、retention 1.0005 [0.9957,1.0052]、weak precision 78.6% [76.3,80.9]；accuracy-only saving 14.1%。**待办**：Day 14 cross-benchmark（RouterBench 侧同法选池+统一 policy）→ Day 15 Native 决策。

>
> ### ✅ Day 9 Calibration Tournament 完成（2026-10-06，commit `e72a671`）
>
> 16-v3 §5 落地：R4-R7 四池型冠军（calibration-only）× {P0/P1/P2/BASE} × llmrb calibration 共 20 runs。**结论**：①P0≡P1 全池成立（band 门槛主导是普遍现象）；②**R7_stability + P2 = Pareto vs BASE**（weak share 0→9.7%、recall 0.115、precision **0.809**、retention 100.4%、saving 12.6%）——**Case B 换池实锤**；③kNN 条件不成立，**缓行**；④R6 证明"矩阵多样性≠可路由性"。产物：pool_tournament_calibration.{json,csv} + tournament_entrants.json + pool_search_full.json（733 池全矩阵）。下一步 Day 11 Freeze（R7+P2 → benchmark_id v0.2）。

>
> ### ✅ Weak Rescue 阶段 Day 6-8 完成（2026-10-06，commits `cc04f5c`/`0e5fa7e`/`f2515d2` 已推送；实施存档见 **15 号文档**，含存储迁移 `9a33bf6`）
>
> 照 13-v2 §四十七 六步全做完（零 API 成本），关键结论：
> - **Day 6**：LLMRouterBench 接入 canonical（27 dataset/700 文件 → 30 dataset_id，27,034 实例 × 541,152 outcomes，raw/normalized 双轨）；首轮 14 dataset screening（tau2 oracle_gain 0.205 / arenahard ~0.165）；经典 20 组 dataset 模型集=7-9B 小模型阵营
> - **Day 7**：Pool Search（28 dataset 枚举，208 跨 dataset 池）：**Top 池 weak_oracle_share 0.70-0.83**——Weak Rescue 空间远大于当前池；小模型阵营（deephermes/r1-distill/intern-s1-mini）与 frontier 组（deepseek_v3_0324/glm_4_6/gpt_5_chat）两条候选线
> - **Day 8**：①V0 lloom ≡ BandOnly（逐题一致）——当前池上 plan() 选择空间被 band 门槛完全定义；②Weak 0% 根因=**100% Blocked**（blocked 434=Oracle weak share，Missed=0）；③V2 soft gate：weak share 0→18%、recall 0.237/precision 0.572、Q −1.7pp；penalty≥0.05 即全挤出（Mistral-7B 质量先验 0.31 结构性过低）；④判定=**13-v2 §四十 Case B+C 并存**：换池 + query-level suitability 是下一杠杆，继续调 λ/penalty 无效
> - 下一步（Day 9-10）：按 13-v2，新池 R4-R7 replay（Pool Search Top 池 × Soft Gate × λ）→ 若 weak recall/precision 仍不达标 → kNN

>
> ### ⚠️ 路线校正（2026-10-06，依据 13-v2 号文档）
>
> 13-v2（`13-LLooM-RouterBench-下一阶段实施计划-v2.md`）重新收束了 Day 6 之后的路线，与本文冲突之处**以 13-v2 为准**：
>
> 1. **Native 自采移出关键路径**：本文 §三 Day 6+ 清单（价格重录/lm-eval 抽查/1400 实例矩阵）与 §2.2 P2 时点全部**降级为 P3 可选项**——触发条件改为「零成本阶段（Pool Search/Soft Gate/kNN）跑出稳定增益后」才允许进入，且首轮缩减为 2 models × 2 datasets × 50 samples sanity check。
> 2. **问题重定义**：Weak share 0% 的主线从「评测基建」转向 **Weak Rescue**——拆分 Blocked Weak（gate 问题）/ Missed Weak（scoring 问题），新指标族 Weak Recall/Precision、Weak Blocking Rate、Cost Regret、matched random 对照。这与 12 号 §十一发现#2（band 门槛是路由质量主源）互为印证：该观察从「生产行为记录」升级为「修复对象」。
> 3. **模型池产生方式变更**：人工初选（Day 2/3 的单调阶梯+成本梯度）升级为 **13 模型全枚举 Pool Search**（LLMRouterBench Performance-Cost，PoolScore 加权公式）；现有池降级为 R0 control。
> 4. **实现红线补充**：Soft Gate 等 benchmark variant 只走「candidate eligibility + PlanInput（quality_override 通道）」，生产 `router.rs`/`score_all()` 不动；kNN/阈值/池选择全部 calibration-only。
> 5. **新执行排期**：Day 6-13 见 13-v2 §四十三（Day 6 = LLMRouterBench 导入+screening，零成本）。本文 Day 1-5 清单与已完成记录继续有效。
>
> 参考资料：`13-参考资料.md`（外部 benchmark/路由研究证据库；编号与 13-v2 冲突，建议改号为 14，未改前以文件名区分）。

### 2.1 已拍板（不再讨论）

| 决策 | 内容 | 权威落点 |
|---|---|---|
| 主线 | LLooM RouterBench v0.1：固定池 + 冻结结果矩阵 + 真实 plan() + 零成本回放 | 09 §零/§二十一 |
| 三 Track 架构 | A Public Screening（基建验证）/ B Native Pool（正式 headline）/ C Showcase-Stress | 09 §二 |
| 数据源优先级 | RouterBench 0-shot（bootstrap）→ LLMRouterBench release（P1）→ Native 自采 | 10 §1.1/§1.2/§23.1 |
| 模型池 | 首选同系三层池 qwen3.6-flash/plus/max-preview（**screen_then_freeze，未最终冻结**）；v0.2 异构池 | 10 §3.2/§23.2，manifest 草案已备 |
| Native 规模 | 1400 实例：MMLU-Pro 300 / MATH500 300 / GSM8K 300 / MBPP 300 / IFEval 200；20/80 切分 seed=3407 | 10 §23.4 |
| Screening 门槛 | Coverage=100% ∧ OracleGain>0 ∧ UpgradeOpp>5% ∧ CheapSafe>10% ∧ CostRatio≥3×（工程 heuristic 非定理） | 10 §5.1/§23.5 |
| Baseline | 8 条：Always Weak/Mid/Strong、Cheapest、Random、Best Single、Oracle、LLooM | 10 §11/§12 |
| 泄漏防护 | calibration/test 严格隔离；test 分数禁入 quality_override；Best Single 只由 calibration 决定 | 09 §六、10 §七 |
| 决策真源红线 | replay 复用 production plan()/score_all()，禁止第二套公式；plan parity test 是第一道 Rust gate | 10 §九/§二十四 |
| 早期冻结清单 | latency 不进 headline、semantic cache 不算 routing 质量、shadow 结果不混 public matrix 等十条 | 10 §三十六 |
| v0.1 不做 | 5-7 模型池、kNN/RouteLLM baseline、latency-aware、多模态——全部 v0.2+ | 10 §三十二/§三十六 |

### 2.2 待定（开工前/过程中拍板，不阻塞 Day 1-2）

| # | 事项 | 选项 | 我的推荐 |
|---|---|---|---|
| P1 | bench_* 表落点 | a) 独立 `benchmarks/routerbench/bench.db` b) 并入 `data/lloom.db`（走 ensure_columns 惯例） | **a**：生产库零污染，零成本红线（10 §9.3「production 表 read-only」）物理化；CLI 照常可开两个连接 |
| P2 | 模型池价格核录 | manifest 草案价格为转述值 | Day 6 自采前从官方页面重录（草案 source_note 已自带此要求） |
| P3 | Demo evidence set 冻结时点 | replay 后自动选（10 §三十一三类 case） | 无需拍板，照 10 执行 |

### 2.3 已验证就绪（2026-10-04 实测，Day 1/4 的风险已解除）

| 项 | 结论 | 证据 |
|---|---|---|
| RouterBench 0-shot 下载 | **hf-mirror 直链可用**：302→200，Content-Length 99,567,659（99.6MB，与 10 §1.1 一致） | 本轮 curl -sIL 实测（此前 09 旧稿记载的 401 系查错仓库名 `routerbench_dataset`，正确仓库为 `withmartian/routerbench` + `/resolve/` 路径） |
| LLMRouterBench release | hf-mirror 直链可用：200，1,283,503,080（1.28GB，与 10 §1.2 一致） | 同上 |
| `plan()` 纯函数无 DB 依赖 | bench.rs 可直接调 | `router.rs:247` |
| decision_json 无需改可见性 | `PlanOutcome.candidates: Vec<Candidate>`（name/score/est_cost/quality/capability_tier）已 pub，与 10 §二十三 decision trace 字段一一对应 | `router.rs:198-205,169-175` |
| `score_all` 私有但无碍 | 走 PlanOutcome.candidates 即可，**不复制公式红线天然满足** | `router.rs:477` |
| `band_for` 确定性零 LLM | Plan Benchmark（B0）band 可用 | `router.rs:86` |
| 虚拟模型内存构造 | Model 全 pub 字段 + serde default，有测试 fixture 先例 | `models.rs:227-252,329` |
| RoutingPolicy::default | bench 可用 Default 或 manifest 冻结快照 | `models.rs:515-529` |
| /v1 非流式（Native 直连用） | `stream: Option<bool>` + unwrap_or(false) 分支 | `openai_compat.rs:49,369,390` |
| PlanInput 组装先例 | route() 外组装 PlanInput 的现成模式 | `review.rs:131,156` |
| starter manifests | 三份 yaml 草案（datasets/model_pool/task_mapping，含 screen_then_freeze 标记）已归档 | `research/手记规划-20260928/manifests-v0.1-starter/` |

---

## 三、Day 1-7 执行清单（排期照原话，锚点与增量标注）

> 图例：〔10 §x〕= 实施规格锚点；〔增量〕= 本轮实测/评审新增，09/10 未覆盖或修正；✅ = 完成判据。

### Day 1 · 下载 + import_frozen.py + screening.py

> ✅ **已完成（2026-10-05，commit `0a45b01`）**。实际产出与执行偏差：
> - 数据：36,497 实例 × 11 模型 = 401,467 outcomes 全覆盖零丢弃；双下载件 sha256 已录 source_manifest（pkl `ba4f77f1…`、tar.gz `b79f8cde…`）
> - 命名照开工指令用 `screening.py`（10 号 §17 写的是 screen.py，以开工指令/本文为准）
> - **切分实现改哈希桶**（sha256("lloom-split:seed:sample_id") 归一 < 0.2 → calibration）替代 shuffle：Mimosa 拦截 random 模块，且哈希桶跨机/跨运行可复现性更强；逐 dataset 实际计数见 source_manifest（统计收敛 20/80）
> - **screening 默认只算 calibration split**（10 号 §7.3：benchmark 选择过程不得污染 test），--split 可改——这是我补的执行决策，原文未明说
> - normalized/ 148MB+40MB 不入库：.gitignore 负向规则只保留 source_manifest.json（矩阵可由 raw + 导入器确定性重建）
> - bench 专用 venv 在 `benchmarks/routerbench/.venv`（uv，pandas 3.0.6/numpy 2.5.3）；产品 .venv 已随纯 Rust 迁移删除
> - screening 结果（临时池 mistral-7b/Yi-34B/GPT-4，按单调能力阶梯+最大成本梯度初选；全池均分 mistral 0.308 / Yi-34B 0.663 / GPT-4 0.795）：7 候选全过 Gate，rank：winogrande > mmlu > hellaswag > gsm8k > mbpp > mt_bench > arc（gsm8k upgrade_opp 最高 0.651；arc oracle_gain 仅 0.016 垫底；mt_bench/mbpp calibration n=16/78 偏小，Day 3 判断要看 test 侧）

1. 建目录 `benchmarks/routerbench/{raw,normalized,manifests,reports,scripts}`〔10 §16/§27〕；**`.gitignore` 加 `benchmarks/**/raw/`、`benchmarks/**/data/`**〔增量：99.6MB/1.28GB 不入库〕。
2. 下载（实测可用命令，勿改仓库名）：
   ```bash
   curl -L -o benchmarks/routerbench/raw/routerbench/routerbench_0shot.pkl \
     "https://hf-mirror.com/datasets/withmartian/routerbench/resolve/main/routerbench_0shot.pkl"
   curl -L -o benchmarks/routerbench/raw/llmrouterbench/bench-release.tar.gz \
     "https://hf-mirror.com/datasets/NPULH/LLMRouterBench/resolve/main/bench-release.tar.gz"
   ```
   落地后 sha256 记录进 source_manifest〔10 §6.2〕。
3. 写 `import_frozen.py`（pkl → canonical instances.jsonl/outcomes.jsonl）〔10 §6.1/§16.1〕。**〔增量〕导入时即生成 calibration/test 切分**（20/80，seed=3407，写入每行 split 字段与 manifest）——这是 09 §六/10 §七的泄漏防护前置，不能等「之后」。
4. 写 `screening.py`（S1-S7 七信号）〔10 §4.2/§17〕。
   ✅ pkl 可读、sample_id 去重、score/cost 非空〔10 §38 Day1 验收〕。

### Day 2 · 算七信号 + 选正式 workload

1. 对候选 bench 全集跑 screening〔10 §4.1 第一轮：mmlu/hellaswag/arc/winogrande/gsm8k/mbpp/mt_bench〕。
2. **〔增量〕screening 用临时池**：逐题互补指标是相对池的量，池 Day 3 才冻结——临时池取 public matrix 内初选 top-3，`screen.py --models` 显式传入，screening.json 记录所用池（`provisional_pool: true`）。
3. 按 10 §5.1 Dataset Gate + §23.5 阈值初筛（不硬设研究结论阈值）。
   ✅ screening.json/csv 产出，正式 workload 候选名单成形〔10 §38 Day2〕。

### Day 3 · 冻结 manifest + hash

1. 以 `research/手记规划-20260928/manifests-v0.1-starter/` 三份 yaml 为底稿，填入 screening 结果 → `model_pool.yaml` / `datasets.yaml` / `task_mapping.yaml`；生成 manifest hash（benchmark/pool/prompt_profile/scorer/split/cost_basis 六 hash）〔10 §6.2/§24 Step2〕。
2. 冻结后任何字段变更 = 新 benchmark_id〔10 §6.2〕。
   ✅ `v0.1-core.yaml` 等产出，Core 3-5 datasets、~900-1800 samples 确定〔10 §38 Day3〕。

### Day 4 · bench.rs（复用 production plan()）

> ✅ **已完成（2026-10-05，commit `9212d13`）**。要点与偏差：
> - `lloom-core/src/bench.rs`：FrozenMatrix 加载（名单过滤+split 自检）/ 8 策略回放 / 指标汇总 / bench.db（决策 1a 独立库）；CLI `lloom-cli bench replay`（本地离线，lloom-cli 新增 lloom-core 依赖——此前为纯 HTTP 客户端）
> - parity test 达成（bench 组装 == production 语义手工组装 → plan() 决策全等）
> - **发现并修正**：RoutingPolicy::default 的 min_capability_tier=2 会把 weak 挡出全池 → bench policy 显式 min_tier=1；band 门槛（medium→tier2 / hard→tier3）让 plan() 的选择空间天然分带，这是生产行为本身
> - 效率单价 p_m 由 calibration 统计导出（input=output 同价），est 固定 profile（chars×0.6/750）、hit_rate 全零、t_epoch=0

### Day 5 · 8 baselines + 第一张 Pareto

> ✅ **已完成（2026-10-05，commit `561e807`）**。首版结果（Core 1000 test 实例，池 mistral-7b/Yi-34B/GPT-4）：
> - **LLooM（λ=0.5，w/o 校准）：Q=0.7087 C=$0.002826/q，saving 36.1% / retention 92.9% / gap_recovery 87.0%**；选择分布 GPT-4 820 / Yi-34B 180（非 trivial ✓）
> - Oracle Q=0.8393 C=$0.001258（43% 的题 weak 即 winner，cheap-safe 空间大）；Best Single=Always Strong（GPT-4 calibration 均分最高）
> - λ sweep 两档曲线：λ≤0.2 → Q=0.763/C=$0.00442（近 Strong）；λ≥0.3 → Q=0.709/C=$0.00283（折中）；pareto_points.json
> - **E5 结论（负结果，如实记录）**：w/ vs w/o quality_override 零增益——静态矩阵上任务级 EWMA 校准不改变任何决策（与先验同源同分布）；EWMA 的真正舞台是 Native Track 数据漂移
> - **quality_score 设计决策**：池模型冷启动先验 = calibration 全局均分（修正初版 0.5 抹平质量维度导致 λ 无效的问题）；w/o=全局先验 vs w/=任务级 override 的 E5 对照由此成立
> - AIQ=0.7036（3 点前沿，凸包梯形积分，report.py）；`reports/bench_report.{json,md}` §四十一 表格产出
> - Gate 2/3/4/5/6 全达成；8 baselines 全出（Gate 5）；bootstrap CI 留 Native headline 前（§13.10）
>
> 原 Day 4/5 执行清单要点（锚点存档）：四件事〔10 §12.1〕/ 不塞 router.rs〔10 §12.2〕/ decision_json 序列化 `PlanOutcome.candidates`（`router.rs:201`）/ est 只用 calibration 统计或固定 profile〔`router.rs:605-607` 冷启动 750〕/ parity fixture〔10 §二十四〕/ Baselines 定义〔10 §12〕/ Random seed=3407 / AIQ+λ sweep〔10 §14〕——均已按此落地。

### Day 6+ · Native Pool 自采（开始花钱）

前置五条件（10 §19）：Public screening 完成 / replay 跑通 / parity 通过 / outcome schema 冻结 / model_pool.yaml 冻结。
1. 固定请求 profile（temperature=0 等〔10 §20.1〕）；**逐模型固定直连，不经 router**〔10 §20.2〕。
2. 首轮 3 models × 3 datasets × 50-100 样本小规模验证 API 兼容/答案抽取/计价〔10 §38 Day6〕；先 `--limit 10` 抽查〔10 §19 lm-eval 惯例〕。
3. 全量 1400 实例矩阵（MMLU-Pro 300 / MATH500 300 / GSM8K 300 / MBPP 300 / IFEval 200）〔10 §23.4〕；cost 按逐样本真实 token 计，缓存/峰谷字段显式入 manifest〔10 §二十一〕。
4. freeze.py 六 hash + calibration → replay → bootstrap CI（10k resamples，seed 对齐 10 §13.10）。
   ✅ Native frozen matrix + 正式 headline 数据。

### 之后 · Calibration→Replay→CI→WebUI（对应 09 §十七/10 §二十九）

- WebUI 四组件（Overview/Pareto/Selection Distribution/Per-instance Explorer）全部吃 bench_runs + bench_run_items 真数据，无 mock〔10 §二十九/§三十七 Gate7〕；
- Single Query Replay（`POST /api/bench/replay/one`）+ 三类展示样本自动挑选（Cheap Win / Smart Escalation / Mid Sweet Spot）〔10 §三十/§三十一〕；
- 五幕演示故事线照 09 §十五/10 §四十二。

---

## 四、强制顺序约束（防泄漏三则，执行时打勾）

- [x] split 在导入时生成（Day 1），晚于此的任何「重切」= 新 benchmark_id；（2026-10-05 ✅ 哈希桶实现）
- [ ] est_in/est_out/hit_rate 只来自 calibration 统计/固定 profile/全零，不触碰 test outcome；
- [ ] Best Single 与 quality_override 只由 calibration split 决定；test 分数只出现在 evaluator。

---

## 五、Gate 与提交索引

- 八道验收 Gate：Gate 0 数据获取 → Gate 7 UI 真数据〔10 §三十七〕，外加 parity gate〔10 §二十四〕；
- 12 个 commit 粒度〔10 §三十〕：每个 `cargo test && cargo clippy` 全绿；
- 43 节成功最低条件（十条）〔10 §四十三〕——立项展示成立的判据，不要求 v0.1 全能。

---

## 六、与文档族的关系

| 文档 | 角色 |
|---|---|
| 本文（11） | 调度执行层：状态板 + Day 清单 + 约束打勾表 |
| 09（重写稿） | 方案原理：为什么是 Router Benchmark、三 Track、指标体系、演示动线 |
| 10 | 实施规格：一切细节的权威落点（schema/命令/阈值/判读规则） |
| 09-superseded | 我 2026-10-04 旧稿，保留备查；其 B2 聚合喂分的泄漏缺陷已在重写稿修正 |
| 00 | 文档族索引（已同步 09/10/11） |
| 00-08 | 主线路由/优化规划——本阶段暂停推进其优先级排序，按 09 §二十收束到 RouterBench 主线；C10/计价修复两小项以 A1/A2 身份并入本计划 |

**开工指令**：Nironta 确认后按 Day 1 执行；Day 1-2 结束回报 screening 结果，Day 3 冻结前回头拍板 §2.2 待定项（P1 bench.db 落点、P2 价格核录时点）。
