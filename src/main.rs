use std::io::{self, Write};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .user_agent("cawir/0.1")
        .build()?;

    let zen = client
        .get("https://api.github.com/zen")
        .send()
        .await?
        .text()
        .await?;

    println!("github says: {}", zen.trim());
    println!();

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
                    println!("you said: {}", other);
                }
            }
        }
    }

    Ok(())
}

fn print_help() {
    println!("  /exit   quit the REPL");
    println!("  /help   show this help");
}
