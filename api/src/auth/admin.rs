//! Admin authentication for the `/v1/keys` management routes, gated by a
//! single `X-Admin-Key` header compared against `UNBAGRND_ADMIN_KEY`.
//!
//! Deliberately separate from [`super::ApiKeyAuth`]: a regular API key
//! (handed out to end users of the background-removal endpoint) must never
//! be able to create, list or revoke *other* keys.

use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use subtle::ConstantTimeEq;

use crate::error::ApiError;
use crate::state::AppState;

/// Extracted once a request's `X-Admin-Key` header has been checked
/// against the server's configured admin key. Carries no data - its mere
/// existence as an extracted value proves the check passed.
pub struct AdminAuth;

impl FromRequestParts<Arc<AppState>> for AdminAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let Some(configured) = state.config.admin_key.as_deref() else {
            return Err(ApiError::AdminKeyNotConfigured);
        };

        let presented = parts
            .headers
            .get("X-Admin-Key")
            .and_then(|v| v.to_str().ok())
            .ok_or(ApiError::AdminKeyRequired)?;

        let matches = presented.len() == configured.len()
            && bool::from(presented.as_bytes().ct_eq(configured.as_bytes()));
        if !matches {
            return Err(ApiError::AdminKeyRequired);
        }

        Ok(AdminAuth)
    }
}
