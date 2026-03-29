use crate::state::{InputListenerState, StorageState};
use crate::storage::AppConfig;

#[tauri::command]
pub fn get_config(state: tauri::State<StorageState>) -> AppConfig {
    state.load_config()
}

#[tauri::command]
pub fn take_runtime_notice(state: tauri::State<StorageState>) -> Option<String> {
    state.take_runtime_notice()
}

#[tauri::command]
pub fn save_config(
    state: tauri::State<StorageState>,
    listener: tauri::State<InputListenerState>,
    config: AppConfig,
) -> Result<(), String> {
    // Update listener flags immediately (hot-reload)
    listener
        .enable_mouse
        .store(config.trigger_mouse, std::sync::atomic::Ordering::Relaxed);
    listener
        .enable_alt
        .store(config.trigger_toggle, std::sync::atomic::Ordering::Relaxed);

    state.save_config(&config).map_err(|e| e.to_string())
}
