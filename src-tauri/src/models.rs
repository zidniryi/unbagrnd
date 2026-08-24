//! The catalog of background-removal models the user can choose from, plus
//! downloading and caching whichever ones they've selected.
//!
//! This is the only module in the app that ever makes a network request, and
//! it only does so lazily: the first time a given model is actually used.
//! Every call after that reads the cached file straight from disk.

use std::path::PathBuf;

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

/// How a model's raw output tensor should be turned into an alpha mask.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// A single-channel prediction: min-max normalize it directly into an
    /// alpha mask. Used by every matting-style model (U2-Net, Silueta,
    /// IS-Net, BiRefNet).
    Alpha,
    /// A multi-class segmentation (background / upper cloth / lower cloth /
    /// full-body cloth): take the argmax per pixel and treat anything that
    /// isn't the background class as foreground. Used by U2-Net's cloth
    /// segmentation model, which classifies garment regions rather than
    /// predicting a single soft matte.
    ClothSegAnyForeground,
}

/// Static description of one selectable background-removal model.
pub struct ModelSpec {
    /// Stable identifier used in the frontend and in settings.json.
    pub key: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    file_name: &'static str,
    url: &'static str,
    pub size_bytes: u64,
    sha256: &'static str,
    /// The model's expected square input resolution.
    pub input_size: u32,
    /// Per-channel (R, G, B) normalization mean.
    pub mean: [f32; 3],
    /// Per-channel (R, G, B) normalization std-dev.
    pub std: [f32; 3],
    /// Whether the raw output needs a sigmoid applied before normalizing
    /// (BiRefNet outputs logits; the U2-Net/IS-Net family doesn't).
    pub apply_sigmoid: bool,
    pub output_mode: OutputMode,
}

/// All models are re-hosted ONNX release assets of `developersharif/bgremover-app`
/// (free to redistribute) - the underlying weights are the same ones used by
/// the popular `rembg` Python tool (U2-Net, IS-Net, BiRefNet, Silueta), each
/// MIT/Apache-licensed and free for personal and commercial use. Exact input
/// size and normalization for each model come from `rembg`'s own session
/// implementations, not guessed.
pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        key: "silueta",
        display_name: "Silueta",
        description: "Default — fastest, smallest download, general use",
        file_name: "silueta.onnx",
        url: "https://github.com/developersharif/bgremover-app/releases/download/silueta/silueta.onnx",
        size_bytes: 44_173_029,
        sha256: "75da6c8d2f8096ec743d071951be73b4a8bc7b3e51d9a6625d63644f90ffeedb",
        input_size: 320,
        mean: [0.485, 0.456, 0.406],
        std: [0.229, 0.224, 0.225],
        apply_sigmoid: false,
        output_mode: OutputMode::Alpha,
    },
    ModelSpec {
        key: "u2net",
        display_name: "U2-Net",
        description: "General use, higher accuracy than Silueta",
        file_name: "u2net.onnx",
        url: "https://github.com/developersharif/bgremover-app/releases/download/u2net/u2net.onnx",
        size_bytes: 175_997_641,
        sha256: "8d10d2f3bb75ae3b6d527c77944fc5e7dcd94b29809d47a739a7a728a912b491",
        input_size: 320,
        mean: [0.485, 0.456, 0.406],
        std: [0.229, 0.224, 0.225],
        apply_sigmoid: false,
        output_mode: OutputMode::Alpha,
    },
    ModelSpec {
        key: "birefnet-general-lite",
        display_name: "BiRefNet (General, Lite)",
        description: "General use, newer architecture, cleaner edges and hair detail",
        file_name: "BiRefNet-general-bb_swin_v1_tiny-epoch_232.onnx",
        url: "https://github.com/developersharif/bgremover-app/releases/download/birefnet-general-lite/BiRefNet-general-bb_swin_v1_tiny-epoch_232.onnx",
        size_bytes: 224_005_088,
        sha256: "5600024376f572a557870a5eb0afb1e5961636bef4e1e22132025467d0f03333",
        input_size: 1024,
        mean: [0.485, 0.456, 0.406],
        std: [0.229, 0.224, 0.225],
        apply_sigmoid: true,
        output_mode: OutputMode::Alpha,
    },
    ModelSpec {
        key: "u2net_human_seg",
        display_name: "U2-Net (Human)",
        description: "Portraits and people",
        file_name: "u2net_human_seg.onnx",
        url: "https://github.com/developersharif/bgremover-app/releases/download/u2net_human_seg/u2net_human_seg.onnx",
        size_bytes: 175_997_641,
        sha256: "01eb6a29a5c4d8edb30b56adad9bb3a2a0535338e480724a213e0acfd2d1c73c",
        input_size: 320,
        mean: [0.485, 0.456, 0.406],
        std: [0.229, 0.224, 0.225],
        apply_sigmoid: false,
        output_mode: OutputMode::Alpha,
    },
    ModelSpec {
        key: "u2net_cloth_seg",
        display_name: "U2-Net (Cloth)",
        description: "Clothing / garment parsing (keeps all detected garment regions)",
        file_name: "u2net_cloth_seg.onnx",
        url: "https://github.com/developersharif/bgremover-app/releases/download/u2net_cloth_seg/u2net_cloth_seg.onnx",
        size_bytes: 176_194_565,
        sha256: "6d2cbc27bfbdc989e1fd325656d65902ecc6a3ccbe94b2d3655ec114efcb128e",
        input_size: 768,
        mean: [0.485, 0.456, 0.406],
        std: [0.229, 0.224, 0.225],
        apply_sigmoid: false,
        output_mode: OutputMode::ClothSegAnyForeground,
    },
    ModelSpec {
        key: "isnet-anime",
        display_name: "IS-Net (Anime)",
        description: "Anime and illustration characters",
        file_name: "isnet-anime.onnx",
        url: "https://github.com/developersharif/bgremover-app/releases/download/isnet-anime/isnet-anime.onnx",
        size_bytes: 176_069_933,
        sha256: "f15622d853e8260172812b657053460e20806f04b9e05147d49af7bed31a6e99",
        input_size: 1024,
        mean: [0.485, 0.456, 0.406],
        std: [1.0, 1.0, 1.0],
        apply_sigmoid: false,
        output_mode: OutputMode::Alpha,
    },
];

pub const DEFAULT_MODEL_KEY: &str = "silueta";

pub fn find_model(key: &str) -> Option<&'static ModelSpec> {
    MODELS.iter().find(|m| m.key == key)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub key: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub size_bytes: u64,
    pub downloaded: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDownloadProgress {
    pub key: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("could not resolve the app data directory: {e}"))
        .map(|dir| dir.join("models"))
}

pub fn model_file_path(app: &AppHandle, spec: &ModelSpec) -> Result<PathBuf, String> {
    Ok(models_dir(app)?.join(spec.file_name))
}

fn is_downloaded(app: &AppHandle, spec: &ModelSpec) -> Result<bool, String> {
    let path = model_file_path(app, spec)?;
    Ok(path
        .metadata()
        .map(|m| m.is_file() && m.len() == spec.size_bytes)
        .unwrap_or(false))
}

pub fn list_models(app: &AppHandle) -> Result<Vec<ModelInfo>, String> {
    MODELS
        .iter()
        .map(|spec| {
            Ok(ModelInfo {
                key: spec.key,
                display_name: spec.display_name,
                description: spec.description,
                size_bytes: spec.size_bytes,
                downloaded: is_downloaded(app, spec)?,
            })
        })
        .collect()
}

/// Ensures the given model is present in the app data directory, downloading
/// and checksum-verifying it if it isn't. Emits `model-download-progress`
/// events (tagged with the model's key) as the download proceeds.
///
/// Safe to call every time inference is needed: if the model is already
/// cached, this does nothing but a metadata check and returns immediately.
pub async fn ensure_model(app: &AppHandle, spec: &ModelSpec) -> Result<PathBuf, String> {
    let final_path = model_file_path(app, spec)?;
    if is_downloaded(app, spec)? {
        return Ok(final_path);
    }

    let dir = models_dir(app)?;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("could not create the app data directory: {e}"))?;

    let tmp_path = dir.join(format!("{}.part", spec.file_name));

    let response = reqwest::get(spec.url)
        .await
        .map_err(|e| format!("model download failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "model download failed: server returned HTTP {}",
            response.status()
        ));
    }
    let total_bytes = response.content_length().unwrap_or(spec.size_bytes);

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
                    key: spec.key.to_string(),
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
    if digest != spec.sha256 {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(format!(
            "downloaded model failed checksum verification (expected {}, got {digest}); \
             the download may have been corrupted or interrupted, please try again",
            spec.sha256
        ));
    }

    tokio::fs::rename(&tmp_path, &final_path)
        .await
        .map_err(|e| format!("could not finalize the model file: {e}"))?;

    Ok(final_path)
}

/// Deletes a cached model file, if present, freeing its disk space.
pub async fn clear_model(app: &AppHandle, spec: &ModelSpec) -> Result<(), String> {
    let path = model_file_path(app, spec)?;
    if path.exists() {
        tokio::fs::remove_file(&path)
            .await
            .map_err(|e| format!("could not remove cached model: {e}"))?;
    }
    Ok(())
}
