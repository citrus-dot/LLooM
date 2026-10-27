# 测试与验收指南

> 目的：用可复制的步骤验证当前工作区，而不是记录某个历史提交的测试过程。
> 自动化命令见 [CONTRIBUTING.md](CONTRIBUTING.md#4-验证要求)。本文用于需要启动服务的手工验收、故障排查与发布前抽查。

## 1. 先隔离测试数据

**不要在未备份的情况下对真实 `data/` 执行修改 SQL、删除对话、创建测试 Key 或标记过期价格。**以下示例使用独立的临时数据目录与端口，避免污染日常使用；Ollama 可以继续复用本机实例。

```bash
# 在仓库根目录执行；lloom-server 的退出由终端 Ctrl+C 处理。
export LLOOM_E2E_DIR="$(mktemp -d /tmp/lloom-e2e.XXXXXX)"
export LLOOM_DATA_DIR="$LLOOM_E2E_DIR/data"
export LLOOM_WEB_PORT=17861
export BASE_URL="http://127.0.0.1:$LLOOM_WEB_PORT"

npm --prefix webui ci
npm --prefix webui run build
cargo run -p lloom-server
```

启动后，另一个终端执行：

```bash
export BASE_URL="http://127.0.0.1:17861"
curl -fsS "$BASE_URL/api/health"
```

预期返回包含 `status: ok` 的 JSON。若旧实例占用了 `17861`，先更换端口，而不是关闭不确定归属的进程。

需要真实云模型时，在 WebUI 模型页配置 API Key，或在启动前于 `.env` 配置对应环境变量。**真实调用可能产生费用。**没有可用云端 Key 时，可以仅做启动/路由/本地 Ollama 验收，并明确记录未覆盖云模型。

测试完成后停止该终端的服务，再删除**自己刚创建的**临时目录：

```bash
# 先确认路径确实是 mktemp 创建的前缀，再清理。
printf '%s\n' "$LLOOM_E2E_DIR"
# 例如前缀确认为 /tmp/lloom-e2e. 后：
rm -rf -- "$LLOOM_E2E_DIR"
unset LLOOM_E2E_DIR LLOOM_DATA_DIR LLOOM_WEB_PORT BASE_URL
```

如果服务在其他终端、旧 shell 或新会话中启动，请先重新设置相同的 `LLOOM_DATA_DIR` 和端口。数据目录在创建后才会出现；首次 L2 调用会尝试下载 FastEmbed 模型，失败时应降级而不是阻塞普通聊天。

## 2. 自动化验收

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm --prefix webui ci
npm --prefix webui run build
npm --prefix webui test
cd tui && npx tsc --noEmit
git diff --check
```

`webui` 测试使用 Bun；本地没有 Bun 时不能将「未运行」写成通过。测试计数会随代码改变，提交说明请记录实际命令、日期与结果。

## 3. 手工验收清单

每项记录「实际结果 / 通过 / 未测原因」。不要仅凭页面能打开就判定功能通过。

### A. 启动、模型与访问边界

- [ ] `/api/health` 可用，首页可加载；没有配置模型时不会把「服务已启动」误报成「Provider 已可用」。
- [ ] 在模型页新增、编辑、停用、重新启用一个模型；编辑弹窗回填名称和配置，名称含 `/` 的模型也能更新。
- [ ] 云端模型 API Base 留空时，OpenAI/DashScope/Anthropic 使用默认地址；`custom` 没有默认地址，调用时应给出可理解的错误，不静默打到错误地址。
- [ ] 「立即测活」结果与历史健康状态分开显示；失败不会被描述为健康。
- [ ] 本机管理 API 可用；把服务绑定到非环回地址且没有 API Key 时，启动拒绝并给出原因。该测试使用独立端口，不影响正常服务。

### B. 普通聊天与流式输出

```bash
curl -N "$BASE_URL/api/chat/stream" \
  -H 'Content-Type: application/json' \
  -d '{"messages":[{"role":"user","content":"简要解释快速排序"}]}'
```

- [ ] 响应是 SSE，不是误用 `q` 字段后的校验错误；能收到完整回答和 `done`。
- [ ] 中文、emoji、较长 Markdown 内容没有被 chunk 边界破坏。
- [ ] 有未命中缓存的新问题时，路由、usage、成本与模型信息与响应一致。
- [ ] 生成中点击「停止」，按钮与状态恢复，已生成内容保留并标为中断/部分完成；服务端尽量保存部分用量。
- [ ] 在 WebUI 快速切换会话、返回对话、刷新页面，历史内容、思考卡和 plan 不错乱。
- [ ] 无响应、Provider 失败、上游 429、流中断时，界面显示真实错误，不出现「成功空回答」。

### C. 缓存与上下文

- [ ] 对同一模型、同一 system prompt、同一温度发送两次等价请求；第二次按实现命中 L1/L2，返回相同内容且记录 `cache_hit`。
- [ ] 改 system prompt、模型或温度后，不错误复用旧答案。
- [ ] 历史前缀相同但正文不同的对话，不因 L1 指纹碰撞而串答案。
- [ ] 长对话裁剪后不会产生孤儿 tool 消息、非法角色序列或丢失 system；多模态/数组 content 不被估成 0 token。
- [ ] L2 模型不可用（例如 Intel Mac 未装 ONNX Runtime）时，仅 L2 降级，L1 和 Provider 调用仍可用。

缓存命中需结合响应元数据和 SQLite 记录验证，不能只根据「返回更快」判断。探针、影子评测和 `/v1` 代理不应与普通会话共享缓存命名空间。

### D. OpenAI-compatible `/v1` 与 API Key

```bash
curl -fsS "$BASE_URL/v1/models"
```

在 WebUI 的 API Keys 页创建一个 Key，明文只显示一次，立即保存到测试环境变量：

```bash
export LLOOM_TEST_KEY='<刚创建的测试 Key>'
curl -fsS "$BASE_URL/v1/models" \
  -H "Authorization: Bearer $LLOOM_TEST_KEY"
```

- [ ] 首个 Key 创建前，本机 `/v1` 可按规则免鉴权；创建后缺少/错误 Bearer 返回 401。
- [ ] 有效 Key 可访问允许模型；停用 Key、过期 Key、模型白名单之外的模型按契约拒绝。
- [ ] 非流式与 `stream:true` 都能返回；`model:"auto"` 走智能路由。
- [ ] 请求只含支持的字段：`model`、`messages`、`temperature`、`max_tokens`、`stream`。
- [ ] 上游 429 向客户端返回 429；上游普通请求错误为 400，连接/5xx 为 502，超时为 504。
- [ ] 并发请求不超过 RPM；额度采用预扣-结算，失败/断连不会永久多占额度。异常预留由后台回收。
- [ ] 远程 `/v1` 必须有 Key；远程 `/api/*` 与 `/metrics` 不可用。未配置远程访问条件时，不尝试把管理 API 暴露到局域网。

### E. 编排、推理与路由评估

- [ ] 复杂任务出现分解、子任务、聚合事件；依赖顺序正确，重复子任务 ID 不会导致事件错乱。
- [ ] 子任务失败、客户端停止与 Provider 错误有明确事件/状态，已产出部分尽量保留。
- [ ] 推理模型的 reasoning 与最终答案分离；刷新会话后思考卡仍在。缓存命中时无新 reasoning 是已知边界，不应误判。
- [ ] 路由体检能生成报告；没有有成本差异的影子样本时，建议为空属于正常数据状态，不应为了截图伪造样本。
- [ ] 人工采纳建议前不改变生产权重；采纳后才生效。历史 band 分布参与重放，不是始终使用空 query 的基带。

### F. 指标、价格与对账

```bash
curl -fsS "$BASE_URL/metrics" | head -30
```

- [ ] 返回 Prometheus 文本格式与 build_info；build_info 版本与当前构建一致，指标没有敏感 Key/完整 PII。注意：`/api/health` 的版本字段目前仍是硬编码 `2.0.0`，不能拿它判断构建版本（见「当前文档与实现差异」）。
- [ ] 价格缺失/非法范围被拒绝；有阶梯价时估算与实际口径一致，未知云价不能伪装成免费模型。
- [ ] 手动修改价格后 manual 锚定不被远端刷新覆盖；参考价与实际 Provider 价分开。
- [ ] 有真实百炼账单时，使用副本执行：

  ```bash
  python3 scripts/bill_reconcile.py --bill /path/to/bill.csv --db "$LLOOM_DATA_DIR/lloom.db" --save
  ```

  报告写入测试数据目录。用量页「已对账」只在报告存在且可读取时显示；缺文件/损坏文件不应导致页面崩溃。
- [ ] 探针流量标记为 `probe`，不与真实 WebUI 用量混淆。

## 4. 故障排查

| 现象 | 优先检查 |
|---|---|
| 页面打不开或静态资源 404 | `npm --prefix webui ci && npm --prefix webui run build` 后重启；确认访问的是当前服务的端口 |
| 服务启动但聊天报「未配置模型」 | 模型是否启用、Provider Key 是否配置；服务进程启动成功不等于 Provider 可用 |
| 启动时提示非环回需要 API Key | 先在本机 WebUI 创建 Key，或恢复 `LLOOM_BIND=127.0.0.1`；不要直接绕过启动保护 |
| `/v1` 返回 401 | 是否已创建 Key、Bearer 是否完整、Key 是否启停/过期、模型是否在白名单 |
| 管理 API 远程 403 | 当前设计为仅本机管理；不要通过代理把 `/api/*` 暴露出去 |
| macOS 无法打开 | 对**解压出的 LLooM 目录**运行 `xattr -cr LLooM`，再启动；不要对任意系统目录执行 |
| Intel Mac 没有 L2 | 安装 `brew install onnxruntime`；L1 与路由不受影响 |
| 本地模型失败 | Ollama 是否运行、`:11434` 是否可访问、模型是否已下载；LLooM 不捆绑 Ollama |
| 首次 L2 较慢或失败 | 查看模型下载/初始化日志；失败应自动降级，不应删除数据目录来修复 |
| 中文流式乱码或错误被吞 | 按 `docs/SSE-CONTRACT.md` 检查客户端是否保留跨 chunk 字节、处理未命名 `data:` 帧与终帧 |
| SQLite 锁或升级缺列 | 使用最新程序启动完成幂等迁移；先备份 `data/`，不要手工删库；报告具体错误与版本 |

## 5. 当前文档与实现差异

本轮文档审查确认以下现状，修复前按实际情况验收，不把旧配置或硬编码字段当成有效功能：

| 项目 | 当前实现 | 验收/使用建议 |
|---|---|---|
| `LLOOM_PROXY_TOKEN` | 已从 `.env.example` 的无效配置示例移除；此前 Rust 代码未读取它 | 使用 WebUI 的 API Keys 页创建下游 Key，不能依赖该变量 |
| `/api/health` 的 `version` | 固定 `2.0.0` | 用 `/metrics` build_info 或 `git describe` 判断构建版本 |
| L1 容量淘汰 | 按创建时间 FIFO；L2 才按最近命中时间淘汰 | 不要以「高频 L1 条目一定不被淘汰」为测试预期 |
| `/v1` 请求字段 | 仅 `model/messages/temperature/max_tokens/stream` | 不把 tools、多模态、response_format 等未实现字段视为兼容 |
| 文档中的「生产级」 | 指工程质量与明确边界，不等于公网托管/多租户能力 | 公网部署需自行补 TLS、反代、备份与访问控制 |

## 6. 证据与报告

验收记录至少包含：版本/commit、OS/arch、启动方式、模型类型（本地/云及是否真实调用）、命令、结果、未测项、日志位置。真实 Key、完整对话与个人信息必须脱敏。

`docs/AUDIT-FIXES-2026-10-23.md` 是审计修复说明，`LLooMprogress.md` 是当前进度；二者不能替代当前版本的实测。研究/Benchmark 的复现使用 [benchmarks/REPRODUCE.md](benchmarks/REPRODUCE.md)，与本文的产品手工验收分开。
