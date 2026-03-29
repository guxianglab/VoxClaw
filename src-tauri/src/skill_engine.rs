use std::sync::atomic::Ordering as AtomicOrdering;

use tauri::{AppHandle, Manager, Runtime};

use crate::skills;
use crate::state::{
    preview_text, ConfigSkillPlan, LlmCancelState, SkillExecutionSession,
    SkillExecutionState, StorageState,
};
use crate::storage::AppConfig;
use crate::window;

// ---------------------------------------------------------------------------
// Skill session management
// ---------------------------------------------------------------------------

pub fn reserve_skill_action(state: &SkillExecutionState, session_id: u64, action_key: &str) -> bool {
    let Ok(mut guard) = (*state).lock() else {
        return false;
    };
    let Some(session) = guard.as_mut() else {
        return false;
    };
    if session.id != session_id {
        return false;
    }
    if session.executed.contains(action_key) || session.pending.contains(action_key) {
        return false;
    }
    session.pending.insert(action_key.to_string());
    true
}

pub fn complete_skill_action(
    state: &SkillExecutionState,
    session_id: u64,
    action_key: &str,
    mark_executed: bool,
) {
    if let Ok(mut guard) = (*state).lock() {
        if let Some(session) = guard.as_mut() {
            if session.id == session_id {
                session.pending.remove(action_key);
                if mark_executed {
                    session.executed.insert(action_key.to_string());
                }
            }
        }
    }
}

pub fn start_skill_execution_session(state: &SkillExecutionState) -> u64 {
    use crate::state::SKILL_SESSION_SEQ;
    use std::collections::HashSet;

    let session_id = SKILL_SESSION_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
    if let Ok(mut guard) = (*state).lock() {
        *guard = Some(SkillExecutionSession {
            id: session_id,
            executed: HashSet::new(),
            pending: HashSet::new(),
            consumed_prefix: String::new(),
            last_streaming_browser_open_action: None,
        });
    }
    session_id
}

pub fn finish_skill_execution_session(state: &SkillExecutionState, session_id: u64) {
    if let Ok(mut guard) = (*state).lock() {
        if guard.as_ref().map(|session| session.id) == Some(session_id) {
            *guard = None;
        }
    }
}

pub fn current_skill_execution_session_id(state: &SkillExecutionState) -> Option<u64> {
    (*state)
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(|session| session.id))
}

// ---------------------------------------------------------------------------
// Action key helpers
// ---------------------------------------------------------------------------

pub fn browser_action_key(action: &skills::BrowserAction) -> String {
    match action {
        skills::BrowserAction::OpenTarget { query } => {
            format!("browser:open_target:{}", query.trim().to_lowercase())
        }
        skills::BrowserAction::SwitchTabIndex { index } => format!("browser:switch_tab:{}", index),
        skills::BrowserAction::Find { query } => format!(
            "browser:find:{}",
            query.as_deref().unwrap_or_default().trim().to_lowercase()
        ),
        other => format!("browser:{:?}", other),
    }
}

pub fn windows_action_key(action: &skills::WindowsAction) -> String {
    match action {
        skills::WindowsAction::OpenTarget { query } => {
            format!("windows:open_target:{}", query.trim().to_lowercase())
        }
        other => format!("windows:{:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Windows query normalization and resolution
// ---------------------------------------------------------------------------

pub fn normalize_direct_windows_query(query: &str) -> String {
    let mut normalized = query
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    ',' | ';' | '.' | ':' | '\n' | '\r' | '\u{ff0c}' | '\u{ff1b}' | '\u{3002}'
                )
        })
        .trim()
        .to_string();

    for prefix in [
        "\u{6253}\u{5f00}",
        "\u{542f}\u{52a8}",
        "\u{8fd0}\u{884c}",
        "open",
        "launch",
        "start",
        "run",
    ] {
        if let Some(stripped) = normalized.strip_prefix(prefix) {
            normalized = stripped.trim().to_string();
            break;
        }
    }

    for suffix in [
        "\u{9875}\u{9762}",
        "\u{754c}\u{9762}",
        "\u{9762}\u{677f}",
        " page",
        " panel",
    ] {
        if let Some(stripped) = normalized.strip_suffix(suffix) {
            normalized = stripped.trim().to_string();
            break;
        }
    }

    normalized
}

pub fn resolve_windows_target_candidate(
    windows_skill: &skills::SkillConfig,
    query: &str,
) -> Option<skills::WindowsTargetConfig> {
    skills::resolve_windows_target(windows_skill, query).or_else(|| {
        let normalized = normalize_direct_windows_query(query);
        if normalized.is_empty() || normalized == query.trim() {
            None
        } else {
            skills::resolve_windows_target(windows_skill, &normalized)
        }
    })
}

pub fn plan_direct_windows_target(
    transcript: &str,
    windows_skill: &skills::SkillConfig,
) -> Option<skills::WindowsActionPlan> {
    let query = normalize_direct_windows_query(transcript);
    if query.is_empty() || resolve_windows_target_candidate(windows_skill, &query).is_none() {
        return None;
    }

    Some(skills::WindowsActionPlan {
        action: skills::WindowsAction::OpenTarget { query },
        action_name: "Open Windows Target".to_string(),
        note: None,
        consumed_end: transcript.len(),
    })
}

// ---------------------------------------------------------------------------
// Streaming readiness checks
// ---------------------------------------------------------------------------

pub fn is_browser_open_query_ready(browser_skill: &skills::SkillConfig, query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return false;
    }
    if skills::normalize_browser_url(trimmed).is_ok() {
        return true;
    }
    if skills::resolve_browser_site_url(browser_skill, trimmed).is_some() {
        return true;
    }

    let visible_len = trimmed.chars().filter(|ch| !ch.is_whitespace()).count();
    let ascii_only = trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ".-_/".contains(ch));
    if ascii_only {
        return visible_len >= 4;
    }

    visible_len >= 2
}

pub fn is_windows_open_query_ready(windows_skill: &skills::SkillConfig, query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return false;
    }
    if resolve_windows_target_candidate(windows_skill, trimmed).is_some() {
        return true;
    }

    let visible_len = trimmed.chars().filter(|ch| !ch.is_whitespace()).count();
    let ascii_only = trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ".-_/".contains(ch));
    if ascii_only {
        return visible_len >= 3;
    }

    visible_len >= 2
}

// ---------------------------------------------------------------------------
// Transcript manipulation
// ---------------------------------------------------------------------------

pub fn split_skill_clause(text: &str) -> (&str, usize) {
    let mut boundary: Option<(usize, usize)> = None;

    for (idx, ch) in text.char_indices() {
        if matches!(
            ch,
            ',' | ';' | '\n' | '\r' | '\u{ff0c}' | '\u{ff1b}' | '\u{3002}'
        ) {
            boundary = Some((idx, ch.len_utf8()));
            break;
        }
    }

    for marker in ["\u{7136}\u{540e}", "\u{5e76}\u{4e14}", "\u{63a5}\u{7740}"] {
        if let Some(idx) = text.find(marker) {
            let should_replace = boundary
                .map(|(current_idx, _)| idx < current_idx)
                .unwrap_or(true);
            if should_replace {
                boundary = Some((idx, marker.len()));
            }
        }
    }

    let Some((boundary_start, boundary_len)) = boundary else {
        return (text.trim_end(), text.len());
    };

    let clause = text[..boundary_start].trim_end_matches(|ch: char| {
        ch.is_whitespace()
            || matches!(
                ch,
                ',' | ';' | '.' | ':' | '\u{ff0c}' | '\u{ff1b}' | '\u{3002}'
            )
    });
    let mut consumed_end = boundary_start + boundary_len;

    while consumed_end < text.len() {
        let Some(next_char) = text[consumed_end..].chars().next() else {
            break;
        };
        if next_char.is_whitespace()
            || matches!(
                next_char,
                ',' | ';' | '.' | ':' | '\u{ff0c}' | '\u{ff1b}' | '\u{3002}'
            )
        {
            consumed_end += next_char.len_utf8();
            continue;
        }
        break;
    }

    (clause, consumed_end)
}

fn trim_skill_transcript_prefix(text: &str) -> (&str, usize) {
    let trimmed = text.trim_start_matches(|ch: char| {
        ch.is_whitespace() || matches!(ch, '\u{ff0c}' | '\u{3002}' | ',' | '.' | '\u{3001}' | ';' | '\u{ff1b}' | ':' | '\u{ff1a}')
    });
    (trimmed, text.len() - trimmed.len())
}

fn shared_prefix_len(left: &str, right: &str) -> usize {
    let mut matched = 0usize;
    let mut left_chars = left.chars();
    let mut right_chars = right.chars();

    loop {
        match (left_chars.next(), right_chars.next()) {
            (Some(left_char), Some(right_char)) if left_char == right_char => {
                matched += left_char.len_utf8();
            }
            _ => break,
        }
    }

    matched
}

pub fn prepare_skill_transcript(
    state: &SkillExecutionState,
    session_id: u64,
    transcript: &str,
) -> Option<(String, usize)> {
    let Ok(mut guard) = (*state).lock() else {
        return None;
    };
    let session = guard.as_mut()?;
    if session.id != session_id {
        return None;
    }

    let consumed_len = shared_prefix_len(&session.consumed_prefix, transcript);
    if consumed_len < session.consumed_prefix.len() {
        session.consumed_prefix.truncate(consumed_len);
    }

    let remaining = &transcript[consumed_len..];
    let (trimmed, leading_offset) = trim_skill_transcript_prefix(remaining);
    if trimmed.trim().is_empty() {
        return None;
    }

    Some((trimmed.to_string(), consumed_len + leading_offset))
}

pub fn advance_skill_transcript_consumed(
    state: &SkillExecutionState,
    session_id: u64,
    transcript: &str,
    consumed_end: usize,
) {
    let Ok(mut guard) = (*state).lock() else {
        return;
    };
    let Some(session) = guard.as_mut() else {
        return;
    };
    if session.id != session_id {
        return;
    }

    let clamped_end = consumed_end.min(transcript.len());
    if !transcript.is_char_boundary(clamped_end) {
        return;
    }

    session.consumed_prefix = transcript[..clamped_end].to_string();
    session.last_streaming_browser_open_action = None;
}

pub fn clear_streaming_browser_open_action_candidate(state: &SkillExecutionState, session_id: u64) {
    if let Ok(mut guard) = (*state).lock() {
        if let Some(session) = guard.as_mut() {
            if session.id == session_id {
                session.last_streaming_browser_open_action = None;
            }
        }
    }
}

pub fn confirm_streaming_browser_open_action(
    state: &SkillExecutionState,
    session_id: u64,
    action_key: &str,
) -> bool {
    let Ok(mut guard) = (*state).lock() else {
        return false;
    };
    let Some(session) = guard.as_mut() else {
        return false;
    };
    if session.id != session_id {
        return false;
    }

    if session.last_streaming_browser_open_action.as_deref() == Some(action_key) {
        session.last_streaming_browser_open_action = None;
        true
    } else {
        session.last_streaming_browser_open_action = Some(action_key.to_string());
        false
    }
}

// ---------------------------------------------------------------------------
// Plan wrappers
// ---------------------------------------------------------------------------

pub fn plan_windows_command(
    transcript: &str,
    windows_skill: &skills::SkillConfig,
    windows_match: Option<&skills::SkillMatch>,
) -> Result<Option<skills::WindowsActionPlan>, String> {
    match skills::plan_windows_action(transcript, windows_skill, windows_match) {
        skills::WindowsPlanResult::None => Ok(None),
        skills::WindowsPlanResult::Feedback(message) => Err(message),
        skills::WindowsPlanResult::Action(plan) => Ok(Some(plan)),
    }
}

pub fn plan_browser_command(
    transcript: &str,
    browser_skill: &skills::SkillConfig,
    browser_match: Option<&skills::SkillMatch>,
) -> Result<Option<skills::BrowserActionPlan>, String> {
    match skills::plan_browser_action(transcript, browser_skill, browser_match) {
        skills::BrowserPlanResult::None => Ok(None),
        skills::BrowserPlanResult::Feedback(message) => Err(message),
        skills::BrowserPlanResult::Action(plan) => Ok(Some(plan)),
    }
}

// ---------------------------------------------------------------------------
// Browser / Windows plan execution
// ---------------------------------------------------------------------------

pub async fn resolve_browser_navigation_target<R: Runtime>(
    app_handle: &AppHandle<R>,
    browser_skill: &skills::SkillConfig,
    query: &str,
    config: &AppConfig,
    llm_cancel: &LlmCancelState,
    seq_id: u64,
) -> Result<(String, String), String> {
    if let Ok(url) = skills::normalize_browser_url(query) {
        return Ok((url, format!("已打开网址：{}", query.trim())));
    }

    if let Some(url) = skills::resolve_browser_site_url(browser_skill, query) {
        return Ok((url, format!("已打开站点：{}", query.trim())));
    }

    let options = browser_skill
        .browser_options
        .as_ref()
        .ok_or_else(|| "浏览器技能配置缺失".to_string())?;

    let mut llm_reason: Option<String> = None;

    if options.llm_site_resolution_enabled {
        use tokio_util::sync::CancellationToken;

        let cancel_token = CancellationToken::new();
        window::store_llm_cancel_token(llm_cancel, Some(cancel_token.clone()));
        window::set_browser_llm_state(app_handle, true);

        let resolution = tokio::select! {
            result = crate::llm::resolve_browser_url(query, &config.llm_config, &config.proxy) => Some(result),
            _ = cancel_token.cancelled() => None,
        };

        window::set_browser_llm_state(app_handle, false);
        window::store_llm_cancel_token(llm_cancel, None);

        match resolution {
            Some(Ok(outcome)) => {
                if let Some(url) = outcome.resolved_url {
                    println!(
                        "[SKILL] #{} Browser target resolved via LLM: {}",
                        seq_id, url
                    );
                    return Ok((url, format!("已解析并打开：{}", query.trim())));
                }
                llm_reason = outcome.fallback_reason;
            }
            Some(Err(error)) => {
                llm_reason = Some(error.to_string());
            }
            None => return Err("浏览器网址解析已取消".to_string()),
        }
    }

    if options.search_fallback_enabled {
        let search_url = skills::build_browser_search_url(&options.search_url_template, query)?;
        let message = if let Some(reason) = llm_reason {
            format!(
                "未识别到精确网址，已改为搜索：{}（{}）",
                query.trim(),
                reason
            )
        } else {
            format!("未识别到精确网址，已改为搜索：{}", query.trim())
        };
        return Ok((search_url, message));
    }

    Err(llm_reason.unwrap_or_else(|| format!("未识别到可打开的网址：{}", query.trim())))
}

pub async fn resolve_windows_target<R: Runtime>(
    app_handle: &AppHandle<R>,
    windows_skill: &skills::SkillConfig,
    query: &str,
    config: &AppConfig,
    llm_cancel: &LlmCancelState,
    seq_id: u64,
) -> Result<(skills::WindowsTargetConfig, String), String> {
    if let Some(target) = resolve_windows_target_candidate(windows_skill, query) {
        return Ok((target, format!("Opened Windows target: {}", query.trim())));
    }

    let options = windows_skill
        .windows_options
        .as_ref()
        .ok_or_else(|| "Windows skill configuration is missing".to_string())?;

    if !options.llm_target_resolution_enabled {
        return Err(format!("No Windows target matched: {}", query.trim()));
    }

    use tokio_util::sync::CancellationToken;

    let cancel_token = CancellationToken::new();
    window::store_llm_cancel_token(llm_cancel, Some(cancel_token.clone()));
    window::set_browser_llm_state(app_handle, true);

    let resolution = tokio::select! {
        result = crate::llm::resolve_windows_target(query, windows_skill, &config.llm_config, &config.proxy) => Some(result),
        _ = cancel_token.cancelled() => None,
    };

    window::set_browser_llm_state(app_handle, false);
    window::store_llm_cancel_token(llm_cancel, None);

    match resolution {
        Some(Ok(outcome)) => {
            if let Some(target_id) = outcome.resolved_target_id {
                if let Some(target) =
                    skills::resolve_windows_target_by_id(windows_skill, &target_id)
                {
                    println!(
                        "[SKILL] #{} Windows target resolved via LLM: {}",
                        seq_id, target_id
                    );
                    return Ok((
                        target,
                        format!("Resolved and opened Windows target: {}", query.trim()),
                    ));
                }
            }
            Err(outcome
                .fallback_reason
                .unwrap_or_else(|| format!("No Windows target matched: {}", query.trim())))
        }
        Some(Err(error)) => Err(error.to_string()),
        None => Err("Windows target resolution was cancelled".to_string()),
    }
}

pub async fn execute_browser_plan<R: Runtime>(
    app_handle: &AppHandle<R>,
    browser_skill: &skills::SkillConfig,
    plan: &skills::BrowserActionPlan,
    config: &AppConfig,
    llm_cancel: &LlmCancelState,
    seq_id: u64,
) -> Result<(), String> {
    let note = plan.note.clone();
    let success_message = match &plan.action {
        skills::BrowserAction::OpenTarget { query } => {
            let (url, message) = resolve_browser_navigation_target(
                app_handle,
                browser_skill,
                query,
                config,
                llm_cancel,
                seq_id,
            )
            .await?;
            skills::open_browser_url(&url)?;
            message
        }
        skills::BrowserAction::Find { query } => {
            skills::execute_browser_shortcut_action(&plan.action)?;
            match query.as_deref() {
                Some(value) if !value.is_empty() => format!("已打开查找并输入：{}", value),
                _ => "已打开页面查找".to_string(),
            }
        }
        skills::BrowserAction::SwitchTabIndex { index } => {
            skills::execute_browser_shortcut_action(&plan.action)?;
            format!("已切换到第 {} 个页面", index)
        }
        other_action => {
            skills::execute_browser_shortcut_action(other_action)?;
            match other_action {
                skills::BrowserAction::NewTab => "已新建浏览器页面".to_string(),
                skills::BrowserAction::CloseTab => "已关闭当前浏览器页面".to_string(),
                skills::BrowserAction::NextTab => "已切换到下一个页面".to_string(),
                skills::BrowserAction::PreviousTab => "已切换到上一个页面".to_string(),
                skills::BrowserAction::ReopenTab => "已重新打开最近关闭的页面".to_string(),
                skills::BrowserAction::GoBack => "已后退".to_string(),
                skills::BrowserAction::GoForward => "已前进".to_string(),
                skills::BrowserAction::Refresh => "已刷新页面".to_string(),
                skills::BrowserAction::HardRefresh => "已强制刷新页面".to_string(),
                skills::BrowserAction::StopLoading => "已停止页面加载".to_string(),
                skills::BrowserAction::GoHome => "已返回主页".to_string(),
                skills::BrowserAction::ScrollUp => "已向上滚动".to_string(),
                skills::BrowserAction::ScrollDown => "已向下滚动".to_string(),
                skills::BrowserAction::ScrollTop => "已滚动到顶部".to_string(),
                skills::BrowserAction::ScrollBottom => "已滚动到底部".to_string(),
                skills::BrowserAction::PageUp => "已向上翻页".to_string(),
                skills::BrowserAction::PageDown => "已向下翻页".to_string(),
                skills::BrowserAction::Fullscreen => "已切换全屏".to_string(),
                skills::BrowserAction::CopyUrl => "已复制当前网址".to_string(),
                skills::BrowserAction::OpenHistory => "已打开历史记录".to_string(),
                skills::BrowserAction::OpenDownloads => "已打开下载列表".to_string(),
                skills::BrowserAction::OpenDevtools => "已打开开发者工具".to_string(),
                skills::BrowserAction::MinimizeWindow => "已最小化浏览器窗口".to_string(),
                skills::BrowserAction::MaximizeWindow => "已最大化浏览器窗口".to_string(),
                skills::BrowserAction::NewPrivateWindow => "已新建隐私窗口".to_string(),
                skills::BrowserAction::CloseOtherTabs => "已执行关闭其他页面".to_string(),
                skills::BrowserAction::CloseTabsToRight => "已执行关闭右侧页面".to_string(),
                skills::BrowserAction::OpenTarget { .. }
                | skills::BrowserAction::Find { .. }
                | skills::BrowserAction::SwitchTabIndex { .. } => unreachable!(),
            }
        }
    };

    window::emit_voice_command_feedback(app_handle, "success", success_message);
    if let Some(note) = note {
        window::emit_voice_command_feedback(app_handle, "info", note);
    }
    Ok(())
}

pub async fn execute_windows_plan<R: Runtime>(
    app_handle: &AppHandle<R>,
    windows_skill: &skills::SkillConfig,
    plan: &skills::WindowsActionPlan,
    config: &AppConfig,
    llm_cancel: &LlmCancelState,
    seq_id: u64,
) -> Result<(), String> {
    let note = plan.note.clone();
    let success_message = match &plan.action {
        skills::WindowsAction::OpenTarget { query } => {
            let (target, message) = resolve_windows_target(
                app_handle,
                windows_skill,
                query,
                config,
                llm_cancel,
                seq_id,
            )
            .await?;
            skills::open_windows_target(&target)?;
            message
        }
        other_action => {
            skills::execute_windows_shortcut_action(other_action)?;
            match other_action {
                skills::WindowsAction::ShowDesktop => "Showed the desktop".to_string(),
                skills::WindowsAction::LockScreen => "Locked the screen".to_string(),
                skills::WindowsAction::OpenRunDialog => "Opened the Run dialog".to_string(),
                skills::WindowsAction::OpenClipboardHistory => {
                    "Opened clipboard history".to_string()
                }
                skills::WindowsAction::OpenQuickSettings => "Opened quick settings".to_string(),
                skills::WindowsAction::OpenNotifications => {
                    "Opened notification center".to_string()
                }
                skills::WindowsAction::OpenTarget { .. } => unreachable!(),
            }
        }
    };

    window::emit_voice_command_feedback(app_handle, "success", success_message);
    if let Some(note) = note {
        window::emit_voice_command_feedback(app_handle, "info", note);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Config skill planning
// ---------------------------------------------------------------------------

pub fn config_skill_requires_more_input(
    transcript: &str,
    skill_match: &skills::SkillMatch,
    next_match: Option<&skills::SkillMatch>,
) -> bool {
    match skill_match.skill_id.as_str() {
        skills::SWITCH_POLISH_SCENE_SKILL_ID => {
            skills::extract_scene_query(transcript, skill_match, next_match).is_empty()
        }
        _ => false,
    }
}

pub fn plan_config_skill_update(
    transcript: &str,
    skill_match: &skills::SkillMatch,
    next_match: Option<&skills::SkillMatch>,
    config: &AppConfig,
) -> Result<ConfigSkillPlan, String> {
    use crate::state::VoiceCommandFeedback;

    match skill_match.skill_id.as_str() {
        skills::SWITCH_POLISH_SCENE_SKILL_ID => {
            let scene_query = skills::extract_scene_query(transcript, skill_match, next_match);
            if scene_query.is_empty() {
                return Ok(ConfigSkillPlan::Feedback(VoiceCommandFeedback {
                    level: "error".to_string(),
                    message: "未识别到要切换的润色场景".to_string(),
                }));
            }

            match skills::resolve_scene(&config.llm_config.profiles, &scene_query) {
                skills::SceneResolveResult::Unique {
                    profile_id,
                    profile_name,
                } => {
                    if config.llm_config.active_profile_id == profile_id {
                        return Ok(ConfigSkillPlan::Feedback(VoiceCommandFeedback {
                            level: "info".to_string(),
                            message: format!("\u{5f53}\u{524d}\u{5df2}\u{7ecf}\u{662f}\u{573a}\u{666f}\u{201c}{}\u{201d}", profile_name),
                        }));
                    }

                    let mut next_config = config.clone();
                    next_config.llm_config.active_profile_id = profile_id;
                    Ok(ConfigSkillPlan::Save {
                        config: Box::new(next_config),
                        feedback: VoiceCommandFeedback {
                            level: "success".to_string(),
                            message: format!("\u{5df2}\u{5207}\u{6362}\u{5230}\u{573a}\u{666f}\u{201c}{}\u{201d}", profile_name),
                        },
                    })
                }
                skills::SceneResolveResult::None => {
                    Ok(ConfigSkillPlan::Feedback(VoiceCommandFeedback {
                        level: "error".to_string(),
                        message: format!("未找到匹配场景：{}", scene_query),
                    }))
                }
                skills::SceneResolveResult::Ambiguous(names) => {
                    Ok(ConfigSkillPlan::Feedback(VoiceCommandFeedback {
                        level: "error".to_string(),
                        message: format!("匹配到多个场景：{}", names.join("\u{3001}")),
                    }))
                }
            }
        }
        _ => Err(format!(
            "Unsupported config skill: {}",
            skill_match.skill_id
        )),
    }
}

pub fn execute_config_skill<R: Runtime>(
    app_handle: &AppHandle<R>,
    transcript: &str,
    skill_match: &skills::SkillMatch,
    next_match: Option<&skills::SkillMatch>,
    config: &mut AppConfig,
) -> Result<(), String> {
    use crate::state::VoiceCommandFeedback;

    match plan_config_skill_update(transcript, skill_match, next_match, config)? {
        ConfigSkillPlan::Save {
            config: next_config,
            feedback,
        } => {
            window::save_and_emit_config_update(app_handle, &next_config)?;
            *config = *next_config;
            let VoiceCommandFeedback { level, message } = feedback;
            window::emit_voice_command_feedback(app_handle, &level, message);
            Ok(())
        }
        ConfigSkillPlan::Feedback(feedback) => {
            let VoiceCommandFeedback { level, message } = feedback;
            window::emit_voice_command_feedback(app_handle, &level, message);
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Reserved plan executors (reserve + execute + complete)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub async fn execute_reserved_windows_plan<R: Runtime>(
    app_handle: &AppHandle<R>,
    skill_state: &SkillExecutionState,
    skill_session_id: u64,
    windows_skill: &skills::SkillConfig,
    plan: &skills::WindowsActionPlan,
    config: &AppConfig,
    llm_cancel: &LlmCancelState,
    seq_id: u64,
) -> bool {
    let action_key = windows_action_key(&plan.action);
    if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
        return false;
    }

    match execute_windows_plan(app_handle, windows_skill, plan, config, llm_cancel, seq_id).await {
        Ok(_) => {
            complete_skill_action(skill_state, skill_session_id, &action_key, true);
            true
        }
        Err(e) => {
            complete_skill_action(skill_state, skill_session_id, &action_key, true);
            window::emit_voice_command_feedback(app_handle, "error", e.clone());
            eprintln!("[SKILL] #{} Windows execution failed: {}", seq_id, e);
            false
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn execute_reserved_browser_plan<R: Runtime>(
    app_handle: &AppHandle<R>,
    skill_state: &SkillExecutionState,
    skill_session_id: u64,
    browser_skill: &skills::SkillConfig,
    plan: &skills::BrowserActionPlan,
    config: &AppConfig,
    llm_cancel: &LlmCancelState,
    seq_id: u64,
) -> bool {
    let action_key = browser_action_key(&plan.action);
    if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
        return false;
    }

    match execute_browser_plan(app_handle, browser_skill, plan, config, llm_cancel, seq_id).await {
        Ok(_) => {
            complete_skill_action(skill_state, skill_session_id, &action_key, true);
            true
        }
        Err(e) => {
            complete_skill_action(skill_state, skill_session_id, &action_key, true);
            window::emit_voice_command_feedback(app_handle, "error", e.clone());
            eprintln!("[SKILL] #{} Browser execution failed: {}", seq_id, e);
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Main skill transcript execution
// ---------------------------------------------------------------------------

pub fn spawn_skill_transcript_processing<R: Runtime>(
    app_handle: &AppHandle<R>,
    text: String,
    llm_cancel: LlmCancelState,
    skill_state: SkillExecutionState,
    skill_session_id: u64,
    seq_id: u64,
) {
    if text.trim().is_empty() {
        return;
    }

    println!(
        "[SKILL] #{} Streaming update: {} chars, preview='{}'",
        seq_id,
        text.len(),
        preview_text(&text, 80)
    );

    let app_handle_clone = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        execute_skill_transcript(
            &app_handle_clone,
            &text,
            &llm_cancel,
            &skill_state,
            skill_session_id,
            seq_id,
            false,
        )
        .await;
    });
}

#[allow(unreachable_code)]
pub async fn execute_skill_transcript<R: Runtime>(
    app_handle: &AppHandle<R>,
    text: &str,
    llm_cancel: &LlmCancelState,
    skill_state: &SkillExecutionState,
    skill_session_id: u64,
    seq_id: u64,
    is_final: bool,
) {
    if text.trim().is_empty() {
        return;
    }

    execute_skill_transcript_streaming(
        app_handle,
        text,
        llm_cancel,
        skill_state,
        skill_session_id,
        seq_id,
        is_final,
    )
    .await;
    return;

    // --- dead code below: original non-streaming implementation preserved for reference ---
    let Some((effective_text, transcript_offset)) =
        prepare_skill_transcript(skill_state, skill_session_id, text)
    else {
        return;
    };

    let storage = app_handle.state::<StorageState>();
    let mut config = storage.load_config();
    let skills_config = config.skills.clone();
    let browser_skill =
        skills::find_skill_config(&skills_config, skills::OPEN_BROWSER_SKILL_ID).cloned();
    let windows_skill =
        skills::find_skill_config(&skills_config, skills::OPEN_WINDOWS_SKILL_ID).cloned();

    let matched_skills = skills::match_skills(&effective_text, &skills_config);
    let mut max_consumed_local_end = 0usize;
    if !matched_skills.is_empty() {
        let matched_ids: Vec<&str> = matched_skills
            .iter()
            .map(|skill_match| skill_match.skill_id.as_str())
            .collect();
        println!(
            "[SKILL] #{} Matched skills: {}",
            seq_id,
            matched_ids.join(", ")
        );

        for (index, skill_match) in matched_skills.iter().enumerate() {
            let next_match = matched_skills.get(index + 1);
            let local_consumed_end = next_match
                .map(|next_skill_match| next_skill_match.start)
                .unwrap_or(effective_text.len());

            if skills::is_config_skill(&skill_match.skill_id) {
                let action_key = format!("config:{}", skill_match.skill_id);
                if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
                    continue;
                }

                match execute_config_skill(
                    app_handle,
                    &effective_text,
                    skill_match,
                    next_match,
                    &mut config,
                ) {
                    Ok(_) => {
                        complete_skill_action(skill_state, skill_session_id, &action_key, true);
                        max_consumed_local_end = max_consumed_local_end.max(local_consumed_end);
                        println!(
                            "[SKILL] #{} Executed config skill successfully: {}",
                            seq_id, skill_match.skill_id
                        );
                    }
                    Err(e) => {
                        complete_skill_action(skill_state, skill_session_id, &action_key, true);
                        window::emit_voice_command_feedback(
                            app_handle,
                            "error",
                            format!("\u{914d}\u{7f6e}\u{66f4}\u{65b0}\u{5931}\u{8d25}\u{ff1a}{}", e),
                        );
                        eprintln!(
                            "[SKILL] #{} Config skill execution failed for {}: {}",
                            seq_id, skill_match.skill_id, e
                        );
                    }
                }
                continue;
            }

            if skill_match.skill_id == skills::OPEN_WINDOWS_SKILL_ID {
                match windows_skill.as_ref() {
                    Some(windows_skill) => match plan_windows_command(
                        &effective_text,
                        windows_skill,
                        Some(skill_match),
                    ) {
                        Ok(Some(plan)) => {
                            if let skills::WindowsAction::OpenTarget { query } = &plan.action {
                                if !is_windows_open_query_ready(windows_skill, query) {
                                    continue;
                                }
                                let action_key = windows_action_key(&plan.action);
                                if !is_final
                                    && !confirm_streaming_browser_open_action(
                                        skill_state,
                                        skill_session_id,
                                        &action_key,
                                    )
                                {
                                    println!(
                                        "[SKILL] #{} Deferred Windows open until transcript stabilizes: {}",
                                        seq_id,
                                        query.trim()
                                    );
                                    continue;
                                }
                            } else {
                                clear_streaming_browser_open_action_candidate(
                                    skill_state,
                                    skill_session_id,
                                );
                            }

                            let action_key = windows_action_key(&plan.action);
                            if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
                                continue;
                            }

                            match execute_windows_plan(
                                app_handle,
                                windows_skill,
                                &plan,
                                &config,
                                llm_cancel,
                                seq_id,
                            )
                            .await
                            {
                                Ok(_) => {
                                    complete_skill_action(
                                        skill_state,
                                        skill_session_id,
                                        &action_key,
                                        true,
                                    );
                                    max_consumed_local_end =
                                        max_consumed_local_end.max(plan.consumed_end);
                                    clear_streaming_browser_open_action_candidate(
                                        skill_state,
                                        skill_session_id,
                                    );
                                    println!(
                                        "[SKILL] #{} Executed Windows command successfully",
                                        seq_id
                                    );
                                }
                                Err(e) => {
                                    complete_skill_action(
                                        skill_state,
                                        skill_session_id,
                                        &action_key,
                                        true,
                                    );
                                    window::emit_voice_command_feedback(app_handle, "error", e.clone());
                                    eprintln!(
                                        "[SKILL] #{} Windows execution failed: {}",
                                        seq_id, e
                                    );
                                }
                            }
                        }
                        Ok(None) => {
                            clear_streaming_browser_open_action_candidate(
                                skill_state,
                                skill_session_id,
                            );
                        }
                        Err(e) => {
                            clear_streaming_browser_open_action_candidate(
                                skill_state,
                                skill_session_id,
                            );
                            window::emit_voice_command_feedback(app_handle, "error", e.clone());
                            eprintln!("[SKILL] #{} Windows plan failed: {}", seq_id, e);
                        }
                    },
                    None => {
                        window::emit_voice_command_feedback(app_handle, "error", "Windows skill missing");
                        eprintln!("[SKILL] #{} Windows skill missing from config", seq_id);
                    }
                }
                continue;
            }

            if skill_match.skill_id == skills::OPEN_BROWSER_SKILL_ID {
                match browser_skill.as_ref() {
                    Some(browser_skill) => match plan_browser_command(
                        &effective_text,
                        browser_skill,
                        Some(skill_match),
                    ) {
                        Ok(Some(plan)) => {
                            if let skills::BrowserAction::OpenTarget { query } = &plan.action {
                                if !is_browser_open_query_ready(browser_skill, query) {
                                    continue;
                                }
                                let action_key = browser_action_key(&plan.action);
                                if !is_final
                                    && !confirm_streaming_browser_open_action(
                                        skill_state,
                                        skill_session_id,
                                        &action_key,
                                    )
                                {
                                    println!(
                                        "[SKILL] #{} Deferred browser open until transcript stabilizes: {}",
                                        seq_id,
                                        query.trim()
                                    );
                                    continue;
                                }
                            } else {
                                clear_streaming_browser_open_action_candidate(
                                    skill_state,
                                    skill_session_id,
                                );
                            }

                            let action_key = browser_action_key(&plan.action);
                            if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
                                continue;
                            }

                            match execute_browser_plan(
                                app_handle,
                                browser_skill,
                                &plan,
                                &config,
                                llm_cancel,
                                seq_id,
                            )
                            .await
                            {
                                Ok(_) => {
                                    complete_skill_action(
                                        skill_state,
                                        skill_session_id,
                                        &action_key,
                                        true,
                                    );
                                    max_consumed_local_end =
                                        max_consumed_local_end.max(plan.consumed_end);
                                    clear_streaming_browser_open_action_candidate(
                                        skill_state,
                                        skill_session_id,
                                    );
                                    println!(
                                        "[SKILL] #{} Executed browser command successfully",
                                        seq_id
                                    );
                                }
                                Err(e) => {
                                    complete_skill_action(
                                        skill_state,
                                        skill_session_id,
                                        &action_key,
                                        true,
                                    );
                                    window::emit_voice_command_feedback(app_handle, "error", e.clone());
                                    eprintln!(
                                        "[SKILL] #{} Browser execution failed: {}",
                                        seq_id, e
                                    );
                                }
                            }
                        }
                        Ok(None) => {
                            clear_streaming_browser_open_action_candidate(
                                skill_state,
                                skill_session_id,
                            );
                        }
                        Err(e) => {
                            clear_streaming_browser_open_action_candidate(
                                skill_state,
                                skill_session_id,
                            );
                            window::emit_voice_command_feedback(app_handle, "error", e.clone());
                            eprintln!("[SKILL] #{} Browser plan failed: {}", seq_id, e);
                        }
                    },
                    None => {
                        window::emit_voice_command_feedback(app_handle, "error", "\u{6d4f}\u{89c8}\u{5668}\u{6280}\u{80fd}\u{672a}\u{914d}\u{7f6e}");
                        eprintln!("[SKILL] #{} Browser skill missing from config", seq_id);
                    }
                }
                continue;
            }

            let action_key = format!("skill:{}", skill_match.skill_id);
            if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
                continue;
            }

            match skills::execute_skill(&skill_match.skill_id) {
                Ok(_) => {
                    complete_skill_action(skill_state, skill_session_id, &action_key, true);
                    max_consumed_local_end = max_consumed_local_end.max(local_consumed_end);
                    println!(
                        "[SKILL] #{} Executed successfully: {}",
                        seq_id, skill_match.skill_id
                    );
                }
                Err(e) => {
                    complete_skill_action(skill_state, skill_session_id, &action_key, true);
                    window::emit_voice_command_feedback(app_handle, "error", e.clone());
                    eprintln!(
                        "[SKILL] #{} Execution failed for {}: {}",
                        seq_id, skill_match.skill_id, e
                    );
                }
            }
        }

        if max_consumed_local_end > 0 {
            advance_skill_transcript_consumed(
                skill_state,
                skill_session_id,
                text,
                transcript_offset + max_consumed_local_end,
            );
        }
        return;
    }

    if let Some(windows_skill) = windows_skill.as_ref() {
        match plan_windows_command(&effective_text, windows_skill, None) {
            Ok(Some(plan)) => {
                if let skills::WindowsAction::OpenTarget { query } = &plan.action {
                    if !is_windows_open_query_ready(windows_skill, query) {
                        return;
                    }
                    let action_key = windows_action_key(&plan.action);
                    if !is_final
                        && !confirm_streaming_browser_open_action(
                            skill_state,
                            skill_session_id,
                            &action_key,
                        )
                    {
                        println!(
                            "[SKILL] #{} Deferred Windows open until transcript stabilizes: {}",
                            seq_id,
                            query.trim()
                        );
                        return;
                    }
                } else {
                    clear_streaming_browser_open_action_candidate(skill_state, skill_session_id);
                }

                let action_key = windows_action_key(&plan.action);
                if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
                    return;
                }

                let is_open_target =
                    matches!(&plan.action, skills::WindowsAction::OpenTarget { .. });
                match execute_windows_plan(
                    app_handle,
                    windows_skill,
                    &plan,
                    &config,
                    llm_cancel,
                    seq_id,
                )
                .await
                {
                    Ok(_) => {
                        complete_skill_action(skill_state, skill_session_id, &action_key, true);
                        advance_skill_transcript_consumed(
                            skill_state,
                            skill_session_id,
                            text,
                            transcript_offset + plan.consumed_end,
                        );
                        clear_streaming_browser_open_action_candidate(
                            skill_state,
                            skill_session_id,
                        );
                        println!("[SKILL] #{} Executed Windows fallback successfully", seq_id);
                        return;
                    }
                    Err(e) => {
                        complete_skill_action(skill_state, skill_session_id, &action_key, true);
                        if is_open_target {
                            eprintln!(
                                "[SKILL] #{} Windows fallback unresolved, trying browser fallback: {}",
                                seq_id, e
                            );
                        } else {
                            window::emit_voice_command_feedback(app_handle, "error", e.clone());
                            eprintln!("[SKILL] #{} Windows fallback failed: {}", seq_id, e);
                            return;
                        }
                    }
                }
            }
            Ok(None) => {}
            Err(_) => {}
        }
    }

    if let Some(browser_skill) = browser_skill.as_ref() {
        match plan_browser_command(&effective_text, browser_skill, None) {
            Ok(Some(plan)) => {
                if let skills::BrowserAction::OpenTarget { query } = &plan.action {
                    if !is_browser_open_query_ready(browser_skill, query) {
                        return;
                    }
                    let action_key = browser_action_key(&plan.action);
                    if !is_final
                        && !confirm_streaming_browser_open_action(
                            skill_state,
                            skill_session_id,
                            &action_key,
                        )
                    {
                        println!(
                            "[SKILL] #{} Deferred browser open until transcript stabilizes: {}",
                            seq_id,
                            query.trim()
                        );
                        return;
                    }
                } else {
                    clear_streaming_browser_open_action_candidate(skill_state, skill_session_id);
                }

                let action_key = browser_action_key(&plan.action);
                if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
                    return;
                }

                match execute_browser_plan(
                    app_handle,
                    browser_skill,
                    &plan,
                    &config,
                    llm_cancel,
                    seq_id,
                )
                .await
                {
                    Ok(_) => {
                        complete_skill_action(skill_state, skill_session_id, &action_key, true);
                        advance_skill_transcript_consumed(
                            skill_state,
                            skill_session_id,
                            text,
                            transcript_offset + plan.consumed_end,
                        );
                        clear_streaming_browser_open_action_candidate(
                            skill_state,
                            skill_session_id,
                        );
                        println!("[SKILL] #{} Executed browser fallback successfully", seq_id);
                    }
                    Err(e) => {
                        complete_skill_action(skill_state, skill_session_id, &action_key, true);
                        window::emit_voice_command_feedback(app_handle, "error", e.clone());
                        eprintln!("[SKILL] #{} Browser fallback failed: {}", seq_id, e);
                        return;
                    }
                }
            }
            Ok(None) => {
                clear_streaming_browser_open_action_candidate(skill_state, skill_session_id);
            }
            Err(_) => {
                clear_streaming_browser_open_action_candidate(skill_state, skill_session_id);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Streaming skill transcript execution
// ---------------------------------------------------------------------------

async fn execute_skill_transcript_streaming<R: Runtime>(
    app_handle: &AppHandle<R>,
    text: &str,
    llm_cancel: &LlmCancelState,
    skill_state: &SkillExecutionState,
    skill_session_id: u64,
    seq_id: u64,
    is_final: bool,
) {
    let storage = app_handle.state::<StorageState>();
    let mut config = storage.load_config();

    loop {
        let Some((effective_text, transcript_offset)) =
            prepare_skill_transcript(skill_state, skill_session_id, text)
        else {
            return;
        };

        let (clause_text, clause_consumed_end) = split_skill_clause(&effective_text);
        let clause_text = clause_text.trim();
        if clause_text.is_empty() {
            advance_skill_transcript_consumed(
                skill_state,
                skill_session_id,
                text,
                transcript_offset + clause_consumed_end,
            );
            continue;
        }

        let skills_config = config.skills.clone();
        let browser_skill =
            skills::find_skill_config(&skills_config, skills::OPEN_BROWSER_SKILL_ID).cloned();
        let windows_skill =
            skills::find_skill_config(&skills_config, skills::OPEN_WINDOWS_SKILL_ID).cloned();
        let matched_skills = skills::match_skills(clause_text, &skills_config);
        let mut consumed_local_end = 0usize;

        if !matched_skills.is_empty() {
            let matched_ids: Vec<&str> = matched_skills
                .iter()
                .map(|skill_match| skill_match.skill_id.as_str())
                .collect();
            println!(
                "[SKILL] #{} Matched skills: {}",
                seq_id,
                matched_ids.join(", ")
            );

            for (index, skill_match) in matched_skills.iter().enumerate() {
                let next_match = matched_skills.get(index + 1);
                let local_consumed_end = next_match
                    .map(|next_skill_match| next_skill_match.start)
                    .unwrap_or(clause_text.len());

                if skills::is_config_skill(&skill_match.skill_id) {
                    if !is_final {
                        let wait_reason = if config_skill_requires_more_input(
                            clause_text,
                            skill_match,
                            next_match,
                        ) {
                            "more input"
                        } else {
                            "final transcript"
                        };
                        println!(
                            "[SKILL] #{} Waiting for {} before executing config skill: {}",
                            seq_id, wait_reason, skill_match.skill_id
                        );
                        return;
                    }

                    let action_key = format!("config:{}", skill_match.skill_id);
                    if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
                        continue;
                    }

                    match execute_config_skill(
                        app_handle,
                        clause_text,
                        skill_match,
                        next_match,
                        &mut config,
                    ) {
                        Ok(_) => {
                            complete_skill_action(skill_state, skill_session_id, &action_key, true);
                            consumed_local_end = consumed_local_end.max(local_consumed_end);
                        }
                        Err(e) => {
                            complete_skill_action(skill_state, skill_session_id, &action_key, true);
                            window::emit_voice_command_feedback(
                                app_handle,
                                "error",
                                format!("\u{914d}\u{7f6e}\u{66f4}\u{65b0}\u{5931}\u{8d25}: {}", e),
                            );
                            eprintln!(
                                "[SKILL] #{} Config skill execution failed for {}: {}",
                                seq_id, skill_match.skill_id, e
                            );
                        }
                    }
                    continue;
                }

                if skill_match.skill_id == skills::OPEN_WINDOWS_SKILL_ID {
                    let Some(windows_skill) = windows_skill.as_ref() else {
                        window::emit_voice_command_feedback(app_handle, "error", "Windows skill missing");
                        eprintln!("[SKILL] #{} Windows skill missing from config", seq_id);
                        continue;
                    };

                    match plan_windows_command(clause_text, windows_skill, Some(skill_match)) {
                        Ok(Some(plan)) => {
                            let should_wait_for_stability = matches!(
                                &plan.action,
                                skills::WindowsAction::OpenTarget { query }
                                    if resolve_windows_target_candidate(windows_skill, query).is_none()
                            );
                            if let skills::WindowsAction::OpenTarget { query } = &plan.action {
                                if !is_windows_open_query_ready(windows_skill, query) {
                                    continue;
                                }
                                if !is_final
                                    && should_wait_for_stability
                                    && !confirm_streaming_browser_open_action(
                                        skill_state,
                                        skill_session_id,
                                        &windows_action_key(&plan.action),
                                    )
                                {
                                    println!(
                                        "[SKILL] #{} Deferred Windows open until transcript stabilizes: {}",
                                        seq_id,
                                        query.trim()
                                    );
                                    return;
                                }
                            } else {
                                clear_streaming_browser_open_action_candidate(
                                    skill_state,
                                    skill_session_id,
                                );
                            }

                            if execute_reserved_windows_plan(
                                app_handle,
                                skill_state,
                                skill_session_id,
                                windows_skill,
                                &plan,
                                &config,
                                llm_cancel,
                                seq_id,
                            )
                            .await
                            {
                                consumed_local_end = consumed_local_end.max(plan.consumed_end);
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            window::emit_voice_command_feedback(app_handle, "error", e.clone());
                            eprintln!("[SKILL] #{} Windows plan failed: {}", seq_id, e);
                        }
                    }
                    continue;
                }

                if skill_match.skill_id == skills::OPEN_BROWSER_SKILL_ID {
                    let Some(browser_skill) = browser_skill.as_ref() else {
                        window::emit_voice_command_feedback(app_handle, "error", "\u{6d4f}\u{89c8}\u{5668}\u{6280}\u{80fd}\u{672a}\u{914d}\u{7f6e}");
                        eprintln!("[SKILL] #{} Browser skill missing from config", seq_id);
                        continue;
                    };

                    match plan_browser_command(clause_text, browser_skill, Some(skill_match)) {
                        Ok(Some(plan)) => {
                            if let skills::BrowserAction::OpenTarget { query } = &plan.action {
                                if !is_browser_open_query_ready(browser_skill, query) {
                                    continue;
                                }
                                if !is_final
                                    && !confirm_streaming_browser_open_action(
                                        skill_state,
                                        skill_session_id,
                                        &browser_action_key(&plan.action),
                                    )
                                {
                                    println!(
                                        "[SKILL] #{} Deferred browser open until transcript stabilizes: {}",
                                        seq_id,
                                        query.trim()
                                    );
                                    return;
                                }
                            } else {
                                clear_streaming_browser_open_action_candidate(
                                    skill_state,
                                    skill_session_id,
                                );
                            }

                            if execute_reserved_browser_plan(
                                app_handle,
                                skill_state,
                                skill_session_id,
                                browser_skill,
                                &plan,
                                &config,
                                llm_cancel,
                                seq_id,
                            )
                            .await
                            {
                                consumed_local_end = consumed_local_end.max(plan.consumed_end);
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            window::emit_voice_command_feedback(app_handle, "error", e.clone());
                            eprintln!("[SKILL] #{} Browser plan failed: {}", seq_id, e);
                        }
                    }
                    continue;
                }

                let action_key = format!("skill:{}", skill_match.skill_id);
                if !reserve_skill_action(skill_state, skill_session_id, &action_key) {
                    continue;
                }

                match skills::execute_skill(&skill_match.skill_id) {
                    Ok(_) => {
                        complete_skill_action(skill_state, skill_session_id, &action_key, true);
                        consumed_local_end = consumed_local_end.max(local_consumed_end);
                    }
                    Err(e) => {
                        complete_skill_action(skill_state, skill_session_id, &action_key, true);
                        window::emit_voice_command_feedback(app_handle, "error", e.clone());
                        eprintln!(
                            "[SKILL] #{} Execution failed for {}: {}",
                            seq_id, skill_match.skill_id, e
                        );
                    }
                }
            }

            if consumed_local_end > 0 {
                advance_skill_transcript_consumed(
                    skill_state,
                    skill_session_id,
                    text,
                    transcript_offset + consumed_local_end.min(clause_consumed_end),
                );
                continue;
            }

            return;
        }

        if let Some(windows_skill) = windows_skill.as_ref() {
            if let Some(plan) = plan_direct_windows_target(clause_text, windows_skill) {
                if execute_reserved_windows_plan(
                    app_handle,
                    skill_state,
                    skill_session_id,
                    windows_skill,
                    &plan,
                    &config,
                    llm_cancel,
                    seq_id,
                )
                .await
                {
                    advance_skill_transcript_consumed(
                        skill_state,
                        skill_session_id,
                        text,
                        transcript_offset + clause_consumed_end,
                    );
                    clear_streaming_browser_open_action_candidate(skill_state, skill_session_id);
                    println!(
                        "[SKILL] #{} Executed direct Windows target successfully",
                        seq_id
                    );
                    continue;
                }
            }

            match plan_windows_command(clause_text, windows_skill, None) {
                Ok(Some(plan)) => {
                    if let skills::WindowsAction::OpenTarget { query } = &plan.action {
                        if !is_windows_open_query_ready(windows_skill, query) {
                            return;
                        }
                        let requires_stable_confirmation =
                            resolve_windows_target_candidate(windows_skill, query).is_none();
                        if !is_final
                            && requires_stable_confirmation
                            && !confirm_streaming_browser_open_action(
                                skill_state,
                                skill_session_id,
                                &windows_action_key(&plan.action),
                            )
                        {
                            println!(
                                "[SKILL] #{} Deferred Windows open until transcript stabilizes: {}",
                                seq_id,
                                query.trim()
                            );
                            return;
                        }
                    } else {
                        clear_streaming_browser_open_action_candidate(
                            skill_state,
                            skill_session_id,
                        );
                    }

                    if execute_reserved_windows_plan(
                        app_handle,
                        skill_state,
                        skill_session_id,
                        windows_skill,
                        &plan,
                        &config,
                        llm_cancel,
                        seq_id,
                    )
                    .await
                    {
                        advance_skill_transcript_consumed(
                            skill_state,
                            skill_session_id,
                            text,
                            transcript_offset + plan.consumed_end.min(clause_consumed_end),
                        );
                        clear_streaming_browser_open_action_candidate(
                            skill_state,
                            skill_session_id,
                        );
                        println!("[SKILL] #{} Executed Windows fallback successfully", seq_id);
                        continue;
                    }
                }
                Ok(None) => {}
                Err(_) => {}
            }
        }

        if let Some(browser_skill) = browser_skill.as_ref() {
            match plan_browser_command(clause_text, browser_skill, None) {
                Ok(Some(plan)) => {
                    if let skills::BrowserAction::OpenTarget { query } = &plan.action {
                        if !is_browser_open_query_ready(browser_skill, query) {
                            return;
                        }
                        if !is_final
                            && !confirm_streaming_browser_open_action(
                                skill_state,
                                skill_session_id,
                                &browser_action_key(&plan.action),
                            )
                        {
                            println!(
                                "[SKILL] #{} Deferred browser open until transcript stabilizes: {}",
                                seq_id,
                                query.trim()
                            );
                            return;
                        }
                    } else {
                        clear_streaming_browser_open_action_candidate(
                            skill_state,
                            skill_session_id,
                        );
                    }

                    if execute_reserved_browser_plan(
                        app_handle,
                        skill_state,
                        skill_session_id,
                        browser_skill,
                        &plan,
                        &config,
                        llm_cancel,
                        seq_id,
                    )
                    .await
                    {
                        advance_skill_transcript_consumed(
                            skill_state,
                            skill_session_id,
                            text,
                            transcript_offset + plan.consumed_end.min(clause_consumed_end),
                        );
                        clear_streaming_browser_open_action_candidate(
                            skill_state,
                            skill_session_id,
                        );
                        println!("[SKILL] #{} Executed browser fallback successfully", seq_id);
                        continue;
                    }
                }
                Ok(None) => {
                    clear_streaming_browser_open_action_candidate(skill_state, skill_session_id);
                    println!(
                        "[SKILL] #{} No skill matched for text: '{}'",
                        seq_id,
                        preview_text(clause_text, 40)
                    );
                }
                Err(_) => {}
            }
        }

        if clause_consumed_end < effective_text.len() {
            advance_skill_transcript_consumed(
                skill_state,
                skill_session_id,
                text,
                transcript_offset + clause_consumed_end,
            );
            continue;
        }

        return;
    }
}
