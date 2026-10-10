//! Tauri command handlers exposed to the frontend.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use base64::Engine;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_fs::FsExt;
use unbagrnd_core::bg_remove::{self, InferenceState};

use crate::background::{self, ShadowSpec};
use crate::matte::{self, EdgeSpec};
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

/// Set by [`cancel_batch`], polled between files by
/// [`remove_background_batch`]. A file already being processed always runs
/// to completion - inference can't be interrupted mid-run - so cancelling
/// takes effect at the next file boundary.
pub struct BatchCancelState(AtomicBool);

impl BatchCancelState {
    pub fn new() -> Self {
        Self(AtomicBool::new(false))
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

/// Like [`last_session`], but clones only what an export or edge adjustment
/// needs - the source path and working image - instead of the whole session
/// including its (potentially large) undo history.
fn last_current(app: &AppHandle) -> Result<(PathBuf, RgbaImage), String> {
    let state = app.state::<LastResultState>();
    let guard = state
        .0
        .lock()
        .map_err(|_| "result state lock was poisoned".to_string())?;
    guard
        .as_ref()
        .map(|s| (s.input_path.clone(), s.current.clone()))
        .ok_or_else(|| "no processed image to edit yet".to_string())
}

/// Replaces the working image with `new_current`, recording the previous
/// one for undo (capped at [`REFINE_UNDO_LIMIT`]) and clearing any redo
/// history. Returns the new working image.
fn commit_edit(app: &AppHandle, new_current: RgbaImage) -> Result<RgbaImage, String> {
    with_last_session(app, |session| {
        session.undo_stack.push(session.current.clone());
        if session.undo_stack.len() > REFINE_UNDO_LIMIT {
            session.undo_stack.remove(0);
        }
        session.redo_stack.clear();
        session.current = new_current;
        Ok(session.current.clone())
    })
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
    let input_path_buf = PathBuf::from(&input_path);
    let output_path = output_path_for(&app, &input_path_buf, output_dir.as_deref(), "-nobg", &format)?;

    tauri::async_runtime::spawn_blocking(move || -> Result<SingleResult, String> {
        let original = open_image(&app, &input_path)?;
        let before_data_url = to_data_url(&original)?;

        let inference = app.state::<InferenceState>();
        let after =
            bg_remove::remove_background(inference.inner(), spec, &model_path, &original)?;
        write_output(&after, &output_path, &format)?;
        publish_to_gallery(&app, &output_path, &format);

        {
            let result_state = app.state::<LastResultState>();
            let mut guard = result_state
                .0
                .lock()
                .map_err(|_| "result state lock was poisoned".to_string())?;
            *guard = Some(ResultSession {
                input_path: input_path_buf.clone(),
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
pub async fn preview_image(app: AppHandle, path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let original = open_image(&app, &path)?;
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
    let output_path = output_path_for(&app, &input_path, output_dir.as_deref(), "-bg", &format)?;

    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let composited = background::composite(&image, background_hex.as_deref(), shadow.as_ref())?;
        write_output(&composited, &output_path, &format)?;
        publish_to_gallery(&app, &output_path, &format);
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

    let preview = commit_edit(&app, new_current)?;
    tauri::async_runtime::spawn_blocking(move || to_data_url(&DynamicImage::ImageRgba8(preview)))
        .await
        .map_err(|e| format!("background task failed: {e}"))?
}

/// Applies edge `edges` (shift / smooth / feather) to the working image at
/// preview resolution - without committing - and returns it as a data URL,
/// for the refine panel's live preview while the sliders are being dragged.
/// The amounts are percentages of the image's longer side, so this matches
/// what [`apply_edges`] will produce at full resolution.
#[tauri::command]
pub async fn preview_edges(app: AppHandle, edges: EdgeSpec) -> Result<String, String> {
    edges.validate()?;
    let (_, image) = last_current(&app)?;
    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let preview_src = if image.width() > PREVIEW_MAX_DIM || image.height() > PREVIEW_MAX_DIM {
            DynamicImage::ImageRgba8(image)
                .resize(PREVIEW_MAX_DIM, PREVIEW_MAX_DIM, FilterType::Triangle)
                .to_rgba8()
        } else {
            image
        };
        let result = matte::apply_edges(&preview_src, &edges);
        to_data_url(&DynamicImage::ImageRgba8(result))
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

/// Commits `edges` at full resolution as a new entry in the refine undo
/// history (so undo/redo cover edge adjustments too), and returns a
/// preview-resolution data URL of the new result.
#[tauri::command]
pub async fn apply_edges(app: AppHandle, edges: EdgeSpec) -> Result<String, String> {
    edges.validate()?;
    let (_, image) = last_current(&app)?;
    let new_current = tauri::async_runtime::spawn_blocking(move || matte::apply_edges(&image, &edges))
        .await
        .map_err(|e| format!("background task failed: {e}"))?;

    let preview = commit_edit(&app, new_current)?;
    tauri::async_runtime::spawn_blocking(move || to_data_url(&DynamicImage::ImageRgba8(preview)))
        .await
        .map_err(|e| format!("background task failed: {e}"))?
}

/// Writes the working image's alpha channel as a black-and-white matte
/// (white = subject, black = background) at full resolution, next to (or
/// into `output_dir`, if given) the original source file, suffixed
/// `-mask`. Reflects any committed refine and edge edits. Returns the
/// written path.
#[tauri::command]
pub async fn export_mask(
    app: AppHandle,
    output_dir: Option<String>,
    export_format: Option<String>,
) -> Result<String, String> {
    let format = resolve_export_format(&app, export_format.as_deref())?;
    let (input_path, image) = last_current(&app)?;
    let output_path = output_path_for(&app, &input_path, output_dir.as_deref(), "-mask", &format)?;

    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let mask = matte::alpha_matte(&image);
        write_output(&mask, &output_path, &format)?;
        publish_to_gallery(&app, &output_path, &format);
        Ok(output_path.display().to_string())
    })
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
    let output_path = output_path_for(&app, &session.input_path, output_dir.as_deref(), "-refined", &format)?;

    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        write_output(&session.current, &output_path, &format)?;
        publish_to_gallery(&app, &output_path, &format);
        Ok(output_path.display().to_string())
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

/// Opens the most recently exported image in the system photo viewer.
///
/// This exists only as the Android fallback for the "reveal in folder"
/// buttons: `tauri-plugin-opener`'s `revealItemInDir` is unimplemented on
/// Android (it returns an error there), so the frontend calls this command
/// when that fails. It's a no-op error on every other platform, since
/// `revealItemInDir` already works there and this is never reached.
#[tauri::command]
pub async fn reveal_last_export_in_gallery(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        crate::android_gallery::open_last_in_gallery(&app)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Err("not supported on this platform".to_string())
    }
}

/// Options applied to every image in a batch run. All optional: with none
/// set, a batch behaves exactly as it always did (cutout only, saved as
/// `<name>-nobg.<format>`).
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BatchOptions {
    /// `#rrggbb` fill for the transparent area; `None` keeps it transparent.
    pub background_hex: Option<String>,
    /// Drop shadow rendered beneath each subject; `None` for no shadow.
    pub shadow: Option<ShadowSpec>,
    /// Output file name (without extension), with `{name}` replaced by the
    /// source file's name and `{index}` by its 1-based position in the
    /// batch. Empty/`None` means `{name}-nobg` (or `{name}-bg` when a
    /// background or shadow is applied).
    pub name_template: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSummary {
    /// Files attempted in this run (successes and per-file failures).
    pub processed: usize,
    /// True if the run stopped early because [`cancel_batch`] was called.
    pub cancelled: bool,
}

/// Asks the batch currently running in [`remove_background_batch`] to stop
/// before it starts its next file.
#[tauri::command]
pub fn cancel_batch(state: tauri::State<BatchCancelState>) {
    state.0.store(true, Ordering::SeqCst);
}

/// Removes the background from every image in `input_paths`, one at a
/// time, emitting a `batch-progress` event after each file completes (or
/// fails) so the UI can render a progress bar without blocking on the
/// whole batch. Each file's processing runs on a blocking-friendly worker
/// thread so the UI stays responsive throughout.
///
/// `positions`, `total` and `reserved_outputs` exist for retrying a subset
/// of an earlier run: `positions[i]` is `input_paths[i]`'s index in the
/// original batch (used for the progress event's `index` and the `{index}`
/// name token), `total` the original batch size, and `reserved_outputs` the
/// files that run already wrote, so a retry never overwrites them. Left
/// out, they default to a fresh batch of `input_paths`.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn remove_background_batch(
    app: AppHandle,
    input_paths: Vec<String>,
    positions: Option<Vec<usize>>,
    total: Option<usize>,
    reserved_outputs: Option<Vec<String>>,
    output_dir: Option<String>,
    model_key: Option<String>,
    export_format: Option<String>,
    options: Option<BatchOptions>,
) -> Result<BatchSummary, String> {
    let options = options.unwrap_or_default();
    // Fail the whole run up front on a bad color instead of once per file.
    if let Some(hex) = options.background_hex.as_deref() {
        background::parse_hex_color(hex)?;
    }
    let compositing = options.background_hex.is_some() || options.shadow.is_some();
    let template = options
        .name_template
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or(if compositing { "{name}-bg" } else { "{name}-nobg" })
        .to_string();

    let spec = resolve_model(&app, model_key.as_deref())?;
    let format = resolve_export_format(&app, export_format.as_deref())?;
    let model_path = models::ensure_model(&app, spec).await?;
    let input_paths = expand_paths(&input_paths)?;
    let positions = match positions {
        Some(positions) if positions.len() == input_paths.len() => positions,
        Some(_) => return Err("positions must have one entry per input path".to_string()),
        None => (0..input_paths.len()).collect(),
    };
    let total = total.unwrap_or(input_paths.len());
    let index_width = total.to_string().len();

    let cancel = app.state::<BatchCancelState>();
    cancel.0.store(false, Ordering::SeqCst);

    let mut taken: HashSet<PathBuf> = reserved_outputs
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .collect();
    let mut processed = 0;
    let mut cancelled = false;

    for (raw_input_path, position) in input_paths.into_iter().zip(positions) {
        if cancel.0.load(Ordering::SeqCst) {
            cancelled = true;
            break;
        }

        let input_path = PathBuf::from(&raw_input_path);
        let file_name = input_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| raw_input_path.clone());

        let planned = source_stem(&input_path).and_then(|stem| {
            let out_stem = render_name_template(&template, &stem, position + 1, index_width);
            output_path_for_stem(&app, &input_path, output_dir.as_deref(), &out_stem, &format)
                .map(|path| unique_output_path(path, &mut taken))
        });

        let file_result = match planned {
            Err(message) => BatchFileResult::Error { message },
            Ok(output_path) => {
                let app = app.clone();
                let model_path = model_path.clone();
                let raw_input_path = raw_input_path.clone();
                let format = format.clone();
                let options = options.clone();
                let outcome = tauri::async_runtime::spawn_blocking(
                    move || -> Result<(PathBuf, String), String> {
                        let original = open_image(&app, &raw_input_path)?;
                        let inference = app.state::<InferenceState>();
                        let mut result = bg_remove::remove_background(
                            inference.inner(),
                            spec,
                            &model_path,
                            &original,
                        )?;
                        if compositing {
                            result = background::composite(
                                &result,
                                options.background_hex.as_deref(),
                                options.shadow.as_ref(),
                            )?;
                        }
                        write_output(&result, &output_path, &format)?;
                        publish_to_gallery(&app, &output_path, &format);
                        let after_data_url = to_data_url_sized(
                            &DynamicImage::ImageRgba8(result),
                            BATCH_THUMB_MAX_DIM,
                        )?;
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
            }
        };

        processed += 1;
        let _ = app.emit(
            "batch-progress",
            BatchProgressEvent {
                index: position,
                total,
                file_name,
                result: file_result,
            },
        );
    }

    Ok(BatchSummary { processed, cancelled })
}

/// Packs `files` (typically a batch's outputs) into a ZIP archive at
/// `zip_path`, flat, under their own file names. Entries are stored rather
/// than deflated: PNG/WebP/SVG output is already compressed, so deflate
/// would burn CPU for no size win. Returns `zip_path`.
#[tauri::command]
pub async fn export_batch_zip(files: Vec<String>, zip_path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        write_zip(&files, Path::new(&zip_path))?;
        Ok(zip_path)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

fn write_zip(files: &[String], zip_path: &Path) -> Result<(), String> {
    if files.is_empty() {
        return Err("there are no files to add to the ZIP".to_string());
    }

    let file = std::fs::File::create(zip_path)
        .map_err(|e| format!("could not create {}: {e}", zip_path.display()))?;
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored);

    let mut used_names: HashSet<String> = HashSet::new();
    for raw in files {
        let path = Path::new(raw);
        if path == zip_path {
            continue;
        }
        let name = path
            .file_name()
            .ok_or_else(|| format!("invalid file path: {raw}"))?
            .to_string_lossy()
            .to_string();
        let entry_name = unique_zip_entry_name(&name, &mut used_names);

        let mut source = std::fs::File::open(path)
            .map_err(|e| format!("could not read {}: {e}", path.display()))?;
        writer
            .start_file(entry_name, options)
            .map_err(|e| format!("could not write ZIP entry: {e}"))?;
        std::io::copy(&mut source, &mut writer)
            .map_err(|e| format!("could not write {} into the ZIP: {e}", path.display()))?;
    }

    writer
        .finish()
        .map_err(|e| format!("could not finish the ZIP: {e}"))?;
    Ok(())
}

/// `name`, or `stem-2.ext`, `stem-3.ext`... if that entry name was already
/// used (two outputs with the same file name from different folders).
fn unique_zip_entry_name(name: &str, used: &mut HashSet<String>) -> String {
    if used.insert(name.to_string()) {
        return name.to_string();
    }
    let path = Path::new(name);
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2..)
        .map(|n| format!("{stem}-{n}{ext}"))
        .find(|candidate| used.insert(candidate.clone()))
        .expect("an unused name always exists")
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
    app: &AppHandle,
    input_path: &Path,
    output_dir: Option<&str>,
    suffix: &str,
    format: &str,
) -> Result<PathBuf, String> {
    let stem = source_stem(input_path)?;
    output_path_for_stem(app, input_path, output_dir, &format!("{stem}{suffix}"), format)
}

/// The source file's name without its extension.
fn source_stem(input_path: &Path) -> Result<String, String> {
    input_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .ok_or_else(|| format!("invalid input path: {}", input_path.display()))
}

/// Same directory rules as [`output_path_for`], for a caller that has
/// already built the complete output file stem (batch name templates).
fn output_path_for_stem(
    app: &AppHandle,
    input_path: &Path,
    output_dir: Option<&str>,
    file_stem: &str,
    format: &str,
) -> Result<PathBuf, String> {
    let file_name = format!("{file_stem}.{format}");

    let dir = match output_dir {
        Some(dir) => PathBuf::from(dir),
        None => match input_path.parent() {
            // A real, existing directory next to the source file - the
            // normal desktop case.
            Some(p) if p.as_os_str() != "" && p.is_dir() => p.to_path_buf(),
            // Either there's no real parent directory at all, or (Android)
            // `input_path` was actually a `content://` URI, which Rust's
            // `Path` happily parses into nonsense segments that don't
            // correspond to any real, writable location. Fall back to a
            // directory inside the app's own storage, which is always
            // writable without extra permissions.
            _ => {
                let dir = app
                    .path()
                    .app_data_dir()
                    .map_err(|e| format!("could not resolve a save directory: {e}"))?
                    .join("exports");
                std::fs::create_dir_all(&dir)
                    .map_err(|e| format!("could not create the exports directory: {e}"))?;
                dir
            }
        },
    };

    Ok(dir.join(file_name))
}

/// Expands a batch file-name template: `{index}` becomes the 1-based
/// position zero-padded to `width` digits, `{name}` the source file's stem.
/// The result is always safe to use as a single path component - anything a
/// file system or a path separator could misread is replaced - so a
/// template can never write outside the chosen folder.
fn render_name_template(template: &str, stem: &str, number: usize, width: usize) -> String {
    // `{index}` first: the stem is substituted last so a source file that
    // happens to be named "{index}" isn't expanded a second time.
    let rendered = template
        .replace("{index}", &format!("{number:0width$}"))
        .replace("{name}", stem);

    let cleaned: String = rendered
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "image".to_string()
    } else {
        // Leave generous room under the common 255-byte file name limit
        // for the extension and a `-N` de-duplication suffix.
        trimmed.chars().take(120).collect()
    }
}

/// Reserves `path` in `taken`, or - if another file in this batch already
/// claimed it - the first free `<stem>-2.<ext>`, `<stem>-3.<ext>`, ...
/// Without this, two sources with the same name from different folders
/// (or a template without `{name}`/`{index}`) would silently overwrite each
/// other in one output folder.
fn unique_output_path(path: PathBuf, taken: &mut HashSet<PathBuf>) -> PathBuf {
    if taken.insert(path.clone()) {
        return path;
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|n| path.with_file_name(format!("{stem}-{n}{ext}")))
        .find(|candidate| taken.insert(candidate.clone()))
        .expect("an unused name always exists")
}

/// Reads an image file's raw bytes, then decodes them.
///
/// Goes through the `fs` plugin rather than `std::fs` directly because on
/// Android, the file/photo picker hands back a `content://` URI rather than
/// a real filesystem path - only the plugin (via the OS's ContentResolver)
/// knows how to turn that into bytes.
fn open_image(app: &AppHandle, raw_path: &str) -> Result<DynamicImage, String> {
    let file_path: tauri_plugin_fs::FilePath = raw_path
        .parse()
        .unwrap_or_else(|e: std::convert::Infallible| match e {});
    let bytes = app
        .fs()
        .read(file_path)
        .map_err(|e| format!("could not read image: {e}"))?;
    decode_image_bytes(&bytes)
}

/// Decodes image bytes by sniffing their actual content rather than
/// trusting a file extension. Browsers and chat apps routinely save images
/// with a mismatched extension (a WebP saved as `.jpg` is common), and
/// `image::open`'s extension-based guess fails outright on those with a
/// confusing decoder error instead of just reading the file.
fn decode_image_bytes(bytes: &[u8]) -> Result<DynamicImage, String> {
    image::load_from_memory(bytes).map_err(|e| format!("could not read image: {e}"))
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

/// On Android, additionally publishes the file just written at `path` into
/// the shared `Pictures/unbagrnd` gallery collection, so it shows up in
/// Gallery/Photos without the user having to go digging through the app's
/// private storage for it - see `android_gallery` for why that's needed at
/// all. A no-op on every other platform, where writing next to the source
/// file (or into the user's chosen folder) is already enough.
///
/// Best-effort: failures are logged and otherwise swallowed rather than
/// failing the export, since the file itself was already written
/// successfully regardless of whether this extra publish step succeeds.
/// "svg" is skipped - it's not a photo Gallery apps know how to render.
#[cfg(target_os = "android")]
fn publish_to_gallery(app: &AppHandle, path: &Path, format: &str) {
    if format == "svg" {
        return;
    }
    let mime_type = if format == "webp" { "image/webp" } else { "image/png" };
    let Some(display_name) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    match std::fs::read(path) {
        Ok(bytes) => {
            if let Err(e) = crate::android_gallery::publish(app, display_name, mime_type, &bytes) {
                eprintln!("could not publish exported image to the gallery: {e}");
            }
        }
        Err(e) => eprintln!("could not read exported image back for the gallery: {e}"),
    }
}

#[cfg(not(target_os = "android"))]
fn publish_to_gallery(_app: &AppHandle, _path: &Path, _format: &str) {}

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
        // `.jpg` is common). `decode_image_bytes` must decode by the real
        // magic bytes, not fail the way `image::open`'s extension-based
        // guess does on a file like this.
        let path = TempPath::new("actually-webp.jpg");
        let image = sample_image();
        image.save_with_format(&path.0, ImageFormat::WebP).unwrap();

        let bytes = std::fs::read(&path.0).unwrap();
        let decoded = decode_image_bytes(&bytes).unwrap().to_rgba8();
        assert_eq!(decoded, image);
    }

    #[test]
    fn name_template_expands_tokens_and_pads_the_index() {
        assert_eq!(render_name_template("{name}-nobg", "photo", 3, 2), "photo-nobg");
        assert_eq!(render_name_template("{index}_{name}", "photo", 3, 3), "003_photo");
        assert_eq!(render_name_template("shot-{index}", "photo", 12, 2), "shot-12");
    }

    #[test]
    fn name_template_cannot_escape_the_output_folder() {
        for template in ["../{name}", "..\\{name}", "a/b/{name}", "C:{name}"] {
            let name = render_name_template(template, "photo", 1, 1);
            assert!(
                !name.contains('/') && !name.contains('\\') && !name.contains(':'),
                "{template:?} produced {name:?}"
            );
        }
        // A degenerate template still yields a usable file name.
        assert_eq!(render_name_template("...", "photo", 1, 1), "image");
        // The stem is inserted verbatim, never re-expanded as a token.
        assert_eq!(render_name_template("{index}-{name}", "{index}", 7, 1), "7-{index}");
    }

    #[test]
    fn unique_output_path_suffixes_collisions_within_a_batch() {
        let mut taken = HashSet::new();
        let first = unique_output_path(PathBuf::from("/out/a-nobg.png"), &mut taken);
        let second = unique_output_path(PathBuf::from("/out/a-nobg.png"), &mut taken);
        let third = unique_output_path(PathBuf::from("/out/a-nobg.png"), &mut taken);
        let other = unique_output_path(PathBuf::from("/out/b-nobg.png"), &mut taken);

        assert_eq!(first, PathBuf::from("/out/a-nobg.png"));
        assert_eq!(second, PathBuf::from("/out/a-nobg-2.png"));
        assert_eq!(third, PathBuf::from("/out/a-nobg-3.png"));
        assert_eq!(other, PathBuf::from("/out/b-nobg.png"));
    }

    #[test]
    fn write_zip_packs_files_and_renames_duplicate_names() {
        use std::io::Read;

        let unique = format!(
            "unbagrnd-zip-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let (dir_a, dir_b) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();
        let (file_a, file_b) = (dir_a.join("same.png"), dir_b.join("same.png"));
        std::fs::write(&file_a, b"first").unwrap();
        std::fs::write(&file_b, b"second").unwrap();
        let zip_path = root.join("out.zip");

        let files = vec![file_a.display().to_string(), file_b.display().to_string()];
        write_zip(&files, &zip_path).unwrap();

        let mut archive = zip::ZipArchive::new(std::fs::File::open(&zip_path).unwrap()).unwrap();
        assert_eq!(archive.len(), 2);
        let mut contents = Vec::new();
        for name in ["same.png", "same-2.png"] {
            let mut entry = archive.by_name(name).unwrap();
            let mut data = String::new();
            entry.read_to_string(&mut data).unwrap();
            contents.push(data);
        }
        assert_eq!(contents, ["first", "second"]);

        assert!(write_zip(&[], &zip_path).is_err());
        let _ = std::fs::remove_dir_all(&root);
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
