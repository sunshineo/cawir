use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    Error, Result,
    auth::{ActiveCredential, ApiKeyCredential, AuthOption, CodexOAuthCredential, RequestAuth},
    prompt::SystemPrompt,
    provider::{
        Provider, ProviderEvent, ProviderMetadata, ProviderRequest, ProviderResponse, TokenUsage,
        ToolDefinition, api_error_from_response, read_sse_data_events,
    },
    session::{Message, MessageContent},
};

const DEFAULT_MODEL: &str = "gpt-4.1";
const DEFAULT_CODEX_MODEL: &str = "gpt-5.4";
const API_KEY_FALLBACK_MODELS: &[&str] = &[DEFAULT_MODEL];
const CODEX_FALLBACK_MODELS: &[&str] = &[DEFAULT_CODEX_MODEL];
const OPENAI_CHAT_COMPLETIONS_URL: &str = "https://api.openai.com/v1/chat/completions";
const OPENAI_MODELS_URL: &str = "https://api.openai.com/v1/models";
const CHATGPT_CODEX_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const CHATGPT_CODEX_MODELS_URL: &str =
    "https://chatgpt.com/backend-api/codex/models?client_version=0.0.0";
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
    stream: bool,
    stream_options: ChatStreamOptions,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
struct ChatStreamOptions {
    include_usage: bool,
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

#[cfg(test)]
#[derive(Deserialize, Debug)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<ChatUsage>,
}

#[derive(Debug, Default)]
struct ResponsesStreamAccumulator {
    output: Vec<ResponsesItem>,
    text_delta: String,
    metadata: ProviderMetadata,
    function_call_ids_by_item_id: BTreeMap<String, String>,
}

#[derive(Debug, Default)]
struct ChatStreamAccumulator {
    text_delta: String,
    tool_calls: BTreeMap<usize, ChatStreamToolCall>,
    metadata: ProviderMetadata,
}

#[derive(Debug, Default)]
struct ChatStreamToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
    emitted_start: bool,
}

#[derive(Debug, PartialEq)]
struct ResponsesStreamResponse {
    blocks: Vec<MessageContent>,
    metadata: ProviderMetadata,
}

#[derive(Deserialize, Debug)]
struct ChatStreamChunk {
    #[serde(default)]
    choices: Vec<ChatStreamChoice>,
    usage: Option<ChatUsage>,
    error: Option<Value>,
}

#[derive(Deserialize, Debug)]
struct ChatStreamChoice {
    delta: ChatStreamDelta,
}

#[derive(Deserialize, Debug, Default)]
struct ChatStreamDelta {
    content: Option<String>,
    tool_calls: Option<Vec<ChatStreamToolCallDelta>>,
}

#[derive(Deserialize, Debug)]
struct ChatStreamToolCallDelta {
    index: usize,
    id: Option<String>,
    function: Option<ChatStreamToolCallFunctionDelta>,
}

#[derive(Deserialize, Debug)]
struct ChatStreamToolCallFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[cfg(test)]
#[derive(Deserialize, Debug)]
struct Choice {
    message: AssistantMessage,
}

#[cfg(test)]
#[derive(Deserialize, Debug)]
struct AssistantMessage {
    content: Option<String>,
    tool_calls: Option<Vec<ResponseToolCall>>,
}

#[cfg(test)]
#[derive(Deserialize, Debug)]
struct ResponseToolCall {
    id: String,
    function: ResponseToolCallFunction,
}

#[cfg(test)]
#[derive(Deserialize, Debug)]
struct ResponseToolCallFunction {
    name: String,
    arguments: String,
}

#[derive(Deserialize, Debug)]
struct ChatUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    prompt_tokens_details: Option<OpenAiInputTokenDetails>,
}

#[derive(Deserialize, Debug)]
struct ResponsesUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    input_tokens_details: Option<OpenAiInputTokenDetails>,
}

#[derive(Deserialize, Debug)]
struct OpenAiInputTokenDetails {
    cached_tokens: Option<u64>,
}

#[derive(Deserialize, Debug)]
struct ModelsResponse {
    data: Vec<ModelInfo>,
}

#[derive(Deserialize, Debug)]
struct ModelInfo {
    id: String,
}

#[derive(Deserialize, Debug)]
struct CodexModelsResponse {
    models: Vec<CodexModelInfo>,
}

#[derive(Deserialize, Debug)]
struct CodexModelInfo {
    slug: String,
    visibility: String,
}

impl Provider for OpenAi {
    fn name(&self) -> &'static str {
        "openai"
    }

    fn auth_options(&self) -> &'static [AuthOption] {
        AUTH_OPTIONS
    }

    fn default_model(&self, credential: &ActiveCredential) -> &'static str {
        if credential.option_name() == "codex-oauth" {
            DEFAULT_CODEX_MODEL
        } else {
            DEFAULT_MODEL
        }
    }

    async fn available_models(
        &self,
        client: &reqwest::Client,
        credential: &ActiveCredential,
    ) -> Result<Vec<String>> {
        if credential.option_name() == "codex-oauth" {
            return self.available_codex_oauth_models(client, credential).await;
        }

        let response = credential
            .attach(client.get(OPENAI_MODELS_URL))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(api_error_from_response(self.name(), response).await);
        }

        let parsed: ModelsResponse = response.json().await?;
        let mut models = parsed
            .data
            .into_iter()
            .map(|model| model.id)
            .filter(|id| is_chat_completions_model(id))
            .collect::<Vec<_>>();
        models.sort();
        Ok(models)
    }

    fn fallback_models(&self, credential: &ActiveCredential) -> &'static [&'static str] {
        if credential.option_name() == "codex-oauth" {
            CODEX_FALLBACK_MODELS
        } else {
            API_KEY_FALLBACK_MODELS
        }
    }

    async fn send(&self, request: ProviderRequest<'_>) -> Result<ProviderResponse> {
        if request.credential.option_name() == "codex-oauth" {
            return self.send_codex_oauth(request).await;
        }

        let req = ChatRequest {
            model: request.model.to_string(),
            messages: to_openai_messages(request.prompt, request.messages),
            tools: request.tools.into_iter().map(OpenAiTool::from).collect(),
            stream: true,
            stream_options: ChatStreamOptions {
                include_usage: true,
            },
        };

        let response = request
            .credential
            .attach(request.client.post(OPENAI_CHAT_COMPLETIONS_URL))
            .header("content-type", "application/json")
            .json(&req)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(api_error_from_response(self.name(), response).await);
        }

        let mut accumulator = ChatStreamAccumulator::default();
        read_sse_data_events(response, |event| {
            accumulator.handle_data_event(event, request.events)
        })
        .await?;
        accumulator.finish()
    }
}

impl OpenAi {
    async fn available_codex_oauth_models(
        &self,
        client: &reqwest::Client,
        credential: &ActiveCredential,
    ) -> Result<Vec<String>> {
        let response = credential
            .attach(client.get(CHATGPT_CODEX_MODELS_URL))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(api_error_from_response(self.name(), response).await);
        }

        let parsed: CodexModelsResponse = response.json().await?;
        let mut models = parsed
            .models
            .into_iter()
            .filter(|model| model.visibility == "list")
            .map(|model| model.slug)
            .collect::<Vec<_>>();
        models.sort();
        Ok(models)
    }

    async fn send_codex_oauth(&self, request: ProviderRequest<'_>) -> Result<ProviderResponse> {
        let req = ResponsesRequest {
            model: request.model.to_string(),
            instructions: request.prompt.render_text(),
            input: to_responses_items(request.messages),
            tools: request.tools.into_iter().map(ResponsesTool::from).collect(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            store: false,
            stream: true,
            include: Vec::new(),
        };

        let response = request
            .credential
            .attach(request.client.post(CHATGPT_CODEX_RESPONSES_URL))
            .header("accept", "text/event-stream")
            .header("content-type", "application/json")
            .json(&req)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(api_error_from_response(self.name(), response).await);
        }

        let mut accumulator = ResponsesStreamAccumulator::default();
        read_sse_data_events(response, |event| {
            accumulator.handle_data_event(event, request.events)
        })
        .await?;
        let response = accumulator.finish();
        if response
            .blocks
            .iter()
            .any(|block| matches!(block, MessageContent::ToolUse { .. }))
        {
            return Ok(ProviderResponse::tool_use(
                response.blocks,
                response.metadata,
            ));
        }

        let reply = render_text_blocks(&response.blocks);
        if reply.is_empty() {
            return Err(Error::EmptyContent(self.name().to_string()));
        }

        Ok(ProviderResponse::text(reply, response.metadata))
    }
}

#[cfg(test)]
impl ChatResponse {
    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata::from_usage(
            self.usage
                .as_ref()
                .map(ChatUsage::token_usage)
                .unwrap_or_default(),
        )
    }
}

impl ChatUsage {
    fn token_usage(&self) -> TokenUsage {
        TokenUsage {
            input_tokens: self.prompt_tokens,
            output_tokens: self.completion_tokens,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: self
                .prompt_tokens_details
                .as_ref()
                .and_then(|details| details.cached_tokens),
        }
    }
}

impl ResponsesUsage {
    fn token_usage(&self) -> TokenUsage {
        TokenUsage {
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: self
                .input_tokens_details
                .as_ref()
                .and_then(|details| details.cached_tokens),
        }
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

fn to_openai_messages(prompt: &SystemPrompt, messages: &[Message]) -> Vec<OpenAiMessage> {
    let mut converted = vec![OpenAiMessage::Chat {
        role: "system".to_string(),
        content: prompt.render_text(),
    }];

    converted.extend(
        messages
            .iter()
            .flat_map(|message| match message.role.as_str() {
                "assistant" => assistant_message(message),
                "user" => user_messages(message),
                _ => Vec::new(),
            }),
    );

    converted
}

fn is_chat_completions_model(id: &str) -> bool {
    (id.starts_with("gpt-") || id.starts_with("o1") || id.starts_with("o3") || id.starts_with("o4"))
        && !id.contains("audio")
        && !id.contains("realtime")
        && !id.contains("transcribe")
        && !id.contains("tts")
        && !id.contains("image")
        && !id.contains("search")
        && !id.contains("embedding")
        && !id.contains("moderation")
        && !id.contains("deep-research")
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

#[cfg(test)]
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

impl ChatStreamAccumulator {
    fn handle_data_event(
        &mut self,
        event: String,
        emit: &mut dyn FnMut(ProviderEvent),
    ) -> Result<()> {
        if event == "[DONE]" {
            return Ok(());
        }

        let chunk: ChatStreamChunk = serde_json::from_str(&event).map_err(|error| {
            Error::Env(format!("failed to parse OpenAI chat stream event: {error}"))
        })?;
        if let Some(error) = chunk.error {
            return Err(Error::Env(format!("OpenAI chat stream failed: {error}")));
        }
        if let Some(usage) = chunk.usage {
            self.metadata = ProviderMetadata::from_usage(usage.token_usage());
        }

        for choice in chunk.choices {
            if let Some(content) = choice.delta.content
                && !content.is_empty()
            {
                emit(ProviderEvent::TextDelta {
                    text: content.clone(),
                });
                self.text_delta.push_str(&content);
            }

            if let Some(tool_calls) = choice.delta.tool_calls {
                for delta in tool_calls {
                    self.apply_tool_delta(delta, emit);
                }
            }
        }

        Ok(())
    }

    fn apply_tool_delta(
        &mut self,
        delta: ChatStreamToolCallDelta,
        emit: &mut dyn FnMut(ProviderEvent),
    ) {
        let tool_call = self.tool_calls.entry(delta.index).or_default();
        if let Some(id) = delta.id {
            tool_call.id = Some(id);
        }
        let mut argument_delta = None;
        if let Some(function) = delta.function {
            if let Some(name) = function.name {
                tool_call.name = Some(name);
            }
            if let Some(arguments) = function.arguments {
                argument_delta = Some(arguments);
            }
        }

        if !tool_call.emitted_start
            && let (Some(id), Some(name)) = (&tool_call.id, &tool_call.name)
        {
            emit(ProviderEvent::ToolUseStart {
                id: id.clone(),
                name: name.clone(),
            });
            tool_call.emitted_start = true;
        }

        if let Some(arguments) = argument_delta {
            if let Some(id) = &tool_call.id {
                emit(ProviderEvent::ToolUseInputDelta {
                    id: id.clone(),
                    partial_json: arguments.clone(),
                });
            }
            tool_call.arguments.push_str(&arguments);
        }
    }

    fn finish(self) -> Result<ProviderResponse> {
        let metadata = self.metadata.clone();
        let blocks = self.into_message_content();
        if blocks
            .iter()
            .any(|block| matches!(block, MessageContent::ToolUse { .. }))
        {
            return Ok(ProviderResponse::tool_use(blocks, metadata));
        }

        let reply = render_text_blocks(&blocks);
        if reply.is_empty() {
            return Err(Error::EmptyContent("openai".to_string()));
        }

        Ok(ProviderResponse::text(reply, metadata))
    }

    fn into_message_content(self) -> Vec<MessageContent> {
        let mut blocks = Vec::new();

        if !self.text_delta.is_empty() {
            blocks.push(MessageContent::Text {
                text: self.text_delta,
            });
        }

        blocks.extend(
            self.tool_calls
                .into_values()
                .filter_map(ChatStreamToolCall::into_message_content),
        );

        blocks
    }
}

impl ChatStreamToolCall {
    fn into_message_content(self) -> Option<MessageContent> {
        let id = self.id?;
        let name = self.name?;
        let input = if self.arguments.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&self.arguments)
                .unwrap_or_else(|_| json!({ "raw_arguments": self.arguments }))
        };

        Some(MessageContent::ToolUse { id, name, input })
    }
}

impl ResponsesStreamAccumulator {
    fn handle_data_event(
        &mut self,
        event: String,
        emit: &mut dyn FnMut(ProviderEvent),
    ) -> Result<()> {
        if event == "[DONE]" {
            return Ok(());
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
                if let Some(delta) = value.get("delta").and_then(Value::as_str)
                    && !delta.is_empty()
                {
                    emit(ProviderEvent::TextDelta {
                        text: delta.to_string(),
                    });
                    self.text_delta.push_str(delta);
                }
            }
            "response.output_item.added" => {
                if let Some(item) = value.get("item")
                    && item.get("type").and_then(Value::as_str) == Some("function_call")
                {
                    let item_id = item.get("id").and_then(Value::as_str);
                    let call_id = item.get("call_id").and_then(Value::as_str).or(item_id);
                    if let (Some(item_id), Some(call_id)) = (item_id, call_id) {
                        self.function_call_ids_by_item_id
                            .insert(item_id.to_string(), call_id.to_string());
                    }
                    if let (Some(call_id), Some(name)) =
                        (call_id, item.get("name").and_then(Value::as_str))
                    {
                        emit(ProviderEvent::ToolUseStart {
                            id: call_id.to_string(),
                            name: name.to_string(),
                        });
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                let call_id = value.get("call_id").and_then(Value::as_str).or_else(|| {
                    value
                        .get("item_id")
                        .and_then(Value::as_str)
                        .and_then(|item_id| self.function_call_ids_by_item_id.get(item_id))
                        .map(String::as_str)
                });
                if let (Some(call_id), Some(delta)) =
                    (call_id, value.get("delta").and_then(Value::as_str))
                {
                    emit(ProviderEvent::ToolUseInputDelta {
                        id: call_id.to_string(),
                        partial_json: delta.to_string(),
                    });
                }
            }
            "response.output_item.done" => {
                if let Some(item) = value.get("item")
                    && let Ok(item) = serde_json::from_value::<ResponsesItem>(item.clone())
                {
                    self.output.push(item);
                }
            }
            "response.completed" => {
                if let Some(usage) = value
                    .get("response")
                    .and_then(|response| response.get("usage"))
                    && let Ok(usage) = serde_json::from_value::<ResponsesUsage>(usage.clone())
                {
                    self.metadata = ProviderMetadata::from_usage(usage.token_usage());
                }
            }
            "response.failed" | "response.incomplete" => {
                return Err(Error::Env(format!("Responses stream failed: {value}")));
            }
            _ => {}
        }

        Ok(())
    }

    fn finish(self) -> ResponsesStreamResponse {
        let blocks = responses_output_to_message_content(self.output);
        if blocks.is_empty() && !self.text_delta.is_empty() {
            ResponsesStreamResponse {
                blocks: vec![MessageContent::Text {
                    text: self.text_delta,
                }],
                metadata: self.metadata,
            }
        } else {
            ResponsesStreamResponse {
                blocks,
                metadata: self.metadata,
            }
        }
    }
}

#[cfg(test)]
fn chat_stream_events_to_response(
    events: impl IntoIterator<Item = String>,
    emit: &mut dyn FnMut(ProviderEvent),
) -> Result<ProviderResponse> {
    let mut accumulator = ChatStreamAccumulator::default();
    for event in events {
        accumulator.handle_data_event(event, emit)?;
    }
    accumulator.finish()
}

#[cfg(test)]
fn responses_stream_events_to_response(
    events: impl IntoIterator<Item = String>,
    emit: &mut dyn FnMut(ProviderEvent),
) -> Result<ResponsesStreamResponse> {
    let mut accumulator = ResponsesStreamAccumulator::default();
    for event in events {
        accumulator.handle_data_event(event, emit)?;
    }
    Ok(accumulator.finish())
}

#[cfg(test)]
fn responses_stream_to_message_content(body: &str) -> Result<ResponsesStreamResponse> {
    let mut ignore_events = |_event: ProviderEvent| {};
    responses_stream_events_to_response(
        crate::provider::parse_sse_data_events(body),
        &mut ignore_events,
    )
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

        let messages = to_openai_messages(&test_prompt(), &history);
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
    fn parses_chat_completions_usage() {
        let body = r#"
        {
            "choices": [
                {
                    "message": {
                        "content": "done"
                    }
                }
            ],
            "usage": {
                "prompt_tokens": 101,
                "completion_tokens": 11
            }
        }
        "#;

        let parsed: ChatResponse = serde_json::from_str(body).unwrap();

        assert_eq!(
            parsed.metadata(),
            ProviderMetadata {
                usage: Some(TokenUsage {
                    input_tokens: Some(101),
                    output_tokens: Some(11),
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                })
            }
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
            model: DEFAULT_CODEX_MODEL.to_string(),
            instructions: test_prompt().render_text(),
            input: Vec::new(),
            tools: Vec::new(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            store: false,
            stream: true,
            include: Vec::new(),
        };

        let serialized = serde_json::to_value(req).unwrap();

        assert_eq!(
            serialized["instructions"],
            "<identity>\nYou are cawir.\n</identity>"
        );
        assert!(!serialized["instructions"].as_str().unwrap().is_empty());
        assert_eq!(serialized["stream"], true);
    }

    #[test]
    fn chat_completions_request_enables_streaming_and_usage() {
        let req = ChatRequest {
            model: DEFAULT_MODEL.to_string(),
            messages: Vec::new(),
            tools: Vec::new(),
            stream: true,
            stream_options: ChatStreamOptions {
                include_usage: true,
            },
        };

        let serialized = serde_json::to_value(req).unwrap();

        assert_eq!(serialized["stream"], true);
        assert_eq!(
            serialized["stream_options"],
            json!({ "include_usage": true })
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

        let response = responses_stream_to_message_content(&body).unwrap();

        assert_eq!(
            response.blocks,
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

        let response = responses_stream_to_message_content(body).unwrap();

        assert_eq!(
            response.blocks,
            vec![MessageContent::Text {
                text: "hello".to_string()
            }]
        );
    }

    #[test]
    fn parses_responses_sse_usage() {
        let body = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":222,\"output_tokens\":33}}}\n\n",
        );

        let response = responses_stream_to_message_content(body).unwrap();

        assert_eq!(
            response.metadata,
            ProviderMetadata {
                usage: Some(TokenUsage {
                    input_tokens: Some(222),
                    output_tokens: Some(33),
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                })
            }
        );
    }

    #[test]
    fn parses_chat_completions_stream_text_and_tool_deltas() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_123\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"Cargo.toml\\\"}\"}}]}}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":4}}\n\n",
            "data: [DONE]\n\n",
        );
        let mut events = Vec::new();

        let response = chat_stream_events_to_response(
            crate::provider::parse_sse_data_events(body),
            &mut |event| events.push(event),
        )
        .unwrap();

        assert_eq!(
            events,
            vec![
                ProviderEvent::TextDelta {
                    text: "hel".to_string()
                },
                ProviderEvent::TextDelta {
                    text: "lo".to_string()
                },
                ProviderEvent::ToolUseStart {
                    id: "call_123".to_string(),
                    name: "read_file".to_string(),
                },
                ProviderEvent::ToolUseInputDelta {
                    id: "call_123".to_string(),
                    partial_json: "{\"path\":".to_string(),
                },
                ProviderEvent::ToolUseInputDelta {
                    id: "call_123".to_string(),
                    partial_json: "\"Cargo.toml\"}".to_string(),
                },
            ]
        );
        assert_eq!(
            response,
            ProviderResponse::tool_use(
                vec![
                    MessageContent::Text {
                        text: "hello".to_string()
                    },
                    MessageContent::ToolUse {
                        id: "call_123".to_string(),
                        name: "read_file".to_string(),
                        input: json!({ "path": "Cargo.toml" })
                    }
                ],
                ProviderMetadata {
                    usage: Some(TokenUsage {
                        input_tokens: Some(10),
                        output_tokens: Some(4),
                        cache_creation_input_tokens: None,
                        cache_read_input_tokens: None,
                    })
                }
            )
        );
    }

    #[test]
    fn parses_responses_stream_text_delta_events() {
        let body = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hel\"}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":22,\"output_tokens\":3}}}\n\n",
        );
        let mut events = Vec::new();

        let response = responses_stream_events_to_response(
            crate::provider::parse_sse_data_events(body),
            &mut |event| events.push(event),
        )
        .unwrap();

        assert_eq!(
            events,
            vec![
                ProviderEvent::TextDelta {
                    text: "hel".to_string()
                },
                ProviderEvent::TextDelta {
                    text: "lo".to_string()
                },
            ]
        );
        assert_eq!(
            response,
            ResponsesStreamResponse {
                blocks: vec![MessageContent::Text {
                    text: "hello".to_string()
                }],
                metadata: ProviderMetadata {
                    usage: Some(TokenUsage {
                        input_tokens: Some(22),
                        output_tokens: Some(3),
                        cache_creation_input_tokens: None,
                        cache_read_input_tokens: None,
                    })
                }
            }
        );
    }

    #[test]
    fn parses_responses_stream_tool_use_deltas_by_item_id() {
        let body = concat!(
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"fc_123\",\"type\":\"function_call\",\"call_id\":\"call_123\",\"name\":\"read_file\",\"arguments\":\"\"}}\n\n",
            "event: response.function_call_arguments.delta\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_123\",\"delta\":\"{\\\"path\\\":\"}\n\n",
            "event: response.function_call_arguments.delta\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_123\",\"delta\":\"\\\"Cargo.toml\\\"}\"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"fc_123\",\"type\":\"function_call\",\"call_id\":\"call_123\",\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"Cargo.toml\\\"}\"}}\n\n",
        );
        let mut events = Vec::new();

        let response = responses_stream_events_to_response(
            crate::provider::parse_sse_data_events(body),
            &mut |event| events.push(event),
        )
        .unwrap();

        assert_eq!(
            events,
            vec![
                ProviderEvent::ToolUseStart {
                    id: "call_123".to_string(),
                    name: "read_file".to_string(),
                },
                ProviderEvent::ToolUseInputDelta {
                    id: "call_123".to_string(),
                    partial_json: "{\"path\":".to_string(),
                },
                ProviderEvent::ToolUseInputDelta {
                    id: "call_123".to_string(),
                    partial_json: "\"Cargo.toml\"}".to_string(),
                },
            ]
        );
        assert_eq!(
            response.blocks,
            vec![MessageContent::ToolUse {
                id: "call_123".to_string(),
                name: "read_file".to_string(),
                input: json!({ "path": "Cargo.toml" })
            }]
        );
    }
}
