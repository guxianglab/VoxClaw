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

export type AsrProviderKind = "volcengine" | "sense_voice_onnx";

export interface SenseVoiceOnnxConfig {
    model_dir: string;
    language: string;
    use_gpu: boolean;
}

export interface AsrConfig {
    provider: AsrProviderKind;
    volcengine: OnlineAsrConfig;
    sensevoice: SenseVoiceOnnxConfig;
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

export interface CompactionSettings {
    enabled: boolean;
    reserve_tokens: number;
    keep_recent_tokens: number;
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
    continuous_mode: boolean;
    compaction: CompactionSettings;
}

export interface AppConfig {
    trigger_mouse: boolean;
    trigger_toggle: boolean;
    asr: AsrConfig;
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

// ---------------------------------------------------------------------------
// Meeting mode
// ---------------------------------------------------------------------------

export type MeetingAudioSource = "mic_only" | "loopback_only" | "mic_and_loopback";

export type MeetingStatus =
    | "recording"
    | "finalizing"
    | "raw_only"
    | "corrected"
    | "summarized"
    | "failed";

export interface MeetingSegment {
    start_ms: number;
    end_ms: number;
    text: string;
    speaker?: string | null;
}

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
    audio_source: MeetingAudioSource;
    asr_provider: string;
    status: MeetingStatus;
    segments: MeetingSegment[];
    raw_text: string;
    corrected_text?: string | null;
    summary?: MeetingSummary | null;
    last_error?: string | null;
    draft_audio_path?: string | null;
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
    include_system_audio: boolean;
}

export interface MeetingStatusEvent {
    state: "recording" | "finalizing" | "idle";
    session_id?: string | null;
}

export interface MeetingPartialEvent {
    session_id: string;
    text: string;
}

export interface MeetingFinalizedEvent {
    id: string;
}

export interface MeetingLlmProgressEvent {
    id: string;
    stage: "correcting" | "correction_done" | "summarising" | "done" | "failed";
    error?: string | null;
}

export const api = {
    getConfig: () => invoke<AppConfig>("get_config"),
    takeRuntimeNotice: () => invoke<string | null>("take_runtime_notice"),
    saveConfig: (config: AppConfig) => invoke("save_config", { config }),
    getHistory: () => invoke<HistoryItem[]>("get_history"),
    clearHistory: () => invoke("clear_history"),
    deleteHistoryItem: (id: string) => invoke("delete_history_item", { id }),
    getAsrStatus: () => invoke<AsrStatus>("get_asr_status"),
    getSenseVoiceDefaultDir: () => invoke<string>("get_sensevoice_default_dir"),
    checkSenseVoiceModelPresent: (modelDir: string) =>
        invoke<boolean>("check_sensevoice_model_present", { modelDir }),
    downloadSenseVoiceModel: (modelDir?: string) =>
        invoke<string>("download_sensevoice_model", { modelDir: modelDir ?? null }),
    getInputDevices: () => invoke<AudioDevice[]>("get_input_devices"),
    getCurrentInputDevice: () => invoke<string>("get_current_input_device"),
    switchInputDevice: (deviceId: string) => invoke("switch_input_device", { deviceId }),
    startAudioTest: () => invoke("start_audio_test"),
    stopAudioTest: () => invoke("stop_audio_test"),
    testLlmConnection: (config: LlmConfig, proxy: ProxyConfig) => invoke<string>("test_llm_connection", { config, proxy }),
    getDefaultSceneTemplate: () => invoke<PromptProfile>("get_default_scene_template"),
    getDefaultSceneProfiles: () => invoke<PromptProfile[]>("get_default_scene_profiles"),
    // Meeting mode
    startMeeting: (includeSystemAudio?: boolean) =>
        invoke<MeetingActiveInfo>("start_meeting", { includeSystemAudio: includeSystemAudio ?? false }),
    stopMeeting: () => invoke<MeetingRecord>("stop_meeting"),
    getActiveMeeting: () => invoke<MeetingActiveInfo | null>("get_active_meeting"),
    listMeetings: () => invoke<MeetingSummaryItem[]>("list_meetings"),
    getMeeting: (id: string) => invoke<MeetingRecord | null>("get_meeting", { id }),
    deleteMeeting: (id: string) => invoke("delete_meeting", { id }),
    polishMeeting: (id: string) => invoke<MeetingRecord>("polish_meeting", { id }),
    cancelAgent: () => invoke("cancel_agent"),
    sessionList: () => invoke<SessionSummary[]>("session_list"),
    sessionLoad: (sessionId: string) => invoke<AgentMessageWire[]>("session_load", { sessionId }),
    sessionNew: () => invoke<string>("session_new"),
    sessionClearCurrent: () => invoke("session_clear_current"),
    sessionCurrent: () => invoke<string | null>("session_current"),
};

export interface SessionSummary {
    session_id: string;
    created_at: string;
    entry_count: number;
    last_user_text: string | null;
    last_assistant_text: string | null;
}

/// Wire format for AgentMessage. Discriminated by `role`. The exact shape of
/// `content` is provider-internal; consumers should narrow by role first.
export type AgentMessageWire =
    | { role: "system"; content: string }
    | { role: "user"; content: unknown; attachments: unknown[] }
    | {
          role: "assistant";
          content: string | null;
          tool_calls: { id: string; name: string; arguments: string }[];
          thinking: string | null;
          usage: unknown;
          stop_reason: string | null;
      }
    | { role: "tool_result"; tool_call_id: string; content: unknown; is_error: boolean };

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
    onAsrModelDownload: (callback: (payload: AsrModelDownloadEvent) => void) =>
        listen<AsrModelDownloadEvent>("asr_model_download", (e) => callback(e.payload)),
    onMeetingStatus: (callback: (payload: MeetingStatusEvent) => void) =>
        listen<MeetingStatusEvent>("meeting_status", (e) => callback(e.payload)),
    onMeetingPartial: (callback: (payload: MeetingPartialEvent) => void) =>
        listen<MeetingPartialEvent>("meeting_partial", (e) => callback(e.payload)),
    onMeetingFinalized: (callback: (payload: MeetingFinalizedEvent) => void) =>
        listen<MeetingFinalizedEvent>("meeting_finalized", (e) => callback(e.payload)),
    onMeetingAudioLevel: (callback: (level: number) => void) =>
        listen<number>("meeting_audio_level", (e) => callback(e.payload)),
    onMeetingLlmProgress: (callback: (payload: MeetingLlmProgressEvent) => void) =>
        listen<MeetingLlmProgressEvent>("meeting_llm_progress", (e) => callback(e.payload)),
};

export type AsrModelDownloadEvent =
    | { phase: "started"; total_files: number }
    | { phase: "file"; name: string; index: number; total: number; downloaded: number; size: number | null }
    | { phase: "finished"; dir: string }
    | { phase: "failed"; message: string };
