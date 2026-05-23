// Jarvis daemon HTTP client
//
// Responsibilities:
// - Read ~/.jarvis/daemon.json (pid/port/token) and probe /health to confirm liveness
// - If daemon is not running, spawn `node dist/daemon/server.js` and wait for readiness
// - Provide call_get / call_post that authenticate with Bearer token and return JSON
//
// Concurrency: ensure_running is serialized by a tokio Mutex so multiple Tauri
// commands can't race to spawn duplicate daemons.

use once_cell::sync::Lazy;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonInfo {
    pub pid: u32,
    pub port: u16,
    pub token: String,
    #[serde(rename = "startedAt")]
    pub started_at: String,
    pub version: String,
}

static HTTP: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(2))
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
        .expect("failed to build reqwest client")
});

// Serializes ensure_running so concurrent callers don't double-spawn.
static SPAWN_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn jarvis_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home).join(".jarvis")
}

fn daemon_info_path() -> PathBuf {
    jarvis_dir().join("daemon.json")
}

fn project_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    if cwd.join("package.json").exists() {
        cwd
    } else if cwd
        .parent()
        .map(|p| p.join("package.json").exists())
        .unwrap_or(false)
    {
        cwd.parent().unwrap().to_path_buf()
    } else {
        cwd
    }
}

fn read_info() -> Option<DaemonInfo> {
    let path = daemon_info_path();
    if !path.exists() {
        return None;
    }
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

#[cfg(windows)]
fn is_process_alive(pid: u32) -> bool {
    // tasklist /FI "PID eq <pid>" /NH outputs "INFO: ..." if not found
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid), "/NH", "/FO", "CSV"])
        .creation_flags(0x08000000)
        .output();
    match out {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout);
            // CSV line starts with quoted process name when found; empty/INFO when not
            !s.trim().is_empty() && !s.contains("INFO:")
        }
        Err(_) => false,
    }
}

#[cfg(not(windows))]
fn is_process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM) }
}

async fn probe_health(info: &DaemonInfo) -> bool {
    let url = format!("http://127.0.0.1:{}/health", info.port);
    let resp = HTTP
        .get(&url)
        .bearer_auth(&info.token)
        .timeout(Duration::from_secs(2))
        .send()
        .await;
    matches!(resp, Ok(r) if r.status().is_success())
}

/// 从 settings.json 读取禅道账号（用作 keychain 的 key）
fn read_zentao_account() -> Option<String> {
    let cfg_path = jarvis_dir().join("config.json");
    let raw = std::fs::read_to_string(cfg_path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("zentao")
        .and_then(|z| z.get("account"))
        .and_then(|a| a.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
}

/// 从 keychain 取密码（账号从 settings 拿）。失败/缺失返回 None，让 daemon 自然降级。
fn read_password_from_keychain() -> Option<String> {
    let account = read_zentao_account()?;
    let entry = keyring::Entry::new("Jarvis", &account).ok()?;
    entry.get_password().ok()
}

#[cfg(windows)]
fn spawn_daemon(root: &PathBuf) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    // DETACHED_PROCESS (0x00000008) | CREATE_NO_WINDOW (0x08000000)
    // Detached so daemon outlives Tauri if we crash.
    let mut cmd = std::process::Command::new("node");
    cmd.arg("dist/daemon/server.js")
        .current_dir(root)
        .creation_flags(0x00000008 | 0x08000000)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // 把密码作为 env 一次性传给 daemon，磁盘上没有明文。daemon 启动后通过
    // process.env.ZENTAO_PASSWORD 拿到，存在内存里直到进程结束。
    if let Some(pwd) = read_password_from_keychain() {
        cmd.env("ZENTAO_PASSWORD", pwd);
    }
    cmd.spawn()?;
    Ok(())
}

#[cfg(not(windows))]
fn spawn_daemon(root: &PathBuf) -> std::io::Result<()> {
    let mut cmd = std::process::Command::new("node");
    cmd.arg("dist/daemon/server.js")
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(pwd) = read_password_from_keychain() {
        cmd.env("ZENTAO_PASSWORD", pwd);
    }
    cmd.spawn()?;
    Ok(())
}

/// Make sure a healthy daemon is reachable and return its connection info.
///
/// Fast path: existing daemon.json + alive pid + /health OK -> return immediately.
/// Slow path: spawn node daemon and poll /health up to ~15s.
pub async fn ensure_running() -> Result<DaemonInfo, String> {
    if let Some(info) = read_info() {
        if is_process_alive(info.pid) && probe_health(&info).await {
            return Ok(info);
        }
    }

    let _guard = SPAWN_LOCK.lock().await;

    // Re-check after acquiring lock (another task may have spawned)
    if let Some(info) = read_info() {
        if is_process_alive(info.pid) && probe_health(&info).await {
            return Ok(info);
        }
    }

    let root = project_root();
    spawn_daemon(&root).map_err(|e| format!("failed to spawn daemon: {}", e))?;

    // Poll for readiness up to 15 seconds.
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let mut last_err = String::from("daemon did not become ready");
    while std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if let Some(info) = read_info() {
            if probe_health(&info).await {
                return Ok(info);
            } else {
                last_err = format!("daemon info present but /health failed on port {}", info.port);
            }
        }
    }
    Err(last_err)
}

async fn request(method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value, String> {
    let info = ensure_running().await?;
    let url = format!("http://127.0.0.1:{}{}", info.port, path);
    let mut req = HTTP.request(method, &url).bearer_auth(&info.token);
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("daemon request failed ({}): {}", path, e))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("daemon read body failed ({}): {}", path, e))?;
    if !status.is_success() {
        // Try to surface the daemon's error payload verbatim.
        return Err(format!("daemon {} returned HTTP {}: {}", path, status.as_u16(), text));
    }
    if text.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text)
        .map_err(|e| format!("daemon {} returned invalid JSON: {} (raw: {})", path, e, &text[..text.len().min(200)]))
}

pub async fn get(path: &str) -> Result<Value, String> {
    request(reqwest::Method::GET, path, None).await
}

pub async fn post(path: &str, body: Value) -> Result<Value, String> {
    request(reqwest::Method::POST, path, Some(body)).await
}

/// Best-effort: tell daemon to exit. Used during app shutdown.
pub async fn try_shutdown() {
    if let Some(info) = read_info() {
        let url = format!("http://127.0.0.1:{}/shutdown", info.port);
        let _ = HTTP
            .post(&url)
            .bearer_auth(&info.token)
            .timeout(Duration::from_secs(2))
            .send()
            .await;
    }
}
