pub mod error;
pub mod session;

pub use error::{Error, Result};

use std::io::{self, Write};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::session::{Message, MessageContent, ToolResult};

const MAX_TOOL_ROUNDS: usize = 42;
const MAX_OUTPUT_TOKENS: u32 = 16_384;

#[derive(Serialize)]
struct MessageRequest {
    model: String,
    max_tokens: u32,
    cache_control: CacheControl,
    messages: Vec<Message>,
    tools: Vec<ToolDefinition>,
}

#[derive(Serialize)]
struct CacheControl {
    #[serde(rename = "type")]
    kind: String,
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

                    if let Err(e) = run_agent_turn(&client, &api_key, &mut history).await {
                        eprintln!("error: {}", e);
                        history.truncate(history_len_before_turn);
                    }
                }
            }
        }
    }

    Ok(())
}

async fn run_agent_turn(
    client: &reqwest::Client,
    api_key: &str,
    history: &mut Vec<Message>,
) -> Result<()> {
    let mut tool_rounds = 0;

    loop {
        match ask_claude(client, api_key, history).await? {
            ClaudeResponse::Text(reply) => {
                println!("claude: {}", reply);
                history.push(Message::assistant(vec![MessageContent::Text {
                    text: reply,
                }]));
                return Ok(());
            }
            ClaudeResponse::ToolUse(blocks) => {
                tool_rounds += 1;
                if tool_rounds > MAX_TOOL_ROUNDS {
                    return Err(Error::ToolLoopLimitExceeded(MAX_TOOL_ROUNDS));
                }

                let tool_results = execute_tool_uses(&blocks);
                if tool_results.is_empty() {
                    return Err(Error::EmptyContent);
                }

                history.push(Message::assistant(blocks));
                history.push(Message::user_tool_results(tool_results));
            }
        }
    }
}

async fn ask_claude(
    client: &reqwest::Client,
    api_key: &str,
    messages: &[Message],
) -> Result<ClaudeResponse> {
    let req = MessageRequest {
        model: "claude-haiku-4-5-20251001".to_string(),
        max_tokens: MAX_OUTPUT_TOKENS,
        cache_control: CacheControl {
            kind: "ephemeral".to_string(),
        },
        messages: messages.to_vec(),
        tools: vec![list_files_tool(), read_file_tool(), write_file_tool()],
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

fn write_file_tool() -> ToolDefinition {
    ToolDefinition {
        name: "write_file".to_string(),
        description: "Write UTF-8 text content to a file in the current project. Use this only when the user asks you to create or replace a file. The write will require explicit user approval before it runs.".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write, preferably relative to the current working directory."
                },
                "content": {
                    "type": "string",
                    "description": "The complete UTF-8 text content to write to the file."
                }
            },
            "required": ["path", "content"],
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

fn execute_tool_uses(blocks: &[MessageContent]) -> Vec<ToolResult> {
    let mut results = Vec::new();

    for block in blocks {
        match block {
            MessageContent::Text { text } => {
                println!("claude: {}", text);
            }
            MessageContent::ToolUse { id, name, input } => {
                println!("tool request: {} ({})", name, id);
                let result = match execute_tool_call(name, input) {
                    Ok(content) => {
                        print_tool_result(name, &content);
                        ToolResult {
                            tool_use_id: id.clone(),
                            content,
                            is_error: false,
                        }
                    }
                    Err(error) => {
                        let content = error.to_string();
                        print_tool_error(name, &content);
                        ToolResult {
                            tool_use_id: id.clone(),
                            content,
                            is_error: true,
                        }
                    }
                };
                results.push(result);
            }
            MessageContent::ToolResult { .. } => {}
        }
    }

    results
}

fn execute_tool_call(name: &str, input: &Value) -> Result<String> {
    match name {
        "list_files" => execute_list_files(input),
        "read_file" => execute_read_file(input),
        "write_file" => execute_write_file(input),
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

fn execute_write_file(input: &Value) -> Result<String> {
    execute_write_file_with_approval(input, approve_write_interactively)
}

fn execute_write_file_with_approval<F>(input: &Value, mut approve: F) -> Result<String>
where
    F: FnMut(&str, &str) -> Result<bool>,
{
    let path = input
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::ToolInput {
            tool: "write_file".to_string(),
            message: "expected input.path to be a string".to_string(),
        })?;

    let content = input
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::ToolInput {
            tool: "write_file".to_string(),
            message: format!(
                "expected input.content to be a string; received input: {}",
                input
            ),
        })?;

    if !approve(path, content)? {
        return Err(Error::ToolDenied {
            tool: "write_file".to_string(),
            message: format!("user denied write to {}", path),
        });
    }

    std::fs::write(path, content)?;

    Ok(format!("wrote {} bytes to {}", content.len(), path))
}

fn approve_write_interactively(path: &str, content: &str) -> Result<bool> {
    println!(
        "write_file wants to write {} bytes to {}",
        content.len(),
        path
    );
    print!("approve write? [y/N] ");
    io::stdout().flush()?;

    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;

    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES"))
}

fn print_tool_result(name: &str, result: &str) {
    println!("tool result from {}: {} bytes", name, result.len());
}

fn print_tool_error(name: &str, error: &str) {
    println!("tool error from {}: {}", name, error);
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
    fn serializes_error_tool_result_message_for_anthropic() {
        let message = Message::user_tool_results(vec![ToolResult {
            tool_use_id: "toolu_123".to_string(),
            content: "io error: No such file or directory".to_string(),
            is_error: true,
        }]);
        let serialized = serde_json::to_value(message).unwrap();

        assert_eq!(
            serialized,
            json!({
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_123",
                        "content": "io error: No such file or directory",
                        "is_error": true
                    }
                ]
            })
        );
    }

    #[test]
    fn tool_loop_limit_error_names_the_limit() {
        let error = Error::ToolLoopLimitExceeded(MAX_TOOL_ROUNDS);

        assert_eq!(error.to_string(), "tool loop exceeded 42 rounds");
    }

    #[test]
    fn message_request_enables_automatic_prompt_caching() {
        let request = MessageRequest {
            model: "claude-haiku-4-5-20251001".to_string(),
            max_tokens: MAX_OUTPUT_TOKENS,
            cache_control: CacheControl {
                kind: "ephemeral".to_string(),
            },
            messages: vec![Message::user_text("hello")],
            tools: Vec::new(),
        };

        let serialized = serde_json::to_value(request).unwrap();

        assert_eq!(
            serialized.get("cache_control"),
            Some(&json!({ "type": "ephemeral" }))
        );
        assert_eq!(serialized.get("max_tokens"), Some(&json!(16_384)));
    }

    #[test]
    fn tool_execution_returns_results_for_all_tool_uses() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cawir-multi-tool-test-{}-{}",
            std::process::id(),
            unique
        ));

        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("first.txt"), "first result").unwrap();

        let blocks = vec![
            MessageContent::Text {
                text: "I'll inspect one file.".to_string(),
            },
            MessageContent::ToolUse {
                id: "toolu_first".to_string(),
                name: "read_file".to_string(),
                input: json!({ "path": path.join("first.txt").to_string_lossy() }),
            },
            MessageContent::ToolUse {
                id: "toolu_second".to_string(),
                name: "list_files".to_string(),
                input: json!({ "path": path.to_string_lossy() }),
            },
        ];

        let results = execute_tool_uses(&blocks);

        assert_eq!(
            results,
            vec![
                ToolResult {
                    tool_use_id: "toolu_first".to_string(),
                    content: "first result".to_string(),
                    is_error: false,
                },
                ToolResult {
                    tool_use_id: "toolu_second".to_string(),
                    content: "first.txt".to_string(),
                    is_error: false,
                }
            ]
        );

        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn tool_execution_turns_failures_into_error_results() {
        let blocks = vec![MessageContent::ToolUse {
            id: "toolu_missing_path".to_string(),
            name: "read_file".to_string(),
            input: json!({}),
        }];

        let results = execute_tool_uses(&blocks);

        assert_eq!(
            results,
            vec![ToolResult {
                tool_use_id: "toolu_missing_path".to_string(),
                content: "invalid input for tool read_file: expected input.path to be a string"
                    .to_string(),
                is_error: true,
            }]
        );
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

    #[test]
    fn executes_write_file_tool_call_when_approved() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cawir-write-file-test-{}-{}.txt",
            std::process::id(),
            unique
        ));

        let input = json!({
            "path": path.to_string_lossy(),
            "content": "hello from cawir\n"
        });
        let result = execute_write_file_with_approval(&input, |_, _| Ok(true)).unwrap();

        assert_eq!(
            result,
            format!("wrote 17 bytes to {}", path.to_string_lossy())
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "hello from cawir\n"
        );

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn write_file_denial_returns_tool_error() {
        let input = json!({
            "path": "scratch.txt",
            "content": "hello from cawir\n"
        });

        let error = execute_write_file_with_approval(&input, |_, _| Ok(false)).unwrap_err();

        match error {
            Error::ToolDenied { tool, message } => {
                assert_eq!(tool, "write_file");
                assert_eq!(message, "user denied write to scratch.txt");
            }
            other => panic!("expected tool denied error, got {:?}", other),
        }
    }

    #[test]
    fn write_file_requires_string_content() {
        let error = execute_tool_call("write_file", &json!({ "path": "scratch.txt" })).unwrap_err();

        match error {
            Error::ToolInput { tool, message } => {
                assert_eq!(tool, "write_file");
                assert!(message.contains("expected input.content to be a string"));
                assert!(message.contains("received input"));
                assert!(message.contains(r#""path":"scratch.txt""#));
            }
            other => panic!("expected tool input error, got {:?}", other),
        }
    }
}
