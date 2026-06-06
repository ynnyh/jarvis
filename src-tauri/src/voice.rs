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

/// 默认模型文件名（base 量化版，约 57MB；下载交由 PR2）。
const DEFAULT_MODEL_FILE: &str = "ggml-base-q5_1.bin";

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

/// 默认模型路径 `~/.jarvis/voice/ggml-base-q5_1.bin`。
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

/// 开始录音。资产没就绪 / 已在录 → Err。
#[tauri::command]
pub fn voice_start() -> Result<(), String> {
    if !voice_assets_ready() {
        return Err("语音资产未就绪（缺 whisper-cli 或模型）".to_string());
    }
    start_recording()
}

/// 停录 → 写 WAV → whisper-cli 转写 → 注入聚焦框 → 返回 `{text}`。
/// 错误分清「没在录」「转写失败」「注入失败」三类，便于前端给出对应提示。
#[tauri::command]
pub fn voice_stop_and_transcribe() -> Result<Value, String> {
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
    Ok(json!({ "text": text }))
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
