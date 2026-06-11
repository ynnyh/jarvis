# Research: 本地 STT 引擎选型（whisper-rs vs sherpa-onnx）

- **Query**: 本地 STT 引擎能否在 Windows 顺利编进 Tauri，中文识别质量，模型档位与体积
- **Scope**: external（crate 元数据已通过 sparse index 核实）
- **Date**: 2026-06-07

## 结论先行（推荐 v1 选型）

**v1 推荐 `whisper-rs`（whisper.cpp 的 Rust 绑定），默认模型 `ggml-base.bin`（约 142 MB，q5_1 量化版约 57 MB），中文混输优先选 `ggml-small`（约 466 MB，q5_1 约 181 MB）。**

理由：
1. whisper.cpp 是目前本地 STT 的事实标准，中文识别质量在 base/small 档已可用，纯 CPU 推理在普通 Windows 笔记本上 base 档约能做到 1-3x 实时（10s 音频 3-10s 出文字），可接受。
2. `whisper-rs` 把 whisper.cpp 通过 `whisper-rs-sys` **静态编进** Rust 二进制，无需运行时分发额外 DLL，符合「轻量 + 隐私 + 本地」定位。
3. 模型是单文件 `.bin`（GGML 格式），从 Hugging Face 直接下载，配合本任务「按需下载」需求天然契合（见 `05-model-download-and-progress.md`）。
4. sherpa-onnx 作为**备选**：体积更小、原生支持流式、但集成复杂度高、需带 ONNX Runtime，中文模型生态不如 whisper 统一。v1 不选，列为后续优化项。

## Findings

### 选型对比表

| 维度 | whisper-rs (whisper.cpp) | sherpa-onnx |
|---|---|---|
| crate 最新版（已核实） | `whisper-rs 0.16.0`，依赖 `whisper-rs-sys ^0.15` | `sherpa-onnx 1.13.2`（Rust 绑定） |
| 集成方式 | whisper.cpp C++ 源码随 `-sys` crate 编进二进制（cc/cmake） | 需 ONNX Runtime（onnxruntime 动态/静态库），绑定层更厚 |
| Windows 编译 | **需 C/C++ 工具链**（见下文风险节）；CPU 后端纯源码编译，无外部依赖 | 需处理 onnxruntime 预编译库的链接/分发，Windows 上更易踩链接坑 |
| 中文质量 | base 可用、small 较好、medium 好（体积代价大） | 取决于所选模型（如 zipformer/paraformer 中文模型），paraformer 中文不错 |
| 流式 | 原生不支持真流式（whisper 是整段/分块模型）；可切窗口模拟 | **原生支持流式**（streaming zipformer），低延迟 |
| 模型分发 | 单文件 `ggml-*.bin`，HF 直链，最易做按需下载 | 多文件（encoder/decoder/tokens.txt 等），打包更碎 |
| 体积（推理库本身） | 编进二进制，增量约几 MB（CPU 后端） | onnxruntime 体积较大（几十 MB 量级） |
| 推荐度（本任务 v1） | **首选** | 备选（流式诉求强时再考虑） |

### whisper-rs 0.16.0 关键事实（已通过 crates 稀疏索引核实）

- `features`：`default = []`（**默认纯 CPU，无 GPU**）。可选 GPU 后端：`cuda` / `metal`（macOS）/ `coreml`（macOS）/ `vulkan`(经 `_gpu`) / `hipblas` / `openblas` / `openmp`。
- 依赖：`whisper-rs-sys ^0.15`（这个 `-sys` crate 内含 whisper.cpp 源码，用 `cc`/`cmake` 在 build 时编译）。
- 可选 deps：`libc`、`log`、`tracing`（都 optional）。
- v1 **只用 default（CPU）feature**，不碰 CUDA/Vulkan——避免显卡/驱动差异带来的编译与运行不确定性，也最符合「轻量」。

### GGML 模型档位（whisper.cpp 官方模型，HF 仓库 `ggerganov/whisper.cpp`）

| 模型 | 全精度体积 | q5_1 量化体积 | CPU 推理速度（相对） | 中文可用性 | 建议 |
|---|---|---|---|---|---|
| `tiny` | ~75 MB | ~31 MB | 最快 | 中文较弱、易错 | 不建议中文 |
| `base` | ~142 MB | ~57 MB | 快 | 中文「能用」 | **v1 默认下载档** |
| `small` | ~466 MB | ~181 MB | 中 | 中文「较好」 | 中文体验档（推荐给在意效果的用户） |
| `medium` | ~1.5 GB | ~539 MB | 慢 | 中文「好」 | 体积过大，不建议默认 |
| `large-v3` | ~2.9 GB | ~1.1 GB | 很慢（CPU 难用） | 最好 | CPU 不现实，排除 |

> 模型下载地址形如：`https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin`
> 量化版命名形如：`ggml-base-q5_1.bin`。**强烈建议默认下量化版（q5_1）**：体积砍一半多，CPU 速度更快，中文质量损失很小。
> 国内访问 HF 可能不稳，下载源需考虑镜像（如 hf-mirror.com）或让用户自定义——见 `05-model-download-and-progress.md` 的决策点。

### 推荐的默认模型档位（兼顾体积与中文）

- **保守默认**：`ggml-base-q5_1.bin`（约 57 MB）——下载快、首次启用门槛低，中文「能用」。
- **进阶可选**：在设置里允许用户切到 `ggml-small-q5_1.bin`（约 181 MB），中文明显更好。
- v1 可以**只先提供 base（量化）一档**落地，small 作为「设置里可切换的进阶档」二期再做，避免一次把下载/校验/多档管理都堆上。

## Caveats / Not Found

- cpal / arboard 的 sparse-index 版本号 tail 在本环境被截断（返回了陈旧分段），未能确认其确切最新版；但这两者不影响本文件结论（见 `02-audio-capture.md`、`04-text-injection.md`）。
- whisper.cpp 各模型在「具体某型号 Windows CPU」上的实测 RTF（实时因子）未实测，上表是社区经验量级，开发后需在目标机器实跑一次确认延迟可接受。
- 最大风险是 whisper-rs 在 Windows 上的**编译可行性**，单独成文详述：见 `03-windows-build-risk.md`（这是决定成败的一道关）。
