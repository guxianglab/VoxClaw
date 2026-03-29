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
