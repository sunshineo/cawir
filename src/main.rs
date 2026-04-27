use std::io::{self, Write};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone, Debug)]
struct Message {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct MessageRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<Message>,
}

#[derive(Deserialize, Debug)]
struct MessageResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize, Debug)]
struct ContentBlock {
    text: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY env var not set. Get one from console.anthropic.com.")?;

    let client = reqwest::Client::builder()
        .user_agent("cawir/0.1")
        .build()?;

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
                        Ok(reply) => {
                            println!("claude: {}", reply);
                            history.push(Message {
                                role: "assistant".to_string(),
                                content: reply,
                            });
                        }
                        Err(e) => {
                            eprintln!("error: {}", e);
                            // pop the user message so the next call doesn't
                            // send two user turns in a row
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
) -> Result<String, Box<dyn std::error::Error>> {
    let req = MessageRequest {
        model: "claude-haiku-4-5-20251001".to_string(),
        max_tokens: 1024,
        messages: messages.to_vec(),
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
        return Err(format!("anthropic api error {}: {}", status, body).into());
    }

    let parsed: MessageResponse = response.json().await?;

    parsed
        .content
        .first()
        .map(|block| block.text.clone())
        .ok_or_else(|| "anthropic returned empty content".into())
}

fn print_help() {
    println!("  /exit   quit the REPL");
    println!("  /help   show this help");
}
