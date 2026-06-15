# 技术设计 / 实现笔记

## 测试崩溃本地排查结论(2026-06-15)

### 现象
`cargo test --lib` 在本机(Win11 build 26200)运行时崩溃:
- 编译通过,二进制生成成功
- 运行时退出码 `0xC0000139 (STATUS_ENTRYPOINT_NOT_FOUND)`
- 进程在 DLL 导入表解析阶段挂掉,早于 `main` 执行,无任何 stdout/stderr
- 即便只跑纯逻辑测试(`cargo test --lib commit_link`,不碰任何原生库)同样崩溃
  → 证明问题在二进制链接期的导入表,而非某个测试逻辑

### 排查过程
1. **排除缓存损坏**:删除 target 里所有 jarvis_lib 产物,干净重编译,新二进制(hash `25cf64126`)同样崩溃。
2. **排除某 commit 回归**:git worktree checkout 到基线 commit `34e2152`(pet 功能前,2026-06-12),全量编译后**同样崩溃** → 早就存在,只是没人跑过测试。
3. **排除 rusqlite 未 bundled**:cargo metadata resolve graph 确认 rusqlite 激活了 `bundled` + `modern_sqlite`(`cargo tree -e features` 的输出有迷惑性,resolve graph 才是权威)。
4. **排除 PATH DLL 劫持**:`where user32.dll / dwmapi.dll / uxtheme.dll` 都指向 `C:\Windows\System32`,无山寨。
5. **排除系统太老**:Win11 build 26200 是 2024+ 版本,不缺新入口点。
6. **二进制导入表差异**:当前崩溃二进制依赖 17 个 DLL(含 user32/gdi32/comctl32/dwmapi/uxtheme/shell32/ole32 等 GUI/shell 库);而一个 6/12 编译的旧二进制(曾短暂能 `--list` 出测试列表)依赖 6 个纯基础库 —— 但两者源码相同,说明"正常"是不可稳定复现的环境偶发状态。

### 结论(本任务采纳)
**本地崩溃是开发机环境特有的 DLL 入口点解析问题**(最可能是某软件往 System32/PATH 塞了同名旧 DLL,或某安全软件 hook 了加载器)。**非代码问题,非依赖问题**。

因此本任务按 prd §2 方案 B 推进:
- **以 CI 为准**:GitHub Actions 的 `windows-latest` 是干净 Windows Server runner,预期不复现。
- ci.yml 推上去后,若 CI 上 `cargo test --lib` 绿 → 本地问题归档为"开发机环境问题",CI 成为测试唯一运行场所(符合"CI 是质量门"定位)。
- 若 CI 上也崩 → 才是真实的代码/依赖问题,届时用 procmon / WinDbg 在 CI runner 每步排查具体缺哪个 DLL 入口点。

### 后续(若 CI 也崩的排查清单)
1. 在 CI 加一步 `dumpbin /imports` 或 PowerShell 脚本打印二进制导入表(需要在 CI 装 VS Build Tools 或用 llvm-objdump)。
2. 二分排除依赖:逐个 `#[cfg(test)]` 跳过触碰原生库的模块(memory / voice),定位是哪个 crate 的链接产物引入了缺入口点的符号。
3. 考虑给 sqlite-vec 的 `register_vec_extension` 加 lazy 化(运行时首次 open 时注册,而非 static Once),看能否规避。

## ci.yml 设计决策
- **独立 workflow 文件**,不碰 release.yml(发版职责单一)/ config.yml(CircleCI 手动发版)。
- **触发**:push main + 所有 PR。给 PR 反馈,给 main 兜底。
- **矩阵**:windows-latest + macos-latest,无 linux(桌面应用无 linux target)。fail-fast: false。
- **clippy**:首版 `-W warnings`(只 warn 不 fail),存量 warning 清理后升级 `-D warnings`。ci.yml 里有 TODO 注释。
- **复用 release.yml 的模式**:Swatinem/rust-cache@v2 (workspaces: src-tauri)、npm cache、Node 20、dtolnay/rust-toolchain@stable、macOS universal targets。
