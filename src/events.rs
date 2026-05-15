use serde_json::Value;

use crate::provider::ProviderMetadata;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StopReason {
    Complete,
    PlanReady,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum AgentEvent {
    UserPromptSubmit {
        prompt: String,
    },
    ModelRequestStart {
        provider: String,
        model: String,
    },
    ModelRequestFinish {
        provider: String,
        model: String,
        metadata: ProviderMetadata,
    },
    ToolUseRequested {
        id: String,
        name: String,
        input: Value,
    },
    ToolUseFinished {
        id: String,
        name: String,
        output_len: usize,
        is_error: bool,
        error: Option<String>,
    },
    AssistantText {
        provider: String,
        text: String,
    },
    Stop {
        reason: StopReason,
    },
    StopFailure {
        message: String,
    },
}
