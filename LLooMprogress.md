# LLooM v2 项目进度

> 最后更新：2026-08-17 · 仓库 `citrus-dot/LLooM` · 分支 `v2` · 工作目录 `/Users/orange/LLooMv2`

---

## 一、项目定位与架构

基于 LiteLLM 的智能 LLM 路由代理 / 管理工具，核心卖点：控制面/数据面分离 + 成本优化 + 安全可控。

**三大核心目标**：
1. 集约化管理模型及 token 用量 — 模型注册、用量追踪、成本计算、预算控制
2. 根据用量智能规划调用 — 两层分类路由、Fallback 容灾、成本感知选模型
3. 语义感知分配任务 — 复杂任务分解、语义缓存、域分类增强

**当前架构**（纯 Rust + Python 微服务，零 Docker）：

| 层 | 技术栈 | 端口 | 说明 |
|----|--------|------|------|
| Rust 主服务 | `lloom-server` (axum) | :7861 | REST + 静态托管 WebUI，拉起 AI 服务与 Ollama |
| Python AI 微服务 | `api/ai_service.py` (FastAPI + litellm) | :7862 | 无状态，只调 litellm，业务逻辑在 Rust 侧 |
| 本地模型 | Ollama | :11434 | 可选，简单问题走本地 |
| WebUI | `webui/` (React + AntD) | :7861 | 总览/用量/对话/模型/设置 |
| CLI / TUI | `lloom-cli` (Rust) / `tui/` (SolidJS + OpenTUI) | — | 共享同一 REST 契约 |

数据：`data/lloom.db`（SQLite）+ `data/conversations/*.json`。密钥：`.env`。

**已发布**：v2.0.0、v2.1.0（GitHub Release）。

---

## 二、关键技术决策

| 决策点 | 选择 | 原因 |
|--------|------|------|
| LLM 调用 | litellm SDK（PyPI 安装，非本地源码）| 去 Docker 代理，少依赖 |
| 数据库 | SQLite | 本地零配置 |
| 语义缓存 | ChromaDB（PersistentClient）| pip 安装即可 |
| GUI → 前端 | React WebUI + Rust 无头服务（v2.1 起）| 控制面/数据面分离 |
| API 框架 | FastAPI + axum | 原生 async + SSE |
| 分支策略 | v2 独立开发，旧版存 `legacy` 分支 | 保留历史 |

---

## 三、安全与健壮性（已修复）

- **SQL 注入**：`db.rs update_model` 列名白名单，非法 key 拒绝。
- **路径穿越**：`conversations.rs validate_id` 校验 `id ∈ [A-Za-z0-9_-]`。
- **密钥泄露**：`get_config` 对 `*_API_KEY/_KEY/_TOKEN/_SECRET` 脱敏为 `****+后4位`，设置页不预填。
- **优雅退出**：SIGINT/SIGTERM 信号处理 + `POST /api/shutdown`，子进程全清理，杜绝端口残留。

---

## 四、待办事项（TODO）

按优先级：`🔥` 高/安全，`⚡` 体验，`🔧` 优化。

> 已修复：B1（chat_stream token 统计）、B2（api_key_for 简化）、B3（NotFound 文案插值）；CLI/TUI 已同步真流式输出、模型标注、对话重命名、标题不覆盖。

- [ ] 🔧 **O2**：`main.rs` 绑定 `0.0.0.0`，建议改 `127.0.0.1`（本地工具；涉及局域网访问，需确认后再改）。
- [ ] ⚡ **O5 复杂判定调优**：多对象比较检测 + 多模型轮询分配已落地，判定边界/过度触发仍需真实语料打磨。
- [ ] ⚡ **O6 子任务并行**：无依赖子任务改并行（`ThreadPoolExecutor`/`asyncio.gather`）提速。
- [ ] 🔧 **多模型拆分**需配置「可用模型 + 有效 Key」才真正生效，否则走单任务兜底。
- [ ] 🔧 **思考过程深度展示（可选）**：当前轻量档（进度+模型标注+流式答案）；深度展示子任务中间输出/推理模型思考需扩展 `token` 事件。

---

## 五、重要问题记录（保留有长期价值的技术教训）

### 1. 语义缓存模型下载卡死
- **根因**：ChromaDB 的 `ONNXMiniLM_L6_V2` 默认从 **AWS S3**（`chroma-onnx-models.s3.amazonaws.com`）拉 ~79MB 模型，**不是** HuggingFace；`HF_ENDPOINT` 对 S3 下载是 **no-op**。受限网络下 S3 直连 ~6KB/s，卡死数小时。
- **解决**：`api/embedding_model.py` 自己预置 6 个文件（sha256 清单 + 多镜像 hf-mirror/modelscope/huggingface 自动选最快 + 断点续传 + 原子落盘到 ChromaDB 约定缓存目录）。冷启动 86.9MiB/13s。量化 int8 实测中文语义坍缩（无关相似度 0.516 ≫ 0.3），**保持 fp32**。

### 2. 缓存命中率自校准（为什么要问「灰区未命中」）
- 若只在命中时收集标签，样本全在阈值之上，Youden's J 会随阈值降低单调增大，把阈值压到 0.70 地板引发大量误命中。
- 因此在灰区未命中（sim 距阈值 ≤0.06）补问「与之前问过的相似吗？」，提供阈值下方负样本，使调优可上下收敛。硬约束 FPR≤1%、clamp 0.70–0.92。

### 3. 失败子任务防幻觉
- 子任务失败时若仍把「执行失败: …」喂给汇总模型，模型会编造「子任务X因API错误中断」甚至生成不存在的测试脚本。
- **解决**：`task_done` 带 `error` 字段；只要有失败子任务就直接拼接失败信息作答，**不再调用汇总模型**；`AGGREGATE_SYSTEM_PROMPT` 加硬约束禁编造。

---

## 六、关键约束（勿踩坑）

- **不可删** `data/`（真实数据）、`.env`（密钥）；可删可重建 `target/`、`build/`、`dist/`、`.venv/`、`node_modules/` 等。
- v2 与 Docker 完全解耦；旧 Docker 栈仅存 `legacy` 分支。
- **网络受限**：官方源（bun.sh/GitHub releases/huggingface/ChromaDB S3）常下载不动；统一镜像 —— npm/bun→npmmirror、pip→清华、Ollama→ghproxy.net、embedding→hf-mirror/modelscope。`HF_ENDPOINT` 对 ChromaDB S3 下载无效。
- `api/` 无 `__init__.py`，需 `pip install -e .` 才能被 `uvicorn api.ai_service:app` 导入。
- `.env.example` 的 `LLOOM_API_PORT=7860` 是旧残留，实际端口由 `LLOOM_WEB_PORT`（默认 7861）控制。
- 对话工具内 `nohup` 启动的进程跨工具调用会被回收，不能持久；持久运行用 `.command` 或系统服务。

---

## 七、开发环境

- Python：`.venv/`（3.13.12），`pip install -e ".[dev]"`（清华镜像）。
- Rust：`cargo build -p lloom-server`。
- WebUI：`cd webui && npm install && npm run build`（根 `.npmrc` 已固定 npmmirror）。
- TUI：`cd tui && bun install && bun run build`（需 `bun`，可走 npmmirror CDN 镜像）。
- 端口：服务器 :7861、AI 服务 :7862、Ollama :11434。
