use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct Update {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<Message>,
    #[serde(default)]
    pub callback_query: Option<CallbackQuery>,
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub id: String,
    #[serde(default)]
    pub from: Option<User>,
    #[serde(default)]
    pub message: Option<Message>,
    #[serde(default)]
    pub data: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    pub message_id: i64,
    #[serde(default)]
    pub chat: Option<Chat>,
    #[serde(default)]
    pub from: Option<User>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub web_app_data: Option<WebAppData>,
}

#[derive(Debug, Deserialize)]
pub struct WebAppData {
    pub data: String,
    pub button_text: String,
}

#[derive(Debug, Deserialize)]
pub struct Chat {
    pub id: i64,
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub id: i64,
    #[serde(default)]
    pub username: Option<String>,
}

pub async fn telegram_webhook(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(update): Json<Update>,
) -> impl IntoResponse {
    if let Some(expected) = std::env::var("WEBHOOK_SECRET_TOKEN").ok().filter(|s| !s.is_empty()) {
        let actual = headers
            .get("x-telegram-bot-api-secret-token")
            .and_then(|v| v.to_str().ok());
        if actual != Some(expected.as_str()) {
            return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
        }
    }

    let update_id = update.update_id;
    let message_text = update
        .message
        .as_ref()
        .and_then(|m| m.text.as_deref())
        .unwrap_or_default();
    tracing::info!(update_id, message_text = %message_text, "telegram webhook update");

    // Telegram expects a fast response. If we want to reply, do it via
    // "webhook reply" (return a JSON method call) to avoid extra outbound HTTP deps.
    if let Some(resp) = webhook_reply(&st, update) {
        return Json(resp).into_response();
    }

    (StatusCode::OK, "ok").into_response()
}

pub fn spawn_set_webhook_on_startup() {
    let bot_token = std::env::var("TELEGRAM_BOT_TOKEN").ok().filter(|s| !s.is_empty());
    let hook_url = std::env::var("HOOK_URL").ok().filter(|s| !s.is_empty());
    if bot_token.is_none() || hook_url.is_none() {
        return;
    }

    let bot_token = bot_token.unwrap();
    let hook_url = hook_url.unwrap();
    let secret_token = std::env::var("WEBHOOK_SECRET_TOKEN").ok().filter(|s| !s.is_empty());

    tokio::spawn(async move {
        if !hook_url.starts_with("https://") {
            tracing::warn!(hook_url = %hook_url, "HOOK_URL must start with https:// for Telegram webhooks");
            return;
        }

        let endpoint = format!("https://api.telegram.org/bot{bot_token}/setWebhook");

        let mut cmd = Command::new("curl");
        cmd.arg("--silent")
            .arg("--show-error")
            .arg("--request")
            .arg("POST")
            .arg("--data-urlencode")
            .arg(format!("url={hook_url}"));

        if let Some(secret) = secret_token {
            cmd.arg("--data-urlencode")
                .arg(format!("secret_token={secret}"));
        }

        cmd.arg(endpoint);

        let output = match cmd.output().await {
            Ok(o) => o,
            Err(err) => {
                tracing::warn!(?err, "failed to run curl to set telegram webhook (is curl installed?)");
                return;
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(status = ?output.status.code(), stderr = %stderr, "curl failed setting telegram webhook");
            return;
        }

        #[derive(Deserialize)]
        struct TelegramResponse {
            ok: bool,
            #[serde(default)]
            description: Option<String>,
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let resp: TelegramResponse = match serde_json::from_str(&stdout) {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!(?err, body = %stdout, "failed to parse Telegram setWebhook response");
                return;
            }
        };

        if !resp.ok {
            tracing::warn!(description = ?resp.description, "Telegram setWebhook returned ok=false");
            return;
        }

        let desc = resp.description.unwrap_or_default();
        if desc.to_lowercase().contains("already") {
            tracing::info!(description = %desc, "telegram webhook already set");
        } else {
            tracing::info!(description = %desc, "telegram webhook set");
        }
    });
}

fn webhook_reply(_st: &AppState, update: Update) -> Option<WebhookMethod> {
    if let Some(cb) = update.callback_query {
        return Some(WebhookMethod::answer_callback_query(cb.id));
    }

    let msg = update.message?;

    if let Some(web_app_data) = msg.web_app_data {
        tracing::info!(
            message_id = msg.message_id,
            data = %web_app_data.data,
            button_text = %web_app_data.button_text,
            "telegram web_app_data"
        );
    }

    let chat_id = msg.chat.as_ref()?.id;
    let text = msg.text.unwrap_or_default();
    if !text.starts_with("/start") {
        return None;
    }

    let miniapp_url = std::env::var("MINIAPP_URL").ok().filter(|s| !s.is_empty());
    let (reply_text, reply_markup) = match miniapp_url {
        Some(url) => (
            "Welcome to Happi. Tap below to open the mini app.".to_string(),
            Some(ReplyMarkup::web_app_button("Open Happi", &url)),
        ),
        None => (
            "Welcome to Happi. MINIAPP_URL is not configured on the server.".to_string(),
            None,
        ),
    };

    Some(WebhookMethod::send_message(chat_id, reply_text, reply_markup))
}

#[derive(Debug, Serialize)]
struct WebhookMethod {
    method: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    chat_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_markup: Option<ReplyMarkup>,
    #[serde(skip_serializing_if = "Option::is_none")]
    callback_query_id: Option<String>,
}

impl WebhookMethod {
    fn send_message(chat_id: i64, text: String, reply_markup: Option<ReplyMarkup>) -> Self {
        Self {
            method: "sendMessage",
            chat_id: Some(chat_id),
            text: Some(text),
            reply_markup,
            callback_query_id: None,
        }
    }

    fn answer_callback_query(callback_query_id: String) -> Self {
        Self {
            method: "answerCallbackQuery",
            chat_id: None,
            text: None,
            reply_markup: None,
            callback_query_id: Some(callback_query_id),
        }
    }
}

#[derive(Debug, Serialize)]
struct ReplyMarkup {
    inline_keyboard: Vec<Vec<InlineKeyboardButton>>,
}

impl ReplyMarkup {
    fn web_app_button(text: &str, url: &str) -> Self {
        Self {
            inline_keyboard: vec![vec![InlineKeyboardButton {
                text: text.to_string(),
                web_app: Some(WebAppInfo {
                    url: url.to_string(),
                }),
                ..Default::default()
            }]],
        }
    }
}

#[derive(Debug, Serialize, Default)]
struct InlineKeyboardButton {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    web_app: Option<WebAppInfo>,
}

#[derive(Debug, Serialize)]
struct WebAppInfo {
    url: String,
}
