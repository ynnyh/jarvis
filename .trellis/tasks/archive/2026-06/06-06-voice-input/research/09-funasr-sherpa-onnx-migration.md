# Research: 用 FunASR 系（Paraformer / SenseVoice）经 sherpa-onnx 替换 whisper 做本地语音转写

- **Query**: Windows + 纯 CPU + 无 cmake + 无 Python + 国内可下 的约束下，能否用 sherpa-onnx 预编译二进制 sidecar 调 SenseVoice/Paraformer 替掉 whisper-cli；预编译包/模型下载源/速度/准度/许可证/改造量
- **Scope**: external（GitHub releases / ModelScope / HF-mirror / sherpa-onnx 源码 与文档，均已实测 HTTP 核实）+ internal（对照现有 `src-tauri/src/voice.rs`）
- **Date**: 2026-06-08

## 结论先行（一句话）

**能干，且强烈建议换。** sherpa-onnx 提供官方 **Windows x64 预编译二进制 + 预编译 ONNX 模型**，可像现在 sidecar 调 `whisper-cli.exe` 一样直接调 `sherpa-onnx-offline.exe`，**无需 cmake / clang / Python**。模型走 **hf-mirror.com（已被本项目验证可达）/ ModelScope（国内原生）** 下载。SenseVoice/Paraformer 都是**非自回归**架构，**没有 whisper 那个 17s encode 包袱**，纯 CPU 上一句话预期 **亚秒级～1～2s**（对比 whisper 的 18s）。许可证 **Apache-2.0 / MIT**，可商用。现有录音/重采样/注入/下载（续传+代理+镜像）脚手架几乎全可复用，主要改 STT 调用那一段（命令行参数 + 输出从「读 .txt」改成「解析 stdout JSON」+ 资产文件名/URL）。

---

## 1. 引擎 / 集成方式（核心：能不能 sidecar）

### sherpa-onnx 有官方 Windows x64 预编译二进制 —— 已核实

- 最新版 **v1.13.2**（与本项目 research 01 里提到的 Rust crate `sherpa-onnx 1.13.2` 同版）。
  - `GET https://github.com/k2-fsa/sherpa-onnx/releases/latest` → 302 到 `releases/tag/v1.13.2`（已核实）。
- release 资产里有大量 `sherpa-onnx-v1.13.2-win-x64-*.tar.bz2`（**已核实存在并可下载**）。我们要的是**带可执行文件的 shared 包**（不是 `-lib` 结尾的纯库包）：

| 资产文件名 | Content-Length（实测 HEAD） | CRT 链接 | 用途 |
|---|---|---|---|
| `sherpa-onnx-v1.13.2-win-x64-shared-MT-Release.tar.bz2` | **23,368,301 B ≈ 22.3 MB** | **静态 CRT（/MT）→ 不依赖 VC++ 运行库** | **推荐**：解压即用，分发最省心 |
| `sherpa-onnx-v1.13.2-win-x64-shared-MD-Release.tar.bz2` | 19,164,500 B ≈ 18.3 MB | 动态 CRT（/MD）→ 需目标机有 VC++ Redist | 备选（体积略小但有运行库依赖） |
| `sherpa-onnx-v1.13.2-win-x64-shared-MT-Release-lib.tar.bz2` | — | 静态 CRT | **不要**：`-lib` 是纯库（无 exe），给开发链接用的 |
| `sherpa-onnx-v1.13.2-win-x64-cuda.tar.bz2` | — | — | **不要**：GPU 版，我们纯 CPU |

> 实测：`GET .../v1.13.2/sherpa-onnx-v1.13.2-win-x64-shared-MT-Release.tar.bz2` 返回 302 → `release-assets.githubusercontent.com` → 200 + `Content-Length: 23368301`。
> 下载直链形态：`https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.2/<asset>.tar.bz2`

**包内容**（据 sherpa-onnx 官方 release 约定，`shared` 包 = `bin/` 下的全部可执行 + ONNX Runtime 动态库）：`sherpa-onnx-offline.exe`（我们要的离线识别 CLI）+ 同目录的 `sherpa-onnx.dll` / `onnxruntime.dll` 等运行库，平铺解压到 `voice_dir()` 即可被子进程调用——**和现在 whisper-cli.exe + 一堆 dll 平铺在 `~/.jarvis/voice/` 的形态完全一致**。

> ⚠️ 验证缺口：未逐字节解包确认 `bin/` 内的精确 exe/dll 文件名清单（环境无法解 .tar.bz2 且无权拉 windows 安装文档原文）。`sherpa-onnx-offline.exe` 这个名字来自源码与文档里的 `./build/bin/sherpa-onnx-offline` 调用约定，可信度高；实现时解包一次核对实际文件名（exe 是 `sherpa-onnx-offline.exe`，DLL 可能含 `sherpa-onnx-c-api.dll` / `sherpa-onnx-core.dll` / `onnxruntime.dll` 等，全平铺即可）。

### 它支持 Paraformer 和 SenseVoice —— 已核实（源码级）

`sherpa-onnx-offline` 的 usage（源码 `sherpa-onnx/csrc/sherpa-onnx-offline.cc`，已读）明确支持多种离线模型，含 **Paraformer**、**Whisper**、**NeMo**、**Moonshine**、**FunASR-nano** 等；**SenseVoice** 通过独立 flag `--sense-voice-model` 支持（源码 `offline-sense-voice-model-config.cc` 已读）。

### 集成方式推荐：**「下个预编译 .tar.bz2 + 预编译 ONNX 模型，sidecar 调 sherpa-onnx-offline.exe」**

完全复刻现有 whisper-cli sidecar 范式，唯三差异：
1. **压缩格式从 `.zip` 变 `.tar.bz2`**（现有 `extract_whisper_zip` 用 `zip` crate，需换/补 tar+bzip2 解包，见改造节）。
2. **命令行参数不同**（见下）。
3. **输出从「`-otxt` 写 .txt 文件」变「stdout 打 JSON」**（见下）。

### 备选 funasr-onnx？不推荐

`funasr-onnx`（FunASR 自带的 Python onnx runtime）是 **Python 包**，要 Python 环境，违背「无 Python」约束。FunASR 官方的 C++ runtime（`runtime/onnxruntime`）需自己 cmake 编，违背「无 cmake」。**sherpa-onnx 是唯一满足「无 cmake + 无 Python + 有 Windows 预编译 + sidecar 可调」的路径。**

---

## 2. 模型选型：SenseVoice-Small vs Paraformer

两者都是**非自回归**（CTC/类 CTC 一次性出全句），**根本没有 whisper 那种「大编码器 + 自回归解码」的 17s 包袱**。预编译 ONNX 模型托管在 sherpa-onnx 的 `asr-models` release（已核实可下）+ HF 镜像（已核实文件树）。

### 关键模型对比（体积均为实测 HEAD）

| 维度 | **SenseVoice-Small（int8）** | **Paraformer-zh-small（int8）** | Paraformer-zh int8（大，2025-10-07） |
|---|---|---|---|
| sherpa 资产仓库 | `sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17` | `sherpa-onnx-paraformer-zh-small-2024-03-09` | `sherpa-onnx-paraformer-zh-int8-2025-10-07` |
| **裸 `model.int8.onnx` 体积** | **239,233,841 B ≈ 228 MB** | **81,828,675 B ≈ 78 MB** | — |
| `tokens.txt` 体积 | 315,894 B ≈ 308 KB | 75,352 B ≈ 74 KB | — |
| 打包 tarball（.tar.bz2，含 test_wavs） | int8 专包 **163 MB**（`...-int8-2024-07-17.tar.bz2`，实测 163,002,883 B）；全量包 1.05 GB（含 fp32+int8，**别用**） | 全量 **78 MB**（实测 77,920,048 B，只含 int8） | 228 MB（实测 228,262,632 B） |
| 语言 | **中英日韩粤 5 语**（`auto/zh/en/ja/ko/yue`） | 中文（含部分中英混、方言） | 中文为主 |
| **中英混说** | **原生强**（多语模型，`--sense-voice-language=auto` 自动判）——契合「技术人员中文夹英文术语」 | 一般（中文模型，英文术语弱于 SenseVoice） | 一般 |
| 自带标点 | **是**，`--sense-voice-use-itn=1` 开 ITN → 输出**带标点**（文档原文确认）；含逆文本归一（数字/日期规整） | 否（需另接标点模型） | 否 |
| 内置 VAD | 否（VAD 是 sherpa 另外的 silero-vad 模型，离线整段转写**不需要** VAD） | 否 | 否 |
| CPU 速度 | 非自回归，RTF 远小于 1；社区常报单核 RTF≈0.07~0.15（10s 音频 < 1s）。一句话 2.5s 预期 **亚秒级** | 更小更快，预期同量级或更快 | 比 small 慢但更准 |
| 中文准度 | 高（FunASR/达摩院中文 SOTA 系，明显强于 whisper-base/small，接近/超 large 在中文短句上的体感） | 高（纯中文场景很强） | 最高（新版） |

### 推荐：**默认 SenseVoice-Small（int8）+ `--sense-voice-use-itn=1` + language=auto**

理由（直接对症用户痛点）：
1. **中英混说**：用户是技术人员，中文夹 API/Docker/commit 等英文术语，SenseVoice 多语模型 + auto 远比纯中文 Paraformer 稳；也比 whisper「锁 zh + prompt 偏置」那套 hack 干净。
2. **自带标点**：ITN 直接出带标点文本，省掉再接标点模型；whisper 方案当时还在靠 prompt 凑。
3. **速度**：非自回归，彻底消灭 17s encode；228MB int8 模型加载也比 574MB whisper-turbo 快。
4. **体积可接受**：裸模型 228MB（比当前 whisper-turbo 574MB 还小）；想更省可上 **Paraformer-zh-small（78MB）** 作为「轻量档/纯中文档」二选项。

> 备选策略：**默认 SenseVoice（混说+标点最优）**，设置里给「纯中文轻量档 = Paraformer-zh-small 78MB」。与现有「设置里可切模型」的二期思路一致。

---

## 3. 模型下载（国内）—— 已核实 URL 形态

预编译 ONNX 模型有两条国内可达路径，**正好解决之前 HF 被墙的痛**：

### 路径 A：hf-mirror.com（本项目已验证可用，最稳妥，直接复用现有下载逻辑）

按文件逐个下（**只下 int8 模型 + tokens，不下 1GB 全量包**）：
```
# SenseVoice-Small（推荐默认）
https://hf-mirror.com/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/model.int8.onnx   # 228MB（实测 239233841）
https://hf-mirror.com/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/tokens.txt        # 308KB（实测 315894）

# Paraformer-zh-small（轻量备选）
https://hf-mirror.com/csukuangfj/sherpa-onnx-paraformer-zh-small-2024-03-09/resolve/main/model.int8.onnx           # 78MB（实测 81828675）
https://hf-mirror.com/csukuangfj/sherpa-onnx-paraformer-zh-small-2024-03-09/resolve/main/tokens.txt                # 74KB（实测 75352）
```
- 已核实：上述 HF 仓库的文件树（`/api/models/...` 返回 `model.int8.onnx` / `model.onnx` / `tokens.txt`）+ resolve HEAD 返回正确 Content-Length。
- ⚠️ 与 whisper 同坑：hf-mirror 对大文件可能 302 跳到 HF 美国 Xet 存储（cas-bridge.xethub.hf.co），需**断点续传 + 走代理**——**而这两道保险现有 `voice.rs` 下载逻辑已经实现**（`download_to_file` / `fetch_with_redirects`），原样复用即可。
- 注意：`model.onnx`（fp32）在镜像上常是 1KB 的 LFS 指针（实测 SenseVoice 的 model.onnx 仅 1014 B、Paraformer-small 的仅 15 B）——**只下 `model.int8.onnx`，别下 `model.onnx`**。

### 路径 B：GitHub asr-models release（整包，国内需镜像/代理）

```
https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2024-07-17.tar.bz2   # 163MB（实测 163002883）
https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-paraformer-zh-small-2024-03-09.tar.bz2                 # 78MB（实测 77920048）
```
- 与二进制同源（GitHub），可复用现有 `ghfast.top` 镜像兜底策略（`WHISPER_BIN_URLS` 那套）。
- 整包是 .tar.bz2，含 test_wavs，要解包。**不如路径 A 逐文件下干净**。

### 路径 C：ModelScope（魔搭，国内原生最快）—— 部分可用，需实现时再定位

- **原始模型** `iic/SenseVoiceSmall` 在 ModelScope **确实存在**（实测 `/api/v1/models/iic/SenseVoiceSmall` 返回 Code 200，许可证 **Apache-2.0**），但它是 **PyTorch 原始权重 + config.yaml/tokens.json**，**不含 sherpa 要的 `model.int8.onnx`**（实测文件树只见 config.yaml / tokens.json，无 onnx）——**不能直接喂 sherpa**，要自己用 Python 导出（违背无 Python 约束）。
- sherpa 作者（csukuangfj）的**预编译 onnx 包在 ModelScope 上未找到**（试了 `csukuangfj/...`、`pkufool/...`、`manyeyes/...` 均 404；ModelScope 搜索 API 也没命中同名仓库）。
- **结论**：国内下载**首选路径 A（hf-mirror，已验证 + 复用现成续传/代理逻辑）**；ModelScope 作为「原生最快」理论上更优，但需实现时再搜准 sherpa-ready onnx 的魔搭仓库（可能在某第三方 space 下），**没确认到之前不要押注**。

---

## 4. 音频要求 —— 比 whisper 更宽松（利好）

- sherpa-onnx-offline 文档原文（源码 usage，已读）：
  > "foo.wav should be of single channel, 16-bit PCM encoded wave file; **its sampling rate can be arbitrary and does not need to be 16kHz.**"
- 即：**单声道 + 16-bit PCM WAV 即可，采样率任意**（sherpa 内部自己重采样）。
- 对照现有 `write_temp_wav`：产出正是 **16k 单声道 16-bit PCM**（`hound::WavSpec { channels:1, bits_per_sample:16, sample_format:Int }`）——**完全满足，零改动**。
- **重采样可以不用我们自己做了**：现有 `resample_to_16k` 是「较糙的线性插值」，之前担心影响准度；改用 sherpa 后，**可以直接把设备原始采样率的单声道 16-bit WAV 丢给 sherpa，让它内部高质量重采样**，反而比我们线性插值更准。不过保留现有 16k 重采样也无害（sherpa 收到 16k 就不再重采样）。→ 「线性插值较糙」这个准度隐患可顺手消除。

---

## 5. 许可证 —— 可商用打进桌面 app

| 组件 | 许可证 | 来源（已核实） | 商用打包 |
|---|---|---|---|
| sherpa-onnx（代码 + 预编译二进制） | **Apache-2.0** | `raw.githubusercontent.com/k2-fsa/sherpa-onnx/master/LICENSE` 首行 "Apache License Version 2.0" | ✅ 可 |
| FunASR（训练框架） | **MIT** | `raw.githubusercontent.com/modelscope/FunASR/main/LICENSE` 首行 "MIT License Copyright (c) 2025 FunASR" | ✅ 可 |
| SenseVoiceSmall 模型 | **Apache-2.0**（按 ModelScope 原始仓库） | `iic/SenseVoiceSmall` 的 `/api/v1/models` 返回 `"License":"Apache License 2.0"` | ✅ 可 |
| Paraformer-zh 模型 | 随 FunASR/ModelScope（一般 Apache-2.0） | sherpa 的 HF 镜像未显式标 license（"other"/空），但模型源自 FunASR（MIT 框架 + Apache 模型） | ✅ 基本可（见 caveat） |
| ONNX Runtime（随包 DLL） | **MIT**（微软） | 常识，sherpa 预编译已含 | ✅ 可 |

⚠️ **caveat**：HF 上 `FunAudioLLM/SenseVoiceSmall` 的 license 字段显示 `"other"`（指向模型卡里的自定义说明），而 ModelScope 上 `iic/SenseVoiceSmall` 标的是 Apache-2.0。两者应为同一模型、以官方 ModelScope 的 Apache-2.0 为准；**正式商用发版前建议人工点开模型卡再确认一眼**（whisper 也是 MIT，本来就在商用 app 里用了，风险同量级）。

---

## 6. 改造工作量评估（基于现有 `src-tauri/src/voice.rs`，已逐行读）

### 现有 voice.rs 的结构（sidecar 调 whisper-cli 形态）

链路：`start_recording`(cpal) → `stop_recording`(归一 16k 单声道 f32) → `write_temp_wav`(hound, 16k/mono/16-bit) → `transcribe_wav`(子进程调 whisper-cli) → `inject_text`(arboard+enigo)。
外加：`download_to_file` / `download_to_file_multi` / `fetch_with_redirects`（续传+代理+302）、`extract_whisper_zip`、全局热键（`sync_hotkey` / `on_hotkey_pressed`）、Tauri 命令（`voice_assets_status` / `voice_download_assets` / `voice_start` / `voice_stop_and_transcribe` / `voice_hotkey_sync`）。

### 原样复用（零改 / 几乎零改）—— 占代码量大头

| 模块 | 复用程度 | 说明 |
|---|---|---|
| 录音 `start_recording` / `stop_recording` / `push_samples` / `to_mono` / cpal 全局状态 | **100% 原样** | STT 无关 |
| `write_temp_wav`（16k 单声道 16-bit PCM） | **100% 原样** | 正好符合 sherpa 要求 |
| `resample_to_16k` | 可留可删 | sherpa 能任意采样率，留着无害；想去「线性插值糙」隐患可直接传原始采样率 |
| 注入 `inject_text`（剪贴板+Ctrl+V） | **100% 原样** | STT 无关 |
| 下载 `download_to_file` / `download_attempt` / `fetch_with_redirects` / `resolve_redirect` / `parse_content_range_total` / `download_to_file_multi` / 代理 `download_proxy` / `build_download_client` | **100% 原样** | 续传+代理+镜像+302 这套是最值钱的资产，URL 一换就能用 |
| 全局热键全套（`sync_hotkey` / `on_hotkey_pressed` / `configured_shortcut` / `global_shortcut_plugin` / `emit_voice_state`） | **100% 原样** | STT 无关 |
| 开关/语言/术语读 config（`voice_input_enabled` / `voice_language`） | **基本原样** | `voiceLanguage` 语义微调（见下） |
| Tauri 命令骨架（status/download/start/stop/hotkey） | **结构原样** | 内部实现按下表改 |

### 需要改的点（集中在 STT 调用 + 资产定义）

| # | 改什么 | 现状 → 目标 | 量级 |
|---|---|---|---|
| 1 | **资产常量** | `DEFAULT_MODEL_FILE="ggml-large-v3-turbo-q5_0.bin"` → `model.int8.onnx`；新增 `TOKENS_FILE="tokens.txt"`；`whisper_cli_path()` → `sherpa_offline_path()`(`sherpa-onnx-offline.exe`) | 小 |
| 2 | **下载 URL 常量** | `WHISPER_BIN_URLS` → sherpa win-x64 tarball（ghfast.top + GitHub 直链兜底）；`MODEL_URL` → hf-mirror 的 `model.int8.onnx`；新增 tokens 的 URL；手动兜底 `*_RAW` 同步换 | 小 |
| 3 | **解包** | `extract_whisper_zip`（zip crate）→ 解 **.tar.bz2**。需加 crate：`tar` + `bzip2`（纯 Rust 的 `bzip2-rs` 或 `bzip2`(libbz2-sys 走 cc，但**只编 C 不需 cmake**，可接受) ；或选 `bzip2` 的 rust 实现 `bzip2-rs` 避免任何 C 编译）。zip-slip 防护逻辑照搬 | **中**（唯一新依赖点） |
| 4 | **转写命令** `transcribe_wav` | whisper `-m <model> -f <wav> -t N -l zh --prompt ... -otxt -nt` → sherpa `--tokens=<tokens.txt> --sense-voice-model=<model.int8.onnx> --num-threads=N --sense-voice-use-itn=1 --debug=0 <wav>` | **中** |
| 5 | **输出解析** | whisper：读 `<wav>.txt` 文件 → sherpa：**解析 stdout 的 JSON**（每个 wav 一行 `{"text":"...","tokens":[...],...}`，取 `text` 字段；源码 `offline-stream.cc:400` 确认字段名是 `"text"`）。serde_json 已在依赖里，直接 `from_str` 取 `.text` | **中** |
| 6 | **资产就绪判定** `voice_assets_ready` | `whisper_cli + model` → `sherpa-onnx-offline.exe + model.int8.onnx + tokens.txt` 三者都在 | 小 |
| 7 | **下载编排** `voice_download_assets` | ① 下 sherpa tarball → 解 .tar.bz2；② 下 model.int8.onnx；③ **新增**下 tokens.txt | 小 |
| 8 | **prompt/术语** | whisper 的 `--prompt` + `voiceTerms` + `PROMPT_PREFIX` 这套**整段删掉**——sherpa SenseVoice 不吃 initial prompt；`build_prompt`/`voice_terms`/`PROMPT_PREFIX`/`DEFAULT_VOICE_TERMS`/`MAX_PROMPT_CHARS` 可移除（设置页若已暴露术语输入框，前端要同步删/隐藏） | 小（净删代码） |
| 9 | **语言映射** | `voiceLanguage` 默认从 `zh` 可改为 `auto`（SenseVoice auto 处理中英混说更好）；值域映射到 sherpa 的 `auto/zh/en/ja/ko/yue` | 小 |
| 10 | **`Cargo.toml`** | 加 `tar` + bzip2 解压依赖；`zip` 若不再他用可移除 | 小 |
| 11 | **诊断日志** | whisper 那段 eprintln 改成 sherpa 命令行 + 解析 sherpa stderr 里的 RTF（`Real time factor (RTF): ...`，源码确认会打）——顺手能在日志里看到实测速度 | 小 |

> 估算（按本项目 effort = 1 + sqrt(loc)/10 配方，改动 loc 约 120~180 行：删 prompt 那坨 + 改命令/解析 + 换 tar 解包）：**约 2~2.5 人时**。核心难点只有「.tar.bz2 解包依赖选型」和「stdout JSON 解析」两处，其余是常量替换。

---

## Findings: 关键文件 / 行号引用

### 内部（现有脚手架）

| 文件 | 关键位置 | 说明 |
|---|---|---|
| `src-tauri/src/voice.rs` | 全文 1465 行 | 现有 whisper-cli sidecar 实现，本次改造的唯一主战场 |
| `voice.rs:82-112` | 下载源常量 | `WHISPER_BIN_URLS`/`MODEL_URL`/`*_RAW`——改 URL 的地方 |
| `voice.rs:400-428` | `write_temp_wav` | 16k 单声道 16-bit PCM，符合 sherpa，零改 |
| `voice.rs:493-586` | `transcribe_wav` | 改命令行 + 输出解析的核心函数 |
| `voice.rs:678-950` | 下载全套 | 续传+代理+302+多源，100% 复用 |
| `voice.rs:957-998` | `extract_whisper_zip` | 改成解 .tar.bz2 |
| `voice.rs:460-471, 447-458, 54-63` | prompt/术语 | sherpa 不需要，整段删 |
| `voice.rs:1180-1350` | Tauri 命令 | 骨架复用，内部按上表改 |

### 外部（已核实直链）

- sherpa-onnx 二进制：`https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.2/sherpa-onnx-v1.13.2-win-x64-shared-MT-Release.tar.bz2`（22.3MB）
- SenseVoice 模型：`https://hf-mirror.com/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/model.int8.onnx`（228MB）+ `.../tokens.txt`（308KB）
- Paraformer-small 模型：`https://hf-mirror.com/csukuangfj/sherpa-onnx-paraformer-zh-small-2024-03-09/resolve/main/model.int8.onnx`（78MB）+ `.../tokens.txt`（74KB）
- 命令范式（文档原文）：`sherpa-onnx-offline --tokens=tokens.txt --sense-voice-model=model.int8.onnx --num-threads=N --sense-voice-use-itn=1 --debug=0 foo.wav`
- 许可证：sherpa-onnx Apache-2.0、FunASR MIT、SenseVoiceSmall（ModelScope）Apache-2.0

## Caveats / Not Found

1. **未实测端到端速度**：SenseVoice/Paraformer 在「目标 Windows 纯 CPU 机器」上的真实 RTF 未实跑，2.5s 音频「亚秒～1~2s」是基于「非自回归 + 社区经验 RTF 0.07~0.15」的推断；但**「消灭 17s encode」这个结论是架构性的、确定的**（whisper 的 encode 慢源于大编码器自回归，SenseVoice/Paraformer 架构上没有）。实现后跑一次看 sherpa stderr 打的 RTF 即可证实。
2. **win-x64 tarball 内精确文件名清单未逐字节核对**：`sherpa-onnx-offline.exe` 名字来自源码/文档约定，DLL 清单未确认（环境不能解 .tar.bz2、且 windows 安装文档原文拉取被拒）。实现时本地解包一次核对（影响 `sherpa_offline_path()` 与「平铺哪些 dll」）。
3. **ModelScope 的 sherpa-ready onnx 仓库未定位到**：国内原生最快的 ModelScope 路径，没找到 csukuangfj 预转 onnx 的确切魔搭仓库（原始 `iic/SenseVoiceSmall` 不含 onnx）。**默认走已验证的 hf-mirror（路径 A）**；想要 ModelScope 原生加速需实现时再搜（非阻塞）。
4. **SenseVoice 模型 license 在 HF 标 "other"**：以 ModelScope 官方 `iic/SenseVoiceSmall` 的 Apache-2.0 为准，但商用发版前建议人工再确认模型卡条款一眼。
5. **.tar.bz2 解包依赖**：纯 Rust bzip2（`bzip2-rs`）可彻底避免 C 编译；若用 `bzip2` crate 默认走 `libbz2-sys`（需 cc 编 C，但**不需要 cmake**，本项目当时排除的是 cmake/libclang，cc 一般 OK——但稳妥起见优先纯 Rust 实现）。这是唯一新增依赖，需在 PR 里验证能在无 cmake 环境编过。
6. **未实测 sherpa 二进制在干净 Windows 上能否直接跑**（缺 DLL / CRT 问题）：选 `-MT-`（静态 CRT）版可最大程度避免 VC++ 运行库依赖；但 ONNX Runtime DLL 必须随包平铺。实现后在无开发环境的机器上验证一次。
