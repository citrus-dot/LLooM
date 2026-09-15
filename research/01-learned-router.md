# 研究报告一：构建本地训练集，训练编排/路由模型

> 方向分类：**学习型路由**（先有数据，再有模型，最后接管 `plan()` 的质量/成本预测输入）
> 研究对象：RouterBench（Hu et al. 2024，arXiv:2403.12031）为主；OmniRouter 检索增强预测器（Mei et al. 2025，arXiv:2502.20576）与 RouteLLM（Ong et al. 2025，ICLR）为辅
> 状态：**研究报告，不含任何代码改动**。所有落点以函数名为锚（行号随版本漂移，仅辅助定位），基于 v2 分支 @ `2098fce`。
> 关联台账：B8/B9（影子样本验收）、B10（Router-R1 RL，千级样本触发）、C4（复杂判定语料）

---

## 一、论文与开源工作要点

### 1.1 RouterBench：把「路由」变成一个可离线评测的数据问题

RouterBench 的核心贡献不是某个路由算法，而是**数据形态**：对每条 query × 每个候选模型，预先记录一次真实调用的四元组：

```
(sample_id, model_name, prompt, model_response, performance[0/1], cost[$], true_label)
```

规模：405,467 条样本 = 11 模型 × 8 数据集（MMLU / HellaSwag / Winogrande / ARC-Challenge / MT-Bench / GSM8K / MBPP + 一个带 ground truth 的 RAG 集）。有了这份数据，任何路由器都可以**零推理成本**离线评测——这就是 LLooM 已有 `routing_calibration` 表（`db.rs:242`）+ `aiq_replay.py` 想做的事的"完整版"。

关键结论（对我们的设计有直接约束）：

1. **Oracle 上限很高**：GPT-4 极少被 Oracle 选中——多数 query 有更便宜的模型能答对。路由的上限收益是真实的。
2. **简单预测路由只够打平 Zero router**：KNN（40 近邻、all-MiniLM-L12-v2 嵌入、余弦相似）与 MLP（2×100 隐层）在 7 个任务上的 AIQ 与 Zero router 基本持平（如 MMLU 0.773 vs 0.763，MBPP 反而更差 0.642 vs 0.660）。**教训：query→模型能力预测这个学习问题比看起来难，收益不保证。**
3. **级联裁判容错上限 ≈ 0.1**：带完美裁判的 cascading router 大幅超 Zero router（MMLU AIQ 0.901 vs 0.763），但裁判错误率 >0.2 时收益崩塌。**LLooM 的 escalation（P4.c）应只在"结构化可判任务"开启——现有设计（ROUTING-PLAN §4.5）与该结论一致，不要扩大裁判适用面。**
4. **域外泛化（out-domain）也成立**：训练集不含目标任务时，KNN/MLP 路由仍接近域内表现——说明"每 query 能力预测"学到的是难度/题型特征而非记忆。
5. **RAG/复合系统场景路由收益最大**：路由器能从 query 中识别时效性特征（"2024"→在线模型）。对 LLooM 的启示：`/v1` 代理流量（外部 agent 客户端）里这类信号更多。
6. 评测指标 **AIQ**（Normalized Area under非递减凸包 NDCH）与 **Zero router 基线**应原样搬进我们的评测脚本（`aiq_replay.py` 已实现其思想，本文建议补齐 Zero router 线与 NDCH 插值语义）。

### 1.2 OmniRouter 预测器：检索增强 + 双头预测（本报告 V1 版蓝本）

OmniRouter 的第一阶段预测器（论文 §3.1）比 RouterBench 的裸 KNN/MLP 强得多（能力预测 81.3% vs 最强基线 76.1%，消融见其表 5）：

- **编码器**：`bert-base-uncased` 分别编码 query 与模型描述文本 → E_q, E_l。
- **头 1（能力）**：`a_pred = σ(W1·(E_q ⊙ E_l) + b1)`，MSE/BCE 训练。
- **头 2（输出长度桶）**：`softmax(W2·(E_q + E_l) + b2)` 分 **10 桶**（消融：10 桶 > 20/50/100，粗粒度反而好），把"预测输出 token 数"这个病态回归问题变成桶分类。**这直接解决 LLooM 的 `est_out_tokens` 痛点**——现在 `db.rs::task_avg_out_tokens` 只有 (model,task) 粒度 EWMA + 750 冷启动保守点（ROUTING-PLAN P5.c），对单条 query 误差很大，而 est_out 误差会被输出单价（输入单价的 2.5~8×）放大，使 `max_cost_per_request` 门槛基本失效。
- **检索分量**：向量库取 top-K=16 相似历史 query，按余弦相似度加权平均其历史 (a, l)（式 4/5）；**消融显示检索分量贡献了 +7.6pp 能力准确率、去掉检索成本 +35.5%**。
- **融合**：`a = γ·a_pred + (1−γ)·a_retrieve`，γ/δ 可学习（实验取 0.5）。
- 训练数据：2.7k 样本即可训出有效预测器（MMLU 1000 / GPQA-Diamond 198 / Math-500 500 / GSM8K 1000），10 个模型，Llama-3.1-70B 做 judge 打正确性标签。

### 1.3 RouteLLM：偏好数据训练路由器的工程参照

RouteLLM（LMSYS，ICLR 2025，github.com/lm-sys/RouteLLM）从 Chatbot Arena 偏好对训练 4 种路由器（矩阵分解 / 相似度加权排序 / BERT 分类器 / 因果 LLM 路由器），strong/weak 双模型设定下 MT-Bench 省 ~85% 成本保 95% GPT-4 质量。对我们的可借鉴点主要是**工程形态**：`pip install routellm` 后提供 OpenAI 兼容 server，`model="router-mf-alpha-0.5"` 语法把权重编码进模型名——LLooM 的 `/v1/chat/completions`（`openai_compat.rs`）`model:"auto"` 语义可以借鉴这种"参数化 auto"（如 `auto@0.5` 表示 WTP 权衡点），把 RouterBench 的 `score = λ·P − cost`（其式：`performance_score_ij = λ·P_ij − cost_j`，λ = willingness to pay）直接暴露给代理客户端。

---

## 二、与 LLooM 现状的逐点映射

| RouterBench/OmniRouter 概念 | LLooM 现状 | 差距/机会 |
|---|---|---|
| per-query×model 能力标签 `a_ij` | **没有 query 级数据**。只有 (model, task_type) 粒度：`model_task_score.ewma_quality`（`db.rs:220`，EWMA α=0.15，sample≥5 才生效，`router.rs::quality_override`） | 影子路径 `run_shadow_pair`（`server.rs:2111` 附近）已在产 (query, routed_model, baseline_model, quality, cost) 四元组，但**只落 `routing_calibration` 不反哺预测器** |
| per-query×model 成本 `c_ij` | `price_specs` 真源 × `est_in/est_out`；est_out 仅 (task) 粒度 EWMA + 750 保守点（P5.c） | 输出长度预测是现成的模型训练目标（OmniRouter 头 2） |
| 查询嵌入 | **已有**：语义缓存 ChromaDB `ONNXMiniLM_L6_V2`（`api/embedding_model.py`，含镜像下载器）；RouterBench 最优 KNN 用的正是 MiniLM 家族（all-MiniLM-L12-v2） | **几乎零基础设施成本**即可做 query 级 KNN 检索 |
| 训练集 | `usage_records`（20 列：tokens/cost/latency/cached_tokens/reasoning_tokens/conversation_id/api_source…，`db.rs:69`）、`routing_decisions`（signals_json/candidates_json/outcome，`db.rs:231`）、`routing_calibration`、`conversations/messages`、`cache_feedback` 点赞点踩 | 数据分散但齐全，缺一个**导出+打标管线**把它们拼成 RouterBench 形态 |
| 预测器服务化 | Python 微服务（`ai_service.py`，端口 7862）只做 litellm 封装 | 加一个 `/v1/predict` 端点即可承载 ONNX 推理，不违反"Rust 决策 / Python 执行"分工 |
| 评测 | `aiq_replay.py` 三线重放 + AIQ（N2 落地）；`review.rs::grid_search_suggestions` 网格搜权重 | 补 Zero router 线、NDCH 插值、hold-out 切分 |

**关键架构洞察**：`plan()` 的 `PlanInput.quality_override: HashMap<String, f64>`（`router.rs:153` 附近）与 `hit_rate` 一样，本来就是"外部每 query 信号注入通道"。**学习型预测器的接入点已经存在**——把 `route()` 里 `quality_override` 的填充来源从"(model,task) EWMA"换成"预测器返回的 per-query a_ij"，`plan()` 一行不用改。这延续了项目"单一真源 / Rust 决策 / 影子先行"的三原则。

---

## 三、目标设计：三步走（V0 检索 → V1 训练 → V2 可选端到端）

### V0：query 级 KNN 检索评分（≈1 天，零训练，先吃检索红利）

OmniRouter 消融里检索分量贡献最大且**不需要训练**。用现成 MiniLM 嵌入做"历史相似 query 在该模型上的实测质量/成本加权平均"：

```text
对候选模型 m：
  a_retrieve(q, m) = Σ_{q'∈topK} sim(q,q')·q_quality(q',m) / Σ sim        # OmniRouter 式(4)
  l_retrieve(q, m) = Σ sim·q_out_tokens(q',m) / Σ sim                     # 式(5)，直接替换 est_out=750
  数据源 = routing_calibration（有质量）∪ usage_records（只有 token/cost，用于 l_retrieve）
  topK=16，sim 阈值 0.6 以下不计（避免噪声，OmniRouter K=16 消融最优）
```

落点：
1. `ai_service.py` 已有 SemanticCache 单例（ChromaDB collection 含 query embedding）。新增只读方法 `knn_history(embedding, k, model)` —— 或者更干净：影子样本落 `routing_calibration` 时**同时写一条带 embedding 的记录进独立 ChromaDB collection `router_history`**（元数据：model, quality, out_tokens, ts）。
2. `server.rs::route()` 在调 `plan()` 前查询（一次 ChromaDB 查询 ~ms 级），填充 `quality_override` 与 `est_out_tokens`。
3. **冷启动语义不变**：检索不到样本（<3 条相似）→ 回落现有 `model_task_score` EWMA + `task_avg_out_tokens`。旧路径永远是兜底，这符合项目"影子先行、不动门槛"惯例。

### V1：双头预测器（1~2 周，真正的"训练编排模型"）

**训练集构建**（新脚本 `scripts/export_router_dataset.py`，只读 SQLite/JSON，不改服务）：

```text
LLooM-Route 数据集行格式（对齐 RouterBench A.3）：
  sample_id     = request_id 或 calib_id
  model_name, provider
  prompt        = user_text（从 conversations/messages 回查，脱敏可选）
  performance   = 质量标签 ∈ {0,1} 或 [0,1]
  cost          = usage_records.cost（真源 priced_usage）
  out_tokens    = usage_records.output_tokens          # 训练头 2 的标签
  task_type, band, est_in_tokens                       # 辅助特征

三个标签来源（按可信度降序，可同时存在、来源列记录）：
  L1 结构化判定：任务本身可判对错——JSON schema 校验成功、代码可编译/测试通过、
     math 结果比对（P4.c `_quality_signal_ok` 的强化版）。零成本，随请求自动产。
  L2 人类反馈：cache_feedback 点赞/点踩（已有表）。
  L3 LLM judge：影子路径已双跑（routed × baseline），judge 打分。
     目前 run_shadow_pair 只给 routed/baseline 各一个质量标，扩展为 judge 独立打 routed 质量。
  L4 隐式信号（弱标签，单独列不混入）：outcome=success/fail、reask、escalation——
     与 P1.c EWMA 信号表同源，作为弱监督/验证集用。
```

**模型与训练**（新脚本 `scripts/train_router.py`，PyTorch CPU 可训，2.7k 样本即可起步——OmniRouter 规模）：

```text
编码器：paraphrase-MiniLM（与语义缓存同源，可共享下载基建；中文需换
        paraphrase-multilingual-MiniLM 或 bge-small-zh——embedding_model.py 已有镜像下载器可复用）
头 1  能力：sigmoid(W1·(E_q ⊙ E_m)) ，E_m = 模型描述文本嵌入（name + capability_tier +
        quality_score + context_window + 价格档序列化成自然语言——Router-R1 式"描述符条件化"，
        新模型零样本可预测）
头 2  长度桶：10 桶 softmax（桶边界按该模型历史 out_tokens 分位数划分，而非等宽）
损失  = BCE(头1) + λ_len·CE(头2)，λ_len=0.5
训练  70/30 hold-out（RouterBench 惯例）；早停按验证集路由命中率而非预测 MSE——
        路由命中率 = argmax_j(λ̂·a_ij − c_ij) 与 Oracle 的一致率
导出  ONNX → data/router_model.onnx（版本化文件名 + settings 记录 metrics）
```

**服务化**（`ai_service.py` 新端点，保持无状态）：

```python
POST /v1/predict   # Rust 在 signals 慢路径调用（预算 <100ms，超时静默回落）
req:  {"query": "...", "models": [{"name","desc"}...], "topk": 16}
resp: {"predictions": [{"model":"deepseek-v3","a":0.83,"len_bucket":3,"est_out":720,
                        "source":"model|retrieval|fallback"}...], "latency_ms": 12}
内部：a = γ·a_pred + (1−γ)·a_retrieve（γ 先固定 0.5，V1.1 再学）
      est_out = 桶中位数 × 该模型 avg_out_tokens 校准系数
```

**Rust 侧接入**（改动最小的路径）：

```rust
// router.rs::route() —— 在现有 quality_override 填充逻辑之后追加：
//   1. signals 慢路径调 ai_client::predict(query, &models)（新方法，复用 reqwest 客户端）
//   2. 成功 → quality_override.insert(name, a_ij)；est_out_tokens ← 预测值（覆盖 task 均值）
//   3. 失败/超时/未加载模型文件 → 完全跳过，旧逻辑原样（影子模式：settings routing.predictor=shadow 时只记日志不生效）
// plan() 本身零改动。
```

**影子评测（上线闸门）**：`settings routing.predictor` 三态：`off / shadow / live`。shadow 态下 plan() 照旧，但把"预测器本应做的选择"与"现行选择"各跑影子对比（复用 `maybe_shadow_sample` 双跑基建），累计 ≥N 样本且 AIQ 不劣于现行 + 成本节省显著 → 人工切 live（保持 N2 的"采纳前人工审查"原则）。

### V2（可选，不建议现在做）：端到端路由模型 / RL

RouteLLM 式端到端 router（直接输出模型名）与 Router-R1 式 RL（模型描述符条件化、可泛化到未见模型）**不推荐**作为当前落点，理由：
1. 破坏"Rust 单一决策真源"（P0.f 教训）——端到端模型把决策权搬进 Python/NN。
2. RouterBench 已证明裸预测路由只打平 Zero router；而 LLooM 的 `plan()` 本身已是一个带门槛/预算/粘性先验的强 Zero router 变体，学习模块应作为其**质量/成本估计器**而非替代者。
3. RL 需要千级带标签影子样本（台账 B10 的触发条件）——V0/V1 的数据管线正是 B10 的前置工程。

---

## 四、数据规模与采集规划

- 影子采样率默认 10%（`routing.shadow_ratio`）。若日均 chat+proxy 请求 R 条，带 L3 judge 标签的样本 ≈ 0.1·R/日。**千级样本（B10 触发线）≈ 10 天 × R=1000**。
- L1 结构化标签零成本、每请求可产；V0 检索层当天就有数据可用（`routing_calibration` 既有行 + usage_records 的 out_tokens 历史全量可回填 embedding——导出脚本含 `--backfill` 模式）。
- 质量标签的成本控制：judge 用便宜模型（qwen-plus 档），仅影子 10% 流量，judge 成本 ≈ 影子请求本身成本，已计入现有影子预算。

## 五、风险与边界

1. **标签噪声**：点赞点踩稀疏且有偏；LLM judge 对开放式问答不可靠（RouterBench 结论 3 的镜像）。→ 预测器只服务"结构化可判 + 影子判"两类，开放式任务的 quality_override 不注入预测值。
2. **分布漂移**：模型池变化（新增/停用）→ 预测器每月重训 + 注册表描述符条件化降低新模型冷启动误差；`needs_calibration` 保守期继续兜底。
3. **隐私**：训练集含用户 prompt → `export_router_dataset.py` 默认脱敏（PII 用 security.rs 同款正则离线过一遍），数据集文件不入 git（`data/` 已 ignore）。
4. **延迟预算**：`/v1/predict` 超时 100ms 硬切断（`routing.predictor_timeout_ms`），Rust 侧 `tokio::time::timeout` 包裹；路由 overhead 指标（`/api/routing/overhead`）自动覆盖回归。
5. **不重蹈双真源**：预测器只产出数值（a_ij / est_out），**决策仍 100% 在 Rust plan()**；Python 不持任何模型名偏好。

## 六、分阶段落地清单（供后续立项，本报告不实施）

| 序 | 内容 | 验收 | 预估 | 关联 |
|---|---|---|---|---|
| A1 | `export_router_dataset.py`（导出+脱敏+标签合并+--backfill） | 数据集行数、标签来源分布报告；与 usage_records 总数对账 | 1 天 | B8 |
| A2 | `router_history` ChromaDB collection + 影子路径双写 | 影子样本落库后 KNN 可查；miss 时回落路径单测 | 1 天 | — |
| A3 | route() 接检索评分（V0 live） | AIQ 不劣于现行；est_out 命中率提升（对账 act vs est 偏差收窄） | 1~2 天 | B9 |
| A4 | `train_router.py` + ONNX 导出 + `/v1/predict`（shadow） | hold-out 路由命中率报告；shadow 双跑 ≥N 样本 AIQ 对比 | 1~2 周 | B10 前置 |
| A5 | 人工审查后切 live + WebUI 预测器状态卡 | `/api/routing/overhead` 无回归；预测器可一键回退 off | 0.5 天 | — |
