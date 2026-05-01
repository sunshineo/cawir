use serde::{Deserialize, Serialize};

use crate::{
    Error, Result,
    session::{Message, MessageContent},
};

const MAX_OUTPUT_TOKENS: u32 = 16_384;

#[derive(Serialize, Clone)]
pub(crate) struct ToolDefinition {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_schema: serde_json::Value,
}

#[derive(Serialize)]
struct MessageRequest {
    model: String,
    max_tokens: u32,
    cache_control: CacheControl,
    messages: Vec<Message>,
    tools: Vec<ToolDefinition>,
}

#[derive(Serialize)]
struct CacheControl {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize, Debug)]
struct MessageResponse {
    content: Vec<MessageContent>,
}

pub(crate) enum ClaudeResponse {
    Text(String),
    ToolUse(Vec<MessageContent>),
}

pub(crate) async fn ask_claude(
    client: &reqwest::Client,
    api_key: &str,
    messages: &[Message],
    tools: Vec<ToolDefinition>,
) -> Result<ClaudeResponse> {
    let req = MessageRequest {
        model: "claude-haiku-4-5-20251001".to_string(),
        max_tokens: MAX_OUTPUT_TOKENS,
        cache_control: CacheControl {
            kind: "ephemeral".to_string(),
        },
        messages: messages.to_vec(),
        tools,
    };

    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&req)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await?;
        return Err(Error::Api { status, body });
    }

    let parsed: MessageResponse = response.json().await?;
    if parsed
        .content
        .iter()
        .any(|block| matches!(block, MessageContent::ToolUse { .. }))
    {
        return Ok(ClaudeResponse::ToolUse(parsed.content));
    }

    let reply = render_text_blocks(&parsed.content);
    if reply.is_empty() {
        return Err(Error::EmptyContent);
    }

    Ok(ClaudeResponse::Text(reply))
}

fn render_text_blocks(blocks: &[MessageContent]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            MessageContent::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn parses_tool_use_blocks_from_anthropic_response() {
        let body = r#"
        {
            "content": [
                {
                    "type": "text",
                    "text": "I'll inspect Cargo.toml."
                },
                {
                    "type": "tool_use",
                    "id": "toolu_123",
                    "name": "read_file",
                    "input": { "path": "Cargo.toml" }
                }
            ]
        }
        "#;

        let parsed: MessageResponse = serde_json::from_str(body).unwrap();

        assert_eq!(
            render_text_blocks(&parsed.content),
            "I'll inspect Cargo.toml."
        );

        match &parsed.content[1] {
            MessageContent::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_123");
                assert_eq!(name, "read_file");
                assert_eq!(
                    input.get("path").and_then(Value::as_str),
                    Some("Cargo.toml")
                );
            }
            other => panic!("expected tool_use block, got {:?}", other),
        }
    }

    #[test]
    fn formats_mixed_blocks_in_original_order() {
        let blocks = vec![
            MessageContent::Text {
                text: "First text.".to_string(),
            },
            MessageContent::ToolUse {
                id: "toolu_123".to_string(),
                name: "read_file".to_string(),
                input: json!({ "path": "Cargo.toml" }),
            },
            MessageContent::Text {
                text: "Second text.".to_string(),
            },
        ];

        assert_eq!(render_text_blocks(&blocks), "First text.\nSecond text.");

        let ordered_kinds = blocks
            .iter()
            .map(|block| match block {
                MessageContent::Text { .. } => "text",
                MessageContent::ToolUse { .. } => "tool_use",
                MessageContent::ToolResult { .. } => "tool_result",
            })
            .collect::<Vec<_>>();

        assert_eq!(ordered_kinds, vec!["text", "tool_use", "text"]);
    }

    #[test]
    fn message_request_enables_automatic_prompt_caching() {
        let request = MessageRequest {
            model: "claude-haiku-4-5-20251001".to_string(),
            max_tokens: MAX_OUTPUT_TOKENS,
            cache_control: CacheControl {
                kind: "ephemeral".to_string(),
            },
            messages: vec![Message::user_text("hello")],
            tools: Vec::new(),
        };

        let serialized = serde_json::to_value(request).unwrap();

        assert_eq!(
            serialized.get("cache_control"),
            Some(&json!({ "type": "ephemeral" }))
        );
        assert_eq!(serialized.get("max_tokens"), Some(&json!(16_384)));
    }
}
