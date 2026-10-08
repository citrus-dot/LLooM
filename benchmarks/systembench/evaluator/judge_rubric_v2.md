# Judge Rubric v2（frozen）— SystemBench Semantic Node 质量评分

- 版本：v2，冻结于 2026-10-08。变更：judge 模型 qwen-plus → qwen-turbo
  （v1 冻结当日 qwen-plus 免费额度耗尽 403 Arrearage，额度约束下换档；
  rubric 正文/锚点/附加规则与 v1 逐字一致，仅 judge 模型与版本号变化）。
- rubric_id: sb_v02_judge_rubric_v2
- judge 模型：qwen-turbo（temperature=0，不在采集池内，避免自评偏差）
- 输出协议：严格 JSON `{"score": <float>, "reason": "<一句话依据>"}`，score 0.0-1.0，0.1 步进
- 无效输出处理：JSON 不可解析 / score 越界 → 该 cell score=null、judge_status=invalid（不重试不改分）

## 评分锚点

| score | 判定 |
|-------|------|
| 1.0 | 完全满足任务要求：正确、完整、无编造；若任务依赖前置任务结果，与所给结果一致 |
| 0.5 | 部分完成：核心要求有明显缺失或错误，但方向正确、有实质内容 |
| 0.0 | 未完成：空、拒绝、离题、与任务要求无关，或编造关键内容 |
| 中间分 | 按完成度线性内插（0.1 步进） |

## judge 输入格式（每 cell 一次调用）

```text
{rubric 全文（本文件 §评分锚点 + 附加规则）}

## 任务要求
{execution_input.final_text —— 含前置任务结果（若有）}

## 助手输出
{model_output}
```

## 附加规则

1. 任务要求中的「前置任务结果」是已冻结的事实输入：输出与其**矛盾**视为错误（扣分）；
   与其**一致并在此基础上推进**视为合格。
2. 输出中出现任务未提供的数据引用（编造）→ 至多 0.3。
3. 格式要求（列表/表格/字数）是任务的一部分：轻微偏离 ≤0.1，严重偏离 ≤0.3。
4. score 与 reason 必须对应；reason 一句话，不展开。
