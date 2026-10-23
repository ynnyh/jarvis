// OS 密钥链封装 + 禅道凭证管理 + 连接测试。
//
// 密码绝不写入磁盘文件。用 OS 密钥链（Windows DPAPI / macOS Keychain /
// Linux SecretService 或 keyutils），只有当前用户能解密。
//
// Service 名固定 "Jarvis"，account 用用户的禅道账号名作 key（同一台机器
// 可以同时存多个禅道账号的密码，理论上支持账号切换，虽然现在只用一个）。

use keyring::Entry;
use serde::{Deserialize, Serialize};

const SERVICE_NAME: &str = "Jarvis";

fn entry(account: &str) -> Result<Entry, String> {
    // 统一在这里 trim：settings 运行时读凭证用的也是 trim 后的账号，
    // 写入时多空格会导致读写键不一致（存进去却读不到）。
    Entry::new(SERVICE_NAME, account.trim()).map_err(|e| format!("无法访问密钥链: {}", e))
}

#[tauri::command]
pub fn credentials_set(account: String, password: String) -> Result<(), String> {
    if account.trim().is_empty() {
        return Err("禅道账号不能为空".to_string());
    }
    if password.is_empty() {
        // 空密码覆盖会让密钥链里已存的密码"凭空消失"，任务读取全部失败。
        return Err("密码不能为空（如需更换密码，请输入新密码）".to_string());
    }
    let e = entry(&account)?;
    e.set_password(&password)
        .map_err(|err| format!("保存密码到密钥链失败: {}", err))
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
    /// 失败时的 HTTP 状态码（成功/未发请求时为 None）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
}

/// 把用户输入的禅道 URL 清洗成可拼 /api.php/v1/... 的根地址。
///
/// 行为同 desktop/src/composables/zentaoUrl.ts 的 normalizeZentaoBaseUrl —— 前端
/// 应该已经清洗过，这里做服务端兜底（用户可能手动改了 settings.json）。
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
        l.ends_with(".html")
            || l.ends_with(".htm")
            || l.ends_with(".php")
            || l.ends_with(".json")
            || l.ends_with(".jsp")
            || l.ends_with(".asp")
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

#[tauri::command]
pub async fn zentao_test_connection(req: ZentaoTestRequest) -> Result<ZentaoTestResult, String> {
    let base = normalize_base_url(&req.base_url);
    if base.is_empty() {
        return Ok(ZentaoTestResult {
            ok: false,
            message: "禅道地址不能为空".to_string(),
            status_code: None,
        });
    }
    if req.account.trim().is_empty() {
        return Ok(ZentaoTestResult {
            ok: false,
            message: "账号不能为空".to_string(),
            status_code: None,
        });
    }

    // 密码空 → 回退到 keychain 已存值。Settings 页的密码框不回填 keychain（防泄露），
    // 用户多半希望直接测"已保存的配置能不能登"，而不是再敲一遍密码。
    let (password, from_keychain) = if req.password.is_empty() {
        match entry(&req.account)?.get_password() {
            Ok(p) => (p, true),
            Err(keyring::Error::NoEntry) => {
                // 密钥链里没存过密码，硬测也只会换来 400，直接把话说明白。
                return Ok(ZentaoTestResult {
                    ok: false,
                    message: "密码框为空，密钥链中也没有保存过密码。请输入密码后点「测试连接」（会自动保存）或「保存密码到密钥链」。".to_string(),
                    status_code: None,
                });
            }
            Err(err) => return Err(format!("读取密钥链失败: {}", err)),
        }
    } else {
        (req.password.clone(), false)
    };

    let result = crate::zentao::test_connection(&base, &req.account, &password).await;
    let mut message = result.message;
    // 用的是密钥链里的密码且被服务端当凭证拒绝 → 大概率服务端改过密码，指路换密码。
    if from_keychain && !result.ok && matches!(result.status_code, Some(400) | Some(401)) {
        message.push_str(
            "\n以上用的是密钥链中保存的密码，服务端已拒绝——如果近期改过禅道密码，请在密码框输入新密码后点「测试连接」（会自动保存到密钥链）。",
        );
    }
    Ok(ZentaoTestResult {
        ok: result.ok,
        message,
        status_code: result.status_code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_base_url_strips_php_entry() {
        assert_eq!(
            normalize_base_url("http://example.com/zentao/index.php"),
            "http://example.com/zentao"
        );
    }

    #[test]
    fn normalize_base_url_preserves_path_segments() {
        assert_eq!(
            normalize_base_url("http://example.com/zentao"),
            "http://example.com/zentao"
        );
    }

    #[test]
    fn normalize_base_url_strips_trailing_slash() {
        // trailing slash produces empty segment that gets filtered
        assert_eq!(
            normalize_base_url("http://example.com/zentao/"),
            "http://example.com/zentao"
        );
    }

    #[test]
    fn normalize_base_url_adds_scheme_if_missing() {
        assert_eq!(
            normalize_base_url("example.com/zentao"),
            "http://example.com/zentao"
        );
    }

    #[test]
    fn normalize_base_url_preserves_https() {
        assert_eq!(
            normalize_base_url("https://example.com/zentao"),
            "https://example.com/zentao"
        );
    }

    #[test]
    fn normalize_base_url_preserves_custom_port() {
        assert_eq!(
            normalize_base_url("http://example.com:9538/zentao"),
            "http://example.com:9538/zentao"
        );
    }

    #[test]
    fn normalize_base_url_strips_html_entry() {
        assert_eq!(
            normalize_base_url("http://example.com/zentao/index.html"),
            "http://example.com/zentao"
        );
    }

    #[test]
    fn normalize_base_url_strips_json_entry() {
        assert_eq!(
            normalize_base_url("http://example.com/api/data.json"),
            "http://example.com/api"
        );
    }

    #[test]
    fn normalize_base_url_empty_input() {
        assert_eq!(normalize_base_url(""), "");
        assert_eq!(normalize_base_url("   "), "");
    }

    #[test]
    fn normalize_base_url_bare_host() {
        assert_eq!(
            normalize_base_url("http://example.com"),
            "http://example.com"
        );
    }
}
