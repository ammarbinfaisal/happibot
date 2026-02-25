mod api;
mod auth;
mod config;
mod db;
mod http_error;
mod state;
mod user_id;

use axum::{http::StatusCode, response::IntoResponse, routing::get, Router};
use state::AppState;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cfg = config::Config::from_env()?;
    let db = db::connect_and_migrate(&cfg.database_url).await?;

    let cors = match cfg.cors_allow_origin {
        Some(origin) => CorsLayer::new()
            .allow_origin(origin.parse::<axum::http::HeaderValue>()?)
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any),
        None => CorsLayer::permissive(),
    };

    let app_state = AppState { db };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .nest("/v1", api::v1::router())
        .with_state(app_state)
        .layer(TraceLayer::new_for_http())
        .layer(cors);

    let listener = tokio::net::TcpListener::bind(cfg.bind).await?;
    tracing::info!("listening on {}", cfg.bind);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}
