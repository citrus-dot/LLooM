# 研究报告二：把编排建模为优化问题（重点方向）

> 方向分类：**约束优化 / 在线优化**。核心主张：LLooM 的路由决策目前是"每请求贪心 argmax + 手调权重 + 手调预算档"，三篇论文共同指出升级路径——把预算、质量下限、并发配额、模型能力不确定性统一进一个**带对偶变量的优化问题**，让数据自己调出"影子价格"，替代手调倍率。
> 研究对象：OmniRouter（Mei et al. 2025，arXiv:2502.20576）为主；StageRoute（Li & Li, ICLR 2026，arXiv:2506.17254）为辅；RouterBench 提供评测框架（AIQ/NDCH，见报告一）。
> 状态：**研究报告，不含任何代码改动**。落点以函数名为锚，基于 v2 @ `2098fce`。
> 关联台账：P5（预算档已落地）、N2（权重网格搜索已落地）、B8/B9（影子验收）、B10（RL 远期）

---

## 一、论文要点：两种互补的优化形态

### 1.1 OmniRouter：窗口内约束优化 + 拉格朗日对偶

OmniRouter 的核心论点：**逐查询贪心路由在全局约束下必然次优**（其图 1 的例子：简单 query 先来抢占了强模型的名额，复杂 query 到达时只剩弱模型）。它把一段时间窗口内 N 条查询、M 个模型的分配形式化为：

```text
min  Σ_i Σ_j  c_ij · x_ij                                  # 总成本最小
s.t. (1/N) Σ_i Σ_j  a_ij · x_ij ≥ α                        # 全局平均质量下限 α
     Σ_i x_ij ≤ L_j   ∀j                                   # 每模型容量/并发上限 L_j
     Σ_j x_ij = 1     ∀i                                   # 每查询恰好一个模型
（a_ij = 模型 j 对查询 i 的成功概率，c_ij = 成本——由其两阶段方案中的预测器给出，
  预测器见报告一 §1.2；x_ij ∈ {0,1}）
```

**求解方式（式 8–25）**：拉格朗日对偶分解，引入三类乘子——λ₁（质量约束）、λ₂ⱼ（每模型容量）、μᵢ（指派约束，解析消去）。KKT 驻点条件给出**极简的贪心规则**：

```text
对每个查询 i：  j*_i = argmin_j ( c_ij − λ₁·a_ij/N + λ₂,j )      # 式(25)
```

乘子按对偶梯度上升迭代（式 22/23）直至收敛：

```text
λ₁ ← max( λ₁ + α₁·( α − avg_quality(x) ),  0 )                # 质量不达标 → λ₁ 升 → 更偏质量
λ₂,j ← max( λ₂,j + α₂·( Σ_i x_ij − L_j ),  0 )                # 模型 j 超载 → λ₂,j 升 → 分流
```

两个值得刻进设计里的命题：
- **命题 1**：上述 argmin 规则即最优指派——说明"评分函数"与"对偶变量"可以完全解耦：**LLooM 不需要引入 LP 求解器，只需要在现有评分公式里把 λ 变成在线更新的变量。**
- **命题 2**：λ₁ > 0 时质量约束恰好贴边满足——λ₁ 就是"质量的价格"。这给了可解释性：WebUI 可以直接展示"当前质量影子价格 λ₁=0.8"，替代现在 throttle/tight 的 magic number。

实验结果：10 模型池（Qwen2.5 7B~72B、Gemma2、GPT-4o-mini/4o、Gemini-flash、Claude-3.5-sonnet）+ 2.7k 查询，准确率 75.19% / 成本 $0.0515，全面优于 S3、PO、EmbedLLM、RouterDC、Hybrid-LLM、CARROT；**可控性对比最能说明问题**：质量约束 α 从 0.70 提到 0.90，贪心基线成本爆炸（EmbedLLM $0.229），OmniRouter 只涨 48%（$0.054）；并发上限 L 收紧到 1 时，基线崩、它保持 73.8%。

### 1.2 StageRoute：部署与路由双时间尺度（ICLR 2026）

Li & Li 把问题再抬高一层：**路由之前还有"部署"**——模型池是流式到达的（新模型不断发布），而系统同时只能"部署"（激活/常驻）M_max 个。两级决策：

```text
部署层（慢，每个 stage 开始时一次）DeployOPT —— MIP：
  max Σ μᵁ_m · d_m        s.t. Σ cᴸ_m · d_m ≤ b,  Σ d_m = 1,
       0 ≤ d_m ≤ α_m·z_m, Σ z_m = min(M_max, |M|),  z_m ∈ {0,1}
  其中 μᵁ_m = UCB(质量), cᴸ_m = LCB(成本)   —— 乐观质量 + 保守成本
  置信半径：frad(v,n) = sqrt(γ·v/n + γ/n)                        # 式(4)(5)

路由层（快，每查询一次）RouteOPT —— 小 LP：
  max Σ μᵁ_m · p(m)  s.t. Σ cᴸ_m·p(m) ≤ b,  Σ p(m) = 1,  0 ≤ p(m) ≤ α_m
  解出分布 p*_t，按概率采样模型                                    # 式(7)
```

理论结论与实践准则：
1. 后悔界 Õ(√(M_max·K·T) + N·T/(M_max·K))，与下界匹配 → Õ(T^{2/3}) 近最优。
2. **K ≈ T^{1/3}**（stage 数约等于查询数的三分之一次方）、M_max ≈ N^{2/3} 时最优——"探索容量（并发上限 × 更新频率）本身是稀缺资源，只盯着少数头部模型在动态池里可证明次优"。
3. 计算开销实测很低：部署 MIP 亚秒级、每查询 LP 毫秒级（M≤10），Gurobi 或开源 CBC/HiGHS 均可，且**热启动**（warm-start：上次解作初值）后更快。
4. UCB/LCB 的作用：`needs_calibration` 类探索不是"罚分"，而是**乐观加成**——新模型被自动尝试，且尝试强度随样本量自动衰减。

对我们的意义：StageRoute 的"部署/路由"两层正好映射 LLooM 的两个真实痛点——**模型注册表里挂着一堆模型，但探针/预算/健康机制没有回答"哪些值得保持活跃/常驻"**（尤其本地 Ollama 的加载-卸载是真实秒级成本）；**模型质量/成本目前是点估计（EWMA），探索靠 0.3 罚分的静态规则**（`router.rs::score_all` 中 `needs_calibration != 0 && tier_req > 1 → s -= 0.3`）。

---

## 二、与 LLooM 现状的逐点映射（差距诊断）

| 论文概念 | LLooM 现状（file:line 锚点） | 诊断 |
|---|---|---|
| 对偶变量 λ₁（质量/预算影子价格） | `router.rs::tier_cost_multiplier`：throttle×1.5 / tight×2.5 手调倍率；`budget_tier_from_ratio` 四档阶梯 | **手调阶梯 = λ 的粗糙量化**。预算从 51% 掉到 49% 瞬间 cost_weight 跳 1.5×，不连续；λ 框架下是连续自适应 |
| 每模型容量约束 λ₂ⱼ | **完全没有**。plan() 门槛只有 health/tier/ctx/cost-cap（`router.rs::plan_with_mode` gate 段） | DashScope RPM/TPM 配额、Ollama 并发上限都没进决策；429 只进健康状态机（`health.rs`），不进路由评分 |
| 窗口批优化（N 个查询联合分配） | chat 路径逐请求贪心（`router.rs::route`）；编排路径每子任务**独立**回调 `plan-subtask`（`server.rs:1647 rust_plan_subtask`，Python `_plan_subtask` @ `ai_service.py:1054`，每波并发执行 N3.a） | **编排波次是天然的优化窗口**：一波 2~8 个子任务同时要模型，正是 OmniRouter 的 N×M 小问题；现在逐个 plan()，占用/预算互相不可见 |
| 质量下限 α | routing_policy 只有 min_capability_tier（能力档代理），无显式质量目标 | OmniRouter 的 α 是"本窗口平均质量 ≥ α"，可与 AIQ 报告联动：体检报告给出质量-成本曲线后，α 变成用户可调的一个数 |
| UCB/LCB 不确定性 | `quality_override` 用 EWMA 点估计（sample≥5）；探索 = needs_calibration 罚 0.3（`router.rs::score_all`） | 罚分规则无原则：0.3 不随样本量衰减（sample_count≥20 才解除）；StageRoute 的 frad 半径**自动**完成"探索→收敛" |
| 预算反馈闭环 | `global_budget_ratio()` → budget_tier → cost_weight 倍率。档位只随水位走，**不随烧钱速度走**（月初猛烧不会提前收紧） | λ 的对偶上升每周期看"实际消耗率 vs 目标消耗率"，烧得快就自动升 |
| 离线权重搜索 | `review.rs::grid_search_suggestions`（7×7 网格重放影子样本找帕累托权重，人工采纳） | 网格搜索本质是**离线学 λ**——但只学到静态权重，没有"预算速率"约束轴，且粒度粗（7×7） |
| 粘性（B15） | `sticky_bonus` 动态证据加分，cap 0.25（`router.rs`） | 粘性是**切换代价**的正向代理——在优化框架里应显式建模为切换成本项（见 §4.5） |

**一句话诊断**：`score_all` 的公式 `s = qw·q − cw·mult·norm_cost − lw·lat + 0.05·priority + sticky − 保守罚` 中，`qw/cw/lw` 是静态手调（N2 网格搜索也只能离线学静态值），`mult` 是手调阶梯，`lat` 恒 0，探索罚是常数。优化方向的改造不是推翻这个公式，而是**把其中三个手调量（mult、探索罚、sticky 边界）替换为有原则的在线估计量（λ、UCB 半径、切换代价）**。

---

## 三、目标设计：五个渐进式改造（每层独立可用、独立可回退）

> 总原则（继承项目惯例）：**不动门槛、只动评分**；影子先行、AIQ 验收；单一决策真源仍在 Rust；每层都有 `settings` 开关与回退路径。

### 3.1 B-opt-1 对偶预算路由 λ（替代 budget_tier 手调倍率）★核心

**形式**：每预算域一个对偶变量 λ ≥ 0（先做 global 域，与 `global_budget_ratio` 对齐）。评分函数把 `tier_cost_multiplier(tier)` 替换为 λ：

```rust
// router.rs::score_all 内（示意）
let s = qw * q - cw * input.budget_lambda * norm_cost - lw * norm_latency + ...;
// 语义：λ=0 → 纯质量导向（≈ 现 normal 档）；λ 大 → 强成本导向（连续滑过 throttle/tight）
```

**在线更新（对偶上升，后台 job）**——OmniRouter 式(22) 的预算速率版：

```rust
// server.rs::spawn_background_jobs 新增 dual_update_loop（每 10min）：
//   spend_rate = 当日 usage_records SUM(cost) / 已流逝分钟           （真源：priced_usage）
//   target_rate = budget_monthly / 当月分钟数 × (1+soft_limit_ratio)   （真源：budgets 表）
//   viol = (spend_rate - target_rate) / target_rate                   （归一化违反度）
//   lambda ← clamp(lambda + eta * viol, 0.0, LAMBDA_MAX)
//   eta 初始 0.5，LAMBDA_MAX 对应旧 protect 档上界（防发散）
//   persist: settings KV "routing.dual_lambda"（重启不丢）
```

- **与现有 budget_tier 的关系**：过渡期两者并存——`budget_tier` 保留为**硬门槛**（protect 仅本地等安全网不变，`plan_with_mode` gate 段不动），λ 只接管**软评分**部分。AIQ 对比确认后再决定是否简化掉 throttle/tight 两档的评分倍率（protect 保留）。
- **验收**：合成流量下，λ 随消耗率单调响应；影子重放中 λ 路由的"成本-质量"曲线压住固定倍率曲线（`aiq_replay.py` 加第四条线）。
- **可解释性**：`/api/routing/review` 报告与 WebUI 体检卡展示当前 λ 值及其含义（"质量影子价格"，呼应 OmniRouter 命题 2）。

### 3.2 B-opt-2 UCB/LCB 置信评分（替换 needs_calibration 罚分）

按 StageRoute 式(4)(5)，把点估计换成区间估计：

```rust
// db.rs：get_model_task_score 返回已有 sample_count；新增纯函数
fn frad(v: f64, n: i64, gamma: f64) -> f64 { (gamma * v / n as f64 + gamma / n as f64).sqrt() }

// router.rs::score_all（示意）：
let n = sc.sample_count as f64;
let q_ucb = (ewma + 2.0 * frad(ewma, sample_count, gamma)).clamp(0.0, 1.0);   // 乐观质量
let c_lcb = (avg_cost - 2.0 * frad(avg_cost, sample_count, gamma)).max(0.0); // 保守成本
// sample_count < 5 时直接用 UCB 上界（frad 大 → 自动获得探索机会）
// needs_calibration 惩罚项移除——UCB 自然完成"新模型被试、试错自动衰减"
```

- `settings routing.ucb_gamma`（论文默认 γ=0.1，我们从小值起步，因为单用户网关的探索成本更痛）。
- **探索配额保险丝**：每 (model, task_type) 每日 UCB 触发的选择次数上限（防小流量下 UCB 长期霸榜），走 settings KV。
- 验收：单测——新模型 sample=0 时 q_ucb=1.0（乐观可入选），sample=20 后 UCB 收敛到 EWMA±ε；影子 AIQ 对比罚分方案。

### 3.3 B-opt-3 并发/吞吐约束 λ₂ⱼ（429 反馈回路）

```text
models 表幂等迁移：rpm_limit INTEGER（0=不限）、concurrency_limit INTEGER
进程内滑窗计数（dashmap，60s 窗口），plan() gate 阶段：
  超 rpm_limit 的候选 → 不是硬剔除，而是 s -= lambda2j（λ₂ⱼ 惩罚）
λ₂ⱼ 更新：两种信号源——
  a) 主动计数：窗口内已发请求数 / limit → 比例惩罚
  b) 被动反馈：health.rs 收到 429/限流 → λ₂ⱼ += δ（时间衰减回落）
（就是 OmniRouter 式(23)：负载违反度 × 步长，且与 B-opt-1 共享"对偶"心智模型）
```

落点：`health.rs` 已捕获上游错误类型（加 429 识别）；`router.rs::PlanInput` 加 `overload_penalty: &HashMap<String, f64>`（与 `hit_rate` 同模式注入）。验收：单测模拟 limit=10、11 请求/分钟 → 第 11 次决策偏向第二候选。

### 3.4 B-opt-4 编排波次联合分配（OmniRouter 完整形态落地）★核心

编排路径的并行波次（N3.a：`ai_service.py` 按 `depends_on` 分波，同波 ThreadPoolExecutor 并发）就是现成的优化窗口。把"每子任务独立回调 plan-subtask"升级为"每波一次联合 plan"：

```text
新端点 POST /api/routing/plan-wave（与 plan-subtask 并存，Python 失败回落旧逐个回调）：
req:  { "items": [ {id, task_type, est_in_tokens, est_out_tokens, band} ... ],
        "quality_alpha": 0.75,          # 可省略 → 用各 task_type routing_policy 加权默认
        "request_id" }
resp: { "assignments": { "<id>": {primary, fallback_chain, ...} } }

Rust 求解（router.rs 新纯函数 plan_wave()，不引外部 LP 库）：
  N ≤ 8、M ≤ 7 → 两级求解：
  1) 拉格朗日松弛（OmniRouter 式 22-25）：固定 (λ₁, λ₂ⱼ)，每 item 独立 argmin——
     这正是现有 plan() 评分公式！λ 从 quality_alpha 与窗口预算推导初值；
  2) 小规模局部交换修复：若窗口质量均值 < α，逐个把"质量增益/成本增量"比最高的
     item 换到更强候选（贪心修复）；容量超限同理反向修复。
  复杂度 O(iter × N × M)，iter≤5，微秒级。MIP/LP 库不引入（受网络限制拉不了 crates.io
  的约束与 StageRoute "开源求解器可行但本问题规模枚举足够" 的观察一致）。
```

- **约束兼容**：λ₁ 初值由 `global_budget_ratio` 与窗口预估总成本共同决定；sticky 证据作为该 item 的评分修正照旧注入（B15 通道不动）；escalation_enabled 语义不变（联合分配只决定初始指派，失败降级仍走 Python 执行体）。
- **收益预估**：OmniRouter 图 1 的场景在 LLooM 真实存在——一波里 1 个 coding + 3 个 general 子任务，独立 plan 可能把 4 个全分给同一强模型（各自都是局部最优），联合分配可在保 α 下把 general 换到便宜模型。
- 验收：`plan_wave` 纯函数单测（构造 α 紧/松、容量紧/松四象限）；影子重放对比"逐个 plan vs plan_wave"的成本-质量曲线；Python 侧 `_plan_wave` 失败自动回落 `_plan_subtask`（兼容老服务）。

### 3.5 B-opt-5 切换代价显式建模（衔接报告四）

B15 的 `sticky_bonus` 本质是"不切换的收益"。优化框架下更准确的形态是**切换代价**进入窗口目标：`c_ij_effective = c_ij + switch_cost(上一模型, j)`，其中 switch_cost = 会话缓存损失（B15 已算出：avg_cached_tokens × cache_price_delta）+ 本地模型换载成本（Ollama 场景：若 j 未常驻，加固定惩罚 `ollama_swap_penalty`）。窗口分配天然支持"整波倾向留在已加载/已粘模型上"。

### 3.6 （远期）B-opt-6 部署层：本地模型常驻集合的 MIP

StageRoute DeployOPT 的本地化版本：Ollama 常驻模型集合 ≤ M_max（显存约束换算），按 UCB 质量与 LCB 成本（含加载延迟）选择加载谁、卸载谁；探针结果作为新 stage 的先验。**远期项**：仅当本地模型数 >2 且换载可感知时才值得做（对应探针系统的扩展），先在此存档。

---

## 四、统一视图：一个公式看懂五个改造

```text
现状：  s(m) = qw·q̂_m − cw·κ(tier)·ĉ_m + sticky(m) − penalty(m)         # q̂,ĉ 点估计，κ 手调
目标：  s(m) = qw·q̂ᵁ_m − cw·λ·ĉᴸ_m + λ₂m 罚 − switch_cost(m) − lw·l̂_m
                ↑UCB       ↑对偶↑LCB  ↑吞吐    ↑B15/3.5              ↑补上延迟项
窗口层：对波内 N 个 item 联合 argmax s(m)，附修复：窗口质量均值 ≥ α、单模型占用 ≤ L_j
```

各量真源：q̂/ĉ 仍走 `price_specs`/`model_task_score`/预测器（报告一）；λ 由后台 job 对偶上升；λ₂ 由计数器+429；α 由用户/体检报告给。

## 五、风险与边界

1. **单用户网关流量小，窗口优化收益依赖编排负载**：B-opt-4 只在多子任务波次有收益；单条 chat 保持逐请求。诚实预期：B-opt-1/2 是"机制正确化"（去手调、可解释），收益幅度需影子数据验证（呼应 B8/B9"待真实样本"）。
2. **λ 发散/震荡**：步长 η 上界 + clamp + 周期衰减（类似 ROI 调度）；λ 状态持久化在 settings，重启恢复。
3. **UCB 探索浪费**：γ 小值起步 + 探索配额保险丝；protect 档仍硬门槛兜底，探索不会烧穿预算。
4. **不引求解器依赖**：全部解析/枚举（OmniRouter 命题 1 保证 argmin 即最优；StageRoute 亦指出小规模 MIP 亚秒级但非必需）。规避 crates.io 受限网络风险（项目既有约束，见 LLooMprogress §八）。
5. **兼容性**：五个改造全部可通过 settings 一键关闭回退到现行行为；影子重放是每个改造的统一验收面（`aiq_replay.py` 加策略参数，多曲线对比）。

## 六、分阶段落地清单（供后续立项，本报告不实施）

| 序 | 内容 | 验收 | 预估 | 依赖 |
|---|---|---|---|---|
| O1 | B-opt-1 对偶 λ（dual_update_loop + score_all 接 λ + settings 开关） | λ 响应性单测；影子 AIQ 压住固定倍率；WebUI 展示 λ | 2~3 天 | — |
| O2 | B-opt-2 UCB/LCB（frad + 探索配额） | 新模型探索→收敛单测；AIQ 对比 | 2 天 | — |
| O3 | B-opt-3 λ₂ⱼ 吞吐约束（迁移 rpm_limit + 计数器 + 429 回路） | 超限偏向第二候选单测 | 2~3 天 | O1 框架 |
| O4 | B-opt-4 plan-wave 端点 + plan_wave 纯函数 + Python 回调升级 | 四象限单测；影子对比；回落兼容 | 4~5 天 | O1（λ 初值） |
| O5 | 切换代价进目标（与报告四 D1 合并实施） | 长会话切模率下降 + AIQ 不降 | 1~2 天 | B15 数据 |
| O6 | （远期）部署层 MIP | Ollama 换载率下降 | 视需求 | O1/O2 |

**推荐实施顺序**：O1 → O2 → O4 → O3 → O5 →（远期 O6）。O1/O2 无依赖、收益最确定（去手调 + 原则化探索）；O4 是本方向"标志性"能力；O3/O5 视真实流量痛点插入。
