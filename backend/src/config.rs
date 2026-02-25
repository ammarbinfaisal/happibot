use std::net::SocketAddr;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub database_url: String,
    pub cors_allow_origin: Option<String>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
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
