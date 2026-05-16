use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    Error, Result,
    auth::{ActiveCredential, ApiKeyCredential, AuthOption, RequestAuth},
    prompt::SystemPrompt,
    provider::{
        Provider, ProviderEvent, ProviderMetadata, ProviderRequest, ProviderResponse, TokenUsage,
        ToolDefinition, api_error_from_response, read_sse_data_events,
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
    stream: bool,
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

#[cfg(test)]
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

#[derive(Default)]
struct AnthropicStreamAccumulator {
    blocks: BTreeMap<usize, AnthropicPendingBlock>,
    usage: TokenUsage,
}

enum AnthropicPendingBlock {
    Text(String),
    ToolUse {
        id: String,
        name: String,
        input_json: String,
    },
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicStreamEvent {
    MessageStart {
        message: AnthropicStreamMessage,
    },
    ContentBlockStart {
        index: usize,
        content_block: MessageContent,
    },
    ContentBlockDelta {
        index: usize,
        delta: AnthropicContentDelta,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        delta: AnthropicMessageDelta,
        usage: Option<AnthropicUsage>,
    },
    MessageStop,
    Ping,
    Error {
        error: Value,
    },
}

#[derive(Deserialize, Debug)]
struct AnthropicStreamMessage {
    usage: Option<AnthropicUsage>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContentDelta {
    TextDelta {
        text: String,
    },
    InputJsonDelta {
        partial_json: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize, Debug)]
struct AnthropicMessageDelta {
    stop_reason: Option<String>,
    stop_sequence: Option<String>,
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

    async fn send(&self, request: ProviderRequest<'_>) -> Result<ProviderResponse> {
        let req = MessageRequest {
            model: request.model.to_string(),
            max_tokens: MAX_OUTPUT_TOKENS,
            stream: true,
            cache_control: CacheControl {
                kind: "ephemeral".to_string(),
            },
            system: anthropic_system_blocks(request.prompt),
            messages: request.messages.to_vec(),
            tools: request.tools,
        };

        let response = request
            .credential
            .attach(request.client.post("https://api.anthropic.com/v1/messages"))
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&req)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(api_error_from_response(self.name(), response).await);
        }

        let mut accumulator = AnthropicStreamAccumulator::default();
        read_sse_data_events(response, |event| {
            accumulator.handle_data_event(event, request.events)
        })
        .await?;
        accumulator.finish()
    }
}

#[cfg(test)]
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

impl AnthropicStreamAccumulator {
    fn handle_data_event(
        &mut self,
        event: String,
        events: &mut dyn FnMut(ProviderEvent),
    ) -> Result<()> {
        let event: AnthropicStreamEvent = serde_json::from_str(&event).map_err(|error| {
            Error::Env(format!("failed to parse Anthropic stream event: {error}"))
        })?;

        match event {
            AnthropicStreamEvent::MessageStart { message } => {
                if let Some(usage) = message.usage {
                    self.merge_usage(usage);
                }
            }
            AnthropicStreamEvent::ContentBlockStart {
                index,
                content_block,
            } => match content_block {
                MessageContent::Text { text } => {
                    if !text.is_empty() {
                        events(ProviderEvent::TextDelta { text: text.clone() });
                    }
                    self.blocks.insert(index, AnthropicPendingBlock::Text(text));
                }
                MessageContent::ToolUse { id, name, input } => {
                    events(ProviderEvent::ToolUseStart {
                        id: id.clone(),
                        name: name.clone(),
                    });
                    let input_json = if input == json!({}) {
                        String::new()
                    } else {
                        input.to_string()
                    };
                    self.blocks.insert(
                        index,
                        AnthropicPendingBlock::ToolUse {
                            id,
                            name,
                            input_json,
                        },
                    );
                }
                MessageContent::ToolResult { .. } => {}
            },
            AnthropicStreamEvent::ContentBlockDelta { index, delta } => {
                match (self.blocks.get_mut(&index), delta) {
                    (
                        Some(AnthropicPendingBlock::Text(text)),
                        AnthropicContentDelta::TextDelta { text: delta },
                    ) => {
                        events(ProviderEvent::TextDelta {
                            text: delta.clone(),
                        });
                        text.push_str(&delta);
                    }
                    (
                        Some(AnthropicPendingBlock::ToolUse { id, input_json, .. }),
                        AnthropicContentDelta::InputJsonDelta { partial_json },
                    ) => {
                        events(ProviderEvent::ToolUseInputDelta {
                            id: id.clone(),
                            partial_json: partial_json.clone(),
                        });
                        input_json.push_str(&partial_json);
                    }
                    _ => {}
                }
            }
            AnthropicStreamEvent::MessageDelta { delta, usage } => {
                let _ = (delta.stop_reason, delta.stop_sequence);
                if let Some(usage) = usage {
                    self.merge_usage(usage);
                }
            }
            AnthropicStreamEvent::ContentBlockStop { index } => {
                let _ = index;
            }
            AnthropicStreamEvent::MessageStop | AnthropicStreamEvent::Ping => {}
            AnthropicStreamEvent::Error { error } => {
                return Err(Error::Env(format!("Anthropic stream failed: {error}")));
            }
        }

        Ok(())
    }

    fn finish(self) -> Result<ProviderResponse> {
        let blocks = self
            .blocks
            .into_values()
            .filter_map(AnthropicPendingBlock::into_message_content)
            .collect::<Vec<_>>();
        let metadata = ProviderMetadata::from_usage(self.usage);

        if blocks
            .iter()
            .any(|block| matches!(block, MessageContent::ToolUse { .. }))
        {
            return Ok(ProviderResponse::tool_use(blocks, metadata));
        }

        let reply = render_text_blocks(&blocks);
        if reply.is_empty() {
            return Err(Error::EmptyContent("anthropic".to_string()));
        }

        Ok(ProviderResponse::text(reply, metadata))
    }

    fn merge_usage(&mut self, usage: AnthropicUsage) {
        let usage = usage.token_usage();

        if usage.input_tokens.is_some() {
            self.usage.input_tokens = usage.input_tokens;
        }
        if usage.output_tokens.is_some() {
            self.usage.output_tokens = usage.output_tokens;
        }
        if usage.cache_creation_input_tokens.is_some() {
            self.usage.cache_creation_input_tokens = usage.cache_creation_input_tokens;
        }
        if usage.cache_read_input_tokens.is_some() {
            self.usage.cache_read_input_tokens = usage.cache_read_input_tokens;
        }
    }
}

impl AnthropicPendingBlock {
    fn into_message_content(self) -> Option<MessageContent> {
        match self {
            Self::Text(text) => (!text.is_empty()).then_some(MessageContent::Text { text }),
            Self::ToolUse {
                id,
                name,
                input_json,
            } => {
                let input = if input_json.trim().is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(&input_json)
                        .unwrap_or_else(|_| json!({ "raw_arguments": input_json }))
                };

                Some(MessageContent::ToolUse { id, name, input })
            }
        }
    }
}

#[cfg(test)]
fn stream_events_to_response(
    events: impl IntoIterator<Item = String>,
    emit: &mut dyn FnMut(ProviderEvent),
) -> Result<ProviderResponse> {
    let mut accumulator = AnthropicStreamAccumulator::default();
    for event in events {
        accumulator.handle_data_event(event, emit)?;
    }
    accumulator.finish()
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
    use crate::provider::{ProviderEvent, ProviderMetadata, TokenUsage};
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
            stream: true,
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
            stream: true,
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
                "stream": true,
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

    #[test]
    fn parses_stream_text_deltas_and_final_usage() {
        let body = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_123\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-haiku-4-5-20251001\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":11,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hel\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":2}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let mut events = Vec::new();

        let response =
            stream_events_to_response(crate::provider::parse_sse_data_events(body), &mut |event| {
                events.push(event);
            })
            .unwrap();

        assert_eq!(
            events,
            vec![
                ProviderEvent::TextDelta {
                    text: "hel".to_string()
                },
                ProviderEvent::TextDelta {
                    text: "lo".to_string()
                }
            ]
        );
        assert_eq!(
            response,
            ProviderResponse::text(
                "hello".to_string(),
                ProviderMetadata {
                    usage: Some(TokenUsage {
                        input_tokens: Some(11),
                        output_tokens: Some(2),
                        cache_creation_input_tokens: None,
                        cache_read_input_tokens: None,
                    })
                }
            )
        );
    }

    #[test]
    fn parses_streamed_tool_use_input_json_deltas() {
        let body = concat!(
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_123\",\"name\":\"read_file\",\"input\":{}}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"Cargo.toml\\\"}\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        );
        let mut events = Vec::new();

        let response =
            stream_events_to_response(crate::provider::parse_sse_data_events(body), &mut |event| {
                events.push(event);
            })
            .unwrap();

        assert_eq!(
            events,
            vec![
                ProviderEvent::ToolUseStart {
                    id: "toolu_123".to_string(),
                    name: "read_file".to_string(),
                },
                ProviderEvent::ToolUseInputDelta {
                    id: "toolu_123".to_string(),
                    partial_json: "{\"path\":".to_string(),
                },
                ProviderEvent::ToolUseInputDelta {
                    id: "toolu_123".to_string(),
                    partial_json: "\"Cargo.toml\"}".to_string(),
                },
            ]
        );
        assert_eq!(
            response,
            ProviderResponse::tool_use(
                vec![MessageContent::ToolUse {
                    id: "toolu_123".to_string(),
                    name: "read_file".to_string(),
                    input: json!({ "path": "Cargo.toml" })
                }],
                ProviderMetadata::empty()
            )
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
