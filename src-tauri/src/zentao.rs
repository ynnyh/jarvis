// 禅道客户端：移植自 src/providers/zentao-provider.ts
//
// 两条独立鉴权通道（禅道 OSS v18+/v20 实测）：
//   1. **OpenAPI Token**（POST /api.php/v1/tokens）：用于读任务（/api.php/v1/tasks 等）
//   2. **zentaosid Cookie**（用户表单登录拿）：用于 PATH_INFO 表单端点（写工时等）
//
// **关键红线**：禅道服务端在 token/cookie 失效时会"假成功"——HTTP 200 + 看似正常的
// JSON 响应，但实际未写入库。所以 add_effort 必须 verify-after-write：写后再读
// consumed 比对，差额不对就抛错。这个配方在 feedback_zentao_workhour_recipe 记忆里。
//
// 防误关：禅道用 left[1]=0 决定任务关闭。必须先读 current left 原样回填，
// consumed 加值但 left 不变，否则会自动 done 任务。

#![allow(dead_code)]
// wiring 在 M5 接入。

use std::sync::Arc;
use std::time::Duration;

use md5::{Digest, Md5};
use reqwest::cookie::Jar;
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::settings::{get_zentao_credentials, ZentaoCredentials};

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZentaoTestResult {
    pub ok: bool,
    pub message: String,
}

// ===== 任务工时分类 =====

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskCategory {
    Ops,     // 运维：需求对接、数据核对、问题反馈
    Daily,   // 事务：公共会议、培训
    Feature, // 新增功能：任务名含 XZGN
    Other,   // 其他：未匹配上述关键词的
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZenTaoTaskBrief {
    pub id: String,
    pub name: String,
    pub status: String,
    pub pri: u8,
    pub deadline: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassifiedTasks {
    pub ops: Vec<ZenTaoTaskBrief>,
    pub daily: Vec<ZenTaoTaskBrief>,
    pub feature: Vec<ZenTaoTaskBrief>,
    pub other: Vec<ZenTaoTaskBrief>,
}

fn classify_task(name: &str) -> TaskCategory {
    // 运维：需求对接 / 数据核对 / 问题反馈
    for kw in &["需求对接", "数据核对", "问题反馈"] {
        if name.contains(kw) {
            return TaskCategory::Ops;
        }
    }
    // 新增功能：任务名含 XZGN（大小写敏感，XZGN 是项目编码约定）
    if name.contains("XZGN") {
        return TaskCategory::Feature;
    }
    // 事务：公共会议 / 培训
    for kw in &["公共会议", "培训"] {
        if name.contains(kw) {
            return TaskCategory::Daily;
        }
    }
    TaskCategory::Other
}

pub struct ZentaoClient {
    base_url: String,
    account: String,
    password: String,
    session_cookie: Option<String>,
    /// OpenAPI v1 token，按需自动 authenticate 后填
    token: tokio::sync::Mutex<Option<String>>,
    /// 共享 cookie jar：登录拿到 zentaosid 后自动带在后续请求里
    jar: Arc<Jar>,
    /// 所有请求共用一个 client（cookie/connection 复用）
    client: reqwest::Client,
}

impl ZentaoClient {
    /// 用当前 settings 初始化。base_url 末尾会被强制补 /，否则 URL join 会丢掉子路径。
    pub fn from_settings() -> Result<Self, String> {
        let cred = get_zentao_credentials();
        Self::new(cred)
    }

    pub fn new(cred: ZentaoCredentials) -> Result<Self, String> {
        let mut base_url = cred.base_url.clone();
        if base_url.is_empty() {
            return Err("禅道 baseUrl 未配置".into());
        }
        if !base_url.ends_with('/') {
            base_url.push('/');
        }
        let jar = Arc::new(Jar::default());
        // 如果用户预先塞了 sessionCookie，直接注入到 jar
        if let Some(sid) = &cred.session_cookie {
            if !sid.is_empty() {
                let cookie_url = reqwest::Url::parse(&base_url).map_err(|e| e.to_string())?;
                let cookie_str = format!("zentaosid={}; Path=/", sid);
                jar.add_cookie_str(&cookie_str, &cookie_url);
            }
        }

        let client = reqwest::Client::builder()
            .cookie_provider(jar.clone())
            .redirect(Policy::none()) // 登录流程依赖 302 不被吃掉
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| format!("禅道 HTTP client 构造失败: {}", e))?;

        Ok(Self {
            base_url,
            account: cred.account,
            password: cred.password,
            session_cookie: cred.session_cookie,
            token: tokio::sync::Mutex::new(None),
            jar,
            client,
        })
    }

    /// 拼接到 base_url 下的子路径（不带前导 /，例如 "api.php/v1/tokens"）。
    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    // ============================================================================
    // OpenAPI Token 认证（读任务用）
    // ============================================================================

    /// 拿 OpenAPI v1 token。已有就直接返回，否则 POST /api.php/v1/tokens。
    async fn ensure_token(&self) -> Result<String, String> {
        {
            let guard = self.token.lock().await;
            if let Some(t) = guard.as_ref() {
                return Ok(t.clone());
            }
        }
        if self.account.is_empty() || self.password.is_empty() {
            return Err("禅道账号或密码为空（检查 keychain 与 config.json）".into());
        }
        let url = self.url("api.php/v1/tokens");
        let body = serde_json::json!({
            "account": self.account,
            "password": self.password,
        });
        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("User-Agent", UA)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("禅道 token 请求失败: {}", e))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!(
                "禅道 token HTTP {}: {}",
                status.as_u16(),
                crate::util::truncate_chars(&text, 300)
            ));
        }
        let v: Value = serde_json::from_str(&text).map_err(|_| {
            format!(
                "禅道 token 响应非 JSON: {}",
                crate::util::truncate_chars(&text, 200)
            )
        })?;
        let token = v
            .get("token")
            .and_then(|x| x.as_str())
            .ok_or_else(|| {
                format!(
                    "禅道 token 响应缺 token 字段: {}",
                    crate::util::truncate_chars(&text, 200)
                )
            })?
            .to_string();
        *self.token.lock().await = Some(token.clone());
        Ok(token)
    }

    fn auth_headers(token: &str) -> Vec<(&'static str, String)> {
        vec![
            ("Content-Type", "application/json".to_string()),
            ("User-Agent", UA.to_string()),
            ("Token", token.to_string()),
        ]
    }

    // ============================================================================
    // 表单登录（写工时用，必须用 zentaosid cookie）
    // ============================================================================

    /// 用账号密码做一次传统表单登录拿 zentaosid。流程见模块顶部注释。
    pub async fn login_via_form(&self) -> Result<(), String> {
        if self.account.is_empty() || self.password.is_empty() {
            return Err("禅道账号或密码为空".into());
        }
        // 已有 session（手动注入或已登录）就跳过
        if self.has_session_cookie() {
            return Ok(());
        }

        let login_url = self.url("user-login.html");

        // 1. GET 登录页拿初始 zentaosid
        let _ = self
            .client
            .get(&login_url)
            .header("User-Agent", UA)
            .send()
            .await
            .map_err(|e| format!("禅道登录页请求失败: {}", e))?;
        // cookie 由 jar 自动接管

        // 2. GET refreshRandom —— 服务端把新 rand 塞进 session 并返回给前端
        let rand_url = self.url("user-refreshRandom.html");
        let rand_res = self
            .client
            .get(&rand_url)
            .header("User-Agent", UA)
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Accept", "application/json, text/javascript, */*; q=0.01")
            .header("Referer", &login_url)
            .send()
            .await
            .map_err(|e| format!("refreshRandom 请求失败: {}", e))?;
        let rand_status = rand_res.status();
        let rand_text = rand_res.text().await.unwrap_or_default();
        // 响应可能是纯数字 / JSON / HTML，正则抓第一串连续数字
        let verify_rand = extract_first_digits(&rand_text).ok_or_else(|| {
            format!(
                "禅道 session 登录失败：refreshRandom 响应无可识别的 rand。HTTP {}，前 200 字: {}",
                rand_status.as_u16(),
                crate::util::truncate_chars(&rand_text, 200)
            )
        })?;

        // 3. md5(md5(password) + rand)
        let encrypted = md5_hex(&format!("{}{}", md5_hex(&self.password), verify_rand));

        // 4. POST 登录（带 cookie）
        let body = format!(
            "account={}&password={}&passwordStrength=1&verifyRand={}&referer={}",
            urlencoding::encode(&self.account),
            encrypted,
            verify_rand,
            urlencoding::encode("/"),
        );
        let login_res = self
            .client
            .post(&login_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("User-Agent", UA)
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Referer", &login_url)
            .body(body)
            .send()
            .await
            .map_err(|e| format!("禅道登录 POST 失败: {}", e))?;
        let login_status = login_res.status();
        // 不读 body，jar 会自动接管 set-cookie

        if !self.has_session_cookie() {
            let text = login_res.text().await.unwrap_or_default();
            return Err(format!(
                "禅道 session 登录失败：响应未带 zentaosid cookie。HTTP {}，前 200 字: {}",
                login_status.as_u16(),
                crate::util::truncate_chars(&text, 200)
            ));
        }

        // 5. verify —— 未登录态也会下发 cookie，必须实测
        let verify_url = self.url("my.html");
        let verify_res = self
            .client
            .get(&verify_url)
            .header("User-Agent", UA)
            .send()
            .await
            .map_err(|e| format!("禅道 verify (/my.html) 请求失败: {}", e))?;
        let v_status = verify_res.status();
        if v_status.is_redirection() {
            let loc = verify_res
                .headers()
                .get("location")
                .and_then(|h| h.to_str().ok())
                .unwrap_or("");
            if loc.to_lowercase().contains("login") {
                return Err(format!(
                    "禅道 session 登录失败：/my.html 被重定向到登录页 ({})。检查账号/密码（rand={}）",
                    loc, verify_rand
                ));
            }
        }
        if v_status.as_u16() == 200 {
            let text = verify_res.text().await.unwrap_or_default();
            if text.contains("id=\"userLogin\"")
                || text.contains("id='userLogin'")
                || text.contains("name=\"passwordStrength\"")
                || text.contains("name='passwordStrength'")
            {
                return Err(format!(
                    "禅道 session 登录失败：/my.html 返回登录页（密码 md5 流程未通过，rand={}）。前 200 字: {}",
                    verify_rand,
                    crate::util::truncate_chars(&text, 200)
                ));
            }
        }
        Ok(())
    }

    fn has_session_cookie(&self) -> bool {
        use reqwest::cookie::CookieStore;
        let cookie_url = match reqwest::Url::parse(&self.base_url) {
            Ok(u) => u,
            Err(_) => return false,
        };
        match self.jar.cookies(&cookie_url) {
            Some(h) => h
                .to_str()
                .map(|s| s.contains("zentaosid="))
                .unwrap_or(false),
            None => false,
        }
    }

    // ============================================================================
    // 业务调用
    // ============================================================================

    /// 拉"指派给我"的任务列表。返回原始 JSON 数组（每条是 ZenTao 后端的 task 对象）。
    pub async fn get_my_tasks(&self) -> Result<Vec<Value>, String> {
        let token = self.ensure_token().await?;
        // 工作台 .json 端点，pagerMyWork cookie 控制每页数量（一次性拉全）
        let url = self.url("my-work-task-assignedTo--id_desc.json");
        let resp = self
            .client
            .get(&url)
            .header("Content-Type", "application/json")
            .header("User-Agent", UA)
            .header("Token", &token)
            .header("Cookie", "pagerMyWork=200")
            .send()
            .await
            .map_err(|e| format!("获取任务失败: {}", e))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!(
                "获取任务失败 HTTP {}: {}",
                status.as_u16(),
                crate::util::truncate_chars(&text, 300)
            ));
        }
        let json: Value = serde_json::from_str(&text).map_err(|_| {
            format!(
                "获取任务返回非 JSON: {}",
                crate::util::truncate_chars(&text, 200)
            )
        })?;
        if json.get("status").and_then(|v| v.as_str()) != Some("success") {
            return Err("禅道返回数据异常（status != success）".into());
        }
        // data 通常是字符串化 JSON
        let inner = match json.get("data") {
            Some(Value::String(s)) => {
                serde_json::from_str::<Value>(s).map_err(|e| format!("data 字段解析失败: {}", e))?
            }
            Some(v) => v.clone(),
            None => return Err("禅道返回数据缺 data 字段".into()),
        };
        let tasks: Vec<Value> = inner
            .get("tasks")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        // 过滤已关闭/取消
        Ok(tasks
            .into_iter()
            .filter(|t| {
                let s = t.get("status").and_then(|v| v.as_str()).unwrap_or("");
                s != "closed" && s != "cancel"
            })
            .collect())
    }

    /// 拉任务并按工时分类：运维 / 事务 / 新增功能 / 其他。
    /// 分类依据：任务名关键词匹配。运维 = 需求对接|数据核对|问题反馈；
    /// 新增功能 = 含 XZGN；事务 = 公共会议|培训；其余归入其他。
    pub async fn get_classified_tasks(&self) -> Result<ClassifiedTasks, String> {
        let tasks = self.get_my_tasks().await?;
        let mut ops = Vec::new();
        let mut daily = Vec::new();
        let mut feature = Vec::new();
        let mut other = Vec::new();

        for t in &tasks {
            // 禅道 id 可能是数字或字符串，统一转成字符串
            let id = t
                .get("id")
                .and_then(|v| {
                    v.as_str()
                        .map(String::from)
                        .or_else(|| v.as_u64().map(|n| n.to_string()))
                })
                .unwrap_or_default();
            let name = t
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let status = t
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let pri = t.get("pri").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let deadline = t
                .get("deadline")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let item = ZenTaoTaskBrief {
                id: id.clone(),
                name: name.clone(),
                status,
                pri,
                deadline,
            };

            let category = classify_task(&name);
            match category {
                TaskCategory::Ops => ops.push(item),
                TaskCategory::Daily => daily.push(item),
                TaskCategory::Feature => feature.push(item),
                TaskCategory::Other => other.push(item),
            }
        }

        Ok(ClassifiedTasks {
            ops,
            daily,
            feature,
            other,
        })
    }

    /// 单任务详情（OpenAPI v1）。404 返回 Ok(None)。
    pub async fn get_task(&self, id: &str) -> Result<Option<Value>, String> {
        let token = self.ensure_token().await?;
        let url = self.url(&format!("api.php/v1/tasks/{}", id));
        let resp = self
            .client
            .get(&url)
            .header("Content-Type", "application/json")
            .header("User-Agent", UA)
            .header("Token", &token)
            .send()
            .await
            .map_err(|e| format!("读任务失败: {}", e))?;
        if resp.status().as_u16() == 404 {
            return Ok(None);
        }
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!(
                "读任务 HTTP {}: {}",
                status.as_u16(),
                crate::util::truncate_chars(&text, 300)
            ));
        }
        let json: Value = serde_json::from_str(&text).map_err(|_| {
            format!(
                "读任务返回非 JSON: {}",
                crate::util::truncate_chars(&text, 200)
            )
        })?;
        // 兼容两种形态：{task: {...}} 或顶层就是 task
        Ok(Some(json.get("task").cloned().unwrap_or(json)))
    }

    /// 写工时。完整配方见 feedback_zentao_workhour_recipe 记忆。
    pub async fn add_effort(
        &self,
        task_id: &str,
        hours: f64,
        work: &str,
        date: Option<&str>,
    ) -> Result<EffortResult, String> {
        self.ensure_token().await?;
        self.login_via_form().await?;

        // 1. 读当前任务拿 left（防误关）+ consumed（用于事后验证）
        let task = self
            .get_task(task_id)
            .await?
            .ok_or_else(|| format!("任务 #{} 不存在", task_id))?;
        let current_left = task
            .get("left")
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .ok_or_else(|| {
                format!(
                    "任务 #{} 未返回 left（剩余工时）字段，已中止写入：缺字段时回填 left=0 会触发禅道把任务自动标记为完成。请在禅道确认该任务后重试。",
                    task_id
                )
            })?;
        let consumed_before = task
            .get("consumed")
            .and_then(|v| {
                v.as_f64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .unwrap_or(0.0);

        // 2. 构造 form body —— 字面方括号 + 3 行占位（与浏览器表单 1:1 对齐）
        let date_str = date
            .map(String::from)
            .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
        let mut parts: Vec<String> = vec![
            format!("date[1]={}", urlencoding::encode(&date_str)),
            format!("work[1]={}", urlencoding::encode(work)),
            format!("consumed[1]={}", hours),
            format!("left[1]={}", current_left),
        ];
        for i in [2, 3] {
            parts.push(format!("date[{}]={}", i, urlencoding::encode(&date_str)));
            parts.push(format!("work[{}]=", i));
            parts.push(format!("consumed[{}]=", i));
            parts.push(format!("left[{}]=", i));
        }
        let form_body = parts.join("&");

        // 3. POST 到 PATH_INFO 端点（必须 cookie 鉴权）
        let endpoint = format!("task-recordWorkhour-{}.json", task_id);
        let url = self.url(&endpoint);
        let referer = self.url(&format!("task-view-{}.html", task_id));
        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("User-Agent", UA)
            // 关键：禅道用这几个判定浏览器 AJAX；缺一项会被当 GET 表单页处理（不写库）
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Accept", "application/json, text/javascript, */*; q=0.01")
            .header("Referer", &referer)
            .body(form_body)
            .send()
            .await
            .map_err(|e| format!("禅道写工时失败: {}", e))?;
        let status = resp.status();
        let resp_text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!(
                "禅道写工时失败 HTTP {}: {} (端点 {})",
                status.as_u16(),
                crate::util::truncate_chars(&resp_text, 300),
                endpoint,
            ));
        }
        let data: Value = serde_json::from_str(&resp_text).map_err(|_| {
            format!(
                "禅道写工时失败：返回非 JSON（session 可能过期）。前 300 字: {}",
                crate::util::truncate_chars(&resp_text, 300)
            )
        })?;
        if data.get("result").and_then(|v| v.as_str()) == Some("fail") {
            let msg = data
                .get("message")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| {
                    let s = data.to_string();
                    crate::util::truncate_chars(&s, 300)
                });
            return Err(format!("禅道写工时失败: {}", msg));
        }

        // 4. verify-after-write：再读 consumed 比对预期增量
        let verify_task = self
            .get_task(task_id)
            .await
            .map_err(|e| format!("写入响应正常但验证读取失败: {}", e))?
            .ok_or_else(|| "写入响应正常但验证读取找不到任务".to_string())?;
        let consumed_after = verify_task
            .get("consumed")
            .and_then(|v| {
                v.as_f64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .unwrap_or(consumed_before);
        let actual_delta = consumed_after - consumed_before;
        if (actual_delta - hours).abs() > 0.001 {
            return Err(format!(
                "禅道服务端返回成功但实际未生效。consumed: {}h → {}h（预期 +{}h，实际 +{}h）。响应: {}",
                consumed_before,
                consumed_after,
                hours,
                actual_delta,
                crate::util::truncate_chars(&resp_text, 200)
            ));
        }

        Ok(EffortResult {
            id: data.get("id").and_then(|v| v.as_u64()),
            endpoint,
            preserved_left: current_left,
            consumed_before,
            consumed_after,
            response_text: resp_text.chars().take(500).collect(),
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffortResult {
    pub id: Option<u64>,
    pub endpoint: String,
    pub preserved_left: f64,
    pub consumed_before: f64,
    pub consumed_after: f64,
    pub response_text: String,
}

// ============================================================================
// 工具函数
// ============================================================================

fn md5_hex(input: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

fn extract_first_digits(s: &str) -> Option<String> {
    let mut start = None;
    let mut end = 0;
    for (i, c) in s.char_indices() {
        if c.is_ascii_digit() {
            if start.is_none() {
                start = Some(i);
            }
            end = i + c.len_utf8();
        } else if start.is_some() {
            break;
        }
    }
    start.map(|i| s[i..end].to_string())
}

// ============================================================================
// 测试连接（给设置 / wizard 用）
// ============================================================================

/// 简化版连接测试：直接打 /api.php/v1/tokens，成功就说能登。
/// 不依赖密钥链，完全从入参取凭证。
pub async fn test_connection(base_url: &str, account: &str, password: &str) -> ZentaoTestResult {
    let mut base = base_url.trim().to_string();
    if base.is_empty() {
        return ZentaoTestResult {
            ok: false,
            message: "禅道地址不能为空".into(),
        };
    }
    if !base.ends_with('/') {
        base.push('/');
    }
    if account.trim().is_empty() {
        return ZentaoTestResult {
            ok: false,
            message: "账号不能为空".into(),
        };
    }

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return ZentaoTestResult {
                ok: false,
                message: format!("HTTP client 构造失败: {}", e),
            }
        }
    };

    let url = format!("{}api.php/v1/tokens", base);
    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("User-Agent", UA)
        .json(&serde_json::json!({ "account": account, "password": password }))
        .send()
        .await;

    match resp {
        Err(e) => ZentaoTestResult {
            ok: false,
            message: format!("请求失败: {}\n实际请求：{}", e, url),
        },
        Ok(r) => {
            let status = r.status();
            let text = r.text().await.unwrap_or_default();
            if !status.is_success() {
                return ZentaoTestResult {
                    ok: false,
                    message: format!(
                        "HTTP {}：{}\n实际请求：{}",
                        status.as_u16(),
                        crate::util::truncate_chars(&text, 300),
                        url
                    ),
                };
            }
            match serde_json::from_str::<Value>(&text) {
                Ok(v) if v.get("token").is_some() => ZentaoTestResult {
                    ok: true,
                    message: "连接成功".into(),
                },
                Ok(v) => ZentaoTestResult {
                    ok: false,
                    message: format!("响应缺 token 字段: {}", v),
                },
                Err(_) => ZentaoTestResult {
                    ok: false,
                    message: format!("响应非 JSON: {}", crate::util::truncate_chars(&text, 200)),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_hex_known_value() {
        assert_eq!(md5_hex("hello"), "5d41402abc4b2a76b9719d911017c592");
        assert_eq!(md5_hex(""), "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn first_digits_from_mixed() {
        assert_eq!(extract_first_digits("12345").as_deref(), Some("12345"));
        assert_eq!(extract_first_digits("abc 789 def").as_deref(), Some("789"));
        assert_eq!(
            extract_first_digits(r#"{"rand":12345}"#).as_deref(),
            Some("12345")
        );
        assert_eq!(extract_first_digits("no digits").as_deref(), None);
    }

    #[test]
    fn classify_task_ops_keywords() {
        assert_eq!(classify_task("需求对接-某某项目"), TaskCategory::Ops);
        assert_eq!(classify_task("数据核对-月报"), TaskCategory::Ops);
        assert_eq!(classify_task("问题反馈-线上故障"), TaskCategory::Ops);
    }

    #[test]
    fn classify_task_feature_keyword() {
        assert_eq!(classify_task("XZGN-用户管理"), TaskCategory::Feature);
        assert_eq!(classify_task("【XZGN】新增导出功能"), TaskCategory::Feature);
    }

    #[test]
    fn classify_task_daily_keywords() {
        assert_eq!(classify_task("公共会议-周会"), TaskCategory::Daily);
        assert_eq!(classify_task("培训-新员工入职"), TaskCategory::Daily);
    }

    #[test]
    fn classify_task_other_fallback() {
        assert_eq!(classify_task("随便什么任务"), TaskCategory::Other);
        assert_eq!(classify_task("临时工作"), TaskCategory::Other);
    }

    #[test]
    fn classify_task_ops_beats_feature_when_both_match() {
        // Ops checked first, so "需求对接 XZGN 系统" should be Ops
        assert_eq!(classify_task("需求对接 XZGN 系统"), TaskCategory::Ops);
    }
}
