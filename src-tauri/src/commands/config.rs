use crate::state::{AsrState, InputListenerState, StorageState};
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
    asr: tauri::State<AsrState>,
    config: AppConfig,
) -> Result<(), String> {
    let previous = state.load_config();

    // Update listener flags immediately (hot-reload)
    listener
        .enable_mouse
        .store(config.trigger_mouse, std::sync::atomic::Ordering::Relaxed);
    listener
        .enable_alt
        .store(config.trigger_toggle, std::sync::atomic::Ordering::Relaxed);

    // Rebuild ASR provider if relevant config changed.
    let asr_changed = !asr_config_eq(&previous.asr, &config.asr)
        || !proxy_config_eq(&previous.proxy, &config.proxy);
    if asr_changed {
        asr.replace(crate::asr::build_provider(&config.asr, &config.proxy));
    }

    state.save_config(&config).map_err(|e| e.to_string())
}

fn asr_config_eq(a: &crate::storage::AsrConfig, b: &crate::storage::AsrConfig) -> bool {
    a.provider == b.provider
        && a.volcengine.app_key == b.volcengine.app_key
        && a.volcengine.access_key == b.volcengine.access_key
        && a.volcengine.resource_id == b.volcengine.resource_id
        && a.sensevoice.model_dir == b.sensevoice.model_dir
        && a.sensevoice.language == b.sensevoice.language
        && a.sensevoice.use_gpu == b.sensevoice.use_gpu
}

fn proxy_config_eq(a: &crate::storage::ProxyConfig, b: &crate::storage::ProxyConfig) -> bool {
    a.enabled == b.enabled && a.url == b.url
}
