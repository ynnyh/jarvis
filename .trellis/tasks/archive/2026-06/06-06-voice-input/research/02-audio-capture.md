# Research: 录音 / 音频采集（cpal + 重采样）

- **Query**: Rust cpal 采音；whisper 需要 16kHz 单声道 f32，是否要重采样、如何做；macOS 麦克风权限
- **Scope**: external + 少量内部（Cargo.toml 现状）
- **Date**: 2026-06-07

## 结论先行

- 用 **`cpal`** 做跨平台麦克风采集（Windows WASAPI / macOS CoreAudio 都覆盖）。
- whisper.cpp 要求 **16 kHz、单声道、f32（[-1,1]）** 的 PCM。麦克风默认采样率通常是 44.1k/48k、可能是多声道、样本格式可能是 i16/f32 —— **几乎一定要做格式转换 + 重采样**。
- 重采样推荐用 `rubato`（高质量、纯 Rust）或简单线性插值（够用且零额外重依赖）。v1 可先用简单降采样（48k→16k 整数倍场景）+ 单声道混合，省一个依赖。
- macOS 必须配 **麦克风权限**（Info.plist 的 `NSMicrophoneUsageDescription` + entitlement）；Windows 一般无需显式权限（系统级麦克风隐私开关由用户控制）。

## Findings

### 当前仓库没有任何音频依赖

`src-tauri/Cargo.toml`（已读，无 cpal/hound/rubato）。需要新增：
- `cpal`（采集）
- 可选 `rubato`（重采样）或自己写线性插值
- 可选 `hound`（写 WAV，仅调试期用来 dump 录音核对，可不进生产）

### cpal 采集要点

- `cpal` 通过 `Host -> Device -> StreamConfig` 拿默认输入设备，`build_input_stream` 注册回调，回调里拿到 `&[f32]` 或 `&[i16]`（取决于 `SampleFormat`）。
- 回调在**音频线程**触发，不能阻塞、不能持锁久 —— 经验做法：回调里只把样本 push 进一个 `Arc<Mutex<Vec<f32>>>` 或 ringbuffer / channel，转写在另一个线程做。
- 这个「音频线程只搬运、处理在别处」的模式，与本仓库现有「`tauri::async_runtime::spawn` + channel」风格一致（参考 `src-tauri/src/channels/` 的后台任务写法）。

### 必做的格式归一化（喂给 whisper 前）

1. **多声道 → 单声道**：把每帧的多个声道求平均（或取首声道）。
2. **样本格式 → f32**：i16 需 `/ 32768.0` 归一到 [-1,1]；cpal 也能直接要 f32 流。
3. **重采样到 16 kHz**：
   - 48000 → 16000 是整数倍（每 3 个样本取 1，配低通更稳），实现最简单。
   - 44100 → 16000 非整数倍，需要分数重采样（`rubato` 或线性插值）。
   - **v1 务实做法**：优先请求设备用 16k 单声道（部分设备支持 `StreamConfig` 直接指定）；不支持时按上面降采样。若想稳妥跨设备，直接上 `rubato` 一步到位。

### 录音触发模型与本任务的关系

- 录音的开始/结束由热键或点小人控制（见 `06-hotkey.md`）。
- 录音中要给前端小人反馈「正在听」状态（见 `07-pet-status-and-reuse.md` 的 state 机制）。
- 录完整段再转（whisper 非流式），所以采集层只需：开始录 → 持续 push 样本 → 停止 → 把整段 buffer 交给转写。不需要在采集层做流式切片（除非二期上 sherpa 流式）。

### macOS 麦克风权限（顾及但非主力平台）

- 必须在 app bundle 的 `Info.plist` 写 `NSMicrophoneUsageDescription`（一句中文说明用途），否则首次采音直接崩或被系统拒。
- Tauri 2 的 macOS 打包配置在 `tauri.conf.json` 的 bundle 段；entitlements 可能要额外文件。**这是 macOS 端唯一的硬性前置**，Windows 无对应项。
- 首次调用采音会触发系统弹窗授权，需处理「用户拒绝」的降级（提示用户去系统设置开权限）。

## Caveats / Not Found

- cpal 确切最新版本号本环境 sparse-index 查询被截断，未确认；开发时 `cargo add cpal` 取最新即可。cpal API 在 0.13→0.15 之间有过签名调整（`build_input_stream` 参数、错误回调），落地时以实际拉到的版本文档为准。
- 未实测目标 Windows 机器默认输入设备的采样率/声道数 —— 这决定要不要分数重采样，开发时打印一次 `default_input_config()` 即可确定。
- 是否要做 VAD（静音检测自动停录）属于体验增强，v1 可不做（按住说话/再按停录即可），列为后续。
