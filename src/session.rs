use serde::{Deserialize, Serialize};
use serde_json::Value;

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message {
    pub role: String,
    pub content: Vec<MessageContent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type")]
pub enum MessageContent {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "is_false")]
        is_error: bool,
    },
}

impl Message {
    pub fn user_text(text: &str) -> Self {
        Self {
            role: "user".to_string(),
            content: vec![MessageContent::Text {
                text: text.to_string(),
            }],
        }
    }

    pub fn assistant(content: Vec<MessageContent>) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
        }
    }

    pub fn user_tool_result(tool_use_id: String, content: String) -> Self {
        Self {
            role: "user".to_string(),
            content: vec![MessageContent::ToolResult {
                tool_use_id,
                content,
                is_error: false,
            }],
        }
    }

    pub fn user_tool_results(results: Vec<ToolResult>) -> Self {
        Self {
            role: "user".to_string(),
            content: results
                .into_iter()
                .map(
                    |ToolResult {
                         tool_use_id,
                         content,
                         is_error,
                     }| MessageContent::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    },
                )
                .collect(),
        }
    }
}
