use serde::{Deserialize, Serialize};

use super::client::{FineReportClient, now_unix, save_cached_auth};
use super::credentials::{keyring_entry, get_fine_report_credentials};
use super::html_parser::{DEFAULT_VIEWLET, EffortRecord, parse_detail_html};
use crate::settings::jarvis_dir;

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
    eprintln!(
        "[FineReport] === get_efforts begin={} end={} realName_set={} ===",
        begin,
        end,
        real_name.is_some()
    );

    let cred = get_fine_report_credentials();
    eprintln!(
        "[FineReport] cred: baseUrl_set={} account_set={} realName_set={} pwd={}",
        !cred.base_url.is_empty(),
        !cred.account.is_empty(),
        !cred.real_name.is_empty(),
        if cred.password.is_empty() {
            "<空>"
        } else {
            "<已读>"
        }
    );

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
    eprintln!(
        "[FineReport] step1 ensure_valid_auth ok ({}ms) exp={}",
        t.elapsed().as_millis(),
        auth.expires_at
    );

    let t = Instant::now();
    let session_id = client
        .open_report_and_get_session(&auth.jwt, DEFAULT_VIEWLET)
        .await
        .map_err(|e| {
            eprintln!(
                "[FineReport] step2 open_report_and_get_session FAILED ({}ms): {}",
                t.elapsed().as_millis(),
                e
            );
            e
        })?;
    eprintln!(
        "[FineReport] step2 sessionID ok ({}ms)",
        t.elapsed().as_millis()
    );

    let cid = FineReportClient::generate_cid(&session_id);
    eprintln!("[FineReport] step2.5 cid generated: {}", cid);

    let t = Instant::now();
    client
        .submit_filter(&auth.jwt, &session_id, &begin, &end, &effective_real_name)
        .await
        .map_err(|e| {
            eprintln!(
                "[FineReport] step3 submit_filter FAILED ({}ms): {}",
                t.elapsed().as_millis(),
                e
            );
            e
        })?;
    eprintln!(
        "[FineReport] step3 submit_filter ok ({}ms)",
        t.elapsed().as_millis()
    );

    let t = Instant::now();
    let summary_html = client
        .fetch_report_html(&auth.jwt, &session_id, &cid, 0)
        .await
        .map_err(|e| {
            eprintln!(
                "[FineReport] step4a fetch summary FAILED ({}ms): {}",
                t.elapsed().as_millis(),
                e
            );
            e
        })?;
    eprintln!(
        "[FineReport] step4a summary ok ({}ms) len={}",
        t.elapsed().as_millis(),
        summary_html.len()
    );
    if cfg!(debug_assertions) {
        let summary_path = jarvis_dir().join("finereport-summary.html");
        let _ = std::fs::write(&summary_path, &summary_html);
    }

    let t = Instant::now();
    let detail_html = client
        .fetch_report_html(&auth.jwt, &session_id, &cid, 1)
        .await
        .map_err(|e| {
            eprintln!(
                "[FineReport] step4b fetch detail FAILED ({}ms): {}",
                t.elapsed().as_millis(),
                e
            );
            e
        })?;
    eprintln!(
        "[FineReport] step4b detail ok ({}ms) len={}",
        t.elapsed().as_millis(),
        detail_html.len()
    );
    if cfg!(debug_assertions) {
        let detail_path = jarvis_dir().join("finereport-detail.html");
        let _ = std::fs::write(&detail_path, &detail_html);
    }

    let records = parse_detail_html(&detail_html).unwrap_or_else(|e| {
        eprintln!("[FineReport] parse_detail_html FAILED: {}", e);
        Vec::new()
    });
    eprintln!("[FineReport] parsed {} records", records.len());

    eprintln!(
        "[FineReport] === get_efforts DONE total {}ms ===",
        total.elapsed().as_millis()
    );
    Ok(EffortFetchResult {
        cid,
        session_id,
        records,
        summary_html,
        detail_html,
    })
}

/// 内部调用：拉工时明细（仅返回 records）。
/// `all_people = true` 时不传 realName 过滤，拉全部门数据（成本分析用）。
pub async fn finereport_get_efforts_raw(
    begin: String,
    end: String,
    real_name: Option<String>,
    all_people: bool,
) -> Result<Vec<EffortRecord>, String> {
    let cred = get_fine_report_credentials();
    // all_people 模式：不传 realName → 帆软返回全部门数据
    let effective_real_name = if all_people {
        String::new()
    } else {
        real_name.unwrap_or(cred.real_name)
    };
    let client = FineReportClient::new(cred.base_url, cred.account, cred.password)?;
    let auth = client.ensure_valid_auth().await?;
    let session_id = client
        .open_report_and_get_session(&auth.jwt, DEFAULT_VIEWLET)
        .await?;
    let cid = FineReportClient::generate_cid(&session_id);
    client
        .submit_filter(&auth.jwt, &session_id, &begin, &end, &effective_real_name)
        .await?;
    client
        .fetch_report_html(&auth.jwt, &session_id, &cid, 1)
        .await
        .and_then(|html| parse_detail_html(&html))
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
        return Ok(FineReportTestResult {
            ok: false,
            message: "帆软地址不能为空".into(),
        });
    }
    if req.account.trim().is_empty() {
        return Ok(FineReportTestResult {
            ok: false,
            message: "账号不能为空".into(),
        });
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
        Err(e) => {
            return Ok(FineReportTestResult {
                ok: false,
                message: e,
            })
        }
    };

    match client.login().await {
        Ok(auth) => {
            // 测试成功顺手把 JWT 缓存起来，下次调用免登录
            let _ = save_cached_auth(&auth);
            let now = now_unix();
            let secs_left = auth.expires_at - now;
            let days_left = secs_left / 86400;
            let hours_left = (secs_left % 86400) / 3600;
            Ok(FineReportTestResult {
                ok: true,
                message: format!(
                    "登录成功。exp={}, now={}, 剩 {} 天 {} 小时",
                    auth.expires_at,
                    now,
                    days_left.max(0),
                    hours_left.max(0)
                ),
            })
        }
        Err(e) => Ok(FineReportTestResult {
            ok: false,
            message: e,
        }),
    }
}
