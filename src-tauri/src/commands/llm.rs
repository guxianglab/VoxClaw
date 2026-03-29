use crate::storage::{LlmConfig, ProxyConfig};

#[tauri::command]
pub async fn test_llm_connection(config: LlmConfig, proxy: ProxyConfig) -> Result<String, String> {
    crate::llm::test_connection(&config, &proxy)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_default_scene_template() -> crate::storage::PromptProfile {
    crate::storage::blank_scene_template()
}

#[tauri::command]
pub fn get_default_scene_profiles() -> Vec<crate::storage::PromptProfile> {
    crate::storage::default_scene_profiles()
}
