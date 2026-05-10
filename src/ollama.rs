use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    Error, Result,
    auth::{ActiveCredential, AuthOption},
    prompt::SystemPrompt,
    provider::{Provider, ProviderResponse, ToolDefinition},
    session::{Message, MessageContent},
};

const DEFAULT_MODEL: &str = "qwen3:8b";
const FALLBACK_MODELS: &[&str] = &[DEFAULT_MODEL];
const OLLAMA_CHAT_URL: &str = "http://localhost:11434/api/chat";
const OLLAMA_TAGS_URL: &str = "http://localhost:11434/api/tags";
const AUTH_OPTIONS: &[AuthOption] = &[AuthOption::None];

pub(crate) struct Ollama;

#[derive(Serialize, Debug, PartialEq)]
struct ChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    tools: Vec<OllamaTool>,
    stream: bool,
    think: bool,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct OllamaMessage {
    role: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<OllamaToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct OllamaToolCall {
    #[serde(default = "function_kind", rename = "type")]
    kind: String,
    function: OllamaToolCallFunction,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct OllamaToolCallFunction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    index: Option<usize>,
    name: String,
    arguments: Value,
}

#[derive(Serialize, Debug, PartialEq)]
struct OllamaTool {
    #[serde(rename = "type")]
    kind: String,
    function: OllamaToolFunction,
}

#[derive(Serialize, Debug, PartialEq)]
struct OllamaToolFunction {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Deserialize, Debug)]
struct ChatResponse {
    message: OllamaMessage,
}

#[derive(Deserialize, Debug)]
struct TagsResponse {
    models: Vec<TagModel>,
}

#[derive(Deserialize, Debug)]
struct TagModel {
    name: String,
}

impl Provider for Ollama {
    fn name(&self) -> &'static str {
        "ollama"
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
        _credential: &ActiveCredential,
    ) -> Result<Vec<String>> {
        let response = client.get(OLLAMA_TAGS_URL).send().await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await?;
            return Err(Error::Api {
                provider: self.name().to_string(),
                status,
                body,
            });
        }

        let parsed: TagsResponse = response.json().await?;
        let mut models = parsed
            .models
            .into_iter()
            .map(|model| model.name)
            .collect::<Vec<_>>();
        models.sort();
        Ok(models)
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
        let req = ChatRequest {
            model: model.to_string(),
            messages: to_ollama_messages(prompt, messages),
            tools: tools.into_iter().map(OllamaTool::from).collect(),
            stream: false,
            think: true,
        };

        let response = credential
            .attach(client.post(OLLAMA_CHAT_URL))
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

        let parsed: ChatResponse = response.json().await?;
        let blocks = to_message_content(parsed.message);
        if blocks
            .iter()
            .any(|block| matches!(block, MessageContent::ToolUse { .. }))
        {
            return Ok(ProviderResponse::ToolUse(blocks));
        }

        let reply = render_text_blocks(&blocks);
        if reply.is_empty() {
            return Err(Error::EmptyContent(self.name().to_string()));
        }

        Ok(ProviderResponse::Text(reply))
    }
}

impl From<ToolDefinition> for OllamaTool {
    fn from(tool: ToolDefinition) -> Self {
        Self {
            kind: "function".to_string(),
            function: OllamaToolFunction {
                name: tool.name,
                description: tool.description,
                parameters: tool.input_schema,
            },
        }
    }
}

fn to_ollama_messages(prompt: &SystemPrompt, messages: &[Message]) -> Vec<OllamaMessage> {
    let mut tool_names_by_id = BTreeMap::new();
    let mut converted = vec![OllamaMessage {
        role: "system".to_string(),
        content: prompt.render_text(),
        tool_calls: Vec::new(),
        tool_name: None,
    }];

    for message in messages {
        match message.role.as_str() {
            "assistant" => {
                converted.push(assistant_message(message, &mut tool_names_by_id));
            }
            "user" => {
                converted.extend(user_messages(message, &tool_names_by_id));
            }
            _ => {}
        }
    }

    converted
}

fn user_messages(
    message: &Message,
    tool_names_by_id: &BTreeMap<String, String>,
) -> Vec<OllamaMessage> {
    let mut messages = Vec::new();
    let mut text_blocks = Vec::new();

    for block in &message.content {
        match block {
            MessageContent::Text { text } => text_blocks.push(text.as_str()),
            MessageContent::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let content = if *is_error {
                    format!("ERROR: {}", content)
                } else {
                    content.clone()
                };
                let tool_name = tool_names_by_id
                    .get(tool_use_id)
                    .cloned()
                    .unwrap_or_else(|| tool_use_id.clone());

                messages.push(OllamaMessage {
                    role: "tool".to_string(),
                    content,
                    tool_calls: Vec::new(),
                    tool_name: Some(tool_name),
                });
            }
            MessageContent::ToolUse { .. } => {}
        }
    }

    if !text_blocks.is_empty() {
        messages.insert(
            0,
            OllamaMessage {
                role: "user".to_string(),
                content: text_blocks.join("\n"),
                tool_calls: Vec::new(),
                tool_name: None,
            },
        );
    }

    messages
}

fn assistant_message(
    message: &Message,
    tool_names_by_id: &mut BTreeMap<String, String>,
) -> OllamaMessage {
    let mut text_blocks = Vec::new();
    let mut tool_calls = Vec::new();

    for block in &message.content {
        match block {
            MessageContent::Text { text } => text_blocks.push(text.as_str()),
            MessageContent::ToolUse { id, name, input } => {
                let index = tool_calls.len();
                tool_names_by_id.insert(id.clone(), name.clone());
                tool_calls.push(OllamaToolCall {
                    kind: "function".to_string(),
                    function: OllamaToolCallFunction {
                        index: Some(index),
                        name: name.clone(),
                        arguments: input.clone(),
                    },
                });
            }
            MessageContent::ToolResult { .. } => {}
        }
    }

    OllamaMessage {
        role: "assistant".to_string(),
        content: text_blocks.join("\n"),
        tool_calls,
        tool_name: None,
    }
}

fn to_message_content(message: OllamaMessage) -> Vec<MessageContent> {
    let mut blocks = Vec::new();

    if !message.content.is_empty() {
        blocks.push(MessageContent::Text {
            text: message.content,
        });
    }

    blocks.extend(
        message
            .tool_calls
            .into_iter()
            .enumerate()
            .map(|(fallback_index, tool_call)| {
                let index = tool_call.function.index.unwrap_or(fallback_index);

                MessageContent::ToolUse {
                    id: format!("ollama_tool_{index}"),
                    name: tool_call.function.name,
                    input: normalize_tool_arguments(tool_call.function.arguments),
                }
            }),
    );

    blocks
}

fn normalize_tool_arguments(arguments: Value) -> Value {
    match arguments {
        Value::String(arguments) => serde_json::from_str(&arguments)
            .unwrap_or_else(|_| json!({ "raw_arguments": arguments })),
        other => other,
    }
}

fn function_kind() -> String {
    "function".to_string()
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

    #[test]
    fn converts_tool_definition_to_ollama_function_tool() {
        let tool = OllamaTool::from(ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        });

        let serialized = serde_json::to_value(tool).unwrap();

        assert_eq!(
            serialized,
            json!({
                "type": "function",
                "function": {
                    "name": "read_file",
                    "description": "Read a file.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" }
                        },
                        "required": ["path"]
                    }
                }
            })
        );
    }

    #[test]
    fn converts_history_to_ollama_messages() {
        let history = vec![
            Message::user_text("read Cargo.toml"),
            Message::assistant(vec![MessageContent::ToolUse {
                id: "ollama_tool_0".to_string(),
                name: "read_file".to_string(),
                input: json!({ "path": "Cargo.toml" }),
            }]),
            Message::user_tool_result("ollama_tool_0".to_string(), "contents".to_string()),
        ];

        let messages = to_ollama_messages(&test_prompt(), &history);
        let serialized = serde_json::to_value(messages).unwrap();

        assert_eq!(
            serialized,
            json!([
                {
                    "role": "system",
                    "content": "<identity>\nYou are cawir.\n</identity>"
                },
                {
                    "role": "user",
                    "content": "read Cargo.toml"
                },
                {
                    "role": "assistant",
                    "tool_calls": [
                        {
                            "type": "function",
                            "function": {
                                "index": 0,
                                "name": "read_file",
                                "arguments": { "path": "Cargo.toml" }
                            }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": "contents",
                    "tool_name": "read_file"
                }
            ])
        );
    }

    fn test_prompt() -> SystemPrompt {
        SystemPrompt {
            sections: vec![PromptSection {
                name: "identity".to_string(),
                content: "You are cawir.".to_string(),
            }],
        }
    }

    #[test]
    fn parses_ollama_tool_call_as_internal_tool_use() {
        let body = r#"
        {
            "message": {
                "role": "assistant",
                "content": "I'll inspect Cargo.toml.",
                "tool_calls": [
                    {
                        "type": "function",
                        "function": {
                            "index": 0,
                            "name": "read_file",
                            "arguments": { "path": "Cargo.toml" }
                        }
                    }
                ]
            }
        }
        "#;

        let parsed: ChatResponse = serde_json::from_str(body).unwrap();
        let blocks = to_message_content(parsed.message);

        assert_eq!(
            blocks,
            vec![
                MessageContent::Text {
                    text: "I'll inspect Cargo.toml.".to_string()
                },
                MessageContent::ToolUse {
                    id: "ollama_tool_0".to_string(),
                    name: "read_file".to_string(),
                    input: json!({ "path": "Cargo.toml" })
                }
            ]
        );
    }

    #[test]
    fn parses_ollama_tool_call_with_string_arguments_and_missing_type() {
        let body = r#"
        {
            "message": {
                "role": "assistant",
                "tool_calls": [
                    {
                        "function": {
                            "name": "read_file",
                            "arguments": "{\"path\":\"Cargo.toml\"}"
                        }
                    }
                ]
            }
        }
        "#;

        let parsed: ChatResponse = serde_json::from_str(body).unwrap();
        let blocks = to_message_content(parsed.message);

        assert_eq!(
            blocks,
            vec![MessageContent::ToolUse {
                id: "ollama_tool_0".to_string(),
                name: "read_file".to_string(),
                input: json!({ "path": "Cargo.toml" })
            }]
        );
    }
}
