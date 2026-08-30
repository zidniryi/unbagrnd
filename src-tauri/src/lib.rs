mod background;
mod bg_remove;
mod commands;
mod models;
mod refine;
mod settings;
mod system_usage;

use bg_remove::InferenceState;
use commands::LastResultState;
use system_usage::SystemUsageState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(InferenceState::new())
        .manage(SystemUsageState::new())
        .manage(LastResultState::new())
        .invoke_handler(tauri::generate_handler![
            commands::list_models,
            commands::get_settings,
            commands::set_selected_model,
            commands::set_theme,
            commands::set_export_format,
            commands::download_model,
            commands::clear_model,
            commands::clear_all_models,
            commands::remove_background_single,
            commands::remove_background_batch,
            commands::expand_batch_paths,
            commands::preview_image,
            commands::preview_background,
            commands::export_background,
            commands::preview_refine,
            commands::apply_refine,
            commands::undo_refine,
            commands::redo_refine,
            commands::export_refine,
            system_usage::get_system_usage,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
