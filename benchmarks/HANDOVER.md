# LLooM Bench 交接总入口（HANDOVER）

> 生成：2026-10-12。目的：合作者（SakiMidare）完整接手 bench 工作。
> 本文件是唯一入口；读完按 §四 的顺序操作即可完整接管。

---

## 一、上传状态（先回答「有什么在 GitHub 上」）

**全部 bench 代码与数据产物已在 `origin/v2`**（HEAD = `138a6d6`），四个目录：

| 目录 | 内容 | 体量 |
|---|---|---|
| `benchmarks/routerbench/` | v0.2 冻结报告/manifest/scripts/venv 说明；normalized/ 与 raw/ 为 **symlink**（见 §三） | 代码+报告 |
| `benchmarks/systembench/` | v0.1 冻结 + S12 全部（cases/annotations/matrices/reports/scripts/manifests/evaluator） | 代码+数据 |
| `benchmarks/orchestration/` | offline 基础层全链 | 代码 |
| `benchmarks/showcase/` | 证据审计与展示包（README 入口/三图/claims/audit/manifest） | 文档+图 |

**决策与过程文档（00-35 号，40 份）原本在本地 `research/`（gitignore）**，现已打包进
**`docs/handover/`**（本次 commit）——这是合作者在 GitHub 上唯一看不到的部分，现已补齐。

## 二、5 分钟上手路径

```bash
git clone git@github.com:citrus-dot/LLooM.git && cd LLooM
git checkout v2

# 1) 验证全部数字（31 项离线校验，纯 stdlib 零 API）
python3 benchmarks/showcase/audit/replay_check.py
#    预期：检查 31 项：PASS 31 / FAIL 0

# 或使用统一入口（会先检查环境，再执行同一份 replay）
make -f benchmarks/Makefile replay

# 2) 读三份入口文档（顺序固定）
open benchmarks/showcase/README.md        # 展示入口：Evidence Tier A-D + 快速复现
open docs/handover/35-LLooM-Bench成果总结与验收指南.md   # 成果与八步验收
open docs/handover/34-LLooM-Bench工作总结核验、成果解读与后续行动.md   # 外部核验视角（含 P0/P1/P2 闭环记录）

# 3) Rust 质量门
cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
#    预期：0 warning；138 passed
```

## 三、环境补充（GitHub 上没有、必须知道的）

### 3.0 评测轨道边界（机器可读真源）

`benchmarks/benchmark_contract.json` 和 `benchmarks/REPRODUCE.md` 将当前 bench
明确分成三条轨道：Replay（零 API、冻结结果复核）、Live（新 provider 采集）和
E2E（真实 workflow/artifact/tool/墙钟执行）。当前只有 Replay 可在普通 clone 后
无条件运行；E2E 尚未实现。不要把 SystemBench 的 frozen outcome 或 simulated
makespan 写成线上 E2E 结论。

环境诊断：

```bash
python3 benchmarks/tools/doctor.py                 # replay 环境，外置盘缺失仅 warning
python3 benchmarks/tools/doctor.py --run-replay    # 诊断 + 31 项校验
python3 benchmarks/tools/doctor.py --mode live --strict
```

### 3.1 大文件外置盘（**symlink 依赖**）

`benchmarks/routerbench/{raw,bench.db}` 是整目录 symlink、`normalized/` 内大 jsonl 是文件级
symlink，实体在 **`/Volumes/Files/LLooM-bench/`**（APFS 外置盘，routerbench 8.6G +
orchestration-raw 677M）。合作者接手的两条路：

- **拿到本机外置盘**：挂载到 `/Volumes/Files` 即一切恢复（盘未挂载时 bench 命令明确报错，无数据损坏）。
- **拿不到外置盘**：确定性重建——
  - normalized：raw 数据 + `benchmarks/routerbench/scripts/import_frozen.py` / `import_llmrouterbench.py`
    （原始下载源与 sha256 见 12/15 号文档 §数据层）
  - bench.db：replay 命令重建（历史 run 记录会丢；重要 run 的 summary 在 reports/ 有存档）
  - 重建后跑 `replay_check.py`，31/31 PASS 即等价。

注意：**Showcase 与 SystemBench v0.1 的展示数字不需要外置盘**——它们只依赖仓内
冻结报告（jsonl/json 已入库），仅重跑 replay/新采集才需要。

### 3.2 API key 与额度现实

- key 在本地 `.env`（gitignore，不入仓）。S12.6 的采集用 DashScope：**qwen-plus/turbo/
  3.6-plus/3.8-flash/3.7-max/3.6-flash/3.6-max-preview 免费额度全部耗尽（403 Arrearage）**，
  当前仅 `qwen3.7-plus` 可用。接手后如需新采集（仅触发器驱动，见 §五），先充值或等免费刷新。
- 判断模型可用性先探测再批量（403 有间歇限流形态，重试能过；`Free quota exhausted` 是硬墙）。
- S12 采集脚本的硬编码依赖：DashScope compatible-mode 端点 + `enable_thinking: false`
  （qwen3.6 系默认开 thinking 会导致截断——详见 routing_input_contract_v1.yaml 文件头）。

### 3.3 Python venv

`benchmarks/routerbench/.venv`（numpy/requests，78M）**在仓内但通常不随 clone 传播**
（部分平台会因 symlink/权限失效）。重建：

```bash
cd benchmarks/routerbench && python3 -m venv .venv
.venv/bin/pip install numpy requests
# 或 uv: uv venv benchmarks/routerbench/.venv && uv pip install --python benchmarks/routerbench/.venv/bin/python numpy requests
```

离线复核（replay_check.py）不需要任何 venv——纯 stdlib。

### 3.4 matplotlib（仅重渲染三图时需要）

本机用 `/tmp/figvenv`（uv + 清华镜像装的 matplotlib 3.11.2）；合作者环境任意 matplotlib ≥3.8 即可。
三图平时不需要重渲染（PNG 已入库），只有改 figure_data CSV 后才重画。

## 四、接手后必读文档（`docs/handover/`，按序）

| 序 | 文档 | 读什么 |
|---|---|---|
| 1 | `35-…成果总结与验收指南.md` | 全景成果 + 八步验收（每条命令实测过） |
| 2 | `34-…核验与后续行动.md` | 外部核验视角：P2 勘误（0.5863/$0.000821 = `lloom@softgate0`）、P0/P1/P2 闭环 |
| 3 | `33-…全景总结.md` | 三 track 过程全景（含勘误块与口径区分注） |
| 4 | `31/32 号` | 封板决策与停止线（maintenance 规则、§11.2 重开触发器） |
| 5 | `29/30 号` | S12 终审逻辑（为什么 GO 降级 CONDITIONAL、双 profile 定案）与额度中断实录 |
| 6 | `00-总览索引.md` | 00-35 号全族导航（深入某 track 细节时用） |

**协议红线（32 号 §0.2，接手即生效）**：不充值扩数据；L1 198 cells 不做 pooled headline；
L2 partial 不进 headline；Best Single 支配 P2 必须同图呈现；不宣布正式 v0.2 release；
不改 router 内核（生产 thinking/max_tokens 问题 = 独立 issue P-01）。

## 五、当前状态与「什么情况下才能动手」

**状态：maintenance**。唯一主动待办：production issue P-01（等 Nironta 确认）。

重开实验**只允许**由 32 号 §11.2 触发器驱动（评审质疑/预注册 RQ/生产 bug/artifact RQ/live
需求五类），每个新实验先写：RQ + 预算上限 + 同质性协议 + 停止条件。任何「把表格填满」式
的扩展都是违规。

## 六、验收（接手完成的定义）

- [ ] `python3 benchmarks/tools/doctor.py --run-replay` 通过；外置盘缺失只产生预期 warning
- [ ] `replay_check.py` 31/31 PASS（含外置盘不挂载的场景——证明展示层零外置依赖）
- [ ] Rust 质量门全绿（clippy 0 + 138 tests）
- [ ] 读完 35/34/32 号；能复述「Best Single 支配 P2」与「双 profile」两个口径
- [ ] 明确本机 `/Volumes/Files` 缺失时的重建路径（§3.1）
- [ ] 知道哪些事**不做**（§四 红线）
