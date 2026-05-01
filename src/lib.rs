mod anthropic;
pub mod error;
pub mod session;
mod tools;

pub use error::{Error, Result};

use std::io::{self, Write};

use crate::{
    anthropic::{ClaudeResponse, ask_claude},
    session::{Message, MessageContent},
};

const MAX_TOOL_ROUNDS: usize = 42;

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
        match ask_claude(client, api_key, history, tools::definitions()).await? {
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

                let tool_results = tools::execute_tool_uses(&blocks);
                if tool_results.is_empty() {
                    return Err(Error::EmptyContent);
                }

                history.push(Message::assistant(blocks));
                history.push(Message::user_tool_results(tool_results));
            }
        }
    }
}

fn print_help() {
    println!("  /exit   quit the REPL");
    println!("  /help   show this help");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::ToolResult;
    use serde_json::json;

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
}
