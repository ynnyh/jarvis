// 轻量语音输入：录音→转写→注入聚焦输入框（本地、轻量、隐私）。
//
// 链路：start_recording（cpal 采音）→ stop_recording（停流、归一到 16kHz 单声道 f32）
//   → 写临时 WAV（hound）→ sherpa-onnx-offline 子进程转写（SenseVoice 模型）→ arboard+enigo 注入。
//
// ---- 关键约束（已实测决定）----
// 本机 **没有 CMake / clang / cc**，CI 也没有。所以 STT 走预编译命令行二进制当子进程调用，
// 全程不现编任何 C/C++。引擎与模型都假定资产已在约定路径下（`~/.jarvis/voice/`），
// 缺资产时报「没就绪」、由设置页引导下载。
//
// ---- 为什么从 whisper 换成 sherpa-onnx + SenseVoice ----
// whisper large-v3-turbo 纯 CPU 实测一句话 ~18s（光 encode 就 ~17s）、中文还不够准。
// SenseVoice/Paraformer 是**非自回归**模型，架构上没有 whisper 那个大编码器自回归解码的包袱，
// 纯 CPU 上一句话亚秒～1~2s，中文 + 中英混说强、**自带标点**（ITN）。sherpa-onnx 提供官方
// Windows x64 预编译二进制（`sherpa-onnx-offline.exe`）+ 预编译 ONNX 模型，sidecar 调用即可，
// 无需 cmake/Python；模型走 hf-mirror（国内可达）。SenseVoice 多语自动判 + 自带标点，
// 不再需要 whisper 那套「锁语言 + initial prompt 偏置术语」的 hack，故一并删除。
//
// ---- 全局录音状态 ----
// 参考 mcp_client.rs 的 `once_cell::Lazy` + `Arc<Mutex<..>>` 全局单例范式：维护一个全局
// VOICE_STATE。cpal 的回调在音频线程触发，只往共享 buffer push 样本（不阻塞、不持锁久）。
// cpal `Stream` 在 WASAPI/CoreAudio 上是 Send（自带音频线程持有 COM 对象），故可存进全局
// Mutex，录音期间靠 buffer 累积、stop 时 drop 掉 Stream 收尾。

#![allow(dead_code)] // 部分路径/工具函数供下载/设置接入时才被调用

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat};
use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use once_cell::sync::Lazy;
use serde_json::{json, Value};

use crate::settings::jarvis_dir;

/// 写 WAV 的目标采样率：16 kHz 单声道。
/// sherpa-onnx 其实能吃任意采样率（内部自己重采样），这里仍归一到 16k 既兼容又省体积。
const TARGET_SAMPLE_RATE: u32 = 16_000;

/// 全局触发热键（toggle）的**默认值**。用户没配 / 配的无效时回退到它。
/// `CommandOrControl+Shift+Space`：macOS 上是 ⌘，Windows/Linux 上是 Ctrl，
/// 加 Shift+Space 这组不易和常见软件冲突。实际生效键位读 config 的 `voiceHotkey`。
const DEFAULT_HOTKEY: &str = "CommandOrControl+Shift+Space";

/// SenseVoice 量化模型文件名（sherpa-onnx 官方预编译的 int8 ONNX，约 228MB）。
/// 中英日韩粤多语、非自回归、自带标点（配合 --sense-voice-use-itn=1）。
const MODEL_FILE: &str = "model.int8.onnx";

/// SenseVoice 词表文件名（与模型配套，约 308KB）。sherpa 转写必需。
const TOKENS_FILE: &str = "tokens.txt";

// ============================================================================
// 资产下载源（已用 curl 实测核实 URL 可达 + 解包核对内部文件名，见任务 research 09/10）
// ============================================================================
//
// sherpa-onnx-offline 预编译二进制：官方 release 的 win-x64-shared-MT-Release 包（.tar.bz2，22MB）。
//   - 选 `-MT-`（静态 CRT）版：不依赖目标机的 VC++ 运行库，分发最省心。
//   - 实测解包：根目录 `sherpa-onnx-v1.13.2-win-x64-shared-MT-Release/`，其下 `bin/` 含
//     sherpa-onnx-offline.exe + onnxruntime.dll + onnxruntime_providers_shared.dll
//     （MT 版把 sherpa 代码静态链进各 exe，故 bin/ 里没有 sherpa-onnx-core 之类的 DLL，
//      离线 CLI 只需这两个 onnxruntime DLL 作同目录依赖）。解包时只取 bin/ 下的 exe + dll，
//      平铺进 voice_dir，和原 whisper-cli 平铺形态一致。
//
// 二进制走 GitHub，国内直连常超时/被墙。故按顺序尝试一组镜像，谁先连上并下完就用谁：
//   1. ghfast.top 国内加速镜像（实测 HTTP 200 可拉，国内优先）；
//   2. GitHub 直连（海外 / 有代理时兜底）。
const SHERPA_BIN_URLS: &[&str] = &[
    "https://ghfast.top/https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.2/sherpa-onnx-v1.13.2-win-x64-shared-MT-Release.tar.bz2",
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.2/sherpa-onnx-v1.13.2-win-x64-shared-MT-Release.tar.bz2",
];

/// 模型直链：走国内可达的 hf-mirror.com 镜像（csukuangfj 的 SenseVoice sherpa-ready 包）。
/// 实测：HEAD 经 302 跳到 HF 美国 Xet 存储（cas-bridge.xethub.hf.co），认 Range（回 206）。
/// 228MB 从美国直传国内可能断流，故配合「走用户代理 + 断点续传」两道保险才稳（见下方下载逻辑）。
const MODEL_URL: &str =
    "https://hf-mirror.com/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/model.int8.onnx";

/// 词表直链：同仓库的 tokens.txt（实测 200、315894 字节）。
const TOKENS_URL: &str =
    "https://hf-mirror.com/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/tokens.txt";

/// 模型 HF 原始直链（手动兜底用：给用户在浏览器/下载工具里手动下）。
const MODEL_URL_RAW: &str =
    "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/model.int8.onnx";

/// 词表 HF 原始直链（手动兜底用）。
const TOKENS_URL_RAW: &str =
    "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/tokens.txt";

/// sherpa-onnx 二进制 .tar.bz2 的 GitHub 原始直链（手动兜底用，列给用户看）。
const SHERPA_BIN_URL_RAW: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.2/sherpa-onnx-v1.13.2-win-x64-shared-MT-Release.tar.bz2";

/// 单次下载流的 HTTP 超时（连接+读取）。大文件靠断点续传扛断流，单流别设太长，
/// 让卡死的流尽早超时进入续传重试，比死等强。
const DOWNLOAD_TIMEOUT_SECS: u64 = 60;

/// 断点续传最大尝试次数（含首次）。每次失败 backoff 后用当前 .part 大小续传。
const MAX_DOWNLOAD_ATTEMPTS: u32 = 8;

/// 手动跟随 302 的最大跳数（hf-mirror → HF → Xet 存储一般 1~2 跳，给足余量）。
const MAX_REDIRECTS: u32 = 6;

// ============================================================================
// 下载代理 + HTTP client
// ============================================================================

/// 读用户 `~/.jarvis/config.json` 里的 `channels.telegram.proxy`（能翻墙的本地代理，
/// 如 `http://127.0.0.1:7897`）。trim 后非空才返回——voice 下载复用它，让原始 GitHub/HF
/// 源在国内也能稳连。和 channels/telegram.rs 读同一处配置，保持单一数据源。
pub fn download_proxy() -> Option<String> {
    crate::settings::load_raw_config()
        .and_then(|v| {
            v.get("channels")
                .and_then(|c| c.get("telegram"))
                .and_then(|t| t.get("proxy"))
                .and_then(|p| p.as_str())
                .map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty())
}

/// 建下载专用 reqwest client：
/// - `.no_gzip().no_brotli().no_deflate()`：二进制/模型不是压缩流，禁自动解码，避免把原始字节当压缩流解坏。
/// - `redirect(Policy::none())`：**手动**跟 302——reqwest 跨 host 重定向会丢自定义头（Range / Accept-Encoding），
///   续传必须每跳都自己带上，故关掉自动跟随。
/// - 若用户配了代理就 `proxy(all)` 走它（GitHub/HF 原始源在国内才稳）。
fn build_download_client(proxy: Option<&str>) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::none())
        .no_gzip()
        .no_brotli()
        .no_deflate();
    if let Some(proxy) = proxy.map(str::trim).filter(|s| !s.is_empty()) {
        let proxy =
            reqwest::Proxy::all(proxy).map_err(|e| format!("下载代理配置无效: {}", e))?;
        builder = builder.proxy(proxy);
    }
    builder
        .build()
        .map_err(|e| format!("下载 HTTP client 构造失败: {}", e))
}

// ============================================================================
// 资产路径
// ============================================================================

/// 语音资产目录 `~/.jarvis/voice/`（sherpa-onnx-offline 可执行 + onnxruntime DLL + 模型 + 词表都放这）。
pub fn voice_dir() -> PathBuf {
    jarvis_dir().join("voice")
}

/// sherpa-onnx-offline 可执行路径。Windows 用 `sherpa-onnx-offline.exe`，其它平台 `sherpa-onnx-offline`。
pub fn sherpa_offline_path() -> PathBuf {
    #[cfg(windows)]
    let name = "sherpa-onnx-offline.exe";
    #[cfg(not(windows))]
    let name = "sherpa-onnx-offline";
    voice_dir().join(name)
}

/// SenseVoice 模型路径 `~/.jarvis/voice/model.int8.onnx`。
pub fn model_path() -> PathBuf {
    voice_dir().join(MODEL_FILE)
}

/// SenseVoice 词表路径 `~/.jarvis/voice/tokens.txt`。
pub fn tokens_path() -> PathBuf {
    voice_dir().join(TOKENS_FILE)
}

/// 语音资产是否就绪：sherpa-onnx-offline 二进制 + 模型 + 词表三者都在。
pub fn voice_assets_ready() -> bool {
    sherpa_offline_path().is_file() && model_path().is_file() && tokens_path().is_file()
}

// ============================================================================
// 全局录音状态
// ============================================================================

/// 录音运行态：持有 cpal Stream（活着即在采音，drop 即停）、共享样本 buffer、
/// 以及采集到的原始采样率/声道数（stop 时据此归一到 16k 单声道）。
struct Recording {
    /// 活着的输入流；drop 它即停止采集。
    stream: cpal::Stream,
    /// 音频线程往里 push 的原始交错样本（已转 f32）。
    samples: Arc<Mutex<Vec<f32>>>,
    /// 设备原始采样率（如 44100 / 48000）。
    sample_rate: u32,
    /// 设备原始声道数（如 1 / 2）。
    channels: u16,
    /// 设备名（诊断用，stop 时打印）。
    device_name: String,
    /// 采样格式（诊断用，stop 时打印）。
    sample_format: SampleFormat,
}

/// 全局语音状态。`Option<Recording>` 为空表示当前没在录。
/// 用 std `Mutex`（录音控制都在同步上下文，无需 await）。
static VOICE_STATE: Lazy<Mutex<Option<Recording>>> = Lazy::new(|| Mutex::new(None));

// ============================================================================
// 录音：start / stop
// ============================================================================

/// 开始录音：取默认输入设备，按其默认配置建流，回调里把样本累进共享 buffer。
/// 已在录 → Err（避免重复建流）。资产是否就绪由命令层先判，这里只管采音。
pub fn start_recording() -> Result<(), String> {
    let mut guard = VOICE_STATE.lock().map_err(|_| "录音状态锁中毒".to_string())?;
    if guard.is_some() {
        return Err("已经在录音中".to_string());
    }

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "找不到默认麦克风设备".to_string())?;
    // 设备名仅作诊断标签：cpal 0.17 标记 name() 弃用（建议 description()，但那返回结构体、
    // 字段更繁，对一行日志不划算），这里局部 allow 沿用 name()。
    #[allow(deprecated)]
    let device_name = device.name().unwrap_or_else(|_| "<未知设备>".to_string());
    let supported = device
        .default_input_config()
        .map_err(|e| format!("读取麦克风默认配置失败: {}", e))?;

    // cpal 0.17：SampleRate 是 `type SampleRate = u32` 别名，直接拿数值（无 .0）。
    let sample_rate = supported.sample_rate();
    let channels = supported.channels();
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    eprintln!(
        "[voice] 开始录音：设备={}, 采样率={}, 声道={}, 样本格式={:?}",
        device_name, sample_rate, channels, sample_format
    );

    let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let err_fn = |e| eprintln!("[voice] 音频流错误: {}", e);

    // 不同样本格式建对应的流：回调里统一转 f32 累进 buffer。
    // 音频线程只搬运、不做重活，避免阻塞采集（重采样/归一放到 stop 时做）。
    let buf = samples.clone();
    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _| push_samples(&buf, data.iter().copied()),
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _| {
                push_samples(&buf, data.iter().map(|s| s.to_sample::<f32>()))
            },
            err_fn,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            &config,
            move |data: &[u16], _| {
                push_samples(&buf, data.iter().map(|s| s.to_sample::<f32>()))
            },
            err_fn,
            None,
        ),
        other => return Err(format!("不支持的麦克风样本格式: {:?}", other)),
    }
    .map_err(|e| format!("创建音频输入流失败: {}", e))?;

    stream
        .play()
        .map_err(|e| format!("启动音频输入流失败: {}", e))?;

    *guard = Some(Recording {
        stream,
        samples,
        sample_rate,
        channels,
        device_name,
        sample_format,
    });
    Ok(())
}

/// 把一批样本追加进共享 buffer。回调在音频线程，锁竞争极短（只 extend）。
fn push_samples(buf: &Arc<Mutex<Vec<f32>>>, iter: impl Iterator<Item = f32>) {
    if let Ok(mut v) = buf.lock() {
        v.extend(iter);
    }
}

/// 停止录音并返回归一后的 16kHz 单声道 f32 buffer。
/// drop Stream 停采集，取出累积样本，先混单声道再降采样到 16k。
/// 没在录 → Err。
pub fn stop_recording() -> Result<Vec<f32>, String> {
    let recording = {
        let mut guard = VOICE_STATE.lock().map_err(|_| "录音状态锁中毒".to_string())?;
        guard.take().ok_or_else(|| "当前没有在录音".to_string())?
    };

    // 显式停流并 drop，确保音频线程不再往 buffer 写。
    let _ = recording.stream.pause();
    drop(recording.stream);

    let raw = recording
        .samples
        .lock()
        .map_err(|_| "样本 buffer 锁中毒".to_string())?
        .clone();

    // 关键诊断：采集到多少样本、约多长。回调从未触发 / 麦克风静默 → 这里就是 0。
    let frames = if recording.channels > 0 {
        raw.len() / recording.channels as usize
    } else {
        raw.len()
    };
    let dur_secs = if recording.sample_rate > 0 {
        frames as f64 / recording.sample_rate as f64
    } else {
        0.0
    };
    eprintln!(
        "[voice] 采集样本数={}, 时长≈{:.2}s, 设备={}, 采样率={}, 声道={}, 格式={:?}",
        raw.len(),
        dur_secs,
        recording.device_name,
        recording.sample_rate,
        recording.channels,
        recording.sample_format
    );

    // 样本数为 0 / 时长≈0 → 明确报错：录音回调根本没采到东西。
    // 常见原因：麦克风被系统隐私开关禁用、设备被别的程序独占、或物理无输入。
    if raw.is_empty() || dur_secs < 0.05 {
        return Err(format!(
            "未采集到音频（设备 {}，采样数 {}）。请检查：① 麦克风设备是否选对/已插好；② Windows 设置→隐私→麦克风 是否允许桌面应用访问；③ 麦克风是否被其它程序独占",
            recording.device_name, raw.len()
        ));
    }

    let mono = to_mono(&raw, recording.channels);
    let resampled = resample_to_16k(&mono, recording.sample_rate);
    eprintln!(
        "[voice] 归一化后：单声道样本数={}, 目标采样率={}",
        resampled.len(),
        TARGET_SAMPLE_RATE
    );
    Ok(resampled)
}

/// 多声道交错样本 → 单声道（每帧各声道求平均）。channels<=1 直接原样返回。
fn to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    let ch = channels as usize;
    interleaved
        .chunks(ch)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect()
}

/// 简单线性插值重采样到 16kHz（单声道）。
/// v1 务实做法：不引 rubato，线性插值对语音转写足够（whisper 对轻微重采样误差不敏感）。
/// 已是 16k 或空 → 原样返回。
fn resample_to_16k(mono: &[f32], src_rate: u32) -> Vec<f32> {
    if src_rate == TARGET_SAMPLE_RATE || mono.is_empty() {
        return mono.to_vec();
    }
    let src_len = mono.len();
    let dst_len = (src_len as u64 * TARGET_SAMPLE_RATE as u64 / src_rate as u64) as usize;
    if dst_len == 0 {
        return Vec::new();
    }
    let ratio = (src_len - 1) as f32 / dst_len.max(1) as f32;
    let mut out = Vec::with_capacity(dst_len);
    for i in 0..dst_len {
        let pos = i as f32 * ratio;
        let idx = pos.floor() as usize;
        let frac = pos - idx as f32;
        let a = mono[idx];
        let b = if idx + 1 < src_len { mono[idx + 1] } else { a };
        out.push(a + (b - a) * frac);
    }
    out
}

// ============================================================================
// 写 WAV（hound）
// ============================================================================

/// 把 16kHz 单声道 f32 样本写成临时 WAV，返回文件路径。
/// 用 16-bit PCM（sherpa-onnx 要求单声道 16-bit PCM、采样率任意；体积也比 f32 小一半）。落 `std::env::temp_dir()`。
fn write_temp_wav(samples: &[f32]) -> Result<PathBuf, String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let path = std::env::temp_dir().join(format!("jarvis-voice-{}.wav", std::process::id()));
    let mut writer = hound::WavWriter::create(&path, spec)
        .map_err(|e| format!("创建临时 WAV 失败: {}", e))?;
    for &s in samples {
        // f32[-1,1] → i16，clamp 防溢出。
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer
            .write_sample(v)
            .map_err(|e| format!("写 WAV 样本失败: {}", e))?;
    }
    writer
        .finalize()
        .map_err(|e| format!("收尾 WAV 失败: {}", e))?;
    // 诊断：WAV 落盘大小（16k 单声道 16-bit → 约 32KB/秒；过小说明几乎没声音）。
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    eprintln!(
        "[voice] 写 WAV：{} 字节（16k 单声道 16-bit），路径={}",
        size,
        path.display()
    );
    Ok(path)
}

// ============================================================================
// 转写（sherpa-onnx-offline 子进程，SenseVoice 模型）
// ============================================================================

/// sherpa-onnx-offline 的 `--num-threads` 线程数：取 CPU 可用并行度，封顶 8。
/// 纯 CPU 推理拉满线程是最直接的提速；封顶 8 避免和系统其它进程抢核。
/// 探测失败（拿不到 available_parallelism）回退到 4。
fn sherpa_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8)
}

/// 从 sherpa-onnx-offline 的 stdout 解析出转写纯文本。
///
/// sherpa-onnx-offline 把每个 wav 的识别结果以 **JSON 一行**打到 stdout（源码
/// `offline-stream.cc` 的 `GetResult().AsJsonString()`，形如
/// `{"lang":"<|zh|>", "emotion":"...", "event":"...", "text":"识别文本", "timestamps":[...], "tokens":[...]}`，
/// 字段用 std::quoted 转义）。文件名 / `----` 分隔 / RTF 等诊断信息走的是 stderr，不混进 stdout。
///
/// 解析策略：逐行找第一个能解析成 JSON 且带 `text` 字段的行，取 `text`；
/// 万一格式变动 JSON 解析不出来，兜底把 stdout 整体 trim 当文本（best-effort，不让格式微调直接报错）。
fn parse_sherpa_text(stdout: &str) -> String {
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
                return t.trim().to_string();
            }
        }
    }
    // 兜底：没解析出 JSON 的 text，退回整体 stdout（去掉空白）。
    stdout.trim().to_string()
}

/// 跑 sherpa-onnx-offline 把 WAV 转成文本。
///
/// 命令：`sherpa-onnx-offline --tokens=<tokens.txt> --sense-voice-model=<model.int8.onnx>
///        --num-threads=<N> --sense-voice-use-itn=1 --debug=0 <wav>`
///   - `--tokens` / `--sense-voice-model`：词表 + SenseVoice int8 ONNX 模型。
///   - `--num-threads=N`：CPU 线程数（拉满核心，见 sherpa_threads，纯 CPU 下最明显的提速）。
///   - `--sense-voice-use-itn=1`：开 ITN（逆文本归一）→ 输出**带标点**、数字/日期规整。
///   - `--debug=0`：少打调试日志。
///   - 不传 `--sense-voice-language` → 默认 auto，自动处理中英混说（用户群中文夹英文术语场景最契合）。
/// 结果 JSON 打到 stdout、诊断（含 RTF）走 stderr。Windows 用 CREATE_NO_WINDOW 防黑窗。
fn transcribe_wav(wav_path: &PathBuf) -> Result<String, String> {
    let cli = sherpa_offline_path();
    let model = model_path();
    let tokens = tokens_path();

    let threads = sherpa_threads();
    // sherpa 用 `--flag=value` 形式，整体作为单个 arg 传（路径含空格也安全：作为一个 OsString）。
    let tokens_arg = {
        let mut s = std::ffi::OsString::from("--tokens=");
        s.push(tokens.as_os_str());
        s
    };
    let model_arg = {
        let mut s = std::ffi::OsString::from("--sense-voice-model=");
        s.push(model.as_os_str());
        s
    };
    let threads_arg = format!("--num-threads={}", threads);

    let mut cmd = silent_command(&cli);
    cmd.arg(&tokens_arg)
        .arg(&model_arg)
        .arg(&threads_arg)
        .arg("--sense-voice-use-itn=1")
        .arg("--debug=0")
        .arg(wav_path);

    // 诊断：打完整命令行，便于复现/排查（模型/词表缺失、参数错、二进制 DLL 缺失都从这看）。
    eprintln!(
        "[voice] 运行 sherpa-onnx-offline：{} --tokens={} --sense-voice-model={} --num-threads={} --sense-voice-use-itn=1 --debug=0 {}",
        cli.display(),
        tokens.display(),
        model.display(),
        threads,
        wav_path.display()
    );

    let output = cmd.output().map_err(|e| {
        format!(
            "启动 sherpa-onnx-offline 失败: {}（路径 {}，缺 .exe 或同目录 onnxruntime DLL？）",
            e,
            cli.display()
        )
    })?;

    // 始终打印 exit code + stderr 摘要（sherpa 的模型加载日志、RTF 都走 stderr）+ stdout（结果 JSON）。
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout_raw = String::from_utf8_lossy(&output.stdout);
    eprintln!(
        "[voice] sherpa-onnx-offline 退出码={:?}\n[voice] --- sherpa stderr ---\n{}\n[voice] --- sherpa stdout ---\n{}\n[voice] --- end ---",
        output.status.code(),
        stderr.trim(),
        stdout_raw.trim()
    );

    if !output.status.success() {
        // 取 stderr 末尾若干行作摘要带回前端（sherpa 报错通常在最后几行）。
        let summary = stderr_tail(&stderr, 5);
        return Err(format!(
            "sherpa-onnx-offline 转写失败（退出码 {:?}）：{}",
            output.status.code(),
            summary
        ));
    }

    // 从 stdout 的结果 JSON 取 text 字段。
    let text = parse_sherpa_text(&stdout_raw);
    eprintln!("[voice] 转写文本（{} 字）：{}", text.chars().count(), text);

    // 转写为空：带上 sherpa stderr 摘要，便于判断是音频空 / 模型加载失败 / 参数问题。
    if text.is_empty() {
        return Err(format!(
            "转写结果为空（未识别到语音内容）。sherpa 日志：{}",
            stderr_tail(&stderr, 4)
        ));
    }

    Ok(text)
}

/// 取 stderr 末尾 n 行作错误摘要（sherpa 报错/关键信息一般在最后几行）。
/// 全空则给个占位，避免把空串塞进错误消息让用户一头雾水。
fn stderr_tail(stderr: &str, n: usize) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return "（sherpa 无错误输出，疑似音频为空或模型未正确加载）".to_string();
    }
    let start = lines.len().saturating_sub(n);
    lines[start..].join(" | ")
}

/// 建不弹 console 窗口的 Command（Windows CREATE_NO_WINDOW）。
/// 与 commands/mod.rs 的 silent_command 同款，避免跨模块 pub 暴露内部细节，这里复刻一份。
fn silent_command(program: &PathBuf) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}

// ============================================================================
// 云端转写（火山引擎 / 豆包「流式语音识别大模型」WebSocket，按 research 11/12）
// ============================================================================
//
// 选型：`wss://openspeech.bytedance.com/api/v3/sauc/bigmodel`（流式大模型，默认端点）。
// 鉴权：握手 header 放 app_id + access_token（无签名、无对象存储）。
// 协议：二进制分帧 = 4 字节定长头 + 可选 4 字节序号(i32) + 4 字节 payload 长度(u32) + payload。
//   ① init 帧（FullClientRequest, PositiveSeq, JSON, Gzip, seq=1）：gzip(json{user,audio,request})。
//   ② 音频帧（AudioOnlyClient, PositiveSeq, Gzip, seq 递增）：把 f32→i16 小端 PCM 按 6400 字节/块、
//      每块 gzip 后发；末块改 NegativeSeq + seq=-seq 标记最后一包。
//   ③ 收 FullServerResponse：payload gzip 解开取 `result.text`；is_final（或 flag&LastNoSeq）即最终文本。
// 关键坑（research 强调）：init 的 JSON 和每个音频块都**必须 gzip**，响应也要解 gzip——漏 gzip 直接参数错。
// 音频格式与本机录音链路完全匹配（16k/16bit/单声道），云端路径**不写 WAV**，直接 i16 LE 裸 PCM。

/// 火山流式 ASR 端点（新版控制台对应的大模型 WebSocket；默认 bigmodel，整段一次性发也 OK）。
const VOLC_ASR_URL: &str = "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel";

/// 流式语音识别大模型（按时长计费）的资源 ID，固定值。
const VOLC_RESOURCE_ID: &str = "volc.bigasr.sauc.duration";

/// keychain 里火山 Access Token 的 account（仿 llm.apiKey 的 strip/hydrate 套路）。
const VOLC_TOKEN_ACCOUNT: &str = "voice.cloud.volcAccessToken";

/// 整段云端转写的总超时（建连 + 发送 + 收最终文本）。录一小段语音 15s 足够，超了多半是网络/鉴权卡死。
const VOLC_TOTAL_TIMEOUT_SECS: u64 = 15;

/// 音频分块大小：16000 * 2 字节 * 1 声道 * 0.2s = 6400 字节/块（约 200ms）。
const VOLC_AUDIO_CHUNK_BYTES: usize = 6400;

// ---- 二进制帧协议常量（4 字节头各位）----
const VOLC_PROTOCOL_VERSION: u8 = 0b0001; // version=1
const VOLC_HEADER_SIZE: u8 = 0b0001; // header 占 1*4=4 字节
const VOLC_MSG_FULL_CLIENT: u8 = 0b0001; // FullClientRequest（init）
const VOLC_MSG_AUDIO_CLIENT: u8 = 0b0010; // AudioOnlyClient（音频块）
const VOLC_MSG_FULL_SERVER: u8 = 0b1001; // FullServerResponse（识别结果）
const VOLC_MSG_ERROR: u8 = 0b1111; // Error（服务端错误帧）
const VOLC_FLAG_POS_SEQ: u8 = 0b0001; // 带正序号
const VOLC_FLAG_NEG_SEQ: u8 = 0b0011; // 末包：带负序号
const VOLC_FLAG_LAST_NO_SEQ: u8 = 0b0010; // 服务端「最后一包」标志位
const VOLC_SER_RAW: u8 = 0b0000; // payload 为裸字节（音频）
const VOLC_SER_JSON: u8 = 0b0001; // payload 为 JSON
const VOLC_COMP_GZIP: u8 = 0b0001; // gzip 压缩

/// 读火山云端凭证：App ID（明文存 config.voiceCloud.volcAppId）+ Access Token（优先 keychain）。
/// token 兜底：keychain → config 明文（非占位符，迁移用）。任一缺失返回 Err（中文提示去控制台开通）。
fn volc_credentials() -> Result<(String, String), String> {
    let cfg = crate::settings::load_raw_config();
    let vc = cfg.as_ref().and_then(|v| v.get("voiceCloud"));
    let s = |key: &str| -> Option<String> {
        vc.and_then(|v| v.get(key))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };

    let app_id = s("volcAppId").ok_or_else(|| {
        "未配置火山引擎 App ID。请在设置→语音输入→选「云端」后填入 App ID 和 Access Token".to_string()
    })?;

    // token 优先 keychain；config 里通常是占位符 ********，仅旧明文配置作迁移兜底。
    let token = crate::settings::secret_get(VOLC_TOKEN_ACCOUNT)
        .or_else(|| s("volcAccessToken").filter(|v| v != crate::settings::SECRET_PLACEHOLDER))
        .ok_or_else(|| {
            "未配置火山引擎 Access Token。请在设置→语音输入→选「云端」后填入 Access Token".to_string()
        })?;

    Ok((app_id, token))
}

/// 生成一个简易 UUID v4 字符串（X-Api-Request-Id / X-Api-Connect-Id 用）。
/// 无 uuid crate：用进程 id + 纳秒时钟 + 计数器拼 16 字节走 RFC4122 v4 排版，足够唯一（仅作请求标识）。
fn volc_uuid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    let cnt = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut bytes = [0u8; 16];
    bytes[0..8].copy_from_slice(&nanos.to_le_bytes());
    bytes[8..16].copy_from_slice(&(pid ^ cnt.rotate_left(32)).to_le_bytes());
    // 置 version(4) 与 variant(10) 位，符合 v4 排版。
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let h = |b: &[u8]| b.iter().map(|x| format!("{:02x}", x)).collect::<String>();
    format!(
        "{}-{}-{}-{}-{}",
        h(&bytes[0..4]),
        h(&bytes[4..6]),
        h(&bytes[6..8]),
        h(&bytes[8..10]),
        h(&bytes[10..16])
    )
}

/// gzip 压缩（init JSON + 每个音频块发送前都要压）。flate2 默认 miniz_oxide 纯 Rust 后端。
fn gzip_compress(data: &[u8]) -> Result<Vec<u8>, String> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data)
        .map_err(|e| format!("gzip 压缩写入失败: {}", e))?;
    enc.finish().map_err(|e| format!("gzip 压缩收尾失败: {}", e))
}

/// gzip 解压（服务端返回的 payload 是 gzip 后的 JSON）。
fn gzip_decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    use flate2::read::GzDecoder;
    use std::io::Read;
    let mut dec = GzDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out)
        .map_err(|e| format!("gzip 解压失败: {}", e))?;
    Ok(out)
}

/// 手写 marshal：拼一个二进制帧 = 4 字节头 [(ver<<4)|hsize, (type<<4)|flag, (ser<<4)|comp, 0x00]
///   + 4 字节大端序号(i32) + 4 字节大端 payload 长度(u32) + payload。
/// 本实现所有帧都带序号（init/音频均带正或负序号），故固定写序号段。payload 已是 gzip 后的字节。
fn volc_marshal(msg_type: u8, flags: u8, serialization: u8, sequence: i32, payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(12 + payload.len());
    buf.push((VOLC_PROTOCOL_VERSION << 4) | VOLC_HEADER_SIZE);
    buf.push((msg_type << 4) | flags);
    buf.push((serialization << 4) | VOLC_COMP_GZIP);
    buf.push(0x00);
    buf.extend_from_slice(&sequence.to_be_bytes());
    buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(payload);
    buf
}

/// 解析后的服务端帧：消息类型 + flags + 解 gzip 后的 payload 字节。
struct VolcServerFrame {
    msg_type: u8,
    flags: u8,
    payload: Vec<u8>,
}

/// 手写 parse：反解服务端二进制帧。读 4 字节头 → 按 header_size 跳头 → 若带序号读 4 字节序号 →
/// 读 4 字节 payload 长度 → 截 payload → 若 compression=gzip 则解压。
/// 头太短 / 长度越界返回 Err（防越界 panic，符合「禁止字节切片越界」铁律）。
fn volc_parse(frame: &[u8]) -> Result<VolcServerFrame, String> {
    if frame.len() < 4 {
        return Err(format!("服务端帧过短（{} 字节，不足 4 字节头）", frame.len()));
    }
    let header_size = (frame[0] & 0x0f) as usize * 4; // 低 4 位是 header_size（单位：4 字节）
    let msg_type = frame[1] >> 4;
    let flags = frame[1] & 0x0f;
    let compression = frame[2] & 0x0f;

    let mut offset = header_size.max(4);
    if frame.len() < offset {
        return Err(format!("服务端帧头声明 {} 字节但实际只有 {}", offset, frame.len()));
    }

    // 带序号（正/负）→ 跳过 4 字节序号字段。
    if flags == VOLC_FLAG_POS_SEQ || flags == VOLC_FLAG_NEG_SEQ {
        if frame.len() < offset + 4 {
            return Err("服务端帧缺序号字段".to_string());
        }
        offset += 4;
    }

    // 4 字节 payload 长度（u32 大端）。
    if frame.len() < offset + 4 {
        return Err("服务端帧缺 payload 长度字段".to_string());
    }
    let len = u32::from_be_bytes([
        frame[offset],
        frame[offset + 1],
        frame[offset + 2],
        frame[offset + 3],
    ]) as usize;
    offset += 4;

    if frame.len() < offset + len {
        return Err(format!(
            "服务端帧 payload 声明 {} 字节但实际只剩 {}",
            len,
            frame.len() - offset
        ));
    }
    let raw = &frame[offset..offset + len];

    let payload = if compression == VOLC_COMP_GZIP {
        gzip_decompress(raw)?
    } else {
        raw.to_vec()
    };

    Ok(VolcServerFrame {
        msg_type,
        flags,
        payload,
    })
}

/// 把 16k 单声道 f32 样本转成 i16 小端 PCM 字节（逻辑同 write_temp_wav 的 f32→i16，clamp 防溢出）。
/// 云端路径据此直接发裸 PCM（format:"pcm", codec:"raw"），不落 WAV。
fn samples_to_pcm_le(samples: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        buf.extend_from_slice(&v.to_le_bytes());
    }
    buf
}

/// 从服务端返回的 JSON 取 `result.text`。结构：`{ "result": { "text": "...", "is_final": bool } }`。
/// 解析不出 result.text 返回 None（中间帧可能没有/为空，由调用方累积取最后非空）。
fn volc_extract_text(payload: &[u8]) -> Option<(String, bool)> {
    let v: Value = serde_json::from_slice(payload).ok()?;
    let result = v.get("result")?;
    let text = result.get("text").and_then(|t| t.as_str()).unwrap_or("");
    let is_final = result
        .get("is_final")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    Some((text.trim().to_string(), is_final))
}

/// 云端转写主流程：连 WS（带 X-Api-* 鉴权头）→ 发 init 帧 → 分块发音频（末块负序号）→ 收最终文本。
/// 入参是 stop_recording() 归一好的 16k 单声道 f32（与本地路径同源），内部转 i16 LE 裸 PCM。
/// 全程 15s 总超时；各阶段 eprintln 留痕（连接/发送/收到文本）；凭证缺失/网络/服务端错误均中文化。
async fn transcribe_volcengine(samples: &[f32]) -> Result<String, String> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Message;

    let (app_id, token) = volc_credentials()?;
    let request_id = volc_uuid();

    eprintln!(
        "[voice] 云端转写：端点={}, app_id={}, 样本数={}",
        VOLC_ASR_URL,
        app_id,
        samples.len()
    );

    // 整段套 15s 超时：建连/发送/收最终文本任一卡住都不至于挂死后台线程。
    let fut = async {
        // 1) 构造带 X-Api-* 鉴权头的握手请求。ClientRequestBuilder 会先用 uri 生成标准
        //    WebSocket 头（Host/Upgrade/Connection/Sec-WebSocket-*），再追加这几个自定义头。
        let uri: tokio_tungstenite::tungstenite::http::Uri = VOLC_ASR_URL
            .parse()
            .map_err(|e| format!("云端 ASR 端点 URL 无效: {}", e))?;
        let request = tokio_tungstenite::tungstenite::ClientRequestBuilder::new(uri)
            .with_header("X-Api-App-Id", app_id.clone())
            .with_header("X-Api-App-Key", app_id.clone()) // 部分端点也认 App-Key（值同 app_id），保险都带
            .with_header("X-Api-Access-Key", token.clone())
            .with_header("X-Api-Resource-Id", VOLC_RESOURCE_ID)
            .with_header("X-Api-Request-Id", request_id.clone())
            .with_header("X-Api-Connect-Id", volc_uuid())
            .into_client_request()
            .map_err(|e| format!("构造云端 ASR 握手请求失败: {}", e))?;

        let (mut ws, _resp) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|e| {
                format!(
                    "连接火山云端 ASR 失败（检查网络 / App ID / Access Token 是否正确）: {}",
                    e
                )
            })?;
        eprintln!("[voice] 云端转写：WebSocket 已连接，发送 init 帧");

        // 2) init 帧：gzip(json 配置)。audio 用 pcm/raw/16000/16bit/单声道，与本机录音完全匹配。
        let init_payload = json!({
            "user": { "uid": request_id },
            "audio": { "format": "pcm", "codec": "raw", "rate": 16000, "bits": 16, "channel": 1 },
            "request": {
                "model_name": "bigmodel",
                "enable_itn": true,
                "enable_punc": true,
                "enable_ddc": true,
                "show_utterances": true,
                "enable_nonstream": false
            }
        });
        let init_bytes =
            serde_json::to_vec(&init_payload).map_err(|e| format!("序列化 init 配置失败: {}", e))?;
        let init_gz = gzip_compress(&init_bytes)?;
        let init_frame = volc_marshal(
            VOLC_MSG_FULL_CLIENT,
            VOLC_FLAG_POS_SEQ,
            VOLC_SER_JSON,
            1,
            &init_gz,
        );
        ws.send(Message::Binary(init_frame))
            .await
            .map_err(|e| format!("发送 init 帧失败: {}", e))?;

        // 3) 音频帧：f32 → i16 LE 裸 PCM，按 6400 字节切块，逐块 gzip 后发；末块用负序号标记结束。
        let pcm = samples_to_pcm_le(samples);
        let chunks: Vec<&[u8]> = if pcm.is_empty() {
            // 极端：没有音频也要发一个空末帧让服务端收尾（理论上不会到这，stop_recording 已挡空）。
            vec![&[]]
        } else {
            pcm.chunks(VOLC_AUDIO_CHUNK_BYTES).collect()
        };
        let total_chunks = chunks.len();
        eprintln!(
            "[voice] 云端转写：PCM {} 字节，分 {} 块发送（{} 字节/块）",
            pcm.len(),
            total_chunks,
            VOLC_AUDIO_CHUNK_BYTES
        );

        let mut seq: i32 = 1;
        for (i, chunk) in chunks.iter().enumerate() {
            seq += 1;
            let is_last = i + 1 == total_chunks;
            let chunk_gz = gzip_compress(chunk)?;
            // 末块：NegativeSeq + seq=-seq；其余：PositiveSeq + seq。音频帧 serialization 用 Raw。
            let (flags, send_seq) = if is_last {
                (VOLC_FLAG_NEG_SEQ, -seq)
            } else {
                (VOLC_FLAG_POS_SEQ, seq)
            };
            let frame = volc_marshal(
                VOLC_MSG_AUDIO_CLIENT,
                flags,
                VOLC_SER_RAW,
                send_seq,
                &chunk_gz,
            );
            ws.send(Message::Binary(frame))
                .await
                .map_err(|e| format!("发送音频帧（第 {}/{} 块）失败: {}", i + 1, total_chunks, e))?;
        }
        eprintln!("[voice] 云端转写：音频发送完毕，等待识别结果");

        // 4) 收包：解析服务端帧，累积 result.text，收到 is_final（或 LastNoSeq 标志）即返回。
        //    中间 partial 文本会渐长，取最后一条非空作最终文本（整段模式）。
        let mut final_text = String::new();
        while let Some(msg) = ws.next().await {
            let msg = msg.map_err(|e| format!("接收云端 ASR 响应失败: {}", e))?;
            let bytes = match msg {
                Message::Binary(b) => b,
                Message::Close(_) => break,
                // 文本/Ping/Pong 等非二进制帧：协议里识别结果都是二进制，跳过。
                _ => continue,
            };
            let parsed = volc_parse(&bytes)?;

            if parsed.msg_type == VOLC_MSG_ERROR {
                let detail = String::from_utf8_lossy(&parsed.payload);
                return Err(format!("火山云端 ASR 返回错误: {}", detail.trim()));
            }
            if parsed.msg_type != VOLC_MSG_FULL_SERVER {
                continue;
            }

            if let Some((text, is_final)) = volc_extract_text(&parsed.payload) {
                if !text.is_empty() {
                    final_text = text;
                }
                let last_by_flag = parsed.flags & VOLC_FLAG_LAST_NO_SEQ != 0;
                if is_final || last_by_flag {
                    eprintln!(
                        "[voice] 云端转写：收到最终文本（{} 字）：{}",
                        final_text.chars().count(),
                        final_text
                    );
                    let _ = ws.close(None).await;
                    return Ok(final_text);
                }
            }
        }

        // 连接结束仍没等到 is_final：有非空累积就用它，否则报空。
        if final_text.is_empty() {
            Err("云端转写结果为空（未识别到语音内容，或服务端未返回最终文本）".to_string())
        } else {
            eprintln!(
                "[voice] 云端转写：连接结束，使用末次文本（{} 字）：{}",
                final_text.chars().count(),
                final_text
            );
            Ok(final_text)
        }
    };

    match tokio::time::timeout(std::time::Duration::from_secs(VOLC_TOTAL_TIMEOUT_SECS), fut).await {
        Ok(r) => r,
        Err(_) => Err(format!(
            "云端转写超时（{}s 未返回结果，请检查网络）",
            VOLC_TOTAL_TIMEOUT_SECS
        )),
    }
}

// ============================================================================
// 注入聚焦输入框（arboard + enigo）
// ============================================================================

/// 把文本注入当前聚焦的输入框：剪贴板存旧值 → 写转写文本 → 模拟 Ctrl+V → 短延时 → 恢复旧值。
///
/// 走剪贴板+Ctrl+V 而非 enigo.text()：Windows 下直接模拟键入中文会丢字/乱码，粘贴绕过 IME 最稳。
/// 非文本剪贴板（图片等）读不出旧值时只能尽力——粘贴照做，旧值无法精确恢复（v1 接受）。
fn inject_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Err("转写结果为空，无可注入文本".to_string());
    }
    eprintln!("[voice] 注入文本（{} 字，走剪贴板+Ctrl+V）：{}", text.chars().count(), text);

    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("打开剪贴板失败: {}", e))?;

    // 暂存旧剪贴板文本（非文本场景读失败 → None，恢复时跳过）。
    let saved = clipboard.get_text().ok();

    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("写入剪贴板失败: {}", e))?;

    // 模拟 Ctrl+V。enigo 0.3：key(Control,Press) → key('v',Click) → key(Control,Release)。
    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| format!("初始化输入模拟失败: {}", e))?;
    enigo
        .key(Key::Control, Press)
        .map_err(|e| format!("模拟 Ctrl 按下失败: {}", e))?;
    enigo
        .key(Key::Unicode('v'), Click)
        .map_err(|e| format!("模拟 V 键失败: {}", e))?;
    enigo
        .key(Key::Control, Release)
        .map_err(|e| format!("模拟 Ctrl 松开失败: {}", e))?;

    // 短延时确保目标程序读完剪贴板再恢复（太早会把没粘完的内容冲掉，~150ms 经验值）。
    std::thread::sleep(std::time::Duration::from_millis(150));

    if let Some(prev) = saved {
        // 恢复旧剪贴板（best-effort：恢复失败不算注入失败，文本已粘贴成功）。
        let _ = clipboard.set_text(prev);
    }

    eprintln!("[voice] 注入完成（已模拟 Ctrl+V）");
    Ok(())
}

// ============================================================================
// 资产下载（reqwest 流式 → 解压 → 落 voice_dir）
// ============================================================================

/// 流式下载一个 URL 到本地文件，**断点续传 + 自动重试 + 手动跟随 302**，边下边 emit 进度。
///
/// 抗断流设计（针对 hf-mirror 302→HF 美国 Xet 大文件必断的场景）：
/// - 下到 `<dest>.part`，每次（重）开始按已有字节 N 带 `Range: bytes=N-` **追加写**；
///   流中途出错 → backoff(1~3s) → 用当前 .part 大小续传，最多 `MAX_DOWNLOAD_ATTEMPTS` 次。
/// - 总大小从 `Content-Range`（带 Range 时回 206）或 `Content-Length`（首个 200）拿；
///   累计达总大小才算完成 → 原子 rename 到目标。
/// - 跨 host 302 手动跟：每跳都带 `Range` + `Accept-Encoding: identity`（reqwest 自动跟随会丢这些头）。
/// - 进度事件 `voice-download-progress { phase, downloaded, total, percent, bytesPerSec }`，
///   续传时 downloaded 从 .part 已有字节起步、不回退。
async fn download_to_file(
    app: &tauri::AppHandle,
    url: &str,
    dest: &PathBuf,
    phase: &str,
    proxy: Option<&str>,
) -> Result<(), String> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建语音目录失败: {}", e))?;
    }

    let client = build_download_client(proxy)?;
    let part = dest.with_extension("part");

    let mut last_err = String::new();
    for attempt in 1..=MAX_DOWNLOAD_ATTEMPTS {
        // 续传起点 = 当前 .part 已有字节（首次没有则 0）。
        let resume_from = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);

        match download_attempt(app, &client, url, &part, phase, resume_from).await {
            Ok(()) => {
                // 下完原子 rename；失败清掉 .part。
                std::fs::rename(&part, dest).map_err(|e| {
                    let _ = std::fs::remove_file(&part);
                    format!("下载文件改名失败: {}", e)
                })?;
                return Ok(());
            }
            Err(e) => {
                last_err = e;
                eprintln!(
                    "[voice] 下载{}第 {}/{} 次中断（已存 {} 字节）：{}",
                    phase, attempt, MAX_DOWNLOAD_ATTEMPTS, resume_from, last_err
                );
                if attempt < MAX_DOWNLOAD_ATTEMPTS {
                    // backoff 1~3s（随尝试次递增，封顶 3s），给对端/网络喘口气再续传。
                    let secs = (attempt as u64).min(3);
                    tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
                }
            }
        }
    }
    Err(format!(
        "下载 {} 失败（已续传重试 {} 次仍未完成）：{}",
        phase, MAX_DOWNLOAD_ATTEMPTS, last_err
    ))
}

/// 单次下载尝试：从 `resume_from` 字节起带 Range 拉流、追加写 `.part`，下到流结束。
/// 返回 Ok 表示本次累计已达总大小（真正下完）；流中途断开返回 Err（交由上层 backoff 续传）。
async fn download_attempt(
    app: &tauri::AppHandle,
    client: &reqwest::Client,
    url: &str,
    part: &PathBuf,
    phase: &str,
    resume_from: u64,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use std::io::Write;
    use tauri::Emitter;

    // 手动跟随 302：每跳带 Range + Accept-Encoding: identity。
    // is_partial=true 表示服务器认了 Range（回 206、接着续传）；false 表示回了整个 200
    // （服务器忽略了 Range），这时必须从头覆盖写，不能往 .part 后追加（否则会拼坏文件）。
    let (resp, total, is_partial) = fetch_with_redirects(client, url, resume_from, phase).await?;

    // 续传起点：服务器认 Range（206）→ 接着已有字节追加；否则（200 整包）从头覆盖。
    let start = if is_partial { resume_from } else { 0 };
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(start > 0)
        .write(true)
        .truncate(start == 0)
        .open(part)
        .map_err(|e| format!("打开下载临时文件失败: {}", e))?;

    let mut downloaded: u64 = start;
    let mut last_emit: u64 = downloaded;
    // 速度估算：记一个采样点（时刻 + 已下字节），相邻 emit 之间算 bytesPerSec。
    let mut sample_at = std::time::Instant::now();
    let mut sample_bytes = downloaded;

    let mut stream = resp.bytes_stream();
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("下载流读取错误（{}）: {}", phase, e))?;
        file.write_all(&chunk)
            .map_err(|e| format!("写下载文件失败: {}", e))?;
        downloaded += chunk.len() as u64;

        // 节流：每累计 ~1MB（或到顶）emit 一次，避免 574MB 刷爆事件通道。
        if downloaded - last_emit >= 1_000_000 || (total > 0 && downloaded >= total) {
            let now = std::time::Instant::now();
            let dt = now.duration_since(sample_at).as_secs_f64();
            let bytes_per_sec = if dt > 0.05 {
                ((downloaded - sample_bytes) as f64 / dt) as u64
            } else {
                0
            };
            if dt >= 0.5 {
                // 每 ~0.5s 刷新一次速度采样基准，避免瞬时抖动。
                sample_at = now;
                sample_bytes = downloaded;
            }
            last_emit = downloaded;
            let percent = if total > 0 {
                (downloaded as f64 / total as f64 * 100.0) as u32
            } else {
                0
            };
            let _ = app.emit(
                "voice-download-progress",
                json!({
                    "phase": phase,
                    "downloaded": downloaded,
                    "total": total,
                    "percent": percent,
                    "bytesPerSec": bytes_per_sec,
                }),
            );
        }
    }

    file.flush().map_err(|e| format!("刷新下载文件失败: {}", e))?;
    drop(file);

    // 流正常结束但没下满 → 视为中途断开，返回 Err 让上层续传。
    if total > 0 && downloaded < total {
        return Err(format!(
            "下载流提前结束（{} / {} 字节），将续传",
            downloaded, total
        ));
    }
    Ok(())
}

/// 手动跟随 302 发起带 Range 的下载请求，返回（最终响应, 总字节数, 是否部分响应 206）。
///
/// 每一跳都带 `Range: bytes=N-` + `Accept-Encoding: identity`——reqwest 关掉自动跟随后由我们
/// 逐跳重发，跨 host（hf-mirror→HF→Xet）才不会丢这两个关键头。
/// - 206（认 Range）：总大小取 `Content-Range` 的 `bytes N-M/TOTAL` 末段；is_partial=true。
/// - 200（忽略 Range，回整包）：总大小就是 `Content-Length`（整文件）；is_partial=false，
///   上层据此从头覆盖写，避免往半截 .part 后面追加拼坏文件。
async fn fetch_with_redirects(
    client: &reqwest::Client,
    url: &str,
    resume_from: u64,
    phase: &str,
) -> Result<(reqwest::Response, u64, bool), String> {
    use reqwest::header::{ACCEPT_ENCODING, CONTENT_RANGE, LOCATION, RANGE};

    let mut current = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        let resp = client
            .get(&current)
            .header(RANGE, format!("bytes={}-", resume_from))
            .header(ACCEPT_ENCODING, "identity")
            .send()
            .await
            .map_err(|e| format!("下载请求失败（{}）: {}", phase, e))?;

        let status = resp.status();

        // 3xx：手动跟随到 Location（拼相对地址）。
        if status.is_redirection() {
            let loc = resp
                .headers()
                .get(LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| format!("下载 {} 收到 {} 重定向但缺 Location 头", phase, status))?;
            // Location 可能是相对路径，用当前 URL 作 base 解析成绝对地址。
            current = resolve_redirect(&current, loc)?;
            continue;
        }

        if !status.is_success() {
            return Err(format!(
                "下载 {} 失败：HTTP {}（{}）",
                phase,
                status.as_u16(),
                current
            ));
        }

        // 206 PARTIAL_CONTENT → 服务器认了 Range，可续传。
        let is_partial = status == reqwest::StatusCode::PARTIAL_CONTENT;
        let total = if is_partial {
            // 206：总长在 Content-Range 末段；缺则用 resume_from + 本段长度兜底。
            resp.headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range_total)
                .or_else(|| resp.content_length().map(|len| resume_from + len))
                .unwrap_or(0)
        } else {
            // 200：Content-Length 就是整文件大小（从头下）。
            resp.content_length().unwrap_or(0)
        };

        return Ok((resp, total, is_partial));
    }
    Err(format!("下载 {} 重定向次数过多（>{}）", phase, MAX_REDIRECTS))
}

/// 把（可能是相对路径的）Location 解析成绝对 URL。无外部 url crate，做最小够用的拼接：
/// 绝对地址（http/https 开头）直接用；`//host/..` 协议相对补 https；`/path` 绝对路径拼 origin；
/// 其余相对路径拼到当前路径目录。
fn resolve_redirect(base: &str, loc: &str) -> Result<String, String> {
    let loc = loc.trim();
    if loc.starts_with("http://") || loc.starts_with("https://") {
        return Ok(loc.to_string());
    }
    if let Some(rest) = loc.strip_prefix("//") {
        return Ok(format!("https://{}", rest));
    }
    // 取 base 的 scheme://host 部分。
    let scheme_end = base.find("://").ok_or_else(|| format!("无法解析重定向基地址: {}", base))?;
    let after_scheme = &base[scheme_end + 3..];
    let host_end = after_scheme.find('/').unwrap_or(after_scheme.len());
    let origin = &base[..scheme_end + 3 + host_end]; // scheme://host[:port]
    if loc.starts_with('/') {
        return Ok(format!("{}{}", origin, loc));
    }
    // 相对当前路径目录。
    let base_path = &base[..base.rfind('/').unwrap_or(base.len())];
    Ok(format!("{}/{}", base_path, loc))
}

/// 解析 `Content-Range: bytes N-M/TOTAL` 里的 TOTAL。`*` 或解析失败返回 None。
fn parse_content_range_total(value: &str) -> Option<u64> {
    let total = value.rsplit('/').next()?.trim();
    if total == "*" {
        return None;
    }
    total.parse::<u64>().ok()
}

/// 按顺序尝试一组 URL 下载到同一目标文件：前一个连不上/出错就试下一个，
/// 任一个成功（下完并落盘）即返回 Ok。全失败才返回 Err（汇总各 URL 的失败原因）。
///
/// 用途：二进制走 GitHub，国内直连常失败，需先试国内加速镜像再兜底直连。
/// `download_to_file` 写的是 `<dest>.part` 临时文件、成功才原子 rename，故某个 URL
/// 下到一半失败不会污染目标；下一个 URL 重新从 `.part` 起步覆盖即可，无需手动清理。
async fn download_to_file_multi(
    app: &tauri::AppHandle,
    urls: &[&str],
    dest: &PathBuf,
    phase: &str,
    proxy: Option<&str>,
) -> Result<(), String> {
    let mut errors: Vec<String> = Vec::new();
    for (i, url) in urls.iter().enumerate() {
        match download_to_file(app, url, dest, phase, proxy).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                eprintln!(
                    "[voice] 下载源 {}/{} 失败（{}）：{}",
                    i + 1,
                    urls.len(),
                    url,
                    e
                );
                errors.push(format!("[{}] {}", url, e));
            }
        }
    }
    Err(format!(
        "下载 {} 失败，已尝试 {} 个源均不可达：\n{}",
        phase,
        urls.len(),
        errors.join("\n")
    ))
}

/// 解压 sherpa-onnx 预编译 `.tar.bz2` 到 `voice_dir()`。
///
/// 官方包内是 `sherpa-onnx-v1.13.2-win-x64-shared-MT-Release/{bin,lib,include}/...`（已解包核对）。
/// 离线 CLI 跑起来只需 `bin/` 下的 `sherpa-onnx-offline.exe` + 同目录的 `onnxruntime.dll` /
/// `onnxruntime_providers_shared.dll`（MT 版把 sherpa 代码静态链进 exe，没有额外 sherpa DLL）。
/// 故只取**父目录名为 `bin` 的 `.exe` / `.dll`**，平铺写进 voice_dir（与原 whisper 平铺形态一致）；
/// `lib/`（c-api 链接库，含重复的 onnxruntime.dll）和 `include/` 头文件一概跳过，省体积、避免重名覆盖。
/// 防路径穿越：只用条目的最末文件名拼到 voice_dir，丢弃任何 `..` / 绝对路径。
///
/// 解压链：bzip2-rs（纯 Rust 解 bz2）→ tar crate（纯 Rust 解 tar）—— 全程不碰 C 编译器。
fn extract_sherpa_tarball(tar_path: &PathBuf) -> Result<(), String> {
    use std::ffi::OsStr;

    let dir = voice_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建语音目录失败: {}", e))?;

    let file =
        std::fs::File::open(tar_path).map_err(|e| format!("打开下载的压缩包失败: {}", e))?;
    // bzip2-rs 的 DecoderReader 边读边解 bz2，喂给 tar::Archive。用 BufReader 减少系统调用。
    let bz = bzip2_rs::DecoderReader::new(std::io::BufReader::new(file));
    let mut archive = tar::Archive::new(bz);

    let mut extracted = 0usize;
    let entries = archive
        .entries()
        .map_err(|e| format!("读取压缩包条目失败: {}", e))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("遍历压缩包条目失败: {}", e))?;
        let path = entry
            .path()
            .map_err(|e| format!("解析压缩包内路径失败: {}", e))?
            .into_owned();

        // 只要 `.../bin/<file>`：父目录名必须是 bin（跳过 lib/include 及顶层目录条目）。
        let in_bin = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n == OsStr::new("bin"))
            .unwrap_or(false);
        if !in_bin {
            continue;
        }

        // 防路径穿越：只取最末文件名。
        let file_name = match path.file_name() {
            Some(f) => f.to_owned(),
            None => continue,
        };
        // 只要 exe / dll（其余 sherpa 自带的辅助文件这里用不到）。
        let keep = {
            let lower = file_name.to_string_lossy().to_ascii_lowercase();
            lower.ends_with(".exe") || lower.ends_with(".dll")
        };
        if !keep {
            continue;
        }

        let out_path = dir.join(&file_name);
        let mut out = std::fs::File::create(&out_path)
            .map_err(|e| format!("写解压文件失败: {}", e))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("解压拷贝失败: {}", e))?;
        extracted += 1;
    }

    if extracted == 0 {
        return Err("压缩包解压后没有取到任何 exe/dll".to_string());
    }
    if !sherpa_offline_path().is_file() {
        return Err(format!(
            "解压完成但缺 sherpa-onnx-offline 可执行（期望 {}）",
            sherpa_offline_path().display()
        ));
    }
    Ok(())
}

// ============================================================================
// 全局热键（tauri-plugin-global-shortcut，toggle 触发）
// ============================================================================
//
// 设计：注册逻辑全在 Rust，前端只在开关翻转 / 启动时调一次 `voice_hotkey_sync`。
// 热键命中后按 toggle：没在录 → 开录；正在录 → 停录+转写+注入。后者阻塞（sherpa
// 子进程要一两秒），丢到后台线程跑，避免卡住 global-shortcut 的回调线程。
// 录音/转写期间往 avatar 窗口 emit `voice-state` 事件，前端据此切小人状态（listening
// / transcribing / idle），让用户知道何时该说话、何时在转写。出错 emit `voice-error`。

use tauri::Emitter;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// 读 `~/.jarvis/config.json` 里的 `voiceInputEnabled`（缺/解析失败按 false）。
/// 后端以磁盘上的 config 为单一数据源，不另维护一份开关状态——前端 config_save 已落盘。
fn voice_input_enabled() -> bool {
    crate::settings::load_raw_config()
        .and_then(|v| v.get("voiceInputEnabled").and_then(|b| b.as_bool()))
        .unwrap_or(false)
}

/// 读 `~/.jarvis/config.json` 里的 `voiceEngine`："cloud" → 云端（火山）；其余（含缺失）→ "local"（本地 sherpa）。
/// 默认本地，保持「本地优先、隐私」的产品调性；云端是用户显式选的可选项。
fn voice_engine() -> String {
    let v = crate::settings::load_raw_config()
        .and_then(|v| {
            v.get("voiceEngine")
                .and_then(|s| s.as_str())
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_default();
    if v == "cloud" {
        "cloud".to_string()
    } else {
        "local".to_string()
    }
}

/// 当前引擎是否「就绪可用」：
/// - 云端：App ID + Access Token 都配齐即可（不依赖本地 sherpa 资产、不需要下载模型）。
/// - 本地：sherpa 引擎 + 模型 + 词表三件套齐全（voice_assets_ready）。
/// 用于热键门禁 / voice_start 校验：两种引擎走各自的「就绪」判断，互不牵连。
fn voice_engine_ready() -> bool {
    if voice_engine() == "cloud" {
        volc_credentials().is_ok()
    } else {
        voice_assets_ready()
    }
}

/// 读 `~/.jarvis/config.json` 里的 `voiceHotkey`（trim 后非空才用）；缺/空回退默认。
/// 只返回字符串，是否能解析成合法 `Shortcut` 由 `configured_shortcut` 把关。
fn voice_hotkey_str() -> String {
    crate::settings::load_raw_config()
        .and_then(|v| {
            v.get("voiceHotkey")
                .and_then(|s| s.as_str())
                .map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_HOTKEY.to_string())
}

/// 记住「上次成功注册的」accelerator 字符串：换键时据此精确注销旧键，避免残留占用。
/// 用全局单例（同 VOICE_STATE 的 once_cell::Lazy + Mutex 范式）。None = 当前没注册任何热键。
static REGISTERED_HOTKEY: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

/// 把 config 里的 `voiceHotkey` 解析成 `(accelerator 字符串, Shortcut)`。
/// 配置值解析失败（无效键位）→ 自动回退默认键并再解析；连默认都解析不了（常量写错）才返回 Err。
fn configured_shortcut() -> Result<(String, Shortcut), String> {
    let accel = voice_hotkey_str();
    match accel.parse::<Shortcut>() {
        Ok(s) => Ok((accel, s)),
        Err(e) => {
            // 用户配了非法键位：退回默认键，保证语音输入仍可用（并留痕）。
            eprintln!("[voice] 配置热键 '{}' 无效（{}），回退默认 '{}'", accel, e, DEFAULT_HOTKEY);
            let fallback = DEFAULT_HOTKEY.to_string();
            let shortcut = fallback
                .parse::<Shortcut>()
                .map_err(|e| format!("解析默认热键 '{}' 失败: {}", DEFAULT_HOTKEY, e))?;
            Ok((fallback, shortcut))
        }
    }
}

/// 给 avatar 窗口发语音状态事件（best-effort，发不出去不影响主流程）。
/// state: "listening"（录音中）/ "transcribing"（转写中）/ "idle"（结束/空闲）。
fn emit_voice_state(app: &tauri::AppHandle, state: &str) {
    let _ = app.emit("voice-state", json!({ "state": state }));
}

/// 全局热键命中时的 toggle 处理：仅 Pressed 边沿响应。
/// 门禁：开关关 / 资产没就绪 → 忽略（eprintln 留痕，不打扰用户）。
fn on_hotkey_pressed(app: &tauri::AppHandle) {
    if !voice_input_enabled() {
        eprintln!("[voice] 热键触发但语音输入未开启，忽略");
        return;
    }
    if !voice_engine_ready() {
        eprintln!("[voice] 热键触发但当前语音引擎未就绪（本地缺资产 / 云端缺凭证），忽略");
        return;
    }

    // 当前是否在录：决定本次是「开录」还是「停录+转写」。
    let recording = VOICE_STATE
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(false);

    if !recording {
        // 开始录音：进入 listening 态。
        match start_recording() {
            Ok(()) => emit_voice_state(app, "listening"),
            Err(e) => {
                eprintln!("[voice] 开始录音失败: {}", e);
                let _ = app.emit("voice-error", json!({ "message": e }));
            }
        }
        return;
    }

    // 停录 → 转写 → 注入：阻塞活儿丢后台线程，回调线程立刻返回。
    // 先切 transcribing 态，让用户知道已停录、正在识别。
    emit_voice_state(app, "transcribing");
    let app = app.clone();
    std::thread::spawn(move || {
        let result = stop_transcribe_inject();
        match result {
            Ok(text) => {
                emit_voice_state(&app, "idle");
                let _ = app.emit("voice-transcribed", json!({ "text": text }));
            }
            Err(e) => {
                eprintln!("[voice] 停录/转写/注入失败: {}", e);
                emit_voice_state(&app, "idle");
                let _ = app.emit("voice-error", json!({ "message": e }));
            }
        }
    });
}

/// 按当前开关状态 + 配置键位注册/注销全局热键，并支持**换键**：
/// - `voiceInputEnabled=true` 且资产就绪 → 注册 config 的 `voiceHotkey`（无效则回退默认）。
/// - 否则 → 注销当前已注册的热键（不在关闭态占用全局热键）。
///
/// 换键正确性：用全局 `REGISTERED_HOTKEY` 记住「上次成功注册的 accelerator」。每次先按它
/// 精确注销旧键（哪怕和新键不同），再注册新键——这样改键不会残留旧热键占用。注册失败
/// （键位无效 / 被别的程序占用）返回中文 Err，且把记录清成 None（旧键已注销、新键没成功）。
/// 幂等：前端开关翻转、改键、下载完成、首启校准都调它。
pub fn sync_hotkey(app: &tauri::AppHandle) -> Result<(), String> {
    let gs = app.global_shortcut();
    let want = voice_input_enabled() && voice_engine_ready();

    let mut registered = REGISTERED_HOTKEY
        .lock()
        .map_err(|_| "热键注册状态锁中毒".to_string())?;

    // 先注销「上次注册的」键：换键时这一步保证旧键被撤、不残留占用。
    // 按记录里的字符串重新解析出 Shortcut 来撤（与注册时同一来源，确保撤的是同一个键）。
    if let Some(prev_accel) = registered.take() {
        if let Ok(prev) = prev_accel.parse::<Shortcut>() {
            if gs.is_registered(prev) {
                let _ = gs.unregister(prev);
            }
        }
    }

    if !want {
        // 关闭态：旧键已在上面注销，记录已清成 None，直接返回。
        return Ok(());
    }

    // 开启态：注册 config 的键位（无效自动回退默认）。
    let (accel, shortcut) = configured_shortcut()?;
    // 双保险：极端情况下该键可能已被注册（如外部状态不同步），先撤再注册避免「已注册」报错。
    if gs.is_registered(shortcut) {
        let _ = gs.unregister(shortcut);
    }
    gs.register(shortcut)
        .map_err(|e| format!("注册语音热键 '{}' 失败（可能被其它程序占用或键位无效）: {}", accel, e))?;

    // 注册成功才记下来，供下次换键时精确注销。
    *registered = Some(accel);
    Ok(())
}

/// 构建 global-shortcut 插件（带 toggle 处理 handler）。在 lib.rs 的 Builder 链上 `.plugin(...)`。
/// handler 对所有已注册热键触发；这里只有一个语音热键，命中 Pressed 边沿走 toggle。
/// 注意：仅 `.plugin(...)` 不会注册任何热键，实际注册要等 `sync_hotkey`（开关开启时）。
/// 用默认 Wry 运行时（全 app 一致），handler 拿到的 AppHandle 即 `tauri::AppHandle`。
pub fn global_shortcut_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                on_hotkey_pressed(app);
            }
        })
        .build()
}

// ============================================================================
// Tauri 命令（在 lib.rs invoke_handler 注册）
// ============================================================================

/// 查询语音资产就绪状态 + 下载相关信息。
///
/// 除就绪态外，一并回手动兜底所需的全部信息：代理（前端显示「下载将通过代理 …」）、
/// 目标目录、三个资产（引擎二进制包 / 模型 / 词表）的文件名与原始下载直链。
/// 前端据此渲染「手动下载」区，自动下不动时用户照链手动下、丢进目录即可。
#[tauri::command]
pub fn voice_assets_status() -> Result<Value, String> {
    #[cfg(windows)]
    let bin_name = "sherpa-onnx-offline.exe";
    #[cfg(not(windows))]
    let bin_name = "sherpa-onnx-offline";

    Ok(json!({
        "ready": voice_assets_ready(),
        "voiceDir": voice_dir().to_string_lossy(),
        "hasBinary": sherpa_offline_path().is_file(),
        "hasModel": model_path().is_file(),
        "hasTokens": tokens_path().is_file(),
        // 下载是否走代理 + 代理地址（None → 直连）。前端显示知情。
        "proxy": download_proxy(),
        // 手动兜底信息：文件名 + 原始直链。
        // binaryName 是解压后要落地的可执行名；binaryUrl 是 .tar.bz2 包（需解压，取 bin/ 下 exe+dll）。
        "binaryName": bin_name,
        "modelName": MODEL_FILE,
        "tokensName": TOKENS_FILE,
        "binaryUrl": SHERPA_BIN_URL_RAW,
        "modelUrl": MODEL_URL_RAW,
        "modelMirrorUrl": MODEL_URL,
        "tokensUrl": TOKENS_URL_RAW,
        "tokensMirrorUrl": TOKENS_URL,
    }))
}

/// 用系统文件管理器打开语音资产目录 `~/.jarvis/voice/`（手动放文件用）。
/// 目录不存在则先建——首次还没下过时点「打开目录」也不报错，直接开空目录给用户放文件。
/// 平台做法与 commands/mod.rs 的 open_url_in_browser 同款（Windows explorer / macOS open / Linux xdg-open）。
#[tauri::command]
pub fn voice_open_dir() -> Result<(), String> {
    let dir = voice_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建语音目录失败: {}", e))?;

    #[cfg(windows)]
    {
        // explorer 打开目录；用 silent_command 走 CREATE_NO_WINDOW，不弹黑窗。
        // explorer 打开成功时退出码也可能非 0，这里只看能否 spawn，不判 status。
        silent_command(&PathBuf::from("explorer"))
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("打开目录失败: {}", e))?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("打开目录失败: {}", e))?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("打开目录失败: {}", e))?;
        return Ok(());
    }
}

/// 下载语音资产到 `~/.jarvis/voice/`：① sherpa-onnx 引擎 .tar.bz2 → 解压；② SenseVoice 模型；③ 词表。
///
/// 已存在的部分跳过（幂等）：引擎齐了就不重下，模型/词表在了就不重下——支持中断后重试只补缺的。
/// 全程 emit `voice-download-progress`（phase=`"binary"`/`"model"`/`"tokens"`）。下完校验 `voice_assets_ready()`。
#[tauri::command]
pub async fn voice_download_assets(app: tauri::AppHandle) -> Result<Value, String> {
    // 读用户代理（若 config 里 channels.telegram.proxy 有值）——下载全程复用它，
    // 让原始 GitHub/HF 源在国内也能稳连，配合断点续传两道保险。
    let proxy = download_proxy();
    let proxy_ref = proxy.as_deref();

    // ① sherpa-onnx 引擎：缺就下 .tar.bz2 → 解压（解压内部会校验 sherpa-onnx-offline 在不在）。
    if !sherpa_offline_path().is_file() {
        let tar_path = voice_dir().join("sherpa-onnx-win-x64.tar.bz2");
        download_to_file_multi(&app, SHERPA_BIN_URLS, &tar_path, "binary", proxy_ref).await?;
        // 解压在阻塞线程跑：bz2 解码是 CPU 活，别占住 async 执行器。
        let tar_clone = tar_path.clone();
        tokio::task::spawn_blocking(move || extract_sherpa_tarball(&tar_clone))
            .await
            .map_err(|e| format!("解压任务调度失败: {}", e))??;
        // 解压完删掉压缩包（best-effort，省空间）。
        let _ = std::fs::remove_file(&tar_path);
    }

    // ② 模型：缺就下（约 228MB，下载耗时最久，进度务必顺滑）。
    if !model_path().is_file() {
        let model = model_path();
        download_to_file(&app, MODEL_URL, &model, "model", proxy_ref).await?;
    }

    // ③ 词表：缺就下（约 308KB，很小，但 sherpa 转写必需）。
    if !tokens_path().is_file() {
        let tokens = tokens_path();
        download_to_file(&app, TOKENS_URL, &tokens, "tokens", proxy_ref).await?;
    }

    if !voice_assets_ready() {
        return Err("下载完成但资产仍不就绪（缺引擎 / 模型 / 词表）".to_string());
    }
    Ok(json!({ "ready": true }))
}

/// 开始录音。引擎没就绪 / 已在录 → Err（同时 emit voice-error 让小人弹警告）。
#[tauri::command]
pub fn voice_start(app: tauri::AppHandle) -> Result<(), String> {
    let run = || -> Result<(), String> {
        if !voice_engine_ready() {
            return Err(if voice_engine() == "cloud" {
                "云端语音未就绪（缺 App ID / Access Token）".to_string()
            } else {
                "语音资产未就绪（缺引擎 / 模型 / 词表）".to_string()
            });
        }
        start_recording()
    };
    match run() {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("[voice] voice_start 失败: {}", e);
            let _ = app.emit("voice-error", json!({ "message": e.clone() }));
            Err(e)
        }
    }
}

/// 停录 → 转写（按引擎分流）→ 注入聚焦框 → 返回转写文本。
/// 命令层与热键处理共用这段，错误分清「没在录」「转写失败」「注入失败」三类。
/// 每个阶段都 eprintln 留痕，配合各子函数内的诊断，全链路可见卡在哪一步。
///
/// 引擎分流（读 config 的 voiceEngine）：
///   - "local"（默认）→ 写临时 WAV → sherpa-onnx-offline 子进程转写。
///   - "cloud"        → 不写 WAV，直接把 16k 单声道 f32 发火山云端 ASR。
/// 录音、注入、热键、小人状态全链路两路共用，仅中间「转写」一段按引擎换实现。
fn stop_transcribe_inject() -> Result<String, String> {
    eprintln!("[voice] === 阶段1/4 停录并取样 ===");
    let samples = stop_recording()?;
    // stop_recording 已对「采集样本为 0」做明确报错，这里是双保险。
    if samples.is_empty() {
        return Err("没有采集到音频（录音太短或麦克风无输入）".to_string());
    }

    let engine = voice_engine();
    let text = if engine == "cloud" {
        // 云端：直接发裸 PCM，不落 WAV。async 流程在当前线程的临时 tokio 运行时里跑完
        //（本函数是同步上下文，由热键后台线程 / 同步命令调用，无保证的环境运行时）。
        eprintln!("[voice] === 阶段2-3/4 云端转写（火山引擎）===");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("创建云端转写运行时失败: {}", e))?;
        rt.block_on(transcribe_volcengine(&samples))?
    } else {
        eprintln!("[voice] === 阶段2/4 写 WAV ===");
        let wav = write_temp_wav(&samples)?;

        eprintln!("[voice] === 阶段3/4 sherpa-onnx-offline 转写 ===");
        let text_result = transcribe_wav(&wav);
        // 转写产物 WAV 用完即删（best-effort）。
        let _ = std::fs::remove_file(&wav);
        text_result?
    };

    if text.is_empty() {
        return Err("转写结果为空（未识别到语音内容）".to_string());
    }

    eprintln!("[voice] === 阶段4/4 注入聚焦输入框 ===");
    inject_text(&text)?;
    eprintln!("[voice] === 全链路完成 ===");
    Ok(text)
}

/// 停录 → 写 WAV → sherpa-onnx-offline 转写 → 注入聚焦框 → 返回 `{text}`。
/// 失败时 emit voice-error（带中文原因），别让命令路径静默失败。
#[tauri::command]
pub fn voice_stop_and_transcribe(app: tauri::AppHandle) -> Result<Value, String> {
    match stop_transcribe_inject() {
        Ok(text) => {
            let _ = app.emit("voice-transcribed", json!({ "text": text.clone() }));
            Ok(json!({ "text": text }))
        }
        Err(e) => {
            eprintln!("[voice] voice_stop_and_transcribe 失败: {}", e);
            let _ = app.emit("voice-error", json!({ "message": e.clone() }));
            Err(e)
        }
    }
}

/// 按当前开关状态注册/注销全局热键。前端在开关翻转、改键、下载完成、首启校准时调用。
/// 返回当前是否已注册热键 + 实际生效的键位（读 config 的 voiceHotkey，无效则回退默认）。
#[tauri::command]
pub fn voice_hotkey_sync(app: tauri::AppHandle) -> Result<Value, String> {
    sync_hotkey(&app)?;
    // 回实际生效键位：configured_shortcut 已处理无效回退，取它的 accelerator 最准。
    let hotkey = configured_shortcut()
        .map(|(accel, _)| accel)
        .unwrap_or_else(|_| DEFAULT_HOTKEY.to_string());
    Ok(json!({
        "registered": voice_input_enabled() && voice_engine_ready(),
        "hotkey": hotkey,
    }))
}

/// 查云端（火山）语音是否就绪：App ID + Access Token 都配齐即 ready。
/// 前端选「云端」并启用时调它做凭证校验——云端不需要下载模型，不走本地那套下载确认框。
/// 缺凭证时回 `{ ready:false, message }`，前端据此提示去控制台开通；不抛错（避免吓人）。
#[tauri::command]
pub fn voice_cloud_status() -> Result<Value, String> {
    match volc_credentials() {
        Ok(_) => Ok(json!({ "ready": true })),
        Err(e) => Ok(json!({ "ready": false, "message": e })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_passthrough_for_single_channel() {
        let s = vec![0.1, 0.2, 0.3];
        assert_eq!(to_mono(&s, 1), s);
    }

    #[test]
    fn mono_averages_stereo_frames() {
        // 双声道交错 [L,R,L,R] → 每帧求平均。
        let stereo = vec![0.0, 1.0, 0.5, -0.5];
        assert_eq!(to_mono(&stereo, 2), vec![0.5, 0.0]);
    }

    #[test]
    fn resample_passthrough_when_already_16k() {
        let s = vec![0.1, 0.2, 0.3, 0.4];
        assert_eq!(resample_to_16k(&s, TARGET_SAMPLE_RATE), s);
    }

    #[test]
    fn resample_48k_to_16k_thirds_the_length() {
        // 48k → 16k 约 1/3 长度。
        let s: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.001).sin()).collect();
        let out = resample_to_16k(&s, 48_000);
        // 长度约为 1/3（线性插值取 floor，允许 ±1）。
        let expected = 48_000u64 * TARGET_SAMPLE_RATE as u64 / 48_000u64;
        assert!(
            (out.len() as i64 - expected as i64).abs() <= 1,
            "重采样后长度 {} 偏离期望 {}",
            out.len(),
            expected
        );
    }

    #[test]
    fn resample_empty_returns_empty() {
        assert!(resample_to_16k(&[], 48_000).is_empty());
    }

    #[test]
    fn assets_paths_under_jarvis_voice() {
        // 路径拼接正确性：都在 ~/.jarvis/voice/ 下。
        assert!(voice_dir().ends_with("voice"));
        assert!(model_path().ends_with(MODEL_FILE));
        assert!(tokens_path().ends_with(TOKENS_FILE));
        let cli = sherpa_offline_path();
        #[cfg(windows)]
        assert!(cli.ends_with("sherpa-onnx-offline.exe"));
        #[cfg(not(windows))]
        assert!(cli.ends_with("sherpa-onnx-offline"));
    }

    #[test]
    fn parse_content_range_total_extracts_total() {
        // 标准 `bytes N-M/TOTAL`：取末段总长。
        assert_eq!(parse_content_range_total("bytes 100-573/574"), Some(574));
        assert_eq!(parse_content_range_total("bytes 0-0/12345"), Some(12345));
        // 总长未知（`*`）或脏数据 → None。
        assert_eq!(parse_content_range_total("bytes 100-573/*"), None);
        assert_eq!(parse_content_range_total("garbage"), None);
    }

    #[test]
    fn resolve_redirect_handles_absolute_and_relative() {
        let base = "https://hf-mirror.com/a/b/file.bin";
        // 绝对地址原样返回。
        assert_eq!(
            resolve_redirect(base, "https://cas-bridge.xethub.hf.co/x").unwrap(),
            "https://cas-bridge.xethub.hf.co/x"
        );
        // 协议相对 `//host/..` 补 https。
        assert_eq!(
            resolve_redirect(base, "//cdn.example.com/y").unwrap(),
            "https://cdn.example.com/y"
        );
        // 绝对路径 `/p` 拼到 origin。
        assert_eq!(
            resolve_redirect(base, "/p/q.bin").unwrap(),
            "https://hf-mirror.com/p/q.bin"
        );
    }

    #[test]
    fn parse_sherpa_text_extracts_text_field() {
        // sherpa-onnx-offline 的 stdout：一行结果 JSON，取 text 字段（去空白）。
        let stdout = r#"{"lang": "<|zh|>", "emotion": "<|NEUTRAL|>", "event": "<|Speech|>", "text": "今天天气不错。", "timestamps": [], "tokens": []}"#;
        assert_eq!(parse_sherpa_text(stdout), "今天天气不错。");
    }

    #[test]
    fn parse_sherpa_text_skips_non_json_lines_and_picks_json() {
        // 前面混入非 JSON 行（如意外打到 stdout 的诊断），仍能挑出带 text 的 JSON 行。
        let stdout = "some noise line\n{\"text\": \"hello world\"}\n";
        assert_eq!(parse_sherpa_text(stdout), "hello world");
    }

    #[test]
    fn parse_sherpa_text_falls_back_to_trimmed_stdout() {
        // 完全没有合法 JSON → 兜底返回整体 trim（不让格式微调直接报错）。
        assert_eq!(parse_sherpa_text("  plain text  "), "plain text");
    }

    #[test]
    fn sherpa_threads_within_bounds() {
        // 线程数恒在 [1, 8]，不依赖具体机器核数。
        let t = sherpa_threads();
        assert!((1..=8).contains(&t), "线程数 {} 不在 [1,8]", t);
    }

    // ===== 云端（火山）协议相关 =====

    #[test]
    fn gzip_roundtrip() {
        // gzip 压缩→解压还原（init JSON / 音频块发送前都靠这压；响应靠这解）。
        let data = b"the quick brown fox \xe4\xbd\xa0\xe5\xa5\xbd"; // 含 UTF-8 中文字节
        let gz = gzip_compress(data).unwrap();
        assert_eq!(gzip_decompress(&gz).unwrap(), data);
    }

    #[test]
    fn volc_marshal_header_layout() {
        // 头四字节布局：[(ver<<4)|hsize, (type<<4)|flag, (ser<<4)|comp, 0x00]。
        let payload = b"abc";
        let frame = volc_marshal(VOLC_MSG_FULL_CLIENT, VOLC_FLAG_POS_SEQ, VOLC_SER_JSON, 1, payload);
        assert_eq!(frame[0], (VOLC_PROTOCOL_VERSION << 4) | VOLC_HEADER_SIZE); // 0x11
        assert_eq!(frame[1], (VOLC_MSG_FULL_CLIENT << 4) | VOLC_FLAG_POS_SEQ);
        assert_eq!(frame[2], (VOLC_SER_JSON << 4) | VOLC_COMP_GZIP);
        assert_eq!(frame[3], 0x00);
        // 序号(i32 大端)=1，紧跟 payload 长度(u32 大端)=3。
        assert_eq!(&frame[4..8], &1i32.to_be_bytes());
        assert_eq!(&frame[8..12], &3u32.to_be_bytes());
        assert_eq!(&frame[12..], payload);
    }

    #[test]
    fn volc_parse_roundtrips_server_frame() {
        // 用 marshal 造一个「服务端」帧（payload gzip 后），parse 应还原类型/标志/解压后的 payload。
        let body = r#"{"result":{"text":"你好世界","is_final":true}}"#.as_bytes();
        let gz = gzip_compress(body).unwrap();
        // 服务端帧带 LastNoSeq 标志（无序号字段），serialization=JSON, compression=Gzip。
        let mut frame = vec![
            (VOLC_PROTOCOL_VERSION << 4) | VOLC_HEADER_SIZE,
            (VOLC_MSG_FULL_SERVER << 4) | VOLC_FLAG_LAST_NO_SEQ,
            (VOLC_SER_JSON << 4) | VOLC_COMP_GZIP,
            0x00,
        ];
        frame.extend_from_slice(&(gz.len() as u32).to_be_bytes());
        frame.extend_from_slice(&gz);

        let parsed = volc_parse(&frame).unwrap();
        assert_eq!(parsed.msg_type, VOLC_MSG_FULL_SERVER);
        assert!(parsed.flags & VOLC_FLAG_LAST_NO_SEQ != 0);
        let (text, is_final) = volc_extract_text(&parsed.payload).unwrap();
        assert_eq!(text, "你好世界");
        assert!(is_final);
    }

    #[test]
    fn volc_parse_rejects_truncated_frame() {
        // 越界保护：声明 payload 100 字节但实际没有 → Err 而非 panic（按字符/字节切片越界铁律）。
        let mut frame = vec![
            (VOLC_PROTOCOL_VERSION << 4) | VOLC_HEADER_SIZE,
            VOLC_MSG_FULL_SERVER << 4, // NoSeq：无序号字段
            VOLC_SER_JSON << 4,        // 不压缩
            0x00,
        ];
        frame.extend_from_slice(&100u32.to_be_bytes()); // 声明 100 字节但后面啥都没有
        assert!(volc_parse(&frame).is_err());
        // 比 4 字节头还短：也应 Err。
        assert!(volc_parse(&[0x11, 0x90]).is_err());
    }

    #[test]
    fn samples_to_pcm_le_clamps_and_packs() {
        // f32[-1,1] → i16 小端：+1.0→32767, 0→0, 超界 clamp。每样本 2 字节。
        let pcm = samples_to_pcm_le(&[0.0, 1.0, -1.0, 2.0]);
        assert_eq!(pcm.len(), 8);
        assert_eq!(&pcm[0..2], &0i16.to_le_bytes());
        assert_eq!(&pcm[2..4], &i16::MAX.to_le_bytes());
        assert_eq!(&pcm[4..6], &(-i16::MAX).to_le_bytes()); // -1.0*MAX
        assert_eq!(&pcm[6..8], &i16::MAX.to_le_bytes()); // 2.0 clamp 到 1.0
    }

    #[test]
    fn volc_uuid_shape() {
        // 形如 8-4-4-4-12，version 位为 4。
        let u = volc_uuid();
        let parts: Vec<&str> = u.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), vec![8, 4, 4, 4, 12]);
        assert!(parts[2].starts_with('4'));
        // 两次生成不相等。
        assert_ne!(volc_uuid(), volc_uuid());
    }
}
