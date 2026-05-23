// OS 密钥链封装 + 禅道凭证管理 + 连接测试。
//
// 密码绝不写入磁盘文件。用 OS 密钥链（Windows DPAPI / macOS Keychain /
// Linux SecretService 或 keyutils），只有当前用户能解密。
//
// Service 名固定 "Jarvis"，account 用用户的禅道账号名作 key（同一台机器
// 可以同时存多个禅道账号的密码，理论上支持账号切换，虽然现在只用一个）。

use keyring::Entry;
use serde::{Deserialize, Serialize};
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

/// 根据响应状态码 + body 形态生成对用户友好的诊断信息。
///
/// 已知 4 种典型失败现场（按出现频率）：
///   - HTTP 500 + HTML：禅道 API 没在后台启用，或者老版本压根没 v1 REST
///   - HTTP 404：URL 路径不对（多半是 baseUrl 漏了 /zentao 子路径）
///   - HTTP 200 + HTML：URL 命中了登录页或别的 HTML（baseUrl 还是有问题）
///   - HTTP 200 + JSON 无 token：账号密码错误，或者 API 返回了别的 shape
fn diagnose_failure(url: &str, status: u16, body: &str) -> String {
    let trimmed = body.trim();
    let snippet: String = trimmed.chars().take(200).collect();
    let looks_html = trimmed.starts_with('<')
        || trimmed.to_lowercase().contains("<!doctype html")
        || trimmed.to_lowercase().contains("<html");

    if status == 500 && looks_html {
        return format!(
            "禅道服务器内部错误（HTTP 500）。最常见原因：\n\
             1) 后台 → 二次开发 → API 未启用 → 联系禅道管理员开启\n\
             2) 禅道版本低于 12.3.3，没有 v1 REST 接口\n\
             实际请求：{}",
            url
        );
    }
    if status == 404 {
        return format!(
            "找不到接口（HTTP 404）。多半是 baseUrl 漏了子路径（常见为 /zentao）。\n\
             实际请求：{}",
            url
        );
    }
    if (status == 200 || status == 201) && looks_html {
        return format!(
            "禅道返回了 HTML 页面而不是 JSON，说明 baseUrl 命中了登录页或别的网页。\n\
             检查 baseUrl 是否多了页面路径（如 /user-login-xxx.html）。\n\
             实际请求：{}",
            url
        );
    }
    if status == 401 || status == 403 {
        return format!(
            "账号或密码错误（HTTP {}）。\n响应：{}",
            status, snippet
        );
    }
    if status >= 200 && status < 300 {
        // 2xx 但没 token —— 大概率账号密码错
        return format!(
            "账号或密码错误，或禅道未返回 token。\n响应：{}",
            snippet
        );
    }
    format!("禅道返回 HTTP {}：{}", status, snippet)
}

#[tauri::command]
pub async fn zentao_test_connection(req: ZentaoTestRequest) -> Result<ZentaoTestResult, String> {
    let base = normalize_base_url(&req.base_url);
    if base.is_empty() {
        return Ok(ZentaoTestResult { ok: false, message: "禅道地址不能为空".to_string() });
    }
    if req.account.trim().is_empty() {
        return Ok(ZentaoTestResult { ok: false, message: "账号不能为空".to_string() });
    }

    let url = format!("{}/api.php/v1/tokens", base);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "account": req.account,
            "password": req.password,
        }))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(err) => {
            return Ok(ZentaoTestResult {
                ok: false,
                message: format!(
                    "无法连接禅道：{}\n请检查地址是否正确、是否在公司网络。\n实际请求：{}",
                    err, url
                ),
            });
        }
    };

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Ok(ZentaoTestResult {
            ok: false,
            message: diagnose_failure(&url, status.as_u16(), &body),
        });
    }

    // 成功响应里应该有 token 字段
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
    let token = parsed.get("token").and_then(|t| t.as_str()).unwrap_or("");
    if token.is_empty() {
        return Ok(ZentaoTestResult {
            ok: false,
            message: diagnose_failure(&url, status.as_u16(), &body),
        });
    }

    Ok(ZentaoTestResult {
        ok: true,
        message: format!(
            "连接成功，已获取 Token（{}...）\nbaseUrl 已规范化为：{}",
            &token[..token.len().min(10)],
            base
        ),
    })
}
