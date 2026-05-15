use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, policy::PermissionMode, provider::ProviderMetadata};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StopReason {
    Complete,
    PlanReady,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FailureKind {
    PromptAssembly,
    ProviderRequest,
    EmptyContent,
    ToolLoopLimit,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AgentEvent {
    SessionStart {
        session_id: String,
        provider: String,
        model: String,
        mode: PermissionMode,
        project_path: String,
    },
    SessionEnd {
        session_id: String,
    },
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
    PreToolUse {
        id: String,
        name: String,
        input: Value,
    },
    PostToolUse {
        id: String,
        name: String,
        input: Value,
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
        kind: FailureKind,
        message: String,
        retryable: bool,
    },
}

impl AgentEvent {
    pub(crate) fn stop_failure(kind: FailureKind, error: &Error) -> Self {
        Self::StopFailure {
            kind,
            message: error.to_string(),
            retryable: matches!(error, Error::RateLimited { .. }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::PermissionMode;
    use serde_json::json;

    #[test]
    fn events_serialize_with_stable_snake_case_types() {
        let value = serde_json::to_value(AgentEvent::SessionStart {
            session_id: "session-1".to_string(),
            provider: "ollama".to_string(),
            model: "qwen3:8b".to_string(),
            mode: PermissionMode::Default,
            project_path: "/tmp/cawir".to_string(),
        })
        .unwrap();

        assert_eq!(
            value,
            json!({
                "type": "session_start",
                "session_id": "session-1",
                "provider": "ollama",
                "model": "qwen3:8b",
                "mode": "default",
                "project_path": "/tmp/cawir"
            })
        );
    }

    #[test]
    fn stop_failure_carries_structured_metadata() {
        let event = AgentEvent::StopFailure {
            kind: FailureKind::ProviderRequest,
            message: "anthropic rate limited".to_string(),
            retryable: true,
        };

        let value = serde_json::to_value(event).unwrap();

        assert_eq!(
            value,
            json!({
                "type": "stop_failure",
                "kind": "provider_request",
                "message": "anthropic rate limited",
                "retryable": true
            })
        );
    }

    #[test]
    fn post_tool_use_serializes_original_input_for_hooks() {
        let event = AgentEvent::PostToolUse {
            id: "toolu_write".to_string(),
            name: "write_file".to_string(),
            input: json!({
                "path": "src/main.rs",
                "content": "fn main() {}\n"
            }),
            output_len: 26,
            is_error: false,
            error: None,
        };

        let value = serde_json::to_value(event).unwrap();

        assert_eq!(
            value,
            json!({
                "type": "post_tool_use",
                "id": "toolu_write",
                "name": "write_file",
                "input": {
                    "path": "src/main.rs",
                    "content": "fn main() {}\n"
                },
                "output_len": 26,
                "is_error": false,
                "error": null
            })
        );
    }
}
