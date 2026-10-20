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
mod state;
pub mod storage;
mod window;

use std::sync::atomic::Ordering as AtomicOrdering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::input_listener::InputEvent;
use crate::state::{
    AgentCancelState, DictationIntent, DictationState, LlmCancelState, ProcessingState,
    StorageState, DICTATION_SESSION_SEQ,
};
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
                    // Immediately set click-through so the invisible window never blocks mouse events.
                    crate::window::init_indicator_click_through(app.handle());
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

            // Initialize Services
            let asr_service = match asr::build_provider(&config.asr, &config.proxy) {
                Ok(provider) => {
                    println!("[ASR] Provider initialized: {}", provider.name());
                    asr::AsrService::new(provider)
                }
                Err(err) => {
                    eprintln!("[ASR] Provider init failed: {err}");
                    eprintln!("[ASR] Application will continue but ASR will not work until provider is properly configured.");
                    // Fall back to Volcengine as a best-effort so the app doesn't crash.
                    // The frontend should check and display the error to the user.
                    let fallback = asr::volcengine::VolcengineProvider::new(
                        config.asr.volcengine.clone(),
                        config.proxy.clone(),
                    );
                    asr::AsrService::new(Arc::new(fallback))
                }
            };
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

            // Background Thread to handle events
            let processing_for_thread = processing_state.clone();
            let llm_cancel_for_thread = llm_cancel_state.clone();
            let agent_cancel_for_thread = agent_cancel_state.clone();
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
                                            // Watchdog: never let the pending
                                            // finalize state wedge forever if
                                            // the ASR session hangs.
                                            let watchdog_tx = event_tx.clone();
                                            std::thread::spawn(move || {
                                                std::thread::sleep(Duration::from_secs(
                                                    crate::state::DICTATION_FINALIZE_TIMEOUT_SECS,
                                                ));
                                                let _ = watchdog_tx.send(
                                                    input_listener::InputEvent::DictationFinalizeWatchdog {
                                                        session_id,
                                                    },
                                                );
                                            });
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
                        InputEvent::StartAgent => {
                            if !matches!(dictation_state, DictationState::Idle) {
                                continue;
                            }

                            if processing_for_thread.load(std::sync::atomic::Ordering::SeqCst) {
                                dictation::cancel_pending_llm(&llm_cancel_for_thread, "AGENT");
                                dictation::cancel_pending_agent(&agent_cancel_for_thread, "AGENT");
                                std::thread::sleep(Duration::from_millis(100));
                            }

                            if processing_for_thread.load(std::sync::atomic::Ordering::SeqCst) {
                                continue;
                            }

                            if dictation::begin_recording_session(
                                &app_handle,
                                &mut streaming_session,
                                DictationIntent::Agent,
                            ) {
                                dictation_state = DictationState::Recording {
                                    intent: DictationIntent::Agent,
                                    started_at: Instant::now(),
                                };
                            }
                        }
                        InputEvent::StopAgent => {
                            if !matches!(
                                dictation_state,
                                DictationState::Recording { intent: DictationIntent::Agent, .. }
                            ) {
                                continue;
                            }

                            dictation_state = DictationState::Idle;

                            // Serialize agent work against other input: while
                            // processing is set, new triggers cancel the
                            // running task instead of starting a new session.
                            processing_for_thread.store(
                                true,
                                std::sync::atomic::Ordering::SeqCst,
                            );

                            dictation::stop_agent_recording_async(
                                &app_handle,
                                &mut streaming_session,
                                processing_for_thread.clone(),
                                agent_cancel_for_thread.clone(),
                            );
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
                        InputEvent::DictationFinalizeWatchdog { session_id } => {
                            let should_reset = if let DictationState::PendingFinalize(pending) =
                                &mut dictation_state
                            {
                                if pending.session_id != session_id {
                                    false
                                } else if pending.asr_result.is_some() {
                                    false
                                } else {
                                    // The ASR session never delivered a result —
                                    // fail the session so input isn't blocked
                                    // forever.
                                    pending.asr_result = Some(Err(format!(
                                        "听写识别在 {} 秒内未完成，已自动取消。请重试。",
                                        crate::state::DICTATION_FINALIZE_TIMEOUT_SECS
                                    )));
                                    dictation::maybe_finalize_pending_dictation(
                                        &app_handle,
                                        pending,
                                        processing_for_thread.clone(),
                                        llm_cancel_for_thread.clone(),
                                        "WATCHDOG",
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

            // Start background device watcher for auto-recovery on plug/unplug
            let watcher_handle = app.handle().clone();
            audio::start_device_watcher(watcher_handle);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::take_runtime_notice,
            commands::save_config,
            commands::get_history,
            commands::clear_history,
            commands::delete_history_item,
            commands::download_zipformer_model,
            commands::get_input_devices,
            commands::get_current_input_device,
            commands::switch_input_device,
            commands::start_audio_test,
            commands::stop_audio_test,
            commands::test_llm_connection,
            commands::get_default_scene_profiles,
            commands::set_indicator_window_expanded,
            commands::hide_indicator,
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
            commands::summarize_meeting,
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
