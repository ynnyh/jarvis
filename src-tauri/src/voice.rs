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

/// 全局触发热键（toggle）。设为常量便于以后做成可配置项。
/// `CommandOrControl+Shift+Space`：macOS 上是 ⌘，Windows/Linux 上是 Ctrl，
/// 加 Shift+Space 这组不易和常见软件冲突。
const DEFAULT_HOTKEY: &str = "CommandOrControl+Shift+Space";

/// 默认模型文件名：large-v3-turbo 量化版（q5_0，约 574MB）。
/// turbo 是 2024 下半年加的解码器，转写又快又准，中英混输够用；q5_0 量化压体积。
const DEFAULT_MODEL_FILE: &str = "ggml-large-v3-turbo-q5_0.bin";

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
const MODEL_URL: &str =
    "https://hf-mirror.com/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin";

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
    let supported = device
        .default_input_config()
        .map_err(|e| format!("读取麦克风默认配置失败: {}", e))?;

    // cpal 0.17：SampleRate 是 `type SampleRate = u32` 别名，直接拿数值（无 .0）。
    let sample_rate = supported.sample_rate();
    let channels = supported.channels();
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

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

    let mono = to_mono(&raw, recording.channels);
    let resampled = resample_to_16k(&mono, recording.sample_rate);
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
    Ok(path)
}

// ============================================================================
// 转写（whisper-cli 子进程）
// ============================================================================

/// 跑 whisper-cli 把 WAV 转成文本。
///
/// 命令：`whisper-cli -m <model> -f <wav> -l auto -otxt -nt`
///   - `-l auto`：自动语言（中英混输）
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

    let mut cmd = silent_command(&cli);
    cmd.arg("-m")
        .arg(&model)
        .arg("-f")
        .arg(wav_path)
        .args(["-l", "auto", "-otxt", "-nt"]);

    let output = cmd
        .output()
        .map_err(|e| format!("启动 whisper-cli 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "whisper-cli 转写失败（exit {:?}）: {}",
            output.status.code(),
            stderr.trim()
        ));
    }

    // 优先读 .txt 输出文件，失败再退回 stdout。
    let text = std::fs::read_to_string(&txt_path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| String::from_utf8_lossy(&output.stdout).trim().to_string());

    // 清理临时产物（best-effort，失败不影响结果）。
    let _ = std::fs::remove_file(&txt_path);

    Ok(text)
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

    Ok(())
}

// ============================================================================
// 资产下载（reqwest 流式 → 解压 → 落 voice_dir）
// ============================================================================

/// 流式下载一个 URL 到本地文件，边下边 emit 进度。
///
/// 要点：
/// - 写**临时文件** `<dest>.part`，下完原子 rename 到目标——中断不会留下半截被当成完整资产。
/// - 从 `Content-Length` 拿总大小（HF/GitHub 都给）；拿不到时 total=0，前端按「未知总量」处理。
/// - 进度事件 `voice-download-progress { phase, downloaded, total, percent }`，phase 标明在下哪块。
async fn download_to_file(
    app: &tauri::AppHandle,
    url: &str,
    dest: &PathBuf,
    phase: &str,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use std::io::Write;
    use tauri::Emitter;

    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建语音目录失败: {}", e))?;
    }

    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("下载请求失败（{}）: {}", phase, e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "下载 {} 失败：HTTP {}（{}）",
            phase,
            resp.status().as_u16(),
            url
        ));
    }

    let total = resp.content_length().unwrap_or(0);
    let part = dest.with_extension("part");
    let mut file =
        std::fs::File::create(&part).map_err(|e| format!("创建临时下载文件失败: {}", e))?;

    let mut downloaded: u64 = 0;
    let mut last_emit: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("下载流读取错误（{}）: {}", phase, e))?;
        file.write_all(&chunk)
            .map_err(|e| format!("写下载文件失败: {}", e))?;
        downloaded += chunk.len() as u64;

        // 节流：每累计 ~1MB 才 emit 一次，避免大模型（574MB）刷爆事件通道。
        if downloaded - last_emit >= 1_000_000 || (total > 0 && downloaded >= total) {
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
                }),
            );
        }
    }

    // 显式 flush + drop，确保字节全落盘再 rename。
    file.flush().map_err(|e| format!("刷新下载文件失败: {}", e))?;
    drop(file);

    std::fs::rename(&part, dest).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("下载文件改名失败: {}", e)
    })?;
    Ok(())
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
) -> Result<(), String> {
    let mut errors: Vec<String> = Vec::new();
    for (i, url) in urls.iter().enumerate() {
        match download_to_file(app, url, dest, phase).await {
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

/// 把默认热键字符串解析成 `Shortcut`。常量写错才会失败，统一转成 String 错误。
fn default_shortcut() -> Result<Shortcut, String> {
    DEFAULT_HOTKEY
        .parse::<Shortcut>()
        .map_err(|e| format!("解析热键 '{}' 失败: {}", DEFAULT_HOTKEY, e))
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

/// 按当前开关状态注册/注销热键：
/// `voiceInputEnabled=true` 且资产就绪 → 注册；否则注销（不在关闭态占用全局热键）。
/// 幂等：重复注册先注销旧的；前端开关翻转、启动校准都调它。
pub fn sync_hotkey(app: &tauri::AppHandle) -> Result<(), String> {
    let shortcut = default_shortcut()?;
    let gs = app.global_shortcut();
    let want = voice_input_enabled() && voice_assets_ready();

    if want {
        // 先确保干净（已注册则先撤），再注册，避免重复注册报错。
        if gs.is_registered(shortcut) {
            let _ = gs.unregister(shortcut);
        }
        gs.register(shortcut)
            .map_err(|e| format!("注册语音热键失败: {}", e))?;
    } else if gs.is_registered(shortcut) {
        gs.unregister(shortcut)
            .map_err(|e| format!("注销语音热键失败: {}", e))?;
    }
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

/// 查询语音资产就绪状态。供 PR2 的设置/下载流程判断是否需要下载。
#[tauri::command]
pub fn voice_assets_status() -> Result<Value, String> {
    Ok(json!({
        "ready": voice_assets_ready(),
        "voiceDir": voice_dir().to_string_lossy(),
        "hasBinary": whisper_cli_path().is_file(),
        "hasModel": model_path().is_file(),
    }))
}

/// 下载语音资产到 `~/.jarvis/voice/`：① whisper-cli 二进制 zip → 解压；② large-v3-turbo 模型。
///
/// 已存在的部分跳过（幂等）：二进制齐了就不重下，模型在了就不重下——支持中断后重试只补缺的。
/// 全程 emit `voice-download-progress`（phase=`"binary"`/`"model"`）。下完校验 `voice_assets_ready()`。
#[tauri::command]
pub async fn voice_download_assets(app: tauri::AppHandle) -> Result<Value, String> {
    // ① whisper-cli 二进制：缺就下 zip → 解压（解压内部会校验 whisper-cli 在不在）。
    if !whisper_cli_path().is_file() {
        let zip_path = voice_dir().join("whisper-bin-x64.zip");
        download_to_file_multi(&app, WHISPER_BIN_URLS, &zip_path, "binary").await?;
        extract_whisper_zip(&zip_path)?;
        // 解压完删掉 zip（best-effort，省空间）。
        let _ = std::fs::remove_file(&zip_path);
    }

    // ② 模型：缺就下（574MB，下载耗时最久，进度务必顺滑）。
    if !model_path().is_file() {
        let model = model_path();
        download_to_file(&app, MODEL_URL, &model, "model").await?;
    }

    if !voice_assets_ready() {
        return Err("下载完成但资产仍不就绪（缺 whisper-cli 或模型）".to_string());
    }
    Ok(json!({ "ready": true }))
}

/// 开始录音。资产没就绪 / 已在录 → Err。
#[tauri::command]
pub fn voice_start() -> Result<(), String> {
    if !voice_assets_ready() {
        return Err("语音资产未就绪（缺 whisper-cli 或模型）".to_string());
    }
    start_recording()
}

/// 停录 → 写 WAV → whisper-cli 转写 → 注入聚焦框 → 返回转写文本。
/// 命令层与热键处理共用这段，错误分清「没在录」「转写失败」「注入失败」三类。
fn stop_transcribe_inject() -> Result<String, String> {
    let samples = stop_recording()?;
    if samples.is_empty() {
        return Err("没有采集到音频（录音太短或麦克风无输入）".to_string());
    }

    let wav = write_temp_wav(&samples)?;
    let text_result = transcribe_wav(&wav);
    // 转写产物 WAV 用完即删（best-effort）。
    let _ = std::fs::remove_file(&wav);
    let text = text_result?;

    if text.is_empty() {
        return Err("转写结果为空（未识别到语音内容）".to_string());
    }

    inject_text(&text)?;
    Ok(text)
}

/// 停录 → 写 WAV → whisper-cli 转写 → 注入聚焦框 → 返回 `{text}`。
#[tauri::command]
pub fn voice_stop_and_transcribe() -> Result<Value, String> {
    let text = stop_transcribe_inject()?;
    Ok(json!({ "text": text }))
}

/// 按当前开关状态注册/注销全局热键。前端在开关翻转、下载完成、首启校准时调用。
/// 返回当前是否已注册热键 + 用的键位，方便前端给提示。
#[tauri::command]
pub fn voice_hotkey_sync(app: tauri::AppHandle) -> Result<Value, String> {
    sync_hotkey(&app)?;
    Ok(json!({
        "registered": voice_input_enabled() && voice_assets_ready(),
        "hotkey": DEFAULT_HOTKEY,
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
}
