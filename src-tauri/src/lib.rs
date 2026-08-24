mod bg_remove;
mod commands;
mod models;
mod settings;
mod system_usage;

use bg_remove::InferenceState;
use system_usage::SystemUsageState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(InferenceState::new())
        .manage(SystemUsageState::new())
        .invoke_handler(tauri::generate_handler![
            commands::list_models,
            commands::get_settings,
            commands::set_selected_model,
            commands::set_theme,
            commands::download_model,
            commands::clear_model,
            commands::clear_all_models,
            commands::remove_background_single,
            commands::remove_background_batch,
            system_usage::get_system_usage,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
