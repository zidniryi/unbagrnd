//! Publishes exported images into Android's shared `MediaStore`
//! (`Pictures/unbagrnd`) so they show up in Gallery/Photos right away.
//!
//! On desktop, writing the file next to the source photo (or into a chosen
//! output folder) is enough for it to be discoverable. Android's scoped
//! storage means a file written to the app's own private directory - which
//! is where exports otherwise land, see `output_path_for` in `commands.rs` -
//! is invisible to every other app, Gallery included, unless it's
//! registered with `MediaStore` explicitly. There's no such registration
//! step from pure Rust, so this talks to a small inlined Kotlin plugin
//! (`GalleryPlugin.kt`, compiled straight into the Android app module) via
//! Tauri's mobile plugin bridge.
//!
//! This also backs the "reveal in folder" buttons' Android fallback:
//! `tauri-plugin-opener`'s `revealItemInDir` is unimplemented on Android,
//! and there's no universal "show this file's folder" affordance on the
//! platform anyway, so [`open_last_in_gallery`] opens the most recently
//! published image directly in the user's photo viewer instead.

use std::sync::Mutex;

use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::{
    plugin::{Builder as PluginBuilder, PluginHandle, TauriPlugin},
    AppHandle, Manager, Runtime,
};

struct GalleryHandle<R: Runtime>(PluginHandle<R>);

/// The `content://` URI of the most recently published gallery image.
struct LastGalleryUri(Mutex<Option<String>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveRequest {
    bytes_base64: String,
    display_name: String,
    mime_type: String,
}

#[derive(Deserialize)]
struct SaveResponse {
    uri: String,
}

#[derive(Serialize)]
struct OpenRequest {
    uri: String,
}

#[derive(Deserialize)]
struct OpenResponse {}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    PluginBuilder::new("gallery")
        .setup(|app, api| {
            let handle = api
                .register_android_plugin("com.unbagrnd.app", "GalleryPlugin")
                .map_err(|e| e.to_string())?;
            app.manage(GalleryHandle(handle));
            app.manage(LastGalleryUri(Mutex::new(None)));
            Ok(())
        })
        .build()
}

/// Writes `bytes` into the public `Pictures/unbagrnd` collection and
/// remembers its URI for [`open_last_in_gallery`].
///
/// Best-effort by design: callers should log and otherwise ignore a
/// failure here rather than fail a whole export, since the file has
/// already been written to the app's own export directory regardless of
/// whether this publish step succeeds.
pub fn publish<R: Runtime>(
    app: &AppHandle<R>,
    display_name: &str,
    mime_type: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let handle = app.state::<GalleryHandle<R>>();
    let request = SaveRequest {
        bytes_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        display_name: display_name.to_string(),
        mime_type: mime_type.to_string(),
    };
    let response = handle
        .0
        .run_mobile_plugin::<SaveResponse>("saveToGallery", request)
        .map_err(|e| e.to_string())?;

    *app
        .state::<LastGalleryUri>()
        .0
        .lock()
        .map_err(|_| "gallery state lock poisoned".to_string())? = Some(response.uri);
    Ok(())
}

/// Opens the most recently published image in the user's photo
/// viewer/gallery app - see the module docs for why this exists.
pub fn open_last_in_gallery<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let uri = app
        .state::<LastGalleryUri>()
        .0
        .lock()
        .map_err(|_| "gallery state lock poisoned".to_string())?
        .clone()
        .ok_or_else(|| "no exported image to open yet".to_string())?;

    let handle = app.state::<GalleryHandle<R>>();
    handle
        .0
        .run_mobile_plugin::<OpenResponse>("openInGallery", OpenRequest { uri })
        .map(|_| ())
        .map_err(|e| e.to_string())
}
