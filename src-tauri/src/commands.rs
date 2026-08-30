//! Tauri command handlers exposed to the frontend.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use base64::Engine;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, RgbaImage};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::background::{self, ShadowSpec};
use crate::bg_remove::{self, InferenceState};
use crate::models::{self, ModelInfo, ModelSpec};
use crate::refine::{self, Stroke};
use crate::settings::{self, Settings};

/// Preview images sent back to the frontend are capped to this size on
/// their longest edge so before/after thumbnails stay a few hundred KB
/// instead of multiple megabytes over IPC. The saved output file on disk
/// is always full resolution regardless of this cap.
const PREVIEW_MAX_DIM: u32 = 1024;

/// Refine undo history is capped so a long editing session on a large
/// photo doesn't grow memory use without bound — each entry is a full
/// extra copy of the working image.
const REFINE_UNDO_LIMIT: usize = 15;

/// Holds the state of the most recent single-image background removal, so
/// the "edit background" and "refine" panels can recomposite it — color
/// fill, drop shadow, brush erase/restore — without re-running inference or
/// re-decoding a possibly-lossy saved file. Single-image only: batch
/// results aren't editable this way.
#[derive(Clone)]
pub struct ResultSession {
    /// The source file this result came from, used to derive output
    /// filenames for anything the editors write.
    pub input_path: PathBuf,
    /// The raw input photo, fully opaque, at full resolution. One of the
    /// two "restore to" targets in the refine panel.
    pub original: RgbaImage,
    /// The model's own cutout, as produced by `remove_background`, fixed
    /// for the lifetime of this session. The other "restore to" target.
    pub start: RgbaImage,
    /// The working image: `start`, plus any refine edits committed via
    /// `apply_refine` since. This is what the background editor and
    /// exports operate on.
    pub current: RgbaImage,
    /// Previous values of `current`, most recent last, for `undo_refine`.
    pub undo_stack: Vec<RgbaImage>,
    /// Values popped off `undo_stack`, for `redo_refine`. Cleared whenever
    /// a new edit is applied.
    pub redo_stack: Vec<RgbaImage>,
}

pub struct LastResultState(Mutex<Option<ResultSession>>);

impl LastResultState {
    pub fn new() -> Self {
        Self(Mutex::new(None))
    }
}

fn last_session(app: &AppHandle) -> Result<ResultSession, String> {
    let state = app.state::<LastResultState>();
    let guard = state
        .0
        .lock()
        .map_err(|_| "result state lock was poisoned".to_string())?;
    guard
        .clone()
        .ok_or_else(|| "no processed image to edit yet".to_string())
}

/// Runs `f` against the live session under its lock, for callers that need
/// to mutate it in place (refine's apply/undo/redo).
fn with_last_session<T>(
    app: &AppHandle,
    f: impl FnOnce(&mut ResultSession) -> Result<T, String>,
) -> Result<T, String> {
    let state = app.state::<LastResultState>();
    let mut guard = state
        .0
        .lock()
        .map_err(|_| "result state lock was poisoned".to_string())?;
    let session = guard
        .as_mut()
        .ok_or_else(|| "no processed image to edit yet".to_string())?;
    f(session)
}

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

/// Resolves an export format from the frontend (which may omit it, meaning
/// "use the user's default") into a validated entry of [`settings::EXPORT_FORMATS`].
fn resolve_export_format(app: &AppHandle, export_format: Option<&str>) -> Result<String, String> {
    let format = match export_format {
        Some(format) => format.to_string(),
        None => settings::load_settings(app)?.export_format,
    };
    if !settings::EXPORT_FORMATS.contains(&format.as_str()) {
        return Err(format!(
            "unknown export format \"{format}\" (expected one of {:?})",
            settings::EXPORT_FORMATS
        ));
    }
    Ok(format)
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
pub async fn set_export_format(app: AppHandle, format: String) -> Result<Settings, String> {
    settings::set_export_format(&app, &format).await
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
    export_format: Option<String>,
) -> Result<SingleResult, String> {
    let spec = resolve_model(&app, model_key.as_deref())?;
    let format = resolve_export_format(&app, export_format.as_deref())?;
    let model_path = models::ensure_model(&app, spec).await?;
    let input_path = PathBuf::from(input_path);
    let output_path = output_path_for(&input_path, output_dir.as_deref(), "-nobg", &format)?;

    tauri::async_runtime::spawn_blocking(move || -> Result<SingleResult, String> {
        let original = open_image(&input_path)?;
        let before_data_url = to_data_url(&original)?;

        let inference = app.state::<InferenceState>();
        let after =
            bg_remove::remove_background(inference.inner(), spec, &model_path, &original)?;
        write_output(&after, &output_path, &format)?;

        {
            let result_state = app.state::<LastResultState>();
            let mut guard = result_state
                .0
                .lock()
                .map_err(|_| "result state lock was poisoned".to_string())?;
            *guard = Some(ResultSession {
                input_path: input_path.clone(),
                original: original.to_rgba8(),
                start: after.clone(),
                current: after.clone(),
                undo_stack: Vec::new(),
                redo_stack: Vec::new(),
            });
        }

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
        let original = open_image(&path)?;
        to_data_url_sized(&original, BATCH_THUMB_MAX_DIM)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

/// Recomposites the most recent single-image background-removal result
/// (color fill + drop shadow) at preview resolution and returns it as a
/// data URL, for the "edit background" panel's live preview. `None` for
/// `background_hex` means a transparent canvas (checkerboard); `None` for
/// `shadow` means no shadow.
#[tauri::command]
pub async fn preview_background(
    app: AppHandle,
    background_hex: Option<String>,
    shadow: Option<ShadowSpec>,
) -> Result<String, String> {
    let image = last_session(&app)?.current;
    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let preview_src = DynamicImage::ImageRgba8(image)
            .resize(PREVIEW_MAX_DIM, PREVIEW_MAX_DIM, FilterType::Triangle)
            .to_rgba8();
        let composited = background::composite(&preview_src, background_hex.as_deref(), shadow.as_ref())?;
        to_data_url(&DynamicImage::ImageRgba8(composited))
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

/// Recomposites the most recent single-image background-removal result at
/// full resolution (color fill + drop shadow) and writes it next to (or
/// into `output_dir`, if given) the original source file, suffixed `-bg`.
/// Returns the written path.
#[tauri::command]
pub async fn export_background(
    app: AppHandle,
    output_dir: Option<String>,
    background_hex: Option<String>,
    shadow: Option<ShadowSpec>,
    export_format: Option<String>,
) -> Result<String, String> {
    let format = resolve_export_format(&app, export_format.as_deref())?;
    let session = last_session(&app)?;
    let (input_path, image) = (session.input_path, session.current);
    let output_path = output_path_for(&input_path, output_dir.as_deref(), "-bg", &format)?;

    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let composited = background::composite(&image, background_hex.as_deref(), shadow.as_ref())?;
        write_output(&composited, &output_path, &format)?;
        Ok(output_path.display().to_string())
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

/// Picks the "restore to" target the refine panel's `restore_to` string
/// names: `"original"` is the raw input photo (fully opaque); anything else
/// (in practice just `"start"`) is the model's own cutout as it stood when
/// this session began.
fn restore_source(session: &ResultSession, restore_to: &str) -> RgbaImage {
    if restore_to == "original" {
        session.original.clone()
    } else {
        session.start.clone()
    }
}

/// Recomposites the current refine state with `strokes` applied — without
/// committing them — at preview resolution, and returns it as a data URL,
/// for the refine panel's live preview while the user is still dragging.
#[tauri::command]
pub async fn preview_refine(
    app: AppHandle,
    strokes: Vec<Stroke>,
    mode: String,
    restore_to: String,
) -> Result<String, String> {
    let session = last_session(&app)?;
    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let source = restore_source(&session, &restore_to);
        let current_preview = DynamicImage::ImageRgba8(session.current)
            .resize(PREVIEW_MAX_DIM, PREVIEW_MAX_DIM, FilterType::Triangle)
            .to_rgba8();
        let source_preview = DynamicImage::ImageRgba8(source)
            .resize(PREVIEW_MAX_DIM, PREVIEW_MAX_DIM, FilterType::Triangle)
            .to_rgba8();
        let result = refine::apply_strokes(&current_preview, &source_preview, &strokes, &mode);
        to_data_url(&DynamicImage::ImageRgba8(result))
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

/// Commits `strokes` at full resolution: applies them to the working
/// image, pushes the previous state onto the undo stack (clearing any redo
/// history), and returns a preview-resolution data URL of the new result.
#[tauri::command]
pub async fn apply_refine(
    app: AppHandle,
    strokes: Vec<Stroke>,
    mode: String,
    restore_to: String,
) -> Result<String, String> {
    let session = last_session(&app)?;
    let new_current = tauri::async_runtime::spawn_blocking(move || -> RgbaImage {
        let source = restore_source(&session, &restore_to);
        refine::apply_strokes(&session.current, &source, &strokes, &mode)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?;

    let preview = with_last_session(&app, |session| {
        session.undo_stack.push(session.current.clone());
        if session.undo_stack.len() > REFINE_UNDO_LIMIT {
            session.undo_stack.remove(0);
        }
        session.redo_stack.clear();
        session.current = new_current;
        Ok(session.current.clone())
    })?;
    tauri::async_runtime::spawn_blocking(move || to_data_url(&DynamicImage::ImageRgba8(preview)))
        .await
        .map_err(|e| format!("background task failed: {e}"))?
}

/// Steps the working image one entry back in its undo history (there is
/// none right after opening the refine panel, only after at least one
/// `apply_refine`), pushing the current state onto the redo stack. Returns
/// a preview-resolution data URL of the restored state.
#[tauri::command]
pub async fn undo_refine(app: AppHandle) -> Result<String, String> {
    let preview = with_last_session(&app, |session| {
        let previous = session.undo_stack.pop().ok_or_else(|| "nothing to undo".to_string())?;
        session.redo_stack.push(session.current.clone());
        session.current = previous;
        Ok(session.current.clone())
    })?;
    tauri::async_runtime::spawn_blocking(move || to_data_url(&DynamicImage::ImageRgba8(preview)))
        .await
        .map_err(|e| format!("background task failed: {e}"))?
}

/// The inverse of [`undo_refine`]: re-applies the most recently undone
/// state, pushing the current one back onto the undo stack.
#[tauri::command]
pub async fn redo_refine(app: AppHandle) -> Result<String, String> {
    let preview = with_last_session(&app, |session| {
        let next = session.redo_stack.pop().ok_or_else(|| "nothing to redo".to_string())?;
        session.undo_stack.push(session.current.clone());
        session.current = next;
        Ok(session.current.clone())
    })?;
    tauri::async_runtime::spawn_blocking(move || to_data_url(&DynamicImage::ImageRgba8(preview)))
        .await
        .map_err(|e| format!("background task failed: {e}"))?
}

/// Writes the working image (any committed refine edits, with no
/// background fill or shadow) at full resolution, next to (or into
/// `output_dir`, if given) the original source file, suffixed `-refined`.
/// Returns the written path.
#[tauri::command]
pub async fn export_refine(
    app: AppHandle,
    output_dir: Option<String>,
    export_format: Option<String>,
) -> Result<String, String> {
    let format = resolve_export_format(&app, export_format.as_deref())?;
    let session = last_session(&app)?;
    let output_path = output_path_for(&session.input_path, output_dir.as_deref(), "-refined", &format)?;

    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        write_output(&session.current, &output_path, &format)?;
        Ok(output_path.display().to_string())
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
    export_format: Option<String>,
) -> Result<(), String> {
    let spec = resolve_model(&app, model_key.as_deref())?;
    let format = resolve_export_format(&app, export_format.as_deref())?;
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
            let format = format.clone();
            let outcome = tauri::async_runtime::spawn_blocking(
                move || -> Result<(PathBuf, String), String> {
                    let output_path =
                        output_path_for(&input_path, output_dir.as_deref(), "-nobg", &format)?;
                    let original = open_image(&input_path)?;
                    let inference = app.state::<InferenceState>();
                    let after = bg_remove::remove_background(
                        inference.inner(),
                        spec,
                        &model_path,
                        &original,
                    )?;
                    write_output(&after, &output_path, &format)?;
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

/// Resolves where a processed image should be written: into `output_dir` if
/// the caller picked one, otherwise next to the source file. Either way the
/// file is named `<original-stem><suffix>.<format>` — `suffix` is `-nobg`
/// for a plain background-removal result or `-bg` for one written by the
/// background editor, and `format` is one of [`settings::EXPORT_FORMATS`],
/// as validated by [`resolve_export_format`].
fn output_path_for(
    input_path: &Path,
    output_dir: Option<&str>,
    suffix: &str,
    format: &str,
) -> Result<PathBuf, String> {
    let stem = input_path
        .file_stem()
        .ok_or_else(|| format!("invalid input path: {}", input_path.display()))?
        .to_string_lossy();
    let file_name = format!("{stem}{suffix}.{format}");

    let dir = match output_dir {
        Some(dir) => PathBuf::from(dir),
        None => input_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".")),
    };

    Ok(dir.join(file_name))
}

/// Decodes an image file by sniffing its actual content rather than
/// trusting its extension. Browsers and chat apps routinely save images
/// with a mismatched extension (a WebP saved as `.jpg` is common), and
/// `image::open`'s extension-based guess fails outright on those with a
/// confusing decoder error instead of just reading the file.
fn open_image(path: &Path) -> Result<DynamicImage, String> {
    image::ImageReader::open(path)
        .map_err(|e| format!("could not read image: {e}"))?
        .with_guessed_format()
        .map_err(|e| format!("could not read image: {e}"))?
        .decode()
        .map_err(|e| format!("could not read image: {e}"))
}

/// Writes the background-removed image to `path` in `format` (one of
/// [`settings::EXPORT_FORMATS`], as validated by [`resolve_export_format`]).
///
/// "svg" is the odd one out: there's no vector data to export here — the
/// model output is a raster alpha mask — so it means what it does in most
/// background-removal tools: an SVG document whose sole content is the PNG
/// re-embedded as a base64 data URI `<image>`, sized to the original pixel
/// dimensions. This still buys the user a format that scales cleanly in
/// vector-aware tools (browsers, design software, print pipelines) without
/// the lossy resampling a raster resize would need. "webp" is written
/// lossless, so it preserves the alpha channel exactly like PNG does, just
/// smaller on disk.
fn write_output(image: &RgbaImage, path: &Path, format: &str) -> Result<(), String> {
    match format {
        "svg" => {
            let mut png_bytes: Vec<u8> = Vec::new();
            image
                .write_to(&mut std::io::Cursor::new(&mut png_bytes), ImageFormat::Png)
                .map_err(|e| format!("could not encode output image: {e}"))?;
            let encoded = base64::engine::general_purpose::STANDARD.encode(png_bytes);
            let (width, height) = (image.width(), image.height());
            let svg = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                 <svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
                 viewBox=\"0 0 {width} {height}\">\n\
                 <image width=\"{width}\" height=\"{height}\" \
                 href=\"data:image/png;base64,{encoded}\"/>\n\
                 </svg>\n"
            );
            std::fs::write(path, svg).map_err(|e| format!("could not write output image: {e}"))
        }
        "webp" => image
            .save_with_format(path, ImageFormat::WebP)
            .map_err(|e| format!("could not write output image: {e}")),
        _ => image
            .save_with_format(path, ImageFormat::Png)
            .map_err(|e| format!("could not write output image: {e}")),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn sample_image() -> RgbaImage {
        RgbaImage::from_fn(3, 2, |x, y| {
            Rgba([x as u8 * 10, y as u8 * 10, 0, 128])
        })
    }

    /// A scratch file path under the OS temp dir, unique to the calling
    /// test and cleaned up on drop.
    struct TempPath(PathBuf);

    impl TempPath {
        fn new(name: &str) -> Self {
            let unique = format!(
                "unbagrnd-test-{}-{}-{name}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            Self(std::env::temp_dir().join(unique))
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn write_output_png_round_trips() {
        let path = TempPath::new("out.png");
        let image = sample_image();

        write_output(&image, &path.0, "png").unwrap();

        let decoded = image::open(&path.0).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), image.dimensions());
        assert_eq!(decoded, image);
    }

    #[test]
    fn write_output_webp_round_trips() {
        let path = TempPath::new("out.webp");
        let image = sample_image();

        write_output(&image, &path.0, "webp").unwrap();

        let decoded = image::open(&path.0).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), image.dimensions());
        assert_eq!(decoded, image);
    }

    #[test]
    fn open_image_sniffs_content_instead_of_trusting_a_mismatched_extension() {
        // Chat apps and browsers routinely save images under an extension
        // that doesn't match their actual encoding (a WebP saved as
        // `.jpg` is common). `open_image` must decode by the real magic
        // bytes, not fail the way `image::open`'s extension-based guess
        // does on a file like this.
        let path = TempPath::new("actually-webp.jpg");
        let image = sample_image();
        image.save_with_format(&path.0, ImageFormat::WebP).unwrap();

        let decoded = open_image(&path.0).unwrap().to_rgba8();
        assert_eq!(decoded, image);
    }

    #[test]
    fn write_output_svg_embeds_the_png_as_a_data_uri() {
        let path = TempPath::new("out.svg");
        let image = sample_image();

        write_output(&image, &path.0, "svg").unwrap();

        let svg = std::fs::read_to_string(&path.0).unwrap();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("width=\"3\" height=\"2\""));

        let marker = "href=\"data:image/png;base64,";
        let start = svg.find(marker).expect("svg should embed a data: URI") + marker.len();
        let end = svg[start..].find('"').expect("closing quote") + start;
        let encoded = &svg[start..end];

        let png_bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("embedded data should be valid base64");
        let decoded = image::load_from_memory_with_format(&png_bytes, ImageFormat::Png)
            .expect("embedded data should decode as PNG")
            .to_rgba8();
        assert_eq!(decoded, image);
    }
}
