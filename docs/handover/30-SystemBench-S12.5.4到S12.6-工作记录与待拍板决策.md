# 30 — SystemBench S12.5.4→S12.6 工作记录：Protocol 纠偏、L1 Matrix 完成与 L2 中断（附待拍板决策）

> 执行窗口：2026-10-08 ～ 2026-10-09。执行者：ZCode（Nironta 授权自主闭环）。
> 上游文档：29 号（S12.5 终审与后续实施规划 v6，本报告的执行权威）、28 号（S12.5 全程记录）。
> 本报告定位：29 号指导落地过程（S12.5.4 → S12.6-B）的工作内容、进度、问题全记录，
> 以及当前**待 Nironta 拍板的决策清单**（§六）。
> 交付状态：**S12.5.4/S12.5.5 完成（REVIEW 判定）；S12.6-A 完成；S12.6-B partial 中断**——
> 账号免费额度全面耗尽，方案级决策待拍板。

---

## 一、执行链总览（29 号 §四十二 序列对照）

```text
29 号 §四十二 指令链：

S12.5.4 Protocol Stability Probe   ✅ REVIEW（agreement 0.667）      [7082687]
        ↓
S12.5.5 Pool Re-audit              ✅ 双口径重审，互补性仍在         [ef5df8c]
        ↓
[Gate] Protocol stable?            → REVIEW 分支：headline 双 profile
        ↓
S12.6-A 33 L1 × 3 models           ✅ 198 cells + judge 197/198      [af9684d]
        ↓                            （中途额度中断一次，b 方案接力完成）
S12.6-B 30 L2 × 3 models           ⚠️ partial：119/180 采 + 49/119 评 [42216b7]
        ↓                            （第二波额度墙，全面见底）
S12.7 v0.2 Freeze                  ⏸ 等额度/等拍板
```

commits（8 个，基线 origin/v2）：

| commit | 内容 |
|---|---|
| `13c26dc` | S12.5.0-S12.5.1（28 号已记） |
| `a443aee` | S12.5.2-S12.5.3（28 号已记） |
| `7082687` | S12.5.4 Protocol Stability Probe：REVIEW |
| `ef5df8c` | S12.5.5 Pool Re-audit 双口径 + S12.6-A 脚本 |
| `74b524f` | S12.6-A partial（第一波额度墙，如实入库） |
| `af9684d` | S12.6-A 完成（b 方案接力，judge 197/198） |
| `42216b7` | S12.6-B partial（第二波额度墙） |

质量门：clippy 0 错 + Rust 138 测试全绿（S12.5 后复核，bench 层纯 Python 不触 Rust）。
实花合计：S12.5.4 $0.27 + S12.5.2 补充 + S12.6-A $2.11 + L2 $0.9 ≈ **$3.5**（全部免费 tier 内，无计费）。

---

## 二、S12.5.4 Protocol Stability Probe（29 号 §九/§十核心指令）

### 2.1 背景

28 号 Pool Audit 判 GO，但 72 cells 中 90.3% truncated（P1536）且排名 weak>mid>strong 反直觉。
29 号终审：GO 降级 **CONDITIONAL GO**——「模型排名是否被 generation protocol 过度主导」
成为最大风险，先验证 protocol stability 再 Full Harvest。

### 2.2 设计与执行

- **节点**：确定性 6 个（6 个 role 槽位各一：calculation/extraction/reasoning/search/verification/synthesis；
  选择规则=槽位内「所在 case 已选最少 → case_id 升序 → node_id 升序」；全部来自现有 3 个 probe workflow）
- **Profile A（P1536）**：复用 S12.5.2 已有 18 cells（零成本）；**Profile B（P4096）**：新采 18 cells（$0.27）
- 只变 max_output 一个变量（1536→4096），其余（prompt/model/judge/sampling）完全冻结
- Gate 阈值（29 号 §十，**结果前冻结**）：GO ≥0.70 agreement 且截断显著下降；REVIEW [0.40, 0.70)；STOP <0.40

### 2.3 结果：REVIEW

| 指标 | P1536 | P4096 |
|---|---|---|
| truncation rate | 94.4% | **55.6%**（显著下降 ✓） |
| tier 均分排序 | weak(0.933) > mid(0.883) > strong(0.850) | **mid(0.967) > strong(0.950) > weak(0.833)** |
| headroom mean | 0.133 | 0.150 |

- **winner agreement = 0.667（4/6）→ REVIEW 区间**
- 2 个 winner inversion（023/n1_s1 weak→strong、030/nC2 weak→mid）**全是 weak 丢失**
  ——P1536 下 weak 的优势部分来自截断伪影，29 号的怀疑被证实

### 2.4 判读（29 号 §十 REVIEW 条款）

> generation budget 是**实质性 routing variable**；headline 必须同时报告两个 profile，
> 不能只选一个。P1536 = Budget Track；P4096 = Capability Sensitivity Track。

后续所有采集（S12.6-A/B）都按双 profile 执行。

---

## 三、S12.5.5 Pool Re-audit（29 号 §三十三）

零成本（只读已有 outcomes），双口径重算 pool 可路由性：

| 口径 | n | winner 分布 | entropy | cheap-safe | 解读 |
|---|---|---|---|---|---|
| P1536 probe24 全量 | 24 | weak 18 / mid 3 / strong 3 | 1.061 | 79.2% | 与 28 号一致 |
| P1536 同 6 节点 | 6 | **weak 6** | **0** | 100% | weak 全胜 = budget 效应 |
| P4096 同 6 节点 | 6 | weak 4 / strong 1 / mid 1 | **1.252** | 66.7% | 三 tier 都有 winner |

**结论**：P1536 的 weak 压倒性优势确认是 budget 效应；但 pool 互补性两个口径下都真实存在。
R7 仅 candidate、system_pool_v01 待 S12.7 生成（29 号 §十六）。

---

## 四、S12.6-A L1 Full Harvest（完成 ✅）

### 4.1 采集设计与中断处置

**设计**：33 independent 节点（L1 严格 counterfactual-capable，29 号 §十三）× 3 模型 × 双 profile
= 198 cells。calibration 7 / test 26 分列。judge=qwen-turbo（rubric v2）。

**第一波额度墙（10-08 夜）**：跑到 87% 时 qwen3.6-plus（mid）403 Free quota exhausted
（54 cells 快速失败，latency 0.17s），随后 qwen-turbo（judge）同步耗尽（71 cells 未评）。
TaskStop → partial 如实入库（`74b524f`）→ 停下回报。

**Nironta 拍板 b（不充值，10-09）**→ b 方案配置：
- mid = **qwen3.7-plus 代采**（¥2/¥8 官方价入档，与 qwen3.6-plus 同价同档，代际对照；
  S12.7 freeze 时作为 system_pool_v01 变更申报审议）
- judge = **qwen3.8-flash**（池外，¥0.8/¥2.7，rubric 独立性保持；rubric v3 = v2 正文逐字不变仅换 judge 模型行）
- 判定细节：新代际模型（qwen3.7-plus / qwen3.8-flash / qwen3.8-max）有**独立免费额度**（探测 200 OK）

### 4.2 最终状态（`af9684d`）

- 198/198 cells 全部有输出（truncated 178 / ok 20）；mid 55 个全部 qwen3.7-plus 补采成功
- judge 链再次中断换棒：qwen3.8-flash 评 113 个后耗尽 → **qwen3.7-max** 补评 84 个（间歇限流重试全过）
- **judge_ok 197/198**（1 个 invalid_json 按协议记 null 不重试）；rubric v3 统一口径
  （113 cells flash 评 + 84 cells max 评，同 frozen rubric + temp=0，跨 judge 模型如实入档）
- 实花 $0.41（resume）+ 免费 tier（finish）

### 4.3 L1 headline 信号（本批最有价值的结果）

| 口径 | split | scored | winner 分布 | entropy | cheap-safe | headroom |
|---|---|---|---|---|---|---|
| P1536 | calibration | 7 | weak 3 / mid 4 | 0.985 | 7/7 | 0.143 |
| P1536 | test | 25 | **weak 14 / mid 6 / strong 5** | 1.427 | 17/25 | 0.304 |
| P4096 | calibration | 7 | weak 5 / mid 1 / strong 1 | 1.149 | 5/7 | 0.171 |
| P4096 | test | 26 | weak 11 / mid 10 / strong 5 | 1.513 | 19/26 | 0.300 |

- **双 profile winner agreement = 0.500（16/32）**——budget 效应在 33 节点上复核成立，
  29 号 REVIEW 判读被更大样本确认；headline 单口径会严重误导
- 两口径下 pool 互补都真实存在（entropy 1.43/1.51 bits，三层都有 winner）
- cheap-safe 68-73%：weak 在多数节点上已达 strong-0.1

---

## 五、S12.6-B L2 Conditional Probe（partial ⚠️，`42216b7`）

### 5.1 设计

30 artifact_dependent 节点（stage0 冻结 final_text 全部就绪，零成本复用）× 3 模型 × 双 profile
= 180 cells。研究问题（29 号 §S12.6-B）：**artifact 是否真正改变 winner**——
L1（base_text）vs L2（final_text = base + 冻结 artifacts）同模型分数位移与 winner 分布差异。

### 5.2 中断状态

双层中断：
- **采集层**：qwen3.6-flash（45 cells）与 qwen3.6-max-preview（14 cells）额度耗尽 → 61 api_error
- **judge 层**：qwen3.7-max 02:31 撞墙 → 70 cells 有输出未评

已得：119/180 cells 有输出（30 节点全覆盖，但 61 个节点-cell 缺）；judge ok 49/119。
**初步信号（49 个已评 cells，方向性）**：strong 在 L2 条件下均值 0.75 显著占优
（weak 0.51 / mid 0.39）——artifact 注入可能有利于强模型，与 L1 的 budget 效应方向相反，
这正是 artifact-conditioned routing 值得研究的证据，但样本太少不足以下结论。

### 5.3 当前账号额度全景（2026-10-09 实测）

| 模型 | 状态 |
|---|---|
| qwen-plus / qwen-turbo / qwen3.6-plus | ❌ 403 Arrearage（10-08 起） |
| qwen3.8-flash / qwen3.7-max | ❌ 403 Free quota exhausted（10-09 L2 期间耗尽） |
| qwen3.6-flash / qwen3.6-max-preview | ❌ 403 Free quota exhausted（10-09 L2 期间耗尽） |
| **qwen3.7-plus** | ✅ 200 OK（仅存） |

b 方案三模型接力打满：今天 judge 已换 4 棒（turbo→flash→max），采集已换 2 棒（plus→3.7-plus）。
**跨 judge/采集模型的可比性在持续受损**（rubric v3 内部 flash/max 双棒、mid 代际替换、
L1/L2 的 judge 模型不同——全部如实入档，但口径复杂度已超免费模式所能支撑的实验设计）。

---

## 六、待拍板决策（当前停点）

### 决策 1：S12.6-B 收尾路线（影响 v0.2 freeze 时间表）

- **决策点一句话**：免费额度已确定性打满，L2 剩余 61 采集 + 70 评审怎么补？
- **a. 充值开启按量付费（推荐）**：按 manifest v2 官方价 L2 收尾 ~$1；后续 S12.7 freeze、
  S13 artifact replay、S14 主结果的 judge/参考执行全部有稳定计费基础。这套 benchmark
  每一步都要花钱，免费接力模式每步都可能中断换口径（今天已实证）。
- **b. 等免费额度月度刷新**：S12.6-B 挂起；先做零成本收尾（S12.6-A 分析成文、v0.2 freeze
  的 manifest/sign-off 打包准备、O3 exact 枚举器、29 号 §三十一 manifest 字段落地），
  额度恢复再续采。零现金成本，但 freeze 时间表不可控（阿里免费额度刷新周期未官方确认）。
- **c. mid-only 单模型口径收尾**：用仅存的 qwen3.7-plus 做「单模型 Budget Track」。
  不推荐——三层互补是 pool audit 前提，单模型没有 routing 问题可问，v0.2 headline 失去意义。

| 维度 | a 充值 | b 等刷新 | c 单模型 |
|---|---|---|---|
| 实现成本 | ~$1-3（含后续 judge） | 0 | ~0 |
| 性能效果 | 全量矩阵完整 | 补齐后同 a | headline 降级 |
| 维护扩展 | 计费脚本就位（价格已入 manifest） | 断点脚本就位 | 需重写口径 |
| 风险副作用 | 现金支出需授权 | 时间表不可控 | 研究价值大损 |
| 回退难度 | 低（免费 tier 数据仍在） | 低 | 高（口径已污） |

- **我的推荐**：**a**（依据：L1 已完整、L2 只差 1/3 收尾、b 方案的口径损伤已开始累积；
  金额在「小额真实消费」授权精神内）
- **取决于你的追问点**：DashScope 充值是否方便；若选 b，免费额度刷新周期你是否知情
  （我不确定，未查到官方明确说明）。

### 决策 2：S12.7 v0.2 freeze 的 scope（决策 1 之后）

29 号 §二十/§三十三 要求 freeze 满足七项 100%（human sign-off / artifact contract /
dependency semantics / routing input hash / execution input hash / L1 coverage / control
no-overlap）+ semantic workflows ≥20 + 双 profile。当前对照：

- ✅ 结构类全部就绪：30 workflows / 175 节点 / contract v1 / sidecar hash / 依赖修复
- ✅ L1 outcome coverage 100%（33/33 节点双 profile 全采 + judge 197/198）
- ⚠️ L2：partial（若选 a/b 补齐则满足；若 c 则 v0.2 降级为 L1-only headline + L2 diagnostic）
- ⚠️ **100% human sign-off（27 号决策 ② headline 节点 gold 条款）尚未执行**——
  需要你本人过目 headline 节点 gold 标注（抽检包 36 节点 + 必审 2 组在 26 号 §报告里），
  这是 freeze gate 的必要条件，无论 1 选什么都躲不开
- **若选 b**，freeze 前的零成本清单我可以立即开工：O3 exact ≤8 节点全枚举器（3^8=6561，
  纯本地计算）、v0.2 manifest 骨架（含 29 号 §三十一新增的 generation_profile_hash /
  outcome_protocol_hash / judge_rubric_hash）、Coordination Cost Ratio 展示口径修正
  （29 号 §十七：18.35 → 19.35× / +1835%）、case-level eligible rate distribution（§二十）

### 决策 3（小项，可并入决策 1）：production finding 的 issue 化

28 号记录的 production finding（ai_client 对 qwen3.6 系列不传 enable_thinking →
生产子任务截断 + 分钟级 latency；max_tokens=estimated 同款截断风险）按 29 号 §二十二
应开独立 issue（P-01）与 benchmark 侧（B-01）分离。等你确认后我在仓库建 issue 或
入台账（不混入 benchmark identity）。

---

## 七、Nironta 原话存档（逐字）

> 「仔细阅读指导，根据内容思考后继续进行（注意上下文管理）」（29 号注入开工）

> 「b」（S12.6-A 额度中断后的账号决策拍板：不充值，用替代模型接力）

> 「检查断点，继续」（两次断点续跑指示）

> 「整理29号指导到现在的工作内容、进度、问题，并附上最近的需要拍板的问题和决策，写为md文档」（本报告）

---

## 八、文件与脚本清单（本窗口新增）

| 文件 | 用途 |
|---|---|
| `benchmarks/systembench/scripts/s12_5_protocol_stability.py` | S12.5.4 双 profile probe + gate 判定（--probe/--gate） |
| `benchmarks/systembench/scripts/s12_5_5_pool_reaudit.py` | S12.5.5 双口径 pool 重审（零成本） |
| `benchmarks/systembench/scripts/s12_6_l1_harvest.py` | S12.6-A 采集（198 cells，断点续采） |
| `benchmarks/systembench/scripts/s12_6_l1_resume.py` | b 方案补采 + judge 统一重评（rubric v3 生成） |
| `benchmarks/systembench/scripts/s12_6_l1_judge_finish.py` | L1 judge 收尾（qwen3.7-max 补 84） |
| `benchmarks/systembench/scripts/s12_6_b_l2_probe.py` | S12.6-B L2 采集（180 cells，断点续采） |
| `benchmarks/systembench/evaluator/judge_rubric_v3.md` | frozen rubric v3（judge=qwen3.8-flash 档，正文同 v2） |
| `benchmarks/systembench/matrices/semantic_outcomes_l1.jsonl` | L1 矩阵 198 cells（judge 197/198） |
| `benchmarks/systembench/matrices/semantic_outcomes_l2.jsonl` | L2 矩阵 180 cells（119 输出 / 49 已评） |
| `benchmarks/systembench/reports/s12_5_protocol_stability.json` | S12.5.4 gate 报告 |
| `benchmarks/systembench/reports/s12_5_5_pool_reaudit.json` | S12.5.5 重审报告 |
| `benchmarks/systembench/reports/s12_6_l1_harvest.json` / `_resume.json` / `_judge_finish.json` | L1 三阶段报告 |
| `benchmarks/systembench/reports/s12_6_b_l2_probe.json` | L2 partial 报告 |
| `benchmarks/routerbench/manifests/model_pool_native.yaml` | v2 + 候选附件（qwen3.7-plus mid 候选 / qwen3.8-flash judge 候选，官方价） |
