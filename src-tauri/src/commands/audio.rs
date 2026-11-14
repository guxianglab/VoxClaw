use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager, Runtime};

use crate::asr::zipformer;
use crate::audio;
use crate::state::{AudioState, StorageState};
use crate::storage::AsrProviderKind;

fn zipformer_default_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("locate app_data_dir failed: {e}"))?;
    Ok(base.join("models").join(zipformer::download::ARCHIVE_DIR_NAME))
}

/// Download and extract the streaming Zipformer model into the default
/// location. Emits `asr_model_download` progress events and returns the model
/// directory. If Zipformer is the active provider, rebuilds it so dictation
/// picks up the model without a restart.
#[tauri::command]
pub async fn download_zipformer_model<R: Runtime>(
    app: AppHandle<R>,
    storage: tauri::State<'_, StorageState>,
    asr: tauri::State<'_, crate::state::AsrState>,
) -> Result<String, String> {
    let target = zipformer_default_dir(&app)?;
    let parent = target.parent().map(Path::to_path_buf).unwrap_or_default();
    let proxy = storage.load_config().proxy;
    let dir = zipformer::download::download_zipformer_model(&app, parent, proxy)
        .await
        .map_err(|e| e.to_string())?;

    let mut config = storage.load_config();
    config.asr.zipformer.model_dir = dir.display().to_string();
    storage.save_config(&config).map_err(|e| e.to_string())?;

    if config.asr.provider == AsrProviderKind::ZipformerStreaming {
        let provider = crate::asr::build_provider(&config.asr, &config.proxy)
            .map_err(|e| e.to_string())?;
        asr.replace(provider).map_err(|e| {
            format!("{e}
模型已下载并保存，但未能立即加载。请停止录音/会议后重试，或重启应用。")
        })?;
    }

    Ok(dir.display().to_string())
}

#[tauri::command]
pub fn get_input_devices() -> Vec<audio::AudioDevice> {
    audio::AudioService::get_input_devices()
}

#[tauri::command]
pub fn get_current_input_device(audio: tauri::State<AudioState>) -> String {
    if let Ok(audio) = audio.lock() {
        audio.get_current_device_name()
    } else {
        String::new()
    }
}

#[tauri::command]
pub fn switch_input_device(
    app: AppHandle,
    audio: tauri::State<AudioState>,
    storage: tauri::State<StorageState>,
    device_id: String,
) -> Result<(), String> {
    // Update audio service
    if let Ok(mut audio) = audio.lock() {
        audio
            .init_with_device(&device_id, app)
            .map_err(|e| e.to_string())?;
    } else {
        return Err("Failed to lock audio service".to_string());
    }

    // Save to config
    let mut config = storage.load_config();
    config.input_device = device_id;
    storage.save_config(&config).map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn start_audio_test(audio: tauri::State<AudioState>) -> Result<(), String> {
    if let Ok(mut audio) = audio.lock() {
        audio.start_test().map_err(|e| e.to_string())
    } else {
        Err("Failed to lock audio service".to_string())
    }
}

#[tauri::command]
pub fn stop_audio_test(audio: tauri::State<AudioState>) -> Result<(), String> {
    if let Ok(mut audio) = audio.lock() {
        audio.stop_test().map_err(|e| e.to_string())
    } else {
        Err("Failed to lock audio service".to_string())
    }
}

#[tauri::command]
pub async fn test_volcengine_connection(
    config: crate::storage::OnlineAsrConfig,
    proxy: crate::storage::ProxyConfig,
) -> Result<String, String> {
    crate::asr::volcengine::test_connection(&config, &proxy)
        .await
        .map_err(|e| e.to_string())
}
