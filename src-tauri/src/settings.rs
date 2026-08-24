//! Small local settings file (default model, theme). Lives next to the
//! cached models in the app data directory; never leaves the device.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::models::{self, DEFAULT_MODEL_KEY};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub selected_model: String,
    /// One of "system", "light", "dark".
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            selected_model: DEFAULT_MODEL_KEY.to_string(),
            theme: "system".to_string(),
        }
    }
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("could not resolve the app data directory: {e}"))?;
    Ok(dir.join("settings.json"))
}

pub fn load_settings(app: &AppHandle) -> Result<Settings, String> {
    let path = settings_path(app)?;
    let Ok(bytes) = std::fs::read(&path) else {
        return Ok(Settings::default());
    };
    let mut settings: Settings = serde_json::from_slice(&bytes).unwrap_or_default();

    // A settings file naming a model that no longer exists in the catalog
    // (e.g. after an app update) falls back to the default rather than
    // leaving the app pointed at nothing.
    if models::find_model(&settings.selected_model).is_none() {
        settings.selected_model = DEFAULT_MODEL_KEY.to_string();
    }
    if !["system", "light", "dark"].contains(&settings.theme.as_str()) {
        settings.theme = "system".to_string();
    }
    Ok(settings)
}

async fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|e| format!("could not create the app data directory: {e}"))?;
    }
    let bytes = serde_json::to_vec_pretty(settings)
        .map_err(|e| format!("could not serialize settings: {e}"))?;
    tokio::fs::write(&path, bytes)
        .await
        .map_err(|e| format!("could not write settings: {e}"))?;
    Ok(())
}

pub async fn set_selected_model(app: &AppHandle, key: &str) -> Result<Settings, String> {
    if models::find_model(key).is_none() {
        return Err(format!("unknown model \"{key}\""));
    }
    let mut settings = load_settings(app)?;
    settings.selected_model = key.to_string();
    save(app, &settings).await?;
    Ok(settings)
}

pub async fn set_theme(app: &AppHandle, theme: &str) -> Result<Settings, String> {
    if !["system", "light", "dark"].contains(&theme) {
        return Err(format!(
            "unknown theme \"{theme}\" (expected system, light, or dark)"
        ));
    }
    let mut settings = load_settings(app)?;
    settings.theme = theme.to_string();
    save(app, &settings).await?;
    Ok(settings)
}
