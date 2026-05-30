use crate::channels::router::OutboundMessage;
use crate::channels::types::{ChannelMessage, QqBotConfig};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::pin::Pin;
use tokio::sync::{mpsc, watch};
use tokio::time::{Duration, Instant, Sleep};
use tokio_tungstenite::tungstenite::Message;

const PROD_BASE: &str = "https://api.sgroup.qq.com";
const SANDBOX_BASE: &str = "https://sandbox.api.sgroup.qq.com";
const TOKEN_URL: &str = "https://bots.qq.com/app/getAppAccessToken";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QqBotProbeResult {
    pub ok: bool,
    pub token_ok: bool,
    pub gateway_ok: bool,
    pub gateway_url: Option<String>,
    pub message: String,
}

pub async fn probe(
    app_id: String,
    app_secret: String,
    sandbox: bool,
) -> Result<QqBotProbeResult, String> {
    let config = QqBotConfig {
        enabled: true,
        app_id,
        app_secret,
        sandbox,
        allow_user_ids: vec![],
        allow_group_ids: vec![],
        notify_user_ids: vec![],
        notify_group_ids: vec![],
    };
    if config.app_id.trim().is_empty() || config.app_secret.trim().is_empty() {
        return Ok(QqBotProbeResult {
            ok: false,
            token_ok: false,
            gateway_ok: false,
            gateway_url: None,
            message: "请先填写 QQ Bot AppID 和 AppSecret。".to_string(),
        });
    }

    let token = match app_access_token(&config).await {
        Ok(token) => token,
        Err(e) => {
            return Ok(QqBotProbeResult {
                ok: false,
                token_ok: false,
                gateway_ok: false,
                gateway_url: None,
                message: format!("AppID/AppSecret 未通过：{}", e),
            });
        }
    };
    match gateway_url(&config, &token).await {
        Ok(url) => Ok(QqBotProbeResult {
            ok: true,
            token_ok: true,
            gateway_ok: true,
            gateway_url: Some(url),
            message: "QQ 凭据有效，网关可访问。下一步：点击“启动 QQ”，然后在 QQ 里给机器人发消息；群聊一般需要 @ 机器人。".to_string(),
        }),
        Err(e) => Ok(QqBotProbeResult {
            ok: false,
            token_ok: true,
            gateway_ok: false,
            gateway_url: None,
            message: format!("AccessToken 已获取，但网关不可用：{}", e),
        }),
    }
}

pub fn spawn(
    config: QqBotConfig,
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

fn base_url(config: &QqBotConfig) -> &'static str {
    if config.sandbox {
        SANDBOX_BASE
    } else {
        PROD_BASE
    }
}

/// QQ Bot HTTP client：带 20s 超时，避免 token / 网关 / 发送请求在弱网下永久挂起阻塞 send_loop。
fn build_qq_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

async fn receive_loop(
    config: QqBotConfig,
    inbound: mpsc::Sender<ChannelMessage>,
    mut stop_rx: watch::Receiver<bool>,
) {
    if config.app_id.trim().is_empty() || config.app_secret.trim().is_empty() {
        eprintln!("[channels/qqbot] appId/appSecret 为空，跳过 QQ Bot 接收");
        return;
    }

    loop {
        if *stop_rx.borrow() {
            break;
        }
        if let Err(e) = run_ws_once(&config, inbound.clone(), stop_rx.clone()).await {
            eprintln!("[channels/qqbot] websocket 断开: {}", e);
        }
        tokio::select! {
            _ = stop_rx.changed() => break,
            _ = tokio::time::sleep(Duration::from_secs(5)) => {}
        }
    }
}

async fn run_ws_once(
    config: &QqBotConfig,
    inbound: mpsc::Sender<ChannelMessage>,
    mut stop_rx: watch::Receiver<bool>,
) -> Result<(), String> {
    let token = app_access_token(config).await?;
    let gateway = gateway_url(config, &token).await?;
    let (mut ws, _) = tokio_tungstenite::connect_async(&gateway)
        .await
        .map_err(|e| format!("连接 QQ Bot 网关失败: {}", e))?;

    let mut seq: Option<u64> = None;
    let mut heartbeat: Pin<Box<Sleep>> = Box::pin(tokio::time::sleep(Duration::from_secs(3600)));
    let mut heartbeat_ready = false;

    loop {
        tokio::select! {
            _ = stop_rx.changed() => {
                break;
            }
            _ = &mut heartbeat, if heartbeat_ready => {
                let hb = json!({ "op": 1, "d": seq });
                ws.send(Message::Text(hb.to_string()))
                    .await
                    .map_err(|e| format!("QQ Bot 心跳发送失败: {}", e))?;
                heartbeat.as_mut().reset(Instant::now() + Duration::from_secs(45));
            }
            frame = ws.next() => {
                let Some(frame) = frame else { break };
                let frame = frame.map_err(|e| format!("读取 QQ Bot 事件失败: {}", e))?;
                let text = match frame {
                    Message::Text(t) => t,
                    Message::Ping(p) => {
                        let _ = ws.send(Message::Pong(p)).await;
                        continue;
                    }
                    Message::Close(_) => break,
                    _ => continue,
                };
                let payload: GatewayPayload =
                    serde_json::from_str(&text).map_err(|e| format!("QQ Bot 事件 JSON 解析失败: {}", e))?;
                if let Some(s) = payload.s {
                    seq = Some(s);
                }
                match payload.op {
                    10 => {
                        let interval = payload
                            .d
                            .get("heartbeat_interval")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(45_000);
                        identify(&mut ws, &token).await?;
                        heartbeat_ready = true;
                        heartbeat.as_mut().reset(Instant::now() + Duration::from_millis(interval));
                    }
                    0 => {
                        if let Some(msg) = event_to_channel_message(config, &payload) {
                            if inbound.send(msg).await.is_err() {
                                return Ok(());
                            }
                        }
                    }
                    1 => {
                        // 服务端要求立即心跳：立刻回一个，避免被判离线踢连接。
                        let hb = json!({ "op": 1, "d": seq });
                        ws.send(Message::Text(hb.to_string()))
                            .await
                            .map_err(|e| format!("QQ Bot 即时心跳发送失败: {}", e))?;
                    }
                    7 | 9 => break,
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

async fn identify<W>(write: &mut W, token: &str) -> Result<(), String>
where
    W: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let payload = json!({
        "op": 2,
        "d": {
            "token": format!("QQBot {}", token),
            "intents": (1 << 25) | (1 << 30),
            "shard": [0, 1],
            "properties": {
                "$os": std::env::consts::OS,
                "$browser": "jarvis",
                "$device": "jarvis"
            }
        }
    });
    write
        .send(Message::Text(payload.to_string()))
        .await
        .map_err(|e| format!("QQ Bot identify 失败: {}", e))
}

async fn app_access_token(config: &QqBotConfig) -> Result<String, String> {
    let client = build_qq_client();
    let resp = client
        .post(TOKEN_URL)
        .json(&json!({
            "appId": config.app_id,
            "clientSecret": config.app_secret,
        }))
        .send()
        .await
        .map_err(|e| format!("QQ Bot access token 请求失败: {}", e))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "QQ Bot access token HTTP {}: {}",
            status.as_u16(),
            redact_qq_secret_echo(&text, &config.app_secret)
        ));
    }
    let parsed: AppAccessTokenResp =
        serde_json::from_str(&text).map_err(|e| format!("QQ Bot token 解析失败: {}", e))?;
    Ok(parsed.access_token)
}

async fn gateway_url(config: &QqBotConfig, token: &str) -> Result<String, String> {
    let client = build_qq_client();
    let url = format!("{}/gateway", base_url(config));
    let resp = client
        .get(url)
        .header("Authorization", format!("QQBot {}", token))
        .send()
        .await
        .map_err(|e| format!("QQ Bot gateway 请求失败: {}", e))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "QQ Bot gateway HTTP {}: {}",
            status.as_u16(),
            redact_tokenish_text(&text)
        ));
    }
    let parsed: GatewayResp =
        serde_json::from_str(&text).map_err(|e| format!("QQ Bot gateway 解析失败: {}", e))?;
    Ok(parsed.url)
}

fn redact_qq_secret_echo(text: &str, app_secret: &str) -> String {
    let mut out = text.to_string();
    if !app_secret.trim().is_empty() {
        out = out.replace(app_secret.trim(), "<redacted>");
    }
    redact_tokenish_text(&out)
}

fn redact_tokenish_text(text: &str) -> String {
    let mut out = text.to_string();
    for pattern in [
        r#""access_token"\s*:\s*"[^"]+""#,
        r#""clientSecret"\s*:\s*"[^"]+""#,
        r#""token"\s*:\s*"[^"]+""#,
    ] {
        if let Ok(re) = regex::Regex::new(pattern) {
            out = re
                .replace_all(&out, |caps: &regex::Captures| {
                    let raw = caps.get(0).map(|m| m.as_str()).unwrap_or("");
                    match raw.split_once(':') {
                        Some((key, _)) => format!("{}:\"<redacted>\"", key),
                        None => "<redacted>".to_string(),
                    }
                })
                .to_string();
        }
    }
    out
}

fn event_to_channel_message(
    config: &QqBotConfig,
    payload: &GatewayPayload,
) -> Option<ChannelMessage> {
    let t = payload.t.as_deref().unwrap_or("");
    let d = &payload.d;
    let text = d
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return None;
    }

    let (chat_id, sender_id) = match t {
        "C2C_MESSAGE_CREATE" => {
            let openid = d
                .get("author")
                .and_then(|a| a.get("user_openid"))
                .and_then(|v| v.as_str())?;
            (format!("c2c:{}", openid), openid.to_string())
        }
        "GROUP_AT_MESSAGE_CREATE" => {
            let group_id = d
                .get("group_openid")
                .or_else(|| d.get("group_id"))
                .and_then(|v| v.as_str())?;
            let member_id = d
                .get("author")
                .and_then(|a| a.get("member_openid").or_else(|| a.get("user_openid")))
                .and_then(|v| v.as_str())
                .unwrap_or(group_id);
            (format!("group:{}", group_id), member_id.to_string())
        }
        _ => return None,
    };

    if chat_id.starts_with("c2c:") && !config.allow_user_ids.is_empty() {
        let id = chat_id.trim_start_matches("c2c:");
        if !config.allow_user_ids.iter().any(|x| x == id) {
            return None;
        }
    }
    if chat_id.starts_with("group:") && !config.allow_group_ids.is_empty() {
        let id = chat_id.trim_start_matches("group:");
        if !config.allow_group_ids.iter().any(|x| x == id) {
            return None;
        }
    }

    Some(ChannelMessage {
        channel: "qqbot".to_string(),
        chat_id,
        sender_id,
        sender_name: None,
        text,
        timestamp: chrono::Utc::now().timestamp(),
        raw: d.clone(),
    })
}

async fn send_loop(
    config: QqBotConfig,
    mut outbound: mpsc::Receiver<OutboundMessage>,
    mut stop_rx: watch::Receiver<bool>,
) {
    if config.app_id.trim().is_empty() || config.app_secret.trim().is_empty() {
        return;
    }
    let client = build_qq_client();
    // access token 有效期约 7200s，缓存复用，避免每条消息都打一次 token 接口触发限流。
    let mut cached_token: Option<(String, Instant)> = None;
    loop {
        let msg = tokio::select! {
            _ = stop_rx.changed() => break,
            msg = outbound.recv() => msg,
        };
        let Some(msg) = msg else { break };
        let token = match &cached_token {
            Some((t, exp)) if *exp > Instant::now() => t.clone(),
            _ => match app_access_token(&config).await {
                Ok(t) => {
                    cached_token = Some((t.clone(), Instant::now() + Duration::from_secs(6000)));
                    t
                }
                Err(e) => {
                    eprintln!("[channels/qqbot] 发送前获取 token 失败: {}", e);
                    continue;
                }
            },
        };
        if let Err(e) = send_message(&client, &config, &token, &msg).await {
            eprintln!("[channels/qqbot] 发送失败: {}", e);
            // 失败可能是 token 过期；清缓存下次强制刷新。
            cached_token = None;
        }
    }
}

async fn send_message(
    client: &reqwest::Client,
    config: &QqBotConfig,
    token: &str,
    msg: &OutboundMessage,
) -> Result<(), String> {
    let endpoint = if let Some(openid) = msg.chat_id.strip_prefix("c2c:") {
        format!("{}/v2/users/{}/messages", base_url(config), openid)
    } else if let Some(openid) = msg.chat_id.strip_prefix("group:") {
        format!("{}/v2/groups/{}/messages", base_url(config), openid)
    } else {
        return Err(format!("未知 QQ Bot chatId: {}", msg.chat_id));
    };
    let resp = client
        .post(endpoint)
        .header("Authorization", format!("QQBot {}", token))
        .json(&json!({
            "content": msg.text,
            "msg_type": 0,
        }))
        .send()
        .await
        .map_err(|e| format!("QQ Bot 发送失败: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("QQ Bot 发送 HTTP {}: {}", status.as_u16(), text));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct AppAccessTokenResp {
    #[serde(rename = "access_token")]
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct GatewayResp {
    url: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct GatewayPayload {
    op: i64,
    #[serde(default)]
    d: Value,
    #[serde(default)]
    s: Option<u64>,
    #[serde(default)]
    t: Option<String>,
}
