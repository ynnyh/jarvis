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

#[tauri::command]
pub async fn zentao_test_connection(req: ZentaoTestRequest) -> Result<ZentaoTestResult, String> {
    let base = req.base_url.trim_end_matches('/');
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
                message: format!("无法连接禅道：{}。请检查地址是否正确、是否在公司网络。", err),
            });
        }
    };

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Ok(ZentaoTestResult {
            ok: false,
            message: format!("禅道返回 HTTP {}：{}", status.as_u16(), body.chars().take(120).collect::<String>()),
        });
    }

    // 成功的响应里应该有 token 字段
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
    let token = parsed.get("token").and_then(|t| t.as_str()).unwrap_or("");
    if token.is_empty() {
        return Ok(ZentaoTestResult {
            ok: false,
            message: format!("账号或密码错误：{}", body.chars().take(160).collect::<String>()),
        });
    }

    Ok(ZentaoTestResult {
        ok: true,
        message: format!("连接成功，已获取 Token（{}...）", &token[..token.len().min(10)]),
    })
}
