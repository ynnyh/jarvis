# 可观测性:tracing 替换 eprintln + 日志落盘 + 导出按钮

> 阶段一(内功期)任务 1/3。目标:让应用从"黑盒运行"变成"可诊断运行"。
> 后续测试网补全、架构拆分都依赖可观测性 —— 测试失败的诊断、拆分引入回归的定位都需要日志。
> 顺序:可观测性 → 测试 → 架构(从内到外:先看得见,再测得准,最后改得安心)。

---

## 1. 背景

### 现状(审计结论)
- **99 个 `eprintln!` 散落在 67 个 Rust 源文件中**,输出到 stderr,无结构、无落盘、无分级。
  - voice.rs 31 个(最多,全是语音诊断)
  - fine_report/commands.rs 23 个
  - channels/telegram.rs 6 个、qqbot.rs 4 个
  - mcp_client.rs 5 个、tools/deploy.rs 4 个、lib.rs 4 个、其余各 1-3 个
- **`tauri-plugin-log` 已在 Cargo.toml 声明依赖,但代码里完全没用**(lib.rs 没有 `.plugin(tauri_plugin_log::init())`)。
- **出问题时用户拿不到日志**:macOS Gatekeeper 拦截、密钥链失败、LLM 调用失败这类问题,用户报 bug 只能口述,开发者盲飞。
- 单用户自用可以忍,但阶段二(团队期)开始就要求"用户一键给日志"。

### 为什么用 tracing 而非 tauri-plugin-log
- `tracing` 是 Rust 生态事实标准,结构化日志(span/event),后续可接 OpenTelemetry、metrics。
- `tauri-plugin-log` 只是把 log crate 桥接到 Tauri,功能弱(无 span、无滚动文件策略的灵活配置)。
- `tracing-appender` 提供按天/按小时滚动,正是我们需要的。
- 已有的 `tauri-plugin-log` 依赖可保留(避免动 Cargo.toml 引起 lock 变动)或后续移除,本任务不强求。

## 2. 方案(用户已确认:完整方案)

### 2.1 引入 tracing 基础设施
- 依赖:`tracing`、`tracing-subscriber`、`tracing-appender`(env-filter)。
- 初始化时机:`lib.rs` 的 `run()` 最开头,在任何 plugin/spawn 之前。
- 初始化逻辑:
  - 日志目录:`~/.jarvis/logs/`(复用 `settings::jarvis_dir()`)。
  - 文件 appender:`tracing_appender::rolling::daily(dir, "jarvis.log")` → 生成 `jarvis.log.YYYY-MM-DD`。
  - 同时输出到 stderr(console):开发模式下看得到,生产模式下 Tauri 的 console window 隐藏(Windows `windows_subsystem`)。
  - 分层订阅:stderr 用 `fmt` 层(带颜色,INFO+),文件用 `fmt` 层(无颜色,DEBUG+)。
  - env-filter:默认 `info`,可通过 `RUST_LOG=debug` 覆盖。第三方 crate 压到 `warn`(避免 reqwest/tokio 刷屏)。
- 非原子降级:初始化失败(如目录无写权限)不阻断 app,降级为只 stderr。

### 2.2 替换 99 个 eprintln!
按语义分级映射(不是机械替换):
| eprintln 语义 | tracing 级别 | 数量预估 |
|---|---|---|
| `[xxx] 启动失败/错误/崩溃` | `error!` | ~15 |
| `[xxx] 拒绝/拦截/不可用` | `warn!` | ~20 |
| `[xxx] 开始录/开始下载/已启动` | `info!` | ~25 |
| `[xxx] 采样数/耗时/退出码` | `debug!` | ~35 |
| `[xxx] 详细字节/逐帧` | `trace!` 或删除 | ~4 |

保留模块前缀(`[voice]` → `tracing::info!(target: "voice", ...)`)便于按模块过滤。

### 2.3 日志导出按钮(前端 + 后端)
- **后端命令** `export_diagnostic_logs`:
  - 打包 `~/.jarvis/logs/` 最近 3 天的日志文件 + 一份环境摘要(OS 版本、app 版本、配置摘要——脱敏,不含密钥链内容)。
  - 用 `rfd`(已有依赖)弹保存对话框,用户选位置导出 zip。
  - 环境摘要字段:`app_version`、`os`(OS 版本)、`config_summary`(各功能开关状态,secret 字段用 `SECRET_PLACEHOLDER`)。
- **前端入口**:设置页"常规"区加"导出诊断日志"按钮,调用 `invoke('export_diagnostic_logs')`。
- **脱敏红线**:导出的 zip 绝不含密钥链内容、LLM apiKey、禅道密码。环境摘要只含开关状态和版本。

### 2.4 日志生命周期
- rolling daily,文件名 `jarvis.log.YYYY-MM-DD`。
- 本任务**不做自动清理**(删旧日志)—— 单用户日志量不大,清理可作为后续优化。
- prd 注明 TODO:后续可加启动时清理 7 天前的日志。

## 3. 范围

### In scope
1. 加 tracing + tracing-subscriber + tracing-appender 依赖。
2. `lib.rs` 初始化 tracing(daily rolling + stderr 双输出 + env-filter)。
3. 替换 99 个 `eprintln!` 为分级 `tracing` 宏(按 §2.2 映射)。
4. 新增 Tauri 命令 `export_diagnostic_logs`(打包日志 + 环境摘要 → rfd 保存)。
5. 设置页加"导出诊断日志"按钮。
6. 脱敏验证:导出内容不含任何 secret。

### Out of scope
- 自动清理旧日志(后续优化)。
- metrics(计数器/直方图)—— 非日志范畴,阶段二再说。
- 前端错误上报(前端 try/catch 写入后端日志)—— 后续任务。
- OpenTelemetry / 远程上报 —— 阶段二/三。
- 移除 `tauri-plugin-log` 依赖(可保留,避免无关 lock 变动)。

## 4. 执行步骤(实现阶段参考)

### Step 1:依赖 + 初始化
1. `Cargo.toml` 加 `tracing`、`tracing-subscriber`(features: env-filter, fmt)、`tracing-appender`。
2. 新建 `src-tauri/src/logging.rs`,实现 `init_logging() -> WorkerGuard`(guard 必须保活,否则 appender 丢日志)。
3. `lib.rs` 的 `run()` 开头调 `init_logging()`,guard 存入 `tauri::app::App`(manage 或 leak —— leak 可接受,生命周期等于进程)。

### Step 2:逐文件替换 eprintln!
按文件批量替换(不是一次性 99 个,容易乱):
- 先做 `lib.rs`(4 个,初始化附近,影响最小,验证基础设施)。
- 再做 `mcp_client.rs`(5)、`channels/*`(10)。
- 再做 `tools/deploy.rs`(4)、`fine_report/`(26)。
- 最后做 `voice.rs`(31,最多,单独一轮)。
- 每轮做完跑 `cargo check`。

### Step 3:导出按钮
1. 后端 `commands/mod.rs` 加 `export_diagnostic_logs`(读 logs/ 目录 → 打包 zip → rfd 保存)。
2. 环境摘要生成函数(读 settings,脱敏)。
3. 前端设置页加按钮。

### Step 4:脱敏验证
- 手动触发导出,grep 导出的 zip 内容,确认无 apiKey/password/token 明文。

## 5. 验收标准

| # | 条件 | 验证方式 |
|---|------|---------|
| 1 | tracing 初始化成功,日志写入 `~/.jarvis/logs/jarvis.log.YYYY-MM-DD` | 启动 app,检查日志文件生成 |
| 2 | stderr 仍有输出(开发模式可见) | `npm run desktop:dev` 看终端 |
| 3 | 99 个 eprintln 全部替换 | `git grep eprintln! src/` 为 0(或仅剩极少合理例外,记录在 info.md) |
| 4 | 导出按钮工作:点击后 rfd 保存 zip,含日志 + 环境摘要 | 手动点击验证 |
| 5 | 导出内容脱敏:无 apiKey/password/token 明文 | grep 导出 zip 解压内容 |
| 6 | 环境摘要含 app 版本、OS、开关状态 | 检查摘要文件 |
| 7 | cargo check + clippy(尽量 -D warnings,不行则 -W)通过 | CI 绿 |
| 8 | check:text 通过 | `npm run check:text` |
| 9 | 日志分级合理(error/warn/info/debug 语义正确) | 人工抽查 voice.rs、mcp_client.rs 的替换 |

## 6. 风险

| 风险 | 应对 |
|------|------|
| WorkerGuard 提前 drop 导致丢日志 | guard 存入 App 生命周期;启动失败降级 stderr,不阻断 |
| 导出 zip 误含密钥链内容 | 环境摘要生成函数显式脱敏;§5 #5 强制 grep 验证 |
| tracing-appender 在某些 Windows 路径权限失败 | 降级 stderr;日志目录用 `jarvis_dir()` 已验证可写 |
| 替换 99 个 eprintln 工作量大、易出错 | 分轮替换(§4 Step 2),每轮 cargo check |
| env-filter 默认级别压得太狠,丢失诊断 | 默认 info,关键路径(voice/mcp/llm)用 debug,文档说明 RUST_LOG |

## 7. 不做功能

遵循"阶段一不新增功能"。本任务的"导出按钮"算**可观测性基础设施的一部分**(诊断工具),不算用户可见功能特性。其余严格不动业务逻辑。
