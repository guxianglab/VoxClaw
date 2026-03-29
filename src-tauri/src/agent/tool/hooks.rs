use async_trait::async_trait;
use crate::agent::core::message::{ToolCall, ToolResult};
use crate::agent::error::AgentError;
use crate::storage::SafetyRule;
use super::{Tool, HookDecision};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::{Mutex, oneshot};
use serde_json::{json, Value};

/// Trait for intercepting tool calls.
#[async_trait]
pub trait ToolHook: Send + Sync {
    async fn before_call(&self, call: &ToolCall, tool: &dyn Tool) -> HookDecision;
    async fn after_call(&self, call: &ToolCall, result: &mut ToolResult) -> Result<(), AgentError>;
}

/// Logs all tool calls to stdout.
pub struct LoggingHook;

#[async_trait]
impl ToolHook for LoggingHook {
    async fn before_call(&self, call: &ToolCall, _tool: &dyn Tool) -> HookDecision {
        println!("[TOOL] Calling {} (id={})", call.name, call.id);
        HookDecision::Allow
    }

    async fn after_call(&self, _call: &ToolCall, result: &mut ToolResult) -> Result<(), AgentError> {
        let status = if result.is_error { "ERROR" } else { "OK" };
        println!("[TOOL] {} {}", result.tool_name, status);
        Ok(())
    }
}

/// Validates tool call arguments are valid JSON objects.
#[allow(dead_code)]
pub struct ValidationHook;

#[async_trait]
impl ToolHook for ValidationHook {
    async fn before_call(&self, call: &ToolCall, _tool: &dyn Tool) -> HookDecision {
        if serde_json::from_str::<Value>(&call.arguments).ok()
            .and_then(|v| if v.is_object() { Some(()) } else { None })
            .is_some()
        {
            HookDecision::Allow
        } else {
            HookDecision::Block { reason: "Invalid JSON arguments".into() }
        }
    }

    async fn after_call(&self, _call: &ToolCall, _result: &mut ToolResult) -> Result<(), AgentError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Pure helper functions for rule evaluation (no generics needed)
// ---------------------------------------------------------------------------

fn action_to_decision(action: &str) -> HookDecision {
    match action {
        "allow" => HookDecision::Allow,
        "deny" => HookDecision::Block {
            reason: "Blocked by safety rule (deny)".into(),
        },
        _ => HookDecision::Allow,
    }
}

/// Check if a command matches a pattern.
/// - Leading `!` is a literal prefix marker (stripped before matching).
/// - Trailing `*` enables prefix matching.
/// - Otherwise exact match.
fn pattern_matches_command(pattern: &str, command: &str) -> bool {
    let core = pattern.strip_prefix('!').unwrap_or(pattern);

    if core.ends_with('*') {
        let prefix = &core[..core.len() - 1];
        command.starts_with(prefix)
    } else {
        command == core
    }
}

/// Check if a path falls within any of the scope directories.
/// Expands `~/` to the user's home directory.
fn path_in_scope(path: &str, scope: &[String]) -> bool {
    let expanded_scopes: Vec<String> = scope
        .iter()
        .map(|s| {
            if s.starts_with("~/") {
                let home = home_dir();
                format!("{}{}", home, &s[2..])
            } else {
                s.clone()
            }
        })
        .collect();

    let path_normalized = path.replace('\\', "/");
    expanded_scopes.iter().any(|scope_dir| {
        let scope_normalized = scope_dir.replace('\\', "/");
        path_normalized.starts_with(&scope_normalized)
    })
}

fn home_dir() -> String {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string())
}

fn build_summary(tool_name: &str, args: &Value) -> String {
    match tool_name {
        "bash" => {
            let cmd = args["command"].as_str().unwrap_or("(unknown)");
            format!("bash: {}", cmd)
        }
        "file_write" => {
            let path = args["path"].as_str().unwrap_or("(unknown)");
            format!("file_write: {}", path)
        }
        "file_edit" => {
            let path = args["path"].as_str().unwrap_or("(unknown)");
            format!("file_edit: {}", path)
        }
        _ => format!("{}: {}", tool_name, args),
    }
}

/// Evaluate rules in 4-level priority against a tool call.
/// Returns `Some(HookDecision)` if a rule matched, `None` otherwise.
pub fn evaluate_safety_rules(
    rules: &[SafetyRule],
    tool_name: &str,
    command: &str,
    path: &str,
) -> Option<HookDecision> {
    // Priority 1: tool + command_pattern match
    for rule in rules.iter() {
        if rule.tool != tool_name {
            continue;
        }
        if let Some(ref pattern) = rule.command_pattern {
            if pattern_matches_command(pattern, command) {
                return Some(action_to_decision(&rule.action));
            }
        }
    }

    // Priority 2: tool + path_scope match
    for rule in rules.iter() {
        if rule.tool != tool_name {
            continue;
        }
        if let Some(ref scope) = rule.path_scope {
            if !path.is_empty() && path_in_scope(path, scope) {
                return Some(action_to_decision(&rule.action));
            }
        }
    }

    // Priority 3: tool only (no pattern, no path)
    for rule in rules.iter() {
        if rule.tool != tool_name {
            continue;
        }
        if rule.command_pattern.is_none() && rule.path_scope.is_none() {
            return Some(action_to_decision(&rule.action));
        }
    }

    // Priority 4: wildcard "*" only (no pattern, no path)
    for rule in rules.iter() {
        if rule.tool != "*" {
            continue;
        }
        if rule.command_pattern.is_none() && rule.path_scope.is_none() {
            return Some(action_to_decision(&rule.action));
        }
    }

    None
}

// ---------------------------------------------------------------------------
// SafetyHook
// ---------------------------------------------------------------------------

/// Evaluates safety rules and requests user confirmation when needed.
pub struct SafetyHook<R: Runtime> {
    pub(crate) rules: Arc<Mutex<Vec<SafetyRule>>>,
    pub(crate) default_policy: String,
    pub(crate) app_handle: AppHandle<R>,
    pub(crate) pending: Arc<StdMutex<HashMap<String, oneshot::Sender<HookDecision>>>>,
}

impl<R: Runtime> SafetyHook<R> {
    const CONFIRMATION_TIMEOUT_SECS: u64 = 30;

    pub fn new(
        rules: Arc<Mutex<Vec<SafetyRule>>>,
        default_policy: String,
        app_handle: AppHandle<R>,
        pending: Arc<StdMutex<HashMap<String, oneshot::Sender<HookDecision>>>>,
    ) -> Self {
        Self {
            rules,
            default_policy,
            app_handle,
            pending,
        }
    }

    /// Resolve a pending confirmation request. Called from Tauri command.
    #[allow(dead_code)]
    pub fn resolve(&self, id: &str, decision: HookDecision) {
        if let Some(sender) = self.pending.lock().ok().and_then(|mut map| map.remove(id)) {
            let _ = sender.send(decision);
        }
    }

    /// Add a rule at runtime.
    #[allow(dead_code)]
    pub async fn add_rule(&self, rule: SafetyRule) {
        self.rules.lock().await.push(rule);
    }

    /// Emit a confirmation request event and await the user's response.
    async fn request_confirmation(&self, tool_name: &str, args: &Value) -> HookDecision {
        let id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        {
            let mut pending = match self.pending.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    return HookDecision::Block {
                        reason: "Safety hook internal error (pending lock poisoned)".into(),
                    };
                }
            };
            pending.insert(id.clone(), tx);
        }

        let summary = build_summary(tool_name, args);
        let _ = self.app_handle.emit(
            "confirmation_request",
            serde_json::json!({
                "id": id,
                "tool": tool_name,
                "summary": summary,
                "timeout_ms": Self::CONFIRMATION_TIMEOUT_SECS * 1000,
            }),
        );

        match tokio::time::timeout(std::time::Duration::from_secs(Self::CONFIRMATION_TIMEOUT_SECS), rx).await {
            Ok(Ok(decision)) => decision,
            Ok(Err(_)) => HookDecision::Block {
                reason: "Confirmation channel closed unexpectedly".into(),
            },
            Err(_) => {
                // Timeout -- remove from pending and block
                self.pending.lock().ok().and_then(|mut map| map.remove(&id));
                HookDecision::Block {
                    reason: format!("操作已被拒绝：用户未在 {} 秒内确认，该操作被自动取消。请尝试其他方案。", Self::CONFIRMATION_TIMEOUT_SECS),
                }
            }
        }
    }
}

#[async_trait]
impl<R: Runtime> ToolHook for SafetyHook<R> {
    async fn before_call(&self, call: &ToolCall, _tool: &dyn Tool) -> HookDecision {
        let args: Value = serde_json::from_str(&call.arguments).unwrap_or(json!({}));
        let command = args["command"].as_str().unwrap_or("").to_string();
        let path = args["path"].as_str().unwrap_or("").to_string();

        let rules = self.rules.lock().await;
        let decision = evaluate_safety_rules(&rules, &call.name, &command, &path);

        match decision {
            Some(HookDecision::Allow) => HookDecision::Allow,
            Some(HookDecision::Block { reason }) => HookDecision::Block { reason },
            _ => {
                // No rule matched -- fall back to default policy
                match self.default_policy.as_str() {
                    "deny" => HookDecision::Block {
                        reason: "Blocked by default safety policy (deny)".into(),
                    },
                    "allow" => HookDecision::Allow,
                    _ => {
                        // "confirm" -- request user confirmation
                        drop(rules); // release lock before async await
                        self.request_confirmation(&call.name, &args).await
                    }
                }
            }
        }
    }

    async fn after_call(&self, _call: &ToolCall, _result: &mut ToolResult) -> Result<(), AgentError> {
        Ok(())
    }
}

#[cfg(test)]
mod safety_tests {
    use super::*;
    use crate::storage::SafetyRule;

    fn make_rule(tool: &str, action: &str, pattern: Option<&str>, paths: Option<Vec<&str>>) -> SafetyRule {
        SafetyRule {
            tool: tool.to_string(),
            action: action.to_string(),
            command_pattern: pattern.map(|s| s.to_string()),
            path_scope: paths.map(|v| v.into_iter().map(String::from).collect()),
        }
    }

    fn match_call(rules: &[SafetyRule], tool_name: &str, command: &str, path: &str) -> Option<HookDecision> {
        evaluate_safety_rules(rules, tool_name, command, path)
    }

    #[test]
    fn wildcard_deny_blocks_everything() {
        let rules = vec![make_rule("*", "deny", None, None)];
        let result = match_call(&rules, "bash", "echo hi", "");
        assert!(matches!(result, Some(HookDecision::Block { .. })));
    }

    #[test]
    fn specific_allow_overrides_wildcard() {
        let rules = vec![
            make_rule("*", "deny", None, None),
            make_rule("bash", "allow", None, None),
        ];
        let result = match_call(&rules, "bash", "echo hi", "");
        assert!(matches!(result, Some(HookDecision::Allow)));
    }

    #[test]
    fn command_pattern_allow() {
        let rules = vec![
            make_rule("bash", "allow", Some("echo *"), None),
            make_rule("bash", "deny", None, None),
        ];
        assert!(matches!(match_call(&rules, "bash", "echo hello", ""), Some(HookDecision::Allow)));
        assert!(matches!(match_call(&rules, "bash", "del file.txt", ""), Some(HookDecision::Block { .. })));
    }

    #[test]
    fn command_pattern_deny_with_bang() {
        let rules = vec![
            make_rule("bash", "deny", Some("!del *"), None),
            make_rule("bash", "allow", None, None),
        ];
        assert!(matches!(match_call(&rules, "bash", "del /s file", ""), Some(HookDecision::Block { .. })));
        assert!(matches!(match_call(&rules, "bash", "echo hi", ""), Some(HookDecision::Allow)));
    }

    #[test]
    fn path_scope_deny_outside_scope() {
        let rules = vec![
            make_rule("file_write", "allow", None, Some(vec!["~/Desktop", "~/Downloads"])),
            make_rule("file_write", "deny", None, None),
        ];
        // Path that doesn't match scope should fall through to the deny rule
        let result = match_call(&rules, "file_write", "", "C:\\Unknown\\Path\\test.md");
        assert!(matches!(result, Some(HookDecision::Block { .. })));
    }

    #[test]
    fn no_matching_rules_returns_none() {
        let rules: Vec<SafetyRule> = vec![];
        let result = match_call(&rules, "bash", "echo hi", "");
        assert!(result.is_none());
    }
}
