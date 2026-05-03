mod agent;
mod anthropic;
pub mod error;
pub mod session;
mod tools;

pub use error::{Error, Result};

use std::io::{self, ErrorKind, Write};

use crate::session::Message;

pub async fn run() -> Result<()> {
    load_dotenv()?;
    let api_key = anthropic_api_key()?;

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

                    if let Err(e) = agent::run_turn(&client, &api_key, &mut history).await {
                        eprintln!("error: {}", e);
                        history.truncate(history_len_before_turn);
                    }
                }
            }
        }
    }

    Ok(())
}

fn load_dotenv() -> Result<()> {
    match dotenvy::dotenv() {
        Ok(_) => Ok(()),
        Err(dotenvy::Error::Io(error)) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Env(format!("failed to load .env: {}", error))),
    }
}

fn anthropic_api_key() -> Result<String> {
    std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
        Error::Env(
            "ANTHROPIC_API_KEY env var not set. Add it to .env or get one from console.anthropic.com."
                .to_string(),
        )
    })
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
}
