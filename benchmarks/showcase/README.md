# LLooM Benchmark Showcase — Research Snapshot（2026-10-09）

> **`showcase_snapshot` — 非正式 SystemBench v0.2 release。**
> 本目录是 32 号决策（`research/手记规划-20260928/32-...md`）的执行交付：
> 现有证据的审计、封板与展示包。RouterBench v0.2 与 SystemBench v0.1 保持冻结；
> semantic L1/L2 数据为 exploratory snapshot。

## 证据层级速览

| Tier | 内容 | 用途 |
|---|---|---|
| **A · 冻结主证据** | RouterBench v0.2（n=12,345）；SystemBench v0.1（80 cases / test 65 / 327 nodes） | 主报告、核心图 |
| **B · 诊断证据（exploratory）** | S11 atomicity（17.5%）；S12.5.4 protocol probe（REVIEW）；L1 matrix 分层视图 | 诊断发现章节，显著标注 |
| **C · partial 存档** | S12.6-B L2（119/180 输出、49 已评） | 仅 coverage table，不进 headline |
| **D · 未测** | 真实 artifact 传播 E2E、live 工具延迟/花费 | 仅 future work |

## 快速复现（全部离线，零 API）

```bash
# 1) 离线复核：figure_data CSV ↔ 冻结报告逐项核对 + audit 自洽 + manifest hash（31 项）
python3 benchmarks/showcase/audit/replay_check.py
# 2) 来源快照：source commit 与 13 个输入文件 SHA-256 见 manifests/source_inventory.json
#    （source_commit 42216b7…；勘误 commit 18a5e35 / f4b5eea，冻结输入未变）
# 3) 图已提交在 figures/；figure_data/*.csv 为绘图数据真源（渲染脚本零 provider 调用）
```

**Snapshot identity**：RouterBench v0.2 = `lloom-routerbench-v0.2`；
SystemBench v0.1 = `lloom-systembench-v0.1`；
showcase = `lloom-benchmark-showcase-2026-10-09`（status: `showcase_snapshot_not_formal_v02_release`）。

## 目录

- `showcase_report.md` — 主叙事（三层证据 + 边界）
- `claims_and_limits.md` — 可引用 claims / 禁止越界 claims
- `audit/walkthrough.md` — 贯穿式 demo（每步标注生产/回放/仿真身份）
- `figures/` — 三张核心图（PNG）
- `figure_data/` — 三图底层 CSV（图由 CSV 再生；v0.1 makespan mean 与 median/p90 分列）
- `manifests/source_inventory.json` — 源 commit + 13 个输入文件 SHA-256
- `audit/` — 逐 cell 可比性审计、分层状态、L2 coverage、Coordination 派生值、`replay_check.py`（离线复核脚本）

## 一句话结论

LLooM 已建立两层可复现的评测证据：**RouterBench v0.2** 冻结测试集上 P2 相对
Always-Strong 节省 13.46% 成本（CI [12.20, 14.73]）、质量保留 1.0005（CI 含 1.0），
并显著优于两种匹配随机基线；**SystemBench v0.1** 在固定 workflow + 冻结 outcome 下
将 routing 效果（cost −14.95%，CI [−26.59, −5.16]）与并行调度的模拟 makespan 效果
分离。**Best Single 在点估计上仍支配 P2**——这是当前最诚实的主结果边界。语义层
（S12）处于 research snapshot 阶段：output budget 会改变模型胜出（protocol
sensitivity），L2 artifact-conditioned 管线已打通但数据 partial。
