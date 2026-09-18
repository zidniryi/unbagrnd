//! Request-time authentication: verifying an `X-API-Key` header against the
//! hashed keys in SQLite (see [`api_key`]), and a separate `X-Admin-Key`
//! check for the key-management routes (see [`admin`]).
//!
//! ```text
//! Request
//!    |
//!    v
//! X-API-Key
//!    |
//!    v
//! hash + look up
//!    |
//!    +---- missing/unknown ----> 401 MISSING_API_KEY / INVALID_API_KEY
//!    |
//!    +---- revoked -------------> 401 INVALID_API_KEY
//!    |
//!    +---- valid ---------------> touch usage, continue to the route
//! ```

pub mod admin;
pub mod api_key;

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use chrono::Utc;

use crate::error::ApiError;
use crate::state::{AppState, RateWindow};

/// Extracted once a request's `X-API-Key` header has been verified against
/// a non-revoked key in the database. Also enforces the per-key rate limit
/// (`UNBAGRND_RATE_LIMIT_PER_MINUTE`) as part of the same check, since
/// every route that needs one needs the other.
pub struct ApiKeyAuth {
    pub key_id: String,
    pub key_prefix: String,
}

impl FromRequestParts<Arc<AppState>> for ApiKeyAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let presented = parts
            .headers
            .get("X-API-Key")
            .and_then(|v| v.to_str().ok())
            .ok_or(ApiError::MissingApiKey)?;

        let hash = api_key::hash_key(presented);
        let record = crate::db::find_by_hash(&state.db, &hash)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or(ApiError::InvalidApiKey)?;

        // Belt-and-suspenders: re-compare in constant time even though the
        // row above was already found by an exact hash match.
        if !api_key::hashes_match(&record.key_hash, &hash) || record.revoked_at.is_some() {
            return Err(ApiError::InvalidApiKey);
        }

        check_rate_limit(state, &record.id)?;

        let now = Utc::now().to_rfc3339();
        if let Err(e) = crate::db::touch_usage(&state.db, &record.id, &now).await {
            tracing::warn!(error = %e, "could not record API key usage");
        }

        // Never log the raw key - only the safe-to-display prefix.
        tracing::info!(key_prefix = %record.key_prefix, "API key authentication succeeded");

        Ok(ApiKeyAuth {
            key_id: record.id,
            key_prefix: record.key_prefix,
        })
    }
}

/// A simple in-memory fixed-window limiter: at most
/// `UNBAGRND_RATE_LIMIT_PER_MINUTE` requests per key per rolling 60-second
/// window. `rate_limit_per_minute == 0` disables the limit entirely.
fn check_rate_limit(state: &AppState, key_id: &str) -> Result<(), ApiError> {
    let limit = state.config.rate_limit_per_minute;
    if limit == 0 {
        return Ok(());
    }

    let mut limiter = state
        .rate_limiter
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let now = Instant::now();

    let window = limiter
        .entry(key_id.to_string())
        .or_insert_with(|| RateWindow {
            window_start: now,
            count: 0,
        });

    if now.duration_since(window.window_start) >= Duration::from_secs(60) {
        window.window_start = now;
        window.count = 0;
    }

    if window.count >= limit {
        return Err(ApiError::RateLimited);
    }

    window.count += 1;
    Ok(())
}
