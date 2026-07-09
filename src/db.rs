use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

const RB_DB_PASSWORD: &str = "402fd482c38817c35ffa8ffb8c7d93143b749e7d315df7a81732a1ff43608497";

// Single connection: manual BEGIN/COMMIT must run on the same connection,
// and a CLI process has no use for connection-level parallelism.
fn pool_options() -> SqlitePoolOptions {
    SqlitePoolOptions::new().max_connections(1)
}

pub async fn open(path: &Path, read_only: bool) -> Result<SqlitePool, sqlx::Error> {
    let quoted_key = format!("'{}'", RB_DB_PASSWORD);
    let mut options = SqliteConnectOptions::new()
        .filename(path)
        .pragma("key", quoted_key.clone());
    if read_only {
        options = options.read_only(true);
    }
    let pool = pool_options().connect_with(options).await?;

    if sqlx::query("SELECT 1 FROM djmdContent LIMIT 1")
        .fetch_optional(&pool)
        .await
        .is_ok()
    {
        return Ok(pool);
    }
    pool.close().await;

    // Unencrypted DB (for testing)
    let mut options = SqliteConnectOptions::new().filename(path);
    if read_only {
        options = options.read_only(true);
    }
    pool_options().connect_with(options).await
}
