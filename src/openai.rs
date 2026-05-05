use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    Error, Result,
    auth::{ActiveCredential, ApiKeyCredential, AuthOption, CodexOAuthCredential, RequestAuth},
    provider::{Provider, ProviderResponse, ToolDefinition},
    session::{Message, MessageContent},
};

const MODEL: &str = "gpt-4.1";
const CODEX_MODEL: &str = "gpt-5.4";
const OPENAI_CHAT_COMPLETIONS_URL: &str = "https://api.openai.com/v1/chat/completions";
const CHATGPT_CODEX_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const CODEX_INSTRUCTIONS: &str =
    "You are cawir, a minimal coding agent. Answer plainly and use available tools when needed.";
const AUTH_OPTIONS: &[AuthOption] = &[
    AuthOption::ApiKey(ApiKeyCredential {
        env_var: "OPENAI_API_KEY",
        storage_key: "openai-api-key",
        attachment: RequestAuth::Bearer,
    }),
    AuthOption::CodexOAuth(CodexOAuthCredential {
        env_var: "OPENAI_CODEX_OAUTH_TOKEN",
        storage_key: "openai-codex-oauth",
    }),
];

pub(crate) struct OpenAi;

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    tools: Vec<OpenAiTool>,
}

#[derive(Serialize)]
struct ResponsesRequest {
    model: String,
    instructions: String,
    input: Vec<ResponsesItem>,
    tools: Vec<ResponsesTool>,
    tool_choice: String,
    parallel_tool_calls: bool,
    store: bool,
    stream: bool,
    include: Vec<String>,
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

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponsesItem {
    Message {
        role: String,
        content: Vec<ResponsesContent>,
    },
    FunctionCall {
        name: String,
        arguments: String,
        call_id: String,
    },
    FunctionCallOutput {
        call_id: String,
        output: String,
    },
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponsesContent {
    InputText { text: String },
    OutputText { text: String },
}

#[derive(Serialize, Debug, PartialEq)]
struct ResponsesTool {
    #[serde(rename = "type")]
    kind: String,
    name: String,
    description: String,
    strict: bool,
    parameters: Value,
}

#[derive(Deserialize, Debug)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Default)]
struct ResponsesStreamAccumulator {
    output: Vec<ResponsesItem>,
    text_delta: String,
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

    fn auth_options(&self) -> &'static [AuthOption] {
        AUTH_OPTIONS
    }

    async fn send(
        &self,
        client: &reqwest::Client,
        credential: &ActiveCredential,
        messages: &[Message],
        tools: Vec<ToolDefinition>,
    ) -> Result<ProviderResponse> {
        if credential.option_name() == "codex-oauth" {
            return self
                .send_codex_oauth(client, credential, messages, tools)
                .await;
        }

        let req = ChatRequest {
            model: MODEL.to_string(),
            messages: to_openai_messages(messages),
            tools: tools.into_iter().map(OpenAiTool::from).collect(),
        };

        let response = credential
            .attach(client.post(OPENAI_CHAT_COMPLETIONS_URL))
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

impl OpenAi {
    async fn send_codex_oauth(
        &self,
        client: &reqwest::Client,
        credential: &ActiveCredential,
        messages: &[Message],
        tools: Vec<ToolDefinition>,
    ) -> Result<ProviderResponse> {
        let req = ResponsesRequest {
            model: CODEX_MODEL.to_string(),
            instructions: CODEX_INSTRUCTIONS.to_string(),
            input: to_responses_items(messages),
            tools: tools.into_iter().map(ResponsesTool::from).collect(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            store: false,
            stream: true,
            include: Vec::new(),
        };

        let response = credential
            .attach(client.post(CHATGPT_CODEX_RESPONSES_URL))
            .header("accept", "text/event-stream")
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

        let body = response.text().await?;
        let blocks = responses_stream_to_message_content(&body)?;
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

impl From<ToolDefinition> for ResponsesTool {
    fn from(tool: ToolDefinition) -> Self {
        Self {
            kind: "function".to_string(),
            name: tool.name,
            description: tool.description,
            strict: false,
            parameters: tool.input_schema,
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

fn to_responses_items(messages: &[Message]) -> Vec<ResponsesItem> {
    messages
        .iter()
        .flat_map(|message| match message.role.as_str() {
            "assistant" => assistant_responses_items(message),
            "user" => user_responses_items(message),
            _ => Vec::new(),
        })
        .collect()
}

fn user_responses_items(message: &Message) -> Vec<ResponsesItem> {
    let mut items = Vec::new();
    let mut text_blocks = Vec::new();

    for block in &message.content {
        match block {
            MessageContent::Text { text } => text_blocks.push(text.as_str()),
            MessageContent::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let output = if *is_error {
                    format!("ERROR: {}", content)
                } else {
                    content.clone()
                };

                items.push(ResponsesItem::FunctionCallOutput {
                    call_id: tool_use_id.clone(),
                    output,
                });
            }
            MessageContent::ToolUse { .. } => {}
        }
    }

    if !text_blocks.is_empty() {
        items.insert(
            0,
            ResponsesItem::Message {
                role: "user".to_string(),
                content: vec![ResponsesContent::InputText {
                    text: text_blocks.join("\n"),
                }],
            },
        );
    }

    items
}

fn assistant_responses_items(message: &Message) -> Vec<ResponsesItem> {
    let mut items = Vec::new();
    let mut text_blocks = Vec::new();

    for block in &message.content {
        match block {
            MessageContent::Text { text } => text_blocks.push(text.as_str()),
            MessageContent::ToolUse { id, name, input } => {
                items.push(ResponsesItem::FunctionCall {
                    name: name.clone(),
                    arguments: input.to_string(),
                    call_id: id.clone(),
                });
            }
            MessageContent::ToolResult { .. } => {}
        }
    }

    if !text_blocks.is_empty() {
        items.insert(
            0,
            ResponsesItem::Message {
                role: "assistant".to_string(),
                content: vec![ResponsesContent::OutputText {
                    text: text_blocks.join("\n"),
                }],
            },
        );
    }

    items
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

fn responses_output_to_message_content(output: Vec<ResponsesItem>) -> Vec<MessageContent> {
    output
        .into_iter()
        .flat_map(|item| match item {
            ResponsesItem::Message { content, .. } => content
                .into_iter()
                .filter_map(|content| match content {
                    ResponsesContent::OutputText { text }
                    | ResponsesContent::InputText { text } => {
                        (!text.is_empty()).then_some(MessageContent::Text { text })
                    }
                })
                .collect(),
            ResponsesItem::FunctionCall {
                name,
                arguments,
                call_id,
            } => {
                let input = serde_json::from_str(&arguments)
                    .unwrap_or_else(|_| json!({ "raw_arguments": arguments }));

                vec![MessageContent::ToolUse {
                    id: call_id,
                    name,
                    input,
                }]
            }
            ResponsesItem::FunctionCallOutput { .. } => Vec::new(),
        })
        .collect()
}

fn responses_stream_to_message_content(body: &str) -> Result<Vec<MessageContent>> {
    let mut accumulator = ResponsesStreamAccumulator::default();

    for event in parse_sse_data_events(body) {
        if event == "[DONE]" {
            continue;
        }

        let value: Value = serde_json::from_str(&event).map_err(|error| {
            Error::Env(format!("failed to parse Responses stream event: {error}"))
        })?;
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();

        match event_type {
            "response.output_text.delta" => {
                if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                    accumulator.text_delta.push_str(delta);
                }
            }
            "response.output_item.done" => {
                if let Some(item) = value.get("item")
                    && let Ok(item) = serde_json::from_value::<ResponsesItem>(item.clone())
                {
                    accumulator.output.push(item);
                }
            }
            "response.failed" | "response.incomplete" => {
                return Err(Error::Env(format!("Responses stream failed: {value}")));
            }
            _ => {}
        }
    }

    let blocks = responses_output_to_message_content(accumulator.output);
    if blocks.is_empty() && !accumulator.text_delta.is_empty() {
        Ok(vec![MessageContent::Text {
            text: accumulator.text_delta,
        }])
    } else {
        Ok(blocks)
    }
}

fn parse_sse_data_events(body: &str) -> Vec<String> {
    body.split("\n\n")
        .filter_map(|event| {
            let data = event
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n");

            (!data.is_empty()).then_some(data)
        })
        .collect()
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

    #[test]
    fn converts_history_to_responses_items() {
        let history = vec![
            Message::user_text("read Cargo.toml"),
            Message::assistant(vec![MessageContent::ToolUse {
                id: "call_123".to_string(),
                name: "read_file".to_string(),
                input: json!({ "path": "Cargo.toml" }),
            }]),
            Message::user_tool_result("call_123".to_string(), "contents".to_string()),
        ];

        let items = to_responses_items(&history);
        let serialized = serde_json::to_value(items).unwrap();

        assert_eq!(
            serialized,
            json!([
                {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": "read Cargo.toml"
                        }
                    ]
                },
                {
                    "type": "function_call",
                    "name": "read_file",
                    "arguments": "{\"path\":\"Cargo.toml\"}",
                    "call_id": "call_123"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_123",
                    "output": "contents"
                }
            ])
        );
    }

    #[test]
    fn parses_responses_output_as_internal_tool_use() {
        let body = r#"
        {
            "output": [
                {
                    "id": "fc_123",
                    "type": "function_call",
                    "name": "read_file",
                    "arguments": "{\"path\":\"Cargo.toml\"}",
                    "call_id": "call_123"
                }
            ]
        }
        "#;

        let parsed: Value = serde_json::from_str(body).unwrap();
        let output: Vec<ResponsesItem> = serde_json::from_value(parsed["output"].clone()).unwrap();
        let blocks = responses_output_to_message_content(output);

        assert_eq!(
            blocks,
            vec![MessageContent::ToolUse {
                id: "call_123".to_string(),
                name: "read_file".to_string(),
                input: json!({ "path": "Cargo.toml" })
            }]
        );
    }

    #[test]
    fn codex_responses_request_includes_required_instructions() {
        let req = ResponsesRequest {
            model: CODEX_MODEL.to_string(),
            instructions: CODEX_INSTRUCTIONS.to_string(),
            input: Vec::new(),
            tools: Vec::new(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            store: false,
            stream: true,
            include: Vec::new(),
        };

        let serialized = serde_json::to_value(req).unwrap();

        assert_eq!(serialized["instructions"], CODEX_INSTRUCTIONS);
        assert!(!serialized["instructions"].as_str().unwrap().is_empty());
        assert_eq!(serialized["stream"], true);
    }

    #[test]
    fn parses_responses_sse_output_item_done() {
        let item = json!({
            "type": "response.output_item.done",
            "item": {
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "output_text",
                        "text": "hello"
                    }
                ]
            }
        });
        let body = format!("event: response.output_item.done\ndata: {item}\n\n");

        let blocks = responses_stream_to_message_content(&body).unwrap();

        assert_eq!(
            blocks,
            vec![MessageContent::Text {
                text: "hello".to_string()
            }]
        );
    }

    #[test]
    fn parses_responses_sse_text_delta_fallback() {
        let body = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hel\"}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_123\"}}\n\n",
        );

        let blocks = responses_stream_to_message_content(body).unwrap();

        assert_eq!(
            blocks,
            vec![MessageContent::Text {
                text: "hello".to_string()
            }]
        );
    }
}
