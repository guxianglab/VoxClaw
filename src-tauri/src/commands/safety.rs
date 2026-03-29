use crate::SafetySharedState;
use crate::state::StorageState;
use crate::storage;

#[tauri::command]
pub async fn respond_confirmation(
    id: String,
    decision: String,
    state: tauri::State<'_, SafetySharedState>,
) -> Result<(), String> {
    let hook_decision = match decision.as_str() {
        "allow" => crate::agent::tool::HookDecision::Allow,
        "deny" => crate::agent::tool::HookDecision::Block {
            reason: "\u{64cd}\u{4f5c}\u{5df2}\u{88ab}\u{7528}\u{6237}\u{62d2}\u{7edd}\u{3002}\u{8bf7}\u{5c1d}\u{8bd5}\u{5176}\u{4ed6}\u{65b9}\u{6848}\u{3002}".into(),
        },
        _ => return Err(format!("Unknown decision: {}", decision)),
    };
    if let Some(mut pending) = state.pending.lock().ok() {
        if let Some(tx) = pending.remove(&id) {
            let _ = tx.send(hook_decision);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn add_safety_rule(
    rule: storage::SafetyRule,
    state: tauri::State<'_, SafetySharedState>,
    storage: tauri::State<'_, StorageState>,
) -> Result<(), String> {
    // Add to in-memory rules for immediate use
    state.rules.lock().await.push(rule.clone());
    // Persist to disk
    let mut config = storage.load_config();
    config.agent_config.safety_rules.push(rule);
    storage.save_config(&config).map_err(|e| e.to_string())?;
    Ok(())
}
