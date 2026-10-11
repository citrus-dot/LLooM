# Showcase 审计检查单（Phase 0-3 完成记录）

> 32 号 §九 Phase 0-3 各完成标准的核对。全部离线，`api_calls_during_render = 0`。

## Phase 0 · 冻结来源与清点 ✅

- [x] source commit `42216b7c4129e5bf322bfb2efc2e764387e8e796`（branch v2，工作树仅会话文件）
- [x] 13 个权威输入文件 SHA-256 → `manifests/source_inventory.json`（无 missing）
- [x] v0.1 旧报告（Core-1000 `bench_report.json`）与 v0.2（12,345 `v02_test_report.json`）明确区分，旧表未入图
- [x] `systembench/README.md` 陈旧状态（「待办 Phase B」）已更新为 frozen + snapshot 现状
- [x] 分母记录：RouterBench test 12,345；SystemBench v0.1 80/65/327；L1 33 节点×6=198；L2 180 计划
- [x] 外部参照（RouteLLM/TwinRouterBench/τ²-bench/AgentRouter）仅方法参照，不入 LLooM 图表

## Phase 1 · 离线审计与口径修正 ✅

- [x] `audit/semantic_l1_comparability.csv`：198 行 × 26 字段（32 号 §7.1 全清单；actual_model_id 含 qwen3.6/3.7 代际、judge 分档、rubric hash、truncated/score 状态、included_in_headline）
- [x] `audit/l1_stratified_status.json`：per-profile / per-generation / per-judge / per-split 11 个视图（198 全有输出、197 scored、178 truncated、1 invalid_json）
- [x] 16/32 winner agreement 标注为 **descriptive**（身份审计后未构造完全同质 paired 子集）
- [x] `audit/l2_coverage.json`：119/180（66.1%）输出、49/119（41.2%）已评、49/180（27.2%）占计划；非 MCAR 声明；`PARTIAL / NOT FOR HEADLINE`
- [x] B2/B3/B4 指标名与分母核对；simulated makespan 与 cost/completion 分开呈现
- [x] `audit/coordination_ratio_derivation.json`：旧字段 18.347 → 派生 19.347× / +1834.7%（median 5.11 右偏注记），不覆盖 frozen 报告
- [x] 图表生成零 provider 调用（matplotlib Agg 本地渲染）

## Phase 2 · Showcase 制作 ✅

- [x] `showcase_report.md`（8 节主叙事）+ `claims_and_limits.md`（C1-C7 可引用 + 7 条禁用）
- [x] 图 1 `routerbench_quality_cost.png`：六策略全含（Best Single 显眼 + 支配箭头注释 + Oracle 标 not deployable）
- [x] 图 2 `systembench_composition.png`：A 块 cost/completion、B 块 simulated makespan 分离
- [x] 图 3 `semantic_protocol_sensitivity.png`：probe（受控）与 L1（exploratory）并列，不做 leaderboard
- [x] `audit/demo_case_sb_v02_sem_030.json`：6 节点 root_goal→role→routing/execution hash→R1 band→frozen artifact 边界标注（semantic gold 未人审，如实标注）
- [x] 所有主图分母/协议/证据等级写入图注与 claims 表

## Phase 3 · 公开前一致性检查 ✅

- [x] README/主报告/claims 引用相同结果文件与版本（showcase_manifest.json v1）
- [x] 未从 v0.1 旧表抄 v0.2 数字；无「v0.2 release」表述（`showcase_snapshot_not_formal_v02_release`）
- [x] 离线再生校验 **30/30 PASS**（`audit/offline_replay_check.json`：CSV↔冻结报告逐项、audit 一致性、manifest figure hash 自洽）
- [x] 「Limitations & Next Gate」在 showcase_report §六-§八（L2 partial / 无 live E2E / Best Single 限制）
- [x] 外部参考全部带链接与访问日期（32 号 §十四）

## Showcase Stop Gate（32 号 §11.1）对照

全部满足 → **当前 benchmark 工作转 maintenance**。重开实验仅由 §11.2 表中触发器驱动。

## P0-2 勘误记录（2026-10-10，34 号核验）

- **P2 Frozen 数字归属修正**（34 号 §2.4）：showcase 图 1/CSV/报告/claims 与 33 号总结原把 `lloom` run（0.5867/$0.000849，实为 P0 Current/BandOnly）标为 P2 Frozen。真源：`v02_test_report.md`「P2 Frozen ⭐ = `lloom@softgate0` = 0.5863/$0.000821」（frozen manifest policy=`P2_softgate0`；cost-matched random $0.000819 与之匹配为佐证）。已修正：figure CSV、图 1 重渲染、showcase_report §2、claims C1、33 号 §2.2（含勘误块）。
- **calibration/test 口径区分**（34 号 §2.4）：33 号 §2.3 加注——calibration weak precision 0.809（Day 9 tournament）≠ frozen test 0.786（CI [0.7629, 0.8086]）。
- **校验与 manifest**：offline_replay_check 30/30 PASS（fig1.lloom 对照源改为 `lloom@softgate0`）；showcase_manifest 升 v2（report/figure_data version 2，erratum 记录）。
