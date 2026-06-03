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

        // dump HTML 备查（仅 debug 构建；release 不落盘，避免工时 PII 残留磁盘）
        let debug_hint = if cfg!(debug_assertions) {
            let debug_path = jarvis_dir().join("finereport-debug.html");
            if let Err(e) = std::fs::write(&debug_path, &html) {
                eprintln!("[FineReport] 写 debug HTML 失败（不致命）: {}", e);
            }
            format!("，已保存到 {}", debug_path.display())
        } else {
            String::new()
        };

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
        let re_uuid =
            regex::Regex::new(r#"([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})"#)
                .map_err(|e| e.to_string())?;
        if let Some(c) = re_uuid.captures(&html) {
            return Ok(c[1].to_string());
        }

        Err(format!(
            "未抠到 sessionID。HTML 总长 {} 字符{}",
            html.len(),
            debug_hint
        ))
    }

    /// 客户端造一个 cid。
    ///
    /// 格式 `<32-hex>#<13 digit ms>#<8-hex>`，复刻 FR.fs.WriteUtils.getReadWContentID() 行为。
    /// 服务端把 cid 当不透明 token 用：第一次见到时和 sessionID 建映射，后续按 cid 命中缓存。
    pub fn generate_cid(session_id: &str) -> String {
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
            eprintln!("[FineReport] submit_filter payload: {}", params_str);
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
            eprintln!(
                "[FineReport] submit_filter resp status={} body_preview={}",
                status, preview
            );
        }
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
        let html = v.get("html").and_then(|x| x.as_str()).ok_or_else(|| {
            format!(
                "报表响应缺 html 字段（原文前 200 字：{}）",
                text.chars().take(200).collect::<String>()
            )
        })?;
        Ok(html.to_string())
    }
}
