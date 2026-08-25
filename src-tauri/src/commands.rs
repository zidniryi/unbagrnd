//! Tauri command handlers exposed to the frontend.

use std::path::{Path, PathBuf};

use base64::Engine;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::bg_remove::{self, InferenceState};
use crate::models::{self, ModelInfo, ModelSpec};
use crate::settings::{self, Settings};

/// Preview images sent back to the frontend are capped to this size on
/// their longest edge so before/after thumbnails stay a few hundred KB
/// instead of multiple megabytes over IPC. The saved output file on disk
/// is always full resolution regardless of this cap.
const PREVIEW_MAX_DIM: u32 = 1024;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SingleResult {
    pub output_path: String,
    pub before_data_url: String,
    pub after_data_url: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum BatchFileResult {
    // `rename_all` on the enum itself only renames the tag value ("done"/"error");
    // it does NOT cascade into each variant's own fields, so without this the
    // frontend actually received `output_path`/`after_data_url` (snake_case) and
    // silently read `undefined` for `outputPath`/`afterDataUrl`.
    #[serde(rename_all = "camelCase")]
    Done {
        output_path: String,
        after_data_url: String,
    },
    #[serde(rename_all = "camelCase")]
    Error {
        message: String,
    },
}

/// Batch row thumbnails are much smaller than single-mode previews since
/// they only need to render at list-row size.
const BATCH_THUMB_MAX_DIM: u32 = 160;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchProgressEvent {
    pub index: usize,
    pub total: usize,
    pub file_name: String,
    #[serde(flatten)]
    pub result: BatchFileResult,
}

/// Resolves a model key from the frontend (which may omit it, meaning "use
/// the user's default") into a concrete, known model spec.
fn resolve_model(app: &AppHandle, model_key: Option<&str>) -> Result<&'static ModelSpec, String> {
    let key = match model_key {
        Some(key) => key.to_string(),
        None => settings::load_settings(app)?.selected_model,
    };
    models::find_model(&key).ok_or_else(|| format!("unknown model \"{key}\""))
}

#[tauri::command]
pub async fn list_models(app: AppHandle) -> Result<Vec<ModelInfo>, String> {
    models::list_models(&app)
}

#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Result<Settings, String> {
    settings::load_settings(&app)
}

#[tauri::command]
pub async fn set_selected_model(app: AppHandle, key: String) -> Result<Settings, String> {
    settings::set_selected_model(&app, &key).await
}

#[tauri::command]
pub async fn set_theme(app: AppHandle, theme: String) -> Result<Settings, String> {
    settings::set_theme(&app, &theme).await
}

#[tauri::command]
pub async fn clear_all_models(app: AppHandle) -> Result<(), String> {
    for spec in models::MODELS {
        models::clear_model(&app, spec).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn download_model(app: AppHandle, key: String) -> Result<ModelInfo, String> {
    let spec = models::find_model(&key).ok_or_else(|| format!("unknown model \"{key}\""))?;
    models::ensure_model(&app, spec).await?;
    models::list_models(&app)?
        .into_iter()
        .find(|m| m.key == spec.key)
        .ok_or_else(|| "model disappeared after download".to_string())
}

#[tauri::command]
pub async fn clear_model(app: AppHandle, key: String) -> Result<(), String> {
    let spec = models::find_model(&key).ok_or_else(|| format!("unknown model \"{key}\""))?;
    models::clear_model(&app, spec).await
}

/// Removes the background from a single image and writes the result next
/// to (or into `output_dir`, if given) the source file, suffixed
/// `-nobg.png`. Returns the output path plus small before/after previews
/// as data URLs for the UI.
#[tauri::command]
pub async fn remove_background_single(
    app: AppHandle,
    input_path: String,
    output_dir: Option<String>,
    model_key: Option<String>,
) -> Result<SingleResult, String> {
    let spec = resolve_model(&app, model_key.as_deref())?;
    let model_path = models::ensure_model(&app, spec).await?;
    let input_path = PathBuf::from(input_path);
    let output_path = output_path_for(&input_path, output_dir.as_deref())?;

    tauri::async_runtime::spawn_blocking(move || -> Result<SingleResult, String> {
        let original =
            image::open(&input_path).map_err(|e| format!("could not read image: {e}"))?;
        let before_data_url = to_data_url(&original)?;

        let inference = app.state::<InferenceState>();
        let after =
            bg_remove::remove_background(inference.inner(), spec, &model_path, &original)?;
        after
            .save_with_format(&output_path, ImageFormat::Png)
            .map_err(|e| format!("could not write output image: {e}"))?;

        let after_data_url = to_data_url(&DynamicImage::ImageRgba8(after))?;

        Ok(SingleResult {
            output_path: output_path.display().to_string(),
            before_data_url,
            after_data_url,
        })
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

/// Expands any directories in `paths` into their individual image files
/// (see [`expand_paths`]), without processing anything. Lets the frontend
/// render one row per file — with its original image as an immediate
/// preview — before background removal has even started.
#[tauri::command]
pub async fn expand_batch_paths(paths: Vec<String>) -> Result<Vec<String>, String> {
    expand_paths(&paths)
}

/// Reads an image straight off disk and returns it as a small preview data
/// URL, with no processing. Used to show a batch row's "before" thumbnail
/// immediately, while the real background-removal pass is still running.
#[tauri::command]
pub async fn preview_image(path: String) -> Result<String, String> {
    let path = PathBuf::from(path);
    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let original = image::open(&path).map_err(|e| format!("could not read image: {e}"))?;
        to_data_url_sized(&original, BATCH_THUMB_MAX_DIM)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

/// Removes the background from every image in `input_paths`, one at a
/// time, emitting a `batch-progress` event after each file completes (or
/// fails) so the UI can render a progress bar without blocking on the
/// whole batch. Each file's processing runs on a blocking-friendly worker
/// thread so the UI stays responsive throughout.
#[tauri::command]
pub async fn remove_background_batch(
    app: AppHandle,
    input_paths: Vec<String>,
    output_dir: Option<String>,
    model_key: Option<String>,
) -> Result<(), String> {
    let spec = resolve_model(&app, model_key.as_deref())?;
    let model_path = models::ensure_model(&app, spec).await?;
    let input_paths = expand_paths(&input_paths)?;
    let total = input_paths.len();

    for (index, raw_input_path) in input_paths.into_iter().enumerate() {
        let input_path = PathBuf::from(&raw_input_path);
        let file_name = input_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| raw_input_path.clone());

        let file_result = {
            let app = app.clone();
            let model_path = model_path.clone();
            let output_dir = output_dir.clone();
            let input_path = input_path.clone();
            let outcome = tauri::async_runtime::spawn_blocking(
                move || -> Result<(PathBuf, String), String> {
                    let output_path = output_path_for(&input_path, output_dir.as_deref())?;
                    let original = image::open(&input_path)
                        .map_err(|e| format!("could not read image: {e}"))?;
                    let inference = app.state::<InferenceState>();
                    let after = bg_remove::remove_background(
                        inference.inner(),
                        spec,
                        &model_path,
                        &original,
                    )?;
                    after
                        .save_with_format(&output_path, ImageFormat::Png)
                        .map_err(|e| format!("could not write output image: {e}"))?;
                    let after_data_url =
                        to_data_url_sized(&DynamicImage::ImageRgba8(after), BATCH_THUMB_MAX_DIM)?;
                    Ok((output_path, after_data_url))
                },
            )
            .await
            .map_err(|e| format!("background task failed: {e}"));

            match outcome {
                Ok(Ok((output_path, after_data_url))) => BatchFileResult::Done {
                    output_path: output_path.display().to_string(),
                    after_data_url,
                },
                Ok(Err(message)) | Err(message) => BatchFileResult::Error { message },
            }
        };

        let _ = app.emit(
            "batch-progress",
            BatchProgressEvent {
                index,
                total,
                file_name,
                result: file_result,
            },
        );
    }

    Ok(())
}

/// Extensions the image decoder understands. Used only to filter which
/// files inside a *dropped or picked folder* get picked up automatically;
/// a file the user pointed at directly is always attempted regardless of
/// its extension, so a wrong guess here just means a clear per-file error
/// in the batch list rather than a silently skipped file.
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff", "gif"];

/// Expands any directories in `paths` into the image files directly inside
/// them (non-recursive, sorted by name), leaving file paths untouched.
/// This lets the frontend hand a dropped or picked folder straight to the
/// batch command without needing its own filesystem access.
fn expand_paths(paths: &[String]) -> Result<Vec<String>, String> {
    let mut expanded = Vec::new();
    for raw_path in paths {
        let path = Path::new(raw_path);
        let metadata = path
            .metadata()
            .map_err(|e| format!("could not access {}: {e}", path.display()))?;

        if metadata.is_dir() {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
                .map_err(|e| format!("could not read folder {}: {e}", path.display()))?
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|p| {
                    p.is_file()
                        && p.extension()
                            .and_then(|ext| ext.to_str())
                            .map(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
                            .unwrap_or(false)
                })
                .collect();
            entries.sort();
            expanded.extend(entries.into_iter().map(|p| p.display().to_string()));
        } else {
            expanded.push(raw_path.clone());
        }
    }
    Ok(expanded)
}

/// Resolves where a processed image should be written: into `output_dir`
/// if the caller picked one, otherwise next to the source file. Either
/// way the file is named `<original-stem>-nobg.png`.
fn output_path_for(input_path: &Path, output_dir: Option<&str>) -> Result<PathBuf, String> {
    let stem = input_path
        .file_stem()
        .ok_or_else(|| format!("invalid input path: {}", input_path.display()))?
        .to_string_lossy();
    let file_name = format!("{stem}-nobg.png");

    let dir = match output_dir {
        Some(dir) => PathBuf::from(dir),
        None => input_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".")),
    };

    Ok(dir.join(file_name))
}

fn to_data_url(img: &DynamicImage) -> Result<String, String> {
    to_data_url_sized(img, PREVIEW_MAX_DIM)
}

fn to_data_url_sized(img: &DynamicImage, max_dim: u32) -> Result<String, String> {
    let preview = if img.width() > max_dim || img.height() > max_dim {
        img.resize(max_dim, max_dim, FilterType::Triangle)
    } else {
        img.clone()
    };

    let mut bytes: Vec<u8> = Vec::new();
    preview
        .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
        .map_err(|e| format!("could not encode preview image: {e}"))?;

    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:image/png;base64,{encoded}"))
}
