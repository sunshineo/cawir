use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    Error, Result,
    provider::{Provider, ProviderResponse, ToolDefinition},
    session::{Message, MessageContent},
};

const MODEL: &str = "gpt-4.1";

pub(crate) struct OpenAi;

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    tools: Vec<OpenAiTool>,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(untagged)]
enum OpenAiMessage {
    Chat {
        role: String,
        content: String,
    },
    AssistantToolUse {
        role: String,
        content: Option<String>,
        tool_calls: Vec<OpenAiToolCall>,
    },
    ToolResult {
        role: String,
        tool_call_id: String,
        content: String,
    },
}

#[derive(Serialize, Debug, PartialEq, Eq)]
struct OpenAiToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    function: OpenAiToolCallFunction,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
struct OpenAiToolCallFunction {
    name: String,
    arguments: String,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
struct OpenAiTool {
    #[serde(rename = "type")]
    kind: String,
    function: OpenAiToolFunction,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
struct OpenAiToolFunction {
    name: String,
    description: String,
    parameters: Value,
    strict: bool,
}

#[derive(Deserialize, Debug)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize, Debug)]
struct Choice {
    message: AssistantMessage,
}

#[derive(Deserialize, Debug)]
struct AssistantMessage {
    content: Option<String>,
    tool_calls: Option<Vec<ResponseToolCall>>,
}

#[derive(Deserialize, Debug)]
struct ResponseToolCall {
    id: String,
    function: ResponseToolCallFunction,
}

#[derive(Deserialize, Debug)]
struct ResponseToolCallFunction {
    name: String,
    arguments: String,
}

impl Provider for OpenAi {
    fn name(&self) -> &'static str {
        "openai"
    }

    fn api_key_env_var(&self) -> &'static str {
        "OPENAI_API_KEY"
    }

    async fn send(
        &self,
        client: &reqwest::Client,
        api_key: &str,
        messages: &[Message],
        tools: Vec<ToolDefinition>,
    ) -> Result<ProviderResponse> {
        let req = ChatRequest {
            model: MODEL.to_string(),
            messages: to_openai_messages(messages),
            tools: tools.into_iter().map(OpenAiTool::from).collect(),
        };

        let response = client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(api_key)
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
        let Some(choice) = parsed.choices.into_iter().next() else {
            return Err(Error::EmptyContent(self.name().to_string()));
        };

        let blocks = to_message_content(choice.message);
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

impl From<ToolDefinition> for OpenAiTool {
    fn from(tool: ToolDefinition) -> Self {
        Self {
            kind: "function".to_string(),
            function: OpenAiToolFunction {
                name: tool.name,
                description: tool.description,
                parameters: tool.input_schema,
                strict: true,
            },
        }
    }
}

fn to_openai_messages(messages: &[Message]) -> Vec<OpenAiMessage> {
    messages
        .iter()
        .flat_map(|message| match message.role.as_str() {
            "assistant" => assistant_message(message),
            "user" => user_messages(message),
            _ => Vec::new(),
        })
        .collect()
}

fn user_messages(message: &Message) -> Vec<OpenAiMessage> {
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

                messages.push(OpenAiMessage::ToolResult {
                    role: "tool".to_string(),
                    tool_call_id: tool_use_id.clone(),
                    content,
                });
            }
            MessageContent::ToolUse { .. } => {}
        }
    }

    if !text_blocks.is_empty() {
        messages.insert(
            0,
            OpenAiMessage::Chat {
                role: "user".to_string(),
                content: text_blocks.join("\n"),
            },
        );
    }

    messages
}

fn assistant_message(message: &Message) -> Vec<OpenAiMessage> {
    let mut text_blocks = Vec::new();
    let mut tool_calls = Vec::new();

    for block in &message.content {
        match block {
            MessageContent::Text { text } => text_blocks.push(text.as_str()),
            MessageContent::ToolUse { id, name, input } => {
                tool_calls.push(OpenAiToolCall {
                    id: id.clone(),
                    kind: "function".to_string(),
                    function: OpenAiToolCallFunction {
                        name: name.clone(),
                        arguments: input.to_string(),
                    },
                });
            }
            MessageContent::ToolResult { .. } => {}
        }
    }

    if tool_calls.is_empty() {
        vec![OpenAiMessage::Chat {
            role: "assistant".to_string(),
            content: text_blocks.join("\n"),
        }]
    } else {
        let content = if text_blocks.is_empty() {
            None
        } else {
            Some(text_blocks.join("\n"))
        };

        vec![OpenAiMessage::AssistantToolUse {
            role: "assistant".to_string(),
            content,
            tool_calls,
        }]
    }
}

fn to_message_content(message: AssistantMessage) -> Vec<MessageContent> {
    let mut blocks = Vec::new();

    if let Some(content) = message.content
        && !content.is_empty()
    {
        blocks.push(MessageContent::Text { text: content });
    }

    if let Some(tool_calls) = message.tool_calls {
        blocks.extend(tool_calls.into_iter().map(|tool_call| {
            let input = serde_json::from_str(&tool_call.function.arguments)
                .unwrap_or_else(|_| json!({ "raw_arguments": tool_call.function.arguments }));

            MessageContent::ToolUse {
                id: tool_call.id,
                name: tool_call.function.name,
                input,
            }
        }));
    }

    blocks
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

    #[test]
    fn converts_tool_definition_to_openai_function_tool() {
        let tool = OpenAiTool::from(ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"],
                "additionalProperties": false
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
                        "required": ["path"],
                        "additionalProperties": false
                    },
                    "strict": true
                }
            })
        );
    }

    #[test]
    fn converts_history_to_openai_chat_messages() {
        let history = vec![
            Message::user_text("read Cargo.toml"),
            Message::assistant(vec![MessageContent::ToolUse {
                id: "call_123".to_string(),
                name: "read_file".to_string(),
                input: json!({ "path": "Cargo.toml" }),
            }]),
            Message::user_tool_result("call_123".to_string(), "contents".to_string()),
        ];

        let messages = to_openai_messages(&history);
        let serialized = serde_json::to_value(messages).unwrap();

        assert_eq!(
            serialized,
            json!([
                {
                    "role": "user",
                    "content": "read Cargo.toml"
                },
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_123",
                            "type": "function",
                            "function": {
                                "name": "read_file",
                                "arguments": "{\"path\":\"Cargo.toml\"}"
                            }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_123",
                    "content": "contents"
                }
            ])
        );
    }

    #[test]
    fn parses_openai_tool_call_as_internal_tool_use() {
        let body = r#"
        {
            "choices": [
                {
                    "message": {
                        "content": "I'll inspect Cargo.toml.",
                        "tool_calls": [
                            {
                                "id": "call_123",
                                "type": "function",
                                "function": {
                                    "name": "read_file",
                                    "arguments": "{\"path\":\"Cargo.toml\"}"
                                }
                            }
                        ]
                    }
                }
            ]
        }
        "#;

        let parsed: ChatResponse = serde_json::from_str(body).unwrap();
        let blocks = to_message_content(parsed.choices.into_iter().next().unwrap().message);

        assert_eq!(
            blocks,
            vec![
                MessageContent::Text {
                    text: "I'll inspect Cargo.toml.".to_string()
                },
                MessageContent::ToolUse {
                    id: "call_123".to_string(),
                    name: "read_file".to_string(),
                    input: json!({ "path": "Cargo.toml" })
                }
            ]
        );
    }
}
