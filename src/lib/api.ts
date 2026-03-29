import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface AsrStatus {
    configured: boolean;
}

export interface AudioDevice {
    id: string;
    name: string;
    is_default: boolean;
}

export interface SceneExample {
    input: string;
    output: string;
}

export interface PromptProfile {
    id: string;
    name: string;
    voice_aliases: string[];
    preset_key: string;
    goal: string;
    tone: string;
    format_style: string;
    preserve_rules: string[];
    glossary: string[];
    examples: SceneExample[];
    advanced_instruction: string;
    expert_mode: boolean;
    legacy_imported: boolean;
}

export interface LlmConfig {
    enabled: boolean;
    base_url: string;
    api_key: string;
    model: string;
    profiles: PromptProfile[];
    active_profile_id: string;
}

export interface ProxyConfig {
    enabled: boolean;
    url: string;
}

export interface OnlineAsrConfig {
    app_key: string;
    access_key: string;
    resource_id: string;
}

export interface SkillConfig {
    id: string;
    name: string;
    keywords: string;
    enabled: boolean;
    sub_commands: SkillSubCommandConfig[];
    browser_options?: BrowserSkillOptions | null;
    windows_options?: WindowsSkillOptions | null;
}

export interface SkillSubCommandConfig {
    id: string;
    name: string;
    keywords: string;
    enabled: boolean;
}

export interface BrowserSiteConfig {
    id: string;
    name: string;
    aliases: string;
    url: string;
    enabled: boolean;
}

export interface BrowserSkillOptions {
    llm_site_resolution_enabled: boolean;
    search_fallback_enabled: boolean;
    search_url_template: string;
    sites: BrowserSiteConfig[];
}

export interface WindowsTargetConfig {
    id: string;
    name: string;
    aliases: string;
    launch_kind: "command" | "shell";
    launch_target: string;
    launch_args: string[];
    enabled: boolean;
}

export interface WindowsSkillOptions {
    llm_target_resolution_enabled: boolean;
    targets: WindowsTargetConfig[];
}

export type SkillAgentMode = "skill" | "agent";

export interface SafetyRule {
    tool: string;
    action: "allow" | "deny";
    command_pattern?: string;
    path_scope?: string[];
}

export interface AgentConfig {
    mode: SkillAgentMode;
    enabled: boolean;
    thinking_level: string;
    max_iterations: number;
    provider_type: string;
    provider_base_url: string;
    provider_api_key: string;
    provider_model: string;
    confirm_dangerous: boolean;
    default_safety_policy: "confirm" | "deny" | "allow";
    safety_rules: SafetyRule[];
    system_prompt: string;
    auto_inject_env: boolean;
    persistent_context: string;
    context_history_count: number;
}

export interface AppConfig {
    trigger_mouse: boolean;
    trigger_toggle: boolean;
    online_asr_config: OnlineAsrConfig;
    input_device: string;
    llm_config: LlmConfig;
    proxy: ProxyConfig;
    skills: SkillConfig[];
    agent_config: AgentConfig;
}

export interface HistoryItem {
    id: string;
    timestamp: string;
    text: string;
    duration_ms: number;
}

export interface VoiceCommandFeedback {
    level: "success" | "error" | "info";
    message: string;
}

export type DictationIntent = "raw" | "polish" | "skill" | "agent" | "none";

export const api = {
    getConfig: () => invoke<AppConfig>("get_config"),
    takeRuntimeNotice: () => invoke<string | null>("take_runtime_notice"),
    saveConfig: (config: AppConfig) => invoke("save_config", { config }),
    getHistory: () => invoke<HistoryItem[]>("get_history"),
    clearHistory: () => invoke("clear_history"),
    deleteHistoryItem: (id: string) => invoke("delete_history_item", { id }),
    getAsrStatus: () => invoke<AsrStatus>("get_asr_status"),
    getInputDevices: () => invoke<AudioDevice[]>("get_input_devices"),
    getCurrentInputDevice: () => invoke<string>("get_current_input_device"),
    switchInputDevice: (deviceId: string) => invoke("switch_input_device", { deviceId }),
    startAudioTest: () => invoke("start_audio_test"),
    stopAudioTest: () => invoke("stop_audio_test"),
    testLlmConnection: (config: LlmConfig, proxy: ProxyConfig) => invoke<string>("test_llm_connection", { config, proxy }),
    getDefaultSceneTemplate: () => invoke<PromptProfile>("get_default_scene_template"),
    getDefaultSceneProfiles: () => invoke<PromptProfile[]>("get_default_scene_profiles"),
};

export const events = {
    onTranscriptionUpdate: (callback: (payload: HistoryItem) => void) => listen<HistoryItem>("transcription_update", (e) => callback(e.payload)),
    onRecordingStatus: (callback: (isRecording: boolean) => void) => listen<boolean>("recording_status", (e) => callback(e.payload)),
    onRecognitionProcessing: (callback: (isProcessing: boolean) => void) => listen<boolean>("recognition_processing", (e) => callback(e.payload)),
    onAudioLevel: (callback: (level: number) => void) => listen<number>("audio_level", (e) => callback(e.payload)),
    onLlmProcessing: (callback: (isProcessing: boolean) => void) => listen<boolean>("llm_processing", (e) => callback(e.payload)),
    onLlmError: (callback: (message: string) => void) => listen<string>("llm_error", (e) => callback(e.payload)),
    onMousePosition: (callback: (pos: { x: number; y: number }) => void) => listen<{ x: number; y: number }>("mouse_position", (e) => callback(e.payload)),
    onStreamUpdate: (callback: (text: string) => void) => listen<string>("stream_update", (e) => callback(e.payload)),
    onDictationIntent: (callback: (intent: DictationIntent) => void) =>
        listen<DictationIntent>("dictation_intent", (e) => callback(e.payload)),
    onConfigUpdated: (callback: (config: AppConfig) => void) => listen<AppConfig>("config_updated", (e) => callback(e.payload)),
    onVoiceCommandFeedback: (callback: (payload: VoiceCommandFeedback) => void) =>
        listen<VoiceCommandFeedback>("voice_command_feedback", (e) => callback(e.payload)),
};
