# LLooM v2 项目进度

> 最后更新：2026-10-04 · 分支 `v2` · 当前架构说明见 [`ARCHITECTURE.md`](ARCHITECTURE.md)。

## 当前状态

LLooM 已完成从 Rust + Python 微服务到**纯 Rust 数据面**的迁移。

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

当前结果：Rust 130 项测试通过，WebUI 构建通过，TUI 类型检查通过。

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
