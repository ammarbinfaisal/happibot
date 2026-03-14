use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
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
    if let Some(expected) = std::env::var("WEBHOOK_SECRET_TOKEN")
        .ok()
        .filter(|s| !s.is_empty())
    {
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

        // Bot commands
        if let Some(text) = msg.text.as_deref() {
            let cmd = text.split_whitespace().next().unwrap_or("");
            match cmd {
                "/start" => {
                    let resp = handle_start();
                    return Json(resp.with_chat_id(chat_id)).into_response();
                }
                "/app" => {
                    let resp = handle_app_command();
                    return Json(resp.with_chat_id(chat_id)).into_response();
                }
                "/goals" => {
                    let db = st.db.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_goals_command(db, chat_id, user_id).await {
                            tracing::error!(chat_id, ?e, "failed /goals");
                            let _ = send_telegram_message(chat_id, "Something went wrong.").await;
                        }
                    });
                    return (StatusCode::OK, "ok").into_response();
                }
                "/checkin" => {
                    let resp = handle_checkin_command();
                    return Json(resp.with_chat_id(chat_id)).into_response();
                }
                _ => {} // fall through to handle_user_message
            }
        }

        // Process text or voice in background so we reply fast to Telegram
        let db = st.db.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_user_message(db, chat_id, user_id, msg).await {
                tracing::error!(chat_id, ?e, "failed to handle user message");
                let _ =
                    send_telegram_message(chat_id, "Sorry, something went wrong. Try again?").await;
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
    let result =
        sqlx::query("INSERT INTO users (user_id) VALUES (?) ON CONFLICT(user_id) DO NOTHING")
            .bind(user_id)
            .execute(&db)
            .await?;

    // New user: set up default reminders
    if result.rows_affected() > 0 {
        if let Err(e) = setup_default_reminders(&db, user_id).await {
            tracing::error!(user_id, ?e, "failed to set up default reminders");
        }
    }

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

    // Load recent chat history for direct context
    let history_window = crate::config::chat_history_window();
    let history: Vec<(String, String)> = sqlx::query_as(
        "SELECT role, content FROM chat_history WHERE user_id = ? ORDER BY id DESC LIMIT ?",
    )
    .bind(user_id)
    .bind(history_window)
    .fetch_all(&db)
    .await?
    .into_iter()
    .rev()
    .collect();

    // Load active goals with richer context for the LLM
    let goals: Vec<(String, Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT title, why, cadence, deadline FROM goals WHERE user_id = ? AND status = 'active' ORDER BY updated_at DESC",
    )
    .bind(user_id)
    .fetch_all(&db)
    .await?;
    let goal_titles: Vec<String> = goals
        .iter()
        .map(|(title, why, cadence, deadline)| {
            let mut s = title.clone();
            if let Some(w) = why {
                s.push_str(&format!(" (why: {w})"));
            }
            if let Some(c) = cadence {
                s.push_str(&format!(" [{c}]"));
            }
            if let Some(d) = deadline {
                s.push_str(&format!(" due:{d}"));
            }
            s
        })
        .collect();

    // Load memory context: observations + semantic search
    let observations = crate::memory::load_active_observations(&db, user_id)
        .await
        .unwrap_or_default();

    // Embed the user message and search for relevant past context
    let retrieved_context = match openai::embed(&[&user_text]).await {
        Ok(embs) if !embs.is_empty() => {
            let top_k = crate::config::semantic_search_top_k();
            let results = crate::memory::search_similar(
                &db,
                user_id,
                &embs[0],
                &["chat", "observation"],
                top_k,
            )
            .await
            .unwrap_or_default();
            crate::memory::load_chat_content(&db, &results)
                .await
                .unwrap_or_default()
        }
        _ => Vec::new(),
    };

    // Parse intent with full memory context
    let intent = openai::parse_intent_with_memory(
        &user_text,
        &history,
        &goal_titles,
        &observations,
        &retrieved_context,
    )
    .await?;
    tracing::info!(chat_id, ?intent, "parsed intent");

    let reply = match intent {
        openai::ParsedIntent::Mood {
            happiness,
            energy,
            stress,
            note,
        } => execute_mood(&db, user_id, happiness, energy, stress, note.as_deref()).await?,
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

    // Store conversation in history (returns the user message row ID)
    let user_msg_id = save_chat_message(&db, user_id, "user", &user_text).await?;
    save_chat_message(&db, user_id, "assistant", &reply).await?;

    send_telegram_message(chat_id, &reply).await?;

    // Async post-message pipeline: embed chat turn + generate observations
    let db_clone = db.clone();
    let reply_clone = reply.clone();
    let user_text_clone = user_text.clone();
    let goal_titles_clone = goal_titles.clone();
    tokio::spawn(async move {
        crate::memory::post_message_pipeline(
            &db_clone,
            user_id,
            user_msg_id,
            &user_text_clone,
            &reply_clone,
            &goal_titles_clone,
        )
        .await;
    });

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
        why.map(|w| format!("\nWhy: {w}\n"))
            .unwrap_or_else(|| "\n".to_string())
    ))
}

// ── Default reminders for new users ──

pub async fn setup_default_reminders(db: &SqlitePool, user_id: i64) -> anyhow::Result<()> {
    // Check if user already has reminders
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM reminders WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(db)
        .await?;

    if count.0 > 0 {
        return Ok(()); // already set up
    }

    let now = Utc::now();

    // Daily mood check-in at 9am UTC (user can adjust timezone later)
    // cron crate uses 7-field format: sec min hour day month weekday year
    let checkin_id = Uuid::new_v4().to_string();
    let checkin_next = crate::scheduler::compute_next_run("cron", "0 0 9 * * * *", now);

    sqlx::query(
        r#"INSERT INTO reminders (id, user_id, type, schedule_kind, schedule, payload_json, next_run_at, enabled)
           VALUES (?, ?, 'daily_checkin', 'cron', '0 0 9 * * * *', '{}', ?, 1)"#,
    )
    .bind(&checkin_id)
    .bind(user_id)
    .bind(checkin_next.map(|t| t.to_rfc3339()))
    .execute(db)
    .await?;

    // Evening goal nudge at 7pm UTC
    let nudge_id = Uuid::new_v4().to_string();
    let nudge_next = crate::scheduler::compute_next_run("cron", "0 0 19 * * * *", now);

    sqlx::query(
        r#"INSERT INTO reminders (id, user_id, type, schedule_kind, schedule, payload_json, next_run_at, enabled)
           VALUES (?, ?, 'goal_update', 'cron', '0 0 19 * * * *', '{}', ?, 1)"#,
    )
    .bind(&nudge_id)
    .bind(user_id)
    .bind(nudge_next.map(|t| t.to_rfc3339()))
    .execute(db)
    .await?;

    // Weekly review on Sunday at 6pm UTC
    let review_id = Uuid::new_v4().to_string();
    let review_next = crate::scheduler::compute_next_run("cron", "0 0 18 * * SUN *", now);

    sqlx::query(
        r#"INSERT INTO reminders (id, user_id, type, schedule_kind, schedule, payload_json, next_run_at, enabled)
           VALUES (?, ?, 'weekly_review', 'cron', '0 0 18 * * SUN *', '{}', ?, 1)"#,
    )
    .bind(&review_id)
    .bind(user_id)
    .bind(review_next.map(|t| t.to_rfc3339()))
    .execute(db)
    .await?;

    tracing::info!(user_id, "set up default reminders");
    Ok(())
}

// ── Chat history ──

async fn save_chat_message(
    db: &SqlitePool,
    user_id: i64,
    role: &str,
    content: &str,
) -> anyhow::Result<i64> {
    let result = sqlx::query("INSERT INTO chat_history (user_id, role, content) VALUES (?, ?, ?)")
        .bind(user_id)
        .bind(role)
        .bind(content)
        .execute(db)
        .await?;

    // No longer cap at 50 — all messages are kept for semantic search via embeddings

    Ok(result.last_insert_rowid())
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

// ── Command handlers ──

fn handle_app_command() -> WebhookReply {
    let miniapp_url = std::env::var("MINIAPP_URL").ok().filter(|s| !s.is_empty());
    match miniapp_url {
        Some(url) => WebhookReply {
            text: "✨ Tap below to open Happi!".to_string(),
            reply_markup: Some(ReplyMarkup::web_app_button("Open Happi", &url)),
        },
        None => WebhookReply {
            text: "The mini app isn't configured yet.".to_string(),
            reply_markup: None,
        },
    }
}

fn handle_checkin_command() -> WebhookReply {
    WebhookReply {
        text: "💭 How are you feeling right now?\n\nJust tell me in your own words — or send a voice message. I'll log your mood (happiness, energy, stress).".to_string(),
        reply_markup: None,
    }
}

async fn handle_goals_command(db: SqlitePool, chat_id: i64, user_id: i64) -> anyhow::Result<()> {
    let goals: Vec<(String, String)> = sqlx::query_as(
        "SELECT title, status FROM goals WHERE user_id = ? AND status = 'active' ORDER BY updated_at DESC",
    )
    .bind(user_id)
    .fetch_all(&db)
    .await?;

    let text = if goals.is_empty() {
        "You don't have any active goals yet.\n\n🎯 Tell me a goal you'd like to work on, or tap below to add one in the app.".to_string()
    } else {
        let list: Vec<String> = goals
            .iter()
            .enumerate()
            .map(|(i, (title, _))| format!("{}. {}", i + 1, title))
            .collect();
        format!(
            "🎯 Your active goals:\n\n{}\n\nTell me about your progress, or tap below to see details.",
            list.join("\n")
        )
    };

    let miniapp_url = std::env::var("MINIAPP_URL").ok().filter(|s| !s.is_empty());
    let reply_markup =
        miniapp_url.map(|url| ReplyMarkup::web_app_button("View Goals", &format!("{url}goals")));

    // Send with optional button
    let bot_token = std::env::var("TELEGRAM_BOT_TOKEN")?;
    let mut body = serde_json::json!({ "chat_id": chat_id, "text": text });
    if let Some(markup) = reply_markup {
        body["reply_markup"] = serde_json::to_value(markup)?;
    }
    reqwest::Client::new()
        .post(format!(
            "https://api.telegram.org/bot{bot_token}/sendMessage"
        ))
        .json(&body)
        .send()
        .await?;

    Ok(())
}

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
    let bot_token = std::env::var("TELEGRAM_BOT_TOKEN")
        .ok()
        .filter(|s| !s.is_empty());
    let hook_url = std::env::var("HOOK_URL").ok().filter(|s| !s.is_empty());
    if bot_token.is_none() || hook_url.is_none() {
        return;
    }

    let bot_token = bot_token.unwrap();
    let hook_url = hook_url.unwrap();
    let secret_token = std::env::var("WEBHOOK_SECRET_TOKEN")
        .ok()
        .filter(|s| !s.is_empty());

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

        // Register bot commands
        let commands = serde_json::json!({
            "commands": [
                {"command": "start", "description": "Welcome & intro"},
                {"command": "app", "description": "Open the Happi mini app"},
                {"command": "goals", "description": "View your active goals"},
                {"command": "checkin", "description": "Quick mood check-in"},
            ]
        });
        let cmd_resp = reqwest::Client::new()
            .post(format!(
                "https://api.telegram.org/bot{bot_token}/setMyCommands"
            ))
            .json(&commands)
            .send()
            .await;
        match cmd_resp {
            Ok(r) if r.status().is_success() => tracing::info!("bot commands registered"),
            Ok(r) => tracing::warn!(status = %r.status(), "setMyCommands failed"),
            Err(e) => tracing::warn!(?e, "setMyCommands request failed"),
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
