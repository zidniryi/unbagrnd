mod bg_remove;
mod commands;
mod model;

use bg_remove::InferenceState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(InferenceState::new())
        .invoke_handler(tauri::generate_handler![
            commands::model_status,
            commands::download_model,
            commands::remove_background_single,
            commands::remove_background_batch,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
