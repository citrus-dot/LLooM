# LLooM v2 项目进度

> 最后更新：2026-10-19 · 分支 `v2` · 当前架构说明见 [`ARCHITECTURE.md`](ARCHITECTURE.md)。

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
- Linux release 主服务约 51 MB

## 当前质量基线

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run build --prefix webui
cd tui && npx tsc --noEmit
```

当前结果：Rust 180 项测试通过，WebUI 构建与 7 项前端契约测试通过，TUI 类型检查通过（2026-10-19，clippy `-D warnings` 干净）。

## 已验证 E2E

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

## 审计遗留待办（2026-10-19）

> 来源：全仓代码审计 [`docs/AUDIT-2026-10-19.md`](docs/AUDIT-2026-10-19.md)。审计查出 P0×4 / P1×15 / P2×18 / P3×14，合作者已于 v3.0.2（`3d28c6f..157fc2b`，11 commits）修复 P0 全部 + P1 的 13/15 + P2 约 12 项（验收记录见审计报告 §9 路线图与当日验收）。下表为**剩余项**，按真实需求触发，不提前堆叠。

| 编号 | 名称 | 内容 | 解锁条件 | 权威落点 |
|---|---|---|---|---|
| L-1 | WebUI 流不可取消 | 全 store 零 AbortController：SSE 流无法中断、页面切换不取消在途请求（selectConv 竞态已用 loadSeq 序号修复） | 长回答用户反馈需要「停止生成」按钮时 | `webui/src/store/chatStore.ts`、`webui/src/api.ts`；方案见审计报告 P1-14 |
| L-2 | review 重放 band 不提档 | 网格重放 `band_for(task_type, "")` 传空串导致复杂任务永不提档，建议基于失真 band 写回生产权重（est_in 已修为真实均值） | 采纳建议前发现建议系统性偏差时 | `crates/lloom-core/src/review.rs:146`；方案见审计报告 P2-2 |
| L-3 | Provider 错误枚举化 | 429/401 等压成 `AiService(String)` 一律 500，客户端无法按状态退避；`From<String>` 便利入口稀释错误语义 | 对外 `/v1` 用户报告错误分类不可用时 | `crates/lloom-core/src/error.rs:56`、`providers/*.rs`；方案见审计报告 P2-12 |
| L-4 | provider 协议枚举化 | `ModelSpec.provider` 仍是字符串，supports_native/chat/stream 三处白名单重复维护，与 `Backend` 枚举双轨 | 新增 provider（Gemini/Bedrock）时一并做 | `crates/lloom-core/src/providers/mod.rs:13-51`；方案见审计报告 P2-13 |
| L-5 | optimal_threshold 死代码 | 校准采集无消费方：函数零调用，threshold 仍走环境变量硬编码 | 决定接上校准闭环或清理死代码时 | `crates/lloom-core/src/db.rs:2469`；方案见审计报告 P2-15 |
| L-6 | 结算失败孤儿 reservation | `settle_api_key_budget` 三处失败被 `let _ =` 吞，失败残留 `api_key_reservations` 孤儿行占额度 | 出现额度对不上账的反馈时 | `crates/lloom-core/src/openai_compat.rs:437/468/687`；审计报告第五节 |
| L-7 | 后台清理 job | API key 过期请求行清理仍靠每次鉴权顺带 DELETE；RPM 已原子化，此项仅剩吞吐优化 | 高并发下出现写锁竞争时 | `crates/lloom-core/src/db.rs:1060` 附近；方案见审计报告 P1-3 |

后续方向（原有六项）保持不变，见下节。L 系列与「后续方向」互不阻塞；L-3/L-4 建议与 Provider 扩展同批走 OpenSpec。

## 后续方向

按真实需求触发，不提前堆叠：

1. Provider 扩展：Gemini、Bedrock、Azure 特殊协议
2. OpenAI tools / function calling / 多模态
3. 大规模向量缓存时从线性扫描升级 HNSW
4. 多租户和 PostgreSQL（仅在出现实际多用户部署时）
5. Agent / MCP 运行时
6. 离线 Batch 通道

## 文档规则

- 当前架构：`ARCHITECTURE.md`
- 用户使用：`README.md` / `README-ZH.md`
- 测试：`TEST-GUIDE.md`
- `docs/archive/`：历史设计快照，不代表当前运行架构

旧计划中关于 Python、LiteLLM、ChromaDB、端口 7862 和双进程调用链的描述仅用于保留迁移历史。
