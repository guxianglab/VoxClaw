use crate::storage::{LlmConfig, PromptProfile, ProxyConfig};

#[tauri::command]
pub async fn test_llm_connection(config: LlmConfig, proxy: ProxyConfig) -> Result<String, String> {
    crate::llm::test_connection(&config, &proxy)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_default_scene_profiles() -> Vec<PromptProfile> {
    crate::storage::default_scene_profiles()
}
