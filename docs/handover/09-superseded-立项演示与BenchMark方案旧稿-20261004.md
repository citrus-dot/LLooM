# 09 · 立项演示与 Benchmark 改造方案（评审稿）

> 日期：2026-10-04 · 代码基线 `v2` @ `412ea10` · 性质：**评审稿，未实施**——经 Nironta 评审拍板后才开工；本文档承接 00-08 文档族（「暂不实施，评审后慢慢完善」原话存档见 §一）。
> 定位：把「先做能呈现效果的产品」的演示需求与「大小模型差异 bench 改造为 RouterBench」的构想落成可评审的完整方案：快速优化四项（A1-A4）+ Benchmark 改造五项（B1-B5）+ 演示动线 + 证据与开放点。

---

## 一、需求原文与已拍板决策

### 1.1 需求原话逐字存档

> 「我现在的需求是先做出能呈现效果的产品，以便立项展示。
> 有什么可以直接快速做完且有实际效果的优化？
> 做完优化后，重点在构造一个合适的benchmark，来测试现有的路由效果。
> bench感觉最好是挑大小模型表现差异很大的bench改一改变成routerbench，比如某个bench原本是测模型的，不同模型差异很大，现在我们就直接把它换成固定模型池+router，然后路由机制介入进行零成本测试。
> 建议广泛调研各种论文、项目、研究中用的主流benchmark，仔细考虑。」

状态备注（2026-10-04）：「先不实施，我评审后慢慢完善」——方案成文冻结于本文档，待评审。

### 1.2 已拍板三决策（AskUserQuestion 实录，2026-10-04）

| # | 决策点 | 拍板结果 | 含义 |
|---|---|---|---|
| D1 | Benchmark 数据源 | **公开矩阵 + 真实池自采（双路线）** | 第一步导入 RouterBench 公开矩阵（405k 样本，零成本立即出图）；第二步 lm-eval 对 LLooM /v1 跑真实池（qwen-flash/plus/deepseek 等，约 $1-5）补充真实闭环演示 |
| D2 | 第一批快速优化 | **内部优化三件套+基建 + WebUI 图表页** | C10 决策快照 + 计价不变量修复 + C12/E1 + 「路由评测」图表页；**不含** polish 写作模板与用户记忆 MVP（留待后续） |
| D3 | 演示受众 | **两者兼顾** | 学术严谨骨架（AIQ 指标定义、基线量化对比、置信区间）+ 现场演示动线（点开单题看省钱明细、实时回放动画），同一页面分层呈现 |

---

## 二、调研证据（2026-10-04 本轮新增，方案的依据）

### 2.1 RouterBench 公开矩阵（A 级，arXiv:2403.12031 HTML 全文提取）

**这就是「大小模型差异大 + 逐实例结果可得」的现成底座**，无需自己改造源 bench：

- **规模**：405,467 样本 = 11 模型 × 8 源 benchmark × 64 任务。
- **8 个源 benchmark**：常识推理（HellaSwag、Winogrande、ARC Challenge）、知识（MMLU）、对话（MT-Bench）、数学（GSM8K）、编程（MBPP）、RAG（Martian 客户 800 条真实查询）。
- **11 模型**（+3 个仅 RAG 的在线模型）：开源 6 = Llama-70B-chat / Mixtral-8x7B-chat / Yi-34B-chat / Code Llama-34B / Mistral-7B-chat / WizardLM-13B；闭源 5 = GPT-4 / GPT-3.5-turbo / Claude-instant-v1 / Claude-v1 / Claude-v2。**大小模型性能差 + 价差天然巨大**（GPT-4 vs Mistral-7B）。
- **JSONL 字段（一行样本）**：`sample_id`（如 "mmlu-astronomy.val.5"）、`model_name`、`eval_name`、`prompt`、`model_response`、`performance`（True/False 对错）、`cost`（闭源按 API 价、开源按 Together 参考价）、`true_label`——**「模型 × 样本 → (对错, 成本)」矩阵完备，正是预置结果矩阵回放所需的全部字段**。
- **AIQ 公式**：`AIQ(Rθ) = 1/(c_max−c_min) × ∫[c_min,c_max] R̃θ dc`，R̃θ = 采样参数生成点集的非递减凸包（NDCH）；两系统比较时插值外推到共同成本区间。与 `aiq_replay.py` 的凸包逻辑同构，可参照重写。
- **数据获取**：论文声明代码与数据公开（github.com/withmartian/routerbench）；**本机网络实测**：raw.githubusercontent 超时、hf-mirror 对 `withmartian/routerbench_dataset` 与 `ynulihao/LLMRouterBench` 均返回 401——拉取路径需按 §九.1 三级回退处理（评审重点之一）。

### 2.2 lm-eval-harness（A 级，EleutherAI README 提取）——自采路线的工具链

- **`--log_samples` 输出逐实例预测**（`<task_name>_eval_samples.json`），`--predict_only` 可只取生成不打分——逐实例「模型 × 样本 → 响应」数据可得。
- **OpenAI 兼容端点直连**：`--model local-chat-completions --model_args base_url=http://127.0.0.1:7861/v1,...`——**LLooM /v1 可直接作为被测后端**（/v1 非流式已支持：`openai_compat.rs:49 stream: Option<bool>`、`:369/390 req.stream.unwrap_or(false)` 分支实测存在）。
- 60+ 任务库（hellaswag/gsm8k/mmlu/arc/winogrande/ifeval/leaderboard 组…），`lm-eval ls tasks` 可列全量。
- **约束**：聊天 API 只能跑 `generate_until` 型任务（无 logprobs）；官方建议闭源 API 先 `--limit 10` 抽查答案抽取是否符合预期。
- 另支持 `litellm` 模型类型（100+ 提供商）——与 LLooM 技术栈同源的备选通道。

### 2.3 其他调研事实（B 级，部分待核）

| 事实 | 等级 | 对方案的影响 |
|---|---|---|
| HELM 提供 released runs，但官网 JS 渲染页抓不到 per-instance 文件结构细节 | B | 不作为主数据源；备选 |
| Qwen3-4B ≈ Qwen2.5-72B-Instruct（官方博客定性，无逐 bench 分数表） | B | 真实池自采的「小模型也能赢」叙事支撑，具体分数以模型卡为准 |
| LLMRouterBench（ACL 2026 Findings，33 模型 × 21 数据集）HF 仓 `ynulihao/LLMRouterBench` 多源确认存在 | B+ | RouterBench 拉取失败时的备选数据源（同样遇 401，待网络好时重试） |
| WebSearch 本轮不可用（模型变更）、GitHub/HF 直连超时、arxiv.org/html 可用 | A（环境实测） | 后续调研/拉数据的环境约束，见 §九.1 |

### 2.4 代码摸底：回放与演示基建现状（Explore 子代理 @ `412ea10`，锚点实测）

**已有的骨架**（与 RouterBench 思路同构，改造有底）：

| 机制 | 锚点 | 事实 |
|---|---|---|
| 影子双跑采样 | `server.rs:2053 run_shadow_pair` | 真实 `router::route()` 选模型 + 基线（settings `routing.shadow_baseline` 钦定或 tier 最高者 :2074-2087）→ **两次真实 LLM 双跑** :2101-2104 → `priced_usage` 计价 → 落 `routing_calibration` :2147 |
| 影子驱动方式 | `server.rs:2169 maybe_shadow_sample`（热路径概率采样，`routing.shadow_ratio` 默认 0.10）；`POST /api/routing/shadow` :2189（单条 query） | 手动端点可单条触发 |
| AIQ 离线重放 | `scripts/aiq_replay.py:81-186` | **只吃 SQLite**（routing_calibration + models + model_task_score，`--db` 唯一输入）；输出弱/当前/强三线 + AIQ 填充比 :164 + 节省比 :166 + 结论建议；弱线成本是**价格比估算**非真实双跑 :147-156 |
| 周期闭环 | `review.rs:32-60` | 每 6h 调 `aiq_replay.py --json` 写 `policy_review` + Rust 侧 7×7 权重网格无副作用重放出建议 |
| 路由组装链 | `router.rs:544-660 route()` | classify → band_for → policy → spec_map → quality_override → hit_rate → PlanInput（review.rs:156 有对齐口径的第二次组装先例——回放器是第三次组装，有模式可循） |

**五个缺口**（= B 部分要补的）：

| # | 缺口 | 锚点/证据 | 后果 |
|---|---|---|---|
| 1 | `routing_calibration` 不存 query 原文与响应文本（只存 FNV 哈希） | `db.rs:242-250`（12 列全清单）；`shadow_hash` @ server.rs:2032 | 无法重放打分或换策略重算 |
| 2 | 无外部数据集入口（不能吃 CSV/JSONL） | `aiq_replay.py:66`（--db 唯一输入） | 评测被绑死在自采样本上 |
| 3 | 影子双跑必打真实 LLM（两侧都无 mock/预置响应通道） | `ai_service.py` 全文无 mock/fake/replay（chat/classify 直连 litellm :816/:886） | 离线/零成本演示不可能 |
| 4 | WebUI 无图表库、无 AIQ 曲线/影子对比页 | `package.json`（仅 antd 6 + react-markdown 等，无 echarts/recharts）；AIQ 三线数字在 OverviewPage 折叠面板纯文本（OverviewPage.tsx:360-400） | 演示只能靠文本面板 |
| 5 | 逐实例的「模型×样本」结果矩阵不存在 | — | 这是回放评测的数据核心，需新建 |

---

## 三、方案 A：快速优化四项（A1-A4，约 4-6 天）

> 定位：全部是 benchmark 展示的前置（数据源、数字可信度、可见功能点、视觉主场）；规格引用 00-08 既有文档，不重复展开。

### A1 · C10 决策快照（1-2 天）——benchmark 页数据源 + 可解释性演示

- 规格：05 §五（DDL 五列 + `CandidateSnapshot` 结构体 + `selection_probability` 的 exploit/randomized_explore/fallback 语义约定）。
- 落点：`server.rs:624-628`（signals_json 组装处）与 `openai_compat.rs:280-284` 两处插入，一次事务同写。
- **B 部分联动**：回放引擎产生的每次路由决策也落快照——演示时逐条点开「候选集/评分/选择概率」，这是「路由为什么这么选」的核心弹药，也是论文创新点 4（倾向日志）的展示体。

### A2 · 计价不变量修复（1-2 天）——成本数字可信的地基

- 规格：07 §一.3（Python 侧归一化三分量 `base/cached/creation`；Rust 废除 `prompt_tokens − cached` 推导；运行时断言 + 中文报错；补 litellm Anthropic 聚合形状单测 `prompt=base+read+creation`）。
- 演示意义：benchmark 页上每个成本数字经得起「评审抠账本」；`routing_shadow` 自采路线的 act_cost 同样受益（真实池计价走的就是这条链）。

### A3 · C12 分类模型配置（0.5 天）+ E1 延迟接线（1 天）

- C12 规格：02 §5.4（settings `classifier_model`，空=自动 pick_classifier；非空须过运行时 validate：is_active ∧ 健康≠down，否则中文报错回落自动并告警）。
- E1 规格：05 §五 E1（`router.rs:511` 死代码复活，EWMA 读 `usage_records.latency_ms`/`avg_latency_ms`，列已在 db.rs:90/224）；`/metrics` 补延迟指标。
- 演示意义：设置页可见的新功能点 + 指标面板出现延迟维度（「可用性/成本评分」两维度的可见化）。

### A4 · WebUI「路由评测」图表页（2 天）——演示视觉主场

- **依赖引入**：echarts（npmmirror 源已固定于根 `.npmrc`，本机已验证可达；package.json 现无图表库）。降级路径见 §八。
- **四个组件**：
  1. **AIQ 曲线图**：五线对比（LLooM 当前策略 / Cheapest / Best-Single / kNN 基线 / Oracle 上界）横轴成本纵轴质量；
  2. **质量-成本 Pareto 散点**：每点 = 一个模型在该 bench 上的 (avg_cost, avg_quality)，直观呈现「大小模型差异大」；
  3. **逐实例选择分布堆叠图**：按 task_family 分组，路由把哪类题分给了哪个模型（堆叠 = 模型，分组 = 任务族）；
  4. **逐实例表**：prompt → 路由所选模型 → 对/错 → 该样本成本 vs 若选最强模型的成本差（「这题省了多少钱」）。
- **数据源**：新端点 `GET /api/bench/results`（读 B1 的 bench_runs/bench_results 表）；页面新增到 `App.tsx:23-28` 路由表。
- D3「两者兼顾」的落法：骨架层学术严谨（AIQ+基线表+共同成本区间标注），动线层效果向（单题省钱明细、现场回放动画）。

---

## 四、方案 B：Benchmark 改造五项（B1-B5，约 6-8 天）

> 核心构想（用户原话）：「固定模型池 + router，路由机制介入进行零成本测试」。机制 = **预置结果矩阵回放**——真实路由决策（LLooM 评分路由原样跑），预置数据（矩阵查表得对错与成本），零 API 调用。

### B1 · 新表（1 天，ensure_columns/幂等迁移惯例，迁移前备份 lloom.db）

```sql
CREATE TABLE IF NOT EXISTS bench_datasets (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT UNIQUE NOT NULL,             -- "routerbench_v1" / "local_pool_v1"
  source TEXT NOT NULL,                  -- routerbench | lm_eval
  meta_json TEXT,                        -- 导入时间/样本数/模型清单/价目口径说明
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS bench_results (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  dataset_id INTEGER NOT NULL REFERENCES bench_datasets(id),
  sample_id TEXT NOT NULL,               -- "gsm8k.test.5" / "mmlu-astronomy.val.5"
  task_family TEXT NOT NULL,             -- gsm8k | mmlu | hellaswag | mbpp | mt_bench | rag | ...
  model_name TEXT NOT NULL,              -- 真实池名或虚拟池名
  prompt TEXT, performance INTEGER,     -- 0/1
  cost REAL NOT NULL,                    -- RouterBench 原始 cost（2024 价）或 LLooM act_cost
  latency_ms REAL, response TEXT, true_label TEXT,
  tokens_in INTEGER DEFAULT 0, tokens_out INTEGER DEFAULT 0,
  UNIQUE(dataset_id, sample_id, model_name)
);
CREATE INDEX IF NOT EXISTS idx_br_ds_sample ON bench_results(dataset_id, sample_id);
CREATE INDEX IF NOT EXISTS idx_br_ds_model ON bench_results(dataset_id, model_name);
-- 回放运行记录（每次回放一行策略×数据集的选择明细）
CREATE TABLE IF NOT EXISTS bench_runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  dataset_id INTEGER NOT NULL, strategy TEXT NOT NULL,   -- lloom | cheapest | best_single | random | knn | oracle
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  summary_json TEXT                       -- AIQ/总成本/总质量/逐任务分项（明细行另存或 JSON 内嵌，评审时定）
);
```

### B2 · 路线一：RouterBench 导入器（1-2 天）

- `scripts/bench_import_routerbench.py`：解析 JSONL（字段见 §2.1）→ 写 bench_results；task_family 从 `eval_name` 前缀解析（hellaswag/mmlu/gsm8k/mbpp/…）。
- **模型名映射层（关键设计）**：2024 模型（GPT-4/Claude-2/Mixtral…）→ 虚拟模型（内存注册表构建，**不污染真实 models 表**）：名字带 `virtual:` 前缀（如 `virtual:gpt-4-2024`），capability_tier 按其 2024 定位标 1-3，价目用 RouterBench 原始 cost 换算的等效单价（或按 token 数回推）。
- **quality_override 构建**（路由介入点的一半）：`(virtual_model, task_family)` 粒度聚合 performance 均值 + sample_count 权重——**大小模型差异正是通过这个通道注入路由评分**（例：gsm8k 上 GPT-4 聚合分 ≈0.85、Mistral-7B ≈0.4 → 路由「知道」谁擅长什么）。
- 数据拉取：见 §九.1 三级回退（已实测 hf-mirror 401 / GitHub raw 超时，评审重点）。

### B3 · 零成本回放引擎（核心，2-3 天）

- **新增 `crates/lloom-core/src/bench.rs`**，不走 ai_client（不调真实 LLM）：
  1. **路由链复现**：对每个 sample 复刻 `route()` 组装链（`router.rs:544-660`：classify → band_for → policy → PlanInput），但 DB 只读 + **虚拟模型池**（B2 内存注册表）+ 映射价目构建 models/price_specs/quality_override。第三次组装有先例模式（route() 本体、review.rs:156 网格重放）。band_for 需吃 task_family 映射（bench 任务族 → LLooM task_type 词汇表的映射表，导入器产出）。
  2. **零成本判定**：`plan()` 选出 m* → 查 bench_results `(sample_id, m*)` 行 → 得 performance 与 cost——**预置矩阵替代真实调用**；未命中行（该模型没跑过此样本）按策略记录 miss 并用该模型在此 task_family 的聚合分模拟（评审时定口径，见 §九.5）。
  3. **六基线对照**：Cheapest（永远选最便宜）/ Best-Single（质量最高者全选）/ Fixed-Random / kNN（虚拟池等价实现）/ Oracle 上界（每样本选「成本加权质量」最优——注意 hindsight oracle 批判，展示时标注为上界参考非可学空间）/ LLooM 当前策略。
  4. **输出**：AIQ（凸包+共同成本区间积分，参照 aiq_replay.py 凸包逻辑重写为纯函数可单测）+ 每样本选择/对错/成本 → bench_runs。
  5. **CLI**：`lloom-cli bench replay --dataset routerbench_v1 --strategies all`。
- **红线遵守**：评分逻辑 100% 复用 `plan()`/`score_all` 本体（不改决策层）；回放全只读 + bench_* 新表（不碰 usage_records/models）；settings `bench_replay_enabled` 总开关。

### B4 · 路线二：真实池自采（lm-eval，1 天配置 + 跑批）

- 命令形态：`lm_eval --model local-chat-completions --model_args base_url=http://127.0.0.1:7861/v1,model=<注册名> --tasks gsm8k,mmlu_* --log_samples --limit 100`。
- 跑法：**逐模型固定**（`--model_args model=qwen-flash` 等，绕开路由直接测裸模型能力）→ 产出「真实模型池 × 样本」结果矩阵 → 导入 bench_results（source=lm_eval）→ **再**用 B3 回放引擎让 LLooM 路由在此真实矩阵上做零成本评测。
- 另一可选跑法（演示彩蛋）：把 base_url 指向 LLooM /v1 且 model=auto——直接测「路由开启时的端到端能力」，与逐模型矩阵对照即路由增益。此跑法打真实 API（成本进 usage_records，A2 修复后计价可信）。
- 成本：每任务 100 条 × 3-4 模型，按现注册价目估算 $1-5（小额真实消费授权范围内；预算护栏 ≤2% 日预算既有）。
- 兼容性注意（§2.2 约束）：先 `--limit 10` 抽查答案抽取；gsm8k 用 generate_until 型任务；mmlu 多选类对聊天 API 可能不适配，备选 hellaswag/arc（同为选择型需 logprobs 的换掉，用 gsm8k/mbpp/ifeval 生成型）。

### B5 · 端点与页面接线（1 天）

- `POST /api/bench/import`（上传 JSONL，落 bench_datasets/bench_results）/ `GET /api/bench/datasets` / `POST /api/bench/replay`（触发回放，同步或后台 job）/ `GET /api/bench/results?run_id=`（A4 页面数据源）。
- `lloom-cli bench import|replay|show` 子命令（对齐既有 CLI 风格）。
- WebUI：A4 页面对接 + Overview 折叠面板的 AIQ 文本区加「查看曲线」跳转。

---

## 五、演示动线（0.5 天打磨，四步脚本）

1. **开场（学术骨架）**：WebUI「路由评测」页 → 五线 AIQ 曲线 + 共同成本区间标注 → 一句话讲清 AIQ 定义与基线（Cheapest=左下角、Best-Single=右上角、LLooM 在两者之间偏右上、Oracle=上界虚线）。
2. **单题深挖（效果向）**：点开 Pareto 图某模型 → 逐实例表过滤 → 「这题 MMLU 天文简单题，路由选了 Mistral-7B（tier 1），performance=对，省 95% 成本；隔壁 GSM8K 难题路由选了 GPT-4」——**大小模型差异 × 路由判断正确**的双重展示。
3. **现场回放（互动）**：挑 10 条样本实时跑 plan() → 选择分布堆叠图动画生长 → 点开任一决策看 C10 快照（候选集/评分/选择概率）——「路由不是黑盒」。
4. **附录（严谨背书）**：真实池自采数据页（act_cost 对账、计价不变量断言绿）+ 延迟指标 + 与 2024 基线模型的对比表 + 「零成本回放方法论」一页（预置矩阵机制图）。

---

## 六、排期与依赖（单人 ~12-13 工作日；三人分工映射见 08）

```text
第 1 周：A1(C10) → A2(计价) → A3(C12+E1)          ~5 天
第 2 周：B1(表) → B2(导入) → B3(回放引擎)          ~5 天
第 3 周前半：B4(自采,可与第2周并行) + B5(接线) + 演示动线  ~2.5 天
依赖：A1 → A4（快照数据源）；B1 → B2 → B3 → B5 → A4（页面出真数据）；A2 → B4（真实池计价可信）
```

08 分工映射（若三人并行）：甲=A1/A2/B3（决策层+回放引擎）、乙=B2/B4（数据管线）、丙=A4/B5（前端+接线）——三人时总周期可压至 ~5-6 天。

---

## 七、验收 gate

| 项 | gate |
|---|---|
| 构建 | `cargo build && cargo test` 全绿（122 单测基线不破）+ bench.rs 新增单测（凸包积分纯函数、六基线选择逻辑、miss 处理） |
| A4 页面 | 四组件（AIQ 五线/Pareto/堆叠/逐实例表）全部真数据渲染，无空态占位 |
| B3 回放 | RouterBench 子集（gsm8k+mbpp+hellaswag 各 200 条抽样）× 6 基线全出数；LLooM 策略 AIQ 落在 Cheapest 与 Best-Single 之间且 > Cheapest（ sanity）；与论文 kNN≈0.9 量级可比对（论文口径 AIQ∈[0,1]） |
| B4 自采 | ≥2 任务 × ≥3 模型 × ≥100 实例；act_cost 落库且对账通过（A2 断言无告警） |
| 决策快照 | 回放运行的每次选择可点开查候选集/评分/概率（C10 字段非空） |
| 回退 | settings 总开关 off 后系统行为与现状逐位一致（单测锁定） |

---

## 八、风险与回退

| 风险 | 缓解 |
|---|---|
| RouterBench 数据拉取被墙（hf-mirror 401、GitHub raw 超时已实测） | 三级回退（§九.1）：HF 官方 CLI 网络好时拉 → 镜像重试 → 论文方法 mini 复现（自跑 200 条/任务 × 虚拟池价目）；LLMRouterBench 同法再试 |
| 虚拟池质量分失真（task_family 聚合掩盖样本内差异） | 聚合分 + sample_count 双列展示；kNN 基线天然吃逐样本信息，若 LLooM 输给 kNN 诚实展示并归因（这正是要测的东西） |
| echarts 引入失败 | 降级 antd 内置（Statistic/Progress/Table）保功能牺牲动效；或 SVG 手绘五线图（~200 行，可控） |
| lm-eval 对 /v1 兼容坑（聊天 API 无 logprobs） | 任务选 generate_until 型（gsm8k/mbpp/ifeval）；`--limit 10` 先抽查；失败换 `--predict_only` + 自写判分 |
| 自采成本超支/限流 | 每任务 100 条起步；429 走既有 fallback；预算护栏 |
| 回放引擎与真实 route() 行为漂移 | 组装口径单测对齐（review.rs:131 先例：注释明写「口径与 route() 对齐」）；band 映射表进 meta_json 可追溯 |
| Oracle 用 hindsight 被质疑 | 展示时显式标注「hindsight 上界，含采样随机性」（07/最终版批判已内化） |

---

## 九、待评审拍板的开放点（评审清单）

1. **RouterBench 数据获取路径**：三级回退的优先级与「都不行」时的决策（mini 复现可接受吗？还是等网络窗口？）——已实测镜像 401，这是最大不确定项。
2. **echarts vs 轻量方案**：引入 echarts（~1MB 依赖，效果最好）还是 antd 内置降级（零新依赖，动效弱）——倾向 echarts（npmmirror 可达）。
3. **自采预算上限**：$1-5 估算是否可接受；逐模型固定跑法（测裸能力）与 model=auto 跑法（测端到端）各跑多少条。
4. **SRTP 时间锚点**：演示时点是否对齐申报节点（影响打磨优先级：曲线页 vs 现场回放）。
5. **虚拟池 miss 口径**：某模型没跑过某样本时，回放按聚合分模拟还是记 miss 剔除（前者样本利用高但有模拟噪声，后者更纯但 kNN/Oracle 基线变弱）。
6. **bench 页面归属**：独立新页（推荐）还是 Overview 扩展标签页。
7. **bench_runs 明细存储**：逐样本明细 JSON 内嵌 summary_json（简单但行大）vs 独立明细表（规范但多一表）——倾向内嵌（回放样本量 ≤ 千级）。

---

## 十、与文档族的关系

- A1/A2/A3 规格 → 05 §五 / 07 §一.3 / 02 §5.4 既有，本文只新增「为什么它们是演示前置」的定位；
- B3 回放引擎是 02 §四 Q1/Q2（oracle 数据集/离线验证）的**具体化先行版**——公开矩阵先当外接数据源用，Q1 的本地导出仍是后续项；
- C10（A1）在 00 §六.2 已建议入账 C10 编号——评审通过后与本文 A1 合并实施；
- 三人分工映射 → 08 §二；
- 本文档不改变 00-08 的任何优先级结论，只在其上叠加「演示优先」的临时排序。
