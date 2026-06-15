# CI 接入 cargo test + clippy + check:text

> 阶段一(内功期)的第一个任务。目标:给项目装上"改了代码立刻知道有没有改坏"的自动防线。
> 这是后续所有测试补全的前置 —— 没有 CI 跑测试,补了测试也是摆设。

---

## 1. 背景

当前 fix(62) > feat(56),每个版本都在修回归。根因之一:**仓库没有任何自动化测试/lint 门槛**。

现状盘点:
- **两套 CI,都只做发版构建,零测试**:
  - `.github/workflows/release.yml`(GitHub Actions,打 tag 触发)
  - `.circleci/config.yml`(CircleCI,手动 `run_release` 参数触发)
- **22 个 Rust 文件有 `#[cfg(test)]` 内联测试**,共 17 个测试用例,但**从没在 CI 跑过**,且**本地也跑不起来**(见 §2)。
- **没有 clippy / lint 门槛**。
- `check:text`(乱码审计)已在 release.yml 跑,但只在发版时,平时改代码没反馈。

## 2. 拦路虎:cargo test 本地崩溃(必须在接 CI 前修掉)

### 现象
- `cargo test --lib --no-run` → **编译通过**。
- `cargo test --lib` → **运行时崩溃**,退出码 `0xC0000139 (STATUS_ENTRYPOINT_NOT_FOUND)`。
- 直接运行测试二进制 `--list` → 同样崩溃,**无任何 stdout/stderr 输出**。
- 这是 Windows 加载器在解析 DLL 导入表入口点时失败 —— 二进制进程还没执行到 `main` 就挂了。

### 已确认的事实(brainstorm 阶段排查结论)
1. **不是缓存损坏**:删除 test 二进制后 `cargo test --no-run` 重新编译,新二进制同样崩溃。
2. **不是某个 commit 的回归**:用 git worktree checkout 到基线 commit `34e2152`(pet 功能之前,2026-06-12),干净全量编译后**同样崩溃**。说明问题更早就存在,只是从没人跑过测试。
3. **不是 rusqlite 未用 bundled**:cargo metadata 确认 rusqlite 实际激活了 `bundled` + `modern_sqlite` 特性(虽然 cargo tree -e features 的输出有迷惑性,resolve graph 才是权威)。
4. **二进制依赖表差异**:对比当前崩溃二进制 vs 一个旧的(曾短暂能 `--list` 出测试的)二进制:
   - 崩溃的:17 个 DLL,含 `user32/gdi32/comctl32/dwmapi/uxtheme/shell32/ole32` 等 **GUI/shell 库**。
   - (曾经)正常的:6 个 DLL,纯系统基础库。
   - 但两者源码相同 —— 说明 GUI 库的引入是**链接期**的,且那个"正常"二进制的正常是**环境相关、不可稳定复现**的。
5. **依赖树**:存在 `windows` crate 3 版本并存(0.58/0.61/0.62)、`windows-sys` 4 版本并存 —— 这是 Tauri 生态常态,通常不致命,但增加了入口点解析的不确定性。

### 修复方向(实现阶段继续定位 + 修复)
由于根因是**环境相关的 Windows DLL 入口点解析失败**,且本地复现不稳定,实现阶段按以下顺序尝试:

**方案 A(首选,最小侵入):排除原生扩展干扰**
- 怀疑 `sqlite-vec` 的 `sqlite3_auto_extension` 在测试二进制启动阶段(static/全局构造)触发了原生库加载。先验证:临时把 `memory` 模块从 lib.rs 的 `mod` 声明里注释掉,看测试能否跑。
  - 能跑 → 根因锁定 sqlite-vec 初始化,修复方向是让 vec 扩展注册**延迟到真正 open db 时**而非二进制启动。
  - 还崩 → 排除 sqlite-vec,转方案 B。

**方案 B:clippy 先行,测试隔离**
- 若测试二进制短期内无法在 Windows 本地跑通,先在 CI 上跑(CI 的 Windows runner 是干净的 Server 环境,可能不复现本机的入口点问题)。
- CI 上能跑 → 本地问题归档为"开发机环境问题",CI 成为测试的唯一运行场所(可接受,符合"CI 是质量门"的定位)。
- CI 上也崩 → 必须修,用 procmon / WinDbg 在 CI runner 上定位具体哪个 DLL 缺入口点。

**方案 C(兜底):#[ignore] 隔离 + feature gate**
- 若上述都不行,把触碰原生库的测试用 `#[cfg(feature = "integration")]` 隔离,默认 `cargo test` 只跑纯逻辑测试(不链接原生库的模块)。保证 CI 有"一道绿线",而非全红。

> ⚠️ 修复方案在实现阶段以"让 `cargo test --lib` 在 CI 上稳定绿"为唯一验收标准,不强求本地复现修复(本地环境可能是开发机特有的 DLL 污染)。

## 3. CI 落点决策

**决策:GitHub Actions 新建独立 `.github/workflows/ci.yml`,push/PR 触发。**

理由(供参考):
- 现有 release.yml 只在打 tag 时跑,平时改代码零反馈,失去意义。
- CircleCI 是手动触发的发版流水线,职责单一,不混入测试。
- 独立 ci.yml 职责清晰:发版归发版,质量门归质量门。
- 发版前 ci.yml 必须绿(可后续在 release.yml 加 `needs` 或 branch protection 规则强制)。

## 4. 范围

### In scope
1. 修复 `cargo test --lib` 运行时崩溃(§2),使测试能稳定执行。
2. 新建 `.github/workflows/ci.yml`,触发条件:push 到 main + 所有 PR。
3. ci.yml 执行三步:
   - `npm run check:text`(乱码审计,已有脚本)
   - `cargo test --lib`(测试,修完崩溃后)
   - `cargo clippy --all-targets -- -D warnings`(lint,warning 即 fail)
4. CI 缓存策略:复用 release.yml 的 `Swatinem/rust-cache@v2` + `cache: 'npm'` 模式。
5. 跨平台:Windows 跑(主平台);macOS 跑(linux 跳过,因为本项目是桌面应用,无 linux target)。

### Out of scope(后续任务)
- 补新的测试用例(本任务只让现有 17 个测试能跑 + CI 接入)。
- 清理 clippy 报出的全部 warning(本任务 clippy 接入后,若存量 warning 太多,可先用 `-W warnings`(只 warn 不 fail)过渡,但最终目标仍是 `-D warnings`)。
- 前端测试(vitest 等),阶段一后续任务。
- CircleCI 加测试步骤(不碰它,保持发版职责单一)。

## 5. 验收标准

| # | 条件 | 验证方式 |
|---|------|---------|
| 1 | `cargo test --lib` 在 CI(Windows + macOS)稳定通过 | CI 绿灯 |
| 2 | `.github/workflows/ci.yml` 存在,push/PR 触发 | 推一个 PR 看 Action 跑 |
| 3 | ci.yml 跑 check:text + cargo test + clippy 三步 | 查 workflow 日志 |
| 4 | clippy 接入(允许首版用 `-W warnings` 过渡,但文件里写明 TODO 升级到 `-D warnings`) | 查 ci.yml |
| 5 | 本地 `cargo test --lib` 若能修好则一并修;修不好则在 prd/info 记录,CI 为准 | 本地 + CI 双验证 |

## 6. 风险

| 风险 | 应对 |
|------|------|
| 测试崩溃根因定位耗时超预期 | 用方案 B/C 兜底,保证 CI 至少有绿线,不阻塞阶段一推进 |
| clippy 存量 warning 巨大 | 首版 `-W warnings`,新开任务专项清理 |
| CI 上 macOS cargo test 也崩 | 说明是真实的代码/依赖问题(非开发机污染),必须修;用方案 A 深挖 |

## 7. 不做功能

本任务严格遵循"阶段一不新增功能,只补地基"。只动 CI 配置 + 必要的测试崩溃修复,不加任何用户可见功能。
