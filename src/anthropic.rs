use serde::{Deserialize, Serialize};

use crate::{
    Error, Result,
    auth::{ActiveCredential, ApiKeyCredential, AuthOption, RequestAuth},
    prompt::SystemPrompt,
    provider::{
        Provider, ProviderMetadata, ProviderResponse, TokenUsage, ToolDefinition,
        api_error_from_response,
    },
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
    system: Vec<AnthropicSystemBlock>,
    messages: Vec<Message>,
    tools: Vec<ToolDefinition>,
}

#[derive(Serialize)]
struct CacheControl {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Serialize)]
struct AnthropicSystemBlock {
    #[serde(rename = "type")]
    kind: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

#[derive(Deserialize, Debug)]
struct MessageResponse {
    content: Vec<MessageContent>,
    usage: Option<AnthropicUsage>,
}

#[derive(Deserialize, Debug)]
struct AnthropicUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
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

        if !response.status().is_success() {
            return Err(api_error_from_response(self.name(), response).await);
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
        prompt: &SystemPrompt,
        messages: &[Message],
        tools: Vec<ToolDefinition>,
    ) -> Result<ProviderResponse> {
        let req = MessageRequest {
            model: model.to_string(),
            max_tokens: MAX_OUTPUT_TOKENS,
            cache_control: CacheControl {
                kind: "ephemeral".to_string(),
            },
            system: anthropic_system_blocks(prompt),
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

        if !response.status().is_success() {
            return Err(api_error_from_response(self.name(), response).await);
        }

        let parsed: MessageResponse = response.json().await?;
        let metadata = parsed.metadata();
        if parsed
            .content
            .iter()
            .any(|block| matches!(block, MessageContent::ToolUse { .. }))
        {
            return Ok(ProviderResponse::tool_use(parsed.content, metadata));
        }

        let reply = render_text_blocks(&parsed.content);
        if reply.is_empty() {
            return Err(Error::EmptyContent(self.name().to_string()));
        }

        Ok(ProviderResponse::text(reply, metadata))
    }
}

impl MessageResponse {
    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata::from_usage(
            self.usage
                .as_ref()
                .map(AnthropicUsage::token_usage)
                .unwrap_or_default(),
        )
    }
}

impl AnthropicUsage {
    fn token_usage(&self) -> TokenUsage {
        TokenUsage {
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_creation_input_tokens: self.cache_creation_input_tokens,
            cache_read_input_tokens: self.cache_read_input_tokens,
        }
    }
}

fn anthropic_system_blocks(prompt: &SystemPrompt) -> Vec<AnthropicSystemBlock> {
    vec![AnthropicSystemBlock {
        kind: "text".to_string(),
        text: prompt.render_text(),
        cache_control: Some(CacheControl {
            kind: "ephemeral".to_string(),
        }),
    }]
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
    use crate::prompt::{PromptSection, SystemPrompt};
    use crate::provider::{ProviderMetadata, TokenUsage};
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
    fn parses_usage_and_cache_counts_from_anthropic_response() {
        let body = r#"
        {
            "content": [
                {
                    "type": "text",
                    "text": "done"
                }
            ],
            "usage": {
                "input_tokens": 123,
                "output_tokens": 45,
                "cache_creation_input_tokens": 67,
                "cache_read_input_tokens": 89
            }
        }
        "#;

        let parsed: MessageResponse = serde_json::from_str(body).unwrap();

        assert_eq!(
            parsed.metadata(),
            ProviderMetadata {
                usage: Some(TokenUsage {
                    input_tokens: Some(123),
                    output_tokens: Some(45),
                    cache_creation_input_tokens: Some(67),
                    cache_read_input_tokens: Some(89),
                })
            }
        );
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
            system: anthropic_system_blocks(&test_prompt()),
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

    #[test]
    fn message_request_combines_system_and_automatic_cache_breakpoints() {
        let request = MessageRequest {
            model: "claude-haiku-4-5-20251001".to_string(),
            max_tokens: MAX_OUTPUT_TOKENS,
            cache_control: CacheControl {
                kind: "ephemeral".to_string(),
            },
            system: anthropic_system_blocks(&test_prompt()),
            messages: vec![Message::user_text("hello")],
            tools: vec![ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file.".to_string(),
                input_schema: json!({ "type": "object" }),
            }],
        };

        let serialized = serde_json::to_value(request).unwrap();

        assert_eq!(
            serialized,
            json!({
                "model": "claude-haiku-4-5-20251001",
                "max_tokens": 16_384,
                "cache_control": { "type": "ephemeral" },
                "system": [
                {
                    "type": "text",
                        "text": "<identity>\nYou are cawir.\n</identity>\n\n<behavior>\nUse tools when useful.\n</behavior>",
                        "cache_control": { "type": "ephemeral" }
                }
                ],
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": "hello"
                            }
                        ]
                    }
                ],
                "tools": [
                    {
                        "name": "read_file",
                        "description": "Read a file.",
                        "input_schema": { "type": "object" }
                    }
                ]
            })
        );
    }

    fn test_prompt() -> SystemPrompt {
        SystemPrompt {
            sections: vec![
                PromptSection {
                    name: "identity".to_string(),
                    content: "You are cawir.".to_string(),
                },
                PromptSection {
                    name: "behavior".to_string(),
                    content: "Use tools when useful.".to_string(),
                },
            ],
        }
    }
}
