use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

pub async fn connect_and_migrate(database_url: &str) -> anyhow::Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;

    // Keep migrations embedded/relative for simple deployment.
    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}

