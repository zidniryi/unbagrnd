//! `/v1/keys` - API key management. Every route here is gated by
//! [`crate::auth::admin::AdminAuth`], not a regular API key: a key handed
//! out to an end user of `/v1/remove-background` must never be able to
//! mint, list or revoke other keys. See `.env.example` for
//! `UNBAGRND_ADMIN_KEY`.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::auth::admin::AdminAuth;
use crate::auth::api_key;
use crate::error::ApiError;
use crate::state::AppState;

#[derive(Deserialize, ToSchema)]
pub struct CreateKeyRequest {
    /// A human-readable label for this key. Defaults to `"default"`.
    #[serde(default)]
    name: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct CreateKeyResponse {
    id: String,
    name: String,
    /// Shown exactly once, right now. It is never returned again - only
    /// `key_prefix` is, in [`ListKeysEntry`].
    api_key: String,
    created_at: String,
}

/// Generates a new API key. Requires `X-Admin-Key`.
#[utoipa::path(
    post,
    path = "/v1/keys",
    tag = "keys",
    request_body = CreateKeyRequest,
    responses(
        (status = 200, description = "Key created - save `api_key` now, it will not be shown again", body = CreateKeyResponse),
        (status = 401, description = "Missing/invalid X-Admin-Key", body = crate::error::ErrorBody),
    ),
    security(("admin_key" = []))
)]
pub async fn create_key(
    _admin: AdminAuth,
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateKeyRequest>,
) -> Result<Json<CreateKeyResponse>, ApiError> {
    let name = body
        .name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "default".to_string());
    let generated = api_key::generate_key();
    let id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();

    crate::db::insert_api_key(
        &state.db,
        &id,
        &name,
        &generated.key_prefix,
        &generated.key_hash,
        &created_at,
    )
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(Json(CreateKeyResponse {
        id,
        name,
        api_key: generated.plaintext,
        created_at,
    }))
}

#[derive(Serialize, ToSchema)]
pub struct ListKeysEntry {
    id: String,
    name: String,
    key_prefix: String,
    created_at: String,
    last_used_at: Option<String>,
    revoked_at: Option<String>,
    request_count: i64,
}

#[derive(Serialize, ToSchema)]
pub struct ListKeysResponse {
    keys: Vec<ListKeysEntry>,
}

/// Lists every API key's metadata (never the key itself or its hash).
/// Requires `X-Admin-Key`.
#[utoipa::path(
    get,
    path = "/v1/keys",
    tag = "keys",
    responses(
        (status = 200, description = "Key metadata", body = ListKeysResponse),
        (status = 401, description = "Missing/invalid X-Admin-Key", body = crate::error::ErrorBody),
    ),
    security(("admin_key" = []))
)]
pub async fn list_keys(
    _admin: AdminAuth,
    State(state): State<Arc<AppState>>,
) -> Result<Json<ListKeysResponse>, ApiError> {
    let records = crate::db::list_api_keys(&state.db)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(Json(ListKeysResponse {
        keys: records
            .into_iter()
            .map(|r| ListKeysEntry {
                id: r.id,
                name: r.name,
                key_prefix: r.key_prefix,
                created_at: r.created_at,
                last_used_at: r.last_used_at,
                revoked_at: r.revoked_at,
                request_count: r.request_count,
            })
            .collect(),
    }))
}

/// Revokes an API key by id. Idempotent-ish: revoking an already-revoked
/// or unknown key both return 404. The row is kept (not deleted) with
/// `revoked_at` set, as an audit trail. Requires `X-Admin-Key`.
#[utoipa::path(
    delete,
    path = "/v1/keys/{id}",
    tag = "keys",
    params(("id" = String, Path, description = "The key's id, from POST /v1/keys or GET /v1/keys")),
    responses(
        (status = 204, description = "Key revoked"),
        (status = 401, description = "Missing/invalid X-Admin-Key", body = crate::error::ErrorBody),
        (status = 404, description = "No such (unrevoked) key", body = crate::error::ErrorBody),
    ),
    security(("admin_key" = []))
)]
pub async fn revoke_key(
    _admin: AdminAuth,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    let revoked_at = Utc::now().to_rfc3339();
    let revoked = crate::db::revoke_api_key(&state.db, &id, &revoked_at)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    if revoked {
        Ok(axum::http::StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::KeyNotFound)
    }
}
