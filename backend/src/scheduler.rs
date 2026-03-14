use chrono::{DateTime, NaiveTime, Utc};
use cron::Schedule;
use sqlx::SqlitePool;
use std::str::FromStr;
use std::time::Duration;

use crate::telegram;

#[derive(sqlx::FromRow)]
struct DueReminder {
    id: String,
    user_id: i64,
    r#type: String,
    schedule_kind: String,
    schedule: String,
    payload_json: String,
    quiet_hours_json: Option<String>,
    timezone: String,
}

#[derive(sqlx::FromRow)]
struct GoalTitle {
    title: String,
}

pub fn spawn_reminder_loop(db: SqlitePool) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Err(e) = process_due_reminders(&db).await {
                tracing::error!(?e, "reminder loop error");
            }
        }
    });
}

async fn process_due_reminders(db: &SqlitePool) -> anyhow::Result<()> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();

    let due: Vec<DueReminder> = sqlx::query_as(
        r#"
        SELECT r.id, r.user_id, r.type, r.schedule_kind, r.schedule,
               r.payload_json, r.quiet_hours_json, u.timezone
        FROM reminders r
        JOIN users u ON u.user_id = r.user_id
        WHERE r.enabled = 1
          AND r.next_run_at IS NOT NULL
          AND r.next_run_at <= ?
        "#,
    )
    .bind(&now_str)
    .fetch_all(db)
    .await?;

    if due.is_empty() {
        return Ok(());
    }

    tracing::info!(count = due.len(), "processing due reminders");

    for reminder in due {
        if let Err(e) = process_one_reminder(db, &reminder, now).await {
            tracing::error!(reminder_id = %reminder.id, ?e, "failed to process reminder");
        }
    }

    Ok(())
}

async fn process_one_reminder(
    db: &SqlitePool,
    reminder: &DueReminder,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    // Check quiet hours
    if is_in_quiet_hours(&reminder.quiet_hours_json, &reminder.timezone, now) {
        tracing::debug!(reminder_id = %reminder.id, "skipping: quiet hours");
        return Ok(());
    }

    // Load user's active goals for context
    let goals: Vec<GoalTitle> = sqlx::query_as(
        "SELECT title FROM goals WHERE user_id = ? AND status = 'active' ORDER BY updated_at DESC",
    )
    .bind(reminder.user_id)
    .fetch_all(db)
    .await?;

    let goal_list = if goals.is_empty() {
        String::new()
    } else {
        let items: Vec<String> = goals
            .iter()
            .enumerate()
            .map(|(i, g)| format!("{}. {}", i + 1, g.title))
            .collect();
        format!("\n\nYour active goals:\n{}", items.join("\n"))
    };

    // Build the message based on reminder type
    let message = build_reminder_message(&reminder.r#type, &reminder.payload_json, &goal_list);

    // Send via Telegram (user_id == chat_id for private chats)
    telegram::send_telegram_message(reminder.user_id, &message).await?;

    tracing::info!(
        reminder_id = %reminder.id,
        user_id = reminder.user_id,
        reminder_type = %reminder.r#type,
        "sent reminder"
    );

    // Compute next run and update
    let next_run = compute_next_run(&reminder.schedule_kind, &reminder.schedule, now);

    match next_run {
        Some(next) => {
            sqlx::query(
                r#"
                UPDATE reminders
                SET next_run_at = ?, last_sent_at = ?,
                    updated_at = (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                WHERE id = ?
                "#,
            )
            .bind(next.to_rfc3339())
            .bind(now.to_rfc3339())
            .bind(&reminder.id)
            .execute(db)
            .await?;
        }
        None => {
            // Can't compute next run (e.g. unsupported rrule), disable
            tracing::warn!(
                reminder_id = %reminder.id,
                schedule_kind = %reminder.schedule_kind,
                "cannot compute next_run, disabling reminder"
            );
            sqlx::query(
                r#"
                UPDATE reminders
                SET enabled = 0, last_sent_at = ?,
                    updated_at = (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                WHERE id = ?
                "#,
            )
            .bind(now.to_rfc3339())
            .bind(&reminder.id)
            .execute(db)
            .await?;
        }
    }

    Ok(())
}

fn build_reminder_message(reminder_type: &str, payload_json: &str, goal_list: &str) -> String {
    match reminder_type {
        "daily_checkin" => format!(
            "Hey! How are you feeling today? \
             Send me a quick mood update (happiness, energy, stress 1-10) \
             or just tell me how your day is going.{goal_list}"
        ),
        "weekly_review" => format!(
            "Time for your weekly review! \
             How did your week go? Any wins, challenges, or reflections?{goal_list}"
        ),
        "goal_update" => format!(
            "Quick check-in: how's progress on your goals? \
             Share an update and I'll log it for you.{goal_list}"
        ),
        _ => {
            // Custom: try to extract message from payload
            if let Ok(payload) = serde_json::from_str::<serde_json::Value>(payload_json) {
                if let Some(msg) = payload.get("message").and_then(|v| v.as_str()) {
                    return format!("{msg}{goal_list}");
                }
            }
            format!("Time for your reminder!{goal_list}")
        }
    }
}

pub fn compute_next_run(
    schedule_kind: &str,
    schedule: &str,
    after: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    match schedule_kind {
        "cron" => {
            let sched = Schedule::from_str(schedule).ok()?;
            sched.after(&after).next()
        }
        _ => {
            tracing::warn!(
                schedule_kind,
                "unsupported schedule_kind, cannot compute next_run"
            );
            None
        }
    }
}

fn is_in_quiet_hours(
    quiet_hours_json: &Option<String>,
    timezone: &str,
    now: DateTime<Utc>,
) -> bool {
    let json = match quiet_hours_json {
        Some(j) => j,
        None => return false,
    };

    let val: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let start_str = val.get("start").and_then(|v| v.as_str()).unwrap_or("");
    let end_str = val.get("end").and_then(|v| v.as_str()).unwrap_or("");

    let start = match NaiveTime::parse_from_str(start_str, "%H:%M") {
        Ok(t) => t,
        Err(_) => return false,
    };
    let end = match NaiveTime::parse_from_str(end_str, "%H:%M") {
        Ok(t) => t,
        Err(_) => return false,
    };

    // Convert UTC now to user's local time
    let tz: chrono_tz::Tz = match timezone.parse() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let local_now = now.with_timezone(&tz).time();

    if start <= end {
        // Simple range: e.g., 22:00-06:00 would be start > end
        local_now >= start && local_now < end
    } else {
        // Wraps midnight: e.g., 22:00 to 06:00
        local_now >= start || local_now < end
    }
}
