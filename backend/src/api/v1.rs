use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::{auth, http_error::HttpError, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/profile", get(get_user_profile))
        .route("/goals", get(list_goals).post(create_goal))
        .route("/goals/{goal_id}", post(update_goal))
        .route("/goals/{goal_id}/alignment", post(save_goal_alignment))
        .route("/progress", post(log_progress))
        .route("/progress/history", get(progress_history))
        .route("/mood", post(log_mood))
        .route("/mood/history", get(mood_history))
        .route("/reminders", post(schedule_reminder))
        .route("/checkins/due", get(get_due_checkins))
        .route("/reviews/weekly", get(summarize_week))
        .route("/dashboard", get(get_dashboard))
        .route("/ikigai", get(get_ikigai).post(save_ikigai))
}

fn user_id_from(headers: &HeaderMap) -> Result<i64, HttpError> {
    let token = std::env::var("TELEGRAM_BOT_TOKEN").ok();
    auth::auth_user_id(headers, token.as_deref())
}

#[derive(Serialize)]
struct UserProfile {
    user_id: i64,
    timezone: String,
    reminder_window_start: Option<String>,
    reminder_window_end: Option<String>,
    quiet_hours_start: Option<String>,
    quiet_hours_end: Option<String>,
    onboarding_state: String,
}

#[derive(sqlx::FromRow)]
struct UserProfileRow {
    user_id: i64,
    timezone: String,
    reminder_window_start: Option<String>,
    reminder_window_end: Option<String>,
    quiet_hours_start: Option<String>,
    quiet_hours_end: Option<String>,
    onboarding_state: String,
}

async fn ensure_user(pool: &SqlitePool, user_id: i64) -> Result<(), HttpError> {
    // Insert-if-missing. Keep defaults minimal; user can update timezone later.
    sqlx::query(
        r#"
        INSERT INTO users (user_id)
        VALUES (?)
        ON CONFLICT(user_id) DO NOTHING
        "#,
    )
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;
    Ok(())
}

async fn get_user_profile(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<UserProfile>, HttpError> {
    let user_id = user_id_from(&headers)?;
    ensure_user(&st.db, user_id).await?;

    let row: UserProfileRow = sqlx::query_as(
        r#"
        SELECT user_id, timezone, reminder_window_start, reminder_window_end,
               quiet_hours_start, quiet_hours_end, onboarding_state
        FROM users
        WHERE user_id = ?
        "#,
    )
    .bind(user_id)
    .fetch_one(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    Ok(Json(UserProfile {
        user_id: row.user_id,
        timezone: row.timezone,
        reminder_window_start: row.reminder_window_start,
        reminder_window_end: row.reminder_window_end,
        quiet_hours_start: row.quiet_hours_start,
        quiet_hours_end: row.quiet_hours_end,
        onboarding_state: row.onboarding_state,
    }))
}

#[derive(Deserialize)]
struct ListGoalsQuery {
    status: Option<String>,
    tag: Option<String>,
    horizon: Option<String>,
}

#[derive(Serialize)]
struct Goal {
    id: String,
    title: String,
    why: Option<String>,
    metric: Option<String>,
    target_kind: String,
    target_value: Option<f64>,
    target_text: Option<String>,
    deadline: Option<String>,
    cadence: Option<String>,
    tags: Vec<String>,
    status: String,
}

#[derive(sqlx::FromRow)]
struct GoalRow {
    id: String,
    title: String,
    why: Option<String>,
    metric: Option<String>,
    target_kind: String,
    target_value: Option<f64>,
    target_text: Option<String>,
    deadline: Option<String>,
    cadence: Option<String>,
    tags_json: String,
    status: String,
}

async fn list_goals(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListGoalsQuery>,
) -> Result<Json<Vec<Goal>>, HttpError> {
    let user_id = user_id_from(&headers)?;
    ensure_user(&st.db, user_id).await?;

    let status = q.status.unwrap_or_else(|| "active".to_string());
    let rows: Vec<GoalRow> = sqlx::query_as(
        r#"
        SELECT id, title, why, metric, target_kind, target_value, target_text,
               deadline, cadence, tags_json, status
        FROM goals
        WHERE user_id = ? AND status = ?
        ORDER BY updated_at DESC
        "#,
    )
    .bind(user_id)
    .bind(&status)
    .fetch_all(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    let mut goals = Vec::with_capacity(rows.len());
    for r in rows {
        let mut tags: Vec<String> = serde_json::from_str(&r.tags_json).unwrap_or_default();
        if let Some(tag) = &q.tag {
            if !tags.iter().any(|t| t == tag) {
                continue;
            }
        }
        if let Some(horizon) = &q.horizon {
            if horizon == "overdue" {
                if let Some(deadline) = &r.deadline {
                    if let Ok(d) = NaiveDate::parse_from_str(deadline, "%Y-%m-%d") {
                        if d >= Utc::now().date_naive() {
                            continue;
                        }
                    }
                } else {
                    continue;
                }
            }
        }

        goals.push(Goal {
            id: r.id,
            title: r.title,
            why: r.why,
            metric: r.metric,
            target_kind: r.target_kind,
            target_value: r.target_value,
            target_text: r.target_text,
            deadline: r.deadline,
            cadence: r.cadence,
            tags: {
                tags.sort();
                tags
            },
            status: r.status,
        });
    }

    Ok(Json(goals))
}

#[derive(Deserialize)]
struct CreateGoalBody {
    title: String,
    why: Option<String>,
    metric: Option<String>,
    target_kind: Option<String>,
    target_value: Option<f64>,
    target_text: Option<String>,
    deadline: Option<String>,
    cadence: Option<String>,
    tags: Option<Vec<String>>,
    ikigai_alignment: Option<serde_json::Value>,
}

async fn create_goal(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateGoalBody>,
) -> Result<Json<Goal>, HttpError> {
    let user_id = user_id_from(&headers)?;
    ensure_user(&st.db, user_id).await?;

    if body.title.trim().is_empty() {
        return Err(HttpError::bad_request("title is required"));
    }

    if let Some(deadline) = &body.deadline {
        NaiveDate::parse_from_str(deadline, "%Y-%m-%d")
            .map_err(|_| HttpError::bad_request("deadline must be YYYY-MM-DD"))?;
    }

    let id = Uuid::new_v4().to_string();
    let target_kind = body.target_kind.unwrap_or_else(|| "number".to_string());
    let tags = body.tags.clone().unwrap_or_default();
    let tags_json = serde_json::to_string(&tags)
        .map_err(|_| HttpError::bad_request("bad tags"))?;
    let ikigai_alignment_json = match body.ikigai_alignment {
        Some(v) => Some(v.to_string()),
        None => None,
    };

    sqlx::query(
        r#"
        INSERT INTO goals (
          id, user_id, title, why, metric, target_kind, target_value, target_text,
          deadline, cadence, tags_json, ikigai_alignment_json
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(user_id)
    .bind(body.title.trim())
    .bind(body.why.as_deref())
    .bind(body.metric.as_deref())
    .bind(&target_kind)
    .bind(body.target_value)
    .bind(body.target_text.as_deref())
    .bind(body.deadline.as_deref())
    .bind(body.cadence.as_deref())
    .bind(tags_json)
    .bind(ikigai_alignment_json)
    .execute(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    Ok(Json(Goal {
        id,
        title: body.title.trim().to_string(),
        why: body.why,
        metric: body.metric,
        target_kind,
        target_value: body.target_value,
        target_text: body.target_text,
        deadline: body.deadline,
        cadence: body.cadence,
        tags,
        status: "active".to_string(),
    }))
}

#[derive(Deserialize)]
struct UpdateGoalBody {
    // Minimal patch surface for MVP. Expand as needed.
    title: Option<String>,
    why: Option<Option<String>>,
    metric: Option<Option<String>>,
    target_kind: Option<String>,
    target_value: Option<Option<f64>>,
    target_text: Option<Option<String>>,
    deadline: Option<Option<String>>,
    cadence: Option<Option<String>>,
    tags: Option<Vec<String>>,
    status: Option<String>,
}

async fn update_goal(
    State(st): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(goal_id): axum::extract::Path<String>,
    Json(body): Json<UpdateGoalBody>,
) -> Result<impl IntoResponse, HttpError> {
    let user_id = user_id_from(&headers)?;
    ensure_user(&st.db, user_id).await?;

    if let Some(Some(deadline)) = &body.deadline {
        NaiveDate::parse_from_str(deadline, "%Y-%m-%d")
            .map_err(|_| HttpError::bad_request("deadline must be YYYY-MM-DD"))?;
    }

    let tags_json = match body.tags {
        Some(tags) => Some(
            serde_json::to_string(&tags).map_err(|_| HttpError::bad_request("bad tags"))?,
        ),
        None => None,
    };

    let res = sqlx::query(
        r#"
        UPDATE goals SET
          title = COALESCE(?, title),
          why = CASE WHEN ? IS NULL THEN why ELSE ? END,
          metric = CASE WHEN ? IS NULL THEN metric ELSE ? END,
          target_kind = COALESCE(?, target_kind),
          target_value = CASE WHEN ? IS NULL THEN target_value ELSE ? END,
          target_text = CASE WHEN ? IS NULL THEN target_text ELSE ? END,
          deadline = CASE WHEN ? IS NULL THEN deadline ELSE ? END,
          cadence = CASE WHEN ? IS NULL THEN cadence ELSE ? END,
          tags_json = COALESCE(?, tags_json),
          status = COALESCE(?, status),
          updated_at = (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        WHERE id = ? AND user_id = ?
        "#,
    )
    .bind(body.title.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()))
    // why tri-state (no change vs set null vs set value)
    .bind(body.why.is_some().then_some(1))
    .bind(body.why.as_ref().and_then(|v| v.as_deref()))
    // metric tri-state
    .bind(body.metric.is_some().then_some(1))
    .bind(body.metric.as_ref().and_then(|v| v.as_deref()))
    .bind(body.target_kind.as_deref())
    // target_value tri-state
    .bind(body.target_value.is_some().then_some(1))
    .bind(body.target_value.flatten())
    // target_text tri-state
    .bind(body.target_text.is_some().then_some(1))
    .bind(body.target_text.flatten().as_deref())
    // deadline tri-state
    .bind(body.deadline.is_some().then_some(1))
    .bind(body.deadline.flatten().as_deref())
    // cadence tri-state
    .bind(body.cadence.is_some().then_some(1))
    .bind(body.cadence.flatten().as_deref())
    .bind(tags_json.as_deref())
    .bind(body.status.as_deref())
    .bind(&goal_id)
    .bind(user_id)
    .execute(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    if res.rows_affected() == 0 {
        return Err(HttpError::bad_request("goal not found"));
    }

    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct LogProgressBody {
    goal_id: String,
    date: String, // YYYY-MM-DD
    value: Option<f64>,
    note: Option<String>,
    confidence: Option<i64>,
    idempotency_key: Option<String>,
}

async fn log_progress(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LogProgressBody>,
) -> Result<impl IntoResponse, HttpError> {
    let user_id = user_id_from(&headers)?;
    ensure_user(&st.db, user_id).await?;

    NaiveDate::parse_from_str(&body.date, "%Y-%m-%d")
        .map_err(|_| HttpError::bad_request("date must be YYYY-MM-DD"))?;

    if let Some(c) = body.confidence {
        if !(1..=5).contains(&c) {
            return Err(HttpError::bad_request("confidence must be 1..5"));
        }
    }

    let id = Uuid::new_v4().to_string();
    let res = sqlx::query(
        r#"
        INSERT INTO progress_logs (id, user_id, goal_id, date, value, note, confidence, idempotency_key)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(user_id)
    .bind(&body.goal_id)
    .bind(&body.date)
    .bind(body.value)
    .bind(body.note.as_deref())
    .bind(body.confidence)
    .bind(body.idempotency_key.as_deref())
    .execute(&st.db)
    .await;

    match res {
        Ok(_) => Ok((axum::http::StatusCode::CREATED, id)),
        Err(e) => {
            // idempotency unique index
            if e.to_string().contains("progress_logs_idem_idx") {
                return Ok((axum::http::StatusCode::OK, "duplicate".to_string()));
            }
            Err(HttpError::bad_request("db error"))
        }
    }
}

#[derive(Deserialize)]
struct LogMoodBody {
    date: String, // YYYY-MM-DD
    happiness: i64,
    energy: i64,
    stress: i64,
    note: Option<String>,
    idempotency_key: Option<String>,
}

async fn log_mood(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LogMoodBody>,
) -> Result<impl IntoResponse, HttpError> {
    let user_id = user_id_from(&headers)?;
    ensure_user(&st.db, user_id).await?;

    NaiveDate::parse_from_str(&body.date, "%Y-%m-%d")
        .map_err(|_| HttpError::bad_request("date must be YYYY-MM-DD"))?;

    for (name, v) in [
        ("happiness", body.happiness),
        ("energy", body.energy),
        ("stress", body.stress),
    ] {
        if !(1..=10).contains(&v) {
            return Err(HttpError::bad_request(format!("{name} must be 1..10")));
        }
    }

    let id = Uuid::new_v4().to_string();
    let res = sqlx::query(
        r#"
        INSERT INTO mood_logs (id, user_id, date, happiness, energy, stress, note, idempotency_key)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id, date) DO UPDATE SET
          happiness = excluded.happiness,
          energy = excluded.energy,
          stress = excluded.stress,
          note = excluded.note
        "#,
    )
    .bind(&id)
    .bind(user_id)
    .bind(&body.date)
    .bind(body.happiness)
    .bind(body.energy)
    .bind(body.stress)
    .bind(body.note.as_deref())
    .bind(body.idempotency_key.as_deref())
    .execute(&st.db)
    .await;

    match res {
        Ok(_) => Ok((axum::http::StatusCode::CREATED, id)),
        Err(e) => {
            if e.to_string().contains("mood_logs_idem_idx") {
                return Ok((axum::http::StatusCode::OK, "duplicate".to_string()));
            }
            Err(HttpError::bad_request("db error"))
        }
    }
}

#[derive(Deserialize)]
struct MoodHistoryQuery {
    days: Option<i64>, // default 30
}

#[derive(Serialize, sqlx::FromRow)]
struct MoodPoint {
    date: String,
    happiness: i64,
    energy: i64,
    stress: i64,
    note: Option<String>,
}

async fn mood_history(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<MoodHistoryQuery>,
) -> Result<Json<Vec<MoodPoint>>, HttpError> {
    let user_id = user_id_from(&headers)?;
    let days = q.days.unwrap_or(30).min(365);
    let date_from = (Utc::now().date_naive() - chrono::Duration::days(days))
        .format("%Y-%m-%d")
        .to_string();

    let rows: Vec<MoodPoint> = sqlx::query_as(
        r#"
        SELECT date, happiness, energy, stress, note
        FROM mood_logs
        WHERE user_id = ? AND date >= ?
        ORDER BY date ASC
        "#,
    )
    .bind(user_id)
    .bind(&date_from)
    .fetch_all(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    Ok(Json(rows))
}

#[derive(Deserialize)]
struct ScheduleReminderBody {
    r#type: String,
    schedule_kind: Option<String>,
    schedule: String,
    payload: Option<serde_json::Value>,
    quiet_hours: Option<serde_json::Value>,
    start_date: Option<String>,
    enabled: Option<bool>,
}

async fn schedule_reminder(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ScheduleReminderBody>,
) -> Result<impl IntoResponse, HttpError> {
    let user_id = user_id_from(&headers)?;
    ensure_user(&st.db, user_id).await?;

    if let Some(sd) = &body.start_date {
        NaiveDate::parse_from_str(sd, "%Y-%m-%d")
            .map_err(|_| HttpError::bad_request("start_date must be YYYY-MM-DD"))?;
    }

    let id = Uuid::new_v4().to_string();
    let payload_json = body.payload.unwrap_or_else(|| serde_json::json!({})).to_string();
    let quiet_hours_json = body.quiet_hours.map(|v| v.to_string());
    let schedule_kind = body
        .schedule_kind
        .unwrap_or_else(|| "rrule".to_string());
    let enabled = body.enabled.unwrap_or(true);

    let next_run_at = crate::scheduler::compute_next_run(&schedule_kind, &body.schedule, Utc::now())
        .map(|dt| dt.to_rfc3339());

    sqlx::query(
        r#"
        INSERT INTO reminders (
          id, user_id, type, schedule_kind, schedule,
          payload_json, quiet_hours_json, start_date, next_run_at, enabled
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(user_id)
    .bind(&body.r#type)
    .bind(schedule_kind)
    .bind(&body.schedule)
    .bind(payload_json)
    .bind(quiet_hours_json)
    .bind(body.start_date.as_deref())
    .bind(next_run_at.as_deref())
    .bind(if enabled { 1 } else { 0 })
    .execute(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    Ok((axum::http::StatusCode::CREATED, id))
}

#[derive(Deserialize)]
struct DueCheckinsQuery {
    date_from: String, // YYYY-MM-DD
    date_to: String,   // YYYY-MM-DD
}

#[derive(Serialize)]
struct DueCheckin {
    user_id: i64,
    kind: String,
    question_hint: String,
}

async fn get_due_checkins(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<DueCheckinsQuery>,
) -> Result<Json<Vec<DueCheckin>>, HttpError> {
    // Bot-facing endpoint typically. We still auth as a user for now (dev simplicity).
    // In production you'd auth with a bot secret and list many users.
    let user_id = user_id_from(&headers)?;
    ensure_user(&st.db, user_id).await?;

    NaiveDate::parse_from_str(&q.date_from, "%Y-%m-%d")
        .map_err(|_| HttpError::bad_request("date_from must be YYYY-MM-DD"))?;
    NaiveDate::parse_from_str(&q.date_to, "%Y-%m-%d")
        .map_err(|_| HttpError::bad_request("date_to must be YYYY-MM-DD"))?;

    // MVP: single-user due checkin if no mood log for today.
    let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
    let mood: Option<i64> = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT 1 FROM mood_logs WHERE user_id = ? AND date = ? LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(&today)
    .fetch_optional(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    let mut out = Vec::new();
    if mood.is_none() {
        out.push(DueCheckin {
            user_id,
            kind: "daily_checkin".to_string(),
            question_hint: "Mood 1-10 + one sentence".to_string(),
        });
    }

    Ok(Json(out))
}

#[derive(Deserialize)]
struct WeeklyReviewQuery {
    week: Option<String>, // YYYY-WW (ISO week)
}

#[derive(Serialize)]
struct WeeklyReviewStats {
    user_id: i64,
    week: String,
    mood_days: i64,
    progress_logs: i64,
    active_goals: i64,
}

async fn summarize_week(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<WeeklyReviewQuery>,
) -> Result<Json<WeeklyReviewStats>, HttpError> {
    let user_id = user_id_from(&headers)?;
    ensure_user(&st.db, user_id).await?;

    let now = Utc::now().date_naive();
    let iso = now.iso_week();
    let week = q.week.unwrap_or_else(|| format!("{}-{:02}", iso.year(), iso.week()));

    // MVP: counts in last 7 days.
    let date_from = now - chrono::Duration::days(6);
    let date_from = date_from.format("%Y-%m-%d").to_string();
    let date_to = now.format("%Y-%m-%d").to_string();

    let mood_days: i64 = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM mood_logs
        WHERE user_id = ? AND date BETWEEN ? AND ?
        "#,
    )
    .bind(user_id)
    .bind(&date_from)
    .bind(&date_to)
    .fetch_one(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    let progress_logs: i64 = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM progress_logs
        WHERE user_id = ? AND date BETWEEN ? AND ?
        "#,
    )
    .bind(user_id)
    .bind(&date_from)
    .bind(&date_to)
    .fetch_one(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    let active_goals: i64 = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM goals
        WHERE user_id = ? AND status = 'active'
        "#,
    )
    .bind(user_id)
    .fetch_one(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    Ok(Json(WeeklyReviewStats {
        user_id,
        week,
        mood_days,
        progress_logs,
        active_goals,
    }))
}

// ── Dashboard ──

#[derive(Serialize)]
struct DashboardResponse {
    goals: Vec<GoalWithProgress>,
    mood_trend: Vec<MoodPoint>,
    weekly_stats: WeeklyReviewStats,
    ikigai: Option<IkigaiProfile>,
    goal_alignments: Vec<GoalAlignmentEntry>,
    streak: StreakInfo,
}

#[derive(Serialize)]
struct GoalWithProgress {
    #[serde(flatten)]
    goal: Goal,
    progress_last_7d: Vec<ProgressEntry>,
    total_logs: i64,
    latest_value: Option<f64>,
    completion_pct: Option<f64>,
}

#[derive(Serialize)]
struct ProgressEntry {
    date: String,
    value: Option<f64>,
    note: Option<String>,
    confidence: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
struct ProgressEntryRow {
    date: String,
    value: Option<f64>,
    note: Option<String>,
    confidence: Option<i64>,
}

#[derive(Serialize)]
struct IkigaiProfile {
    mission: Option<String>,
    themes: Vec<String>,
}

#[derive(Serialize)]
struct GoalAlignmentEntry {
    goal_id: String,
    goal_title: String,
    alignment_score: i64,
    quadrants: Vec<String>,
}

#[derive(Serialize)]
struct StreakInfo {
    current_mood_streak: i64,
    current_progress_streak: i64,
}

async fn get_dashboard(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<DashboardResponse>, HttpError> {
    let user_id = user_id_from(&headers)?;
    ensure_user(&st.db, user_id).await?;

    let now = Utc::now().date_naive();
    let seven_days_ago = (now - Duration::days(6)).format("%Y-%m-%d").to_string();
    let thirty_days_ago = (now - Duration::days(30)).format("%Y-%m-%d").to_string();
    let today = now.format("%Y-%m-%d").to_string();

    // Load active goals
    let goal_rows: Vec<GoalRow> = sqlx::query_as(
        r#"
        SELECT id, title, why, metric, target_kind, target_value, target_text,
               deadline, cadence, tags_json, status
        FROM goals
        WHERE user_id = ? AND status = 'active'
        ORDER BY updated_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    let mut goals_with_progress = Vec::with_capacity(goal_rows.len());
    for r in &goal_rows {
        let tags: Vec<String> = serde_json::from_str(&r.tags_json).unwrap_or_default();

        // Progress for last 7 days
        let progress_rows: Vec<ProgressEntryRow> = sqlx::query_as(
            r#"
            SELECT date, value, note, confidence
            FROM progress_logs
            WHERE user_id = ? AND goal_id = ? AND date >= ?
            ORDER BY date ASC
            "#,
        )
        .bind(user_id)
        .bind(&r.id)
        .bind(&seven_days_ago)
        .fetch_all(&st.db)
        .await
        .map_err(|_| HttpError::bad_request("db error"))?;

        let total_logs: i64 = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM progress_logs WHERE user_id = ? AND goal_id = ?",
        )
        .bind(user_id)
        .bind(&r.id)
        .fetch_one(&st.db)
        .await
        .map_err(|_| HttpError::bad_request("db error"))?;

        let latest_value: Option<f64> = sqlx::query_scalar(
            "SELECT value FROM progress_logs WHERE user_id = ? AND goal_id = ? ORDER BY date DESC LIMIT 1",
        )
        .bind(user_id)
        .bind(&r.id)
        .fetch_optional(&st.db)
        .await
        .map_err(|_| HttpError::bad_request("db error"))?
        .flatten();

        let completion_pct = match (latest_value, r.target_value) {
            (Some(v), Some(t)) if t > 0.0 => Some((v / t * 100.0).min(100.0)),
            _ => None,
        };

        let progress_entries: Vec<ProgressEntry> = progress_rows
            .into_iter()
            .map(|p| ProgressEntry {
                date: p.date,
                value: p.value,
                note: p.note,
                confidence: p.confidence,
            })
            .collect();

        goals_with_progress.push(GoalWithProgress {
            goal: Goal {
                id: r.id.clone(),
                title: r.title.clone(),
                why: r.why.clone(),
                metric: r.metric.clone(),
                target_kind: r.target_kind.clone(),
                target_value: r.target_value,
                target_text: r.target_text.clone(),
                deadline: r.deadline.clone(),
                cadence: r.cadence.clone(),
                tags,
                status: r.status.clone(),
            },
            progress_last_7d: progress_entries,
            total_logs,
            latest_value,
            completion_pct,
        });
    }

    // Mood trend (last 30 days)
    let mood_trend: Vec<MoodPoint> = sqlx::query_as(
        r#"
        SELECT date, happiness, energy, stress, note
        FROM mood_logs
        WHERE user_id = ? AND date >= ?
        ORDER BY date ASC
        "#,
    )
    .bind(user_id)
    .bind(&thirty_days_ago)
    .fetch_all(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    // Weekly stats
    let iso = now.iso_week();
    let week_str = format!("{}-{:02}", iso.year(), iso.week());

    let mood_days: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM mood_logs WHERE user_id = ? AND date BETWEEN ? AND ?",
    )
    .bind(user_id)
    .bind(&seven_days_ago)
    .bind(&today)
    .fetch_one(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    let progress_log_count: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM progress_logs WHERE user_id = ? AND date BETWEEN ? AND ?",
    )
    .bind(user_id)
    .bind(&seven_days_ago)
    .bind(&today)
    .fetch_one(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    let active_goals_count = goal_rows.len() as i64;

    // Ikigai profile
    let ikigai = {
        let row: Option<(Option<String>, String)> = sqlx::query_as(
            "SELECT mission, themes_json FROM ikigai_profiles WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_optional(&st.db)
        .await
        .map_err(|_| HttpError::bad_request("db error"))?;

        row.map(|(mission, themes_json)| {
            let themes: Vec<String> = serde_json::from_str(&themes_json).unwrap_or_default();
            IkigaiProfile { mission, themes }
        })
    };

    // Goal alignments
    let alignment_rows: Vec<(String, i64, String, String)> = sqlx::query_as(
        r#"
        SELECT ga.goal_id, ga.alignment_score, ga.quadrants_json, g.title
        FROM goal_alignment ga
        JOIN goals g ON g.id = ga.goal_id
        WHERE ga.user_id = ? AND g.status = 'active'
        "#,
    )
    .bind(user_id)
    .fetch_all(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    let goal_alignments: Vec<GoalAlignmentEntry> = alignment_rows
        .into_iter()
        .map(|(goal_id, score, quadrants_json, title)| {
            let quadrants: Vec<String> = serde_json::from_str(&quadrants_json).unwrap_or_default();
            GoalAlignmentEntry {
                goal_id,
                goal_title: title,
                alignment_score: score,
                quadrants,
            }
        })
        .collect();

    // Streaks
    let mood_streak = compute_streak(
        &st.db,
        user_id,
        "SELECT DISTINCT date FROM mood_logs WHERE user_id = ? ORDER BY date DESC",
        now,
    )
    .await;

    let progress_streak = compute_streak(
        &st.db,
        user_id,
        "SELECT DISTINCT date FROM progress_logs WHERE user_id = ? ORDER BY date DESC",
        now,
    )
    .await;

    Ok(Json(DashboardResponse {
        goals: goals_with_progress,
        mood_trend,
        weekly_stats: WeeklyReviewStats {
            user_id,
            week: week_str,
            mood_days,
            progress_logs: progress_log_count,
            active_goals: active_goals_count,
        },
        ikigai,
        goal_alignments,
        streak: StreakInfo {
            current_mood_streak: mood_streak,
            current_progress_streak: progress_streak,
        },
    }))
}

async fn compute_streak(
    db: &SqlitePool,
    user_id: i64,
    query: &str,
    today: NaiveDate,
) -> i64 {
    let dates: Vec<(String,)> = match sqlx::query_as(query)
        .bind(user_id)
        .fetch_all(db)
        .await
    {
        Ok(d) => d,
        Err(_) => return 0,
    };

    let mut streak = 0i64;
    let mut expected = today;

    for (date_str,) in &dates {
        if let Ok(d) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
            if d == expected {
                streak += 1;
                expected -= Duration::days(1);
            } else if d < expected {
                break;
            }
        }
    }

    streak
}

// ── Progress history ──

#[derive(Deserialize)]
struct ProgressHistoryQuery {
    goal_id: String,
    days: Option<i64>,
}

async fn progress_history(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ProgressHistoryQuery>,
) -> Result<Json<Vec<ProgressEntry>>, HttpError> {
    let user_id = user_id_from(&headers)?;
    let days = q.days.unwrap_or(30).min(365);
    let date_from = (Utc::now().date_naive() - Duration::days(days))
        .format("%Y-%m-%d")
        .to_string();

    let rows: Vec<ProgressEntryRow> = sqlx::query_as(
        r#"
        SELECT date, value, note, confidence
        FROM progress_logs
        WHERE user_id = ? AND goal_id = ? AND date >= ?
        ORDER BY date ASC
        "#,
    )
    .bind(user_id)
    .bind(&q.goal_id)
    .bind(&date_from)
    .fetch_all(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    let entries: Vec<ProgressEntry> = rows
        .into_iter()
        .map(|r| ProgressEntry {
            date: r.date,
            value: r.value,
            note: r.note,
            confidence: r.confidence,
        })
        .collect();

    Ok(Json(entries))
}

// ── Ikigai ──

async fn get_ikigai(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Option<IkigaiProfile>>, HttpError> {
    let user_id = user_id_from(&headers)?;
    ensure_user(&st.db, user_id).await?;

    let row: Option<(Option<String>, String)> = sqlx::query_as(
        "SELECT mission, themes_json FROM ikigai_profiles WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_optional(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    let profile = row.map(|(mission, themes_json)| {
        let themes: Vec<String> = serde_json::from_str(&themes_json).unwrap_or_default();
        IkigaiProfile { mission, themes }
    });

    Ok(Json(profile))
}

#[derive(Deserialize)]
struct SaveIkigaiBody {
    mission: Option<String>,
    themes: Option<Vec<String>>,
}

async fn save_ikigai(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SaveIkigaiBody>,
) -> Result<impl IntoResponse, HttpError> {
    let user_id = user_id_from(&headers)?;
    ensure_user(&st.db, user_id).await?;

    let themes_json =
        serde_json::to_string(&body.themes.unwrap_or_default()).unwrap_or_else(|_| "[]".into());

    sqlx::query(
        r#"
        INSERT INTO ikigai_profiles (user_id, mission, themes_json)
        VALUES (?, ?, ?)
        ON CONFLICT(user_id) DO UPDATE SET
          mission = excluded.mission,
          themes_json = excluded.themes_json,
          updated_at = (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        "#,
    )
    .bind(user_id)
    .bind(body.mission.as_deref())
    .bind(&themes_json)
    .execute(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ── Goal alignment ──

#[derive(Deserialize)]
struct SaveAlignmentBody {
    alignment_score: i64,
    quadrants: Vec<String>,
}

async fn save_goal_alignment(
    State(st): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(goal_id): axum::extract::Path<String>,
    Json(body): Json<SaveAlignmentBody>,
) -> Result<impl IntoResponse, HttpError> {
    let user_id = user_id_from(&headers)?;
    ensure_user(&st.db, user_id).await?;

    if !(1..=100).contains(&body.alignment_score) {
        return Err(HttpError::bad_request("alignment_score must be 1..100"));
    }

    let valid_quadrants = ["passion", "mission", "profession", "vocation"];
    for q in &body.quadrants {
        if !valid_quadrants.contains(&q.as_str()) {
            return Err(HttpError::bad_request(format!(
                "invalid quadrant: {q}. Must be one of: passion, mission, profession, vocation"
            )));
        }
    }

    let quadrants_json = serde_json::to_string(&body.quadrants)
        .map_err(|_| HttpError::bad_request("bad quadrants"))?;

    sqlx::query(
        r#"
        INSERT INTO goal_alignment (goal_id, user_id, alignment_score, quadrants_json)
        VALUES (?, ?, ?, ?)
        ON CONFLICT(goal_id) DO UPDATE SET
          alignment_score = excluded.alignment_score,
          quadrants_json = excluded.quadrants_json,
          updated_at = (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        "#,
    )
    .bind(&goal_id)
    .bind(user_id)
    .bind(body.alignment_score)
    .bind(&quadrants_json)
    .execute(&st.db)
    .await
    .map_err(|_| HttpError::bad_request("db error"))?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}
