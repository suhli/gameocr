pub mod adaptive;
mod capture;
mod commands;
pub mod cuda_runtime;
pub mod executor;
mod model;
mod ocr;
mod service;
mod settings;
pub mod translation;
mod window;
use std::sync::Arc;
use tauri::Manager;
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            window::start_identity_monitor()?;
            let state = service::Service::new(app.handle().clone())?;
            state.launch();
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_windows,
            commands::select_window,
            commands::list_adb_devices,
            commands::connect_adb,
            commands::start_capture,
            commands::pause_capture,
            commands::resume_capture,
            commands::stop_capture,
            commands::get_capture_preview,
            commands::start_ocr,
            commands::stop_ocr,
            commands::recognize_once,
            commands::list_models,
            commands::download_model,
            commands::import_model,
            commands::import_onnx_model,
            commands::load_model,
            commands::unload_model,
            commands::delete_model,
            commands::get_settings,
            commands::get_adaptive_statistics,
            commands::clear_adaptive_profiles,
            commands::update_settings,
            commands::get_performance,
            commands::get_model_status,
            commands::get_cuda_runtime_status,
            commands::cancel_cuda_runtime_download,
            commands::get_history,
            commands::get_status,
            commands::export_result,
            commands::save_screenshot,
            commands::get_original_screenshot
            ,commands::translation_settings,commands::update_translation_settings,commands::translation_status,
            commands::translation_models,commands::download_translation_model,commands::cancel_translation_download,
            commands::import_translation_model,commands::delete_translation_model,commands::load_translation_model,
            commands::unload_translation_model,commands::translate_once,commands::translation_history,
            commands::clear_translation_cache,commands::confirm_translation_context,commands::export_translation
            ,commands::import_translation_profiles,commands::export_translation_profiles
        ])
        .build(tauri::generate_context!())
        .expect("GameOCR 无法启动；需要 Windows 11 和系统 WebView2 Runtime");
    app.run(|handle, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            let state = handle.state::<Arc<service::Service>>().inner().clone();
            if !state.shutdown.load(std::sync::atomic::Ordering::Acquire) {
                api.prevent_exit();
                let app = handle.clone();
                tauri::async_runtime::spawn(async move {
                    state.close().await;
                    app.exit(0);
                });
            }
        }
    });
}
