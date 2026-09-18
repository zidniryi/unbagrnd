//! SQLite-backed storage for API key metadata. Every query here is
//! runtime-checked (`sqlx::query`/`query_as`, never the `query!` macros) on
//! purpose: it means building this crate - including inside the Docker
//! builder stage - never needs a live `DATABASE_URL` or a checked-in
//! `.sqlx` query cache, just the `sqlite` feature.

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

/// One row of the `api_keys` table. Note there is deliberately no field
/// for the plaintext key - it's never stored anywhere, only its hash.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ApiKeyRecord {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub key_hash: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
    pub request_count: i64,
}

/// Opens (creating if necessary) the SQLite database at `database_url` and
/// applies any pending migrations. `database_url` is a standard sqlx SQLite
/// URL, e.g. `sqlite:///data/unbagrnd.db`.
pub async fn connect(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    // `SqliteConnectOptions::create_if_missing` creates the database *file*
    // but not its parent directory, so make sure that exists first (the
    // default `/data` is a fresh volume mount that won't exist yet on a
    // brand-new deployment).
    if let Some(path) = database_url
        .strip_prefix("sqlite://")
        .or_else(|| database_url.strip_prefix("sqlite:"))
    {
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
    }

    let options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    sqlx::migrate!("../migrations").run(&pool).await?;

    Ok(pool)
}

pub async fn insert_api_key(
    pool: &SqlitePool,
    id: &str,
    name: &str,
    key_prefix: &str,
    key_hash: &str,
    created_at: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO api_keys (id, name, key_prefix, key_hash, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(name)
    .bind(key_prefix)
    .bind(key_hash)
    .bind(created_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn find_by_hash(
    pool: &SqlitePool,
    key_hash: &str,
) -> Result<Option<ApiKeyRecord>, sqlx::Error> {
    sqlx::query_as::<_, ApiKeyRecord>("SELECT * FROM api_keys WHERE key_hash = ?")
        .bind(key_hash)
        .fetch_optional(pool)
        .await
}

pub async fn list_api_keys(pool: &SqlitePool) -> Result<Vec<ApiKeyRecord>, sqlx::Error> {
    sqlx::query_as::<_, ApiKeyRecord>("SELECT * FROM api_keys ORDER BY created_at DESC")
        .fetch_all(pool)
        .await
}

/// Soft-deletes a key by stamping `revoked_at` (an already-revoked key is
/// left untouched). Returns whether a row was actually updated, so the
/// caller can tell "already revoked"/"doesn't exist" apart from a real
/// no-op - the route handler treats both as 404 either way.
pub async fn revoke_api_key(
    pool: &SqlitePool,
    id: &str,
    revoked_at: &str,
) -> Result<bool, sqlx::Error> {
    let result =
        sqlx::query("UPDATE api_keys SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL")
            .bind(revoked_at)
            .bind(id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}

/// Records a successful authentication: bumps `request_count` and updates
/// `last_used_at`. Best-effort from the caller's perspective - a failure
/// here shouldn't fail the request it's attached to.
pub async fn touch_usage(pool: &SqlitePool, id: &str, used_at: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE api_keys SET last_used_at = ?, request_count = request_count + 1 WHERE id = ?",
    )
    .bind(used_at)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}
