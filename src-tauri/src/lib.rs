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
        .setup(|_app| {
            #[cfg(target_os = "android")]
            init_rustls_platform_verifier(_app);
            Ok(())
        })
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

// `reqwest`'s TLS backend (rustls-platform-verifier) needs to be handed the
// Android JVM/activity before it can verify any certificate, or it panics on
// first use (e.g. the model download). Tauri doesn't hand us the JNIEnv
// directly, so we reach it through the webview's JNI handle, which also
// means going through a different major version of the `jni` crate than the
// one wry/tao use internally - hence the raw-pointer round trip below.
#[cfg(target_os = "android")]
fn init_rustls_platform_verifier(app: &tauri::App) {
    use tauri::Manager;

    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.with_webview(|webview| {
        webview.jni_handle().exec(|env, activity, _webview| {
            let raw_env = env.get_raw() as *mut std::ffi::c_void;
            let raw_activity = activity.as_raw() as *mut std::ffi::c_void;

            let mut env = unsafe { jni::EnvUnowned::from_raw(raw_env as *mut jni::sys::JNIEnv) };
            let outcome = env
                .with_env(|env| {
                    let activity = unsafe {
                        jni::objects::JObject::from_raw(env, raw_activity as jni::sys::jobject)
                    };
                    rustls_platform_verifier::android::init_with_env(env, activity)
                })
                .into_outcome();

            if let jni::Outcome::Err(e) = outcome {
                eprintln!("failed to initialize rustls-platform-verifier: {e:?}");
            }
        });
    });
}
