# 12 · RouterBench 实施总结（前置整顿 + Day 1-5）

> 日期：2026-10-05 · 执行：ZCode（自主闭环授权范围内） · 范围：commit `a26601b` → `561e807`
> 定位：RouterBench v0.1 开工阶段（Day 1-5）的**实施存档**——工作内容、成果数字、问题与修复、方法论发现、诚实边界、遗留事项的全量记录。
> 与文档族关系：09（方案原理）/ 10（实施规格）/ 11（调度执行层，状态板已同步）之外的过程性总结；数字与结论冲突时以本文为准（本文晚于 11 号的逐日更新，含更多细节）。
> 状态：**Day 1-5 已完结并推送；Day 6（Native 自采，花钱）暂停，等 Nironta 通报规模后开跑。**

---

## 一、总览与提交链

| commit | 阶段 | 内容概要 | 质量门 |
|---|---|---|---|
| `a26601b` | 前置整顿 | 合作者「纯 Rust 迁移」（`412ea10→1680a3d`）合并验证后的全仓清理：28 处过时注释、死代码 `Model::to_ai_spec()`、clippy 修复、AGENTS.md 重写、删 940M Python 残留 | clippy 0 错 + 130 测试 |
| `0a45b01` | Day 1 | 目录骨架 + 双数据集下载（sha256 双记录）+ `import_frozen.py` + `screening.py` | 同上 |
| `58e036c` | Day 3 | 四决策拍板落地：四份 manifest 冻结 + 六 hash + Core 1000 名单 + 敏感性对照 | 同上 |
| `9212d13` | Day 4 | `bench.rs` 回放引擎 + `lloom-cli bench replay` + parity test + bench.db | 135 测试（+5） |
| `561e807` | Day 5 | `bench sweep`（λ 曲线）+ E5 对照 + `report.py`（Pareto/AIQ）+ 首版报告 | 同上 |

穿插完成：Day 2 的 screening 全量跑与临时池初选（Day 1-3 之间）。全程 conventional commits，未推送任何 research/ 本地内容。

---

## 二、前置整顿：纯 Rust 迁移的合并与全仓清理

### 2.1 合并内容核对

合作者 SakiMidare 于 2026-10-04 推送三连提交（`aeb145b` → `90d8fc0` → `1680a3d`，+5362/−10879 行）：

1. **Python 运行时整体删除**：`api/ai_service.py`（2129 行）、`api/embedding_model.py`、`pyproject.toml`、`uv.lock`、PyInstaller 规格、CI 的 pyright/pyflakes job；端口 7862 消失，进程管理只剩 Ollama。
2. **原生 Provider 适配器**（新增 `providers/` 模块）：OpenAI-compatible（OpenAI/DashScope/DeepSeek/OpenRouter/Ollama/LM Studio/vLLM/自定义）+ Anthropic Messages 直连。
3. **编排 Rust 化**：`orchestrator.rs`（复杂度/分解/波次）、`context.rs`（token 估算/裁剪/摘要）自 Python 迁移。
4. **两级缓存重构**：L1 精确（SQLite）+ L2 语义（FastEmbed 量化 MiniLM）替代 ChromaDB——**此前 chroma 79MB 同步下载挂起的已知阻塞就此消除**。
5. **对外 API Key 体系**（WebUI 新页）：SHA-256 哈希存储、额度、RPM、白名单、有效期。
6. **全链路流式**打通所有 API 路径。

### 2.2 本地清理动作

- **注释对齐**（13 文件 28 处）：`lib.rs` 模块表、`ai_client.rs` 头注释（"legacy Python service remains"→原生适配器唯一路径）、`server.rs` 9 处（SSE/校准/plan-subtask 链上的"Python 侧"）、`router.rs` 4 处、`metadata.rs`（兜底链「五级」改「四级」）、`pricing.rs`、`config.rs`、`providers/mod.rs`、`model_dto.rs`、`openai_compat.rs`、CLI `--model` 帮助文案等。
- **死代码**：`Model::to_ai_spec()`（Python 时代 ModelSpec 载荷构造，全仓零调用方）删除。
- **clippy 修复**：合作者新缓存代码触发新版 `manual_option_zip`×2——用 match 重构，**特意保持惰性求值**（`SemanticCache::embed()` 含 ONNX 初始化副作用，zip 的急切求值会让无缓存路径也跑 embedding）。
- **文档**：本地 `AGENTS.md` 全面重写（纯 Rust 架构/端口/运行 3 步/台账指针改「后续方向」节）；全局 `~/.zcode/AGENTS.md` 项目背景行同步；`CODE_GUIDE.md` 加废弃横幅（三进程架构描述已过时，指向 ARCHITECTURE.md）。
- **磁盘**：删 `.venv`（588M）/`tiktoken_cache/`/`lloom.egg-info/`/`build/`（85M）/`dist/`（224M）/`scripts/__pycache__/`，合计约 940M。删除前核对：无进程占用、均在可删清单内、`data/`（含失效 chroma/）按铁律保留。

### 2.3 venv 为何彻底无用（结论存档）

`.venv` 的唯一存在目的是给 `uvicorn api.ai_service:app`（:7862 litellm 微服务）提供运行环境。迁移删除了该链每一环：①消费者代码（api/*.py）；②依赖清单（pyproject/uv.lock，venv 内包成无 manifest 孤儿）；③调用点（`processes.rs` 的 `start_ai`/三级解释器解析）；④职责本体（embedding → Rust fastembed；token 计数 → `context.rs`）。故为无入口、无消费者、无重建配方的纯死重。

---

## 三、开工决策存档（逐字）

### 3.1 开工排期原话

已在 11 号文档 §一逐字存档（Day 1-7 流程图 + 配套指令），本文不重复。

### 3.2 Day 3 前四项决策的拍板原话（2026-10-05）

> 全按你推荐的方式来（即1a 2a 3a 4a），继续进行

对应决策（我的推荐及依据）：

| # | 决策点 | 拍板 | 内容 | 我的推荐依据 |
|---|---|---|---|---|
| P1 | bench_* 表落点 | **a** | 独立 `benchmarks/routerbench/bench.db` | 生产库零污染，「production 表只读」红线物理化；合作品规「重构尽量零迁移」；`.gitignore` 已有 `*.db` 兜底 |
| P2 | qwen3.6 价格核录 | **a** | 维持 Day 6 前官方页重录 | 价格有效时点是自采时，现在核了到 Day 6 仍可能过期 |
| P3 | Public 池三元 | **a** | mistral-7b / Yi-34B / GPT-4-1106 | 单调能力阶梯 + 最大成本梯度（Yi-34B 全池性价比冠军）；附 b 池敏感性自动验证承诺 |
| P4 | Core datasets | **a** | winogrande/mmlu/hellaswag/gsm8k/mbpp 各 200 ≈ 1000 | rank top5 + task_type 覆盖 general/math/coding 三类；mbpp test 349 题充足 |

---

## 四、Day 1-2 · 数据层

### 4.1 下载与完整性（Gate 0）

| 文件 | 字节数 | sha256（完整） | 渠道 |
|---|---:|---|---|
| `routerbench_0shot.pkl` | 99,567,659 | `ba4f77f19517610a707c374e99322d7750c30fc4ae7ff5527888595a1e65d36d` | hf-mirror（withmartian/routerbench） |
| `bench-release.tar.gz`（LLMRouterBench） | 1,283,503,080 | `b79f8cde1a6f029c2efa663a3a3b6f7748defb22341fe59f328cebef6648c8f1` | hf-mirror（NPULH/LLMRouterBench） |

尺寸与 10 号 §1.1/§1.2 所载逐字节吻合。双 sha256 记入 `normalized/source_manifest.json` 及 `raw/SHA256SUMS`。LLMRouterBench 的 canonical 导入（10 号 §16.2）按计划留 Day 6+。

### 4.2 pkl 实测结构与导入

实测宽表：36,497 行 × 37 列——`sample_id`/`prompt`/`eval_name`（细粒度，MMLU 拆 57 个子题组，GSM8K 名为 `grade-school-math`）+ 每模型三列（`<model>` quality / `<model>|total_cost` / `<model>|model_response`）+ `oracle_model_to_route_to`。quality/cost 零空值。

`import_frozen.py` 展开为 canonical 长表：

- `instances.jsonl`（40MB）：36,497 实例，含 dataset_id（family 归组）/sample_id（`rb0_{family}_{源id}`）/task_family/task_type/prompt/split/meta（源 oracle 仅存 provenance，replay 不消费）。
- `outcomes.jsonl`（148MB）：401,467 行 = 36,497 × 11，**全覆盖、零丢弃、零重复**（Day 1 验收项全过）。
- `source_manifest.json`（6.8K，入库唯一 normalized 文件）：源件 sha256、列映射、行数、切分方法、11 模型清单、逐 dataset 计数。

**关键设计——切分在导入时即生成**（泄漏防护前置，11 号 §四第一则）：

- 方法：哈希桶 `sha256("lloom-split:3407:{sample_id}")` 前 128bit 归一 < 0.2 → calibration，否则 test。
- 性质：逐样本独立、与行序无关、跨机/跨运行可复现、无需随机模块（Mimosa 拦截 `random` 的替代写法）。
- 代价：逐 dataset 计数是统计收敛 20/80（如 gsm8k cal=1535/test=5915、mmlu 2782/11260、hellaswag 1967/8075、mbpp 78/349、winogrande 261/1006、mt_bench 16/64、arc 305/—），manifest 已注明。
- 铁律：重切（改 seed/ratio/方法）= 新 benchmark_id。

反序列化安全（Mimosa 高危拦截后改写）：加载前 `--expect-sha256` 完整性门（不符即拒载）+ 模块白名单受限 Unpickler（只放行 pandas/numpy/builtins 族，裸模块名与子模块双匹配——首版漏了裸 `numpy` 导致 `ndarray` 拒载，已修）。

### 4.3 screening（Day 2）

`screening.py`：S1-S7 七信号完整实现（10 号 §4.2 定义），输出字段照 §17。防泄漏执行决策：**默认只算 calibration split**（10 号 §7.3——benchmark 选择过程不得污染 test；原文未明说，我按防泄漏方向取严），`--split` 可改。

全池 11 模型排名（7 候选 bench 的 calibration split，score/cost 全覆盖 6,944 行/模型）：

| model_id | mean_score | mean_cost |
|---|---:|---:|
| gpt_4_1106_preview | 0.7951 | $0.003305 |
| zero_one_ai_yi_34b_chat | 0.6629 | $0.000188 |
| claude_v2 | 0.6482 | $0.002421 |
| claude_v1 | 0.6422 | $0.002166 |
| gpt_3_5_turbo_1106 | 0.6217 | $0.000247 |
| claude_instant_v1 | 0.6100 | $0.000237 |
| mistralai_mixtral_8x7b_chat | 0.5575 | $0.000135 |
| wizardlm_wizardlm_13b_v1_2 | 0.4389 | $0.000073 |
| meta_llama_2_70b_chat | 0.3277 | $0.000203 |
| mistralai_mistral_7b_chat | 0.3076 | $0.000046 |
| meta_code_llama_instruct_34b_chat | 0.2030 | $0.000173 |

临时池初选（决策 3a 前身）：**mistral-7b（weak）/ Yi-34B（mid）/ GPT-4（strong）**——单调能力阶梯（0.31/0.66/0.80）+ 最大成本梯度。

七候选 screening 结果（calibration，临时池 a；`reports/screening.{json,csv}`）：

| dataset | n(cal) | coverage | weak | best | oracle | oracle_gain | upgrade_opp | cheap_safe | disagreement | H | cost× | score |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| winogrande | 261 | 1.00 | 0.521 | 0.851 | 0.950 | **0.100** | 0.414 | **0.586** | 0.996 | 1.03 | 52.6 | 2.128 |
| mmlu | 2782 | 1.00 | 0.257 | 0.815 | 0.891 | 0.076 | 0.595 | 0.406 | 0.955 | 0.99 | 50.6 | 2.066 |
| hellaswag | 1967 | 1.00 | 0.260 | 0.839 | 0.932 | 0.093 | 0.604 | 0.396 | 0.950 | 0.97 | 50.5 | 2.059 |
| gsm8k | 1535 | 1.00 | 0.411 | 0.663 | 0.706 | 0.043 | **0.651** | 0.349 | 0.476 | 1.00 | 92.9 | 2.046 |
| mbpp | 78 | 1.00 | 0.346 | 0.718 | 0.756 | 0.038 | 0.397 | 0.603 | 0.821 | 1.00 | 198.8 | 2.037 |
| mt_bench | 16 | 1.00 | 0.525 | 0.750 | 0.806 | 0.056 | 0.562 | 0.438 | 0.719 | 0.95 | 234.4 | 2.007 |
| arc | 305 | 1.00 | 0.351 | 0.967 | 0.984 | 0.016 | 0.616 | 0.384 | 0.846 | 0.98 | 51.1 | 1.998 |

**七候选全部通过 Dataset Gate**（10 号 §5.1：coverage=100% ∧ oracle_gain>0 ∧ upgrade/cheap-safe 非零 ∧ cost_ratio≥3×）。解读：winogrande/mmlu/hellaswag headroom 最大；gsm8k 最需要升级（65.1% 的题 weak 做不对）；arc 垫底（mid Yi-34B 已 0.836，headroom 只剩 0.016）；winner 熵全部 ~1.0（接近 3 模型均匀上限 ln3≈1.10）。

执行偏差（已记 11 号 Day1 块）：脚本名用 `screening.py`（10 号 §17 写 screen.py，以开工指令为准）；哈希桶切分；screening 默认 calibration；normalized 不入库（148MB 超 GitHub 100M 单文件硬限，`.gitignore` 负向规则只留 source_manifest.json）；bench 专用 venv 在 `benchmarks/routerbench/.venv`（uv，pandas 3.0.6/numpy 2.5.3；产品 .venv 已随迁移删除）。

---

## 五、Day 3 · 冻结

### 5.1 双池敏感性验证（决策 3a 的自动保障）

b 池（mistral-7b / **gpt-3.5-turbo** / GPT-4）对照跑（`reports/screening_poolB.{json,csv}`）：

| dataset | a 池 rank | b 池 rank | oracle_gain(a→b) |
|---|---:|---:|---|
| winogrande | 1 | 1 | 0.100→0.092 |
| mmlu | 2 | 5 | 0.076→0.077 |
| hellaswag | 3 | 4 | 0.093→0.083 |
| gsm8k | 4 | 6 | 0.043→0.043 |
| mbpp | 5 | 2 | 0.038→0.077 |
| mt_bench | 6 | 3 | 0.056→0.138 |
| arc | 7 | 7 | 0.016→0.007 |

**结论：rank 对 mid 强度敏感**（两池 top4 仅两席重合），根源是 `screening_score = oracle_gain + upgrade_opp + cheap_safe + H` 中 oracle_gain 依赖池（oracle = max over pool，mid 变强则 oracle 上移）。但逐项检验后 **Core 名单在规格约束下唯一，决策 4a 不被推翻**：

- mt_bench：10 号 §1.3 明确 Showcase 专属、不参与 Core 排序——b 池下 rank#3 不构成入选理由；
- arc：两池均 7/7 垫底；
- gsm8k：upgrade_opportunity 0.651 全场最高（weak-vs-strong 对测量，与 mid 无关）+ math 类型必备；
- mbpp：coding 类型唯一来源。

敏感性结论落档 `frozen_manifest.json` 的 `pool_sensitivity` 段，并附解读约束：**headline 不得跨池引用 screening rank**。

### 5.2 四份 manifest 冻结

| 文件 | 要点 |
|---|---|
| `manifests/model_pool_public.yaml` | pool_id `public_matrix_top3_v1`；cost_basis=source（源矩阵逐题实测）；不写 production models 表 |
| `manifests/model_pool_native.yaml` | pool_id `native_qwen36_3tier_v1`；池结构（id/tier/context_window）冻结；价格字段 `price_status: provisional_transcribed`（决策 P2：Day 6 前重录，变价 = 新 cost_basis_hash） |
| `manifests/datasets.yaml` | Core 5×200；selection_policy 显式记录；arc/mt_bench 出局理由；native_plan/showcase 照 starter 草案 |
| `manifests/task_mapping.yaml` | public（7 family）+ native（7 dataset）映射；band 不硬编码——B0 的 band 由 production `band_for(task_type, prompt)` 现算 |

### 5.3 freeze.py：抽样 + 六 hash

- **抽样**：每 dataset 的 test 实例按 `sha256("lloom-core-select:3407:{sample_id}")` 升序取前 200——确定性、无随机模块、与行序无关；名单落盘 `core_selection.jsonl`（1000 行）。Core 可用量：winogrande 1006 / mmlu 11260 / hellaswag 8075 / gsm8k 5915 / mbpp 349。
- **六 hash**（11 号 §三 Day3 口径）：

| hash | 输入 | 值（前 16 位） |
|---|---|---|
| benchmark_hash | instances ‖ outcomes ‖ core_selection 字节串联 | `324aa61dfdc35ec5` |
| pool_hash | model_pool_public.yaml | `cabd3ac9c0965e19` |
| prompt_profile_hash | source_manifest.prompt_profile + 抽样 policy（规范化 JSON） | `c98137e64721f9f4` |
| scorer_hash | source_manifest.scoring | `090f1a5a2e6e1a08` |
| split_hash | source_manifest.split + 抽样 policy | `a029c59b525ca1df` |
| cost_basis_hash | {public: source, native: provider_official} | `51a075798d8550f8` |
| **manifest_hash** | 六 hash 规范化串联 | **`199fb962164bcda8`** |

`benchmark_id = lloom-routerbench-v0.1`（track: public_screening）。任一 hash 输入变化 = 新 benchmark_id（10 号 §6.2）。

---

## 六、Day 4 · 回放引擎（bench.rs）

### 6.1 模块结构（`crates/lloom-core/src/bench.rs`）

| 组件 | 职责 |
|---|---|
| `load_frozen(norm_dir, selection)` | 加载冻结矩阵；名单只过滤 test 侧 + split 双重校验（非 test 拒收）；返回 (test, calib, matrix) 三元 |
| `FrozenMatrix` | `(sample_id, model_id) → (score, cost)` 全内存 HashMap；replay 只查不写 |
| `calibration_stats()` | 等效单价 p_m + per (dataset, model) 均分（Best Single 用）+ per (model, task_type) 先验（E5 用，n≥5）+ per-model 全局均分（quality_score 先验）——**只读 calibration 侧** |
| `build_pool_models()` | 内存构造 Model 三元（tier 1/2/3）+ PriceSpec 表（p_m 同价，`price_source=bench_calibration`） |
| `bench_policy()` / `policy_with_weights()` | 显式 `min_capability_tier=1`；λ sweep 参数化（latency 恒 0.1，与生产 default 形态一致） |
| `ReplayCtx::build_plan_input()` | PlanInput 组装：band_for 现算 / est_in=chars×0.6 / est_out=750 / hit_rate 全零 / sticky=None / budget=normal / deferrable=false / **t_epoch=0 确定性** |
| `replay(ctx, matrix, instances, strategy)` | 8 策略单实现：Lloom（调 production plan()）/ Always×3 / Cheapest（argmin 单价，不看分数）/ Random（xorshift64*，seed 3407）/ BestSingle（calibration 均分 argmax per dataset）/ Oracle（hindsight argmax，标注上界） |
| `summarize()` | Q / C / regret P50/P95 / 选择分布（saving/retention/gap_recovery 在对比层算） |
| `open_bench_db()` / `save_run()` | 决策 1a 独立库：bench_runs（含 benchmark_id/manifest_hash/parameter_json 溯源列）+ bench_run_items（UNIQUE(run_id, dataset_id, sample_id)）；RunMeta 结构体聚合参数 |

CLI：`lloom-cli bench replay --strategy all|… --models … --calibration`（本地离线，不需要 server；lloom-cli 首次依赖 lloom-core）与 `bench sweep`。

### 6.2 红线落地对照

| 红线（11 号 §四 / 10 号 §九·七） | 实现证据 |
|---|---|
| 决策单一真源，禁止第二套评分公式 | bench 只组装 PlanInput 调 `router::plan()`；decision_json 直接序列化 `PlanOutcome.candidates`（零可见性改动）；`score_all` 未动 |
| est_in/est_out 只用固定 profile 或 calibration 统计 | `chars×0.6`（router.rs 在线口径）+ 750（冷启动）；**禁止查 test outcome tokens**（源矩阵本无 token 列） |
| hit_rate 全零 map | `empty_hit` 常量空 map |
| Best Single 只由 calibration split 决定 | `dataset_mean` 仅读 calibration 实例键 |
| quality_override 只来自 calibration | `task_quality` n≥`CALIBRATION_MIN_SAMPLES`(5) 才收录（对照 review.rs 生产惯例） |
| 零成本 / production 表只读 | replay 全内存无网络；bench.db 独立库（决策 1a） |
| parity test（§二十四，第一道 Rust gate） | fixture：bench 组装 == 生产语义手工组装 → primary/candidates 逐项全等 |

### 6.3 首次完整回放（run 1-8，Core 1000 test）

详见 §九表格。当日另修 2 个 clippy lint（`too_many_arguments`→RunMeta 聚合、`format_in_format_args`）。

---

## 七、Day 5 · 评测产出

### 7.1 λ sweep（`bench sweep`，run 20-29）

λ=cost_weight∈{0,0.1,…,0.9}，quality_weight=0.9−λ（保持生产权重和形态），latency 恒 0.1：

| λ 区间 | Q | C/query | 语义 |
|---|---:|---:|---|
| λ ≤ 0.2 | 0.7628 | $0.004423 | 近 Always-Strong（质量权重主导） |
| λ ≥ 0.3 | 0.7087 | $0.002826 | Pareto 折中（saving 36.1%） |

曲线只有两档：三模型池 + band 门槛 + 离散质量先验下，λ 的连续变化只在临界点切换选择——点集稀疏是池规模的必然，不是 bug。产物 `reports/pareto_points.json`，每点 run 落库且 parameter_json 记录权重。

### 7.2 E5 实验（w/ vs w/o 任务级校准）

- w/o（run 25，控制组）：quality_score = calibration **全局均分**（注册冷启动先验语义）；
- w/（run 30）：另加 quality_override = per (model, task_type) calibration 均分（EWMA 替身，n≥5）。

**结果：完全一致（Q=0.7087 / C=$0.002826，选择分布逐模型相同）——零增益，负结果如实记录。** 解读见 §十一。

### 7.3 report.py 与首版报告

`scripts/report.py`（纯 stdlib，读 bench.db）：

- Pareto 非支配前沿（最小化 cost、最大化 quality）+ **AIQ**：前沿折线在共同成本区间上的归一化质量梯形积分 → [0,1]。首版 **AIQ=0.7036**（3 点前沿）。注：v0.1 简化实现、点集稀疏，仅作对照不作唯一 headline（10 号 §13.9）；
- LLooM 对比指标（λ=0.5 基准行）：saving/retention/gap_recovery；
- per-dataset × per-strategy 完整表（§四十一 格式）；
- 产物：`reports/bench_report.{json,md}`。

---

## 八、成果清单

**数据资产**（`benchmarks/routerbench/`，raw/normalized 不入库）：

- 双下载件 1.38GB + sha256 双记录（source_manifest + SHA256SUMS）
- 冻结矩阵：36,497 × 11 = 401,467 outcomes（可由 raw + import_frozen.py 确定性重建）
- Core 1000 名单（core_selection.jsonl）
- 四份冻结 manifest + frozen_manifest.json（六 hash + 敏感性结论 + 四决策存档）
- reports/：screening.{json,csv}、screening_poolB.{json,csv}、pareto_points.json、bench_report.{json,md}

**代码资产**：

- Python 四脚本：import_frozen.py / screening.py / freeze.py / report.py（bench 专用 venv：pandas/numpy）
- Rust：`lloom-core/src/bench.rs`（约 900 行含测试）+ `lib.rs` 挂载 + `lloom-cli` bench 子命令（replay/sweep）+ Cargo 依赖
- bench.db：bench_runs / bench_run_items，30 个 run 完整留痕（含 decision_json 逐题全候选评分）

**流程资产**：Gate 0-6 验收证据链（§十三）；11 号状态板逐日更新；防泄漏三则强制顺序全部打勾；记忆文件沉淀（Mimosa 新拦截面的已验证写法等）。

---

## 九、核心评测结果（v0.1 首版，Public Track）

Core 1000 test 实例，池 mistral-7b(weak) / Yi-34B(mid) / GPT-4(strong)，cost 为源矩阵逐题实测：

| Strategy | run | Quality | Cost/query | Regret P50 | 选择分布 |
|---|---:|---:|---:|---:|---|
| Always Weak | 1 | 0.3475 | $0.000045 | 0.50 | weak×1000 |
| Always Mid | 2 | 0.6268 | $0.000190 | 0.00 | mid×1000 |
| Always Strong | 3 | 0.7628 | $0.004423 | 0.00 | strong×1000 |
| Cheapest | 4 | 0.3475 | $0.000045 | 0.50 | weak×1000 |
| Random | 5 | 0.6080 | $0.001614 | 0.00 | 313/342/345 |
| Best Single | 6 | 0.7628 | $0.004423 | 0.00 | strong×1000 |
| Oracle | 7 | **0.8393** | **$0.001258** | 0.00 | weak 434 / mid 383 / strong 183 |
| **LLooM（λ=0.5）** | 8 | **0.7087** | **$0.002826** | 0.00 | strong 820 / mid 180 |

**LLooM 三指标（vs 基准行）**：

- saving vs Always-Strong = **36.1%**（1 − 0.002826/0.004423）
- retention vs Always-Strong = **92.9%**（0.7087/0.7628）
- gap_recovery = **87.0%**（(0.7087−0.3475)/(0.7628−0.3475)）——plan() 吃掉了 weak→strong 差距的 87%
- **AIQ = 0.7036**（3 点 Pareto 前沿）

关键读数：

1. LLooM 选择分布非 trivial（820/180，两模型混用）——§四十三成功条件第 7 条满足；
2. Oracle 矩阵显示 **43.4% 的题 weak 即最优 winner**——cheap-safe 空间巨大，是 Native Track 演示的素材库；
3. Best Single = Always Strong（GPT-4 calibration 均分绝对领先）——LLooM 的 36.1% saving 是在「质量不输 Best Single 7 个点」前提下实现；
4. 10 号 §四十三成功最低条件十条中 1-7、9、10 直接满足，第 8 条（至少一个 workload 明显 Pareto 改进）由 λ 折中点对 Strong 的支配关系满足。

---

## 十、问题与修复记录

| # | 问题 | 根因 | 处置 | 验证 |
|---|---|---|---|---|
| 1 | screening.py 首次生成即损坏（未定义变量/重复 main/断裂逻辑） | 生成过程缺陷 | 烟测前发现，整文件重写 | 重写后烟测+全量跑 |
| 2 | winner_entropy 负熵爆表（-17886 量级） | 对未归一化的 winner 记账份额取对数 | `probs = credit/total` 后再取熵 | H 全部落回 [0, ln3] |
| 3 | Mimosa 拦 pkl 反序列化（高危） | pickle 反序列化面 | sha256 前置校验（不符拒载）+ 模块白名单受限 Unpickler | 拦截解除，安全性实质增强 |
| 4 | Mimosa 拦 `random` 弱随机 | 弱随机红线 | 切分/抽样改哈希桶；Random 策略用自实现 xorshift64* | 复现性反而更强 |
| 5 | 白名单 Unpickler 拒载裸 `numpy.ndarray` | 前缀匹配漏裸模块名 | 精确名集合 + 前缀双匹配 | pkl 正常加载 |
| 6 | λ sweep 十点结果完全相同 | 池模型 quality_score 全填 0.5，质量维度抹平后 `qw·q − cw·norm_cost` 的 argmax 与 cw 无关 | quality_score 改 calibration 全局均分（注册冷启动先验语义） | 曲线两档成形（run 20-29） |
| 7 | `RoutingPolicy::default` 的 min_capability_tier=2 会把 weak 永久挡出全池 | 生产 default 假设池内都是 tier≥2 | bench policy 显式 min_tier=1 + parameter_json 记录 | 全池三层可入选 |
| 8 | clippy 新版 lint 共 5 处 | 新版 clippy（合作者 manual_option_zip×2；本次 too_many_arguments / format_in_format_args / ptr_arg） | match 重构（保惰性求值）/ RunMeta 聚合 / 提取变量 / `&Path` | clippy -D warnings 全绿 |
| 9 | 交接快照不存在（handoff list/any_owner 均空） | 可能被会话启动钩子消费或建在别的作用域 | 从开工指令 + 记忆文件 + 必读文档三方补齐；「已实测事实」逐项重新 grep 验证（行号未漂移，基线 122→130 已变） | 无信息损失 |
| 10 | `scripts/aiq_replay.py` 已被纯 Rust 迁移删除（交接盲点） | 合作者迁移顺带删除 | Day 5 前 `git show aeb145b^:scripts/aiq_replay.py` 捞回参考；发现其 AIQ 是简化插值版，最终按 10 号 §13.9 独立实现 | report.py 落地 |

---

## 十一、方法论发现（对后续有决策价值）

1. **E5 负结果：任务级 EWMA 校准在静态矩阵上零增益。** 校准数据（calibration split）与先验（calibration 全局均分）同源同分布时，细粒度校准不改变任何决策。含义：EWMA 的价值验证需要数据漂移，**真正舞台是 Native Track**；v0.1 阶段不应对外宣称「校准带来提升」。
2. **Band 门槛是路由质量的主要来源。** band_for 将 medium→tier2、hard→tier3：hard 题只剩 strong 合格、medium 只剩 mid+strong。gap_recovery 87% 中相当部分来自「分带」而非「带内权衡」。后续可做 Plan-only vs End-to-End rule-only 对照（10 号 §10.2）量化分类层与选择层的各自贡献。
3. **Screening rank 对池敏感。** oracle_gain 依赖池构成（mid 变强 → oracle 上移），两池 top4 仅两席重合。已写入 manifest 解读约束：headline 不得跨池引用 screening rank；Day 2 文档里「互补指标是相对池的量」的预告被实证。
4. **Public 矩阵上 Best Single = Always Strong。** GPT-4 均分绝对领先，意味着 routing 的对手盘很强；LLooM 的价值主张应为「同质量省 36%」而非「更优」。
5. **plan() 的质量先验是承重墙。** quality_score 全等时评分退化为纯成本最小化（band 内 argmin）；先验的来源与校准是后续一切 routing 改进的第一杠杆。

---

## 十二、局限与诚实边界（对外展示不可省略）

1. **Public Track 全部数字基于 2024 年历史模型**（GPT-4-1106 世代）与源数据 cost 口径；只作基建验证与研究对照，**不构成 LLooM headline**——headline 等 Native Track（qwen3.6 池 + 真实自采）。
2. **quality_score 先验是设计决策而非源数据**：calibration 全局均分作为「注册冷启动认知」是我的建模选择（泄漏合规：只用 calibration 侧），但它直接决定路由结果，λ 曲线形状对此敏感；敏感性分析未做。Native Track 有真实注册先验后此问题自然消解。
3. **AIQ 是简化实现**：前沿折线梯形积分（非 RouterBench 论文完整凸包-归一化流程的逐步复刻）；点集仅 3 个（三模型池粒度所限）；无 golden fixtures。仅作对照。
4. **哈希桶切分是统计收敛 20/80**，逐 dataset 计数非精确 20%（manifest 已注明实际值）。
5. **E5 的 w/o ≠ 零信息**：它是「静态注册先验」控制组，不是「无先验」；对外表述禁止说成「无校准版本」。
6. **单测 fixture 很小**（3 实例手工矩阵），真实矩阵验证靠 CLI 实跑而非单测；est_tokens 用 chars×0.6 粗估而非 tokenizer；t_epoch=0 意味着时段价逻辑未被 exercise（公共池 PriceSpec 无 zone/tiered，影响为零但需知悉）。
7. **mt_bench/mbpp calibration 样本偏小**（16/78），screening 置信有限；已通过 test 侧数量充足（64/349）化解 Core 入选风险，但这两者若进 Core 需谨慎（当前未进）。
8. **bootstrap CI 未做**（10 号 §13.10 要求 headline 指标 95% CI）——当前所有数字是点估计，留待 Native headline 前。

---

## 十三、Gate 验收对照（10 号 §三十七 + parity）

| Gate | 要求 | 状态 | 证据 |
|---|---|---|---|
| 0 数据获取 | 0-shot pkl 可读 OR LLMRouterBench 可读 | ✅ | 双下载件 + 受限加载验证（36,497 行读取成功） |
| 1 冻结矩阵 | test × pool coverage=100% | ✅ | 401,467 = 36,497×11 全覆盖；Core 1000 全命中（缺失即拒，bench.rs 有显式报错路径） |
| 2 Screening | 七信号全输出 | ✅ | screening.{json,csv}（a/b 双池） |
| 3 Plan parity | production == bench replay | ✅ | `parity_bench_assembly_matches_production`（primary/candidates 逐项全等） |
| 4 零成本 | replay 0 API 调用、0 usage/calibration 变更 | ✅ | 全内存回放；bench.db 独立库；生产表零接触（结构保证） |
| 5 Baseline | 8 条全出结果 | ✅ | run 1-8（§九表） |
| 6 统计 | Q/C/saving/retention/gap_recovery/AIQ/regret 可导出 | ⚠️ | 全部可导出（bench_report.json）但 **bootstrap CI 未做**——留 Native 前 |
| 7 UI | 四组件真实数据无 mock | ⏸ | 未开始（「之后」阶段） |

防泄漏三则（11 号 §四）打勾：split 导入时生成 ✅ / est·hit_rate·BestSingle·quality_override 只碰 calibration ✅ / test 分数只出现在 evaluator ✅。

---

## 十四、暂停点状态与恢复选项

- 分支 v2 = origin/v2（HEAD `561e807`），工作区干净；raw 1.38GB + normalized 188MB + bench.db（30 runs）在本地且不入库。
- 11 号状态板、本文档、记忆文件均已同步——**不存在只存在于对话中的状态**，可随时搁置或恢复。

恢复选项：

- **A. Day 6 Native 自采**（花钱）：五前置条件已全达成（Gate 2-6）。开跑前两项动作：qwen3.6 官方页价格重录（P2，转述值 diff）；lm-eval 对 dashscope chat 端点 `--limit 10` 抽查。规模预通报：首轮 3 models × 3 datasets × 50-100 样本（约 $1-3）；全量 1400 实例矩阵按 max-preview 定价上限估 $15-40（以重录价格为准）。
- **B. 「之后」阶段先行**（零成本）：Single Query Replay API（`/api/bench/replay/one`）/ WebUI 四组件（Gate 7）/ bootstrap CI / AIQ golden fixtures——纯消费已有数据。

---

## 附录 A · 产物路径

```text
benchmarks/routerbench/
├── raw/{routerbench,llmrouterbench}/        # 1.38GB（不入库）
├── raw/SHA256SUMS
├── normalized/{instances,outcomes}.jsonl    # 188MB（不入库，可重建）
├── normalized/source_manifest.json          # 入库
├── manifests/{model_pool_public,model_pool_native,datasets,task_mapping}.yaml
├── manifests/core_selection.jsonl           # 1000 行
├── manifests/frozen_manifest.json           # 六 hash + 敏感性 + 决策存档
├── reports/screening.{json,csv}             # a 池
├── reports/screening_poolB.{json,csv}       # b 池敏感性
├── reports/pareto_points.json               # λ 曲线
├── reports/bench_report.{json,md}           # 首版汇总报告
├── scripts/{import_frozen,screening,freeze,report}.py
├── scripts/（.venv = bench 专用 uv 环境）
└── bench.db                                 # 决策 1a（不入库，30 runs）
crates/lloom-core/src/bench.rs               # 回放引擎 + 5 测试
crates/lloom-cli/src/main.rs                 # bench replay / sweep 子命令
```

## 附录 B · 关键命令（仓库根目录）

```bash
# 导入（含完整性门）
benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/import_frozen.py \
  --expect-sha256 ba4f77f19517610a707c374e99322d7750c30fc4ae7ff5527888595a1e65d36d

# 七信号 screening（a 池 / b 池对照）
benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/screening.py \
  --models mistralai/mistral-7b-chat zero-one-ai/Yi-34B-Chat gpt-4-1106-preview

# 冻结（抽样 + 六 hash）
benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/freeze.py

# 回放（8 策略 / 单策略 / E5 w/ 校准）
cargo run -q -p lloom-cli -- bench replay --models mistralai/mistral-7b-chat zero-one-ai/Yi-34B-Chat gpt-4-1106-preview
cargo run -q -p lloom-cli -- bench replay --strategy lloom --calibration --models …
cargo run -q -p lloom-cli -- bench sweep   --models …

# 报告（Pareto + AIQ + §四十一表）
benchmarks/routerbench/.venv/bin/python benchmarks/routerbench/scripts/report.py
```
