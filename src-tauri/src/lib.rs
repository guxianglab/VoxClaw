mod agent;
mod asr;
mod audio;
pub mod commands;
mod dictation;
mod http_client;
mod input_listener;
pub mod keyboard;
mod llm;
mod meeting;
mod skill_engine;
mod skills;
mod state;
pub mod storage;
mod window;

use std::collections::HashMap;

/// Shared state for safety system Tauri commands.
pub struct SafetySharedState {
    pub pending: Arc<std::sync::Mutex<HashMap<String, tokio::sync::oneshot::Sender<agent::tool::HookDecision>>>>,
    pub rules: Arc<tokio::sync::Mutex<Vec<storage::SafetyRule>>>,
}
// TODO: 流式模块暂时禁用，等待完整集成
// mod streaming_asr;

use std::sync::atomic::Ordering as AtomicOrdering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::input_listener::InputEvent;
use crate::state::{
    DictationIntent, DictationState, LlmCancelState, AgentCancelState, ProcessingState,
    SkillExecutionState, StorageState, DICTATION_SESSION_SEQ,
};
use crate::storage::SkillAgentMode;
use crate::window::{emit_dictation_intent, emit_session_complete, show_main_window};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .setup(|app| {
            let app_handle = app.handle().clone();

            // Create indicator window
            println!("Creating indicator window...");
            let indicator_url = WebviewUrl::App("indicator.html".into());
            println!("Indicator URL: {:?}", indicator_url);

            match WebviewWindowBuilder::new(app, "indicator", indicator_url)
                .title("")
                .inner_size(800.0, 200.0)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .visible(false)
                .shadow(false)
                .focused(false)
                .build()
            {
                Ok(window) => {
                    println!("Indicator window created successfully: {:?}", window.label());
                },
                Err(e) => eprintln!("Failed to create indicator window: {:?}", e),
            }

            let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;
            let tray_icon = app
                .default_window_icon()
                .cloned()
                .expect("default window icon is required for tray");

            TrayIconBuilder::with_id("main-tray")
                .icon(tray_icon)
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => show_main_window(app),
                    "quit" => std::process::exit(0),
                    _ => {}
                })
                .build(app)?;

            // Initialize Storage (config in AppData\Roaming)
            let app_dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("data"));
            let storage_service = storage::StorageService::new(app_dir.clone());
            let config = storage_service.load_config();

            let safety_shared_state = SafetySharedState {
                pending: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
                rules: Arc::new(tokio::sync::Mutex::new(config.agent_config.safety_rules.clone())),
            };
            app.manage(safety_shared_state);

            // Initialize Services
            let asr_service = asr::AsrService::new(asr::build_provider(&config.asr, &config.proxy));
            let mut audio_service = audio::AudioService::new();

            // Try to initialize with configured device, fallback to default if it fails
            let device_init_result = audio_service.init_with_device(&config.input_device, app_handle.clone());

            if let Err(e) = device_init_result {
                eprintln!("Failed to init audio with configured device '{}': {}", config.input_device, e);
                eprintln!("Attempting to fallback to default audio device...");

                // Try to initialize with empty device name (default device)
                match audio_service.init_with_device("", app_handle.clone()) {
                    Ok(_) => {
                        println!("Successfully initialized with default audio device");
                        println!("Please select your preferred device in Settings");
                    },
                    Err(fallback_err) => {
                        eprintln!("Failed to init audio with default device: {}", fallback_err);
                        eprintln!("Application will continue but audio recording will not work until a device is selected in settings.");
                    }
                }
            }

            let audio_state = Mutex::new(audio_service);

            let input_listener = input_listener::InputListener::new();
            // Update listener flags based on config
            input_listener.enable_mouse.store(config.trigger_mouse, std::sync::atomic::Ordering::Relaxed);
            input_listener.enable_alt.store(config.trigger_toggle, std::sync::atomic::Ordering::Relaxed);

            // Channel for Input Events
            let (tx, rx) = std::sync::mpsc::channel();
            input_listener.start(tx.clone());

            // Shared processing flag
            let processing_state: ProcessingState = Arc::new(std::sync::atomic::AtomicBool::new(false));

            // LLM cancellation state - allows cancelling ongoing LLM requests
            let llm_cancel_state: LlmCancelState = Arc::new(Mutex::new(None));
            // Agent cancellation state - allows cancelling running agent tasks
            let agent_cancel_state: AgentCancelState = Arc::new(Mutex::new(None));
            let skill_execution_state: SkillExecutionState = Arc::new(Mutex::new(None));

            // Background Thread to handle events
            let processing_for_thread = processing_state.clone();
            let llm_cancel_for_thread = llm_cancel_state.clone();
            let agent_cancel_for_thread = agent_cancel_state.clone();
            let skill_execution_for_thread = skill_execution_state.clone();
            let event_tx = tx.clone();
            #[allow(unreachable_code)]
            std::thread::spawn(move || {
                let mut dictation_state = DictationState::Idle;
                let mut streaming_session: Option<asr::StreamingSession> = None;

                for event in rx {
                    match event {
                        InputEvent::Click => {
                            match &mut dictation_state {
                                DictationState::Idle => {
                                    if processing_for_thread.load(std::sync::atomic::Ordering::SeqCst) {
                                        dictation::cancel_pending_llm(&llm_cancel_for_thread, "CLICK");
                                        dictation::cancel_pending_agent(&agent_cancel_for_thread, "CLICK");
                                        std::thread::sleep(Duration::from_millis(100));
                                    }

                                    if processing_for_thread.load(std::sync::atomic::Ordering::SeqCst) {
                                        continue;
                                    }

                                    let forced_polish = app_handle
                                        .state::<StorageState>()
                                        .load_config()
                                        .llm_config
                                        .enabled;
                                    let intent = if forced_polish {
                                        DictationIntent::Polish
                                    } else {
                                        DictationIntent::Raw
                                    };

                                    if dictation::begin_recording_session(
                                        &app_handle,
                                        &mut streaming_session,
                                        intent,
                                        false,
                                        llm_cancel_for_thread.clone(),
                                        skill_execution_for_thread.clone(),
                                    ) {
                                        dictation_state = DictationState::Recording {
                                            intent,
                                            started_at: Instant::now(),
                                        };
                                    }
                                }
                                DictationState::Recording { intent, started_at } => {
                                    let forced_polish = app_handle
                                        .state::<StorageState>()
                                        .load_config()
                                        .llm_config
                                        .enabled;

                                    if started_at.elapsed()
                                        <= Duration::from_millis(crate::state::DOUBLE_CLICK_WINDOW_MS)
                                    {
                                        if !forced_polish && *intent == DictationIntent::Raw {
                                            *intent = DictationIntent::Polish;
                                            emit_dictation_intent(
                                                &app_handle,
                                                DictationIntent::Polish,
                                            );
                                        }
                                        continue;
                                    }

                                    if processing_for_thread
                                        .compare_exchange(
                                            false,
                                            true,
                                            std::sync::atomic::Ordering::SeqCst,
                                            std::sync::atomic::Ordering::SeqCst,
                                        )
                                        .is_err()
                                    {
                                        continue;
                                    }

                                    let session_id =
                                        DICTATION_SESSION_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
                                    let final_intent = if forced_polish {
                                        DictationIntent::Polish
                                    } else {
                                        *intent
                                    };

                                    match dictation::stop_dictation_recording(
                                        &app_handle,
                                        &mut streaming_session,
                                        session_id,
                                        event_tx.clone(),
                                        "CLICK",
                                    ) {
                                        Ok(()) => {
                                            dictation_state = DictationState::PendingFinalize(
                                                crate::state::PendingFinalizeState {
                                                    session_id,
                                                    intent: final_intent,
                                                    window_elapsed: false,
                                                    asr_result: None,
                                                },
                                            );
                                        }
                                        Err(err) => {
                                            eprintln!(
                                                "[CLICK] Failed to finalize dictation session: {}",
                                                err
                                            );
                                            processing_for_thread.store(
                                                false,
                                                std::sync::atomic::Ordering::SeqCst,
                                            );
                                            emit_dictation_intent(
                                                &app_handle,
                                                DictationIntent::None,
                                            );
                                            emit_session_complete(&app_handle);
                                            dictation_state = DictationState::Idle;
                                        }
                                    }
                                }
                                DictationState::PendingFinalize(pending) => {
                                    if pending.window_elapsed {
                                        continue;
                                    }

                                    let forced_polish = app_handle
                                        .state::<StorageState>()
                                        .load_config()
                                        .llm_config
                                        .enabled;
                                    if !forced_polish
                                        && pending.intent == DictationIntent::Raw
                                    {
                                        pending.intent = DictationIntent::Polish;
                                        emit_dictation_intent(
                                            &app_handle,
                                            DictationIntent::Polish,
                                        );
                                    }
                                }
                            }
                        }
                        InputEvent::StartSkill => {
                            if !matches!(dictation_state, DictationState::Idle) {
                                continue;
                            }

                            if processing_for_thread.load(std::sync::atomic::Ordering::SeqCst) {
                                dictation::cancel_pending_llm(&llm_cancel_for_thread, "SKILL");
                                dictation::cancel_pending_agent(&agent_cancel_for_thread, "SKILL");
                                std::thread::sleep(Duration::from_millis(100));
                            }

                            if processing_for_thread.load(std::sync::atomic::Ordering::SeqCst) {
                                continue;
                            }

                            // Determine intent based on agent config
                            let storage = app_handle.state::<StorageState>();
                            let config = storage.load_config();
                            let intent = if config.agent_config.mode == SkillAgentMode::Agent {
                                DictationIntent::Agent
                            } else {
                                DictationIntent::Skill
                            };

                            if dictation::begin_recording_session(
                                &app_handle,
                                &mut streaming_session,
                                intent,
                                true,
                                llm_cancel_for_thread.clone(),
                                skill_execution_for_thread.clone(),
                            ) {
                                dictation_state = DictationState::Recording {
                                    intent,
                                    started_at: Instant::now(),
                                };
                            }
                        }
                        InputEvent::StopSkill => {
                            let recording_intent = match &dictation_state {
                                DictationState::Recording { intent, .. } => Some(*intent),
                                _ => None,
                            };
                            if recording_intent.is_none() {
                                continue;
                            }
                            let intent = recording_intent.unwrap();

                            dictation_state = DictationState::Idle;

                            if intent == DictationIntent::Agent {
                                dictation::stop_agent_recording_async(
                                    &app_handle,
                                    &mut streaming_session,
                                    processing_for_thread.clone(),
                                    agent_cancel_for_thread.clone(),
                                );
                            } else {
                                dictation::stop_skill_recording_async(
                                    &app_handle,
                                    &mut streaming_session,
                                    llm_cancel_for_thread.clone(),
                                    skill_execution_for_thread.clone(),
                                    "SKILL",
                                );
                            }
                        }
                        InputEvent::DictationFinalizeWindowElapsed {
                            session_id,
                        } => {
                            let should_reset = if let DictationState::PendingFinalize(pending) =
                                &mut dictation_state
                            {
                                if pending.session_id != session_id {
                                    false
                                } else {
                                    pending.window_elapsed = true;
                                    dictation::maybe_finalize_pending_dictation(
                                        &app_handle,
                                        pending,
                                        processing_for_thread.clone(),
                                        llm_cancel_for_thread.clone(),
                                        "CLICK",
                                    )
                                }
                            } else {
                                false
                            };

                            if should_reset {
                                dictation_state = DictationState::Idle;
                            }
                        }
                        InputEvent::DictationAsrFinished {
                            session_id,
                            result,
                        } => {
                            let should_reset = if let DictationState::PendingFinalize(pending) =
                                &mut dictation_state
                            {
                                if pending.session_id != session_id {
                                    false
                                } else {
                                    if let Ok(text) = result.as_ref() {
                                        app_handle.emit("stream_update", text.clone()).ok();
                                    }
                                    app_handle.emit("recognition_processing", false).ok();
                                    pending.asr_result = Some(result);
                                    dictation::maybe_finalize_pending_dictation(
                                        &app_handle,
                                        pending,
                                        processing_for_thread.clone(),
                                        llm_cancel_for_thread.clone(),
                                        "CLICK",
                                    )
                                }
                            } else {
                                false
                            };

                            if should_reset {
                                dictation_state = DictationState::Idle;
                            }
                        }
                    }
                }
            });

            // manage states
            app.manage(audio_state);
            app.manage(asr_service);
            app.manage(storage_service);
            app.manage(input_listener);
            app.manage(processing_state);
            app.manage(agent_cancel_state);
            app.manage(commands::init_meeting_state());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::take_runtime_notice,
            commands::save_config,
            commands::get_history,
            commands::clear_history,
            commands::delete_history_item,
            commands::get_asr_status,
            commands::get_sensevoice_default_dir,
            commands::check_sensevoice_model_present,
            commands::download_sensevoice_model,
            commands::get_input_devices,
            commands::get_current_input_device,
            commands::switch_input_device,
            commands::start_audio_test,
            commands::stop_audio_test,
            commands::test_llm_connection,
            commands::get_default_scene_template,
            commands::get_default_scene_profiles,
            commands::respond_confirmation,
            commands::add_safety_rule,
            commands::set_indicator_window_expanded,
            commands::cancel_agent,
            commands::session_list,
            commands::session_load,
            commands::session_new,
            commands::session_clear_current,
            commands::session_current,
            commands::start_meeting,
            commands::stop_meeting,
            commands::get_active_meeting,
            commands::list_meetings,
            commands::get_meeting,
            commands::delete_meeting,
            commands::polish_meeting,
        ])
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    crate::window::hide_main_window(window.app_handle());
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use crate::skill_engine::{
        advance_skill_transcript_consumed, config_skill_requires_more_input,
        confirm_streaming_browser_open_action, plan_config_skill_update, prepare_skill_transcript,
    };
    use crate::state::{ConfigSkillPlan, SkillExecutionSession, SkillExecutionState,
        VoiceCommandFeedback};
    use crate::storage::AppConfig;
    use crate::skills::{SkillMatch, SWITCH_POLISH_SCENE_SKILL_ID};
    use crate::storage::PromptProfile;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    fn expect_saved_config(plan: ConfigSkillPlan) -> (AppConfig, VoiceCommandFeedback) {
        match plan {
            ConfigSkillPlan::Save { config, feedback } => (*config, feedback),
            ConfigSkillPlan::Feedback(feedback) => {
                panic!("expected saved config, got feedback: {:?}", feedback)
            }
        }
    }

    fn profile(id: &str, name: &str, voice_aliases: &[&str]) -> PromptProfile {
        PromptProfile {
            id: id.to_string(),
            name: name.to_string(),
            voice_aliases: voice_aliases
                .iter()
                .map(|alias| alias.to_string())
                .collect(),
            ..PromptProfile::new_default()
        }
    }

    fn test_skill_state() -> SkillExecutionState {
        Arc::new(Mutex::new(Some(SkillExecutionSession {
            id: 7,
            executed: HashSet::new(),
            pending: HashSet::new(),
            consumed_prefix: String::new(),
            last_streaming_browser_open_action: None,
        })))
    }

    #[test]
    fn split_skill_clause_stops_at_first_separator() {
        let input = "控制面板，打开命令提示符";
        let (clause, consumed_end) = crate::skill_engine::split_skill_clause(input);

        assert_eq!(clause, "控制面板");
        assert_eq!(&input[consumed_end..], "打开命令提示符");
    }

    #[test]
    fn prepare_skill_transcript_can_continue_after_unmatched_intro_clause() {
        let state = test_skill_state();
        let transcript = "那现在的话，我可以直接跟他说，帮我切换到中译英";
        let (first_clause, consumed_end) = crate::skill_engine::split_skill_clause(transcript);

        assert_eq!(first_clause, "那现在的话");

        advance_skill_transcript_consumed(&state, 7, transcript, consumed_end);
        let prepared =
            prepare_skill_transcript(&state, 7, transcript).expect("expected remaining clause");

        assert_eq!(prepared.0, "我可以直接跟他说，帮我切换到中译英");
    }

    #[test]
    fn normalize_direct_windows_query_strips_open_prefix_and_page_suffix() {
        assert_eq!(
            crate::skill_engine::normalize_direct_windows_query("打开设置页面"),
            "设置"
        );
    }

    #[test]
    fn switch_scene_matches_voice_alias() {
        let mut config = AppConfig::default();
        config.llm_config.profiles = vec![
            profile("default", "默认", &[]),
            profile("email", "邮件写作", &["邮件"]),
        ];
        config.llm_config.active_profile_id = "default".to_string();

        let transcript = "切换到邮件";
        let plan = plan_config_skill_update(
            transcript,
            &SkillMatch {
                skill_id: SWITCH_POLISH_SCENE_SKILL_ID.to_string(),
                keyword: "切换到".to_string(),
                start: 0,
                end: "切换到".len(),
            },
            None,
            &config,
        )
        .expect("plan should succeed");

        let (next_config, feedback) = expect_saved_config(plan);
        assert_eq!(next_config.llm_config.active_profile_id, "email");
        assert_eq!(feedback.message, "\u{5df2}\u{5207}\u{6362}\u{5230}\u{573a}\u{666f}\u{201c}\u{90ae}\u{4ef6}\u{5199}\u{4f5c}\u{201d}");
    }

    #[test]
    fn switch_scene_falls_back_to_profile_name() {
        let mut config = AppConfig::default();
        config.llm_config.profiles = vec![profile("meeting", "会议纪要", &[])];
        config.llm_config.active_profile_id = "meeting".to_string();

        let plan = plan_config_skill_update(
            "切换到会议纪要模式",
            &SkillMatch {
                skill_id: SWITCH_POLISH_SCENE_SKILL_ID.to_string(),
                keyword: "切换到".to_string(),
                start: 0,
                end: "切换到".len(),
            },
            None,
            &config,
        )
        .expect("plan should succeed");

        match plan {
            ConfigSkillPlan::Feedback(feedback) => {
                assert_eq!(feedback.message, "\u{5f53}\u{524d}\u{5df2}\u{7ecf}\u{662f}\u{573a}\u{666f}\u{201c}\u{4f1a}\u{8bae}\u{7eaa}\u{8981}\u{201d}");
            }
            ConfigSkillPlan::Save { .. } => panic!("expected no-op feedback when already active"),
        }
    }

    #[test]
    fn switch_scene_reports_alias_conflicts() {
        let mut config = AppConfig::default();
        config.llm_config.profiles = vec![
            profile("email", "邮件", &["客服"]),
            profile("support", "客服回复", &["客服"]),
        ];

        let plan = plan_config_skill_update(
            "切换到客服",
            &SkillMatch {
                skill_id: SWITCH_POLISH_SCENE_SKILL_ID.to_string(),
                keyword: "切换到".to_string(),
                start: 0,
                end: "切换到".len(),
            },
            None,
            &config,
        )
        .expect("plan should succeed");

        match plan {
            ConfigSkillPlan::Feedback(feedback) => {
                assert_eq!(feedback.level, "error");
                assert!(feedback.message.contains("匹配到多个场景"));
            }
            ConfigSkillPlan::Save { .. } => panic!("expected ambiguity feedback"),
        }
    }

    #[test]
    fn switch_scene_waits_for_scene_name_during_streaming() {
        let skill_match = SkillMatch {
            skill_id: SWITCH_POLISH_SCENE_SKILL_ID.to_string(),
            keyword: "切换到".to_string(),
            start: 0,
            end: "切换到".len(),
        };

        assert!(config_skill_requires_more_input(
            "切换到",
            &skill_match,
            None
        ));
        assert!(!config_skill_requires_more_input(
            "切换到中译英",
            &skill_match,
            None
        ));
    }

    #[test]
    fn prepare_skill_transcript_removes_consumed_prefix_and_leading_punctuation() {
        let state = test_skill_state();
        advance_skill_transcript_consumed(&state, 7, "打开新浪", "打开新浪".len());

        let prepared = prepare_skill_transcript(&state, 7, "打开新浪。打开谷歌")
            .expect("expected remaining transcript");

        assert_eq!(prepared, ("打开谷歌".to_string(), "打开新浪。".len()));
    }

    #[test]
    fn prepare_skill_transcript_rewinds_to_common_prefix_when_asr_rewrites_text() {
        let state = test_skill_state();
        advance_skill_transcript_consumed(&state, 7, "打开新浪", "打开新浪".len());

        let prepared =
            prepare_skill_transcript(&state, 7, "打开新郎").expect("expected rewritten transcript");

        assert_eq!(prepared, ("郎".to_string(), "打开新".len()));
    }

    #[test]
    fn streaming_browser_open_requires_two_matching_updates() {
        let state = test_skill_state();

        assert!(!confirm_streaming_browser_open_action(
            &state,
            7,
            "browser:open_target:新浪"
        ));
        assert!(confirm_streaming_browser_open_action(
            &state,
            7,
            "browser:open_target:新浪"
        ));
        assert!(!confirm_streaming_browser_open_action(
            &state,
            7,
            "browser:open_target:谷歌"
        ));
    }
}
