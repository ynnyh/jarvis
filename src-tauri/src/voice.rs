// 轻量语音输入（PR1：依赖 + 录音→转写→注入后端一条龙，不含下载 UI / 热键 / 设置 section）。
//
// 链路：start_recording（cpal 采音）→ stop_recording（停流、归一到 16kHz 单声道 f32）
//   → 写临时 WAV（hound）→ whisper-cli 子进程转写 → arboard+enigo 注入聚焦输入框。
//
// ---- 关键约束（已实测决定）----
// 本机 **没有 CMake / clang / libclang**，CI 也没有。所以 STT 不走 whisper-rs / 不现编
// whisper.cpp（它们要 cmake+libclang，编不过）。改用 whisper.cpp 官方**预编译命令行二进制
// （whisper-cli）当子进程调用** + ggml 模型。二进制和模型都假定资产已在约定路径下
// （`~/.jarvis/voice/`），下载 UI 是 PR2，本模块只在缺资产时报「没就绪」。
// 全链路只用纯 Rust 依赖（cpal / hound / enigo / arboard），无需编译工具。
//
// ---- 全局录音状态 ----
// 参考 mcp_client.rs 的 `once_cell::Lazy` + `Arc<Mutex<..>>` 全局单例范式：维护一个全局
// VOICE_STATE。cpal 的回调在音频线程触发，只往共享 buffer push 样本（不阻塞、不持锁久）。
// cpal `Stream` 在 WASAPI/CoreAudio 上是 Send（自带音频线程持有 COM 对象），故可存进全局
// Mutex，录音期间靠 buffer 累积、stop 时 drop 掉 Stream 收尾。

#![allow(dead_code)] // PR1 部分函数（voice_dir / model_path 等）供 PR2 下载/设置接入时才被调用

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

/// whisper 要求的目标采样率：16 kHz 单声道 f32。
const TARGET_SAMPLE_RATE: u32 = 16_000;

/// 全局触发热键（toggle）的**默认值**。用户没配 / 配的无效时回退到它。
/// `CommandOrControl+Shift+Space`：macOS 上是 ⌘，Windows/Linux 上是 Ctrl，
/// 加 Shift+Space 这组不易和常见软件冲突。实际生效键位读 config 的 `voiceHotkey`。
const DEFAULT_HOTKEY: &str = "CommandOrControl+Shift+Space";

/// 默认模型文件名：large-v3-turbo 量化版（q5_0，约 574MB）。
/// turbo 是 2024 下半年加的解码器，转写又快又准，中英混输够用；q5_0 量化压体积。
const DEFAULT_MODEL_FILE: &str = "ggml-large-v3-turbo-q5_0.bin";

/// 转写语言的**默认值**：锁定中文（`zh`）。用户群日常中文为主、夹杂英文术语，
/// `-l auto` 在短句上会误判成日/韩等导致整段乱码，固定 zh 比 auto 稳得多
/// （whisper 在 zh 模式下照样能识别夹在中文里的英文术语）。实际生效读 config 的 `voiceLanguage`。
const DEFAULT_VOICE_LANGUAGE: &str = "zh";

/// 术语提示词（initial prompt）的**固定前缀**：交代「中文夹英文术语」的语境，
/// 让 whisper 偏向按这个语境解码。后面拼用户配的 `voiceTerms` 一起喂进去。
const PROMPT_PREFIX: &str = "以下是中文语音，可能夹杂英文技术术语：";

/// 术语提示词的**默认术语表**：用户没配 `voiceTerms` 时塞这一小撮常见开发术语，
/// 偏置英文技术名词的转写正确率。用户可在设置里改成自己项目的技术栈/项目名。
const DEFAULT_VOICE_TERMS: &str =
    "API, bug, deploy, commit, merge, Docker, Kubernetes, Redis, PR, token";

/// 初始提示词的字符上限。whisper 初始提示按 token 截断（约 224 token），
/// 中文 1 字≈1~2 token，这里按字符保守截到 200，避免 prompt 过长挤占解码窗口或被硬截断。
const MAX_PROMPT_CHARS: usize = 200;

// ============================================================================
// 资产下载源（已用 GitHub API + whisper.cpp 源码核实，见任务 research）
// ============================================================================
//
// whisper-cli 预编译二进制：whisper.cpp 官方 release 的 CPU 版 zip。
//   - 仓库已从 ggerganov 迁到 ggml-org（旧名仍 302 重定向，但用新名最稳）。
//   - 选 v1.8.6（2026-06-02 发布，远晚于 turbo 加入时间 → 支持 large-v3-turbo）。
//   - whisper-bin-x64.zip 是纯 CPU 构建（4.1MB，不带 CUDA/BLAS），最轻、无显卡依赖。
//   - zip 内是 Compress-Archive 打的 build/bin 全量（已核实 examples/cli/CMakeLists.txt
//     里 `set(TARGET whisper-cli)`）：whisper-cli.exe + whisper.dll / ggml.dll /
//     ggml-base.dll / ggml-cpu.dll / SDL2.dll，全平铺在 zip 根，解压到 voice_dir 即可用。
//
// 二进制走 GitHub，国内直连常超时/被墙（实测报 `error sending request for url`，且 app 的
// reqwest 不走系统代理）。故按顺序尝试一组镜像，谁先连上并下完就用谁：
//   1. ghfast.top 国内加速镜像（已实测 HTTP 206 可拉，国内优先）；
//   2. GitHub 直连（海外 / 有代理时兜底）。
// ghfast.top 会回 302/206，reqwest 默认跟随重定向，无需额外处理。
const WHISPER_BIN_URLS: &[&str] = &[
    "https://ghfast.top/https://github.com/ggml-org/whisper.cpp/releases/download/v1.8.6/whisper-bin-x64.zip",
    "https://github.com/ggml-org/whisper.cpp/releases/download/v1.8.6/whisper-bin-x64.zip",
];

/// 模型直链：走国内可达的 hf-mirror.com 镜像。
/// 实测：huggingface.co 在国内被墙不通；whisper.cpp 的 HF 仓库虽已从 ggerganov 改名 ggml-org，
/// 但 hf-mirror.com 镜像仍只认旧名 ggerganov（HEAD 返回 200，新名在镜像上不通）。
/// 注意：hf-mirror 对大文件只回 302 跳到 HF 美国 Xet 存储（cas-bridge.xethub.hf.co），
/// 574MB 从美国直传国内必断流。故配合「走用户代理 + 断点续传」两道保险才稳（见下方下载逻辑）。
const MODEL_URL: &str =
    "https://hf-mirror.com/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin";

/// 模型 HF 原始直链（手动兜底用：给用户在浏览器/下载工具里手动下）。
/// 自动下载默认走 hf-mirror（配代理时直连 HF 也行），但手动场景把原始链一并列出。
const MODEL_URL_RAW: &str =
    "https://huggingface.co/ggml-org/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin";

/// whisper-cli 二进制 zip 的 GitHub 原始直链（手动兜底用，列给用户看）。
const WHISPER_BIN_URL_RAW: &str =
    "https://github.com/ggml-org/whisper.cpp/releases/download/v1.8.6/whisper-bin-x64.zip";

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

/// 语音资产目录 `~/.jarvis/voice/`（whisper-cli 可执行 + ggml 模型都放这）。
pub fn voice_dir() -> PathBuf {
    jarvis_dir().join("voice")
}

/// whisper-cli 可执行路径。Windows 用 `whisper-cli.exe`，其它平台 `whisper-cli`。
pub fn whisper_cli_path() -> PathBuf {
    #[cfg(windows)]
    let name = "whisper-cli.exe";
    #[cfg(not(windows))]
    let name = "whisper-cli";
    voice_dir().join(name)
}

/// 默认模型路径 `~/.jarvis/voice/ggml-large-v3-turbo-q5_0.bin`。
pub fn model_path() -> PathBuf {
    voice_dir().join(DEFAULT_MODEL_FILE)
}

/// 语音资产是否就绪：whisper-cli 二进制 + 模型都存在。
pub fn voice_assets_ready() -> bool {
    whisper_cli_path().is_file() && model_path().is_file()
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
/// 用 16-bit PCM（whisper-cli 通吃；体积也比 f32 小一半）。落 `std::env::temp_dir()`。
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
// 转写（whisper-cli 子进程）
// ============================================================================

/// 读 `~/.jarvis/config.json` 里的 `voiceLanguage`（trim 后非空才用）；缺/空回退默认 `zh`。
/// 与 voiceHotkey 同范式：只读字符串，whisper 不认的语言码由它自己处理（极端兜底是 zh）。
fn voice_language() -> String {
    crate::settings::load_raw_config()
        .and_then(|v| {
            v.get("voiceLanguage")
                .and_then(|s| s.as_str())
                .map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_VOICE_LANGUAGE.to_string())
}

/// 读 `~/.jarvis/config.json` 里的 `voiceTerms`（用户常用术语，逗号/换行分隔）。
/// 字段缺失 → 用默认术语表；字段存在但 trim 后为空（用户特意清空）→ 返回空串（不加术语）。
fn voice_terms() -> String {
    match crate::settings::load_raw_config().and_then(|v| {
        v.get("voiceTerms")
            .and_then(|s| s.as_str())
            .map(str::to_string)
    }) {
        Some(s) => s.trim().to_string(),
        None => DEFAULT_VOICE_TERMS.to_string(),
    }
}

/// 拼 whisper 初始提示词（initial prompt）：固定前缀 + 用户术语，按字符截断到 `MAX_PROMPT_CHARS`。
/// 术语为空 → 只回前缀（仍交代「中文夹英文」语境）。截断按字符边界（避免 panic / 切坏多字节）。
fn build_prompt(terms: &str) -> String {
    let mut prompt = String::from(PROMPT_PREFIX);
    if !terms.is_empty() {
        prompt.push_str(terms);
    }
    if prompt.chars().count() > MAX_PROMPT_CHARS {
        prompt = prompt.chars().take(MAX_PROMPT_CHARS).collect();
    }
    prompt
}

/// whisper-cli 的 `-t` 线程数：取 CPU 可用并行度，封顶 8。
/// 纯 CPU 推理拉满线程是最直接的提速；封顶 8 是经验值——再多线程在 whisper 上收益递减、
/// 且会和系统其它进程抢核。探测失败（拿不到 available_parallelism）回退到 4。
fn whisper_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8)
}

/// 跑 whisper-cli 把 WAV 转成文本。
///
/// 命令：`whisper-cli -m <model> -f <wav> -t <N> -l <lang> --prompt <提示词> -otxt -nt`
///   - `-t <N>`：CPU 线程数（拉满核心，见 whisper_threads，纯 CPU 下最明显的提速）
///   - `-l <lang>`：转写语言，读 config 的 voiceLanguage（默认 zh，空回退 zh）
///   - `--prompt`：初始提示词，交代「中文夹英文术语」语境 + 用户常用术语，提升英文术语转写正确率
///   - `-otxt`：输出纯文本到 `<wav>.txt`
///   - `-nt`：no timestamps，去时间戳
/// whisper-cli 既会写 `<wav>.txt` 也会把转写打到 stdout。优先读 `.txt`（最稳），
/// 读不到再退回 stdout。Windows 用 CREATE_NO_WINDOW 防黑窗。
fn transcribe_wav(wav_path: &PathBuf) -> Result<String, String> {
    let cli = whisper_cli_path();
    let model = model_path();
    let txt_path = {
        // whisper-cli 的 -otxt 会在输入文件名后追加 .txt（如 a.wav → a.wav.txt）。
        let mut p = wav_path.clone().into_os_string();
        p.push(".txt");
        PathBuf::from(p)
    };

    // 语言：空回退 zh（whisper 的 zh 模式下照样能识别夹在中文里的英文术语，比 auto 短句误判稳）。
    let lang = {
        let l = voice_language();
        if l.is_empty() {
            DEFAULT_VOICE_LANGUAGE.to_string()
        } else {
            l
        }
    };
    // 线程数 + 提示词（提示词 = 固定前缀 + 用户术语，按字符截断防超 token 上限）。
    let threads = whisper_threads();
    let threads_str = threads.to_string();
    let prompt = build_prompt(&voice_terms());

    let mut cmd = silent_command(&cli);
    cmd.arg("-m")
        .arg(&model)
        .arg("-f")
        .arg(wav_path)
        .args(["-t", &threads_str])
        .args(["-l", &lang])
        .args(["--prompt", &prompt])
        .args(["-otxt", "-nt"]);

    // 诊断：打完整命令行，便于复现/排查（模型缺失、参数错、二进制 DLL 缺失都从这看）。
    eprintln!(
        "[voice] 运行 whisper-cli：{} -m {} -f {} -t {} -l {} --prompt \"{}\" -otxt -nt",
        cli.display(),
        model.display(),
        wav_path.display(),
        threads,
        lang,
        prompt
    );

    let output = cmd
        .output()
        .map_err(|e| format!("启动 whisper-cli 失败: {}（路径 {}，缺 .exe 或同目录 DLL？）", e, cli.display()))?;

    // 始终打印 exit code + stderr 摘要（whisper 的进度/模型加载日志都走 stderr）。
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout_raw = String::from_utf8_lossy(&output.stdout);
    eprintln!(
        "[voice] whisper-cli 退出码={:?}\n[voice] --- whisper stderr ---\n{}\n[voice] --- whisper stdout ---\n{}\n[voice] --- end ---",
        output.status.code(),
        stderr.trim(),
        stdout_raw.trim()
    );

    if !output.status.success() {
        // 取 stderr 末尾若干行作摘要带回前端（whisper 报错通常在最后几行）。
        let summary = stderr_tail(&stderr, 5);
        return Err(format!(
            "whisper-cli 转写失败（退出码 {:?}）：{}",
            output.status.code(),
            summary
        ));
    }

    // 优先读 .txt 输出文件，失败再退回 stdout。
    let from_txt = std::fs::read_to_string(&txt_path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let text = from_txt.unwrap_or_else(|| stdout_raw.trim().to_string());
    eprintln!(
        "[voice] 转写文本（{} 字）：{}",
        text.chars().count(),
        text
    );

    // 清理临时产物（best-effort，失败不影响结果）。
    let _ = std::fs::remove_file(&txt_path);

    // 转写为空：带上 whisper stderr 摘要，便于判断是音频空 / 模型加载失败 / 参数问题。
    if text.is_empty() {
        return Err(format!(
            "转写结果为空（未识别到语音内容）。whisper 日志：{}",
            stderr_tail(&stderr, 4)
        ));
    }

    Ok(text)
}

/// 取 stderr 末尾 n 行作错误摘要（whisper 报错/关键信息一般在最后几行）。
/// 全空则给个占位，避免把空串塞进错误消息让用户一头雾水。
fn stderr_tail(stderr: &str, n: usize) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return "（whisper 无错误输出，疑似音频为空或模型未正确加载）".to_string();
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

/// 解压 whisper-cli zip 到 `voice_dir()`。
///
/// 官方 zip 是 Compress-Archive 打的 `build/bin` 全量（whisper-cli.exe + 一堆 DLL，平铺在根）。
/// 用纯 Rust `zip` crate（deflate-flate2 后端，无需 cmake）逐条解出，**只取文件名**（防 zip-slip：
/// 丢弃任何带路径分隔/`..` 的条目），平铺写进 voice_dir。
fn extract_whisper_zip(zip_path: &PathBuf) -> Result<(), String> {
    let dir = voice_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建语音目录失败: {}", e))?;

    let file = std::fs::File::open(zip_path).map_err(|e| format!("打开下载的 zip 失败: {}", e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("读取 zip 失败: {}", e))?;

    let mut extracted = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取 zip 条目失败: {}", e))?;
        if entry.is_dir() {
            continue;
        }
        // 防 zip-slip：只用 enclosed_name 的最末文件名，平铺到 voice_dir。
        let name = match entry.enclosed_name() {
            Some(p) => match p.file_name() {
                Some(f) => f.to_owned(),
                None => continue,
            },
            None => continue,
        };
        let out_path = dir.join(&name);
        let mut out =
            std::fs::File::create(&out_path).map_err(|e| format!("写解压文件失败: {}", e))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("解压拷贝失败: {}", e))?;
        extracted += 1;
    }

    if extracted == 0 {
        return Err("zip 解压后没有任何文件".to_string());
    }
    if !whisper_cli_path().is_file() {
        return Err(format!(
            "解压完成但缺 whisper-cli 可执行（期望 {}）",
            whisper_cli_path().display()
        ));
    }
    Ok(())
}

// ============================================================================
// 全局热键（tauri-plugin-global-shortcut，toggle 触发）
// ============================================================================
//
// 设计：注册逻辑全在 Rust，前端只在开关翻转 / 启动时调一次 `voice_hotkey_sync`。
// 热键命中后按 toggle：没在录 → 开录；正在录 → 停录+转写+注入。后者阻塞（whisper
// 子进程要几秒），丢到后台线程跑，避免卡住 global-shortcut 的回调线程。
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
    if !voice_assets_ready() {
        eprintln!("[voice] 热键触发但语音资产未就绪，忽略");
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
    let want = voice_input_enabled() && voice_assets_ready();

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
/// 目标目录、两个文件名、各自的原始下载直链（二进制 zip 解压后平铺、模型 .bin 直接放）。
/// 前端据此渲染「手动下载」区，自动下不动时用户照链手动下、丢进目录即可。
#[tauri::command]
pub fn voice_assets_status() -> Result<Value, String> {
    #[cfg(windows)]
    let bin_name = "whisper-cli.exe";
    #[cfg(not(windows))]
    let bin_name = "whisper-cli";

    Ok(json!({
        "ready": voice_assets_ready(),
        "voiceDir": voice_dir().to_string_lossy(),
        "hasBinary": whisper_cli_path().is_file(),
        "hasModel": model_path().is_file(),
        // 下载是否走代理 + 代理地址（None → 直连）。前端显示知情。
        "proxy": download_proxy(),
        // 手动兜底信息：文件名 + 原始直链。
        "binaryName": bin_name,
        "modelName": DEFAULT_MODEL_FILE,
        "binaryUrl": WHISPER_BIN_URL_RAW,
        "modelUrl": MODEL_URL_RAW,
        "modelMirrorUrl": MODEL_URL,
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
        return Ok(());
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

/// 下载语音资产到 `~/.jarvis/voice/`：① whisper-cli 二进制 zip → 解压；② large-v3-turbo 模型。
///
/// 已存在的部分跳过（幂等）：二进制齐了就不重下，模型在了就不重下——支持中断后重试只补缺的。
/// 全程 emit `voice-download-progress`（phase=`"binary"`/`"model"`）。下完校验 `voice_assets_ready()`。
#[tauri::command]
pub async fn voice_download_assets(app: tauri::AppHandle) -> Result<Value, String> {
    // 读用户代理（若 config 里 channels.telegram.proxy 有值）——下载全程复用它，
    // 让原始 GitHub/HF 源在国内也能稳连，配合断点续传两道保险。
    let proxy = download_proxy();
    let proxy_ref = proxy.as_deref();

    // ① whisper-cli 二进制：缺就下 zip → 解压（解压内部会校验 whisper-cli 在不在）。
    if !whisper_cli_path().is_file() {
        let zip_path = voice_dir().join("whisper-bin-x64.zip");
        download_to_file_multi(&app, WHISPER_BIN_URLS, &zip_path, "binary", proxy_ref).await?;
        extract_whisper_zip(&zip_path)?;
        // 解压完删掉 zip（best-effort，省空间）。
        let _ = std::fs::remove_file(&zip_path);
    }

    // ② 模型：缺就下（574MB，下载耗时最久，进度务必顺滑）。
    if !model_path().is_file() {
        let model = model_path();
        download_to_file(&app, MODEL_URL, &model, "model", proxy_ref).await?;
    }

    if !voice_assets_ready() {
        return Err("下载完成但资产仍不就绪（缺 whisper-cli 或模型）".to_string());
    }
    Ok(json!({ "ready": true }))
}

/// 开始录音。资产没就绪 / 已在录 → Err（同时 emit voice-error 让小人弹警告）。
#[tauri::command]
pub fn voice_start(app: tauri::AppHandle) -> Result<(), String> {
    let run = || -> Result<(), String> {
        if !voice_assets_ready() {
            return Err("语音资产未就绪（缺 whisper-cli 或模型）".to_string());
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

/// 停录 → 写 WAV → whisper-cli 转写 → 注入聚焦框 → 返回转写文本。
/// 命令层与热键处理共用这段，错误分清「没在录」「转写失败」「注入失败」三类。
/// 每个阶段都 eprintln 留痕，配合各子函数内的诊断，全链路可见卡在哪一步。
fn stop_transcribe_inject() -> Result<String, String> {
    eprintln!("[voice] === 阶段1/4 停录并取样 ===");
    let samples = stop_recording()?;
    // stop_recording 已对「采集样本为 0」做明确报错，这里是双保险。
    if samples.is_empty() {
        return Err("没有采集到音频（录音太短或麦克风无输入）".to_string());
    }

    eprintln!("[voice] === 阶段2/4 写 WAV ===");
    let wav = write_temp_wav(&samples)?;

    eprintln!("[voice] === 阶段3/4 whisper-cli 转写 ===");
    let text_result = transcribe_wav(&wav);
    // 转写产物 WAV 用完即删（best-effort）。
    let _ = std::fs::remove_file(&wav);
    let text = text_result?;

    if text.is_empty() {
        return Err("转写结果为空（未识别到语音内容）".to_string());
    }

    eprintln!("[voice] === 阶段4/4 注入聚焦输入框 ===");
    inject_text(&text)?;
    eprintln!("[voice] === 全链路完成 ===");
    Ok(text)
}

/// 停录 → 写 WAV → whisper-cli 转写 → 注入聚焦框 → 返回 `{text}`。
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
        "registered": voice_input_enabled() && voice_assets_ready(),
        "hotkey": hotkey,
    }))
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
        assert!(model_path().ends_with(DEFAULT_MODEL_FILE));
        let cli = whisper_cli_path();
        #[cfg(windows)]
        assert!(cli.ends_with("whisper-cli.exe"));
        #[cfg(not(windows))]
        assert!(cli.ends_with("whisper-cli"));
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
    fn build_prompt_includes_prefix_and_terms() {
        let p = build_prompt("API, Docker");
        assert!(p.starts_with(PROMPT_PREFIX));
        assert!(p.contains("API, Docker"));
    }

    #[test]
    fn build_prompt_empty_terms_is_prefix_only() {
        assert_eq!(build_prompt(""), PROMPT_PREFIX);
    }

    #[test]
    fn build_prompt_truncates_by_chars_not_bytes() {
        // 超长术语（全中文，多字节）应按字符截断到上限，且不 panic、不切坏多字节。
        let long_terms = "术语".repeat(300);
        let p = build_prompt(&long_terms);
        assert_eq!(p.chars().count(), MAX_PROMPT_CHARS);
        // 截断点落在字符边界：能无损还原成字符串即证明没切坏 UTF-8。
        assert_eq!(p.chars().count(), p.chars().collect::<String>().chars().count());
    }

    #[test]
    fn whisper_threads_within_bounds() {
        // 线程数恒在 [1, 8]，不依赖具体机器核数。
        let t = whisper_threads();
        assert!((1..=8).contains(&t), "线程数 {} 不在 [1,8]", t);
    }
}
