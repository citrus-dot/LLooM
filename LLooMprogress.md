# LLooM v2 项目进度

> 最后更新：2026-10-27 · 分支 `v2` · 当前架构说明见 [`ARCHITECTURE.md`](ARCHITECTURE.md)。
> 最新发布版本：**v3.0.2**。工作区另含已复核但尚未发布的功能修复；本文件描述工作区现状，不等同于已发布包的能力清单。
> 文档入口：[README-ZH.md](README-ZH.md)（用户）/ [CONTRIBUTING.md](CONTRIBUTING.md)（开发）/ [TEST-GUIDE.md](TEST-GUIDE.md)（验收）/ [SECURITY.md](SECURITY.md)（安全）。

## 当前状态

LLooM 已完成从 Rust + Python 微服务到**纯 Rust 数据面**的迁移。

**Benchmark 体系（2026-10-05 ～ 10-09）**：三条 track 建成并封板——RouterBench v0.2 冻结（test 12,345：P2 Frozen 省 13.46% 成本 CI[12.20,14.73]、胜过两种匹配随机、Best Single 点估计支配如实入档）；SystemBench v0.1 冻结（frozen composition：routing cost −14.95% CI、simulated makespan 2184.6→500ms）；OrchestrationBench v0.1 offline 基础层；S12 semantic research snapshot（L1 198 cells exploratory 分层、L2 partial）。证据审计与展示包：[`benchmarks/showcase/`](benchmarks/showcase/README.md)（Evidence Tier A-D、claims/limits、三图离线可再生）。**当前处于 maintenance**；重开实验仅由具体研究问题触发（32 号 §11.2 触发器表）。

| 层 | 技术 | 状态 |
|---|---|---|
| 主服务 | Rust / axum / tokio | ✅ |
| 数据库 | SQLite WAL | ✅ |
| Provider | OpenAI-compatible + Anthropic 原生 Rust | ✅ |
| 路由与编排 | Rust 单一决策真源 | ✅ |
| 精确缓存 | SQLite | ✅ |
| 语义缓存 | FastEmbed + ONNX + SQLite 向量 | ✅ |
| WebUI | React + Ant Design | ✅ |
| CLI | Rust | ✅ |
| TUI | OpenTUI + SolidJS | ✅ |

不再依赖 Python、FastAPI、LiteLLM、ChromaDB、uv 或 PyInstaller。

## 已完成里程碑

- 模型注册、健康状态、成本和预算管理
- 规则 + LLM 两层分类与成本/质量评分路由
- fallback、熔断、健康探针和预算档联动
- Rust 任务分解、依赖波次、子任务执行与聚合
- SQLite 对话、上下文裁剪与滚动摘要
- L1 精确缓存和 L2 FastEmbed 语义缓存
- OpenAI-compatible 对外代理
- 多 API Key 管理：哈希存储、状态、额度、RPM、模型白名单、有效期
- 路由影子评测、AIQ 报告和权重建议
- Prometheus 指标与账单对账入口
- WebUI 独立 API Keys 页面和精简设置页
- CLI/TUI 契约同步；TUI 终端布局重整
- Linux release 主服务约 51 MB（历史发布记录；当前包体积以对应 Release 资产为准）

## 当前质量基线

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run build --prefix webui
cd tui && npx tsc --noEmit
```

当前结果：Rust 185 项 core + 1 项 server 测试通过，WebUI 构建与 7 项前端契约测试通过，TUI 类型检查通过（2026-10-27，clippy `-D warnings` 干净）。

## 已验证 E2E

以下为此前人工验收记录，不代表本轮已重新执行；本轮完成的是文档整理与自动化质量门复核（见上节）。

- Ollama 原生聊天
- 精确缓存二次命中
- 语义改写命中 FastEmbed 缓存
- FastEmbed 量化模型首次下载与持久化
- 复杂任务编排完整事件链
- API Key 创建、只显示一次、Bearer 鉴权、禁用
- API Key 模型白名单、成本回填和 last-used 更新
- 空 Provider Key 不进入生产路由

## 重要路径

| 内容 | 路径 |
|---|---|
| 主数据库 | `data/lloom.db` |
| 精确缓存 | `data/cache_exact.sqlite3` |
| 语义缓存 | `data/cache_semantic.sqlite3` |
| FastEmbed 模型 | `data/models/fastembed` |
| Rust 核心 | `crates/lloom-core/src` |
| WebUI | `webui/src` |
| TUI | `tui/src` |

## 审计遗留待办（2026-10-19 入账 · 2026-10-24 全部清账）

> 来源：全仓代码审计 [`docs/AUDIT-2026-10-19.md`](docs/AUDIT-2026-10-19.md)。审计查出 P0×4 / P1×15 / P2×18 / P3×14，合作者已于 v3.0.2（`3d28c6f..157fc2b`，11 commits）修复 P0 全部 + P1 的 13/15 + P2 约 12 项。剩余 L-1～L-7 已于 **2026-10-24 全部完成**（同批含 P2/P3 尾巴与「供应商默认 API Base」缺口修复，验收见当日改动清单），此表留档作闭环记录。

| 编号 | 名称 | 状态 | 落点（2026-10-24） |
|---|---|---|---|
| L-1 | WebUI 流不可取消 | ✅ 已修 | `chatStore.ts` AbortController + `stopGenerate()`；ChatPage「停止」按钮；token 80ms 节流 |
| L-2 | review 重放 band 不提档 | ✅ 已修 | `db.modal_routing_band`（30 天众数）替代空串 band_for |
| L-3 | Provider 错误枚举化 | ✅ 已修 | `error.rs::ProviderError`（429→429/400→400/401/403/5xx→502/超时→504）；两 adapter 分类器 + `/v1` 出口 |
| L-4 | provider 协议枚举化 | ✅ 已修 | `providers::Protocol` 单一映射（of/default_base），supports_native/chat/stream 全部委托 |
| L-5 | optimal_threshold 死代码 | ✅ 已删除 | 校准死链整条移除（采集/读取/函数/建表），存量库旧表保留不动 |
| L-6 | 结算失败孤儿 reservation | ✅ 已修 | 三处 `let _ =` 改 warn 日志；24h 孤儿由后台 job 回收（合作者已建 loop） |
| L-7 | 后台清理 job | ✅ 已修 | `api_key_request_cleanup` 循环（60s RPM 行 + 24h 预留回收），鉴权路径不再内联 DELETE |

**2026-10-24 同批收口**（审计 P2/P3 尾巴 + 新发现）：

- 云端模型 API Base 留空 → 供应商默认（`Protocol::default_base`；此前 WebUI 承诺「留空走默认」实际全不生效——新发现的真实缺口）
- L1 指纹改全文 sha256（80 字符截断撞指纹 → 跨会话串答案风险）
- `enhance_with_domain` 移到 plan 之前（领域改写参与策略/候选池，消除口径分裂）
- orchestrate 客户端 history 上限 200 条；`insert_usage` api_source 白名单断言
- 编排分解重复 id 去重；`fallback_depth` clamp(1,8)；`LLOOM_CACHE_MAX_ENTRIES` OnceLock 化
- webui：消息 key 用 seq；PricingPage「仅看过期」闭包修复；总览/用量页轮询 visibilitychange 门控

质量基线（2026-10-24）：clippy `-D warnings` 干净；Rust **186** 测试全绿；webui build + 7 契约测试；tui tsc 通过。

后续方向（原有六项）保持不变，见下节。

## 后续方向

按真实需求触发，不提前堆叠。以下均为规划而非已实现能力：

1. Provider 扩展：Gemini、Bedrock、Azure 特殊协议
2. OpenAI tools / function calling / 多模态
3. 大规模向量缓存时从线性扫描升级 HNSW
4. 多租户和 PostgreSQL（仅在出现实际多用户部署时）
5. Agent / MCP 运行时
6. 离线 Batch 通道

文档核查另记录实现级小项（不阻塞用户使用）：`/api/health` 的 version 仍硬编码 `2.0.0`，应与 package version 统一；`LLOOM_PROXY_TOKEN` 的无效配置示例已从 `.env.example` 与启动注释清理，鉴权统一走 WebUI API Keys。L1 容量淘汰当前为 FIFO，若需要 LRU 再单独立项。

## 文档规则

- 用户入口：`README.md` / `README-ZH.md`。
- 当前架构与 API 契约：`ARCHITECTURE.md`；SSE 细节：`docs/SSE-CONTRACT.md`。
- 开发与发布：`CONTRIBUTING.md`；手工验收：`TEST-GUIDE.md`。
- 安全策略与私密漏洞报告：`SECURITY.md`；许可证：`LICENSE`。
- 当前进度与后续方向：本文件。审计历史：[`docs/AUDIT-FIXES-2026-10-23.md`](docs/AUDIT-FIXES-2026-10-23.md)。
- 研究/Benchmark 交接：[`benchmarks/HANDOVER.md`](benchmarks/HANDOVER.md) 与 [`docs/handover/00-总览与手记索引.md`](docs/handover/00-总览与手记索引.md)，不是用户快速上手材料。
- `docs/archive/`：历史设计快照，不代表当前运行架构。

旧计划中关于 Python、LiteLLM、ChromaDB、端口 7862 和双进程调用链的描述仅用于保留迁移历史。
