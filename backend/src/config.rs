use std::net::SocketAddr;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub database_url: String,
    pub cors_allow_origin: Option<String>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8580".to_string());
        let bind: SocketAddr = bind.parse()?;

        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://./happi.db?mode=rwc".to_string());

        let cors_allow_origin = std::env::var("CORS_ALLOW_ORIGIN").ok();

        Ok(Self {
            bind,
            database_url,
            cors_allow_origin,
        })
    }
}

// ── LLM configuration ──
// All model choices and parameters in one place for easy tuning.

/// Model used for intent parsing + chat responses
pub fn chat_model() -> String {
    std::env::var("HAPPI_CHAT_MODEL").unwrap_or_else(|_| "gpt-5-mini".to_string())
}

/// Model used for generating observations about the user
pub fn observation_model() -> String {
    std::env::var("HAPPI_OBSERVATION_MODEL").unwrap_or_else(|_| "gpt-5-mini".to_string())
}

/// Embedding model for semantic search
pub fn embedding_model() -> String {
    std::env::var("HAPPI_EMBEDDING_MODEL").unwrap_or_else(|_| "text-embedding-3-large".to_string())
}

/// Reasoning effort for intent parsing + chat responses.
pub fn chat_reasoning_effort() -> String {
    std::env::var("HAPPI_CHAT_REASONING_EFFORT").unwrap_or_else(|_| "low".to_string())
}

/// Reasoning effort for observation generation.
pub fn observation_reasoning_effort() -> String {
    std::env::var("HAPPI_OBSERVATION_REASONING_EFFORT").unwrap_or_else(|_| "low".to_string())
}

/// Verbosity for user-facing coaching responses.
pub fn chat_verbosity() -> String {
    std::env::var("HAPPI_CHAT_VERBOSITY").unwrap_or_else(|_| "low".to_string())
}

/// Verbosity for structured observation outputs.
pub fn observation_verbosity() -> String {
    std::env::var("HAPPI_OBSERVATION_VERBOSITY").unwrap_or_else(|_| "low".to_string())
}

/// Max time to wait for the critical-path intent response before falling back.
pub fn chat_timeout_ms() -> u64 {
    std::env::var("HAPPI_CHAT_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(15_000)
}

/// Max time to wait for background observation generation before skipping it.
pub fn observation_timeout_ms() -> u64 {
    std::env::var("HAPPI_OBSERVATION_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20_000)
}

/// Model used for forced ikigai generation.
pub fn ikigai_model() -> String {
    std::env::var("HAPPI_IKIGAI_MODEL").unwrap_or_else(|_| "gpt-5-mini".to_string())
}

/// Reasoning effort for ikigai generation.
pub fn ikigai_reasoning_effort() -> String {
    std::env::var("HAPPI_IKIGAI_REASONING_EFFORT").unwrap_or_else(|_| "medium".to_string())
}

/// Verbosity for structured ikigai outputs.
pub fn ikigai_verbosity() -> String {
    std::env::var("HAPPI_IKIGAI_VERBOSITY").unwrap_or_else(|_| "low".to_string())
}

/// Max time to wait for forced ikigai generation.
pub fn ikigai_timeout_ms() -> u64 {
    std::env::var("HAPPI_IKIGAI_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(25_000)
}

/// Number of recent chat messages to include as direct context
pub fn chat_history_window() -> i32 {
    std::env::var("HAPPI_CHAT_HISTORY_WINDOW")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10)
}

/// Number of semantically retrieved past conversations
pub fn semantic_search_top_k() -> usize {
    std::env::var("HAPPI_SEMANTIC_TOP_K")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5)
}

/// Min messages between observation generation rounds
pub fn observation_interval() -> i64 {
    std::env::var("HAPPI_OBSERVATION_INTERVAL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(6)
}

/// Max active observations to include in context
pub fn max_observations_in_context() -> i32 {
    std::env::var("HAPPI_MAX_OBSERVATIONS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(15)
}

/// When true the bot stays online but rejects all messages with a "stopped" notice.
pub fn stopped_mode() -> bool {
    matches!(
        std::env::var("HAPPI_STOPPED").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}
