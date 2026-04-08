use std::sync::atomic::Ordering as AtomicOrdering;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use enigo::{Enigo, Key, Keyboard, Settings};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::asr;
use crate::input_listener;
use crate::state::{
    DictationIntent, PendingFinalizeState, ProcessingState, LlmCancelState, AgentCancelState,
    SkillExecutionState, StorageState, TRANSCRIPTION_SEQ,
};
use crate::storage::HistoryItem;
use crate::window;

/// Process transcribed text: apply LLM correction if enabled, save to history, emit event, paste
pub fn process_transcription<R: Runtime>(
    app_handle: &AppHandle<R>,
    text: String,
    processing: ProcessingState,
    llm_cancel: LlmCancelState,
    seq_id: u64,
    polish_requested: bool,
) {
    use crate::state::preview_text;
    use tokio_util::sync::CancellationToken;

    if text.trim().is_empty() {
        println!("[TRANSCRIPTION] #{} empty, skipping", seq_id);
        processing.store(false, std::sync::atomic::Ordering::SeqCst);
        window::emit_dictation_intent(app_handle, DictationIntent::None);
        return;
    }

    println!(
        "[TRANSCRIPTION] #{} Processing: {} chars, preview='{}'",
        seq_id,
        text.len(),
        preview_text(&text, 80)
    );

    let storage = app_handle.state::<StorageState>();
    let config = storage.load_config();
    let llm_config = config.llm_config.clone();
    let proxy_config = config.proxy.clone();
    let effective_polish = llm_config.enabled || polish_requested;

    let app_handle_clone = app_handle.clone();
    let processing_clone = processing.clone();
    let llm_cancel_clone = llm_cancel.clone();

    // Use tokio runtime to handle async LLM correction
    tauri::async_runtime::spawn(async move {
        // Always clear the processing flag when this async pipeline is done
        struct ProcessingGuard(ProcessingState);
        impl Drop for ProcessingGuard {
            fn drop(&mut self) {
                self.0.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let _guard = ProcessingGuard(processing_clone);

        let final_text = if effective_polish {
            // Create cancellation token for this LLM request
            let cancel_token = CancellationToken::new();
            {
                if let Ok(mut guard) = (*llm_cancel_clone).lock() {
                    *guard = Some(cancel_token.clone());
                }
            }

            app_handle_clone.emit("llm_processing", true).ok();
            window::show_indicator_window(&app_handle_clone);

            // Use tokio::select! to race between LLM request and cancellation
            let llm_result = tokio::select! {
                result = crate::llm::correct_text(&text, &llm_config, &proxy_config) => {
                    Some(result)
                }
                _ = cancel_token.cancelled() => {
                    println!("[TRANSCRIPTION] #{} LLM request cancelled", seq_id);
                    None
                }
            };

            // Clear the cancel token
            {
                if let Ok(mut guard) = (*llm_cancel_clone).lock() {
                    *guard = None;
                }
            }

            app_handle_clone.emit("llm_processing", false).ok();
            window::emit_dictation_intent(&app_handle_clone, DictationIntent::None);
            window::emit_session_complete(&app_handle_clone);

            match llm_result {
                Some(Ok(outcome)) => {
                    println!(
                        "[TRANSCRIPTION] #{} scene='{}' fallback={}",
                        seq_id,
                        outcome.applied_scene,
                        outcome.fallback_reason.is_some()
                    );
                    if let Some(reason) = outcome.fallback_reason.clone() {
                        app_handle_clone.emit("llm_error", reason).ok();
                    }
                    outcome.final_text
                }
                Some(Err(e)) => {
                    eprintln!("LLM correction failed, using original text: {}", e);
                    // Emit error event for frontend
                    app_handle_clone.emit("llm_error", e.to_string()).ok();
                    text
                }
                None => {
                    // Cancelled - don't output anything
                    println!("[TRANSCRIPTION] #{} aborted due to cancellation", seq_id);
                    window::emit_dictation_intent(&app_handle_clone, DictationIntent::None);
                    return;
                }
            }
        } else {
            window::emit_dictation_intent(&app_handle_clone, DictationIntent::None);
            window::emit_session_complete(&app_handle_clone);
            text
        };

        if final_text.trim().is_empty() {
            println!("[TRANSCRIPTION] #{} final empty, skipping", seq_id);
            window::emit_dictation_intent(&app_handle_clone, DictationIntent::None);
            return;
        }

        // Save to history
        let item = HistoryItem {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            text: final_text.clone(),
            duration_ms: 0,
        };
        let storage = app_handle_clone.state::<StorageState>();
        storage.add_history_item(item.clone()).ok();
        app_handle_clone.emit("transcription_update", item).ok();

        // Output text (blocking, on a dedicated thread to not block tokio)
        let text_to_paste = final_text;
        let id = seq_id;
        std::thread::spawn(move || {
            output_text(&text_to_paste, id);
        })
        .join()
        .ok();
    });
}

/// Process transcription through the AI agent (when agent mode is enabled).
/// Sends text to the agent for tool-assisted processing.
pub fn process_transcription_for_agent<R: Runtime>(
    app_handle: &AppHandle<R>,
    text: String,
    selected_text: String,
    processing: ProcessingState,
    agent_cancel: AgentCancelState,
    seq_id: u64,
) {
    use crate::agent;
    use crate::agent::error::AgentError;
    use crate::state::preview_text;

    if text.trim().is_empty() {
        println!("[AGENT] #{} empty, skipping", seq_id);
        processing.store(false, std::sync::atomic::Ordering::SeqCst);
        return;
    }

    println!("[AGENT] #{} Processing: {} chars, preview='{}'", seq_id, text.len(), preview_text(&text, 80));

    let storage = app_handle.state::<StorageState>();
    let config = storage.load_config();
    let agent_config = config.agent_config.clone();
    let llm_config = config.llm_config.clone();
    let proxy_config = config.proxy.clone();

    let app_handle_clone = app_handle.clone();
    let processing_clone = processing.clone();
    let agent_cancel_clone = agent_cancel.clone();

    tauri::async_runtime::spawn(async move {
        struct ProcessingGuard(ProcessingState);
        impl Drop for ProcessingGuard {
            fn drop(&mut self) {
                self.0.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let _guard = ProcessingGuard(processing_clone);
        let app_dir = app_handle_clone.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("data"));

        if llm_config.base_url.trim().is_empty() || llm_config.api_key.trim().is_empty() {
            eprintln!("[AGENT] #{} LLM not configured, falling back to plain dictation", seq_id);
            // Fall back to plain text paste
            let id = seq_id;
            std::thread::spawn(move || {
                output_text(&text, id);
            }).join().ok();
            return;
        }

        // Resolve provider config: agent_config fields fall back to llm_config fields
        let base_url = if agent_config.provider_base_url.is_empty() { &llm_config.base_url } else { &agent_config.provider_base_url };
        let api_key = if agent_config.provider_api_key.is_empty() { &llm_config.api_key } else { &agent_config.provider_api_key };
        let model = if agent_config.provider_model.is_empty() { &llm_config.model } else { &agent_config.provider_model };
        let provider_type = if agent_config.provider_type.is_empty() { "openai_compatible" } else { &agent_config.provider_type };

        println!("[AGENT] #{} LLM config: provider_type='{}', base_url='{}', model='{}'", seq_id, provider_type, base_url, model);

        // Build the agent using the new framework
        let provider = match agent::create_provider(provider_type, base_url, api_key, model, &proxy_config) {
            Ok(p) => {
                println!("[AGENT] #{} Provider created successfully", seq_id);
                p
            }
            Err(e) => {
                eprintln!("[AGENT] #{} Failed to create provider: {}", seq_id, e);
                let id = seq_id;
                std::thread::spawn(move || { output_text(&text, id); }).join().ok();
                return;
            }
        };

        let max_iterations = if agent_config.max_iterations == 0 { 10 } else { agent_config.max_iterations };

        // Parse thinking level
        let thinking = agent::core::request::ThinkingConfig {
            level: agent::core::request::ThinkingLevel::from_str_lossy(&agent_config.thinking_level),
            budget_tokens: None,
        };

        // Parse execution mode
        let execution_mode = match agent_config.execution_mode.as_str() {
            "parallel" => agent::tool::ExecutionMode::Parallel,
            _ => agent::tool::ExecutionMode::Sequential,
        };

        let mut builder = agent::AgentBuilder::new()
            .provider(provider)
            .model(model)
            .thinking(thinking)
            .execution_mode(execution_mode);

        // Assemble system prompt: base/custom + persistent context + history
        let system_prompt = {
            let base = if agent_config.system_prompt.trim().is_empty() {
                agent::core::message::default_system_prompt().to_string()
            } else {
                agent_config.system_prompt.trim().to_string()
            };

            let mut parts = Vec::new();

            if !selected_text.is_empty() {
                parts.push(format!("\n## Selected Text Context\nThe user has currently selected the following text:\n```\n{}\n```\nPlease use this context appropriately if it relates to the user's request.", selected_text));
            }

            if !agent_config.persistent_context.trim().is_empty() {
                parts.push(format!("\n## Persistent Context\n{}", agent_config.persistent_context.trim()));
            }

            if agent_config.context_history_count > 0 {
                let recent = agent::history::read_recent(&app_dir, agent_config.context_history_count);
                let ctx = agent::history::format_as_context(&recent);
                if !ctx.is_empty() {
                    parts.push(ctx);
                }
            }

            if parts.is_empty() {
                base
            } else {
                format!("{}{}", base, parts.join("\n"))
            }
        };

        builder = builder.system_prompt(system_prompt);

        // Add context transformers to limit conversation length
        builder = builder.context_transformer(Box::new(
            agent::core::context::TruncationTransformer { max_messages: 50 }
        ));
        builder = builder.context_transformer(Box::new(
            agent::core::context::TokenBudgetTransformer { max_tokens: 8000 }
        ));

        // Register all built-in tools
        for tool in agent::create_all_tools() {
            builder = builder.tool(tool);
        }

        // Add logging hook always, safety hook for rule evaluation
        builder = builder.hook(Box::new(agent::tool::hooks::LoggingHook));
        let shared_safety = app_handle_clone.state::<crate::SafetySharedState>();
        let safety_hook = agent::tool::hooks::SafetyHook::new(
            shared_safety.rules.clone(),
            agent_config.default_safety_policy.clone(),
            app_handle_clone.clone(),
            shared_safety.pending.clone(),
        );

        builder = builder.hook(Box::new(safety_hook));

        builder = builder.max_iterations(max_iterations as u32);

        let mut agent = match builder.build() {
            Ok(a) => {
                println!("[AGENT] #{} Agent built with max_iterations={}", seq_id, max_iterations);
                a
            }
            Err(e) => {
                eprintln!("[AGENT] #{} Failed to build agent: {}", seq_id, e);
                let id = seq_id;
                std::thread::spawn(move || { output_text(&text, id); }).join().ok();
                return;
            }
        };

        // Store the cancel token so external callers (e.g. frontend stop button) can abort us.
        {
            if let Ok(mut guard) = agent_cancel_clone.lock() {
                *guard = Some(agent.cancellation_token());
            }
        }

        // Subscribe to events for real-time UI updates
        let mut event_rx = agent.subscribe();
        let event_handle = app_handle_clone.clone();
        tokio::spawn(async move {
            use tokio::sync::broadcast::error::RecvError;
            loop {
                match event_rx.recv().await {
                    Ok(event) => {
                        event_handle.emit("agent_event", &event).ok();
                    }
                    Err(RecvError::Lagged(n)) => {
                        println!("[AGENT] Event receiver lagged by {} events", n);
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        });

        // Show indicator
        app_handle_clone.emit("agent_processing", true).ok();

        match agent.process(&text).await {
            Ok(result) => {
                // Clear the cancel token — agent is done.
                if let Ok(mut guard) = agent_cancel_clone.lock() { *guard = None; }
                app_handle_clone.emit("agent_processing", false).ok();
                window::emit_session_complete(&app_handle_clone);

                let final_text = result.text;
                if !final_text.trim().is_empty() {
                    // Save to history
                    let item = HistoryItem {
                        id: uuid::Uuid::new_v4().to_string(),
                        timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                        text: final_text.clone(),
                        duration_ms: 0,
                    };
                    let storage = app_handle_clone.state::<StorageState>();
                    storage.add_history_item(item.clone()).ok();
                    app_handle_clone.emit("transcription_update", item).ok();

                    // Save to agent conversation history
                    let tool_names: Vec<String> = result.actions.iter().map(|a| a.tool_name.clone()).collect();
                    let history_entry = agent::history::AgentHistoryEntry {
                        timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                        user_input: text.clone(),
                        agent_output: final_text.clone(),
                        tool_summaries: tool_names,
                    };
                    agent::history::append_entry(&app_dir, &history_entry).ok();

                    let id = seq_id;
                    if result.actions.is_empty() {
                        std::thread::spawn(move || {
                            output_text(&final_text, id);
                        }).join().ok();
                    } else {
                        println!("[AGENT] #{} Skipping paste: actions were executed", seq_id);
                    }
                }

                if !result.actions.is_empty() {
                    let names: Vec<String> = result.actions.iter().map(|a| a.tool_name.clone()).collect();
                    println!("[AGENT] #{} Actions: {}", seq_id, names.join(", "));
                }
            }
            Err(e) => {
                // Clear the cancel token regardless.
                if let Ok(mut guard) = agent_cancel_clone.lock() { *guard = None; }
                app_handle_clone.emit("agent_processing", false).ok();
                window::emit_session_complete(&app_handle_clone);
                app_handle_clone.emit("agent_error", e.to_string()).ok();
                eprintln!("[AGENT] #{} Error: {}", seq_id, e);
                // Only fall back to plain text paste for non-cancellation errors
                if !matches!(e, AgentError::Cancelled) {
                    let id = seq_id;
                    std::thread::spawn(move || { output_text(&text, id); }).join().ok();
                }
            }
        }
    });
}

/// Paste text into the currently focused window using clipboard + Ctrl+V.
pub fn output_text(text: &str, seq_id: u64) {
    println!("[OUTPUT] #{} start: {} chars", seq_id, text.len());

    const INPUT_SETTLE_DELAY_MS: u64 = 80;
    std::thread::sleep(std::time::Duration::from_millis(INPUT_SETTLE_DELAY_MS));

    let mut clipboard = match Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[OUTPUT] #{} clipboard init failed: {:?}", seq_id, e);
            return;
        }
    };

    let original_text = clipboard.get_text().ok();

    if let Err(e) = clipboard.set_text(text) {
        eprintln!("[OUTPUT] #{} clipboard set_text failed: {:?}", seq_id, e);
        return;
    }

    std::thread::sleep(std::time::Duration::from_millis(10));

    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[OUTPUT] #{} enigo init failed: {:?}", seq_id, e);
            return;
        }
    };

    if let Err(e) = enigo.key(Key::Control, enigo::Direction::Press) {
        eprintln!("[OUTPUT] #{} Ctrl press failed: {:?}", seq_id, e);
    }
    std::thread::sleep(std::time::Duration::from_millis(5));
    if let Err(e) = enigo.key(Key::Unicode('v'), enigo::Direction::Click) {
        eprintln!("[OUTPUT] #{} V click failed: {:?}", seq_id, e);
    }
    std::thread::sleep(std::time::Duration::from_millis(5));
    if let Err(e) = enigo.key(Key::Control, enigo::Direction::Release) {
        eprintln!("[OUTPUT] #{} Ctrl release failed: {:?}", seq_id, e);
    }

    println!("[OUTPUT] #{} paste done", seq_id);

    std::thread::sleep(std::time::Duration::from_millis(100));
    if let Some(original) = original_text {
        let _ = clipboard.set_text(&original);
        println!("[OUTPUT] #{} clipboard restored", seq_id);
    }
}

// ---------------------------------------------------------------------------
// Recording session management
// ---------------------------------------------------------------------------

pub fn begin_recording_session<R: Runtime>(
    app_handle: &AppHandle<R>,
    streaming_session: &mut Option<asr::StreamingSession>,
    intent: DictationIntent,
    skill_mode: bool,
    llm_cancel: LlmCancelState,
    skill_state: SkillExecutionState,
) -> bool {
    use crate::state::AudioState;
    use crate::state::AsrState;

    let started_at = Instant::now();
    let audio = app_handle.state::<AudioState>();
    let (sample_rate, stream_rx) = match audio.lock() {
        Ok(audio) => match audio.start_recording_with_streaming() {
            Ok(rx) => (audio.get_sample_rate(), rx),
            Err(err) => {
                eprintln!("[START] Failed to start audio capture: {}", err);
                return false;
            }
        },
        Err(_) => return false,
    };

    app_handle.emit("recording_status", true).ok();
    window::emit_dictation_intent(app_handle, intent);
    window::show_indicator_window(app_handle);

    let storage = app_handle.state::<StorageState>();
    let config = storage.load_config();
    let asr = app_handle.state::<AsrState>();
    let handle = app_handle.clone();
    let skill_session_id = if skill_mode {
        crate::skill_engine::start_skill_execution_session(&skill_state)
    } else {
        0
    };
    match asr.start_streaming_session(
        stream_rx,
        sample_rate,
        config.online_asr_config,
        config.proxy,
        move |text| {
            handle.emit("stream_update", &text).ok();
            if skill_mode {
                let seq_id = TRANSCRIPTION_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
                crate::skill_engine::spawn_skill_transcript_processing(
                    &handle,
                    text,
                    llm_cancel.clone(),
                    skill_state.clone(),
                    skill_session_id,
                    seq_id,
                );
            }
        },
    ) {
        Ok(session) => {
            *streaming_session = Some(session);
            println!(
                "[START] Recording session ready in {} ms",
                started_at.elapsed().as_millis()
            );
            true
        }
        Err(err) => {
            eprintln!("[START] Failed to start streaming preview: {}", err);
            app_handle.emit("recording_status", false).ok();
            window::emit_dictation_intent(app_handle, DictationIntent::None);
            window::emit_session_complete(app_handle);
            if let Ok(audio) = app_handle.state::<AudioState>().lock() {
                let _ = audio.stop_recording();
            }
            false
        }
    }
}

pub fn stop_recording_now<R: Runtime>(app_handle: &AppHandle<R>) -> (Vec<f32>, u32) {
    use crate::state::AudioState;

    app_handle.emit("recording_status", false).ok();

    let audio = app_handle.state::<AudioState>();
    let mut buffer = Vec::new();
    let mut sample_rate = 48_000u32;
    if let Ok(audio) = audio.lock() {
        sample_rate = audio.get_sample_rate();
        if let Ok(b) = audio.stop_recording() {
            buffer = b;
        }
    }

    (buffer, sample_rate)
}

pub fn finish_streaming_asr_async(
    tx: std::sync::mpsc::Sender<input_listener::InputEvent>,
    session_id: u64,
    session: asr::StreamingSession,
    log_tag: &str,
) {
    let log_tag = log_tag.to_string();
    std::thread::spawn(move || {
        let transcribe_started = Instant::now();
        let result = session.finish_and_wait().map_err(|err| err.to_string());
        println!(
            "[{}] Dictation ASR finished in {} ms for session {}",
            log_tag,
            transcribe_started.elapsed().as_millis(),
            session_id
        );
        tx.send(input_listener::InputEvent::DictationAsrFinished { session_id, result })
            .ok();
    });
}

pub fn dispatch_final_transcription<R: Runtime>(
    app_handle: &AppHandle<R>,
    text: String,
    intent: DictationIntent,
    processing: ProcessingState,
    llm_cancel: LlmCancelState,
    log_tag: &str,
) {
    use crate::state::preview_text;

    let seq_id = TRANSCRIPTION_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
    println!(
        "[{}] #{} Dispatching final dictation, intent={:?}, {} chars, preview='{}'",
        log_tag,
        seq_id,
        intent,
        text.len(),
        preview_text(&text, 80)
    );
    process_transcription(
        app_handle,
        text,
        processing,
        llm_cancel,
        seq_id,
        matches!(intent, DictationIntent::Polish),
    );
}

pub fn maybe_finalize_pending_dictation<R: Runtime>(
    app_handle: &AppHandle<R>,
    pending: &mut PendingFinalizeState,
    processing: ProcessingState,
    llm_cancel: LlmCancelState,
    log_tag: &str,
) -> bool {
    if !pending.window_elapsed {
        return false;
    }

    let Some(result) = pending.asr_result.take() else {
        return false;
    };

    match result {
        Ok(text) => {
            dispatch_final_transcription(
                app_handle,
                text,
                pending.intent,
                processing,
                llm_cancel,
                log_tag,
            );
        }
        Err(err) => {
            eprintln!("[{}] Transcription error: {}", log_tag, err);
            app_handle.emit("recognition_processing", false).ok();
            window::emit_dictation_intent(app_handle, DictationIntent::None);
            window::emit_session_complete(app_handle);
            processing.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }

    true
}

pub fn begin_pending_finalize_window(
    tx: std::sync::mpsc::Sender<input_listener::InputEvent>,
    session_id: u64,
) {
    use crate::state::DOUBLE_CLICK_WINDOW_MS;

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(DOUBLE_CLICK_WINDOW_MS));
        tx.send(input_listener::InputEvent::DictationFinalizeWindowElapsed { session_id })
            .ok();
    });
}

pub fn stop_dictation_recording<R: Runtime>(
    app_handle: &AppHandle<R>,
    streaming_session: &mut Option<asr::StreamingSession>,
    session_id: u64,
    tx: std::sync::mpsc::Sender<input_listener::InputEvent>,
    log_tag: &str,
) -> Result<(), String> {
    let stop_started = Instant::now();
    let (_buffer, _sample_rate) = stop_recording_now(app_handle);
    println!(
        "[{}] Capture stopped in {} ms",
        log_tag,
        stop_started.elapsed().as_millis()
    );

    app_handle.emit("recognition_processing", true).ok();
    begin_pending_finalize_window(tx.clone(), session_id);

    let Some(session) = streaming_session.take() else {
        return Err("No active streaming session to finish".to_string());
    };

    finish_streaming_asr_async(tx, session_id, session, log_tag);
    Ok(())
}

pub fn stop_agent_recording_async<R: Runtime>(
    app_handle: &AppHandle<R>,
    streaming_session: &mut Option<asr::StreamingSession>,
    processing: ProcessingState,
    agent_cancel: AgentCancelState,
) {
    let (_buffer, _sample_rate) = stop_recording_now(app_handle);

    let Some(session) = streaming_session.take() else {
        return;
    };

    let app_handle_clone = app_handle.clone();
    std::thread::spawn(move || {
        // Start fetching the selected text in parallel
        let selected_text_thread = std::thread::spawn(|| {
            get_selected_text_sync()
        });

        match session.finish_and_wait() {
            Ok(text) => {
                let selected_text = selected_text_thread.join().unwrap_or_default();
                app_handle_clone.emit("stream_update", text.clone()).ok();
                let seq_id = TRANSCRIPTION_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
                process_transcription_for_agent(
                    &app_handle_clone,
                    text,
                    selected_text,
                    processing,
                    agent_cancel,
                    seq_id,
                );
            }
            Err(e) => {
                eprintln!("[AGENT] Transcription error: {}", e);
            }
        }
    });
}

pub fn stop_skill_recording_async<R: Runtime>(
    app_handle: &AppHandle<R>,
    streaming_session: &mut Option<asr::StreamingSession>,
    llm_cancel: LlmCancelState,
    skill_state: SkillExecutionState,
    log_tag: &str,
) {
    let stop_started = Instant::now();
    let (_buffer, _sample_rate) = stop_recording_now(app_handle);
    println!(
        "[{}] Skill capture stopped in {} ms",
        log_tag,
        stop_started.elapsed().as_millis()
    );
    window::emit_dictation_intent(app_handle, DictationIntent::None);
    window::emit_session_complete(app_handle);

    let Some(session) = streaming_session.take() else {
        if let Some(session_id) = crate::skill_engine::current_skill_execution_session_id(&skill_state) {
            crate::skill_engine::finish_skill_execution_session(&skill_state, session_id);
        }
        return;
    };

    let Some(skill_session_id) = crate::skill_engine::current_skill_execution_session_id(&skill_state) else {
        return;
    };

    let app_handle_clone = app_handle.clone();
    let log_tag = log_tag.to_string();
    std::thread::spawn(move || match session.finish_and_wait() {
        Ok(text) => {
            app_handle_clone.emit("stream_update", text.clone()).ok();
            let seq_id = TRANSCRIPTION_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
            let app_handle_for_async = app_handle_clone.clone();
            let skill_state_for_async = skill_state.clone();
            tauri::async_runtime::spawn(async move {
                crate::skill_engine::execute_skill_transcript(
                    &app_handle_for_async,
                    &text,
                    &llm_cancel,
                    &skill_state_for_async,
                    skill_session_id,
                    seq_id,
                    true,
                )
                .await;
                crate::skill_engine::finish_skill_execution_session(&skill_state_for_async, skill_session_id);
            });
        }
        Err(e) => {
            eprintln!("[{}] Skill final transcription error: {}", log_tag, e);
            crate::skill_engine::finish_skill_execution_session(&skill_state, skill_session_id);
        }
    });
}

pub fn cancel_pending_llm(llm_cancel: &LlmCancelState, log_tag: &str) {
    if let Ok(guard) = (*llm_cancel).lock() {
        if let Some(token) = guard.as_ref() {
            println!("[{}] Cancelling ongoing LLM request", log_tag);
            token.cancel();
        }
    }
}

pub fn cancel_pending_agent(agent_cancel: &AgentCancelState, log_tag: &str) {
    if let Ok(guard) = (*agent_cancel).lock() {
        if let Some(token) = guard.as_ref() {
            println!("[{}] Cancelling ongoing agent", log_tag);
            token.cancel();
        }
    }
}

// ---------------------------------------------------------------------------
// Context Gathering
// ---------------------------------------------------------------------------

pub fn get_selected_text_sync() -> String {
    use arboard::Clipboard;
    use enigo::{Enigo, Key, Keyboard, Settings};
    use std::time::Duration;

    let mut clipboard = match Clipboard::new() {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    let original = clipboard.get_text().unwrap_or_default();
    let _ = clipboard.set_text("");

    std::thread::sleep(Duration::from_millis(10));

    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(_) => return String::new(),
    };

    let _ = enigo.key(Key::Control, enigo::Direction::Press);
    std::thread::sleep(Duration::from_millis(5));
    let _ = enigo.key(Key::Unicode('c'), enigo::Direction::Click);
    std::thread::sleep(Duration::from_millis(5));
    let _ = enigo.key(Key::Control, enigo::Direction::Release);
    
    std::thread::sleep(Duration::from_millis(50));

    let new_text = clipboard.get_text().unwrap_or_default();

    if !original.is_empty() {
        std::thread::sleep(Duration::from_millis(10));
        let _ = clipboard.set_text(&original);
    } else {
        let _ = clipboard.set_text("");
    }

    new_text.trim().to_string()
}
