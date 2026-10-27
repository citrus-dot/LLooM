# 参与开发

感谢改进 LLooM。本文是开发与贡献的唯一操作入口；架构与 API 契约见 [ARCHITECTURE.md](ARCHITECTURE.md)，手工验收见 [TEST-GUIDE.md](TEST-GUIDE.md)。当前开发分支为 `v2`，不将本地未提交工作区描述为已发布版本。

## 1. 环境

| 工具 | 用途 |
|---|---|
| stable Rust / Cargo | workspace 构建与测试 |
| Node.js 20+ / npm | WebUI 依赖与构建 |
| Bun（可选） | TUI 与 `webui` 的 Bun 测试 |
| Ollama（可选） | 本地模型端到端验证 |

```bash
git clone -b v2 https://github.com/citrus-dot/LLooM.git
cd LLooM
cp .env.example .env
npm --prefix webui ci
npm --prefix webui run build
cargo run -p lloom-server
```

打开 <http://localhost:7861>，在模型页配置模型。没有 Key 也能启动服务，但实际调用需要已配置的模型。`cargo build --workspace` 生成 server 与 CLI；修改 WebUI 后必须重新构建其静态文件，后端只托管 `webui/dist`。

TUI 在另一个终端运行：

```bash
cd tui
bun install
bun run dev
```

完整 release 构建可运行 `bash scripts/build.sh`。Intel Mac 的 L2 另需 `brew install onnxruntime`，缺失时仅 L2 降级。

## 2. 修改前：先确定边界

1. **小修复**（一行修正、文案、注释）可直接修改。
2. **新功能、架构调整、协议/API 变更、多文件或高风险修改**，先写清问题、影响范围、兼容性与验收方式；按项目约定走 OpenSpec 评审后再实现。
3. 修改前先找权威来源，不复制一份新的规则或数据流：架构/API → `ARCHITECTURE.md`；待办 → `LLooMprogress.md`「后续方向」；流式协议 → `docs/SSE-CONTRACT.md`。
4. `docs/archive/` 只读，不能当作当前实现；研究/Benchmark 材料不应被写成正式产品能力。

## 3. 工程约定

- Rust 风格与类型边界：`rustfmt`；以枚举表达协议/后端，跨层使用 From/TryFrom，DTO/边界先校验。
- 生产路径避免无意义 `unwrap()`；锁中毒、数据库失败与降级路径必须可诊断，不静默吞错。
- 错误分类保留语义：Provider 错误使用 `ProviderError`，HTTP/CLI/前端按实际状态处理，不能一律 500。
- SQL 参数绑定；Schema 修改必须有兼容旧库的幂等迁移与相应测试。
- 日志使用统一 logging facade，敏感 Key、完整鉴权信息与 PII 原文不得写入日志。
- 前端流式处理必须支持取消、UTF-8 分片、未命名错误帧与结束帧；更新 WebUI 后运行构建与测试。
- 遵守项目 `AGENTS.md` 的协作与目录边界（若你的工作区包含该本地文件）。

**不要提交：**`.env`、`data/`、真实对话/Key/账单、构建产物、本地研究材料、`target/`、`webui/node_modules/`、`tui/node_modules/`。修改 Schema 时先考虑已有用户的 `data/`，不能通过要求删库解决迁移问题。

## 4. 验证要求

提交前在仓库根目录运行：

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm --prefix webui ci
npm --prefix webui run build
npm --prefix webui test
cd tui && npx tsc --noEmit
git diff --check
```

若只改了某一层，也至少运行该层的检查；涉及共享契约、路由、缓存、计费、鉴权时运行完整集合。`webui` 测试需要 Bun；`npm --prefix webui test` 调用 `bun test tests`。

当前工作区验证基线（2026-10-27）：Rust 185 项 core 测试 + 1 项 server 测试通过；WebUI 7 项契约测试通过；TUI 类型检查通过。数量是当日快照，不作为长期硬编码目标。

**代码完成不等于验证完成。**提交说明中区分：

- 自动化测试已执行及结果；
- 真实 Provider/Ollama 手工验证及范围（可能产生费用）；
- 未验证的部分与已知边界；
- 数据迁移、配置、协议或破坏性变更。

修复应优先附带能复现原问题的回归测试，而不是只描述修复意图。缓存/流式/并发/鉴权相关改动需要真实数据流或边界测试，不能只测 helper 的静态返回值。

## 5. 提交与评审

使用 Conventional Commits，例如：

```text
fix(cache): isolate semantic entries by system prompt
feat(proxy): preserve upstream rate-limit status
docs(readme): clarify local-only management boundary
```

提交信息写清机制与影响位置，不用「优化代码」「修复若干问题」代替。PR 描述包含问题与价值、变更内容、兼容性、自动化测试、手工验证、未测边界；UI 变化附截图或录屏。合作者更新 WebUI/TUI 后， reviewers 应重新构建再验收。

未经明确要求，不在用户仓库中代为推送、发 Release 或改写他人未提交改动。

## 6. 发布流程（维护者）

1. 确认目标提交包含全部拟发布修复，四道质量门通过。
2. 同步三个 crate 与 `webui/package.json` 的版本号及 `Cargo.lock`；发布版本由 git tag 决定，安装包名来自 `git describe`。
3. 更新进度与必要文档，检查 README 的「最新发布版本」没有误写。
4. 提交发布准备 commit，创建 annotated tag `vX.Y.Z`，按维护者授权推送分支与 tag。
5. `release.yml` 构建 macOS arm64/x86_64、Linux x86_64、Windows x86_64 四个包并创建 Release。
6. 检查产物与 smoke 结果，补充面向用户的 Release Notes；区分新增能力、修复、迁移与平台限制。

## 7. 安全与文档

- 发现漏洞请先阅读 [SECURITY.md](SECURITY.md)，不要在公开 Issue 中披露未修复漏洞。
- 新增或变更 API、配置、数据目录、权限边界、缓存语义时，必须同步 `ARCHITECTURE.md` 和必要的 `docs/SSE-CONTRACT.md`。
- README 只保留新用户所需内容，不堆叠研究进度、旧提交号、已完成 bug 列表或重复 CLI 参考。
- `LLooMprogress.md` 记录状态与待办；报告、设计与历史快照各自归位，不反复复制到入口文档。
