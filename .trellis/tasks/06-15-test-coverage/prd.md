# 测试网补全:worklog + settings 纯逻辑单测

> 阶段一(内功期)任务 2/3。目标:给两个 0 测试的核心模块补纯逻辑单测。
> 前置已就绪:CI(ci.yml 已建,跑 cargo test)+ 可观测性(tracing 日志,测试失败可诊断)。

---

## 1. 背景

核心模块测试骨架其实已经不错:
- chat_agent(7 测试)、mcp_client(19)、credentials(10)、llm(16)、commit_link(4)、commit_classifier(3)、daily_review(2)、fine_report/html_parser(2)、git_scan(4)、zentao(2)、channels/router(2)

**真正的缺口**:worklog.rs(870 行,0 测试)、settings.rs(216 行,0 测试)。

这两个模块大部分函数是 Tauri command / IO 操作(不可单测),但都有**纯逻辑辅助函数**(输入→输出,无副作用),值得补单测保护。

## 2. 范围

### In scope —— worklog.rs 纯逻辑函数
按可测性和价值排序:

| 函数 | 行号 | 测试要点 |
|---|---|---|
| `today_str()` | 163 | 返回当天日期字符串(格式) |
| `session_path(date)` | 167 | 日期 → 路径(拼接逻辑) |
| `summarize(cards)` | 228 | 卡片列表 → 汇总(各类计数 + 总工时;空列表/NaN/负数边界) |
| `work_profile_from_config(cfg)` | 264 | config.workStyle → 画像(4 合法值 + 非法值兜底 balanced) |
| `config_hours_per_day(cfg)` | 278 | config.workSchedule.periods → 日总工时(空/多段/非法格式) |
| `task_system_hint(task_id)` | 334 | 任务 ID → 系统提示(格式判定) |
| `derive_card_state(card)` | 505 | 卡片 → 状态(各状态转移) |
| `merge_card(fresh, saved)` | 515 | 新卡 + 旧卡合并(冲突优先级) |

### In scope —— settings.rs 纯逻辑函数
settings.rs 主要是密钥链 IO,纯逻辑少。可测的:
- `jarvis_dir()` / `config_path()`:路径拼接(但要测环境变量,需隔离 USERPROFILE/HOME)
- `load_raw_config()`:缺文件返回默认值(可测降级)

> settings.rs 的密钥链函数(`secret_get/set/exists/clear`)依赖 OS 密钥链,不单测(CI 环境不可用,且会污染密钥链)。

### Out of scope
- 核心模块已有测试的(chat_agent/mcp_client/llm 等)不重复补。
- Tauri command 函数(需 AppHandle/State)不单测 —— 那是集成测试范畴。
- 密钥链 IO 不单测(CI 环境 + 污染问题)。
- 前端测试(vitest)不在本任务。
- 本地 cargo test 崩溃问题(判定为开发机环境问题,以 CI 为准)。

## 3. 执行步骤

### Step 1:worklog.rs 加 `#[cfg(test)]` 模块
- 读 worklog.rs 全文,确认上述函数的完整签名和实现细节。
- 逐个函数写测试:正常 case + 边界 case(空输入/非法值)。
- 测试要能独立编译(不依赖文件系统/网络)—— `session_path` 测拼接逻辑不测真实写入。
- 用 serde_json::json! 宏构造 config Value,避免手写字符串。

### Step 2:settings.rs 加 `#[cfg(test)]` 模块
- `load_raw_config`:测缺文件降级(用临时路径)。
- 路径函数:测拼接结果(可能需要临时改环境变量,注意线程安全 —— std::env::set_var 非线程安全,测试里要避免并发)。

### Step 3:验证
- `cargo test --lib --no-run` 编译通过。
- `cargo test --lib` 本地尝试运行(若仍崩溃,以 CI 为准,不阻塞)。
- `cargo clippy` 无新 warning。

## 4. 验收标准

| # | 条件 | 验证方式 |
|---|------|---------|
| 1 | worklog.rs 有 `#[cfg(test)]` 模块,覆盖 §2 列的 8 个函数 | 代码审查 |
| 2 | 每个函数至少 2 个测试(正常 + 边界) | 测试计数 |
| 3 | settings.rs 有 `#[cfg(test)]` 模块,覆盖降级逻辑 | 代码审查 |
| 4 | `cargo test --lib --no-run` 编译通过 | 本地 |
| 5 | `cargo check` + `check:text` 通过 | 本地 |
| 6 | 新测试不依赖文件系统/网络/密钥链(纯逻辑) | 代码审查 |

## 5. 风险

| 风险 | 应对 |
|------|------|
| 本地 cargo test 崩溃,跑不了 | 以 CI 为准;`--no-run` 能编译即视为通过 |
| worklog.rs 函数签名需要调整可见性(测试要访问私有函数) | `#[cfg(test)]` 模块在同一文件内,可访问私有函数,无需改可见性 |
| settings.rs 环境变量测试非线程安全 | 避免测环境变量;只测降级逻辑 |

## 6. 不做功能

遵循"阶段一不新增功能"。只加测试,不改业务逻辑。
