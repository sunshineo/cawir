use std::io::{self, Write};

use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct Repo {
    full_name: String,
    description: Option<String>,
    stargazers_count: u32,
    open_issues_count: u32,
    forks_count: u32,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .user_agent("cawir/0.1")
        .build()?;

    let repo: Repo = client
        .get("https://api.github.com/repos/rust-lang/rust")
        .send()
        .await?
        .json()
        .await?;

    println!("{}", repo.full_name);
    match &repo.description {
        Some(desc) => println!("  {}", desc),
        None => println!("  (no description)"),
    }
    println!("  stars:  {}", repo.stargazers_count);
    println!("  issues: {}", repo.open_issues_count);
    println!("  forks:  {}", repo.forks_count);
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
