# 研究报告三：模型思考过程中的通信（多模型协作推理）

> 方向分类：**推理期多模型协作**——不止"选哪个模型"，而是"多个模型在一次思考过程中如何交换信息"。LLooM 作为网关，天然站在多模型通信的枢纽位置。
> 研究对象：MoA 与 Self-MoA 反思（arXiv:2502.00674）、多智能体辩论与混合推理链（Reasoning Relay, arXiv:2512.20647）、跨模型 KV 缓存迁移（NVIDIA, arXiv:2608.03893）、SpecCoT/PyroDash 等 SLM-LLM 协作推理；Switchyard 的 escalation 会话闩锁与 libsy Step 流。
> 状态：**研究报告，不含任何代码改动**。落点以函数名为锚，基于 v2 @ `2098fce`。
> 关联台账：B12（BEST-Route 并行采样）、C1（思考过程展示已落地）、B11（Switchyard）、B4/G2（Agent 运行时）

---

## 一、研究图景：通信发生在哪三个层面

**层面 A：输出级并行聚合（parallel + aggregate）**——N 个模型各自完整生成，再由聚合器合成。代表：Mixture-of-Agents（Wang et al. 2024：分层聚合多模型输出显著提升）、LLM-Blender（配对排序+生成融合）、self-consistency（同模型多采样投票）。
**关键反例（诚实设计必须吸收）**：*Rethinking Mixture-of-Agents: Is Mixing Different Large Language Models Beneficial?*（arXiv:2502.00674，ICML 2025）发现 **Self-MoA（只用最强模型多次采样再聚合）平均比混合多模型高 6.6%**——"多样性有益"这个直觉在大量场景下不成立。**结论：跨模型并行聚合不应作为默认开启的策略，而应作为与 Self-MoA 对比的实验臂，由影子数据裁决。**

**层面 B：序列级接力与修订（handoff / critique）**——模型之间沿时间轴传递"半成品思考"。代表：
- **Reasoning Relay**（arXiv:2512.20647）：在 CoT 的 25%/50%/75% 处截断并**换另一个模型续写**，用 Process Reward Model 评测——混合推理链经常保持甚至提升最终准确率与逻辑结构。含义：思考中段换模型的"接力"是可行的，前提是接力点质量可控。
- **NVIDIA 跨模型 KV 缓存迁移**（arXiv:2608.03893）：同家族模型间翻译 KV cache，接力提速 ~25×；但存在任务相关塌缩风险（GSM8K 上无翻译时准确率崩）。含义：**网关层的接力应默认走"文本级"（把已生成的思考作为上下文传给下一个模型），KV 级迁移只有自建 vLLM 同家族部署才有意义**（对 LLooM 当前 DashScope/Ollama 形态不适用，记为远期）。
- **SpecCoT / SCoT / PyroDash**：token/步级的小-大模型交替（小模型起草、大模型验证/接管），延迟与成本导向。
- **AutoMix / BEST-Route（台账 B12）**：小模型先答 + 置信/裁判判断是否升级——LLooM 的 escalation 已属此族。

**层面 C：会话/轨迹级路由（agent 通信中的阶段信号）**——agent 工作流里，对话历史本身就是"思考过程"：tool 结果、报错、长轨迹都是阶段信号。代表：**Switchyard stage_router**（按会话中 tool 结果/错误信号逐 turn 在 capable/efficient 两档间切换，信号读取零额外模型调用）与 **escalation 会话闩锁**（弱档起步，judge 读轨迹判质；升到强档后**连续 2 次确认才回弱档**，judge 失败放行不阻塞）。这层与 `/v1` 代理路径（N1）直接相关：外部 agent 客户端的每一 turn 都是 LLooM 的路由决策点。

---

## 二、与 LLooM 现状的逐点映射

LLooM 已经有一个**骨架完整的思考管线**，缺的是"通信协议"：

| 能力 | 现状（锚点） | 通信缺口 |
|---|---|---|
| 分解→执行→聚合 | `ai_service.py::orchestrate_stream`（:1537），分解器/执行器/聚合器由 Rust `plan_decision` 分别指派（P0.f） | 分解器的**计划与推理**（reasoning）不传给子任务——子任务只拿到任务描述字符串；聚合器只看子任务结果，不看执行过程 |
| 升档重试（escalation） | P4.c：零成本质量信号 `_quality_signal_ok`（非空/长度/无失败哨兵）不达标 → `_strongest_model` 升档**重试** | 升档是**重跑**不是**修订**：强模型看不到弱模型草稿与失败原因——最贵的一种"无通信" |
| 并行执行 | N3.a 波次并发（ThreadPoolExecutor，`ai_service.py:1886` 附近） | 并行只用于**无依赖子任务**，没有"同题多采样并行思考 + 聚合"模式 |
| 思考内容捕获 | C1 已落地：`_call_llm` 捕获 `reasoning_content`（`ai_service.py:1303-1334`），SSE 透传 + WebUI 折叠展示 | reasoning 只做展示，**不进任何下游模型的上下文**——管道里的油没被烧 |
| 轨迹感知路由 | `/v1/chat/completions`（`openai_compat.rs`）逐请求 route()，不解析会话最后一条消息类型 | agent 客户端的 tool 结果/错误 turn 与普通 chat turn 得到同等对待；无 Switchyard 式阶段切换与会话闩锁 |
| 裁判 | P4.c 零成本信号（结构化解析）+ 单价代理定"强档" | 无 LLM judge 环节（RouterBench 结论：裁判错误率 >0.2 时级联崩塌——零成本信号优先是对的，但结构化不可判任务因此完全无升级通道） |

---

## 三、目标设计：四个通信原语（按投入产出排序）

### 3.1 C-comm-1 升档重跑 → 升档修订（评审-修订通信）★最小改动、最大收益

把 P4.c 的"升档重试"从重跑改为**带失败上下文的修订**：

```python
# ai_service.py::_execute_task 升档分支（示意）
# 现状：result = _call_llm(strong_model, task_prompt)                 # 重跑
# 目标：
revise_prompt = (
    f"{task_prompt}\n\n---\n以下是上一次尝试的草稿，质检未通过（信号：{fail_signal}）。\n"
    f"草稿：\n{draft}\n请在其基础上修订并给出最终答案，不要从零重写。"
)
result = _call_llm(strong_model, revise_prompt)
```

- **成本语义**：修订多付输入 token（草稿 + 失败原因），省输出 token 与"推倒重来"的隐性质量损失。对 coding/math 类任务，草稿含部分正确结构时修订收益最大（Reasoning Relay 的结论支持：部分链保留优于重开）。
- **契约改动**：SSE `task_done` 事件加 `mode: "retry" | "revise"` 字段；Rust 侧成效信号把"修订成功"记为独立 σ（`models.rs::QualitySignalKind` 加 `ReviseSuccess`，值介于 Success(+0.7) 与首过之间，如 +0.5——因为修订过的事实应折价记入模型质量）。
- **验收**：影子/冒烟中构造必失败子任务，对比重跑 vs 修订的 (成本, 通过率)；`escalated_from` 语义扩展为 `escalated_from + revise=true`。

### 3.2 C-comm-2 分解计划下行（规划作为通信载体）

分解器产出的不只是子任务列表，还有**整体计划**（依赖关系、目标约束）。把计划下行给每个子任务：

```python
# DECOMPOSE_SYSTEM_PROMPT 的输出 schema 追加 "plan": "一段 ≤200 字的整体策略"
# _execute_task 执行子任务时：
sub_prompt = f"整体任务：{query}\n整体计划：{plan}\n你的子任务：{task}\n{依赖上下文}"
```

- 依据：MoT 级联（Yue et al., ICLR 2024）与 least-to-most 的经验——中间表示共享提升弱模型在子任务上的正确率；且 plan 字段天然是聚合器对齐各子任务口径的锚。
- **成本护栏**：plan ≤200 字 + 仅传给同波子任务一次；`est_in` 回调时已含该增量（Python `count_tokens` 现算）。
- **reasoning 上行（可选开关）**：子任务 `task_done` 携带 `reasoning_digest`（C1 已捕获的 reasoning 截断摘要，≤300 字），聚合器 prompt 里作为"执行过程备注"折叠注入——只对 `band=hard` 开启（token 成本控制）。

### 3.3 C-comm-3 并行思考 + 聚合（Self-MoA 优先，Mixed-MoA 对照）

对 `escalation_enabled=1` 且 `band=hard` 的任务，提供"并行思考"模式（挂接台账 B12）：

```text
模式 S（默认，Self-MoA）：最强可用模型 n=2 温度采样（T=0.7/1.0）→ 现有聚合器二选一融合
模式 M（对照，Mixed-MoA）：两个不同档位模型各答一次 → 聚合器融合
执行：复用 N3.a 波次线程池（把"同一子任务的 n 个采样"当波内并行单元）
成本闸门：routing_policy 加列 max_parallel_think INTEGER DEFAULT 0（0=关闭）；
         触发预算 = plan() 估得本任务成本 × n，走 max_cost_per_request 门槛复核
裁决：影子路径 AB 对比 S/M/单发三臂的 (AIQ, 胜率, 成本)，数据说话——
     若 Mixed-MoA 无显著优势（Self-MoA 论文预期），常开模式定格为 S 或关
```

- 与 BEST-Route（B12）的关系：BEST-Route 是"低置信才并行"，本项是"hard 带显式并行"；共享并行执行体与聚合器，B12 触发时只需加置信判据。
- 聚合器提示词需加"多候选一致性"模式：两候选冲突时指出分歧而非盲选（对应 MoA 的合成式聚合而非投票）。

### 3.4 C-comm-4 agent 轨迹感知路由（/v1 代理路径，Switchyard 本土化）

把 Switchyard stage_router + escalation 闩锁的机制移植到 `openai_compat.rs`（提前兑现 B11 的核心收益，不等其 v1.0）：

```rust
// openai_compat.rs 代理路径在 route() 前加轨迹信号检测（零模型调用，纯启发式）：
//   last_msg_type = messages.last() 类型判定：
//     role=tool / content 含 error 关键字 / tool_calls 非空       → "stage:capable" 信号
//     普通 user turn + 会话已在中段                              → "stage:efficient" 信号
// 会话闩锁（SessionLatch）：
//   HashMap<session_key, Latch>（内存 LRU + TTL 30min；session_key = api_key 或 conversation 头）
//   规则（Switchyard 语义）：
//     - tool/error turn → 强制 capable 档候选（min_capability_tier 抬到 3）
//     - 闩锁 strong：连续 2 个 capable turn 确认后才允许回 efficient（防抖）
//     - 检测失败放行普通 route()（fail-open，永不因信号解析阻塞请求）
// 配置：settings routing.agent_stage_mode = off | shadow | live
```

- **与 B15 粘性的关系**：阶段信号决定"能力档需求"，粘性决定"档内选谁"——两层正交，都注入 `plan()`（stage 信号走 band/tier_req 抬升，等价于一次自动 `band_for` 增强）。
- **观测**：`routing_decisions.signals_json` 已有字段记录 `agent_stage` 信号；`/metrics` 加 stage 分布计数（`metrics.rs::render` 扩展一个计数器）。
- **价值**：为 G2/B4（MCP/Agent 运行时）铺路——LLooM 从"逐请求代理"升级为"理解 agent 轨迹的路由器"，这是 Switchyard 的产品定位，也是 OpenAI/Anthropic 兼容层之上真正的差异化。

### 3.5 明确不做（诚实边界）

- **KV 级跨模型接力**：需要自建 vLLM 同家族部署 + KV 翻译层（NVIDIA 方案），当前云端+Ollama 形态不适用；Ollama 单实例同模型会话续接已由"会话粘性"覆盖。记为 G1 之后的多实例远期项。
- **通用 LLM judge 裁判**：RouterBench 证明裁判错误率 >0.2 反噬收益；开放式任务继续不做质量判据（维持 P4.c 边界），judge 只用于影子评测离线打标（报告一 L3）。

## 四、验收方案（复用影子基建）

| 实验 | 对比臂 | 指标 | 判据 |
|---|---|---|---|
| E1 升档修订 | 重跑 vs 修订（构造失败子任务） | 成本/通过率/修订后质量 | 同通过率下成本降或同成本通过率升 |
| E2 计划下行 | 有/无 plan 字段 | 子任务首过率、escalation 触发率 | 首过率升且 est 成本增幅 <10% |
| E3 并行思考 | 单发 / Self-MoA / Mixed-MoA | AIQ、胜率、成本 | 数据裁决常开档位 |
| E4 轨迹路由 | 现行 route() vs stage 信号（shadow） | agent 会话任务完成率、每会话成本 | 完成率不降、成本降 |

全部通过 `/api/routing/shadow` 双跑 + `aiq_replay.py` 扩展多臂模式实现，不新增评测基建。

## 五、分阶段落地清单（供后续立项，本报告不实施）

| 序 | 内容 | 预估 | 关联 |
|---|---|---|---|
| C1' | 升档修订（C-comm-1）+ ReviseSuccess 信号 | 1~2 天 | P4.c 既有代码 |
| C2' | 计划下行 + reasoning_digest（C-comm-2） | 2 天 | C1 既有 reasoning 捕获 |
| C3' | 并行思考 S/M 模式（C-comm-3） | 3~4 天 | N3.a 线程池、B12 |
| C4' | 轨迹感知路由 + 会话闩锁（C-comm-4） | 3~4 天 | openai_compat.rs、B11 |
