# Research: whisper-rs 在 Windows 上的编译风险（成败关键）

- **Query**: whisper-rs（whisper.cpp 绑定）能否在 Windows 顺利编进 Tauri；需要什么工具链；常见坑；有无 prebuilt
- **Scope**: external（结合 whisper-rs-sys 构建机制 + 本仓库 release/CI 现状）
- **Date**: 2026-06-07

## 一句话风险评级

**中等偏可控风险。** whisper-rs 在 Windows 上**可以**编过，但**不是 `cargo add` 就完事**——`whisper-rs-sys` 会在 build 时用 `cmake` + C/C++ 编译器从源码编 whisper.cpp。**前置条件是开发机和 CI 都装好 MSVC C++ 工具链 + CMake。** 这是整个任务最容易卡住的一步，必须在 PRD 里作为「第一道验证关」单列。

## Findings

### 为什么有风险：whisper-rs-sys 是源码编译的 -sys crate

- `whisper-rs 0.16.0` → 依赖 `whisper-rs-sys ^0.15`（已核实）。
- `whisper-rs-sys` 的 `build.rs` 会调用 **CMake** 构建 whisper.cpp（C/C++），再用 `bindgen` 生成 FFI。
- 这意味着构建机必须有：
  1. **C/C++ 编译器**：Windows 上是 MSVC（Visual Studio Build Tools 的 "Desktop development with C++"，含 MSVC + Windows SDK）。
  2. **CMake**（在 PATH 里）。
  3. （bindgen 路径）通常还需要 **LLVM/libclang**——`LIBCLANG_PATH` 环境变量有时要手动指。这是 Windows 上最常见的报错点（`bindgen` 找不到 libclang）。

### Windows 上的常见坑（务必在 PRD 里提示）

1. **缺 MSVC / Windows SDK** → CMake 配置阶段失败。修：装 VS Build Tools，勾 C++ 桌面开发。
2. **bindgen 找不到 libclang** → 报 `Unable to find libclang`。修：装 LLVM，设 `LIBCLANG_PATH` 指向 `bin` 目录。
3. **CMake 不在 PATH** → build.rs 起不来。修：装 CMake 并加 PATH。
4. **首次编译很慢**：whisper.cpp 是 C++ 项目，冷编几分钟正常；会拖长本地和 CI 的构建时间。
5. **`panic = "abort"`（本仓库 release profile 已设）**：whisper-rs 内部出错会直接 abort 进程，不能 catch_unwind 兜——错误处理要在调用层用 `Result` 显式处理，别依赖 panic 恢复。

### 与本仓库现状的冲突点（重点核对）

- **本仓库目前完全没有 C/C++ 构建依赖**：`Cargo.toml` 里全是纯 Rust + 预编译特性的 crate（`rusqlite` 用 `bundled` 也会编 C，但 SQLite 的 C 编译比 whisper.cpp + cmake + clang 这套轻量很多）。
  - 注意：`rusqlite = { features=["bundled"] }` 已经在用——说明**构建链里已经能编 C**（cc crate 路径通了）。但 whisper-rs 额外需要 **CMake + libclang**，比 SQLite bundled 多两个外部依赖。
- **CI / 发版**：本仓库 release 通过 GitHub Actions + Tauri 打包（见 memory `rule_release_workflow.md`，以及 `desktop/scripts/pre-release.mjs`、`publish-to-gitee.mjs`）。CI 的 Windows runner **必须预装 CMake + LLVM**，否则发版直接挂。GitHub `windows-latest` runner 自带 VS + CMake，但 **libclang/LLVM 可能要显式 setup**（`LIBCLANG_PATH`）。这要在打包流程里验证。
- **macOS 端**：whisper-rs 在 macOS 上编译相对顺（clang 是系统自带），还能选 `metal`/`coreml` 加速，但 v1 只用 CPU。

### 有没有 prebuilt / 规避源码编译的路子

- whisper-rs 官方**不提供 prebuilt 二进制**，必须本地编。
- 规避思路（若 Windows 编译卡死，作为 plan B）：
  - **plan B-1**：改用 `sherpa-onnx`（带 onnxruntime 预编译库，绕开 cmake 编 C++），但引入 onnxruntime 链接/分发问题，并非更省心。
  - **plan B-2**：把 whisper.cpp 编成独立 sidecar 可执行（Tauri sidecar），主进程通过进程间通信调用——本仓库已有成熟的子进程模式（`rmcp` 的 `transport-child-process`、`mcp_client.rs` spawn 子进程、`silent_command` 防黑窗）。这能把 C++ 编译与主二进制解耦，但要管 sidecar 的分发与平台二进制。
  - **plan B-3**：whisper.cpp 提供官方 `whisper-cli`/server 可执行，直接当 sidecar 下发（但要自己管多平台二进制，体积也上去了）。

## 建议的「第一道验证关」（写进 PRD / 计划）

> 在写任何业务代码前，**先做一个最小 spike**：在目标 Windows 开发机上，新建/在本仓库加 `whisper-rs` 依赖，写个 10 行 main 加载 `ggml-base` 跑通一段 wav 转文字，确认 `cargo build` 能过 + `tauri build` 能过 + CI Windows job 能过。**这一步通过，整个任务才算技术可行。** 不通过则立即转 plan B（sidecar 或 sherpa）。

## Caveats / Not Found

- 未在真实目标 Windows 机器实跑 `cargo build`，以上是基于 whisper-rs-sys 构建机制的推断 + 社区高频报错。实测结果以 spike 为准。
- 未核实本仓库 GitHub Actions workflow 文件里 Windows runner 是否已装 LLVM/CMake（应去 `.github/workflows/` 确认；本次研究聚焦 src，未深挖 CI yaml）—— 这是发版前必查项。
- `whisper-rs-sys 0.15` 的确切 build.rs 行为（是否纯静态、是否生成 dll）未逐行核对，落地 spike 时观察产物即可。
