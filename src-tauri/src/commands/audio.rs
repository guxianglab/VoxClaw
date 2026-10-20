use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager, Runtime};

use crate::asr::zipformer;
use crate::audio;
use crate::state::{AudioState, StorageState};
use crate::storage::AsrProviderKind;

#[tauri::command]
pub fn get_asr_status(state: tauri::State<StorageState>) -> crate::state::AsrStatus {
    let config = state.load_config();
    let configured = match config.asr.provider {
        AsrProviderKind::Volcengine => {
            !config.asr.volcengine.app_key.is_empty()
                && !config.asr.volcengine.access_key.is_empty()
        }
        AsrProviderKind::ZipformerStreaming => zipformer::model::is_present(Path::new(
            &config.asr.zipformer.model_dir,
        )),
    };
    crate::state::AsrStatus { configured }
}

fn zipformer_default_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("locate app_data_dir failed: {e}"))?;
    Ok(base.join("models").join(zipformer::download::ARCHIVE_DIR_NAME))
}

/// Default model directory for Zipformer (`<app_data>/models/<archive>`).
#[tauri::command]
pub fn get_zipformer_default_dir<R: Runtime>(app: AppHandle<R>) -> Result<String, String> {
    Ok(zipformer_default_dir(&app)?.display().to_string())
}

#[tauri::command]
pub fn check_zipformer_model_present(model_dir: String) -> bool {
    !model_dir.is_empty() && zipformer::model::is_present(Path::new(&model_dir))
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
pub fn switch_input_device<R: Runtime>(
    app: AppHandle<R>,
    audio: tauri::State<AudioState>,
    storage: tauri::State<StorageState>,
    device_id: String,
) -> Result<(), String> {
    // Update audio service
    if let Ok(mut audio) = audio.lock() {
        audio
            .init_with_device(&device_id, app.clone())
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
    if let Ok(audio) = audio.lock() {
        audio.start_test().map_err(|e| e.to_string())
    } else {
        Err("Failed to lock audio service".to_string())
    }
}

#[tauri::command]
pub fn stop_audio_test(audio: tauri::State<AudioState>) -> Result<(), String> {
    if let Ok(audio) = audio.lock() {
        audio.stop_test().map_err(|e| e.to_string())
    } else {
        Err("Failed to lock audio service".to_string())
    }
}
