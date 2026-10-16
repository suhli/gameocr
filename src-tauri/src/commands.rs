use crate::{
    capture::{self, Preview},
    model::ModelEntry,
    service::{ModelStatus, Performance, Service, Snapshot},
    settings::Settings,
    window::{self, WindowInfo},
};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;
use tauri::State;
type AppState<'a> = State<'a, Arc<Service>>;
type Reply<T> = Result<T, String>;
#[tauri::command]
pub fn get_adaptive_statistics(
    state: AppState<'_>,
) -> gameocr_core::adaptive::profiles::LearningSummary {
    state.adaptive_profiles.summary()
}
#[tauri::command]
pub async fn clear_adaptive_profiles(state: AppState<'_>) -> Reply<()> {
    let profiles = state.adaptive_profiles.clone();
    tokio::task::spawn_blocking(move || {
        profiles.clear();
        profiles.flush()
    })
    .await
    .map_err(err)?
    .map_err(err)
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
#[tauri::command]
pub async fn list_windows(show_all: bool) -> Reply<Vec<WindowInfo>> {
    tokio::task::spawn_blocking(move || window::list(show_all))
        .await
        .map_err(err)?
        .map_err(err)
}
#[tauri::command]
pub async fn select_window(state: AppState<'_>, id: String) -> Reply<WindowInfo> {
    let s = state.inner().clone();
    tokio::task::spawn_blocking(move || -> Reply<WindowInfo> {
        let window = window::list(true)
            .map_err(err)?
            .into_iter()
            .find(|w| w.id == id)
            .ok_or("窗口已失效，请刷新".to_string())?;
        window.validate().map_err(err)?;
        s.reset_target();
        s.capture.lock().stop();
        *s.selected.lock() = Some(window.clone());
        let mut settings = s.settings.read().clone();
        settings.recent_window = Some(format!("{}|{}", window.process_name, window.title));
        s.update_settings(settings).map_err(err)?;
        Ok(window)
    })
    .await
    .map_err(err)?
}
#[tauri::command]
pub async fn list_adb_devices(state: AppState<'_>) -> Reply<Vec<capture::adb::AdbDevice>> {
    let path = state.settings.read().adb_path.clone();
    capture::adb::devices(&path).await.map_err(err)
}
#[tauri::command]
pub async fn connect_adb(state: AppState<'_>, address: String) -> Reply<String> {
    if address.is_empty() || address.starts_with('-') || address.chars().any(char::is_whitespace) {
        return Err("请输入有效的 ADB 地址 host:port".into());
    }
    let path = state.settings.read().adb_path.clone();
    capture::adb::run(&path, &["connect", &address])
        .await
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .map_err(err)
}
#[tauri::command]
pub async fn start_capture(state: AppState<'_>) -> Reply<()> {
    let s = state.inner().clone();
    tokio::task::spawn_blocking(move || s.start_capture())
        .await
        .map_err(err)?
        .map_err(err)
}
#[tauri::command]
pub fn pause_capture(state: AppState<'_>) -> Reply<()> {
    state.stop_ocr();
    state.capture.lock().pause().map_err(err)
}
#[tauri::command]
pub fn resume_capture(state: AppState<'_>) -> Reply<()> {
    state.capture.lock().resume().map_err(err)
}
#[tauri::command]
pub async fn stop_capture(state: AppState<'_>) -> Reply<()> {
    let s = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        s.reset_target();
        s.capture.lock().stop();
    })
    .await
    .map_err(err)
}
#[tauri::command]
pub async fn get_capture_preview(state: AppState<'_>) -> Reply<Option<Preview>> {
    let f = state.shared.preview_frame.lock().clone();
    tokio::task::spawn_blocking(move || f.map(|f| capture::preview(&f)).transpose())
        .await
        .map_err(err)?
        .map_err(err)
}
#[tauri::command]
pub fn start_ocr(state: AppState<'_>) -> Reply<()> {
    state.start_ocr().map_err(err)
}
#[tauri::command]
pub fn stop_ocr(state: AppState<'_>) {
    state.stop_ocr();
}
#[tauri::command]
pub async fn recognize_once(state: AppState<'_>) -> Reply<()> {
    state.inner().clone().recognize(true).await.map_err(err)
}
#[tauri::command]
pub async fn list_models(state: AppState<'_>) -> Reply<Vec<ModelEntry>> {
    let s = state.inner().clone();
    tokio::task::spawn_blocking(move || s.list_models())
        .await
        .map_err(err)?
        .map_err(err)
}
#[tauri::command]
pub async fn download_model(state: AppState<'_>, id: String) -> Reply<()> {
    let s = state.settings.read().clone();
    let result = state
        .models
        .download(
            s.model_dir,
            id.clone(),
            s.download_mirror,
            state.app.clone(),
        )
        .await
        .map_err(err);
    if let Err(message) = &result {
        let _ = state.app.emit(
            "model-download-progress",
            serde_json::json!({"id":id,"state":"failed","message":message}),
        );
    }
    result
}
#[tauri::command]
pub async fn import_model(state: AppState<'_>, model: String, mmproj: String) -> Reply<ModelEntry> {
    let manager = state.models.clone();
    let dir = state.settings.read().model_dir.clone();
    tokio::task::spawn_blocking(move || {
        manager.import(&dir, &PathBuf::from(model), &PathBuf::from(mmproj))
    })
    .await
    .map_err(err)?
    .map_err(err)
}
#[tauri::command]
pub async fn import_onnx_model(
    state: AppState<'_>,
    variant: String,
    detector: String,
    recognizer: String,
    dictionary: String,
) -> Reply<ModelEntry> {
    let _transition = state
        .lifecycle
        .try_lock()
        .map_err(|_| "模型正在切换".to_string())?;
    let dir = state.settings.read().model_dir.clone();
    state
        .models
        .import_onnx(
            dir,
            variant,
            [detector.into(), recognizer.into(), dictionary.into()],
            state.app.clone(),
        )
        .await
        .map_err(err)
}
#[tauri::command]
pub async fn load_model(state: AppState<'_>, id: String) -> Reply<()> {
    state.inner().clone().load_model(id).await.map_err(err)
}
#[tauri::command]
pub async fn unload_model(state: AppState<'_>) -> Reply<()> {
    state.unload_model().await.map_err(err)
}
#[tauri::command]
pub async fn delete_model(state: AppState<'_>, id: String) -> Reply<()> {
    let _transition = state
        .lifecycle
        .try_lock()
        .map_err(|_| "模型正在切换".to_string())?;
    if state.model_status.lock().status != "unloaded" {
        return Err("删除模型前请先卸载模型".into());
    }
    let manager = state.models.clone();
    let dir = state.settings.read().model_dir.clone();
    tokio::task::spawn_blocking(move || manager.delete(&dir, &id))
        .await
        .map_err(err)?
        .map_err(err)
}
#[tauri::command]
pub fn get_settings(state: AppState<'_>) -> Settings {
    state.settings.read().clone()
}
#[tauri::command]
pub fn update_settings(state: AppState<'_>, settings: Settings) -> Reply<()> {
    state.update_settings(settings).map_err(err)
}
#[tauri::command]
pub fn get_performance(state: AppState<'_>) -> Performance {
    state.performance.lock().clone()
}
#[tauri::command]
pub fn get_model_status(state: AppState<'_>) -> ModelStatus {
    state.model_status.lock().clone()
}
#[tauri::command]
pub fn get_history(state: AppState<'_>) -> Vec<Snapshot> {
    state.history.lock().iter().cloned().collect()
}
#[tauri::command]
pub fn get_status(state: AppState<'_>) -> serde_json::Value {
    serde_json::json!({"capture":*state.shared.status.lock(),"ocr":*state.ocr_status.lock(),"selected":*state.selected.lock(),"warning":state.settings_error})
}
#[tauri::command]
pub async fn export_result(state: AppState<'_>, frame_id: u64, path: String) -> Reply<()> {
    let result = state
        .history
        .lock()
        .iter()
        .find(|s| s.result.frame_id == frame_id)
        .map(|s| s.result.clone())
        .ok_or("此结果已超出历史保留范围".to_string())?;
    tokio::task::spawn_blocking(move || crate::settings::atomic_json(&PathBuf::from(path), &result))
        .await
        .map_err(err)?
        .map_err(err)
}
#[tauri::command]
pub async fn save_screenshot(state: AppState<'_>, path: String, recognized: bool) -> Reply<()> {
    let frame = if recognized {
        state.recognized_frame.lock().clone()
    } else {
        state.shared.preview_frame.lock().clone()
    }
    .ok_or("暂无截图".to_string())?;
    tokio::task::spawn_blocking(move || frame.image.save_with_format(path, image::ImageFormat::Png))
        .await
        .map_err(err)?
        .map_err(err)
}
#[tauri::command]
pub async fn get_original_screenshot(state: AppState<'_>, recognized: bool) -> Reply<String> {
    use base64::Engine;
    let frame = if recognized {
        state.recognized_frame.lock().clone()
    } else {
        state.shared.preview_frame.lock().clone()
    }
    .ok_or("暂无截图".to_string())?;
    tokio::task::spawn_blocking(move || -> Reply<String> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        frame
            .image
            .write_to(&mut bytes, image::ImageFormat::Png)
            .map_err(err)?;
        Ok(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
        ))
    })
    .await
    .map_err(err)?
}
