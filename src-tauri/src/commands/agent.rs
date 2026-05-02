use crate::state::{AgentCancelState, StorageState};

/// Cancel any currently-running Agent task.
/// Calls `.cancel()` on the stored `CancellationToken`, causing `agent.process()` to return
/// `Err(AgentError::Cancelled)` at the next cancellation check-point.
#[tauri::command]
pub async fn cancel_agent(state: tauri::State<'_, AgentCancelState>) -> Result<(), String> {
    if let Ok(guard) = state.lock() {
        if let Some(token) = guard.as_ref() {
            token.cancel();
            println!("[CMD] cancel_agent: cancellation token triggered");
        } else {
            println!("[CMD] cancel_agent: no active agent to cancel");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Session commands (Phase 2)
// ---------------------------------------------------------------------------

fn data_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    app.path().app_data_dir().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn session_list(
    app: tauri::AppHandle,
) -> Result<Vec<crate::agent::session::SessionSummary>, String> {
    let dir = data_dir(&app)?;
    crate::agent::session::list_sessions(&dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn session_load(
    app: tauri::AppHandle,
    session_id: String,
) -> Result<Vec<crate::agent::core::message::AgentMessage>, String> {
    let dir = data_dir(&app)?;
    let store = crate::agent::session::SessionStore::open(&dir, &session_id)
        .map_err(|e| e.to_string())?;
    store.replay_messages().map_err(|e| e.to_string())
}

/// Start a brand-new session, returning the new session id. Persists no
/// entries — the next dictation cycle will populate it as messages flow.
#[tauri::command]
pub async fn session_new(
    app: tauri::AppHandle,
    storage: tauri::State<'_, StorageState>,
) -> Result<String, String> {
    let dir = data_dir(&app)?;
    let store = crate::agent::session::SessionStore::create(&dir, None, None)
        .map_err(|e| e.to_string())?;
    storage.set_current_session_id(Some(store.session_id.clone()));
    Ok(store.session_id)
}

/// Forget the current continuous-mode session id without deleting the file,
/// so the next utterance starts a fresh session.
#[tauri::command]
pub async fn session_clear_current(
    storage: tauri::State<'_, StorageState>,
) -> Result<(), String> {
    storage.set_current_session_id(None);
    Ok(())
}

#[tauri::command]
pub async fn session_current(
    storage: tauri::State<'_, StorageState>,
) -> Result<Option<String>, String> {
    Ok(storage.current_session_id())
}
