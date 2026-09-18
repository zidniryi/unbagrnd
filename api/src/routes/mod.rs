pub mod health;
pub mod keys;
pub mod remove_background;

use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::http::HeaderValue;
use axum::routing::{delete, get, post};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::state::AppState;

pub fn build_router(state: Arc<AppState>) -> Router {
    let body_limit = state.config.max_body_bytes();
    let cors = build_cors(&state.config.cors_origins);

    Router::new()
        .route("/health", get(health::health))
        .route("/v1/keys", post(keys::create_key).get(keys::list_keys))
        .route("/v1/keys/{id}", delete(keys::revoke_key))
        .route(
            "/v1/remove-background",
            post(remove_background::remove_background).layer(DefaultBodyLimit::max(body_limit)),
        )
        .route(
            "/v1/remove-background/batch",
            post(remove_background::remove_background_batch)
                .layer(DefaultBodyLimit::max(state.config.max_batch_body_bytes())),
        )
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}

/// Builds the CORS layer from `UNBAGRND_CORS_ORIGINS`: `*` (the default)
/// allows any origin; otherwise it's treated as a comma-separated allowlist.
/// `*` is convenient for local development and simple deployments, but the
/// README documents restricting this for anything exposed to the public
/// internet.
fn build_cors(origins: &str) -> CorsLayer {
    let origins = origins.trim();
    if origins.is_empty() || origins == "*" {
        return CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);
    }

    let allowed: Vec<HeaderValue> = origins
        .split(',')
        .map(str::trim)
        .filter(|o| !o.is_empty())
        .filter_map(|o| o.parse().ok())
        .collect();

    CorsLayer::new()
        .allow_origin(allowed)
        .allow_methods(Any)
        .allow_headers(Any)
}
