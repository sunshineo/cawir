pub mod error;
pub mod session;

pub use error::{Error, Result};

use std::io::{self, Write};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::session::{Message, MessageContent};

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
    content: Vec<MessageContent>,
}

enum ClaudeResponse {
    Text(String),
    ToolUse(Vec<MessageContent>),
}

struct ToolExecution {
    assistant_content: Vec<MessageContent>,
    tool_use_id: String,
    tool_name: String,
    tool_result: String,
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
                    let history_len_before_turn = history.len();
                    history.push(Message::user_text(other));

                    match ask_claude(&client, &api_key, &history).await {
                        Ok(ClaudeResponse::Text(reply)) => {
                            println!("claude: {}", reply);
                            history.push(Message::assistant(vec![MessageContent::Text {
                                text: reply,
                            }]));
                        }
                        Ok(ClaudeResponse::ToolUse(blocks)) => {
                            match handle_one_tool_use(&client, &api_key, &mut history, blocks).await
                            {
                                Ok(()) => {}
                                Err(e) => {
                                    eprintln!("error: {}", e);
                                    history.truncate(history_len_before_turn);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("error: {}", e);
                            history.truncate(history_len_before_turn);
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
        tools: vec![list_files_tool(), read_file_tool()],
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
        .any(|block| matches!(block, MessageContent::ToolUse { .. }))
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

fn list_files_tool() -> ToolDefinition {
    ToolDefinition {
        name: "list_files".to_string(),
        description: "List the files and directories inside a folder from the current project. Use this before read_file when you need to discover repository structure or find likely files to inspect. Provide a path relative to the current working directory when possible.".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to list, preferably relative to the current working directory."
                }
            },
            "required": ["path"],
            "additionalProperties": false
        }),
    }
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

async fn handle_one_tool_use(
    client: &reqwest::Client,
    api_key: &str,
    history: &mut Vec<Message>,
    blocks: Vec<MessageContent>,
) -> Result<()> {
    let Some(execution) = execute_first_tool_use(&blocks)? else {
        return Err(Error::EmptyContent);
    };

    history.push(Message::assistant(execution.assistant_content));
    history.push(Message::user_tool_result(
        execution.tool_use_id,
        execution.tool_result,
    ));

    match ask_claude(client, api_key, history).await? {
        ClaudeResponse::Text(reply) => {
            println!("claude: {}", reply);
            history.push(Message::assistant(vec![MessageContent::Text {
                text: reply,
            }]));
        }
        ClaudeResponse::ToolUse(blocks) => {
            println!(
                "claude requested another tool after {}; 3f will add the repeat loop.",
                execution.tool_name
            );
            print_unhandled_tool_use_response(&blocks);
            return Err(Error::ToolLoopNotReady);
        }
    }

    Ok(())
}

fn execute_first_tool_use(blocks: &[MessageContent]) -> Result<Option<ToolExecution>> {
    for (index, block) in blocks.iter().enumerate() {
        match block {
            MessageContent::Text { text } => {
                println!("claude: {}", text);
            }
            MessageContent::ToolUse { id, name, input } => {
                println!("tool request: {} ({})", name, id);
                let result = execute_tool_call(name, input)?;
                print_tool_result(name, &result);
                return Ok(Some(ToolExecution {
                    assistant_content: blocks[..=index].to_vec(),
                    tool_use_id: id.clone(),
                    tool_name: name.clone(),
                    tool_result: result,
                }));
            }
            MessageContent::ToolResult { .. } => {}
        }
    }

    Ok(None)
}

fn print_unhandled_tool_use_response(blocks: &[MessageContent]) {
    for block in blocks {
        match block {
            MessageContent::Text { text } => {
                println!("claude: {}", text);
            }
            MessageContent::ToolUse { id, name, .. } => {
                println!("tool request: {} ({})", name, id);
            }
            MessageContent::ToolResult { .. } => {}
        }
    }
}

fn execute_tool_call(name: &str, input: &Value) -> Result<String> {
    match name {
        "list_files" => execute_list_files(input),
        "read_file" => execute_read_file(input),
        _ => Err(Error::UnknownTool(name.to_string())),
    }
}

fn execute_list_files(input: &Value) -> Result<String> {
    let path = input
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::ToolInput {
            tool: "list_files".to_string(),
            message: "expected input.path to be a string".to_string(),
        })?;

    let mut entries = std::fs::read_dir(path)?
        .map(|entry| -> Result<String> {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let mut name = entry.file_name().to_string_lossy().into_owned();

            if file_type.is_dir() {
                name.push('/');
            }

            Ok(name)
        })
        .collect::<Result<Vec<_>>>()?;

    entries.sort();
    Ok(entries.join("\n"))
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
            MessageContent::ToolUse { id, name, input } => {
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
            MessageContent::Text {
                text: "First text.".to_string(),
            },
            MessageContent::ToolUse {
                id: "toolu_123".to_string(),
                name: "read_file".to_string(),
                input: json!({ "path": "Cargo.toml" }),
            },
            MessageContent::Text {
                text: "Second text.".to_string(),
            },
        ];

        assert_eq!(render_text_blocks(&blocks), "First text.\nSecond text.");

        let ordered_kinds = blocks
            .iter()
            .map(|block| match block {
                MessageContent::Text { .. } => "text",
                MessageContent::ToolUse { .. } => "tool_use",
                MessageContent::ToolResult { .. } => "tool_result",
            })
            .collect::<Vec<_>>();

        assert_eq!(ordered_kinds, vec!["text", "tool_use", "text"]);
    }

    #[test]
    fn serializes_tool_result_message_for_anthropic() {
        let message = Message::user_tool_result("toolu_123".to_string(), "Cargo.toml".to_string());
        let serialized = serde_json::to_value(message).unwrap();

        assert_eq!(
            serialized,
            json!({
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_123",
                        "content": "Cargo.toml"
                    }
                ]
            })
        );
    }

    #[test]
    fn first_tool_execution_keeps_only_answered_assistant_content() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cawir-first-tool-test-{}-{}.txt",
            std::process::id(),
            unique
        ));

        std::fs::write(&path, "first result").unwrap();

        let blocks = vec![
            MessageContent::Text {
                text: "I'll inspect one file.".to_string(),
            },
            MessageContent::ToolUse {
                id: "toolu_first".to_string(),
                name: "read_file".to_string(),
                input: json!({ "path": path.to_string_lossy() }),
            },
            MessageContent::ToolUse {
                id: "toolu_second".to_string(),
                name: "list_files".to_string(),
                input: json!({ "path": "." }),
            },
        ];

        let execution = execute_first_tool_use(&blocks).unwrap().unwrap();

        assert_eq!(execution.tool_use_id, "toolu_first");
        assert_eq!(execution.tool_result, "first result");
        assert_eq!(execution.assistant_content.len(), 2);

        std::fs::remove_file(path).unwrap();
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

    #[test]
    fn executes_list_files_tool_call() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cawir-list-files-test-{}-{}",
            std::process::id(),
            unique
        ));

        std::fs::create_dir(&path).unwrap();
        std::fs::create_dir(path.join("src")).unwrap();
        std::fs::write(path.join("Cargo.toml"), "[package]\nname = \"cawir\"\n").unwrap();

        let input = json!({ "path": path.to_string_lossy() });
        let result = execute_tool_call("list_files", &input).unwrap();

        assert_eq!(result, "Cargo.toml\nsrc/");

        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn list_files_requires_a_string_path() {
        let error = execute_tool_call("list_files", &json!({})).unwrap_err();

        match error {
            Error::ToolInput { tool, message } => {
                assert_eq!(tool, "list_files");
                assert_eq!(message, "expected input.path to be a string");
            }
            other => panic!("expected tool input error, got {:?}", other),
        }
    }
}
