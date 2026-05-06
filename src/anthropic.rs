use serde::{Deserialize, Serialize};

use crate::{
    Error, Result,
    auth::{ActiveCredential, ApiKeyCredential, AuthOption, RequestAuth},
    provider::{Provider, ProviderResponse, ToolDefinition},
    session::{Message, MessageContent},
};

const MAX_OUTPUT_TOKENS: u32 = 16_384;
const DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";
const FALLBACK_MODELS: &[&str] = &[DEFAULT_MODEL];
const MODELS_URL: &str = "https://api.anthropic.com/v1/models";
const AUTH_OPTIONS: &[AuthOption] = &[AuthOption::ApiKey(ApiKeyCredential {
    env_var: "ANTHROPIC_API_KEY",
    storage_key: "anthropic-api-key",
    attachment: RequestAuth::Header("x-api-key"),
})];

pub(crate) struct Anthropic;

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

#[derive(Deserialize, Debug)]
struct ModelsResponse {
    data: Vec<ModelInfo>,
}

#[derive(Deserialize, Debug)]
struct ModelInfo {
    id: String,
}

impl Provider for Anthropic {
    fn name(&self) -> &'static str {
        "anthropic"
    }

    fn auth_options(&self) -> &'static [AuthOption] {
        AUTH_OPTIONS
    }

    fn default_model(&self, _credential: &ActiveCredential) -> &'static str {
        DEFAULT_MODEL
    }

    async fn available_models(
        &self,
        client: &reqwest::Client,
        credential: &ActiveCredential,
    ) -> Result<Vec<String>> {
        let response = credential
            .attach(client.get(MODELS_URL))
            .header("anthropic-version", "2023-06-01")
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await?;
            return Err(Error::Api {
                provider: self.name().to_string(),
                status,
                body,
            });
        }

        let parsed: ModelsResponse = response.json().await?;
        Ok(parsed.data.into_iter().map(|model| model.id).collect())
    }

    fn fallback_models(&self, _credential: &ActiveCredential) -> &'static [&'static str] {
        FALLBACK_MODELS
    }

    async fn send(
        &self,
        client: &reqwest::Client,
        credential: &ActiveCredential,
        model: &str,
        messages: &[Message],
        tools: Vec<ToolDefinition>,
    ) -> Result<ProviderResponse> {
        let req = MessageRequest {
            model: model.to_string(),
            max_tokens: MAX_OUTPUT_TOKENS,
            cache_control: CacheControl {
                kind: "ephemeral".to_string(),
            },
            messages: messages.to_vec(),
            tools,
        };

        let response = credential
            .attach(client.post("https://api.anthropic.com/v1/messages"))
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&req)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await?;
            return Err(Error::Api {
                provider: self.name().to_string(),
                status,
                body,
            });
        }

        let parsed: MessageResponse = response.json().await?;
        if parsed
            .content
            .iter()
            .any(|block| matches!(block, MessageContent::ToolUse { .. }))
        {
            return Ok(ProviderResponse::ToolUse(parsed.content));
        }

        let reply = render_text_blocks(&parsed.content);
        if reply.is_empty() {
            return Err(Error::EmptyContent(self.name().to_string()));
        }

        Ok(ProviderResponse::Text(reply))
    }
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
