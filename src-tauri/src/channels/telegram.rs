use crate::channels::router::OutboundMessage;
use crate::channels::types::{ChannelMessage, TelegramConfig};
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{mpsc, watch};

const DEFAULT_API_BASE: &str = "https://api.telegram.org";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramProbeResult {
    pub ok: bool,
    pub bot_username: Option<String>,
    pub bot_name: Option<String>,
    pub recent_chats: Vec<TelegramRecentChat>,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramRecentChat {
    pub chat_id: String,
    pub chat_type: Option<String>,
    pub title: Option<String>,
    pub from_id: Option<String>,
    pub from_name: Option<String>,
    pub text: Option<String>,
}

pub async fn probe(
    bot_token: String,
    api_base_url: Option<String>,
    proxy: Option<String>,
) -> Result<TelegramProbeResult, String> {
    if bot_token.trim().is_empty() {
        return Ok(TelegramProbeResult {
            ok: false,
            bot_username: None,
            bot_name: None,
            recent_chats: vec![],
            message: "请先填写 Telegram botToken。".to_string(),
        });
    }

    let api_base = normalize_api_base(api_base_url.as_deref().unwrap_or(DEFAULT_API_BASE));
    let client = build_client(proxy.as_deref())?;
    let me_url = format!("{}/bot{}/getMe", api_base, bot_token);
    let me_resp = client
        .get(&me_url)
        .send()
        .await
        .map_err(|e| format_telegram_request_error("getMe", &me_url, &e))?;
    let status = me_resp.status();
    let me_text = me_resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Ok(TelegramProbeResult {
            ok: false,
            bot_username: None,
            bot_name: None,
            recent_chats: vec![],
            message: format!(
                "Telegram 拒绝 token: HTTP {}。如果你在国内网络，请确认代理/API 地址是否可用。",
                status.as_u16()
            ),
        });
    }
    let me: TelegramGetMe =
        serde_json::from_str(&me_text).map_err(|e| format!("Telegram getMe 解析失败: {}", e))?;
    if !me.ok {
        return Ok(TelegramProbeResult {
            ok: false,
            bot_username: None,
            bot_name: None,
            recent_chats: vec![],
            message: "Telegram token 无效。".to_string(),
        });
    }

    let updates_url = format!("{}/bot{}/getUpdates", api_base, bot_token);
    let updates_resp = client
        .post(updates_url)
        .json(&serde_json::json!({ "limit": 10, "allowed_updates": ["message"] }))
        .send()
        .await;
    let updates = match updates_resp {
        Ok(resp) => resp.json::<TelegramUpdates>().await.ok(),
        Err(_) => None,
    }
    .map(|u| {
            u.result
                .into_iter()
                .filter_map(|update| update.message)
                .filter_map(|msg| {
                    let chat_id = msg.chat.id.to_string();
                    Some(TelegramRecentChat {
                        chat_id,
                        chat_type: msg.chat.kind,
                        title: msg.chat.title.or(msg.chat.username),
                        from_id: msg.from.as_ref().map(|f| f.id.to_string()),
                        from_name: msg.from.as_ref().and_then(|f| {
                            f.username
                                .clone()
                                .or_else(|| f.first_name.clone())
                                .or_else(|| f.last_name.clone())
                        }),
                        text: msg.text,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let message = if updates.is_empty() {
        "Token 有效。现在去 Telegram 给这个 bot 发一句“今天有哪些任务？”，再回来点一次检查，就能看到 chat id。".to_string()
    } else {
        "Token 有效。已读到最近会话；需要收紧访问时，把 chat id 加到白名单。".to_string()
    };

    Ok(TelegramProbeResult {
        ok: true,
        bot_username: me.result.username,
        bot_name: me.result.first_name,
        recent_chats: updates,
        message,
    })
}

pub fn spawn(
    config: TelegramConfig,
    inbound: mpsc::Sender<ChannelMessage>,
    outbound: mpsc::Receiver<OutboundMessage>,
    stop_rx: watch::Receiver<bool>,
) {
    let send_config = config.clone();
    let send_stop_rx = stop_rx.clone();
    tauri::async_runtime::spawn(async move {
        receive_loop(config, inbound, stop_rx).await;
    });
    tauri::async_runtime::spawn(async move {
        send_loop(send_config, outbound, send_stop_rx).await;
    });
}

async fn receive_loop(
    config: TelegramConfig,
    inbound: mpsc::Sender<ChannelMessage>,
    mut stop_rx: watch::Receiver<bool>,
) {
    if config.bot_token.trim().is_empty() {
        eprintln!("[channels/telegram] botToken 为空，跳过 Telegram 接收");
        return;
    }
    let api_base = normalize_api_base(if config.api_base_url.trim().is_empty() {
        DEFAULT_API_BASE
    } else {
        config.api_base_url.trim()
    });
    let client = match build_client(Some(config.proxy.trim())) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[channels/telegram] client 构造失败: {}", e);
            return;
        }
    };
    let mut offset: i64 = 0;
    loop {
        if *stop_rx.borrow() {
            break;
        }
        let url = format!("{}/bot{}/getUpdates", api_base, config.bot_token);
        let req = client
            .post(&url)
            .json(&serde_json::json!({
                "offset": offset,
                "timeout": 25,
                "allowed_updates": ["message"],
            }))
            .send();
        let resp = tokio::select! {
            _ = stop_rx.changed() => break,
            resp = req => resp,
        };

        let Ok(resp) = resp else {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            continue;
        };
        let parsed = match resp.json::<TelegramUpdates>().await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[channels/telegram] getUpdates 解析失败: {}", e);
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                continue;
            }
        };
        if !parsed.ok {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            continue;
        }
        for update in parsed.result {
            // 先记下"这条之后"的 offset，但只有在这条被妥善处理（成功投递，或
            // 确属可跳过的非文本 / 白名单外消息）之后才推进。避免投递失败时 offset
            // 已跨过这条 → 该消息被永久跳过丢失。
            let next_offset = update.update_id + 1;
            if let Some(msg) = update.message {
                if let Some(text) = msg.text.clone() {
                    let chat_id = msg.chat.id.to_string();
                    if is_allowed(&config.allow_chat_ids, &chat_id) {
                        let sender_id = msg
                            .from
                            .as_ref()
                            .map(|f| f.id.to_string())
                            .unwrap_or_else(|| chat_id.clone());
                        let sender_name = msg.from.as_ref().and_then(|f| {
                            f.username
                                .clone()
                                .or_else(|| f.first_name.clone())
                                .or_else(|| f.last_name.clone())
                        });
                        let incoming = ChannelMessage {
                            channel: "telegram".to_string(),
                            chat_id,
                            sender_id,
                            sender_name,
                            text,
                            timestamp: msg.date.unwrap_or_else(|| chrono::Utc::now().timestamp()),
                            raw: msg.raw,
                        };
                        if inbound.send(incoming).await.is_err() {
                            // 接收端已关闭（dispatcher 退出）：不推进 offset，这条留待
                            // 下次轮询 / 重启后重新拉取，不丢；已处理过的因 offset 已推进不会重复。
                            return;
                        }
                    }
                }
            }
            offset = next_offset;
        }
    }
}

async fn send_loop(
    config: TelegramConfig,
    mut outbound: mpsc::Receiver<OutboundMessage>,
    mut stop_rx: watch::Receiver<bool>,
) {
    if config.bot_token.trim().is_empty() {
        return;
    }
    let api_base = normalize_api_base(if config.api_base_url.trim().is_empty() {
        DEFAULT_API_BASE
    } else {
        config.api_base_url.trim()
    });
    let client = match build_client(Some(config.proxy.trim())) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[channels/telegram] client 构造失败: {}", e);
            return;
        }
    };
    loop {
        let msg = tokio::select! {
            _ = stop_rx.changed() => break,
            msg = outbound.recv() => msg,
        };
        let Some(msg) = msg else { break };
        let url = format!("{}/bot{}/sendMessage", api_base, config.bot_token);
        let chunks = split_message(&msg.text, 3500);
        for chunk in chunks {
            send_telegram_chunk(&client, &url, &msg.chat_id, &chunk).await;
        }
    }
}

/// 发送单条消息，对 429 / 5xx / 网络错误重试最多 3 次；失败时记日志而非静默吞。
async fn send_telegram_chunk(client: &reqwest::Client, url: &str, chat_id: &str, text: &str) {
    let body = serde_json::json!({
        "chat_id": chat_id,
        "text": text,
        "disable_web_page_preview": true,
    });
    for attempt in 1..=3u32 {
        match client.post(url).json(&body).send().await {
            Ok(resp) if resp.status().is_success() => return,
            Ok(resp) => {
                let code = resp.status().as_u16();
                if !(code == 429 || code >= 500) || attempt == 3 {
                    eprintln!(
                        "[channels/telegram] sendMessage 失败 HTTP {}（第 {} 次，放弃）",
                        code, attempt
                    );
                    return;
                }
            }
            Err(e) => {
                if attempt == 3 {
                    eprintln!(
                        "[channels/telegram] sendMessage 网络失败（第 {} 次，放弃）: {}",
                        attempt, e
                    );
                    return;
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
    }
}

fn normalize_api_base(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

fn build_client(proxy: Option<&str>) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(35));
    if let Some(proxy) = choose_proxy(proxy) {
        let proxy = reqwest::Proxy::all(proxy)
            .map_err(|e| format!("Telegram 代理配置无效: {}", e))?;
        builder = builder.proxy(proxy);
    }
    builder
        .build()
        .map_err(|e| format!("Telegram HTTP client 构造失败: {}", e))
}

fn choose_proxy(proxy: Option<&str>) -> Option<String> {
    if let Some(proxy) = proxy.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(proxy.to_string());
    }
    for key in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim().to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn format_telegram_request_error(op: &str, url: &str, err: &reqwest::Error) -> String {
    let mut hints = Vec::new();
    if err.is_timeout() {
        hints.push("请求超时");
    }
    if err.is_connect() {
        hints.push("连接失败，通常是网络或代理没有生效");
    }
    let proxy_hint = choose_proxy(None)
        .map(|p| format!("当前检测到环境代理: {}", p))
        .unwrap_or_else(|| "未检测到环境代理；如果在国内网络，请在 Telegram 的“代理”里填 http://127.0.0.1:7890 或你的实际端口".to_string());
    format!(
        "Telegram {} 请求失败: {}\nURL: {}\n{}\n支持 http/https/socks5 代理，例如 socks5://127.0.0.1:7890",
        op,
        err,
        redact_telegram_url(url),
        if hints.is_empty() { proxy_hint } else { format!("{}；{}", hints.join("，"), proxy_hint) }
    )
}

fn redact_telegram_url(url: &str) -> String {
    match regex::Regex::new(r"/bot[^/]+/") {
        Ok(re) => re.replace(url, "/bot<redacted>/").to_string(),
        Err(_) => url.to_string(),
    }
}

fn is_allowed(allow: &[String], chat_id: &str) -> bool {
    allow.is_empty() || allow.iter().any(|id| id.trim() == chat_id)
}

fn split_message(text: &str, max_chars: usize) -> Vec<String> {
    if text.chars().count() <= max_chars {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if cur.chars().count() >= max_chars {
            out.push(cur);
            cur = String::new();
        }
        cur.push(ch);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[derive(Debug, Deserialize)]
struct TelegramUpdates {
    ok: bool,
    result: Vec<TelegramUpdate>,
}

#[derive(Debug, Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    message: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessage {
    chat: TelegramChat,
    from: Option<TelegramUser>,
    text: Option<String>,
    date: Option<i64>,
    #[serde(flatten)]
    raw: Value,
}

#[derive(Debug, Deserialize)]
struct TelegramChat {
    id: i64,
    #[serde(rename = "type")]
    kind: Option<String>,
    title: Option<String>,
    username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramUser {
    id: i64,
    username: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramGetMe {
    ok: bool,
    result: TelegramMe,
}

#[derive(Debug, Deserialize)]
struct TelegramMe {
    username: Option<String>,
    first_name: Option<String>,
}
