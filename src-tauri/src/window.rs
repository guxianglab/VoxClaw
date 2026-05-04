use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::state::{DictationIntent, INDICATOR_BOTTOM_MARGIN, INDICATOR_COLLAPSED_HEIGHT,
    INDICATOR_EXPANDED_HEIGHT, INDICATOR_LOGICAL_WIDTH};

// ---------------------------------------------------------------------------
// Indicator window layout helpers
// ---------------------------------------------------------------------------

pub fn position_indicator_window<R: Runtime>(
    app_handle: &AppHandle<R>,
    logical_width: f64,
    logical_height: f64,
) {
    if let Some(window) = app_handle.get_webview_window("indicator") {
        if let Ok(Some(monitor)) = window.current_monitor() {
            let pos = monitor.position();
            let size = monitor.size();
            let scale_factor = monitor.scale_factor();

            let physical_center_x = pos.x as f64 + (size.width as f64 / 2.0);
            let physical_bottom_y = pos.y as f64 + size.height as f64;

            let window_x = physical_center_x - (logical_width * scale_factor / 2.0);
            let window_y = physical_bottom_y
                - ((logical_height + INDICATOR_BOTTOM_MARGIN) * scale_factor);

            let window_pos = tauri::PhysicalPosition::new(window_x as i32, window_y as i32);
            window.set_position(window_pos).ok();
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

/// Make the indicator window click-through (transparent to mouse events) or interactive.
/// On Windows, we toggle WS_EX_TRANSPARENT on the extended window style.
/// This MUST be called after every show/hide transition.
#[cfg(target_os = "windows")]
fn set_indicator_click_through(hwnd: windows::Win32::Foundation::HWND, click_through: bool) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE,
    };
    // WS_EX_TRANSPARENT (0x20): mouse events fall through to the window beneath.
    // WS_EX_LAYERED   (0x80000): required on some Windows versions for WS_EX_TRANSPARENT to work
    //                             on a WebView-hosted window.
    const WS_EX_LAYERED: isize     = 0x0008_0000;
    const WS_EX_TRANSPARENT: isize = 0x0000_0020;
    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let new_style = if click_through {
            ex_style | WS_EX_TRANSPARENT | WS_EX_LAYERED
        } else {
            (ex_style | WS_EX_LAYERED) & !WS_EX_TRANSPARENT
        };
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style);
    }
}

/// Apply initial click-through state to the indicator window right after creation.
/// Call this once during app setup so the window never blocks input while invisible.
pub fn init_indicator_click_through<R: Runtime>(app_handle: &AppHandle<R>) {
    #[cfg(target_os = "windows")]
    if let Some(window) = app_handle.get_webview_window("indicator") {
        if let Ok(hwnd) = window.hwnd() {
            use windows::Win32::Foundation::HWND;
            let hwnd_ptr: HWND = unsafe { std::mem::transmute(hwnd) };
            set_indicator_click_through(hwnd_ptr, true);
        }
    }
}

pub fn show_indicator_window<R: Runtime>(app_handle: &AppHandle<R>) {
    set_indicator_window_layout(app_handle, false);
    if let Some(window) = app_handle.get_webview_window("indicator") {
        #[cfg(target_os = "windows")]
        if let Ok(hwnd) = window.hwnd() {
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};
            unsafe {
                let hwnd_ptr: HWND = std::mem::transmute(hwnd);
                // First make it interactive (remove click-through), THEN show without activating.
                set_indicator_click_through(hwnd_ptr, false);
                let _ = ShowWindow(hwnd_ptr, SW_SHOWNOACTIVATE);
            }
            return;
        }

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

pub fn hide_indicator_window<R: Runtime>(app_handle: &AppHandle<R>) {
    if let Some(window) = app_handle.get_webview_window("indicator") {
        #[cfg(target_os = "windows")]
        if let Ok(hwnd) = window.hwnd() {
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
            unsafe {
                let hwnd_ptr: HWND = std::mem::transmute(hwnd);
                // Re-enable click-through BEFORE hiding so the brief transition
                // period cannot swallow any stray mouse events.
                set_indicator_click_through(hwnd_ptr, true);
                let _ = ShowWindow(hwnd_ptr, SW_HIDE);
            }
            return;
        }
        window.hide().ok();
    }
}

pub fn hide_main_window<R: Runtime>(app_handle: &AppHandle<R>) {
    if let Some(window) = app_handle.get_webview_window("main") {
        window.hide().ok();
    }
    hide_indicator_window(app_handle);
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
