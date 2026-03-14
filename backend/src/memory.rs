use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::openai;

// ── Embedding helpers ──

fn f32s_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn bytes_to_f32s(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom < 1e-10 { 0.0 } else { dot / denom }
}

fn content_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

// ── Store embedding ──

async fn store_embedding(
    db: &SqlitePool,
    user_id: i64,
    source_type: &str,
    source_id: &str,
    embedding: &[f32],
    text: &str,
) -> anyhow::Result<()> {
    let blob = f32s_to_bytes(embedding);
    let hash = content_hash(text);

    sqlx::query(
        r#"INSERT INTO embeddings (user_id, source_type, source_id, embedding, content_hash)
           VALUES (?, ?, ?, ?, ?)
           ON CONFLICT(source_type, source_id) DO UPDATE SET
             embedding = excluded.embedding,
             content_hash = excluded.content_hash"#,
    )
    .bind(user_id)
    .bind(source_type)
    .bind(source_id)
    .bind(&blob)
    .bind(&hash)
    .execute(db)
    .await?;

    Ok(())
}

// ── Semantic search ──

pub struct SearchResult {
    pub source_type: String,
    pub source_id: String,
    pub score: f32,
}

pub async fn search_similar(
    db: &SqlitePool,
    user_id: i64,
    query_embedding: &[f32],
    source_types: &[&str],
    top_k: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    // Build placeholders for source_types
    let placeholders: Vec<String> = source_types
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 2))
        .collect();
    let query = format!(
        "SELECT source_type, source_id, embedding FROM embeddings WHERE user_id = ?1 AND source_type IN ({})",
        placeholders.join(",")
    );

    let mut q = sqlx::query_as::<_, (String, String, Vec<u8>)>(&query).bind(user_id);
    for st in source_types {
        q = q.bind(*st);
    }
    let rows = q.fetch_all(db).await?;

    let mut scored: Vec<SearchResult> = rows
        .into_iter()
        .map(|(source_type, source_id, blob)| {
            let emb = bytes_to_f32s(&blob);
            let score = cosine_similarity(query_embedding, &emb);
            SearchResult {
                source_type,
                source_id,
                score,
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(top_k);
    Ok(scored)
}

/// Load the actual chat content for search results
pub async fn load_chat_content(
    db: &SqlitePool,
    results: &[SearchResult],
) -> anyhow::Result<Vec<String>> {
    let mut contents = Vec::new();
    for r in results {
        if r.source_type != "chat" {
            continue;
        }
        // source_id is the chat_history id (the user message id)
        // Load the user message and the following assistant message
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT role, content, created_at FROM chat_history WHERE id >= ? AND id <= ? + 1 ORDER BY id",
        )
        .bind(&r.source_id)
        .bind(&r.source_id)
        .fetch_all(db)
        .await?;

        if !rows.is_empty() {
            let date = rows[0].2.split('T').next().unwrap_or("?");
            let turn: Vec<String> = rows
                .iter()
                .map(|(role, content, _)| format!("{role}: {content}"))
                .collect();
            contents.push(format!("[{date}] {}", turn.join(" | ")));
        }
    }
    Ok(contents)
}

// ── Post-message pipeline: embed chat + maybe generate observations ──

pub async fn post_message_pipeline(
    db: &SqlitePool,
    user_id: i64,
    user_msg_id: i64,
    user_text: &str,
    reply: &str,
    goal_titles: &[String],
) {
    // 1. Embed the conversation turn
    if let Err(e) = embed_chat_turn(db, user_id, user_msg_id, user_text, reply).await {
        tracing::error!(user_id, ?e, "failed to embed chat turn");
    }

    // 2. Maybe generate observations (every 4th message or if substantive)
    if let Err(e) = maybe_generate_observations(db, user_id, goal_titles).await {
        tracing::error!(user_id, ?e, "failed to generate observations");
    }
}

async fn embed_chat_turn(
    db: &SqlitePool,
    user_id: i64,
    user_msg_id: i64,
    user_text: &str,
    reply: &str,
) -> anyhow::Result<()> {
    let combined = format!("User: {user_text}\nAssistant: {reply}");
    let embeddings = openai::embed(&[&combined]).await?;
    if let Some(vec) = embeddings.first() {
        store_embedding(
            db,
            user_id,
            "chat",
            &user_msg_id.to_string(),
            vec,
            &combined,
        )
        .await?;
    }
    Ok(())
}

async fn maybe_generate_observations(
    db: &SqlitePool,
    user_id: i64,
    goal_titles: &[String],
) -> anyhow::Result<()> {
    // Count messages since last observation was generated
    let last_obs_time: Option<(String,)> = sqlx::query_as(
        "SELECT created_at FROM observations WHERE user_id = ? ORDER BY created_at DESC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(db)
    .await?;

    let msg_count: (i64,) = match &last_obs_time {
        Some((ts,)) => {
            sqlx::query_as("SELECT COUNT(*) FROM chat_history WHERE user_id = ? AND created_at > ?")
                .bind(user_id)
                .bind(ts)
                .fetch_one(db)
                .await?
        }
        None => {
            sqlx::query_as("SELECT COUNT(*) FROM chat_history WHERE user_id = ?")
                .bind(user_id)
                .fetch_one(db)
                .await?
        }
    };

    let interval = crate::config::observation_interval();
    if msg_count.0 < interval {
        return Ok(());
    }

    // Load recent chat for the observation LLM
    let recent_chat: Vec<(String, String)> = sqlx::query_as(
        "SELECT role, content FROM chat_history WHERE user_id = ? ORDER BY id DESC LIMIT 20",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?
    .into_iter()
    .rev()
    .collect();

    // Load existing active observations
    let existing_obs: Vec<(String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT id, category, goal_id, content FROM observations WHERE user_id = ? AND superseded_by IS NULL ORDER BY updated_at DESC LIMIT 20",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;

    // Call LLM
    let new_obs = openai::generate_observations(&recent_chat, &existing_obs, goal_titles).await?;

    for obs in &new_obs {
        let id = Uuid::new_v4().to_string();

        // If superseding an existing observation, mark the old one
        if let Some(ref supersedes_id) = obs.supersedes {
            sqlx::query("UPDATE observations SET superseded_by = ?, updated_at = (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) WHERE id = ?")
                .bind(&id)
                .bind(supersedes_id)
                .execute(db)
                .await?;
        }

        // Resolve goal_id from title if provided
        let goal_id: Option<String> = if let Some(ref gt) = obs.goal_title {
            let row: Option<(String,)> = sqlx::query_as(
                "SELECT id FROM goals WHERE user_id = ? AND status = 'active' AND LOWER(title) LIKE '%' || LOWER(?) || '%' LIMIT 1",
            )
            .bind(user_id)
            .bind(gt)
            .fetch_optional(db)
            .await?;
            row.map(|(id,)| id)
        } else {
            None
        };

        sqlx::query(
            "INSERT INTO observations (id, user_id, goal_id, category, content, confidence) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(user_id)
        .bind(&goal_id)
        .bind(&obs.category)
        .bind(&obs.content)
        .bind(obs.confidence)
        .execute(db)
        .await?;

        // Embed the observation too
        if let Ok(embs) = openai::embed(&[&obs.content]).await {
            if let Some(vec) = embs.first() {
                let _ = store_embedding(db, user_id, "observation", &id, vec, &obs.content).await;
            }
        }
    }

    if !new_obs.is_empty() {
        tracing::info!(user_id, count = new_obs.len(), "generated observations");
    }

    Ok(())
}

/// Load active observations for a user (for context injection)
/// Returns (category, goal_title, content, created_date)
pub async fn load_active_observations(
    db: &SqlitePool,
    user_id: i64,
) -> anyhow::Result<Vec<(String, Option<String>, String, String)>> {
    let limit = crate::config::max_observations_in_context();
    let rows: Vec<(String, Option<String>, String, String)> = sqlx::query_as(
        r#"SELECT o.category, g.title, o.content, o.created_at
           FROM observations o
           LEFT JOIN goals g ON g.id = o.goal_id
           WHERE o.user_id = ? AND o.superseded_by IS NULL
           ORDER BY o.confidence DESC, o.updated_at DESC
           LIMIT ?"#,
    )
    .bind(user_id)
    .bind(limit)
    .fetch_all(db)
    .await?;

    Ok(rows)
}

/// Load superseded observation history for a specific observation chain
pub async fn load_observation_history(
    db: &SqlitePool,
    observation_id: &str,
) -> anyhow::Result<Vec<(String, String, String)>> {
    // Walk backwards through the supersession chain
    // Returns (content, category, created_at) from oldest to newest
    let mut chain = Vec::new();
    let mut current_id = observation_id.to_string();

    // Find the root (walk backwards via superseded_by)
    loop {
        let row: Option<(String, String, String, Option<String>)> = sqlx::query_as(
            "SELECT content, category, created_at, id FROM observations WHERE superseded_by = ?",
        )
        .bind(&current_id)
        .fetch_optional(db)
        .await?;

        match row {
            Some((content, category, created_at, Some(id))) => {
                chain.push((content, category, created_at));
                current_id = id;
            }
            _ => break,
        }
    }

    chain.reverse();
    Ok(chain)
}
