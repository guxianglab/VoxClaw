use tauri::{AppHandle, Runtime};

use crate::window as win;

#[tauri::command]
pub fn set_indicator_window_expanded<R: Runtime>(
    app_handle: AppHandle<R>,
    expanded: bool,
) -> Result<(), String> {
    win::set_indicator_window_layout(&app_handle, expanded);
    Ok(())
}

/// Called by the indicator JS when it wants to hide the window.
/// This ensures WS_EX_TRANSPARENT is restored before the window disappears,
/// so it can never accidentally swallow mouse events while transitioning.
#[tauri::command]
pub fn hide_indicator<R: Runtime>(app_handle: AppHandle<R>) -> Result<(), String> {
    win::hide_indicator_window(&app_handle);
    Ok(())
}
