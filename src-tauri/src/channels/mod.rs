pub mod qqbot;
pub mod router;
pub mod telegram;
pub mod types;

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tokio::sync::watch;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChannelServiceStatus {
    pub running: bool,
    pub message: String,
}

pub struct ChannelServiceState {
    pub status: Arc<Mutex<ChannelServiceStatus>>,
    stop_tx: Mutex<Option<watch::Sender<bool>>>,
}

impl Default for ChannelServiceState {
    fn default() -> Self {
        Self {
            status: Arc::new(Mutex::new(ChannelServiceStatus {
                running: false,
                message: "未启动".to_string(),
            })),
            stop_tx: Mutex::new(None),
        }
    }
}

impl ChannelServiceState {
    fn set_status(&self, running: bool, message: impl Into<String>) {
        if let Ok(mut status) = self.status.lock() {
            status.running = running;
            status.message = message.into();
        }
    }
}

#[tauri::command]
pub async fn channels_start(app: tauri::AppHandle) -> Result<ChannelServiceStatus, String> {
    let state = app.state::<ChannelServiceState>();
    {
        let mut slot = state
            .stop_tx
            .lock()
            .map_err(|e| format!("渠道服务锁定失败: {}", e))?;
        if slot.is_some() {
            return channel_status(app.clone());
        }
        let cfg = router::load_channels_config();
        let enabled = router::enabled_names(&cfg);
        if enabled.is_empty() {
            state.set_status(false, "没有启用任何聊天渠道");
            return channel_status(app.clone());
        }
        let (tx, rx) = watch::channel(false);
        *slot = Some(tx);
        state.set_status(true, format!("渠道服务运行中: {}", enabled.join(", ")));

        let status = state.status.clone();
        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
            router::run_gateway(app_handle, status.clone(), rx).await;
            if let Ok(mut status) = status.lock() {
                status.running = false;
                if status.message.trim().is_empty() || status.message == "渠道服务启动中" {
                    status.message = "渠道服务已停止".to_string();
                }
            }
        });
    }
    channel_status(app)
}

#[tauri::command]
pub fn channels_stop(app: tauri::AppHandle) -> Result<ChannelServiceStatus, String> {
    let state = app.state::<ChannelServiceState>();
    let tx = {
        let mut slot = state
            .stop_tx
            .lock()
            .map_err(|e| format!("渠道服务锁定失败: {}", e))?;
        slot.take()
    };
    if let Some(tx) = tx {
        let _ = tx.send(true);
        state.set_status(false, "正在停止渠道服务");
    } else {
        state.set_status(false, "渠道服务未启动");
    }
    channel_status(app)
}

#[tauri::command]
pub fn channel_status(app: tauri::AppHandle) -> Result<ChannelServiceStatus, String> {
    let state = app.state::<ChannelServiceState>();
    state
        .status
        .lock()
        .map(|s| s.clone())
        .map_err(|e| format!("渠道服务状态读取失败: {}", e))
}

#[tauri::command]
pub async fn telegram_probe(
    bot_token: String,
    api_base_url: Option<String>,
    proxy: Option<String>,
) -> Result<telegram::TelegramProbeResult, String> {
    telegram::probe(bot_token, api_base_url, proxy).await
}

#[tauri::command]
pub async fn qqbot_probe(
    app_id: String,
    app_secret: String,
    sandbox: bool,
) -> Result<qqbot::QqBotProbeResult, String> {
    qqbot::probe(app_id, app_secret, sandbox).await
}
