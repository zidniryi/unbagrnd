#[cfg(target_os = "android")]
mod android_gallery;
mod background;
mod commands;
mod matte;
mod models;
mod refine;
mod settings;
mod system_usage;

use commands::{BatchCancelState, LastResultState};
use system_usage::SystemUsageState;
use unbagrnd_core::bg_remove::InferenceState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init());
    #[cfg(target_os = "android")]
    {
        builder = builder.plugin(android_gallery::init());
    }
    builder
        .manage(InferenceState::new())
        .manage(SystemUsageState::new())
        .manage(LastResultState::new())
        .manage(BatchCancelState::new())
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
            commands::preview_edges,
            commands::apply_edges,
            commands::export_mask,
            commands::cancel_batch,
            commands::export_batch_zip,
            commands::reveal_last_export_in_gallery,
            system_usage::get_system_usage,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
