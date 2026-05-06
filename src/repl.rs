use std::{
    collections::BTreeMap,
    future::Future,
    io::{self, ErrorKind, Write},
    pin::Pin,
};

use crate::{
    Error, Result, agent,
    anthropic::Anthropic,
    auth::{
        ActiveCredential, AuthOption, ProviderPreference, acquire_codex_oauth, find_option,
        load_provider_preference, resolve_for_provider, save_api_key, save_provider_preference,
    },
    ollama::Ollama,
    openai::OpenAi,
    policy::PermissionMode,
    provider::Provider,
    session::Message,
};

type CommandFuture<'a> =
    Pin<Box<dyn Future<Output = std::result::Result<CommandOutcome, String>> + 'a>>;

trait Command {
    fn name(&self) -> &'static str;
    fn run<'a>(&'a self, args: &'a str, context: CommandContext<'a>) -> CommandFuture<'a>;
}

struct CommandRegistry {
    commands: Vec<Box<dyn Command>>,
}

impl CommandRegistry {
    fn builtins() -> Self {
        Self {
            commands: vec![
                Box::new(ExitCommand),
                Box::new(HelpCommand),
                Box::new(ProviderCommand),
                Box::new(ModelCommand),
                Box::new(ModeCommand),
            ],
        }
    }

    async fn execute(
        &self,
        input: &str,
        context: CommandContext<'_>,
    ) -> Option<std::result::Result<CommandOutcome, String>> {
        let command_name = input.split_whitespace().next()?;
        let command = self
            .commands
            .iter()
            .find(|command| command.name() == command_name)?;
        let args = input[command_name.len()..].trim();

        Some(command.run(args, context).await)
    }
}

struct CommandContext<'a> {
    provider: &'a mut ActiveProvider,
    credential: &'a mut ActiveCredential,
    model: &'a mut String,
    model_preferences: &'a mut BTreeMap<String, String>,
    client: &'a reqwest::Client,
    mode: &'a mut PermissionMode,
}

enum CommandOutcome {
    Continue,
    Exit,
}

struct ExitCommand;

impl Command for ExitCommand {
    fn name(&self) -> &'static str {
        "/exit"
    }

    fn run<'a>(&'a self, args: &'a str, _context: CommandContext<'a>) -> CommandFuture<'a> {
        Box::pin(async move {
            if args.is_empty() {
                Ok(CommandOutcome::Exit)
            } else {
                Err("usage: /exit".to_string())
            }
        })
    }
}

struct HelpCommand;

impl Command for HelpCommand {
    fn name(&self) -> &'static str {
        "/help"
    }

    fn run<'a>(&'a self, args: &'a str, _context: CommandContext<'a>) -> CommandFuture<'a> {
        Box::pin(async move {
            if args.is_empty() {
                print_help();
                Ok(CommandOutcome::Continue)
            } else {
                Err("usage: /help".to_string())
            }
        })
    }
}

struct ProviderCommand;

impl Command for ProviderCommand {
    fn name(&self) -> &'static str {
        "/provider"
    }

    fn run<'a>(&'a self, args: &'a str, context: CommandContext<'a>) -> CommandFuture<'a> {
        Box::pin(async move {
            switch_provider(
                args,
                context.provider,
                context.credential,
                context.model,
                context.model_preferences,
                context.client,
            )
            .await?;
            Ok(CommandOutcome::Continue)
        })
    }
}

struct ModelCommand;

impl Command for ModelCommand {
    fn name(&self) -> &'static str {
        "/model"
    }

    fn run<'a>(&'a self, args: &'a str, context: CommandContext<'a>) -> CommandFuture<'a> {
        Box::pin(async move {
            switch_model(
                args,
                context.provider,
                context.credential,
                context.model,
                context.model_preferences,
                context.client,
            )
            .await?;
            Ok(CommandOutcome::Continue)
        })
    }
}

struct ModeCommand;

impl Command for ModeCommand {
    fn name(&self) -> &'static str {
        "/mode"
    }

    fn run<'a>(&'a self, args: &'a str, context: CommandContext<'a>) -> CommandFuture<'a> {
        Box::pin(async move {
            switch_mode(args, context.mode)?;
            Ok(CommandOutcome::Continue)
        })
    }
}

enum ActiveProvider {
    Anthropic(Anthropic),
    Ollama(Ollama),
    OpenAi(OpenAi),
}

impl Provider for ActiveProvider {
    fn name(&self) -> &'static str {
        match self {
            Self::Anthropic(provider) => provider.name(),
            Self::Ollama(provider) => provider.name(),
            Self::OpenAi(provider) => provider.name(),
        }
    }

    fn auth_options(&self) -> &'static [crate::auth::AuthOption] {
        match self {
            Self::Anthropic(provider) => provider.auth_options(),
            Self::Ollama(provider) => provider.auth_options(),
            Self::OpenAi(provider) => provider.auth_options(),
        }
    }

    fn default_model(&self, credential: &ActiveCredential) -> &'static str {
        match self {
            Self::Anthropic(provider) => provider.default_model(credential),
            Self::Ollama(provider) => provider.default_model(credential),
            Self::OpenAi(provider) => provider.default_model(credential),
        }
    }

    async fn available_models(
        &self,
        client: &reqwest::Client,
        credential: &ActiveCredential,
    ) -> Result<Vec<String>> {
        match self {
            Self::Anthropic(provider) => provider.available_models(client, credential).await,
            Self::Ollama(provider) => provider.available_models(client, credential).await,
            Self::OpenAi(provider) => provider.available_models(client, credential).await,
        }
    }

    fn fallback_models(&self, credential: &ActiveCredential) -> &'static [&'static str] {
        match self {
            Self::Anthropic(provider) => provider.fallback_models(credential),
            Self::Ollama(provider) => provider.fallback_models(credential),
            Self::OpenAi(provider) => provider.fallback_models(credential),
        }
    }

    async fn send(
        &self,
        client: &reqwest::Client,
        credential: &ActiveCredential,
        model: &str,
        messages: &[Message],
        tools: Vec<crate::provider::ToolDefinition>,
    ) -> Result<crate::provider::ProviderResponse> {
        match self {
            Self::Anthropic(provider) => {
                provider
                    .send(client, credential, model, messages, tools)
                    .await
            }
            Self::Ollama(provider) => {
                provider
                    .send(client, credential, model, messages, tools)
                    .await
            }
            Self::OpenAi(provider) => {
                provider
                    .send(client, credential, model, messages, tools)
                    .await
            }
        }
    }
}

pub async fn run() -> Result<()> {
    load_dotenv()?;

    let client = reqwest::Client::builder().user_agent("cawir/0.1").build()?;
    let preference = load_provider_preference()?;
    let (mut provider, mut credential) = startup_provider(preference.as_ref(), &client).await?;
    let mut model_preferences = preference
        .as_ref()
        .map(|preference| preference.models.clone())
        .unwrap_or_default();
    let mut model = model_for_provider(&provider, &credential, &model_preferences);
    model_preferences.insert(model_preference_key(&provider, &credential), model.clone());
    save_current_preference(&provider, &credential, &model_preferences)?;
    print_active_provider(&provider, &credential, &model);

    let mut history: Vec<Message> = Vec::new();
    let mut mode = PermissionMode::Default;
    let command_registry = CommandRegistry::builtins();
    println!("mode: {}", mode.name());

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
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('/') {
            let context = CommandContext {
                provider: &mut provider,
                credential: &mut credential,
                model: &mut model,
                model_preferences: &mut model_preferences,
                client: &client,
                mode: &mut mode,
            };

            match command_registry.execute(trimmed, context).await {
                Some(Ok(CommandOutcome::Continue)) => {}
                Some(Ok(CommandOutcome::Exit)) => break,
                Some(Err(error)) => println!("{}", error),
                None => println!("unknown command: {}", trimmed),
            }
            continue;
        }

        let history_len_before_turn = history.len();
        history.push(Message::user_text(trimmed));

        if let Err(e) = run_agent_until_complete(
            &provider,
            &client,
            &credential,
            &model,
            &mut mode,
            &mut history,
        )
        .await
        {
            eprintln!("error: {}", e);
            history.truncate(history_len_before_turn);
        }
    }

    Ok(())
}

async fn run_agent_until_complete<P: Provider>(
    provider: &P,
    client: &reqwest::Client,
    credential: &ActiveCredential,
    model: &str,
    mode: &mut PermissionMode,
    history: &mut Vec<Message>,
) -> Result<()> {
    loop {
        match agent::run_turn(provider, client, credential, model, *mode, history).await? {
            agent::TurnOutcome::Complete => return Ok(()),
            agent::TurnOutcome::PlanReady(plan_ready) => {
                println!();
                println!("proposed plan:");
                println!("{}", plan_ready.plan);
                if approve_plan_interactively()? {
                    *mode = PermissionMode::Default;
                    println!("mode: {}", (*mode).name());
                    if let Some(tool_use_id) = plan_ready.tool_use_id {
                        history.push(Message::user_tool_result(
                            tool_use_id,
                            "plan approved; continue in default mode".to_string(),
                        ));
                    } else {
                        return Ok(());
                    }
                } else {
                    if let Some(tool_use_id) = plan_ready.tool_use_id {
                        history.push(Message::user_tool_results(vec![
                            crate::session::ToolResult {
                                tool_use_id,
                                content: "plan denied by user; stay in plan mode".to_string(),
                                is_error: true,
                            },
                        ]));
                    } else {
                        println!("mode: {}", (*mode).name());
                        return Ok(());
                    }
                }
            }
        }
    }
}

fn approve_plan_interactively() -> Result<bool> {
    print!("approve plan and switch to default mode? [y/N] ");
    io::stdout().flush()?;

    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;

    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES"))
}

fn load_dotenv() -> Result<()> {
    match dotenvy::dotenv() {
        Ok(_) => Ok(()),
        Err(dotenvy::Error::Io(error)) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Env(format!("failed to load .env: {}", error))),
    }
}

async fn startup_provider(
    preference: Option<&ProviderPreference>,
    client: &reqwest::Client,
) -> Result<(ActiveProvider, ActiveCredential)> {
    let mut candidates = Vec::new();

    if let Ok(provider_name) = std::env::var("CAWIR_PROVIDER") {
        candidates.push(provider_name);
    }
    if let Some(preference) = preference {
        candidates.push(preference.provider.clone());
    }
    candidates.extend([
        "anthropic".to_string(),
        "openai".to_string(),
        "ollama".to_string(),
    ]);

    for name in candidates {
        let provider = provider_by_name(&name)
            .map_err(|message| Error::Env(format!("unknown provider value: {message}")))?;
        if let Some(credential) = try_credential_for_provider(&provider, preference, client).await?
        {
            return Ok((provider, credential));
        }
    }

    println!("No configured provider found.");
    let provider = prompt_provider()?;
    let credential = acquire_credential_for_provider(&provider, client).await?;
    Ok((provider, credential))
}

fn provider_by_name(name: &str) -> std::result::Result<ActiveProvider, String> {
    match name {
        "anthropic" => Ok(ActiveProvider::Anthropic(Anthropic)),
        "ollama" => Ok(ActiveProvider::Ollama(Ollama)),
        "openai" => Ok(ActiveProvider::OpenAi(OpenAi)),
        other => Err(format!("{}. Expected anthropic, openai, or ollama.", other)),
    }
}

fn available_providers() -> [ActiveProvider; 3] {
    [
        ActiveProvider::Anthropic(Anthropic),
        ActiveProvider::Ollama(Ollama),
        ActiveProvider::OpenAi(OpenAi),
    ]
}

async fn credential_for_provider(
    provider: &impl Provider,
    preference: Option<&ProviderPreference>,
    client: &reqwest::Client,
) -> Result<ActiveCredential> {
    if let Some(credential) = try_credential_for_provider(provider, preference, client).await? {
        return Ok(credential);
    }

    println!("No configured credentials found for {}.", provider.name());
    acquire_credential_for_provider(provider, client).await
}

fn model_for_provider(
    provider: &impl Provider,
    credential: &ActiveCredential,
    model_preferences: &BTreeMap<String, String>,
) -> String {
    let key = model_preference_key(provider, credential);
    model_preferences
        .get(&key)
        .or_else(|| model_preferences.get(provider.name()))
        .cloned()
        .unwrap_or_else(|| provider.default_model(credential).to_string())
}

fn save_current_preference(
    provider: &impl Provider,
    credential: &ActiveCredential,
    model_preferences: &BTreeMap<String, String>,
) -> Result<()> {
    let mut models = model_preferences.clone();
    models
        .entry(model_preference_key(provider, credential))
        .or_insert_with(|| provider.default_model(credential).to_string());

    save_provider_preference(&ProviderPreference {
        provider: provider.name().to_string(),
        auth_option: credential.option_name().to_string(),
        models,
    })
}

fn model_preference_key(provider: &impl Provider, credential: &ActiveCredential) -> String {
    model_preference_key_parts(provider.name(), credential.option_name())
}

fn model_preference_key_parts(provider: &str, auth_option: &str) -> String {
    format!("{provider}:{auth_option}")
}

async fn try_credential_for_provider(
    provider: &impl Provider,
    preference: Option<&ProviderPreference>,
    client: &reqwest::Client,
) -> Result<Option<ActiveCredential>> {
    let preferred_option = preference
        .filter(|preference| preference.provider == provider.name())
        .map(|preference| preference.auth_option.as_str());

    match resolve_for_provider(
        provider.name(),
        provider.auth_options(),
        preferred_option,
        client,
    )
    .await
    {
        Ok(credential) => Ok(Some(credential)),
        Err(_) => Ok(None),
    }
}

async fn switch_provider(
    input: &str,
    provider: &mut ActiveProvider,
    active_credential: &mut ActiveCredential,
    model: &mut String,
    model_preferences: &mut BTreeMap<String, String>,
    client: &reqwest::Client,
) -> std::result::Result<(), String> {
    let mut words = input.split_whitespace();

    let Some(name) = words.next() else {
        print_providers(provider);
        return Ok(());
    };

    let new_provider = provider_by_name(name)?;
    let mut requested_option = None;
    let mut reset = false;

    for word in words {
        match word {
            "--reset" => reset = true,
            option if requested_option.is_none() => requested_option = Some(option),
            _ => return Err(provider_usage()),
        }
    }

    let new_credential = if reset {
        acquire_credential_for_provider_with_option(&new_provider, requested_option, client)
            .await
            .map_err(|error| error.to_string())?
    } else {
        let preference = requested_option
            .map(|auth_option| ProviderPreference {
                provider: new_provider.name().to_string(),
                auth_option: auth_option.to_string(),
                models: model_preferences.clone(),
            })
            .or(load_provider_preference().map_err(|error| error.to_string())?);

        credential_for_provider(&new_provider, preference.as_ref(), client)
            .await
            .map_err(|error| error.to_string())?
    };

    *provider = new_provider;
    *active_credential = new_credential;
    *model = model_for_provider(provider, active_credential, model_preferences);
    model_preferences.insert(
        model_preference_key(provider, active_credential),
        model.clone(),
    );
    save_current_preference(provider, active_credential, model_preferences)
        .map_err(|error| error.to_string())?;

    println!(
        "note: existing conversation history will be sent to {} on the next turn",
        provider.name()
    );
    Ok(())
}

async fn acquire_credential_for_provider(
    provider: &impl Provider,
    client: &reqwest::Client,
) -> Result<ActiveCredential> {
    acquire_credential_for_provider_with_option(provider, None, client).await
}

async fn acquire_credential_for_provider_with_option(
    provider: &impl Provider,
    requested_option: Option<&str>,
    client: &reqwest::Client,
) -> Result<ActiveCredential> {
    println!("Set up credentials for {}.", provider.name());
    let option = if let Some(requested_option) = requested_option {
        let Some(option) = find_option(provider.auth_options(), requested_option) else {
            return Err(Error::Env(format!(
                "unknown credential option for {}: {}",
                provider.name(),
                requested_option
            )));
        };
        option
    } else if let Some(option) = find_option(provider.auth_options(), "none") {
        option
    } else {
        print_auth_options(provider.auth_options());
        prompt_auth_option(provider.auth_options())?
    };

    match option {
        AuthOption::None => {
            let credential =
                resolve_for_provider(provider.name(), provider.auth_options(), None, client)
                    .await?;
            Ok(credential)
        }
        AuthOption::ApiKey(_) => {
            let prompt = format!(
                "Paste {} for {}: ",
                option.env_var().unwrap_or("API key"),
                provider.name()
            );
            let api_key = rpassword::prompt_password(prompt)?;
            let credential = save_api_key(option, api_key.trim())?;
            Ok(credential)
        }
        AuthOption::CodexOAuth(_) => {
            let credential = acquire_codex_oauth(option, client).await?;
            Ok(credential)
        }
    }
}

fn print_auth_options(options: &[AuthOption]) {
    println!("available credential options:");
    for option in options.iter().filter(|option| option.is_acquirable()) {
        println!("  {}", option.name());
    }
}

fn prompt_auth_option(options: &[AuthOption]) -> Result<&AuthOption> {
    loop {
        print!("credential option> ");
        io::stdout().flush()?;

        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let name = line.trim();
        if let Some(option) = find_option(options, name)
            && option.is_acquirable()
        {
            return Ok(option);
        }

        println!("unknown credential option: {}", name);
        print_auth_options(options);
    }
}

fn prompt_provider() -> Result<ActiveProvider> {
    loop {
        print_available_providers();
        print!("provider> ");
        io::stdout().flush()?;

        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let name = line.trim();

        match provider_by_name(name) {
            Ok(provider) => return Ok(provider),
            Err(error) => println!("{}", error),
        }
    }
}

fn print_help() {
    println!("  /exit                quit the REPL");
    println!("  /help                show this help");
    println!("  /mode                show current permission mode");
    println!("  /mode <name>         switch permission modes");
    println!("  /model               show current model");
    println!("  /model <name>        switch model for the current provider");
    println!("  /provider            list providers");
    println!("  /provider <name>     switch providers");
    println!("  /provider <name> <credential-option> --reset");
}

fn switch_mode(input: &str, mode: &mut PermissionMode) -> std::result::Result<(), String> {
    let mut words = input.split_whitespace();

    let Some(mode_name) = words.next() else {
        print_mode(*mode);
        return Ok(());
    };

    if words.next().is_some() {
        return Err(mode_usage());
    }

    let Some(new_mode) = PermissionMode::parse(mode_name) else {
        return Err(format!("unknown mode: {mode_name}\n{}", mode_usage()));
    };

    *mode = new_mode;
    print_mode(*mode);
    Ok(())
}

fn print_mode(mode: PermissionMode) {
    println!("current mode: {}", mode.name());
    println!("available modes:");
    println!("  default");
    println!("  plan");
    println!("  accept-edits");
    println!("  bypass");
}

async fn switch_model(
    input: &str,
    provider: &ActiveProvider,
    credential: &ActiveCredential,
    model: &mut String,
    model_preferences: &mut BTreeMap<String, String>,
    client: &reqwest::Client,
) -> std::result::Result<(), String> {
    let mut words = input.split_whitespace();

    let Some(new_model) = words.next() else {
        print_model(provider, credential, model, client).await;
        return Ok(());
    };

    if words.next().is_some() {
        return Err(model_usage());
    }

    *model = new_model.to_string();
    model_preferences.insert(model_preference_key(provider, credential), model.clone());
    save_current_preference(provider, credential, model_preferences)
        .map_err(|error| error.to_string())?;
    Ok(())
}

async fn print_model(
    provider: &ActiveProvider,
    credential: &ActiveCredential,
    model: &str,
    client: &reqwest::Client,
) {
    println!("current provider: {}", provider.name());
    println!("current model: {}", model);
    println!(
        "default model for {}: {}",
        provider.name(),
        provider.default_model(credential)
    );
    match provider.available_models(client, credential).await {
        Ok(models) if models.is_empty() => {
            println!("available models: none returned by provider");
        }
        Ok(models) => {
            println!("available models:");
            for available_model in models {
                println!("  {}", available_model);
            }
        }
        Err(error) => {
            println!("available models: failed to query provider: {}", error);
            println!("fallback models:");
            for fallback_model in provider.fallback_models(credential) {
                println!("  {}", fallback_model);
            }
        }
    }
    println!();
    println!("use: /model <name>");
}

fn print_active_provider(provider: &impl Provider, credential: &ActiveCredential, model: &str) {
    println!(
        "provider: {} ({} from {})",
        provider.name(),
        credential.option_name(),
        credential.source_name()
    );
    println!("model: {}", model);
}

fn print_providers(provider: &ActiveProvider) {
    println!("current provider: {}", provider.name());
    print_available_providers();
    println!();
    println!("use: /provider <name>");
    println!("use: /provider <name> <credential-option> --reset");
}

fn print_available_providers() {
    println!("available providers:");
    for provider in available_providers() {
        let credential_options = provider
            .auth_options()
            .iter()
            .map(AuthOption::name)
            .collect::<Vec<_>>()
            .join(", ");
        let credential_options = if credential_options.is_empty() {
            "none".to_string()
        } else {
            credential_options
        };

        println!(
            "  {} (credential options: {})",
            provider.name(),
            credential_options
        );
    }
}

fn provider_usage() -> String {
    "usage: /provider <anthropic|openai|ollama> [none|api-key|codex-oauth] [--reset]".to_string()
}

fn model_usage() -> String {
    "usage: /model [model-name]".to_string()
}

fn mode_usage() -> String {
    "usage: /mode [default|plan|accept-edits|bypass]".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_preference_key_includes_provider_and_auth_option() {
        assert_eq!(
            model_preference_key_parts("openai", "api-key"),
            "openai:api-key"
        );
        assert_eq!(
            model_preference_key_parts("openai", "codex-oauth"),
            "openai:codex-oauth"
        );
        assert_eq!(model_preference_key_parts("ollama", "none"), "ollama:none");
    }

    #[test]
    fn switch_mode_accepts_known_modes() {
        let mut mode = PermissionMode::Default;

        switch_mode("plan", &mut mode).unwrap();

        assert_eq!(mode, PermissionMode::Plan);
    }

    #[test]
    fn switch_mode_rejects_unknown_modes() {
        let mut mode = PermissionMode::Default;
        let error = switch_mode("turbo", &mut mode).unwrap_err();

        assert!(error.contains("unknown mode: turbo"));
        assert_eq!(mode, PermissionMode::Default);
    }
}
