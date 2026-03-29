use crate::state::AgentCancelState;

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
