use serde_json::Value;

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
