# Research: FunASR 系替换 whisper —— 决策与回报摘要

- **Query**: 综合结论——能不能干、推荐什么、预期速度、改造量、最大风险、值不值得换
- **Scope**: 决策摘要（详细证据见 `09-funasr-sherpa-onnx-migration.md`）
- **Date**: 2026-06-08

## 一句话结论

**值得换，且能干。** 用 **sherpa-onnx 官方 Windows x64 预编译二进制（sidecar 调 `sherpa-onnx-offline.exe`）+ SenseVoice-Small int8 ONNX 模型**，满足 Windows + 纯 CPU + 无 cmake + 无 Python + 国内可下 全部约束。**不建议改用外部开源 app**——现有录音/注入/下载/热键/小人脚手架质量很高、几乎全可复用，换引擎只是「换 sidecar 命令 + 换模型 URL + 换解包格式」，比引入外部 app 更轻、更可控、更贴「本地+轻量+隐私」定位。

## 能不能干（逐约束核对，均已实测）

| 约束 | 满足？ | 依据 |
|---|---|---|
| Windows | ✅ | sherpa-onnx v1.13.2 有官方 `win-x64-shared-MT-Release.tar.bz2`（22MB，实测可下） |
| 纯 CPU 无 GPU | ✅ | `--provider=cpu`；选非 cuda 包；SenseVoice/Paraformer 非自回归、CPU 友好 |
| 无 cmake | ✅ | 用预编译二进制 + 预编译 onnx 模型，全程不编译（唯一新依赖 .tar.bz2 解包，可选纯 Rust bzip2） |
| 无 Python | ✅ | sidecar 调 exe + 现成 onnx，不碰 Python（排除了 funasr-onnx/FunASR 自编 runtime） |
| 国内可下 | ✅ | 模型走 **hf-mirror.com**（本项目已验证可达 + 现成续传/代理逻辑）；二进制走 ghfast.top 镜像兜底 |

## 推荐（引擎 + 模型 + 下载源）

- **引擎**：sherpa-onnx v1.13.2，sidecar 调 `sherpa-onnx-offline.exe`
  - 二进制：`https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.2/sherpa-onnx-v1.13.2-win-x64-shared-MT-Release.tar.bz2`（22MB，静态 CRT 无运行库依赖）+ ghfast.top 镜像兜底
- **默认模型**：**SenseVoice-Small int8**（中英日韩粤多语，中英混说强，自带标点）
  - `https://hf-mirror.com/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/model.int8.onnx`（228MB）
  - `.../resolve/main/tokens.txt`（308KB）
  - 命令：`sherpa-onnx-offline --tokens=tokens.txt --sense-voice-model=model.int8.onnx --num-threads=N --sense-voice-use-itn=1 --debug=0 foo.wav`（`use-itn=1` → 输出带标点；不传 language = auto，自动处理中英混说）
- **轻量备选模型**（设置里可切）：**Paraformer-zh-small int8**（78MB，纯中文场景）
  - `https://hf-mirror.com/csukuangfj/sherpa-onnx-paraformer-zh-small-2024-03-09/resolve/main/model.int8.onnx` + `tokens.txt`

## 预期速度（对比 whisper 18s）

- **架构性结论（确定）**：SenseVoice/Paraformer 是**非自回归**模型，**没有 whisper 的「大编码器 + 自回归解码」结构** → **彻底消灭那个 17s encode 包袱**。
- **量级预期（推断，待实测）**：一句 2.5s 音频，纯 CPU 预期 **亚秒级 ~ 1~2s**（社区经验 RTF≈0.07~0.15）。即 **18s → ~1s 量级，约 10~20 倍提速**。
- **额外利好**：228MB int8 模型加载比 574MB whisper-turbo 更快；sherpa stderr 会直接打 RTF，实测一目了然。
- **准度**：FunASR/达摩院中文 SOTA 系，中文短句明显强于 whisper-base/small；SenseVoice 对「中文夹英文术语」原生支持，根治当时「测试→色试」的中文准度问题。

## 改造工作量

- **约 2~2.5 人时**（改动 ~120~180 行）。
- **原样复用（大头）**：录音、16k 单声道 WAV、注入、**下载续传+代理+镜像+302 全套**、全局热键、小人状态、Tauri 命令骨架。
- **要改（集中 3 处）**：
  1. STT 命令行（whisper 参数 → sherpa 参数）；
  2. 输出解析（读 `.txt` 文件 → 解析 stdout 的 JSON `text` 字段）；
  3. 解包（`.zip` → `.tar.bz2`，唯一新依赖）。
- **净删**：whisper 的 prompt/术语偏置那一坨（SenseVoice 不需要）。
- 详见 `09-...md` 第 6 节的逐项改造表 + voice.rs 行号引用。

## 最大风险（按严重度）

1. **`.tar.bz2` 解包新依赖能否在无 cmake 环境编过**（中）：优先选纯 Rust `bzip2-rs` 规避任何 C 编译；`tar` crate 是纯 Rust 无虞。PR 里需验证编译通过。
2. **win-x64 包内精确 exe/dll 文件名未逐字节核对**（中）：`sherpa-onnx-offline.exe` 来自源码/文档约定，可信但未解包实证；实现时本地解包核对一次。
3. **干净 Windows 上 DLL/CRT 依赖**（低-中）：选 `-MT-`（静态 CRT）规避 VC++ 运行库；ONNX Runtime DLL 随包平铺即可。无开发环境机器上验证一次。
4. **端到端速度未实测**（低）：架构上 17s encode 必然消失，只是具体落到几百 ms 还是 1~2s 待实跑；属「好到什么程度」而非「行不行」的风险。
5. **ModelScope 原生源未定位**（低，非阻塞）：已有 hf-mirror（验证可用）兜底，ModelScope 只是「更快」的锦上添花。

## 不建议「直接用外部开源 app」的理由

- 现有脚手架（cpal 录音 / 16k WAV / arboard+enigo 注入 / 续传+代理+镜像下载 / 全局热键 / 小人状态 / 默认关开关 / 设置页）**已经是完整可用的产品级实现**，换引擎只动其中一小段。
- 外部 app 无法与「点桌面小人 / 注入当前聚焦输入框 / 全局热键 / 本地隐私」深度集成，且违背 Jarvis「本地优先的轻量个人助手」定位。
- sherpa-onnx sidecar 与现有 whisper-cli sidecar 形态同构，迁移成本远低于接入并维护一个外部 app。

## 详细证据

见同目录 `09-funasr-sherpa-onnx-migration.md`（含所有实测 HTTP 结果、源码行号、URL、体积、许可证、逐项改造表与 caveats）。
