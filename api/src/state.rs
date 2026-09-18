//! The application's shared state, handed to every route via Axum's
//! `State` extractor.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use sqlx::SqlitePool;
use tokio::sync::Semaphore;
use unbagrnd_core::bg_remove::InferenceState;
use unbagrnd_core::models::ModelSpec;

use crate::config::Config;

/// One caller's rate-limit bookkeeping: how many requests they've made in
/// the current fixed window, and when that window started.
pub struct RateWindow {
    pub window_start: Instant,
    pub count: u32,
}

pub struct AppState {
    pub db: SqlitePool,
    /// The model is loaded once at startup and shared across every
    /// request - see the module docs on `main.rs` for why that matters.
    pub inference: InferenceState,
    pub model_spec: &'static ModelSpec,
    pub model_path: PathBuf,
    pub config: Config,
    /// Bounds how many `remove_background` calls run at once, regardless
    /// of how many requests are in flight. `Arc`-wrapped so a route
    /// handler can hold an owned permit (`acquire_owned`) across a
    /// `spawn_blocking` move of the rest of the shared state.
    pub inference_semaphore: Arc<Semaphore>,
    /// In-memory, per-API-key fixed-window rate limiting. Resets on
    /// restart - see `.env.example` / README for why that's an accepted
    /// tradeoff for a self-hosted single-instance deployment.
    pub rate_limiter: Mutex<HashMap<String, RateWindow>>,
}
