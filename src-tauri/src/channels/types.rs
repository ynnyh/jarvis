use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChannelsConfig {
    #[serde(default)]
    pub auto_start: bool,
    #[serde(default)]
    pub telegram: TelegramConfig,
    #[serde(default)]
    pub qqbot: QqBotConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TelegramConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token: String,
    #[serde(default)]
    pub api_base_url: String,
    #[serde(default)]
    pub proxy: String,
    #[serde(default)]
    pub allow_chat_ids: Vec<String>,
    #[serde(default)]
    pub notify_chat_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QqBotConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub app_secret: String,
    #[serde(default)]
    pub sandbox: bool,
    #[serde(default)]
    pub allow_user_ids: Vec<String>,
    #[serde(default)]
    pub allow_group_ids: Vec<String>,
    #[serde(default)]
    pub notify_user_ids: Vec<String>,
    #[serde(default)]
    pub notify_group_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMessage {
    pub channel: String,
    pub chat_id: String,
    pub sender_id: String,
    pub sender_name: Option<String>,
    pub text: String,
    pub timestamp: i64,
    #[serde(default)]
    pub raw: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingAction {
    pub id: String,
    pub channel: String,
    pub chat_id: String,
    pub kind: String,
    pub payload: serde_json::Value,
    pub summary: String,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct AgentReply {
    pub text: String,
}
