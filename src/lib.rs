pub mod error;
pub mod session;

pub use error::{Error, Result};

use std::io::{self, Write};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::session::Message;

#[derive(Serialize)]
struct MessageRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<Message>,
    tools: Vec<ToolDefinition>,
}

#[derive(Serialize)]
struct ToolDefinition {
    name: String,
    description: String,
    input_schema: Value,
}

#[derive(Deserialize, Debug)]
struct MessageResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
}

enum ClaudeResponse {
    Text(String),
    ToolUse(Vec<ContentBlock>),
}

pub async fn run() -> Result<()> {
    let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
        Error::Env(
            "ANTHROPIC_API_KEY env var not set. Get one from console.anthropic.com.".to_string(),
        )
    })?;

    let client = reqwest::Client::builder().user_agent("cawir/0.1").build()?;

    let mut history: Vec<Message> = Vec::new();

    loop {
        print!("cawir> ");
        io::stdout().flush()?;

        let mut line = String::new();
        let bytes_read = io::stdin().read_line(&mut line)?;
        if bytes_read == 0 {
            println!();
            break;
        }

        let trimmed = line.trim();
        match trimmed {
            "" => continue,
            "/exit" => break,
            "/help" => print_help(),
            other => {
                if other.starts_with('/') {
                    println!("unknown command: {}", other);
                } else {
                    history.push(Message {
                        role: "user".to_string(),
                        content: other.to_string(),
                    });

                    match ask_claude(&client, &api_key, &history).await {
                        Ok(ClaudeResponse::Text(reply)) => {
                            println!("claude: {}", reply);
                            history.push(Message {
                                role: "assistant".to_string(),
                                content: reply,
                            });
                        }
                        Ok(ClaudeResponse::ToolUse(blocks)) => {
                            if let Err(e) = handle_tool_use_response(&blocks) {
                                eprintln!("error: {}", e);
                            }

                            // 3c can execute one tool locally, but the turn is still incomplete
                            // until 3d sends a tool_result back to Claude.
                            history.pop();
                            break;
                        }
                        Err(e) => {
                            eprintln!("error: {}", e);
                            history.pop();
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

async fn ask_claude(
    client: &reqwest::Client,
    api_key: &str,
    messages: &[Message],
) -> Result<ClaudeResponse> {
    let req = MessageRequest {
        model: "claude-haiku-4-5-20251001".to_string(),
        max_tokens: 1024,
        messages: messages.to_vec(),
        tools: vec![read_file_tool()],
    };

    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&req)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await?;
        return Err(Error::Api { status, body });
    }

    let parsed: MessageResponse = response.json().await?;
    if parsed
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolUse { .. }))
    {
        return Ok(ClaudeResponse::ToolUse(parsed.content));
    }

    let reply = render_text_blocks(&parsed.content);
    if reply.is_empty() {
        return Err(Error::EmptyContent);
    }

    Ok(ClaudeResponse::Text(reply))
}

fn read_file_tool() -> ToolDefinition {
    ToolDefinition {
        name: "read_file".to_string(),
        description: "Read the full contents of a UTF-8 text file from the current project. Use this when you need to inspect source code, configuration, documentation, or other text files before answering. Provide a path relative to the current working directory when possible. Do not use this for files outside the project unless the user clearly asks for them.".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read, preferably relative to the current working directory."
                }
            },
            "required": ["path"],
            "additionalProperties": false
        }),
    }
}

fn render_text_blocks(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn handle_tool_use_response(blocks: &[ContentBlock]) -> Result<()> {
    for block in blocks {
        match block {
            ContentBlock::Text { text } => {
                println!("claude: {}", text);
            }
            ContentBlock::ToolUse { id, name, input } => {
                println!("tool request: {} ({})", name, id);
                let result = execute_tool_call(name, input)?;
                print_tool_result(name, &result);
            }
        }
    }

    Ok(())
}

fn execute_tool_call(name: &str, input: &Value) -> Result<String> {
    match name {
        "read_file" => execute_read_file(input),
        _ => Err(Error::UnknownTool(name.to_string())),
    }
}

fn execute_read_file(input: &Value) -> Result<String> {
    let path = input
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::ToolInput {
            tool: "read_file".to_string(),
            message: "expected input.path to be a string".to_string(),
        })?;

    Ok(std::fs::read_to_string(path)?)
}

fn print_tool_result(name: &str, result: &str) {
    println!("tool result from {}:", name);
    print!("{}", result);
    if !result.ends_with('\n') {
        println!();
    }
}

fn print_help() {
    println!("  /exit   quit the REPL");
    println!("  /help   show this help");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

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
            ContentBlock::ToolUse { id, name, input } => {
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
    fn formats_mixed_blocks_in_original_order() {
        let blocks = vec![
            ContentBlock::Text {
                text: "First text.".to_string(),
            },
            ContentBlock::ToolUse {
                id: "toolu_123".to_string(),
                name: "read_file".to_string(),
                input: json!({ "path": "Cargo.toml" }),
            },
            ContentBlock::Text {
                text: "Second text.".to_string(),
            },
        ];

        assert_eq!(render_text_blocks(&blocks), "First text.\nSecond text.");

        let ordered_kinds = blocks
            .iter()
            .map(|block| match block {
                ContentBlock::Text { .. } => "text",
                ContentBlock::ToolUse { .. } => "tool_use",
            })
            .collect::<Vec<_>>();

        assert_eq!(ordered_kinds, vec!["text", "tool_use", "text"]);
    }

    #[test]
    fn executes_read_file_tool_call() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cawir-read-file-test-{}-{}.txt",
            std::process::id(),
            unique
        ));

        std::fs::write(&path, "tokio = \"1\"\nserde = \"1\"\n").unwrap();

        let input = json!({ "path": path.to_string_lossy() });
        let result = execute_tool_call("read_file", &input).unwrap();

        assert_eq!(result, "tokio = \"1\"\nserde = \"1\"\n");

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn read_file_requires_a_string_path() {
        let error = execute_tool_call("read_file", &json!({})).unwrap_err();

        match error {
            Error::ToolInput { tool, message } => {
                assert_eq!(tool, "read_file");
                assert_eq!(message, "expected input.path to be a string");
            }
            other => panic!("expected tool input error, got {:?}", other),
        }
    }
}
