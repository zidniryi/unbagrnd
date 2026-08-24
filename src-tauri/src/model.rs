//! Downloads and caches the background-removal model.
//!
//! This is the only module in the app that ever makes a network request, and
//! it only does so once: on first use, to fetch the ONNX model into the
//! app's local data directory. Every call after that reads the cached file
//! from disk.

use std::path::PathBuf;

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

/// IS-Net "general use" background-removal model, in ONNX format.
///
/// Hosted as a release asset of `danielgatis/rembg` (MIT-licensed tool);
/// the model weights themselves are Apache-2.0, from Qin et al., "Highly
/// Accurate Dichotomous Image Segmentation" (ECCV 2022). Free for personal
/// and commercial use, no account or API key required.
pub const MODEL_URL: &str =
    "https://github.com/danielgatis/rembg/releases/download/v0.0.0/isnet-general-use.onnx";
pub const MODEL_FILENAME: &str = "isnet-general-use.onnx";
pub const MODEL_SIZE_BYTES: u64 = 178_648_008;
pub const MODEL_SHA256: &str = "60920e99c45464f2ba57bee2ad08c919a52bbf852739e96947fbb4358c0d964a";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub downloaded: bool,
    pub size_bytes: u64,
    pub path: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

fn model_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("could not resolve the app data directory: {e}"))
}

pub fn model_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(model_dir(app)?.join(MODEL_FILENAME))
}

pub fn model_status(app: &AppHandle) -> Result<ModelStatus, String> {
    let path = model_path(app)?;
    let downloaded = path
        .metadata()
        .map(|m| m.is_file() && m.len() == MODEL_SIZE_BYTES)
        .unwrap_or(false);
    Ok(ModelStatus {
        downloaded,
        size_bytes: MODEL_SIZE_BYTES,
        path: path.display().to_string(),
    })
}

/// Ensures the model is present in the app data directory, downloading and
/// checksum-verifying it if it isn't. Emits `model-download-progress` events
/// as the download proceeds so the frontend can show a progress bar.
///
/// Safe to call every time inference is needed: if the model is already
/// cached, this does nothing but a metadata check and returns immediately.
pub async fn ensure_model(app: &AppHandle) -> Result<PathBuf, String> {
    let final_path = model_path(app)?;
    if model_status(app)?.downloaded {
        return Ok(final_path);
    }

    let dir = model_dir(app)?;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("could not create the app data directory: {e}"))?;

    let tmp_path = dir.join(format!("{MODEL_FILENAME}.part"));

    let response = reqwest::get(MODEL_URL)
        .await
        .map_err(|e| format!("model download failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "model download failed: server returned HTTP {}",
            response.status()
        ));
    }
    let total_bytes = response.content_length().unwrap_or(MODEL_SIZE_BYTES);

    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| format!("could not create the model file: {e}"))?;

    let mut hasher = Sha256::new();
    let mut downloaded_bytes: u64 = 0;
    let mut stream = response.bytes_stream();
    let mut last_emit = std::time::Instant::now();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("model download failed: {e}"))?;
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("could not write the model file: {e}"))?;
        downloaded_bytes += chunk.len() as u64;

        if last_emit.elapsed().as_millis() >= 100 || downloaded_bytes == total_bytes {
            last_emit = std::time::Instant::now();
            let _ = app.emit(
                "model-download-progress",
                ModelDownloadProgress {
                    downloaded_bytes,
                    total_bytes,
                },
            );
        }
    }
    file.flush()
        .await
        .map_err(|e| format!("could not write the model file: {e}"))?;
    drop(file);

    let digest = hex::encode(hasher.finalize());
    if digest != MODEL_SHA256 {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(format!(
            "downloaded model failed checksum verification (expected {MODEL_SHA256}, got {digest}); \
             the download may have been corrupted or interrupted, please try again"
        ));
    }

    tokio::fs::rename(&tmp_path, &final_path)
        .await
        .map_err(|e| format!("could not finalize the model file: {e}"))?;

    Ok(final_path)
}
