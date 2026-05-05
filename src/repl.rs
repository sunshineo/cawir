use std::io::{self, ErrorKind, Write};

use crate::{
    Error, Result, agent, anthropic::Anthropic, openai::OpenAi, provider::Provider,
    session::Message,
};

enum ActiveProvider {
    Anthropic(Anthropic),
    OpenAi(OpenAi),
}

impl Provider for ActiveProvider {
    fn name(&self) -> &'static str {
        match self {
            Self::Anthropic(provider) => provider.name(),
            Self::OpenAi(provider) => provider.name(),
        }
    }

    fn api_key_env_var(&self) -> &'static str {
        match self {
            Self::Anthropic(provider) => provider.api_key_env_var(),
            Self::OpenAi(provider) => provider.api_key_env_var(),
        }
    }

    async fn send(
        &self,
        client: &reqwest::Client,
        api_key: &str,
        messages: &[Message],
        tools: Vec<crate::provider::ToolDefinition>,
    ) -> Result<crate::provider::ProviderResponse> {
        match self {
            Self::Anthropic(provider) => provider.send(client, api_key, messages, tools).await,
            Self::OpenAi(provider) => provider.send(client, api_key, messages, tools).await,
        }
    }
}

pub async fn run() -> Result<()> {
    load_dotenv()?;
    let mut provider = active_provider()?;
    let mut api_key = api_key(&provider)?;

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
                if other.split_whitespace().next() == Some("/provider") {
                    if let Err(error) = switch_provider(other, &mut provider, &mut api_key) {
                        println!("{}", error);
                    }
                } else if other.starts_with('/') {
                    println!("unknown command: {}", other);
                } else {
                    let history_len_before_turn = history.len();
                    history.push(Message::user_text(other));

                    if let Err(e) =
                        agent::run_turn(&provider, &client, &api_key, &mut history).await
                    {
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

fn active_provider() -> Result<ActiveProvider> {
    let name = std::env::var("CAWIR_PROVIDER").unwrap_or_else(|_| "anthropic".to_string());
    provider_by_name(&name)
        .map_err(|message| Error::Env(format!("unknown CAWIR_PROVIDER value: {message}")))
}

fn provider_by_name(name: &str) -> std::result::Result<ActiveProvider, String> {
    match name {
        "anthropic" => Ok(ActiveProvider::Anthropic(Anthropic)),
        "openai" => Ok(ActiveProvider::OpenAi(OpenAi)),
        other => Err(format!("{}. Expected anthropic or openai.", other)),
    }
}

fn api_key(provider: &impl Provider) -> Result<String> {
    let env_var = provider.api_key_env_var();

    std::env::var(env_var).map_err(|_| {
        Error::Env(format!(
            "{} env var not set. Add it to .env before using the {} provider.",
            env_var,
            provider.name()
        ))
    })
}

fn switch_provider(
    input: &str,
    provider: &mut ActiveProvider,
    active_api_key: &mut String,
) -> std::result::Result<(), String> {
    let mut words = input.split_whitespace();
    let _command = words.next();

    let Some(name) = words.next() else {
        print_providers(provider);
        return Ok(());
    };

    if words.next().is_some() {
        return Err("usage: /provider <anthropic|openai>".to_string());
    }

    let new_provider = provider_by_name(name)?;
    let new_api_key = api_key(&new_provider).map_err(|error| error.to_string())?;

    *provider = new_provider;
    *active_api_key = new_api_key;

    println!("provider: {}", provider.name());
    println!(
        "note: existing conversation history will be sent to {} on the next turn",
        provider.name()
    );
    Ok(())
}

fn print_help() {
    println!("  /exit                quit the REPL");
    println!("  /help                show this help");
    println!("  /provider            list providers");
    println!("  /provider <name>     switch providers");
}

fn print_providers(provider: &ActiveProvider) {
    println!("current provider: {}", provider.name());
    println!("available providers:");
    println!("  anthropic");
    println!("  openai");
    println!();
    println!("use: /provider <name>");
}
