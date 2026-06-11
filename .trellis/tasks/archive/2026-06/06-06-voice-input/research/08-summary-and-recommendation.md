# Research: 综合结论 —— 推荐的 v1 技术栈 + 待拍板决策点 + 最大风险

- **Scope**: 汇总（基于 01-07 各专题）
- **Date**: 2026-06-07

## 推荐的 v1 技术栈（每项带理由）

| 环节 | v1 选择 | 理由 | 详见 |
|---|---|---|---|
| STT 引擎 | **whisper-rs 0.16**（whisper.cpp 绑定，纯 CPU/default feature） | 本地 STT 事实标准，中文可用，单文件模型最易按需下载，静态编进二进制无需分发 DLL | 01 |
| 默认模型 | **`ggml-base-q5_1.bin`（约 57MB）**；进阶档 `ggml-small-q5_1`（约 181MB）设置里可切 | base 量化版下载快、中文「能用」、CPU 速度可接受；small 给在意效果的用户 | 01 |
| 录音 | **cpal**（WASAPI/CoreAudio），48k/i16→16k 单声道 f32 归一（rubato 或线性插值） | 跨平台、whisper 要求 16kHz 单声道 f32，必做格式转换 | 02 |
| 文字注入 | **剪贴板 + 模拟 Ctrl+V**（arboard 存取 + enigo 发组合键），用完恢复剪贴板 | Windows 下中文/Unicode 只有粘贴稳；直接模拟键入会丢字/乱码 | 04 |
| 模型下载 | **reqwest 流式**（已开 stream feature）→ `~/.jarvis/models/`，emit 进度到前端 | 复用现成 `llm.rs` 的 bytes_stream + `app.emit` 事件 | 05 |
| 热键 | **tauri-plugin-global-shortcut 2.3.2**，**toggle（按一下开/再按一下停）** | 仓库零基础需新增；toggle 比 push-to-talk 边沿可靠、冲突少 | 06 |
| 触发入口 | 热键 + 点小人，两者汇聚到同一后端命令 | 约束要求两入口 | 06 |
| 功能开关 | **`voiceInputEnabled: boolean`（默认 false）**，照搬 deployEnabled 全套接线 | 约束要求默认关闭、开启才生效 | 05/07 |
| 状态可视化 | App.vue `JarvisState` 加 `listening`/`transcribing` 两态，复用 `PetAvatar` 发光环 | 现成状态机，几乎零成本 | 07 |
| 转写形态 | **录完整段再转**（whisper 非流式） | whisper 本质非流式；整段转最简单稳定，流式列二期（sherpa） | 01 |
| 语言 | **中英混输**（whisper 多语言模型天然支持，可设 language=auto 或 zh） | base/small 模型本就多语言，无需额外成本 | 01 |

### 一句话架构

> 热键/点小人 → `cpal` 录音（16kHz 单声道 f32）→ 停止 → `whisper-rs` 本地转写整段 → `arboard` 写剪贴板 + `enigo` 模拟 Ctrl+V 注入聚焦框 → 恢复剪贴板。全程 Rust 后端一条龙，小人发光环显示 listening/transcribing 状态，功能默认关、首次开启弹框下模型（reqwest 流式 → `~/.jarvis/models/`）。

## 仍需用户拍板的决策点

1. **触发方式**：v1 定 toggle 还是要 push-to-talk？默认热键键位是什么（建议 `Ctrl+Shift+Space` 之类，且做成可配置）？（见 06）
2. **默认模型档位**：接受 `base-q5_1`（57MB，中文「能用」）作默认吗？还是直接上 `small-q5_1`（181MB，中文更好但下载久）？（见 01）
3. **完整段转 vs 流式**：v1 锁「录完再转」对吗？（流式要换 sherpa，成本高，建议二期）（见 01）
4. **中文 / 多语言范围**：v1 做「中英混输（auto）」还是「仅中文」？（whisper 两者都行，仅是参数差异）（见 01）
5. **模型下载源**：huggingface.co 直链国内不稳——是否要内置 hf-mirror 镜像兜底 / 允许用户填自定义 URL？（见 05）
6. **剪贴板污染容忍度**：确认走「剪贴板+Ctrl+V，用完恢复」，接受非文本剪贴板（如图片）场景下恢复不完美？（见 04）

## ⚠️ 最大风险（决定成败，必须最先验证）

### whisper-rs 在 Windows 上能否编过 + 能否进 CI 发版

**这是整个任务的命门，且当前 CI 几乎肯定会挂。** 已核实证据：

1. `whisper-rs-sys` 用 **CMake + C/C++ 编译器 + libclang(bindgen)** 从源码编 whisper.cpp（见 03）。
2. **本仓库 `.github/workflows/release.yml` 的 Windows job 只装了 Node + `dtolnay/rust-toolchain@stable`，没有任何 CMake / LLVM setup 步骤**（已逐行读 `release.yml`，确认无 `install cmake` / `setup llvm` / `LIBCLANG_PATH`）。
   - `windows-latest` runner 自带 VS Build Tools 和 CMake，但 **libclang/LLVM 路径常需显式处理**，bindgen 找不到 libclang 是 Windows 上 whisper-rs 的高频报错。
   - 即便 runner 自带可用，本地开发机若没装 VS C++ 工作负载 + CMake + LLVM，`cargo build` 直接失败。
3. 本仓库目前唯一的 C 编译是 `rusqlite` 的 `bundled`（走 cc，轻量），**没有 cmake/clang 这套重型 native 构建**——whisper-rs 是第一个引入它的。

**必须做的第一步（写进计划，先于一切业务代码）**：
> 在目标 Windows 开发机加 `whisper-rs` 依赖，写 10 行 spike 加载 `ggml-base` 转一段 wav，确认：(a) 本地 `cargo build` 过；(b) `npx tauri build` 过；(c) **改 `release.yml` 给 Windows job 加 CMake + LLVM 安装步骤后，CI 能过**。
> 这一步过了任务才技术可行；过不了立即转 plan B。

**plan B（若 Windows 编译卡死）**：
- B-2（推荐）：把 whisper 做成 **Tauri sidecar 子进程**，主二进制不直接链 C++。本仓库已有成熟子进程模式（`rmcp` transport-child-process、`mcp_client.rs`、`silent_command` 防黑窗），解耦编译复杂度。
- B-1：换 `sherpa-onnx`（带 onnxruntime 预编译库，绕 cmake 编 C++），但引入 onnxruntime 链接/分发问题，并非更省心。

### 次要风险
- 剪贴板恢复时序竞态（Ctrl+V 后多久恢复，~150ms 量级需实测）（见 04）。
- macOS 端：麦克风权限（Info.plist）+ 可能的「辅助功能/输入监控」权限（enigo 注入、全局热键），Windows 无此限制（见 02/06）。
- 模型在具体 Windows CPU 上的实测延迟（RTF）未验证，base 档量级可接受但需实跑确认（见 01）。

## 文件清单（本目录 research/）

- `01-stt-engine-selection.md` —— whisper-rs vs sherpa-onnx，模型档位/体积/下载
- `02-audio-capture.md` —— cpal 采音 + 16kHz 重采样 + macOS 权限
- `03-windows-build-risk.md` —— whisper-rs Windows 编译风险（成败关键，单独看）
- `04-text-injection.md` —— 剪贴板+Ctrl+V vs 模拟键入，中文坑，恢复
- `05-model-download-and-progress.md` —— reqwest 流式下载 + 进度 + deployEnabled 接线
- `06-hotkey.md` —— tauri-plugin-global-shortcut，toggle vs push-to-talk
- `07-pet-status-and-reuse.md` —— 小人状态机复用 + 设置范式 + keychain
- `08-summary-and-recommendation.md` —— 本文件（综合结论）
