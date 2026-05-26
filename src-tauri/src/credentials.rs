// OS 密钥链封装 + 禅道凭证管理 + 连接测试。
//
// 密码绝不写入磁盘文件。用 OS 密钥链（Windows DPAPI / macOS Keychain /
// Linux SecretService 或 keyutils），只有当前用户能解密。
//
// Service 名固定 "Jarvis"，account 用用户的禅道账号名作 key（同一台机器
// 可以同时存多个禅道账号的密码，理论上支持账号切换，虽然现在只用一个）。

use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

const SERVICE_NAME: &str = "Jarvis";

fn entry(account: &str) -> Result<Entry, String> {
    Entry::new(SERVICE_NAME, account).map_err(|e| format!("无法访问密钥链: {}", e))
}

#[tauri::command]
pub fn credentials_set(account: String, password: String) -> Result<(), String> {
    if account.trim().is_empty() {
        return Err("禅道账号不能为空".to_string());
    }
    let e = entry(&account)?;
    e.set_password(&password).map_err(|err| format!("保存密码到密钥链失败: {}", err))
}

#[tauri::command]
pub fn credentials_get(account: String) -> Result<Option<String>, String> {
    let e = entry(&account)?;
    match e.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(format!("读取密钥链失败: {}", err)),
    }
}

#[tauri::command]
pub fn credentials_delete(account: String) -> Result<(), String> {
    let e = entry(&account)?;
    match e.delete_credential() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(format!("删除密钥链条目失败: {}", err)),
    }
}

// ===== daemon 重启 =====
//
// 用户在 wizard 完成 / 在设置面板保存新密码后调。
//
// daemon 启动时通过 spawn env 拿到 ZENTAO_PASSWORD（从 keychain 读出来塞进去），
// 启动后 process.env 不会变。所以改了密码 / 改了 baseUrl 之后必须重启 daemon，
// 否则它仍用旧凭证调禅道 → 认证失败。
#[tauri::command]
pub async fn daemon_restart() -> Result<(), String> {
    // 1. 让现有 daemon 优雅退出
    crate::daemon_client::try_shutdown().await;

    // 2. 主动删 daemon.json —— 否则 ensure_running 看到 daemon.json + pid 还活
    //    +/health 还能响应（旧 daemon 退出前能撑几秒）会判定旧 daemon 健康直接
    //    复用，新密码永远生效不了。删掉这个状态文件强制下一次走 spawn 路径。
    let info_path = crate::daemon_client::daemon_info_path_pub();
    let _ = std::fs::remove_file(&info_path);

    // 3. 给旧 daemon 一点时间释放端口（OS 分配的随机端口，新 daemon 不会抢，
    //    但保险起见等一下让旧 daemon 进程真正退出）
    tokio::time::sleep(Duration::from_millis(500)).await;

    // 4. ensure_running 现在看不到 daemon.json，必定走 spawn，新进程通过 env
    //    拿到 keychain 里最新的密码
    crate::daemon_client::ensure_running()
        .await
        .map(|_| ())
        .map_err(|e| format!("daemon 重启失败: {}", e))
}

// ===== 禅道连接测试 =====
//
// 给引导/设置窗口的"测试连接"按钮调。直接打禅道的 token 接口，验证 baseUrl
// + account + password 是不是真能登。不写 settings，不存密码——纯只读检测。

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZentaoTestRequest {
    pub base_url: String,
    pub account: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZentaoTestResult {
    pub ok: bool,
    pub message: String,
}

/// 把用户输入的禅道 URL 清洗成可拼 /api.php/v1/... 的根地址。
///
/// 行为同 desktop/src/composables/zentaoUrl.ts 的 normalizeZentaoBaseUrl —— 前端
/// 应该已经清洗过，这里做服务端兜底（用户可能手动改了 settings.json）。
///
/// 规则：
///   - 缺 scheme 补 http://
///   - 丢 query / fragment
///   - path 按 '/' 切段，遇到第一个 *.html / *.htm / *.php / *.json / *.jsp
///     / *.asp / *.aspx 即截断（那是入口文件名而非路径前缀）
///   - 去尾斜杠
fn normalize_base_url(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let with_scheme: String = if trimmed.to_lowercase().starts_with("http://")
        || trimmed.to_lowercase().starts_with("https://")
    {
        trimmed.to_string()
    } else {
        format!("http://{}", trimmed)
    };

    let url = match reqwest::Url::parse(&with_scheme) {
        Ok(u) => u,
        Err(_) => return trimmed.to_string(),
    };

    let scheme = url.scheme();
    let host = url.host_str().unwrap_or("");
    let host_port = match url.port() {
        Some(p) => format!("{}:{}", host, p),
        None => host.to_string(),
    };

    let is_entry = |seg: &str| {
        let l = seg.to_lowercase();
        l.ends_with(".html") || l.ends_with(".htm")
            || l.ends_with(".php") || l.ends_with(".json")
            || l.ends_with(".jsp") || l.ends_with(".asp")
            || l.ends_with(".aspx")
    };

    let mut kept: Vec<&str> = Vec::new();
    for seg in url.path().split('/').filter(|s| !s.is_empty()) {
        if is_entry(seg) {
            break;
        }
        kept.push(seg);
    }
    let path = if kept.is_empty() {
        String::new()
    } else {
        format!("/{}", kept.join("/"))
    };

    format!("{}://{}{}", scheme, host_port, path)
}

/// 诊断逻辑已搬到 bundled/zentao-test.mjs（与 fetch 在同一进程里就近做），
/// Rust 这边不再需要本地拷贝。Tauri 后端只把 helper 的 JSON 结果原样回传。

#[tauri::command]
pub async fn zentao_test_connection(req: ZentaoTestRequest) -> Result<ZentaoTestResult, String> {
    let base = normalize_base_url(&req.base_url);
    if base.is_empty() {
        return Ok(ZentaoTestResult { ok: false, message: "禅道地址不能为空".to_string() });
    }
    if req.account.trim().is_empty() {
        return Ok(ZentaoTestResult { ok: false, message: "账号不能为空".to_string() });
    }

    // 直接用原生 Rust zentao client 探活。原来这里 spawn 过 bundled/node + zentao-test.mjs
    // 是为了绕某些有 WAF 的禅道前置（reqwest 指纹被拦），用户当前环境
    // 是内网 IP，按 Occam's razor 直接走 reqwest；遇到 WAF 再加 hyper 兜底。
    let result = crate::zentao::test_connection(&base, &req.account, &req.password).await;
    Ok(ZentaoTestResult { ok: result.ok, message: result.message })
}

/// 调 bundled node.exe + zentao-test.mjs 完成实际连接测试，从 stdout 取 JSON。
#[allow(dead_code)]
async fn spawn_node_zentao_test(
    base: &str,
    account: &str,
    password: &str,
) -> Result<ZentaoTestResult, String> {
    let (node_bin, helper) = match resolve_zentao_test_helper() {
        Some(t) => t,
        None => {
            return Ok(ZentaoTestResult {
                ok: false,
                message: "找不到禅道测试 helper（bundled/zentao-test.mjs）。请重装应用。".to_string(),
            });
        }
    };

    let url_for_msg = format!("{}/api.php/v1/tokens", base);
    let base = base.to_string();
    let account = account.to_string();
    let password = password.to_string();
    let node_bin_clone = node_bin.clone();
    let helper_clone = helper.clone();

    // 阻塞性的 Command::output 放到 blocking 线程，避免堵 tokio runtime。
    // 8s 超时由 spawn 包一层 tokio::time::timeout 控制（Command 自身没超时）。
    let output_fut = tokio::task::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(&node_bin_clone);
        cmd.arg(&helper_clone).arg(&base).arg(&account).arg(&password);
        // 关键：擦掉所有代理 env。Tauri/WebView2 进程在 Windows 上会注入
        // HTTP(S)_PROXY 反映系统代理设置，子进程继承后 undici fetch 会读取
        // 并走代理 —— 而代理把禅道域名拦成 403 "禁止访问"HTML。
        // 直接命令行跑 bundled node.exe 没这些 env，所以能 201 成功。
        for v in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY",
                  "http_proxy", "https_proxy", "all_proxy",
                  "NO_PROXY", "no_proxy"] {
            cmd.env_remove(v);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        cmd.output()
    });

    let output = match tokio::time::timeout(Duration::from_secs(15), output_fut).await {
        Ok(Ok(Ok(o))) => o,
        Ok(Ok(Err(e))) => {
            return Ok(ZentaoTestResult {
                ok: false,
                message: format!("启动 node helper 失败：{}\n实际请求：{}", e, url_for_msg),
            });
        }
        Ok(Err(e)) => {
            return Ok(ZentaoTestResult {
                ok: false,
                message: format!("node helper 任务异常：{}", e),
            });
        }
        Err(_) => {
            return Ok(ZentaoTestResult {
                ok: false,
                message: format!("禅道测试超时（>15s）。\n实际请求：{}", url_for_msg),
            });
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    // helper 用一行 `__JARVIS_RESULT__{json}` 包结果，避免被无关日志干扰
    let parsed = stdout
        .lines()
        .filter_map(|l| l.strip_prefix("__JARVIS_RESULT__"))
        .last()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok());

    let v = match parsed {
        Some(v) => v,
        None => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(ZentaoTestResult {
                ok: false,
                message: format!(
                    "node helper 未返回有效结果。stdout:\n{}\nstderr:\n{}",
                    stdout.trim().chars().take(300).collect::<String>(),
                    stderr.trim().chars().take(300).collect::<String>(),
                ),
            });
        }
    };

    Ok(ZentaoTestResult {
        ok: v.get("ok").and_then(|b| b.as_bool()).unwrap_or(false),
        message: v.get("message").and_then(|m| m.as_str()).unwrap_or("").to_string(),
    })
}

/// 找 bundled/node.exe + bundled/zentao-test.mjs。逻辑同 daemon_client.rs 的
/// resolve_daemon_launch —— 生产从 exe 同级 resources/bundled 找，dev 从项目
/// 根 src-tauri/bundled 找；找不到 node.exe 时回退到系统 node（适配 dev）。
#[allow(dead_code)]
fn resolve_zentao_test_helper() -> Option<(PathBuf, PathBuf)> {
    let exe = std::env::current_exe().ok();
    let exe_dir = exe.as_ref().and_then(|p| p.parent()).map(PathBuf::from);
    let node_bin_name = if cfg!(windows) { "node.exe" } else { "node" };

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(d) = &exe_dir {
        candidates.push(d.join("resources").join("bundled"));
        if let Some(parent) = d.parent() {
            candidates.push(parent.join("Resources").join("bundled"));
        }
        candidates.push(d.join("bundled"));
    }

    // 项目根（dev）
    let cwd = std::env::current_dir().unwrap_or_default();
    let root = if cwd.join("package.json").exists() {
        cwd.clone()
    } else if cwd.parent().map(|p| p.join("package.json").exists()).unwrap_or(false) {
        cwd.parent().unwrap().to_path_buf()
    } else {
        cwd
    };
    candidates.push(root.join("src-tauri").join("bundled"));

    for dir in &candidates {
        let helper = dir.join("zentao-test.mjs");
        if !helper.exists() {
            continue;
        }
        let bundled_node = dir.join(node_bin_name);
        let node = if bundled_node.exists() {
            bundled_node
        } else {
            // dev 模式 bundled/node.exe 可能没下载 —— 用系统 node
            PathBuf::from("node")
        };
        return Some((node, helper));
    }

    None
}
