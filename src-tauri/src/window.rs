use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::state::{DictationIntent, INDICATOR_BOTTOM_MARGIN, INDICATOR_COLLAPSED_HEIGHT,
    INDICATOR_EXPANDED_HEIGHT, INDICATOR_LOGICAL_WIDTH, InputListenerState};

// ---------------------------------------------------------------------------
// Indicator window layout helpers
// ---------------------------------------------------------------------------

pub fn position_indicator_window<R: Runtime>(
    app_handle: &AppHandle<R>,
    logical_width: f64,
    logical_height: f64,
) {
    if let Some(window) = app_handle.get_webview_window("indicator") {
        let listener = app_handle.state::<InputListenerState>();
        let (x, y) = listener.get_last_mouse_position();

        if let Ok(monitors) = app_handle.available_monitors() {
            for monitor in monitors {
                let pos = monitor.position();
                let size = monitor.size();

                let in_x = x >= pos.x as f64 && x < (pos.x + size.width as i32) as f64;
                let in_y = y >= pos.y as f64 && y < (pos.y + size.height as i32) as f64;

                if in_x && in_y {
                    let scale_factor = monitor.scale_factor();

                    let physical_center_x = pos.x as f64 + (size.width as f64 / 2.0);
                    let physical_bottom_y = pos.y as f64 + size.height as f64;

                    let window_x = physical_center_x - (logical_width * scale_factor / 2.0);
                    let window_y = physical_bottom_y
                        - ((logical_height + INDICATOR_BOTTOM_MARGIN) * scale_factor);

                    let window_pos = tauri::PhysicalPosition::new(window_x as i32, window_y as i32);
                    window.set_position(window_pos).ok();
                    break;
                }
            }
        }
    }
}

pub fn set_indicator_window_layout<R: Runtime>(app_handle: &AppHandle<R>, expanded: bool) {
    if let Some(window) = app_handle.get_webview_window("indicator") {
        let logical_height = if expanded {
            INDICATOR_EXPANDED_HEIGHT
        } else {
            INDICATOR_COLLAPSED_HEIGHT
        };

        window
            .set_size(tauri::LogicalSize::new(
                INDICATOR_LOGICAL_WIDTH,
                logical_height,
            ))
            .ok();
        position_indicator_window(app_handle, INDICATOR_LOGICAL_WIDTH, logical_height);
    }
}

// ---------------------------------------------------------------------------
// Window show / hide
// ---------------------------------------------------------------------------

pub fn show_indicator_window<R: Runtime>(app_handle: &AppHandle<R>) {
    set_indicator_window_layout(app_handle, false);
    if let Some(window) = app_handle.get_webview_window("indicator") {
        window.show().ok();
    }
}

pub fn show_main_window<R: Runtime>(app_handle: &AppHandle<R>) {
    if let Some(window) = app_handle.get_webview_window("main") {
        window.show().ok();
        window.unminimize().ok();
        window.set_focus().ok();
    }
}

pub fn hide_main_window<R: Runtime>(app_handle: &AppHandle<R>) {
    if let Some(window) = app_handle.get_webview_window("main") {
        window.hide().ok();
    }
    if let Some(indicator) = app_handle.get_webview_window("indicator") {
        indicator.hide().ok();
    }
}

// ---------------------------------------------------------------------------
// Event emitters
// ---------------------------------------------------------------------------

pub fn emit_dictation_intent<R: Runtime>(app_handle: &AppHandle<R>, intent: DictationIntent) {
    app_handle.emit("dictation_intent", intent.as_event()).ok();
}

pub fn emit_session_complete<R: Runtime>(app_handle: &AppHandle<R>) {
    app_handle.emit("session_complete", ()).ok();
}

pub fn emit_voice_command_feedback<R: Runtime>(
    app_handle: &AppHandle<R>,
    level: &str,
    message: impl Into<String>,
) {
    use crate::state::VoiceCommandFeedback;
    app_handle
        .emit(
            "voice_command_feedback",
            VoiceCommandFeedback {
                level: level.to_string(),
                message: message.into(),
            },
        )
        .ok();
}

// ---------------------------------------------------------------------------
// Config update helper
// ---------------------------------------------------------------------------

pub fn save_and_emit_config_update<R: Runtime>(
    app_handle: &AppHandle<R>,
    config: &crate::storage::AppConfig,
) -> Result<(), String> {
    let storage = app_handle.state::<crate::state::StorageState>();
    storage.save_config(config).map_err(|e| e.to_string())?;
    app_handle.emit("config_updated", config.clone()).ok();
    Ok(())
}

// ---------------------------------------------------------------------------
// LLM state helpers (used by skill engine)
// ---------------------------------------------------------------------------

pub fn set_browser_llm_state<R: Runtime>(app_handle: &AppHandle<R>, active: bool) {
    app_handle.emit("llm_processing", active).ok();
    let listener = app_handle.state::<InputListenerState>();
    listener
        .track_mouse_position
        .store(active, std::sync::atomic::Ordering::Relaxed);
    if active {
        show_indicator_window(app_handle);
    }
}

pub fn store_llm_cancel_token(
    llm_cancel: &crate::state::LlmCancelState,
    token: Option<tokio_util::sync::CancellationToken>,
) {
    if let Ok(mut guard) = llm_cancel.lock() {
        *guard = token;
    }
}
