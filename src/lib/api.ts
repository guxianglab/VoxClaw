import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface AudioDevice {
    id: string;
    name: string;
    is_default: boolean;
}

export interface PromptProfile {
    id: string;
    name: string;
    goal: string;
    tone: string;
    format_style: string;
    preserve_rules: string[];
    glossary: string[];
    advanced_instruction: string;
}

export type LlmProviderType = "openai_compatible" | "anthropic";

export interface LlmConfig {
    enabled: boolean;
    provider_type: LlmProviderType;
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
    api_key: string;
    app_key: string;
    access_key: string;
    resource_id: string;
}

export type AsrProviderKind = "volcengine" | "zipformer_streaming";

export interface AsrConfig {
    provider: AsrProviderKind;
    volcengine: OnlineAsrConfig;
    zipformer: { model_dir: string };
}

export interface AgentConfig {
    thinking_level: string;
    max_iterations: number;
    system_prompt: string;
    persistent_context: string;
    context_history_count: number;
    continuous_mode: boolean;
}

export interface AppConfig {
    trigger_mouse: boolean;
    trigger_toggle: boolean;
    asr: AsrConfig;
    input_device: string;
    llm_config: LlmConfig;
    proxy: ProxyConfig;
    agent_config: AgentConfig;
}

export interface HistoryItem {
    id: string;
    timestamp: string;
    text: string;
    duration_ms: number;
}

// ---------------------------------------------------------------------------
// Meeting mode
// ---------------------------------------------------------------------------

export type MeetingStatus = "recording" | "transcribed" | "summarized" | "failed";

export interface MeetingSummary {
    title: string;
    key_points: string[];
    todos: string[];
    decisions: string[];
}

export interface MeetingRecord {
    id: string;
    started_at: string;
    ended_at?: string | null;
    duration_ms: number;
    asr_provider: string;
    status: MeetingStatus;
    transcript: string;
    summary?: MeetingSummary | null;
    last_error?: string | null;
}

export interface MeetingSummaryItem {
    id: string;
    started_at: string;
    duration_ms: number;
    status: MeetingStatus;
    title: string;
}

export interface MeetingActiveInfo {
    session_id: string;
    started_at: string;
    asr_provider: string;
}

export interface MeetingStatusEvent {
    state: "recording" | "finalizing" | "idle";
    session_id?: string | null;
}

export interface MeetingPartialEvent {
    session_id: string;
    text: string;
}

export interface MeetingUpdatedEvent {
    id: string;
    summarizing: boolean;
}

export const api = {
    getConfig: () => invoke<AppConfig>("get_config"),
    takeRuntimeNotice: () => invoke<string | null>("take_runtime_notice"),
    saveConfig: (config: AppConfig) => invoke<string | null>("save_config", { config }),
    getHistory: () => invoke<HistoryItem[]>("get_history"),
    clearHistory: () => invoke("clear_history"),
    deleteHistoryItem: (id: string) => invoke("delete_history_item", { id }),
    downloadZipformerModel: () => invoke<string>("download_zipformer_model"),
    getInputDevices: () => invoke<AudioDevice[]>("get_input_devices"),
    getCurrentInputDevice: () => invoke<string>("get_current_input_device"),
    switchInputDevice: (deviceId: string) => invoke("switch_input_device", { deviceId }),
    startAudioTest: () => invoke("start_audio_test"),
    stopAudioTest: () => invoke("stop_audio_test"),
    testVolcengineConnection: (config: OnlineAsrConfig, proxy: ProxyConfig) =>
        invoke<string>("test_volcengine_connection", { config, proxy }),
    testLlmConnection: (config: LlmConfig, proxy: ProxyConfig) => invoke<string>("test_llm_connection", { config, proxy }),
    getDefaultSceneProfiles: () => invoke<PromptProfile[]>("get_default_scene_profiles"),
    // Meeting mode
    startMeeting: () => invoke<MeetingActiveInfo>("start_meeting"),
    stopMeeting: () => invoke<MeetingRecord>("stop_meeting"),
    getActiveMeeting: () => invoke<MeetingActiveInfo | null>("get_active_meeting"),
    listMeetings: () => invoke<MeetingSummaryItem[]>("list_meetings"),
    getMeeting: (id: string) => invoke<MeetingRecord | null>("get_meeting", { id }),
    deleteMeeting: (id: string) => invoke("delete_meeting", { id }),
    summarizeMeeting: (id: string) => invoke<MeetingRecord>("summarize_meeting", { id }),
    sessionNew: () => invoke<string>("session_new"),
};

export const events = {
    onTranscriptionUpdate: (callback: (payload: HistoryItem) => void) => listen<HistoryItem>("transcription_update", (e) => callback(e.payload)),
    onAudioLevel: (callback: (level: number) => void) => listen<number>("audio_level", (e) => callback(e.payload)),
    onLlmError: (callback: (message: string) => void) => listen<string>("llm_error", (e) => callback(e.payload)),
    onAsrModelDownload: (callback: (payload: AsrModelDownloadEvent) => void) =>
        listen<AsrModelDownloadEvent>("asr_model_download", (e) => callback(e.payload)),
    onMeetingStatus: (callback: (payload: MeetingStatusEvent) => void) =>
        listen<MeetingStatusEvent>("meeting_status", (e) => callback(e.payload)),
    onMeetingPartial: (callback: (payload: MeetingPartialEvent) => void) =>
        listen<MeetingPartialEvent>("meeting_partial", (e) => callback(e.payload)),
    onMeetingAudioLevel: (callback: (level: number) => void) =>
        listen<number>("meeting_audio_level", (e) => callback(e.payload)),
    onMeetingUpdated: (callback: (payload: MeetingUpdatedEvent) => void) =>
        listen<MeetingUpdatedEvent>("meeting_updated", (e) => callback(e.payload)),
};

export type AsrModelDownloadEvent =
    | { phase: "started"; total_files: number }
    | { phase: "file"; name: string; index: number; total: number; downloaded: number; size: number | null }
    | { phase: "finished"; dir: string }
    | { phase: "failed"; message: string };
