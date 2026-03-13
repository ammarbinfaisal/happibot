mod api;
mod auth;
mod config;
mod db;
mod dotenv;
mod http_error;
mod openai;
mod scheduler;
mod state;
mod telegram;
mod user_id;

use axum::{http::StatusCode, response::IntoResponse, routing::get, Router};
use state::AppState;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    dotenv::load_dotenv_if_present();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async_main())
}

async fn async_main() -> anyhow::Result<()> {
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

    telegram::spawn_set_webhook_on_startup();
    scheduler::spawn_reminder_loop(db.clone());

    let app_state = AppState { db };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/telegram/webhook", axum::routing::post(telegram::telegram_webhook))
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
