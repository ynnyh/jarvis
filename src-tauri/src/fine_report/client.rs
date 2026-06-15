#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use md5::{Digest, Md5};
use reqwest::cookie::Jar;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::settings::jarvis_dir;
use crate::fine_report::credentials::{UA, get_fine_report_credentials};

/// 检测帆软返回的 HTML 是否为登录页（JWT 失效时服务器返回 200 + 登录页而非 401）。
///
/// 只认正向信号：登录端点 URL `decision/login`。不要用「没有 FR 报表 JS 就当登录页」。
/// 这种反向启发式——read_w_content 返回的是 `{"html":"<table>..."}` 片段，本就不含
/// FR.SessionMgr / finereport.main.js，会被误判成登录页导致死循环重登。
fn is_login_page(html: &str) -> bool {
    html.to_lowercase().contains("decision/login")
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

pub(super) fn load_cached_auth() -> Option<CachedAuth> {
    let raw = std::fs::read_to_string(cache_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

pub(super) fn save_cached_auth(auth: &CachedAuth) -> Result<(), String> {
    let dir = jarvis_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建配置目录失败: {}", e))?;
    let json = serde_json::to_string_pretty(auth).map_err(|e| e.to_string())?;
    crate::util::write_atomic(&cache_path(), &json)
        .map_err(|e| format!("写入帆软认证缓存失败: {}", e))
}

pub(super) fn delete_cached_auth() {
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

pub(super) fn now_unix() -> i64 {
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

    /// 默认 10s timeout：日报这类小查询足够，定位卡点也灵敏。成本/团队人员
    /// 这类大报表（all_people + 长周期）走 `new_with_timeout` 调到 60s。
    pub fn new(base_url: String, account: String, password: String) -> Result<Self, String> {
        Self::new_with_timeout(base_url, account, password, 10)
    }

    /// 同 `new` 但可指定请求超时（秒）。大报表查询（all_people + 长周期）
    /// 数据量大，10s 扛不住，调用方传 60。
    pub fn new_with_timeout(
        base_url: String,
        account: String,
        password: String,
        timeout_secs: u64,
    ) -> Result<Self, String> {
        if base_url.is_empty() {
            return Err("帆软 baseUrl 未配置".into());
        }
        let mut base = base_url.trim().trim_end_matches('/').to_string();
        if !base.to_lowercase().starts_with("http") {
            base = format!("http://{}", base);
        }
        let jar = Arc::new(Jar::default());
        let client = reqwest::Client::builder()
            .cookie_provider(jar.clone())
            .timeout(Duration::from_secs(timeout_secs))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| format!("帆软 HTTP client 构造失败: {}", e))?;
        Ok(Self {
            base_url: base,
            account,
            password,
            client,
            jar,
        })
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
                    if err_msg.is_empty() {
                        "未知错误"
                    } else {
                        err_msg
                    },
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

    /// 打开报表页，建立 cookie 会话，从 HTML 里抓 sessionID。
    ///
    /// sessionID 是帆软定位 writePane 上下文的关键参数：submit_filter / read_w_content
    /// 等 API 都需要在 URL 里带 sessionID，光靠 cookie 不够。
    /// 返回抓到的 sessionID（UUID 形式）。
    pub async fn open_report(&self, jwt: &str, viewlet: &str) -> Result<String, String> {
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
        // HTML 为空或返回登录页说明 JWT 已过期，调用方应清缓存重试
        if html.is_empty() {
            return Err("AUTH_EXPIRED: 打开报表返回空内容，JWT 可能已过期".into());
        }
        if is_login_page(&html) {
            return Err("AUTH_EXPIRED: 打开报表返回登录页 HTML，JWT 已过期".into());
        }

        // dump HTML 备查（仅 debug 构建；release 不落盘，避免工时 PII 残留磁盘）
        if cfg!(debug_assertions) {
            let debug_path = jarvis_dir().join("finereport-debug.html");
            if let Err(e) = std::fs::write(&debug_path, &html) {
                tracing::error!(target: "FineReport", "写 debug HTML 失败（不致命）: {}", e);
            }
        }

        // 抓 sessionID：帆软 HTML 里会包含 FR.SessionMgr.register(...) 或 currentSessionID 赋值
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
        // 再兜底：HTML 里任意 UUID
        let re_uuid = regex::Regex::new(
            r#"([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})"#
        ).map_err(|e| e.to_string())?;
        if let Some(c) = re_uuid.captures(&html) {
            return Ok(c[1].to_string());
        }

        Err(format!("未抓到 sessionID。HTML 总长 {} 字符", html.len()))
    }

    /// 初始化某个 reportIndex 的编辑器会话。对应真实浏览器抓包里 read_w_content 之前
    /// 那个 `op=fr_write&cmd=getEditorConfig` 请求——服务端靠它把当前 cookie 会话和
    /// 这个 reportIndex 的 writePane 绑定好，之后 read_w_content 才能命中。
    ///
    /// 它只是初始化握手，body 是不是 JSON 不重要，所以这里不解析 body：status 非 success
    /// 直接报错；命中登录页当 AUTH_EXPIRED 抛给上层重登；否则 `Ok(())`。
    pub async fn get_editor_config(&self, jwt: &str, report_index: u32) -> Result<(), String> {
        let url = self.url(&format!(
            "/webroot/decision/view/report?op=fr_write&cmd=getEditorConfig&reportIndex={}&_={}",
            report_index,
            now_unix() * 1000
        ));
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", jwt))
            .header("User-Agent", UA)
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Accept", "application/json, text/javascript, */*; q=0.01")
            .send()
            .await
            .map_err(|e| format!("getEditorConfig 请求失败: {}", e))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!(
                "getEditorConfig 返回 {}：{}",
                status,
                body.chars().take(200).collect::<String>()
            ));
        }
        if is_login_page(&body) {
            return Err("AUTH_EXPIRED: getEditorConfig 返回登录页 HTML，JWT 已过期".into());
        }
        Ok(())
    }

    /// 生成 CID（客户端标识符），复刻帆软 fingerprintHandle 的输出格式。
    ///
    /// 帆软前端 JS 的 fingerprintHandle 实现：
    ///   1. 用 FingerprintJS 生成 32-hex 指纹
    ///   2. 拼接 "#" + 时间戳
    ///   3. 拼接 "#" + MD5(前两段).hex().slice(-8)  // 校验和
    ///
    /// 服务端 BrowserConcurrencyManager.verifyCid 校验此校验和。
    /// 第一段用 MD5 替代 FingerprintJS 即可——服务端不验指纹内容，只验第三段与一二段的 MD5 关系。
    pub fn generate_cid(seed: &str) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // 第一段：32-hex（MD5 替代 FingerprintJS 指纹）
        let mut h = Md5::new();
        h.update(seed.as_bytes());
        h.update(b"_");
        h.update(ts_ms.to_string().as_bytes());
        let part1 = hex::encode(h.finalize());

        // 第二段：毫秒时间戳
        let part2 = ts_ms.to_string();

        // 第三段：MD5("part1#part2") hex 的后 8 位（校验和）
        let checksum_input = format!("{}#{}", part1, part2);
        let mut h2 = Md5::new();
        h2.update(checksum_input.as_bytes());
        let checksum_hex = hex::encode(h2.finalize());
        let part3 = &checksum_hex[checksum_hex.len() - 8..];

        format!("{}#{}#{}", part1, part2, part3)
    }

    /// 提交日期 + REAL_NAME 过滤。对应真实浏览器抓包里的 parameters_d 请求。
    ///
    /// REAL_NAME 是多选下拉控件，FR 期望真正的 JSON 数组 `["<姓名>"]`，不是字符串化的
    /// `"[\"<姓名>\"]"`——后者被当 LIKE 字面量匹配，永远 0 条命中（重要教训）。
    /// 空就传空数组 `[]`，相当于不过滤（让 SQL 走 IS NULL 分支不再 AND 收敛）。
    ///
    /// 真实浏览器抓包显示这个请求不带 sessionID，参数全在 POST body 里，靠 cookie 会话定位上下文。
    ///
    /// `project_name` 填进 PJ_NAME（帆软项目名筛选位）；空串=不按项目过滤（日报/chat 路径）。
    ///
    /// `user_status` 填进 USER_STATUS（员工状态筛选位）：`"0"`=仅在职（日报/chat 维持现状）；
    /// `""`=不筛（含离职，成本分析「含离职」勾选时用）。
    pub async fn submit_filter(
        &self,
        jwt: &str,
        session_id: &str,
        begin: &str,
        end: &str,
        real_name: &str,
        project_name: &str,
        user_status: &str,
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
            "USER_STATUS": user_status,
            "PJ_NAME": project_name,
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
        if cfg!(debug_assertions) {
            tracing::debug!(target: "FineReport", "submit_filter payload: {}", params_str);
        }

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
            .header(
                "Content-Type",
                "application/x-www-form-urlencoded; charset=UTF-8",
            )
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
        if cfg!(debug_assertions) {
            tracing::debug!(target: "FineReport", "submit_filter resp status={} body_preview={}", status, preview);
        }
        if !status.is_success() {
            return Err(format!("parameters_d 返回 {}：{}", status, preview));
        }
        Ok(())
    }

    /// 拉 sheet 的 HTML。reportIndex=0 是「禅道工时汇总」，reportIndex=1 是「任务完成明细」。
    ///
    /// read_w_content 需要 sessionID 参数来定位 writePane 上下文，光靠 cookie 不够。
    /// cid 是客户端生成的不透明 token，server 第一次见到时和当前 cookie 会话建映射，后续按 cid 命中缓存。
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
        // read_w_content 正常一定返回 JSON；拿到 HTML（以 `<` 开头）说明会话/认证失效
        // （帆软返回 200 + 登录/错误页而非 401，且页面里不一定有 decision/login 字样）。
        // 当 AUTH_EXPIRED 抛给上层触发重登重试。
        let trimmed = text.trim_start();
        if trimmed.is_empty() {
            return Err("AUTH_EXPIRED: read_w_content 返回空响应，会话可能已失效".into());
        }
        if trimmed.starts_with('<') || is_login_page(&text) {
            if cfg!(debug_assertions) {
                let p = jarvis_dir().join("finereport-readwcontent-fail.html");
                let _ = std::fs::write(&p, &text);
            }
            return Err("AUTH_EXPIRED: read_w_content 返回 HTML（非 JSON），会话已失效".into());
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| {
            format!(
                "报表响应解析失败：{}（原文前 200 字：{}）",
                e,
                text.chars().take(200).collect::<String>()
            )
        })?;
        let html = v.get("html").and_then(|x| x.as_str()).ok_or_else(|| {
            format!(
                "报表响应缺 html 字段（原文前 200 字：{}）",
                text.chars().take(200).collect::<String>()
            )
        })?;
        Ok(html.to_string())
    }
}
