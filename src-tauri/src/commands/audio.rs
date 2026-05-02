use tauri::{AppHandle, Manager, Runtime};

use crate::audio;
use crate::state::{AudioState, StorageState};

#[tauri::command]
pub fn get_asr_status(state: tauri::State<StorageState>) -> Result<crate::state::AsrStatus, String> {
    let config = state.load_config();
    let configured = match config.asr.provider {
        crate::storage::AsrProviderKind::Volcengine => {
            !config.asr.volcengine.app_key.is_empty()
                && !config.asr.volcengine.access_key.is_empty()
        }
        crate::storage::AsrProviderKind::SenseVoiceOnnx => {
            !config.asr.sensevoice.model_dir.is_empty()
                && crate::asr::sensevoice::model::is_present(std::path::Path::new(
                    &config.asr.sensevoice.model_dir,
                ))
        }
    };
    Ok(crate::state::AsrStatus { configured })
}

/// Default location for the bundled SenseVoice model files. Created lazily on
/// first download.
#[tauri::command]
pub fn get_sensevoice_default_dir<R: Runtime>(app: AppHandle<R>) -> Result<String, String> {
    let dir = sensevoice_default_dir(&app)?;
    Ok(dir.display().to_string())
}

#[tauri::command]
pub fn check_sensevoice_model_present(model_dir: String) -> bool {
    if model_dir.is_empty() {
        return false;
    }
    crate::asr::sensevoice::model::is_present(std::path::Path::new(&model_dir))
}

/// Trigger a (possibly resumable) download of the SenseVoice model into
/// `model_dir`. Emits `asr_model_download` events with progress. Returns the
/// final directory path so the frontend can persist it back into config.
#[tauri::command]
pub async fn download_sensevoice_model<R: Runtime>(
    app: AppHandle<R>,
    storage: tauri::State<'_, StorageState>,
    asr: tauri::State<'_, crate::state::AsrState>,
    model_dir: Option<String>,
) -> Result<String, String> {
    let target = match model_dir {
        Some(s) if !s.is_empty() => std::path::PathBuf::from(s),
        _ => sensevoice_default_dir(&app)?,
    };

    let proxy = storage.load_config().proxy;
    let final_dir =
        crate::asr::sensevoice::download::download_model(&app, target, proxy)
            .await
            .map_err(|e| e.to_string())?;

    // Persist the path back into config and rebuild the provider.
    let mut config = storage.load_config();
    config.asr.sensevoice.model_dir = final_dir.display().to_string();
    storage.save_config(&config).map_err(|e| e.to_string())?;

    if matches!(
        config.asr.provider,
        crate::storage::AsrProviderKind::SenseVoiceOnnx
    ) {
        asr.replace(crate::asr::build_provider(&config.asr, &config.proxy));
    }

    Ok(final_dir.display().to_string())
}

fn sensevoice_default_dir<R: Runtime>(app: &AppHandle<R>) -> Result<std::path::PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("locate app_data_dir failed: {e}"))?;
    Ok(base.join("models").join("SenseVoiceSmall-onnx"))
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
