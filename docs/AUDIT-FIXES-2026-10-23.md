# 审计修复前后对照说明

> 更新：2026-10-23  
> 分支：`v2`  
> 最近修复提交：`61e6851`、`b79550c`、`ab1b4b9`、`157fc2b`

本文记录 LLooM 本轮全仓审计修复的**修改前行为、修改后行为、影响范围和验证方式**。“修改前”指本轮修复开始时的代码状态。

## 1. 总体结果

本轮把缓存、计费、限流、鉴权、输入安全、Provider 流式协议和管理界面中的问题统一收口到 Rust 数据面。

修改后：

- 非环回地址绑定在没有 API Key 时拒绝启动。
- 管理接口和 OpenAI-compatible 接口具备来源、鉴权和请求边界保护。
- PII 默认脱敏，越狱/提示词注入默认拦截。
- API Key 的 RPM 和额度预留使用数据库原子操作。
- Provider 重试只针对可重试错误，并在读取响应流前完成。
- 缓存严格区分系统提示词、温度和模型上下文。
- 用量、价格和预算边界在缺失或浮点误差场景下不会静默错账。
- SSE 副作用不再阻塞热路径，UTF-8 分片不会破坏中文内容。
- 日志、运行时阈值和时间/价格参数集中管理。

## 2. P0：高风险问题

### 2.1 缓存上下文隔离

**修改前：**缓存命中条件不足以表达完整的 system prompt 和请求上下文；不同 system prompt 或 temperature 的请求存在错误复用结果的风险；L1/L2 缓存没有完整串联到主聊天路径。

**修改后：**`chat_stream` 显式接收 `ChatCacheCtx`；精确缓存和语义缓存都要求模型、system hash、temperature 等上下文匹配；新增 `StreamEvent::CacheHit`；完全相同请求可在不调用 Provider 的情况下返回缓存结果。

**主要位置：**`crates/lloom-core/src/ai_client.rs`、`exact_cache.rs`、`semantic_cache.rs`。

### 2.2 HTTP 来源、Host 和 CSRF

**修改前：**管理面请求对 Host/Origin 的约束不足，远程管理和跨 Origin 浏览器请求边界不清晰。

**修改后：**本地管理请求检查环回 Host/Origin；远程管理请求拒绝；远程 `/v1` 访问必须满足 API Key 条件；非环回绑定且没有 API Key 时启动失败；`/api/shutdown` 增加 handler 层环回检查。

**主要位置：**`crates/lloom-core/src/server.rs`、`crates/lloom-server/src/main.rs`。

### 2.3 模型管理和价格完整性

**修改前：**模型编辑、恢复、停用和价格缺失场景可能状态不一致；缺少有效价格的云模型可能进入生产路由并产生零成本记录。

**修改后：**模型状态和 Provider 注册保持一致；缺失或 `invalid_cost` 的云模型不作为可计费生产模型；价格录入强制检查 USD/token 范围；单候选成本归一化不再错误地产生非零成本权重。

## 3. P1：可靠性、计费和输入安全

### 3.1 RPM 限流

**修改前：**先查询后插入存在并发竞争，多个请求可能同时通过同一 RPM 上限。

**修改后：**使用单条原子 SQL 预留 RPM slot；后台清理过期请求记录；回归测试验证并发请求不能超过限制。

### 3.2 API Key 额度

**修改前：**请求开始和结束时的扣费流程可能在 Provider 失败、流中断或重复结算时重复扣除或长期占用额度。

**修改后：**请求开始原子预留预算；结束时按 request ID 幂等结算实际成本；退回预留与实际成本差额；后台清理过期 reservation。

### 3.3 Provider 重试

**修改前：**连接失败、超时和 5xx 没有统一重试策略。

**修改后：**统一处理 408、409、429、5xx、连接错误和超时；最多重试 2 次，退避基准 500ms/1s/2s 并加入 jitter；`Retry-After` 最多接受 120 秒；只有响应流未消费且 request 可 clone 时才重试。

### 3.4 用量合并

**修改前：**部分 usage frame 可能覆盖此前字段；Provider 不返回 usage 时代理可能记录零用量。

**修改后：**Anthropic 和 OpenAI 使用 snapshot merge；缺失字段保留已有值；代理根据流内容和输入估算缺失 usage；结算、usage 记录和成本计算使用同一最终用量。

### 3.5 PII 与提示词注入

**修改前：**用户文本可能直接发送给 Provider；信用卡/身份证依赖宽松正则，大小写混合、中文注入和标点变形可能绕过检测。

**修改后：**检测邮箱、手机号、SSN、信用卡、IP、身份证和银行账户；信用卡增加发行商前缀与 Luhn 校验；中国身份证增加日期与校验码检查；默认替换为 `[EMAIL]`、`[PHONE]`、`[CARD]` 等掩码；`security.block_pii=true` 时直接拒绝；越狱默认拦截并支持中英文归一化；超长输入执行 64 KiB 防护。

### 3.6 结构化日志

**修改前：**`println!`/`eprintln!` 分散在多个模块，没有统一时间、级别和 target。

**修改后：**新增 `crates/lloom-core/src/logging.rs`，输出格式为：

```text
2026-10-23T12:34:56Z WARN [cache] L2 vector generation failed
```

支持 `LLOOM_LOG=error|warn|info`，core/server 日志统一迁移到该门面。

## 4. P2：性能、数据一致性和边界

### 4.1 SSE 和后台副作用

**修改前：**每个 SSE 事件可能同步触发数据库写入，数据库或质量信号失败会拖慢 token 流。

**修改后：**事件持久化和编排副作用转移到 `spawn_blocking`/后台路径；token 发送不等待非关键副作用；副作用失败只记录日志。

### 4.2 SSE UTF-8 解码

**修改前：**HTTP chunk 不一定是 SSE frame 或 UTF-8 字符边界，中文拆分可能乱码或解析失败。

**修改后：**使用字节级 `LineDecoder`，在完整 UTF-8 和换行边界后解析；支持无结尾换行的最后一帧；新增中文拆分和异常 UTF-8 测试。

### 4.3 Review 重放

**修改前：**网格重放重复扫描数据库，49 个单元放大读取成本，输入 token 固定为 0 导致成本比较失真。

**修改后：**每个 task type 只读取一次数据库快照；49 个网格单元在内存中重放；输入 token 使用近期至少 20 个样本的任务平均值。

### 4.4 Schema 和 usage 写入

**修改前：**多处重复 schema column 检查，usage INSERT 的列和值有错位风险。

**修改后：**统一 `Db::ensure_columns`；usage 写入使用成对列/值构造；补齐 `idx_usage_conv` 等索引。

### 4.5 浮点预算边界

**修改前：**`0.1 + 0.2 <= 0.3` 一类浮点尾差可能错误拒绝请求。

**修改后：**预算边界加入 `1e-9` 容差；价格和成本边界比较使用命名常量。

## 5. P3：清理与纯重构

### 5.1 死代码清理

删除了确认只有声明、没有调用方的遗留 API：`ClassifyResult`、旧 `parse_sse`/`mk_event`、`EnvConfig`、`ServiceStatus`、`SuiteStatus`、`SmartRestartResult`、旧 `add_api_key_usage`、未使用的 settings/routing API、`PII_TYPES` 和未使用的 `stop_ollama`。

### 5.2 魔法数字集中化

**修改前：**重试次数、退避时间、健康探测默认值、API Key RPM、价格范围、后台周期等数字散落在表达式中。

**修改后：**改为 `MAX_RETRIES`、`INITIAL_BACKOFF_MS`、`DEFAULT_HEALTH_PROBE_SEC`、`API_KEY_MAX_RPM`、`MIN_PRICE_PER_TOKEN`、`SECONDS_PER_DAY` 等命名常量。数值和运行行为不变，只提升可读性与审计可追踪性。

### 5.3 日历与时区换算

北京时区换算统一使用分钟、小时、天秒数常量，保持 Asia/Shanghai 固定 +8 小时语义，不引入新的时区依赖。

## 6. 个人信息保护的边界

当前系统属于应用层 PII 防护，不是完整企业级 DLP：覆盖进入模型前的用户文本、Chat、OpenAI-compatible 接口和用户消息写入路径；默认脱敏，也可切换为检测即拒绝。但不会自动回溯清洗历史数据库，也不覆盖终端、邮件、剪贴板、云盘、文件外发、数据库静态加密或统一合规审计。

## 7. 验证结果

```bash
cargo test --workspace --quiet
cargo clippy --workspace --all-targets -- -D warnings
rustfmt --check --edition 2021 <touched files>
git diff --check
cd webui && npm run build && npm test
cd tui && ./node_modules/.bin/tsc --noEmit
```

结果：Rust workspace 测试通过，核心库当前 179 项测试通过；Clippy、rustfmt 和 diff check 通过；WebUI 构建通过，Bun 测试 7/7 通过；TUI TypeScript 检查通过。

## 8. 尚未修改的内容

### `old_db` 时区/decode lint

当前 `v2` 分支和可见 legacy 分支中没有 `old_db` 模块或对应迁移代码，因此没有安全的修改目标。未凭空新增兼容层，避免改变未知历史数据库的迁移语义。

### 审计原文缺失的具体条目

旧审计报告正文不在当前仓库，部分编号只有摘要，没有文件位置、复现步骤和预期行为。已经修复所有能从代码和摘要确定根因的项目；无法确认的项目没有盲改。
