use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message {
    pub role: String,
    pub content: Vec<MessageContent>,
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
            }],
        }
    }

    pub fn user_tool_results(results: Vec<(String, String)>) -> Self {
        Self {
            role: "user".to_string(),
            content: results
                .into_iter()
                .map(|(tool_use_id, content)| MessageContent::ToolResult {
                    tool_use_id,
                    content,
                })
                .collect(),
        }
    }
}
