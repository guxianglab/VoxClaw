use crate::state::StorageState;
use crate::storage::HistoryItem;

#[tauri::command]
pub fn get_history(state: tauri::State<StorageState>) -> Vec<HistoryItem> {
    state.load_history()
}

#[tauri::command]
pub fn clear_history(state: tauri::State<StorageState>) -> Result<(), String> {
    state.clear_history().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_history_item(id: String, state: tauri::State<StorageState>) -> Result<(), String> {
    state.delete_history_item(id).map_err(|e| e.to_string())
}
