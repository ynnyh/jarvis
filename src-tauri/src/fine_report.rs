// 帆软 BI（FineReport / 决策平台）接入。
//
// 这是公司用来看禅道工时的 BI 工具，JDBC 直连禅道 MySQL，开源版禅道本身的
// HTTP API 没有 effort listing，只能借道帆软读。
//
// 鉴权链路：
//   1. POST /webroot/decision/login 用账密换 JWT（14 天有效），返回带 fine_auth_token
//   2. JWT 缓存在 ~/.jarvis/finereport.json，剩 <1 天自动用账密续期
//   3. 调报表前先打开 viewlet 页面，从 HTML 里抠 cid（绑定一份"参数化的报表实例"）
//   4. POST op=fr_dialog&cmd=parameters_d 提交日期 + REAL_NAME 过滤
//   5. GET op=fr_write&cmd=read_w_content&cid=...&reportIndex=N 拿 JSON.html
//
// 密码绝不写入磁盘，进 OS keychain，service="Jarvis-FineReport"。

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use keyring::Entry;
use md5::{Digest, Md5};
use reqwest::cookie::Jar;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::settings::jarvis_dir;

const SERVICE_NAME: &str = "Jarvis-FineReport";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

// ============================================================================
// Credentials (keychain)
// ============================================================================

fn keyring_entry(account: &str) -> Result<Entry, String> {
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

// ============================================================================
// 配置读取
// ============================================================================

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

    FineReportCredentials { base_url, account, password, real_name }
}

// ============================================================================
// JWT 缓存
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CachedAuth {
    pub account: String,
    pub base_url: String,
    /// fine_auth_token，14 天有效
    pub jwt: String,
    /// JWT exp（unix 秒），缓存时算出来，省得每次解码
    pub expires_at: i64,
    /// 通过 JWT 换出来的 sessionID（UUID 形式），加在 X-Header 里
    pub session_id: Option<String>,
}

fn cache_path() -> std::path::PathBuf {
    jarvis_dir().join("finereport.json")
}

fn load_cached_auth() -> Option<CachedAuth> {
    let raw = std::fs::read_to_string(cache_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

fn save_cached_auth(auth: &CachedAuth) -> Result<(), String> {
    let dir = jarvis_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建配置目录失败: {}", e))?;
    let json = serde_json::to_string_pretty(auth).map_err(|e| e.to_string())?;
    std::fs::write(cache_path(), json).map_err(|e| format!("写入帆软认证缓存失败: {}", e))
}

#[allow(dead_code)]
fn delete_cached_auth() {
    let _ = std::fs::remove_file(cache_path());
}

/// 解码 JWT payload 拿 exp（unix 秒）。失败返回 None 让上层兜底（14 天 - 1 小时）。
fn decode_jwt_exp(jwt: &str) -> Option<i64> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    // JWT 用 base64-url (no padding)
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()?;
    let v: Value = serde_json::from_slice(&payload).ok()?;
    v.get("exp").and_then(|e| e.as_i64())
}

fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ============================================================================
// 客户端
// ============================================================================

pub struct FineReportClient {
    pub base_url: String,
    pub account: String,
    pub password: String,
    pub client: reqwest::Client,
    pub jar: Arc<Jar>,
}

impl FineReportClient {
    /// 从 config + keychain 构建。
    pub fn from_settings() -> Result<Self, String> {
        let cred = get_fine_report_credentials();
        Self::new(cred.base_url, cred.account, cred.password)
    }

    pub fn new(base_url: String, account: String, password: String) -> Result<Self, String> {
        if base_url.is_empty() {
            return Err("帆软 baseUrl 未配置".into());
        }
        let mut base = base_url.trim().trim_end_matches('/').to_string();
        if !base.to_lowercase().starts_with("http") {
            base = format!("http://{}", base);
        }
        let jar = Arc::new(Jar::default());
        // 10s timeout：原 30s 太慢，定位卡点不够灵敏；如果服务端真要长拉
        // 再后续按步调高。
        let client = reqwest::Client::builder()
            .cookie_provider(jar.clone())
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| format!("帆软 HTTP client 构造失败: {}", e))?;
        Ok(Self { base_url: base, account, password, client, jar })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// 用账密换 JWT。
    ///
    /// 帆软决策平台标准端点：POST /webroot/decision/login。
    /// 实测响应形如：
    ///   { "status": "success", "data": { "accessToken": "eyJ..." }, "errorCode": null }
    /// 但不同 FR 版本字段路径可能不同（accessToken / token / fine_auth_token），
    /// 这里做兜底解析。
    pub async fn login(&self) -> Result<CachedAuth, String> {
        if self.account.is_empty() || self.password.is_empty() {
            return Err("帆软账号或密码为空".into());
        }
        let url = self.url("/webroot/decision/login");
        let body = serde_json::json!({
            "username": self.account,
            "password": self.password,
            "validity": -1,
            "sliderToken": "",
            "origin": ""
        });
        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("User-Agent", UA)
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("帆软登录请求失败: {}", e))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!("帆软登录返回 {}：{}", status, text));
        }

        let v: Value = serde_json::from_str(&text)
            .map_err(|e| format!("帆软登录响应不是合法 JSON：{}（原文：{}）", e, text))?;

        // 兜底提取 JWT：多个版本路径
        let jwt = v
            .pointer("/data/accessToken")
            .or_else(|| v.pointer("/data/token"))
            .or_else(|| v.pointer("/data/fine_auth_token"))
            .or_else(|| v.pointer("/accessToken"))
            .or_else(|| v.pointer("/token"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());

        let jwt = match jwt {
            Some(j) if !j.is_empty() => j,
            _ => {
                let err_msg = v
                    .get("errorMsg")
                    .or_else(|| v.get("description"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                return Err(format!(
                    "帆软登录未拿到 JWT：{}（原文：{}）",
                    if err_msg.is_empty() { "未知错误" } else { err_msg },
                    text
                ));
            }
        };

        // 14 天 - 1 小时兜底
        let exp = decode_jwt_exp(&jwt).unwrap_or_else(|| now_unix() + 14 * 86400 - 3600);

        Ok(CachedAuth {
            account: self.account.clone(),
            base_url: self.base_url.clone(),
            jwt,
            expires_at: exp,
            session_id: None,
        })
    }

    /// 拿一个一定可用的 JWT：缓存里的够新就用，否则重新登录。
    ///
    /// 实测帆软 login 返回的是 ~13h 短期 token（不是 14 天的 fine_auth_token），
    /// 所以阈值定 30 分钟——剩 <30min 时 silent re-login。
    pub async fn ensure_valid_auth(&self) -> Result<CachedAuth, String> {
        let now = now_unix();
        if let Some(cached) = load_cached_auth() {
            let matches_identity =
                cached.account == self.account && cached.base_url == self.base_url;
            let still_fresh = cached.expires_at - now > 1800;
            if matches_identity && still_fresh {
                return Ok(cached);
            }
        }
        let fresh = self.login().await?;
        save_cached_auth(&fresh)?;
        Ok(fresh)
    }

    // ========================================================================
    // 报表调用链：open → submit_filter → fetch_html
    // ========================================================================

    /// 打开报表页，从 HTML 里抠 sessionID（UUID 形式）。
    ///
    /// 实测发现：cid 不在 HTML 里（32-hex 串去重 0 个），FR 后端真正用的主键是 sessionID
    /// (UUID)；cid 完全由浏览器 JS 客户端生成，server 在第一次 read_w_content 时建立
    /// (sessionID, cid) → state 映射。所以这里只抠 sessionID，cid 走 generate_cid() 客户端造。
    ///
    /// sessionID 来自 HTML 里的 `FR.SessionMgr.register('<uuid>', contentPane)` 或
    /// `this.currentSessionID = '<uuid>'`。
    pub async fn open_report_and_get_session(
        &self,
        jwt: &str,
        viewlet: &str,
    ) -> Result<String, String> {
        let url = self.url(&format!(
            "/webroot/decision/view/report?op=write&viewlet={}",
            urlencoding::encode(viewlet)
        ));
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", jwt))
            .header("User-Agent", UA)
            .header("Accept", "text/html,application/xhtml+xml,*/*")
            .send()
            .await
            .map_err(|e| format!("打开报表请求失败: {}", e))?;
        let status = resp.status();
        let html = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!(
                "打开报表返回 {}：{}",
                status,
                html.chars().take(200).collect::<String>()
            ));
        }

        // dump HTML 备查
        let debug_path = jarvis_dir().join("finereport-debug.html");
        if let Err(e) = std::fs::write(&debug_path, &html) {
            eprintln!("[FineReport] 写 debug HTML 失败（不致命）: {}", e);
        }

        // 抠 sessionID：优先 FR.SessionMgr.register('<uuid>', ...)，兜底 currentSessionID = '<uuid>'
        let re_register = regex::Regex::new(
            r#"FR\.SessionMgr\.register\(\s*['"]([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})['"]"#
        ).map_err(|e| e.to_string())?;
        if let Some(c) = re_register.captures(&html) {
            return Ok(c[1].to_string());
        }
        let re_current = regex::Regex::new(
            r#"currentSessionID\s*=\s*['"]([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})['"]"#
        ).map_err(|e| e.to_string())?;
        if let Some(c) = re_current.captures(&html) {
            return Ok(c[1].to_string());
        }
        // 再兜底：HTML 里任何 UUID 形式都试一下
        let re_uuid = regex::Regex::new(
            r#"([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})"#
        ).map_err(|e| e.to_string())?;
        if let Some(c) = re_uuid.captures(&html) {
            return Ok(c[1].to_string());
        }

        Err(format!(
            "未抠到 sessionID。HTML 总长 {} 字符（已保存到 {}）",
            html.len(),
            debug_path.display()
        ))
    }

    /// 客户端造一个 cid。
    ///
    /// 格式 `<32-hex>#<13 digit ms>#<8-hex>`，复刻 FR.fs.WriteUtils.getReadWContentID() 行为。
    /// 服务端把 cid 当不透明 token 用：第一次见到时和 sessionID 建映射，后续按 cid 命中缓存。
    fn generate_cid(session_id: &str) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // 32-hex：MD5(sessionID + "_" + ts_ms + "_" + ts_ms<<1) —— 任意 deterministic seed 都行，
        // server 不验证它是真的 MD5，只要 32 hex 即可
        let mut h = Md5::new();
        h.update(session_id.as_bytes());
        h.update(b"_");
        h.update(ts_ms.to_string().as_bytes());
        h.update(b"_");
        h.update((ts_ms.wrapping_mul(2654435761)).to_string().as_bytes());
        let md5_hex = hex::encode(h.finalize());

        // 8-hex 随机后缀：用 ts_ms 低 32 位扰一下
        let rand_part = format!("{:08x}", (ts_ms as u32).wrapping_mul(0x9E3779B1));

        format!("{}#{}#{}", md5_hex, ts_ms, rand_part)
    }

    /// 提交日期 + REAL_NAME 过滤。对应 Image #20 那个 parameters_d 请求。
    ///
    /// REAL_NAME 是多选下拉控件，FR 期望真正的 JSON 数组 `["<姓名>"]`，不是字符串化的
    /// `"[\"<姓名>\"]"`——后者被当 LIKE 字面量匹配，永远 0 条命中（重要教训）。
    /// 空就传空数组 `[]`，相当于不过滤（让 SQL 走 IS NULL 分支不再 AND 收敛）。
    ///
    /// sessionID 加在 URL 参数里——FR 后端用它定位之前 op=write 那一步建立的 writePane 上下文。
    pub async fn submit_filter(
        &self,
        jwt: &str,
        session_id: &str,
        begin: &str,
        end: &str,
        real_name: &str,
    ) -> Result<(), String> {
        let real_name_field: serde_json::Value = if real_name.is_empty() {
            serde_json::json!([])
        } else {
            serde_json::json!([real_name])
        };

        // 帆软 t_zt_effort 列是 datetime（精确到秒），纯 yyyy-MM-dd 会被解为 00:00:00，
        // 导致 `workdate >= begin AND workdate <= end` 同日变成 [00:00:00, 00:00:00] 零宽区间
        // ——只有恰好 00:00:00 的记录命中。补足时间边界 [00:00:00, 23:59:59] 才稳。
        let begin_full = if begin.contains(' ') {
            begin.to_string()
        } else {
            format!("{} 00:00:00", begin)
        };
        let end_full = if end.contains(' ') {
            end.to_string()
        } else {
            format!("{} 23:59:59", end)
        };

        let parameters = serde_json::json!({
            "BEGIN_TIME": begin_full,
            "END_TIME": end_full,
            "REAL_NAME": real_name_field,
            "USER_STATUS": "0",
            "PJ_NAME": "",
            "TASK_NAME": "",
            "EFFORT_WORK": "",
            "ROLE_NAME": "",
            // 用户分享的请求里带的 UI label，保险也带上
            "LABELSTARTTIME_C_C": "日期：",
            "LABELENDTIME_C": "结日：",
            "LABELENDTIME_C_C": "结工状态",
            "LABELSTARTTIME_C_C_C": "项目名：",
            "LABELSTARTTIME_C_C_C_C": "任务名：",
            "LABELSTARTTIME_C_C_C_C_C": "工作内容：",
            "LABELSTARTTIME_C_C_C_C_C_C": "角色：",
        });
        let params_str = parameters.to_string();
        eprintln!("[FineReport] submit_filter payload: {}", params_str);

        let url = self.url(&format!(
            "/webroot/decision/view/report?op=fr_dialog&cmd=parameters_d&sessionID={}",
            urlencoding::encode(session_id)
        ));
        let form_body = format!(
            "__parameters__={}&_={}",
            urlencoding::encode(&params_str),
            now_unix() * 1000
        );
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", jwt))
            .header("Content-Type", "application/x-www-form-urlencoded; charset=UTF-8")
            .header("User-Agent", UA)
            .header("Accept", "*/*")
            .header("X-Requested-With", "XMLHttpRequest")
            .body(form_body)
            .send()
            .await
            .map_err(|e| format!("提交日期过滤失败: {}", e))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        let preview: String = body.chars().take(500).collect();
        eprintln!("[FineReport] submit_filter resp status={} body_preview={}", status, preview);
        if !status.is_success() {
            return Err(format!("parameters_d 返回 {}：{}", status, preview));
        }
        Ok(())
    }

    /// 拉 sheet 的 HTML。reportIndex=0 是「禅道工时汇总」，reportIndex=1 是「任务完成明细」。
    ///
    /// sessionID 加在 URL 参数里定位 writePane 上下文；cid 是客户端生成的不透明 token，
    /// server 第一次见到时和 sessionID 建映射。
    pub async fn fetch_report_html(
        &self,
        jwt: &str,
        session_id: &str,
        cid: &str,
        report_index: u32,
    ) -> Result<String, String> {
        let url = format!(
            "{}/webroot/decision/view/report?_={}&__boxModel__=true&op=fr_write&cmd=read_w_content&sessionID={}&cid={}&reportIndex={}&browserWidth=1920&__cutpage__=&pn=1&__webpage__=true&_paperWidth=1920&_paperHeight=1000&__fit__=false",
            self.base_url,
            now_unix() * 1000,
            urlencoding::encode(session_id),
            urlencoding::encode(cid),
            report_index
        );
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", jwt))
            .header("User-Agent", UA)
            .header("Accept", "application/json, text/javascript, */*; q=0.01")
            .header("X-Requested-With", "XMLHttpRequest")
            .send()
            .await
            .map_err(|e| format!("拉报表 HTML 失败: {}", e))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!(
                "read_w_content 返回 {}：{}",
                status,
                text.chars().take(200).collect::<String>()
            ));
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| {
            format!(
                "报表响应解析失败：{}（原文前 200 字：{}）",
                e,
                text.chars().take(200).collect::<String>()
            )
        })?;
        let html = v
            .get("html")
            .and_then(|x| x.as_str())
            .ok_or_else(|| {
                format!(
                    "报表响应缺 html 字段（原文前 200 字：{}）",
                    text.chars().take(200).collect::<String>()
                )
            })?;
        Ok(html.to_string())
    }
}

const DEFAULT_VIEWLET: &str = "zentao/effort-report-example.cpt";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffortRecord {
    pub date: String,
    pub department: String,
    pub employee: String,
    pub daily_total_hours: f32,
    pub item_hours: f32,
    pub project_name: String,
    pub system: String,
    pub task_name: String,
    pub work_content: String,
}

/// 解析 reportIndex=1「禅道工时统计明细」的 HTML。
///
/// FR x-table 4 区结构（frozen-corner / frozen-north / frozen-west / frozen-center）：
/// 我们只关心 frozen-center 里 row>=4 的 data row。
///
/// 每个 td 形如：
///   `<td ... col="X" ... row="Y" [rowSpan="N"] ...><div ...>TEXT</div></td>`
///
/// 列定义（9 列）：0 部门 / 1 员工 / 2 日期 / 3 当日总工时 / 4 单项工时 /
///                  5 项目名称 / 6 所属系统 / 7 任务名称 / 8 工作内容
///
/// 跨行合并：col 0-3（人员维度）会用 `rowSpan="N"` 合并多行，后续行直接缺这些 col 的 td，
/// 解析时需要把 rowSpan 覆盖区回填到 (row+1..row+N) 同 col。
///
/// 要跳过的：合计行（col=0 文本"合计："+ colSpan=3）、备注行（display:none）、隐藏行
/// （style 含 `display:none`）。
pub fn parse_detail_html(html: &str) -> Result<Vec<EffortRecord>, String> {
    use std::collections::HashMap;

    // 第一步：找 frozen-center div 范围。它在 HTML 最后一个 <td valign="top"> 里。
    // 简化处理：直接全文 regex，反正其他区只有表头，row>=4 的数据 td 只在 frozen-center 里。
    let td_re = regex::Regex::new(
        r#"<td\b([^>]*?)>\s*<div[^>]*>([\s\S]*?)</div>\s*</td>"#,
    ).map_err(|e| format!("td regex 编译失败: {}", e))?;
    let attr_re = regex::Regex::new(r#"(col|row|rowSpan|colSpan|style)="([^"]*)""#)
        .map_err(|e| format!("attr regex 编译失败: {}", e))?;

    // (row, col) → cell text
    let mut cells: HashMap<(u32, u32), String> = HashMap::new();
    let mut max_row: u32 = 0;
    let mut skip_rows: std::collections::HashSet<u32> = std::collections::HashSet::new();

    for cap in td_re.captures_iter(html) {
        let attrs = &cap[1];
        let inner = cap[2].trim();

        let mut col: Option<u32> = None;
        let mut row: Option<u32> = None;
        let mut row_span: u32 = 1;
        let mut col_span: u32 = 1;
        let mut hidden = false;
        for a in attr_re.captures_iter(attrs) {
            match &a[1] {
                "col" => col = a[2].parse().ok(),
                "row" => row = a[2].parse().ok(),
                "rowSpan" => row_span = a[2].parse().unwrap_or(1),
                "colSpan" => col_span = a[2].parse().unwrap_or(1),
                "style" => {
                    if a[2].contains("display:none") {
                        hidden = true;
                    }
                }
                _ => {}
            }
        }
        // tr 级 display:none（"备注：" 那行）：td 自身 style 通常不写 display:none，
        // 由父 tr 控制。但实测备注行的 td 也带 max-height:0px 的 div，文本是"备注："，
        // 直接按文本 = "备注：" 跳即可。
        let (col, row) = match (col, row) {
            (Some(c), Some(r)) => (c, r),
            _ => continue,
        };
        // 跳标题/表头/合计/备注：
        // - row < 4: 标题(1) 表头(2) 合计(3)
        // - 文本=备注：（row=12）
        if row < 4 {
            continue;
        }
        if inner == "备注：" || hidden {
            skip_rows.insert(row);
            continue;
        }

        let text = decode_html_entities(inner);

        // 主单元格写入，rowSpan>1 时覆盖到下面行的同列
        for rr in row..row.saturating_add(row_span) {
            for cc in col..col.saturating_add(col_span) {
                cells.entry((rr, cc)).or_insert_with(|| text.clone());
            }
        }
        if row + row_span - 1 > max_row {
            max_row = row + row_span - 1;
        }
    }

    let mut records: Vec<EffortRecord> = Vec::new();
    for r in 4..=max_row {
        if skip_rows.contains(&r) {
            continue;
        }
        // 至少要有 col 4（单项工时）才算一条明细记录；col 0-3 可能从 rowSpan 回填
        let item_hours_raw = match cells.get(&(r, 4)) {
            Some(v) if !v.is_empty() => v.clone(),
            _ => continue,
        };
        let department = cells.get(&(r, 0)).cloned().unwrap_or_default();
        let employee = cells.get(&(r, 1)).cloned().unwrap_or_default();
        let date = cells.get(&(r, 2)).cloned().unwrap_or_default();
        let daily_total_raw = cells.get(&(r, 3)).cloned().unwrap_or_default();
        let project_name = cells.get(&(r, 5)).cloned().unwrap_or_default();
        let system = cells.get(&(r, 6)).cloned().unwrap_or_default();
        let task_name = cells.get(&(r, 7)).cloned().unwrap_or_default();
        let work_content = cells.get(&(r, 8)).cloned().unwrap_or_default();

        // 跳合计行：col=0 文本"合计："
        if department == "合计：" {
            continue;
        }

        records.push(EffortRecord {
            date,
            department,
            employee,
            daily_total_hours: daily_total_raw.parse::<f32>().unwrap_or(0.0),
            item_hours: item_hours_raw.parse::<f32>().unwrap_or(0.0),
            project_name,
            system,
            task_name,
            work_content,
        });
    }
    Ok(records)
}

/// 解 HTML 实体：FR div 文本里偶尔会出现 `&amp;` `&lt;` `&#45;` 等。
/// 但实测 div 里大多已经是明文，这里做兜底。
fn decode_html_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some(end) = s[i..].find(';') {
                let entity = &s[i + 1..i + end];
                let decoded: Option<char> = if let Some(rest) = entity.strip_prefix('#') {
                    if let Some(hex) = rest.strip_prefix('x').or_else(|| rest.strip_prefix('X')) {
                        u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                    } else {
                        rest.parse::<u32>().ok().and_then(char::from_u32)
                    }
                } else {
                    match entity {
                        "amp" => Some('&'),
                        "lt" => Some('<'),
                        "gt" => Some('>'),
                        "quot" => Some('"'),
                        "apos" => Some('\''),
                        "nbsp" => Some(' '),
                        _ => None,
                    }
                };
                if let Some(ch) = decoded {
                    out.push(ch);
                    i += end + 1;
                    continue;
                }
            }
        }
        // safe utf-8 推进：找下一个字符边界
        let mut step = 1;
        while i + step < bytes.len() && !s.is_char_boundary(i + step) {
            step += 1;
        }
        out.push_str(&s[i..i + step]);
        i += step;
    }
    out
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffortFetchResult {
    pub cid: String,
    pub session_id: String,
    /// 解析后的明细记录（reportIndex=1）
    pub records: Vec<EffortRecord>,
    /// reportIndex=0 「禅道工时汇总」原始 HTML（debug 用）
    pub summary_html: String,
    /// reportIndex=1 「禅道工时任务完成明细」原始 HTML（debug 用）
    pub detail_html: String,
}

/// 拉指定日期范围的工时（双 sheet）。前端测试 / chat tool 都用这个。
/// realName 为空时回退 config.fineReport.realName；都空则不带过滤（拉全部）。
#[tauri::command]
pub async fn finereport_get_efforts(
    begin: String,
    end: String,
    real_name: Option<String>,
) -> Result<EffortFetchResult, String> {
    use std::time::Instant;
    let total = Instant::now();
    eprintln!("[FineReport] === get_efforts begin={} end={} realName={:?} ===", begin, end, real_name);

    let cred = get_fine_report_credentials();
    eprintln!("[FineReport] cred: baseUrl={} account={} realName={} pwd={}",
        cred.base_url, cred.account, cred.real_name, if cred.password.is_empty() { "<空>" } else { "<已读>" });

    // 显式传入 > config > 空（不过滤）
    let effective_real_name = real_name
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| cred.real_name.clone());

    // 隐私保护：不允许空 realName 查询——帆软服务端不会按账号收敛到本人，
    // 空过滤会拉到整个软件部数据。前端、chat tool 都从这里兜底。
    if effective_real_name.is_empty() {
        return Err(
            "未提供中文姓名，已阻止查询。请在设置里填写 fineReport.realName 后重试。".into(),
        );
    }

    let client = FineReportClient::new(cred.base_url, cred.account, cred.password)?;

    let t = Instant::now();
    let auth = client.ensure_valid_auth().await?;
    eprintln!("[FineReport] step1 ensure_valid_auth ok ({}ms) exp={}", t.elapsed().as_millis(), auth.expires_at);

    let t = Instant::now();
    let session_id = client
        .open_report_and_get_session(&auth.jwt, DEFAULT_VIEWLET)
        .await
        .map_err(|e| {
            eprintln!("[FineReport] step2 open_report_and_get_session FAILED ({}ms): {}", t.elapsed().as_millis(), e);
            e
        })?;
    eprintln!("[FineReport] step2 sessionID ok ({}ms) sessionID={}", t.elapsed().as_millis(), session_id);

    let cid = FineReportClient::generate_cid(&session_id);
    eprintln!("[FineReport] step2.5 cid generated: {}", cid);

    let t = Instant::now();
    client
        .submit_filter(&auth.jwt, &session_id, &begin, &end, &effective_real_name)
        .await
        .map_err(|e| {
            eprintln!("[FineReport] step3 submit_filter FAILED ({}ms): {}", t.elapsed().as_millis(), e);
            e
        })?;
    eprintln!("[FineReport] step3 submit_filter ok ({}ms) realName='{}'", t.elapsed().as_millis(), effective_real_name);

    let t = Instant::now();
    let summary_html = client.fetch_report_html(&auth.jwt, &session_id, &cid, 0).await
        .map_err(|e| {
            eprintln!("[FineReport] step4a fetch summary FAILED ({}ms): {}", t.elapsed().as_millis(), e);
            e
        })?;
    eprintln!("[FineReport] step4a summary ok ({}ms) len={}", t.elapsed().as_millis(), summary_html.len());
    let summary_path = jarvis_dir().join("finereport-summary.html");
    let _ = std::fs::write(&summary_path, &summary_html);

    let t = Instant::now();
    let detail_html = client.fetch_report_html(&auth.jwt, &session_id, &cid, 1).await
        .map_err(|e| {
            eprintln!("[FineReport] step4b fetch detail FAILED ({}ms): {}", t.elapsed().as_millis(), e);
            e
        })?;
    eprintln!("[FineReport] step4b detail ok ({}ms) len={}", t.elapsed().as_millis(), detail_html.len());
    let detail_path = jarvis_dir().join("finereport-detail.html");
    let _ = std::fs::write(&detail_path, &detail_html);

    let records = parse_detail_html(&detail_html).unwrap_or_else(|e| {
        eprintln!("[FineReport] parse_detail_html FAILED: {}", e);
        Vec::new()
    });
    eprintln!("[FineReport] parsed {} records", records.len());

    eprintln!("[FineReport] === get_efforts DONE total {}ms ===", total.elapsed().as_millis());
    Ok(EffortFetchResult { cid, session_id, records, summary_html, detail_html })
}

// ============================================================================
// 测试连接（设置面板按钮）
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FineReportTestRequest {
    pub base_url: String,
    pub account: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FineReportTestResult {
    pub ok: bool,
    pub message: String,
}

/// 测试帆软登录。密码空 → 回退 keychain 已存值（同禅道行为）。
#[tauri::command]
pub async fn finereport_test_connection(
    req: FineReportTestRequest,
) -> Result<FineReportTestResult, String> {
    let base = req.base_url.trim().to_string();
    if base.is_empty() {
        return Ok(FineReportTestResult { ok: false, message: "帆软地址不能为空".into() });
    }
    if req.account.trim().is_empty() {
        return Ok(FineReportTestResult { ok: false, message: "账号不能为空".into() });
    }

    let password = if req.password.is_empty() {
        match keyring_entry(req.account.trim()).and_then(|e| match e.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(format!("读取密钥链失败: {}", err)),
        }) {
            Ok(Some(p)) => p,
            _ => req.password.clone(),
        }
    } else {
        req.password.clone()
    };

    let client = match FineReportClient::new(base, req.account, password) {
        Ok(c) => c,
        Err(e) => return Ok(FineReportTestResult { ok: false, message: e }),
    };

    match client.login().await {
        Ok(auth) => {
            // 测试成功顺手把 JWT 缓存起来，下次调用免登录
            let _ = save_cached_auth(&auth);
            let now = now_unix();
            let secs_left = auth.expires_at - now;
            let days_left = secs_left / 86400;
            let hours_left = (secs_left % 86400) / 3600;
            // 诊断信息：把 JWT 头部 + exp 原始值带回来，方便定位"0 天"
            let jwt_preview: String = auth.jwt.chars().take(40).collect();
            Ok(FineReportTestResult {
                ok: true,
                message: format!(
                    "登录成功。JWT={}…；exp={}, now={}, 剩 {} 天 {} 小时",
                    jwt_preview, auth.expires_at, now, days_left.max(0), hours_left.max(0)
                ),
            })
        }
        Err(e) => Ok(FineReportTestResult { ok: false, message: e }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 极简 fixture：还原明细 HTML 的关键结构——2 名员工，各 1-2 条记录。
    /// 覆盖 rowSpan 回填 / 合计行跳过 / "备注：" 隐藏行跳过。
    #[test]
    fn parse_detail_minimal() {
        let html = r#"
<table><tbody>
<tr><td col="0" row="1" colSpan="9"><div>软件部禅道工时统计明细</div></td></tr>
<tr><td col="0" row="2"><div>部门</div></td></tr>
<tr><td col="0" row="3" colSpan="3"><div>合计：</div></td><td col="3" row="3"><div>12</div></td><td col="4" row="3"><div>12</div></td></tr>
<tr>
  <td rowSpan="2" col="0" row="4"><div>开发组</div></td>
  <td rowSpan="2" col="1" row="4"><div>丁佳斌</div></td>
  <td rowSpan="2" col="2" row="4"><div>2026-05-28</div></td>
  <td rowSpan="2" col="3" row="4"><div>8</div></td>
  <td col="4" row="4"><div>4</div></td>
  <td col="5" row="4"><div>项目A</div></td>
  <td col="6" row="4"><div>系统A</div></td>
  <td col="7" row="4"><div>任务A</div></td>
  <td col="8" row="4"><div>内容A</div></td>
</tr>
<tr>
  <td col="4" row="5"><div>4</div></td>
  <td col="5" row="5"><div>项目A</div></td>
  <td col="6" row="5"><div>系统A</div></td>
  <td col="7" row="5"><div>任务B</div></td>
  <td col="8" row="5"><div>内容B</div></td>
</tr>
<tr>
  <td col="0" row="6"><div>运维组</div></td>
  <td col="1" row="6"><div>吕巧艳</div></td>
  <td col="2" row="6"><div>2026-05-28</div></td>
  <td col="3" row="6"><div>4</div></td>
  <td col="4" row="6"><div>4</div></td>
  <td col="5" row="6"><div>项目B</div></td>
  <td col="6" row="6"><div>系统B</div></td>
  <td col="7" row="6"><div>任务C</div></td>
  <td col="8" row="6"><div>跟踪进度&amp;问题</div></td>
</tr>
<tr><td col="0" row="7"><div>备注：</div></td></tr>
</tbody></table>
"#;
        let records = parse_detail_html(html).expect("parse ok");
        assert_eq!(records.len(), 3, "expect 3 records");

        assert_eq!(records[0].employee, "丁佳斌");
        assert_eq!(records[0].department, "开发组");
        assert_eq!(records[0].daily_total_hours, 8.0);
        assert_eq!(records[0].item_hours, 4.0);
        assert_eq!(records[0].task_name, "任务A");

        // rowSpan 回填：第二条应仍带丁佳斌/开发组/2026-05-28/8h
        assert_eq!(records[1].employee, "丁佳斌");
        assert_eq!(records[1].department, "开发组");
        assert_eq!(records[1].daily_total_hours, 8.0);
        assert_eq!(records[1].task_name, "任务B");

        assert_eq!(records[2].employee, "吕巧艳");
        assert_eq!(records[2].department, "运维组");
        // HTML 实体解码
        assert_eq!(records[2].work_content, "跟踪进度&问题");
    }

    #[test]
    fn decode_entities_chinese_and_ascii() {
        assert_eq!(decode_html_entities("&#37096;&#38376;"), "部门");
        assert_eq!(decode_html_entities("a&amp;b"), "a&b");
        assert_eq!(decode_html_entities("&#x8F6F;&#x4EF6;"), "软件");
        assert_eq!(decode_html_entities("plain"), "plain");
    }
}
