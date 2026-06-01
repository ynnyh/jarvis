use keyring::Entry;

pub(super) const SERVICE_NAME: &str = "Jarvis-FineReport";
pub(super) const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

pub(super) fn keyring_entry(account: &str) -> Result<Entry, String> {
    Entry::new(SERVICE_NAME, account).map_err(|e| format!("无法访问密钥链: {}", e))
}

#[tauri::command]
pub fn finereport_credentials_set(account: String, password: String) -> Result<(), String> {
    if account.trim().is_empty() {
        return Err("帆软账号不能为空".to_string());
    }
    keyring_entry(&account)?
        .set_password(&password)
        .map_err(|e| format!("保存密码到密钥链失败: {}", e))
}

#[tauri::command]
pub fn finereport_credentials_get(account: String) -> Result<Option<String>, String> {
    match keyring_entry(&account)?.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("读取密钥链失败: {}", e)),
    }
}

#[tauri::command]
pub fn finereport_credentials_delete(account: String) -> Result<(), String> {
    match keyring_entry(&account)?.delete_credential() {
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("删除密钥链条目失败: {}", e)),
    }
}

#[derive(Debug, Clone)]
pub struct FineReportCredentials {
    pub base_url: String,
    pub account: String,
    pub password: String,
    /// 中文显示名，用于 REAL_NAME 过滤。空则不查询（隐私保护）。
    pub real_name: String,
}

/// 从 config.json + keychain 读帆软凭证。
pub fn get_fine_report_credentials() -> FineReportCredentials {
    let cfg = crate::settings::load_raw_config();
    let fr = cfg.as_ref().and_then(|v| v.get("fineReport"));

    let s = |key: &str| -> Option<String> {
        fr.and_then(|v| v.get(key))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };

    let base_url = s("baseUrl").unwrap_or_default();
    let account = s("account").unwrap_or_default();
    let real_name = s("realName").unwrap_or_default();
    let password = if account.is_empty() {
        String::new()
    } else {
        Entry::new(SERVICE_NAME, &account)
            .ok()
            .and_then(|e| e.get_password().ok())
            .unwrap_or_default()
    };

    FineReportCredentials {
        base_url,
        account,
        password,
        real_name,
    }
}
