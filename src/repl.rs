use std::{
    collections::BTreeMap,
    future::Future,
    io::{self, ErrorKind, Write},
    path::{Path, PathBuf},
    pin::Pin,
};

use clap::Parser;

use crate::{
    Error, Result, agent,
    anthropic::Anthropic,
    auth::{
        ActiveCredential, AuthOption, ProviderPreference, acquire_codex_oauth, find_option,
        load_provider_preference, resolve_for_provider, save_api_key, save_provider_preference,
    },
    events::AgentEvent,
    ollama::Ollama,
    openai::OpenAi,
    policy::PermissionMode,
    prompt::SystemPrompt,
    provider::Provider,
    session::{
        Message, MessageContent, Session, current_project_path, is_resumable,
        list_resumable_project_sessions, load_most_recent_session, load_session, save_session,
    },
    tools::ToolApprovalRequest,
};

#[derive(Debug, Parser)]
#[command(name = "cawir", about = "Coding Agent Written in Rust")]
struct Cli {
    #[arg(long, value_name = "ID", conflicts_with = "continue_session")]
    resume: Option<String>,

    #[arg(long = "continue", conflicts_with = "resume")]
    continue_session: bool,
}

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
                Box::new(ResumeCommand),
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
    session: &'a mut Session,
}

enum CommandOutcome {
    Continue,
    Exit,
}

struct Runtime {
    provider: ActiveProvider,
    credential: ActiveCredential,
    model: String,
    model_preferences: BTreeMap<String, String>,
    client: reqwest::Client,
    command_registry: CommandRegistry,
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
            switch_mode(args, &mut context.session.mode)?;
            Ok(CommandOutcome::Continue)
        })
    }
}

struct ResumeCommand;

impl Command for ResumeCommand {
    fn name(&self) -> &'static str {
        "/resume"
    }

    fn run<'a>(&'a self, args: &'a str, context: CommandContext<'a>) -> CommandFuture<'a> {
        Box::pin(async move {
            resume_session(args, context).await?;
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
        prompt: &SystemPrompt,
        messages: &[Message],
        tools: Vec<crate::provider::ToolDefinition>,
    ) -> Result<crate::provider::ProviderResponse> {
        match self {
            Self::Anthropic(provider) => {
                provider
                    .send(client, credential, model, prompt, messages, tools)
                    .await
            }
            Self::Ollama(provider) => {
                provider
                    .send(client, credential, model, prompt, messages, tools)
                    .await
            }
            Self::OpenAi(provider) => {
                provider
                    .send(client, credential, model, prompt, messages, tools)
                    .await
            }
        }
    }
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    load_dotenv()?;

    let client = reqwest::Client::builder().user_agent("cawir/0.1").build()?;
    let preference = load_provider_preference()?;
    let resumed_session = load_requested_session(&cli)?;
    let is_resuming = resumed_session.is_some();

    let (provider, credential) = if let Some(session) = &resumed_session {
        startup_provider_for_session(session, &client).await?
    } else {
        startup_provider(preference.as_ref(), &client).await?
    };

    let mut model_preferences = preference
        .as_ref()
        .map(|preference| preference.models.clone())
        .unwrap_or_default();
    let model = resumed_session
        .as_ref()
        .map(|session| session.model.clone())
        .unwrap_or_else(|| model_for_provider(&provider, &credential, &model_preferences));
    model_preferences.insert(model_preference_key(&provider, &credential), model.clone());
    save_current_preference(&provider, &credential, &model_preferences)?;

    let mut runtime = Runtime {
        provider,
        credential,
        model,
        model_preferences,
        client,
        command_registry: CommandRegistry::builtins(),
    };
    let mut session = resumed_session.unwrap_or_else(|| {
        Session::new(
            runtime.provider.name(),
            runtime.credential.option_name(),
            &runtime.model,
        )
    });
    sync_session_from_runtime(&mut session, &runtime);
    save_session_if_needed(&mut session, is_resuming)?;

    print_active_provider(&runtime.provider, &runtime.credential, &runtime.model);
    println!("session: {}", session.id);
    println!("mode: {}", session.mode.name());
    if is_resuming {
        print_transcript(&session);
    }

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
                provider: &mut runtime.provider,
                credential: &mut runtime.credential,
                model: &mut runtime.model,
                model_preferences: &mut runtime.model_preferences,
                client: &runtime.client,
                session: &mut session,
            };

            match runtime.command_registry.execute(trimmed, context).await {
                Some(Ok(CommandOutcome::Continue)) => {
                    sync_session_from_runtime(&mut session, &runtime);
                    save_session_if_needed(&mut session, is_resuming)?;
                }
                Some(Ok(CommandOutcome::Exit)) => {
                    sync_session_from_runtime(&mut session, &runtime);
                    save_session_if_needed(&mut session, is_resuming)?;
                    break;
                }
                Some(Err(error)) => println!("{}", error),
                None => println!("unknown command: {}", trimmed),
            }
            continue;
        }

        let history_len_before_turn = session.messages.len();
        let mut render = render_agent_event;
        agent::submit_user_prompt(trimmed, &mut session.messages, &mut render);

        if let Err(e) = run_agent_until_complete(
            &runtime,
            session_project_path(&session)?,
            &mut session.mode,
            &mut session.messages,
            &mut render,
        )
        .await
        {
            eprintln!("error: {}", e);
            session.messages.truncate(history_len_before_turn);
        }
        sync_session_from_runtime(&mut session, &runtime);
        save_session_if_needed(&mut session, is_resuming)?;
    }

    Ok(())
}

fn load_requested_session(cli: &Cli) -> Result<Option<Session>> {
    if let Some(id) = &cli.resume {
        let session = load_session(id)?;
        println!("resuming session: {}", session.id);
        return Ok(Some(session));
    }

    if cli.continue_session {
        let Some(session) = load_most_recent_session()? else {
            return Err(Error::Env(
                "no saved sessions found for --continue".to_string(),
            ));
        };
        println!("continuing session: {}", session.id);
        return Ok(Some(session));
    }

    Ok(None)
}

async fn startup_provider_for_session(
    session: &Session,
    client: &reqwest::Client,
) -> Result<(ActiveProvider, ActiveCredential)> {
    let provider = provider_by_name(&session.provider)
        .map_err(|message| Error::Env(format!("saved session has unknown provider: {message}")))?;
    let preference = ProviderPreference {
        provider: session.provider.clone(),
        auth_option: session.auth_option.clone(),
        models: BTreeMap::from([(
            model_preference_key_parts(&session.provider, &session.auth_option),
            session.model.clone(),
        )]),
    };
    let credential = credential_for_provider(&provider, Some(&preference), client).await?;

    Ok((provider, credential))
}

fn sync_session_from_runtime(session: &mut Session, runtime: &Runtime) {
    session.provider = runtime.provider.name().to_string();
    session.auth_option = runtime.credential.option_name().to_string();
    session.model = runtime.model.clone();
    if session.project_path.is_none() {
        session.project_path = current_project_path();
    }
}

fn save_session_if_needed(session: &mut Session, was_loaded_from_disk: bool) -> Result<()> {
    if was_loaded_from_disk || is_resumable(session) {
        save_session(session)?;
    }

    Ok(())
}

async fn resume_session(
    input: &str,
    context: CommandContext<'_>,
) -> std::result::Result<(), String> {
    let mut words = input.split_whitespace();
    let Some(id) = words.next() else {
        print_resume_sessions(context.session).map_err(|error| error.to_string())?;
        return Ok(());
    };

    if words.next().is_some() {
        return Err(resume_usage());
    }

    let new_session = load_session(id).map_err(|error| error.to_string())?;
    let (new_provider, new_credential) = startup_provider_for_session(&new_session, context.client)
        .await
        .map_err(|error| error.to_string())?;

    *context.provider = new_provider;
    *context.credential = new_credential;
    *context.model = new_session.model.clone();
    context.model_preferences.insert(
        model_preference_key(context.provider, context.credential),
        context.model.clone(),
    );
    save_current_preference(
        context.provider,
        context.credential,
        context.model_preferences,
    )
    .map_err(|error| error.to_string())?;
    *context.session = new_session;

    println!("resumed session: {}", context.session.id);
    print_active_provider(context.provider, context.credential, context.model);
    println!("mode: {}", context.session.mode.name());
    print_transcript(context.session);
    Ok(())
}

fn print_resume_sessions(current_session: &Session) -> Result<()> {
    let sessions = list_resumable_project_sessions()?
        .into_iter()
        .filter(|session| session.id != current_session.id)
        .collect::<Vec<_>>();
    if sessions.is_empty() {
        println!("saved sessions for this project: none");
        return Ok(());
    }

    println!("saved sessions for this project:");
    for session in sessions {
        println!("  {}", session_summary_line(&session));
    }
    println!();
    println!("use: /resume <session-id>");
    Ok(())
}

fn session_summary_line(session: &Session) -> String {
    format!(
        "{}  {}  {}  {} messages  updated {}",
        session.id,
        session.provider,
        session.model,
        session.messages.len(),
        session.updated_at
    )
}

fn print_transcript(session: &Session) {
    if session.messages.is_empty() {
        println!("transcript: empty");
        return;
    }

    println!();
    println!("previous conversation:");
    for message in &session.messages {
        let rendered = render_message_for_transcript(message);
        if !rendered.is_empty() {
            println!("{}: {}", message.role, rendered);
        }
    }
    println!();
}

fn render_message_for_transcript(message: &Message) -> String {
    message
        .content
        .iter()
        .map(render_content_for_transcript)
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_content_for_transcript(content: &MessageContent) -> String {
    match content {
        MessageContent::Text { text } => text.clone(),
        MessageContent::ToolUse { name, input, .. } => {
            format!("tool_use: {name} {}", compact_json(input))
        }
        MessageContent::ToolResult {
            content, is_error, ..
        } => {
            let status = if *is_error {
                "tool_result error"
            } else {
                "tool_result"
            };
            format!("{status}: {}", truncate_for_transcript(content))
        }
    }
}

fn compact_json(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

fn truncate_for_transcript(value: &str) -> String {
    const MAX_CHARS: usize = 240;

    let mut chars = value.chars();
    let truncated = chars.by_ref().take(MAX_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

async fn run_agent_until_complete(
    runtime: &Runtime,
    project_root: PathBuf,
    mode: &mut PermissionMode,
    history: &mut Vec<Message>,
    emit: &mut impl FnMut(AgentEvent),
) -> Result<()> {
    loop {
        let mut approve_tool = approve_tool_interactively;
        let mut hooks = agent::TurnHooks {
            emit,
            approve: &mut approve_tool,
        };

        let context = agent::TurnContext {
            provider: &runtime.provider,
            client: &runtime.client,
            credential: &runtime.credential,
            model: &runtime.model,
            project_root: &project_root,
            mode: *mode,
        };

        match agent::run_turn(context, history, &mut hooks).await? {
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

fn session_project_path(session: &Session) -> Result<PathBuf> {
    if let Some(project_path) = &session.project_path {
        return Ok(Path::new(project_path).to_path_buf());
    }

    std::env::current_dir().map_err(Error::Io)
}

fn render_agent_event(event: AgentEvent) {
    match event {
        AgentEvent::UserPromptSubmit { prompt } => {
            let _ = prompt;
        }
        AgentEvent::ModelRequestStart { provider, model } => {
            let _ = (provider, model);
        }
        AgentEvent::ToolUseRequested { id, name, input } => {
            let _ = input;
            println!("tool request: {} ({})", name, id);
        }
        AgentEvent::ToolUseFinished {
            name,
            output_len,
            is_error,
            error,
            ..
        } => {
            if is_error {
                println!(
                    "tool error from {}: {}",
                    name,
                    error.unwrap_or_else(|| "unknown tool error".to_string())
                );
            } else if name == "exit_plan_mode" {
                println!("plan ready for approval");
            } else {
                println!("tool result from {}: {} bytes", name, output_len);
            }
        }
        AgentEvent::AssistantText { provider, text } => {
            println!("{}: {}", provider, text);
        }
        AgentEvent::Stop { reason } => {
            let _ = reason;
        }
        AgentEvent::StopFailure { message } => {
            let _ = message;
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

fn approve_tool_interactively(request: &ToolApprovalRequest) -> Result<bool> {
    println!("{} wants to {}", request.tool_name(), request.summary());
    print!("approve? [y/N] ");
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
    println!("  /resume              list saved sessions");
    println!("  /resume <id>         switch to a saved session");
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

fn resume_usage() -> String {
    "usage: /resume <session-id>".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::is_resumable;
    use serde_json::json;

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

    #[test]
    fn resume_usage_names_resume_command() {
        assert_eq!(resume_usage(), "usage: /resume <session-id>");
    }

    #[test]
    fn transcript_renders_text_and_summarizes_tool_blocks() {
        let message = Message {
            role: "assistant".to_string(),
            content: vec![
                MessageContent::Text {
                    text: "I will inspect the file.".to_string(),
                },
                MessageContent::ToolUse {
                    id: "toolu_123".to_string(),
                    name: "read_file".to_string(),
                    input: json!({ "path": "Cargo.toml" }),
                },
            ],
        };

        let rendered = render_message_for_transcript(&message);

        assert!(rendered.contains("I will inspect the file."));
        assert!(rendered.contains(r#"tool_use: read_file {"path":"Cargo.toml"}"#));
    }

    #[test]
    fn transcript_truncates_long_tool_results() {
        let rendered = truncate_for_transcript(&"x".repeat(300));

        assert_eq!(rendered.len(), 243);
        assert!(rendered.ends_with("..."));
    }

    #[test]
    fn session_summary_line_includes_resume_id_and_metadata() {
        let mut session = Session::new("ollama", "none", "qwen3:8b");
        session.id = "session-id".to_string();
        session.updated_at = 123;
        session.messages.push(Message::user_text("hello"));

        let line = session_summary_line(&session);

        assert_eq!(
            line,
            "session-id  ollama  qwen3:8b  1 messages  updated 123"
        );
    }

    #[test]
    fn empty_sessions_are_not_resumable_conversations() {
        let session = Session::new("ollama", "none", "qwen3:8b");

        assert!(!is_resumable(&session));
    }

    #[test]
    fn sessions_with_messages_are_resumable_conversations() {
        let mut session = Session::new("ollama", "none", "qwen3:8b");
        session.messages.push(Message::user_text("hello"));

        assert!(is_resumable(&session));
    }
}
