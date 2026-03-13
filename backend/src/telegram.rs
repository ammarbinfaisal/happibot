use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::{openai, state::AppState};

// ── Telegram types ──

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
    pub voice: Option<Voice>,
    #[serde(default)]
    pub web_app_data: Option<WebAppData>,
}

#[derive(Debug, Deserialize)]
pub struct Voice {
    pub file_id: String,
    #[serde(default)]
    pub duration: Option<i64>,
    #[serde(default)]
    pub file_size: Option<i64>,
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

// ── Webhook handler ──

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

    // Handle callback queries immediately
    if let Some(cb) = update.callback_query {
        return Json(WebhookMethod::answer_callback_query(cb.id)).into_response();
    }

    if let Some(msg) = update.message {
        // Log web_app_data if present
        if let Some(web_app_data) = &msg.web_app_data {
            tracing::info!(
                message_id = msg.message_id,
                data = %web_app_data.data,
                button_text = %web_app_data.button_text,
                "telegram web_app_data"
            );
        }

        let chat_id = match msg.chat.as_ref() {
            Some(c) => c.id,
            None => return (StatusCode::OK, "ok").into_response(),
        };
        let user_id = msg.from.as_ref().map(|u| u.id).unwrap_or(chat_id);

        // /start command
        if msg.text.as_deref().map(|t| t.starts_with("/start")).unwrap_or(false) {
            let resp = handle_start();
            return Json(resp.with_chat_id(chat_id)).into_response();
        }

        // Process text or voice in background so we reply fast to Telegram
        let db = st.db.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_user_message(db, chat_id, user_id, msg).await {
                tracing::error!(chat_id, ?e, "failed to handle user message");
                let _ = send_telegram_message(chat_id, "Sorry, something went wrong. Try again?").await;
            }
        });
    }

    (StatusCode::OK, "ok").into_response()
}

// ── Message handling ──

async fn handle_user_message(
    db: SqlitePool,
    chat_id: i64,
    user_id: i64,
    msg: Message,
) -> anyhow::Result<()> {
    // Ensure user exists
    sqlx::query("INSERT INTO users (user_id) VALUES (?) ON CONFLICT(user_id) DO NOTHING")
        .bind(user_id)
        .execute(&db)
        .await?;

    // Get the text: either from text field or transcribe voice
    let user_text = if let Some(voice) = msg.voice {
        // Send typing indicator
        let _ = send_telegram_action(chat_id, "typing").await;

        let transcript = transcribe_voice(&voice.file_id).await?;
        tracing::info!(chat_id, transcript = %transcript, "voice transcribed");

        // Let user know what we heard
        send_telegram_message(chat_id, &format!("I heard: \"{transcript}\"")).await?;
        transcript
    } else if let Some(text) = msg.text {
        text
    } else {
        return Ok(());
    };

    if user_text.trim().is_empty() {
        return Ok(());
    }

    // Send typing indicator
    let _ = send_telegram_action(chat_id, "typing").await;

    // Load recent chat history (last 10 messages for context)
    let history: Vec<(String, String)> = sqlx::query_as(
        "SELECT role, content FROM chat_history WHERE user_id = ? ORDER BY id DESC LIMIT 10",
    )
    .bind(user_id)
    .fetch_all(&db)
    .await?
    .into_iter()
    .rev()
    .collect();

    // Load active goals for context
    let goals: Vec<(String,)> =
        sqlx::query_as("SELECT title FROM goals WHERE user_id = ? AND status = 'active'")
            .bind(user_id)
            .fetch_all(&db)
            .await?;
    let goal_titles: Vec<String> = goals.into_iter().map(|(t,)| t).collect();

    // Parse intent via LLM
    let intent = openai::parse_intent(&user_text, &history, &goal_titles).await?;
    tracing::info!(chat_id, ?intent, "parsed intent");

    let reply = match intent {
        openai::ParsedIntent::Mood {
            happiness,
            energy,
            stress,
            note,
        } => {
            execute_mood(&db, user_id, happiness, energy, stress, note.as_deref()).await?
        }
        openai::ParsedIntent::Progress {
            goal_title,
            value,
            note,
        } => execute_progress(&db, user_id, &goal_title, value, note.as_deref()).await?,
        openai::ParsedIntent::CreateGoal { title, why } => {
            execute_create_goal(&db, user_id, &title, why.as_deref()).await?
        }
        openai::ParsedIntent::Chat { reply } => reply,
    };

    // Store conversation in history
    save_chat_message(&db, user_id, "user", &user_text).await?;
    save_chat_message(&db, user_id, "assistant", &reply).await?;

    send_telegram_message(chat_id, &reply).await?;
    Ok(())
}

// ── Intent executors ──

async fn execute_mood(
    db: &SqlitePool,
    user_id: i64,
    happiness: i64,
    energy: i64,
    stress: i64,
    note: Option<&str>,
) -> anyhow::Result<String> {
    let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
    let id = Uuid::new_v4().to_string();

    sqlx::query(
        r#"
        INSERT INTO mood_logs (id, user_id, date, happiness, energy, stress, note)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id, date) DO UPDATE SET
          happiness = excluded.happiness,
          energy = excluded.energy,
          stress = excluded.stress,
          note = excluded.note
        "#,
    )
    .bind(&id)
    .bind(user_id)
    .bind(&today)
    .bind(happiness.clamp(1, 10))
    .bind(energy.clamp(1, 10))
    .bind(stress.clamp(1, 10))
    .bind(note)
    .execute(db)
    .await?;

    let emoji = if happiness >= 7 {
        "🌟"
    } else if happiness >= 4 {
        "👍"
    } else {
        "💙"
    };

    Ok(format!(
        "{emoji} Mood logged! Happiness: {}/10, Energy: {}/10, Stress: {}/10.{}",
        happiness.clamp(1, 10),
        energy.clamp(1, 10),
        stress.clamp(1, 10),
        note.map(|n| format!("\nNote: {n}")).unwrap_or_default()
    ))
}

async fn execute_progress(
    db: &SqlitePool,
    user_id: i64,
    goal_title: &str,
    value: Option<f64>,
    note: Option<&str>,
) -> anyhow::Result<String> {
    // Find the best matching active goal
    let goal: Option<(String, String)> = sqlx::query_as(
        "SELECT id, title FROM goals WHERE user_id = ? AND status = 'active' ORDER BY updated_at DESC",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?
    .into_iter()
    .find(|(_, title): &(String, String)| {
        title.to_lowercase().contains(&goal_title.to_lowercase())
            || goal_title.to_lowercase().contains(&title.to_lowercase())
    });

    let (goal_id, goal_title) = match goal {
        Some(g) => g,
        None => {
            // List active goals for the user
            let goals: Vec<(String,)> =
                sqlx::query_as("SELECT title FROM goals WHERE user_id = ? AND status = 'active'")
                    .bind(user_id)
                    .fetch_all(db)
                    .await?;

            if goals.is_empty() {
                return Ok(
                    "You don't have any active goals yet. Tell me a goal you'd like to work on!"
                        .to_string(),
                );
            }

            let list = goals
                .iter()
                .enumerate()
                .map(|(i, (t,))| format!("{}. {t}", i + 1))
                .collect::<Vec<_>>()
                .join("\n");

            return Ok(format!(
                "I couldn't match \"{goal_title}\" to a goal. Your active goals:\n{list}\n\nTry again with the goal name?"
            ));
        }
    };

    let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
    let id = Uuid::new_v4().to_string();

    sqlx::query(
        "INSERT INTO progress_logs (id, user_id, goal_id, date, value, note) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(user_id)
    .bind(&goal_id)
    .bind(&today)
    .bind(value)
    .bind(note)
    .execute(db)
    .await?;

    Ok(format!(
        "✅ Progress logged for \"{goal_title}\"!{}{}",
        value.map(|v| format!(" Value: {v}")).unwrap_or_default(),
        note.map(|n| format!("\nNote: {n}")).unwrap_or_default()
    ))
}

async fn execute_create_goal(
    db: &SqlitePool,
    user_id: i64,
    title: &str,
    why: Option<&str>,
) -> anyhow::Result<String> {
    let id = Uuid::new_v4().to_string();

    sqlx::query(
        r#"
        INSERT INTO goals (id, user_id, title, why, tags_json)
        VALUES (?, ?, ?, ?, '[]')
        "#,
    )
    .bind(&id)
    .bind(user_id)
    .bind(title)
    .bind(why)
    .execute(db)
    .await?;

    Ok(format!(
        "🎯 Goal created: \"{title}\"{}You can log progress anytime by telling me about it!",
        why.map(|w| format!("\nWhy: {w}\n")).unwrap_or_else(|| "\n".to_string())
    ))
}

// ── Chat history ──

async fn save_chat_message(
    db: &SqlitePool,
    user_id: i64,
    role: &str,
    content: &str,
) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO chat_history (user_id, role, content) VALUES (?, ?, ?)")
        .bind(user_id)
        .bind(role)
        .bind(content)
        .execute(db)
        .await?;

    // Keep only last 50 messages per user to avoid unbounded growth
    sqlx::query(
        "DELETE FROM chat_history WHERE user_id = ? AND id NOT IN (SELECT id FROM chat_history WHERE user_id = ? ORDER BY id DESC LIMIT 50)",
    )
    .bind(user_id)
    .bind(user_id)
    .execute(db)
    .await?;

    Ok(())
}

// ── Voice transcription ──

async fn transcribe_voice(file_id: &str) -> anyhow::Result<String> {
    let bot_token = std::env::var("TELEGRAM_BOT_TOKEN")?;

    // Get file path from Telegram
    #[derive(Deserialize)]
    struct GetFileResponse {
        ok: bool,
        result: Option<TgFile>,
    }
    #[derive(Deserialize)]
    struct TgFile {
        file_path: Option<String>,
    }

    let resp: GetFileResponse = reqwest::Client::new()
        .get(format!(
            "https://api.telegram.org/bot{bot_token}/getFile?file_id={file_id}"
        ))
        .send()
        .await?
        .json()
        .await?;

    let file_path = resp
        .result
        .and_then(|f| f.file_path)
        .ok_or_else(|| anyhow::anyhow!("Telegram getFile returned no file_path"))?;

    // Download the file
    let file_bytes = reqwest::Client::new()
        .get(format!(
            "https://api.telegram.org/file/bot{bot_token}/{file_path}"
        ))
        .send()
        .await?
        .bytes()
        .await?
        .to_vec();

    // Transcribe with Whisper
    let filename = file_path.rsplit('/').next().unwrap_or("voice.ogg");
    openai::transcribe(file_bytes, filename).await
}

// ── Telegram API helpers ──

pub async fn send_telegram_message(chat_id: i64, text: &str) -> anyhow::Result<()> {
    let bot_token = std::env::var("TELEGRAM_BOT_TOKEN")?;

    let resp = reqwest::Client::new()
        .post(format!(
            "https://api.telegram.org/bot{bot_token}/sendMessage"
        ))
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "text": text,
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        tracing::warn!(chat_id, body = %body, "sendMessage failed");
    }

    Ok(())
}

async fn send_telegram_action(chat_id: i64, action: &str) -> anyhow::Result<()> {
    let bot_token = std::env::var("TELEGRAM_BOT_TOKEN")?;

    reqwest::Client::new()
        .post(format!(
            "https://api.telegram.org/bot{bot_token}/sendChatAction"
        ))
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "action": action,
        }))
        .send()
        .await?;

    Ok(())
}

// ── /start handler ──

fn handle_start() -> WebhookReply {
    let miniapp_url = std::env::var("MINIAPP_URL").ok().filter(|s| !s.is_empty());
    let (text, reply_markup) = match miniapp_url {
        Some(url) => (
            "Welcome to Happi! 🌟\n\nI'm your wellbeing & goals coach. You can:\n\
             • Send me a text or voice message about how you feel → I'll log your mood\n\
             • Tell me about progress on a goal → I'll track it\n\
             • Ask me to create a new goal\n\
             • Just chat — I'm here to help!\n\n\
             Tap below to open the full app."
                .to_string(),
            Some(ReplyMarkup::web_app_button("Open Happi", &url)),
        ),
        None => (
            "Welcome to Happi! 🌟\n\nI'm your wellbeing & goals coach. You can:\n\
             • Send me a text or voice message about how you feel → I'll log your mood\n\
             • Tell me about progress on a goal → I'll track it\n\
             • Ask me to create a new goal\n\
             • Just chat — I'm here to help!"
                .to_string(),
            None,
        ),
    };

    WebhookReply { text, reply_markup }
}

struct WebhookReply {
    text: String,
    reply_markup: Option<ReplyMarkup>,
}

impl WebhookReply {
    fn with_chat_id(self, chat_id: i64) -> WebhookMethod {
        WebhookMethod::send_message(chat_id, self.text, self.reply_markup)
    }
}

// ── Webhook reply types ──

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

        let mut params = vec![("url", hook_url)];
        if let Some(secret) = secret_token {
            params.push(("secret_token", secret));
        }

        let resp = match reqwest::Client::new()
            .post(&endpoint)
            .form(&params)
            .send()
            .await
        {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(?err, "failed to set telegram webhook");
                return;
            }
        };

        #[derive(Deserialize)]
        struct TelegramResponse {
            ok: bool,
            #[serde(default)]
            description: Option<String>,
        }

        let body = match resp.text().await {
            Ok(b) => b,
            Err(err) => {
                tracing::warn!(?err, "failed to read telegram webhook response");
                return;
            }
        };

        let parsed: TelegramResponse = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!(?err, body = %body, "failed to parse Telegram setWebhook response");
                return;
            }
        };

        if !parsed.ok {
            tracing::warn!(description = ?parsed.description, "Telegram setWebhook returned ok=false");
            return;
        }

        let desc = parsed.description.unwrap_or_default();
        if desc.to_lowercase().contains("already") {
            tracing::info!(description = %desc, "telegram webhook already set");
        } else {
            tracing::info!(description = %desc, "telegram webhook set");
        }
    });
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
