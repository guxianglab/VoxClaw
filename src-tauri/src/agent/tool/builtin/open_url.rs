use std::net::IpAddr;

use async_trait::async_trait;
use reqwest::Url;
use serde_json::{json, Value};
use tokio::process::Command;

use crate::agent::error::AgentError;
use crate::agent::tool::{Tool, ToolContext, ToolOutput};

pub struct OpenUrlTool;

#[async_trait]
impl Tool for OpenUrlTool {
    fn name(&self) -> &str {
        "open_url"
    }

    fn description(&self) -> &str {
        "Open a public http/https URL in the default browser."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "The http/https URL to open" }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, AgentError> {
        let url = args["url"].as_str().unwrap_or("");
        // Validate strictly (http/https only, rejects private/link-local IPs
        // and shell metacharacters) and quote the argument so cmd never
        // interprets the URL as additional commands.
        let normalized = normalize_browser_url(url)
            .map_err(|e| AgentError::Tool(format!("Invalid URL: {}", e)))?;
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", &format!("\"{}\"", normalized)]);
        let status = cmd
            .status()
            .await
            .map_err(|e| AgentError::Tool(format!("Failed to open URL: {}", e)))?;

        if status.success() {
            Ok(ToolOutput::Text(format!("Opened {}", normalized)))
        } else {
            Ok(ToolOutput::Error("Failed to open URL".into()))
        }
    }
}

/// Validate a URL for opening in the browser: http/https only, no shell
/// metacharacters, no private/link-local IPs. Bare hosts get `https://`.
pub fn normalize_browser_url(raw: &str) -> Result<String, String> {
    let candidate = raw.trim();
    if candidate.is_empty() {
        return Err("URL 为空".to_string());
    }
    if candidate.chars().any(char::is_whitespace) {
        return Err("URL 包含空白字符".to_string());
    }

    if let Ok(url) = Url::parse(candidate) {
        return validate_safe_browser_url(url);
    }

    if candidate.contains("://") {
        return Err("仅支持 http 或 https 地址".to_string());
    }

    let url = Url::parse(&format!("https://{}", candidate))
        .map_err(|_| "无法识别为可打开的网址".to_string())?;
    let host = url.host_str().unwrap_or_default();
    let likely_public = host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok()
        || (host.contains('.') && !host.starts_with('.') && !host.ends_with('.'));
    if !likely_public {
        return Err("无法识别为可打开的网址".to_string());
    }

    validate_safe_browser_url(url)
}

fn validate_safe_browser_url(url: Url) -> Result<String, String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err("仅支持 http 或 https 地址".to_string());
    }
    // Reject shell metacharacters outright: even quoted args must not contain
    // `"` or `^` (cmd's escape char).
    let raw = url.as_str();
    if raw.contains('"') || raw.contains('^') {
        return Err("URL 包含非法字符".to_string());
    }
    // Reject non-public IPs (loopback/private/link-local/CGNAT/metadata ranges
    // like 169.254.169.254) so model-chosen URLs can't point at internal
    // infrastructure. Localhost hostnames are still allowed.
    if let Some(Ok(ip)) = url.host_str().map(str::parse::<IpAddr>) {
        if is_private_ip(&ip) {
            return Err("不支持打开内网或链路本地地址".to_string());
        }
    }
    Ok(url.to_string())
}

/// True for loopback, private, link-local, CGNAT and unspecified ranges.
fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 0 || o[0] == 10 || o[0] == 127
                || (o[0] == 169 && o[1] == 254)
                || (o[0] == 172 && (16..=31).contains(&o[1]))
                || (o[0] == 192 && o[1] == 168)
                || (o[0] == 100 && (o[1] & 0xC0) == 0x40)
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            // ::1, ::
            (s[..7].iter().all(|&v| v == 0) && s[7] <= 1)
                // fc00::/7
                || (s[0] & 0xfe00) == 0xfc00
                // fe80::/10
                || (s[0] & 0xffc0) == 0xfe80
        }
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_browser_url;

    #[test]
    fn bare_host_gets_https() {
        assert_eq!(normalize_browser_url("github.com").unwrap(), "https://github.com/");
    }

    #[test]
    fn rejects_unsafe_urls() {
        assert!(normalize_browser_url("javascript:alert(1)").is_err());
        assert!(normalize_browser_url("file:///C:/Windows").is_err());
        assert!(normalize_browser_url("http://169.254.169.254/").is_err());
    }
}
