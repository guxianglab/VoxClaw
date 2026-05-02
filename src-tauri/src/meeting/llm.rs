//! LLM correction + summarisation for meeting transcripts.
//!
//! Reuses `agent::create_provider` so we transparently support OpenAI-compatible
//! providers and Anthropic. Both helpers stream results and concatenate the
//! text deltas into a single string.

use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::agent;
use crate::agent::core::request::{
    LlmMessage, LlmRequest, LlmStreamEvent, ThinkingConfig, ThinkingLevel,
};
use crate::storage::{AgentConfig, LlmConfig, MeetingSummary, ProxyConfig};

/// Resolve provider settings. Mirrors the dictation agent path: any empty
/// agent_config field falls back to the matching llm_config field.
struct Resolved<'a> {
    provider_type: &'a str,
    base_url: &'a str,
    api_key: &'a str,
    model: &'a str,
}

fn resolve<'a>(agent_config: &'a AgentConfig, llm_config: &'a LlmConfig) -> Resolved<'a> {
    Resolved {
        provider_type: if agent_config.provider_type.is_empty() {
            "openai_compatible"
        } else {
            &agent_config.provider_type
        },
        base_url: if agent_config.provider_base_url.is_empty() {
            &llm_config.base_url
        } else {
            &agent_config.provider_base_url
        },
        api_key: if agent_config.provider_api_key.is_empty() {
            &llm_config.api_key
        } else {
            &agent_config.provider_api_key
        },
        model: if agent_config.provider_model.is_empty() {
            &llm_config.model
        } else {
            &agent_config.provider_model
        },
    }
}

async fn run_oneshot(
    system_prompt: &str,
    user_prompt: &str,
    agent_config: &AgentConfig,
    llm_config: &LlmConfig,
    proxy: &ProxyConfig,
    max_tokens: u32,
) -> Result<String> {
    let r = resolve(agent_config, llm_config);
    if r.base_url.trim().is_empty() || r.api_key.trim().is_empty() || r.model.trim().is_empty() {
        return Err(anyhow!("LLM is not configured"));
    }

    let provider =
        agent::create_provider(r.provider_type, r.base_url, r.api_key, r.model, proxy)
            .map_err(|e| anyhow!("failed to create LLM provider: {e}"))?;

    let request = LlmRequest {
        model: r.model.to_string(),
        messages: vec![
            LlmMessage::system(system_prompt),
            LlmMessage::user(user_prompt),
        ],
        tools: vec![],
        thinking: Some(ThinkingConfig {
            level: ThinkingLevel::None,
            budget_tokens: None,
        }),
        max_tokens: Some(max_tokens),
        temperature: Some(0.2),
    };

    let mut stream = provider
        .stream(request)
        .await
        .map_err(|e| anyhow!("LLM stream failed: {e}"))?;

    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event.map_err(|e| anyhow!("LLM stream error: {e}"))? {
            LlmStreamEvent::TextDelta(delta) => text.push_str(&delta),
            LlmStreamEvent::Done { .. } => break,
            _ => {}
        }
    }
    Ok(text)
}

const CORRECTION_SYSTEM: &str = "你是一名专业的会议记录编辑。你的任务是对自动语音识别（ASR）产出的中文/英文会议转写文本进行轻度润色：\n\n1. 修正明显的同音/近音错别字与漏字。\n2. 补全标点、拆分长段、调整段落以提升可读性。\n3. 当判断说话人切换时，使用 “说话人A：” / “说话人B：” 等占位标签换行（仅在原文有明显切换迹象时）。\n4. 删除明显没有语义的 ASR 噪声，例如孤立的乱码、重复碎片、明显误识别且不承载语义的长数字串或字符串；如果无法确定是否为噪声，则保留原文。\n5. 严禁改写、扩写、概括、翻译或加入原文未提及的信息。\n6. 输出只包含润色后的会议正文，不要解释、不要前言、不要使用 markdown 标题。";

const SUMMARY_SYSTEM: &str = "你是一名会议纪要助理。基于提供的会议转写，输出一个 JSON 对象，字段固定为：\n{\n  \"title\": \"<不超过 30 字的中文标题>\",\n  \"key_points\": [\"要点1\", \"要点2\", ...],\n  \"todos\": [\"待办1\", ...],\n  \"decisions\": [\"决议1\", ...]\n}\n\n严格要求：\n- 只输出合法 JSON，不要使用 markdown 代码块包裹。\n- 列表字段如果没有内容，输出空数组 []。\n- 内容必须来自原文，不要编造。\n- title 必须能概括整场会议主题。";

/// Polish the raw transcript. Returns the corrected text.
pub async fn correct_transcript(
    raw_text: &str,
    agent_config: &AgentConfig,
    llm_config: &LlmConfig,
    proxy: &ProxyConfig,
) -> Result<String> {
    let user_prompt = format!("以下是 ASR 原始转写，请按照系统指令进行润色：\n\n```\n{}\n```", raw_text);
    run_oneshot(CORRECTION_SYSTEM, &user_prompt, agent_config, llm_config, proxy, 4096).await
}

#[derive(Serialize, Deserialize)]
struct SummaryJson {
    #[serde(default)]
    title: String,
    #[serde(default)]
    key_points: Vec<String>,
    #[serde(default)]
    todos: Vec<String>,
    #[serde(default)]
    decisions: Vec<String>,
}

/// Summarise the (preferably corrected) transcript into structured form.
pub async fn summarise_transcript(
    text: &str,
    agent_config: &AgentConfig,
    llm_config: &LlmConfig,
    proxy: &ProxyConfig,
) -> Result<MeetingSummary> {
    let user_prompt = format!("以下是会议转写，请按照系统指令输出 JSON：\n\n```\n{}\n```", text);
    let raw = run_oneshot(SUMMARY_SYSTEM, &user_prompt, agent_config, llm_config, proxy, 2048).await?;

    let json_str = extract_json_object(&raw)
        .ok_or_else(|| anyhow!("LLM did not return a JSON object"))?;
    let parsed: SummaryJson = serde_json::from_str(json_str)
        .map_err(|e| anyhow!("failed to parse summary JSON: {e}; raw: {json_str}"))?;

    Ok(MeetingSummary {
        title: parsed.title,
        key_points: parsed.key_points,
        todos: parsed.todos,
        decisions: parsed.decisions,
    })
}

/// Extract the first balanced `{ ... }` block, skipping markdown fences.
fn extract_json_object(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')?;
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_handles_fences() {
        let s = "noise ```json\n{\"title\":\"x\",\"key_points\":[]} ``` after";
        let j = extract_json_object(s).unwrap();
        assert!(j.starts_with('{') && j.ends_with('}'));
        let parsed: SummaryJson = serde_json::from_str(j).unwrap();
        assert_eq!(parsed.title, "x");
    }

    #[test]
    fn extract_json_skips_braces_in_strings() {
        let s = "{\"a\":\"}}}\",\"b\":1}";
        assert_eq!(extract_json_object(s).unwrap(), s);
    }
}
