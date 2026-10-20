//! LLM summary for meeting transcripts.

use anyhow::{anyhow, Result};
use serde::Deserialize;

use crate::llm;
use crate::storage::{LlmConfig, MeetingSummary, ProxyConfig};

const SUMMARY_SYSTEM: &str = "你是一名会议纪要助理。基于提供的会议转写，输出一个 JSON 对象，字段固定为：\n{\n  \"title\": \"<不超过 30 字的中文标题>\",\n  \"key_points\": [\"要点1\", \"要点2\", ...],\n  \"todos\": [\"待办1\", ...],\n  \"decisions\": [\"决议1\", ...]\n}\n\n严格要求：\n- 只输出合法 JSON，不要使用 markdown 代码块包裹。\n- 转写来自语音识别，可能有同音错字，请按上下文理解。\n- 列表字段如果没有内容，输出空数组 []。\n- 内容必须来自原文，不要编造。\n- title 必须能概括整场会议主题。";

#[derive(Deserialize)]
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

pub async fn summarise_transcript(
    transcript: &str,
    config: &LlmConfig,
    proxy: &ProxyConfig,
) -> Result<MeetingSummary> {
    let user_prompt = format!("以下是会议转写，请按照系统指令输出 JSON：\n\n```\n{}\n```", transcript);
    let raw = llm::complete(config, proxy, SUMMARY_SYSTEM, &user_prompt, 2048).await?;

    let json_str =
        llm::extract_json_object(&raw).ok_or_else(|| anyhow!("LLM did not return a JSON object"))?;
    let parsed: SummaryJson = serde_json::from_str(json_str)
        .map_err(|e| anyhow!("failed to parse summary JSON: {e}; raw: {json_str}"))?;

    Ok(MeetingSummary {
        title: parsed.title,
        key_points: parsed.key_points,
        todos: parsed.todos,
        decisions: parsed.decisions,
    })
}
