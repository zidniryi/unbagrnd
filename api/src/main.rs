//! unbagrnd-api: a self-hosted REST API wrapping the same on-device
//! background-removal core the Tauri desktop app uses (`unbagrnd-core`).
//!
//! Startup order matters here: the model is downloaded/verified and loaded
//! into an ONNX Runtime session exactly once, before the server starts
//! accepting connections, and then shared across every request via
//! `AppState`. See `unbagrnd_core::bg_remove` for why - `ort` sessions are
//! expensive to build and not meant to be recreated per request.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::net::TcpListener;
use tokio::sync::Semaphore;

use unbagrnd_api::config::Config;
use unbagrnd_api::state::AppState;
use unbagrnd_api::{db, openapi, routes};

#[tokio::main]
async fn main() {
    // Missing a `.env` file is fine (e.g. real env vars set by Docker
    // Compose / the host) - only a malformed one is worth failing loudly
    // for, and dotenvy already only errors on that, not on "not found".
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env();

    if config.admin_key.is_none() {
        tracing::warn!(
            "UNBAGRND_ADMIN_KEY is not set - /v1/keys (create/list/revoke) is disabled until it is"
        );
    }

    let spec = unbagrnd_core::models::find_model(&config.model_key).unwrap_or_else(|| {
        panic!(
            "unknown UNBAGRND_MODEL_KEY \"{}\" - see unbagrnd_core::models::MODELS for valid keys",
            config.model_key
        )
    });

    tracing::info!(
        model = spec.key,
        "ensuring the background-removal model is available"
    );
    let model_path = unbagrnd_core::models::ensure_model(&config.models_dir, spec, |progress| {
        tracing::info!(
            model = %progress.key,
            downloaded_bytes = progress.downloaded_bytes,
            total_bytes = progress.total_bytes,
            "downloading model"
        );
    })
    .await
    .expect("failed to prepare the background-removal model");
    tracing::info!(model = spec.key, path = %model_path.display(), "model ready");

    let db = db::connect(&config.database_url)
        .await
        .expect("failed to open the database");
    tracing::info!(database_url = %config.database_url, "database ready");

    let inference_semaphore = Arc::new(Semaphore::new(config.max_concurrent_inferences.max(1)));

    let addr = config.addr();
    let state = Arc::new(AppState {
        db,
        inference: unbagrnd_core::bg_remove::InferenceState::new(),
        model_spec: spec,
        model_path,
        config,
        inference_semaphore,
        rate_limiter: Mutex::new(HashMap::new()),
    });

    let app = routes::build_router(state).merge(openapi::swagger_ui());

    let listener = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    tracing::info!(%addr, "unbagrnd API listening - docs at /docs");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutting down");
}
