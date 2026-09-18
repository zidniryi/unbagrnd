//! Thin Tauri-specific wrapper around [`unbagrnd_core::models`]: resolves
//! where models are cached for this app (`app_data_dir()/models`) and turns
//! download progress into `model-download-progress` events for the
//! frontend. The actual catalog, download and checksum-verification logic
//! all live in the shared core crate - see there for the real
//! implementation.

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Manager};

pub use unbagrnd_core::models::{find_model, DEFAULT_MODEL_KEY, MODELS};
pub use unbagrnd_core::models::{ModelInfo, ModelSpec};

fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("could not resolve the app data directory: {e}"))
        .map(|dir| dir.join("models"))
}

pub fn list_models(app: &AppHandle) -> Result<Vec<ModelInfo>, String> {
    Ok(unbagrnd_core::models::list_models(&models_dir(app)?))
}

/// Ensures the given model is present in the app's data directory,
/// downloading and checksum-verifying it if it isn't. Emits
/// `model-download-progress` events (tagged with the model's key) as the
/// download proceeds.
pub async fn ensure_model(app: &AppHandle, spec: &ModelSpec) -> Result<PathBuf, String> {
    let dir = models_dir(app)?;
    let app = app.clone();
    unbagrnd_core::models::ensure_model(&dir, spec, move |progress| {
        let _ = app.emit("model-download-progress", progress);
    })
    .await
}

/// Deletes a cached model file, if present, freeing its disk space.
pub async fn clear_model(app: &AppHandle, spec: &ModelSpec) -> Result<(), String> {
    unbagrnd_core::models::clear_model(&models_dir(app)?, spec).await
}
