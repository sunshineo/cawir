use std::{
    collections::BTreeMap,
    future::Future,
    io::{self, Write},
    path::Path,
    pin::Pin,
};

use crate::{
    Error, Result,
    auth::{
        ActiveCredential, AuthOption, ProviderPreference, acquire_codex_oauth, find_option,
        load_provider_preference, resolve_for_provider, save_api_key,
    },
    events::AgentEvent,
    hooks::HookRegistry,
    plugins::{PluginCatalog, PluginCommandContribution},
    policy::PermissionMode,
    provider::{Provider, ProviderMetadata},
    runtime::{self, ActiveProvider, Runtime},
    session::{
        Message, MessageContent, Session, list_resumable_project_sessions,
        load_most_recent_session, load_session,
    },
    settings::SettingsResolver,
    skills::SkillCatalog,
    tools::{PlanReady, ToolApprovalRequest, ToolRegistry},
};

#[derive(Debug)]
pub(crate) struct ReplOptions {
    pub(crate) resume: Option<String>,
    pub(crate) continue_session: bool,
}

type CommandFuture<'a> =
    Pin<Box<dyn Future<Output = std::result::Result<CommandOutcome, String>> + 'a>>;

trait Command {
    fn name(&self) -> &str;
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

    fn for_project(project_root: &Path) -> Result<Self> {
        let settings = SettingsResolver::for_project(project_root)?.load()?;
        let plugins = PluginCatalog::from_settings(&settings, project_root)?;
        let mut registry = Self::builtins();
        for command in plugins.commands(project_root)? {
            registry.register(Box::new(PluginSlashCommand { command }))?;
        }

        Ok(registry)
    }

    fn register(&mut self, command: Box<dyn Command>) -> Result<()> {
        if self
            .commands
            .iter()
            .any(|existing| existing.name() == command.name())
        {
            return Err(Error::Env(format!(
                "duplicate slash command registered: {}",
                command.name()
            )));
        }

        self.commands.push(command);
        Ok(())
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

    #[cfg(test)]
    fn names(&self) -> Vec<&str> {
        self.commands.iter().map(|command| command.name()).collect()
    }
}

struct CommandContext<'a> {
    provider: &'a mut ActiveProvider,
    credential: &'a mut ActiveCredential,
    model: &'a mut String,
    model_preferences: &'a mut BTreeMap<String, String>,
    tool_registry: &'a mut ToolRegistry,
    hook_registry: &'a mut HookRegistry,
    skill_catalog: &'a mut SkillCatalog,
    client: &'a reqwest::Client,
    session: &'a mut Session,
}

enum CommandOutcome {
    Continue,
    Exit,
    ReloadProjectCommands,
}

struct ExitCommand;

impl Command for ExitCommand {
    fn name(&self) -> &str {
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
    fn name(&self) -> &str {
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
    fn name(&self) -> &str {
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
    fn name(&self) -> &str {
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
    fn name(&self) -> &str {
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
    fn name(&self) -> &str {
        "/resume"
    }

    fn run<'a>(&'a self, args: &'a str, context: CommandContext<'a>) -> CommandFuture<'a> {
        Box::pin(async move {
            resume_session(args, context).await?;
            Ok(CommandOutcome::ReloadProjectCommands)
        })
    }
}

struct PluginSlashCommand {
    command: PluginCommandContribution,
}

impl Command for PluginSlashCommand {
    fn name(&self) -> &str {
        &self.command.name
    }

    fn run<'a>(&'a self, args: &'a str, _context: CommandContext<'a>) -> CommandFuture<'a> {
        Box::pin(async move {
            let output = self.command.run(args).map_err(|error| error.to_string())?;
            if !output.is_empty() {
                print!("{output}");
                io::stdout().flush().map_err(|error| error.to_string())?;
            }
            Ok(CommandOutcome::Continue)
        })
    }
}

pub(crate) async fn run(options: ReplOptions) -> Result<()> {
    runtime::load_dotenv()?;

    let client = runtime::build_http_client()?;
    let preference = load_provider_preference()?;
    let resumed_session = load_requested_session(&options)?;
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
        .unwrap_or_else(|| runtime::model_for_provider(&provider, &credential, &model_preferences));
    model_preferences.insert(
        runtime::model_preference_key(&provider, &credential),
        model.clone(),
    );
    runtime::save_current_preference(&provider, &credential, &model_preferences)?;

    let mut runtime = Runtime {
        provider,
        credential,
        model,
        model_preferences,
        client,
        tool_registry: ToolRegistry::builtins(),
        hook_registry: HookRegistry::empty(),
        skill_catalog: SkillCatalog::empty(),
    };
    let mut session = resumed_session.unwrap_or_else(|| {
        Session::new(
            runtime.provider.name(),
            runtime.credential.option_name(),
            &runtime.model,
        )
    });
    let project_path = runtime::session_project_path(&session)?;
    let mut command_registry = CommandRegistry::for_project(&project_path)?;
    runtime.tool_registry = runtime::tool_registry_for_project(&project_path)?;
    runtime.skill_catalog = runtime::skill_catalog_for_project(&project_path)?;
    if is_resuming {
        warn_if_tool_fingerprint_changed(&session, &runtime.tool_registry)?;
    }
    runtime.hook_registry = HookRegistry::for_project(&project_path)?;
    runtime::sync_session_from_runtime(&mut session, &runtime)?;
    runtime::save_session_if_needed(&mut session, is_resuming)?;

    let mut render_state = TerminalRenderState::default();
    let mut render = |event| render_agent_event(event, &mut render_state);
    render(AgentEvent::SessionStart {
        session_id: session.id.clone(),
        provider: runtime.provider.name().to_string(),
        model: runtime.model.clone(),
        mode: session.mode,
        project_path: runtime::session_project_path(&session)?
            .display()
            .to_string(),
    });

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
                tool_registry: &mut runtime.tool_registry,
                hook_registry: &mut runtime.hook_registry,
                skill_catalog: &mut runtime.skill_catalog,
                client: &runtime.client,
                session: &mut session,
            };

            match command_registry.execute(trimmed, context).await {
                Some(Ok(CommandOutcome::Continue)) => {
                    runtime::sync_session_from_runtime(&mut session, &runtime)?;
                    runtime::save_session_if_needed(&mut session, is_resuming)?;
                }
                Some(Ok(CommandOutcome::Exit)) => {
                    runtime::sync_session_from_runtime(&mut session, &runtime)?;
                    runtime::save_session_if_needed(&mut session, is_resuming)?;
                    break;
                }
                Some(Ok(CommandOutcome::ReloadProjectCommands)) => {
                    let project_path = runtime::session_project_path(&session)?;
                    command_registry = CommandRegistry::for_project(&project_path)?;
                    runtime.skill_catalog = runtime::skill_catalog_for_project(&project_path)?;
                    runtime::sync_session_from_runtime(&mut session, &runtime)?;
                    runtime::save_session_if_needed(&mut session, is_resuming)?;
                }
                Some(Err(error)) => println!("{}", error),
                None => println!("unknown command: {}", trimmed),
            }
            continue;
        }

        let history_len_before_turn = session.messages.len();
        crate::agent::submit_user_prompt(trimmed, &mut session.messages, &mut render);

        let mut approve_tool = approve_tool_interactively;
        let mut approve_plan = approve_plan_interactively;
        let mut surface_hooks = runtime::SurfaceTurnHooks {
            emit: &mut render,
            approve_tool: &mut approve_tool,
            approve_plan: &mut approve_plan,
        };

        if let Err(e) = runtime::run_agent_until_complete(
            &runtime,
            runtime::session_project_path(&session)?,
            &mut session.mode,
            &mut session.messages,
            trimmed,
            &mut surface_hooks,
        )
        .await
        {
            eprintln!("error: {}", e);
            session.messages.truncate(history_len_before_turn);
        }
        runtime::sync_session_from_runtime(&mut session, &runtime)?;
        runtime::save_session_if_needed(&mut session, is_resuming)?;
    }

    render(AgentEvent::SessionEnd {
        session_id: session.id.clone(),
    });

    Ok(())
}

fn load_requested_session(options: &ReplOptions) -> Result<Option<Session>> {
    if let Some(id) = &options.resume {
        let session = load_session(id)?;
        println!("resuming session: {}", session.id);
        return Ok(Some(session));
    }

    if options.continue_session {
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
    let provider = runtime::provider_by_name(&session.provider)
        .map_err(|message| Error::Env(format!("saved session has unknown provider: {message}")))?;
    let preference = ProviderPreference {
        provider: session.provider.clone(),
        auth_option: session.auth_option.clone(),
        models: BTreeMap::from([(
            runtime::model_preference_key_parts(&session.provider, &session.auth_option),
            session.model.clone(),
        )]),
    };
    let credential = credential_for_provider(&provider, Some(&preference), client).await?;

    Ok((provider, credential))
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

    let project_path =
        runtime::session_project_path(&new_session).map_err(|error| error.to_string())?;
    let new_tool_registry =
        runtime::tool_registry_for_project(&project_path).map_err(|error| error.to_string())?;
    let new_skill_catalog =
        runtime::skill_catalog_for_project(&project_path).map_err(|error| error.to_string())?;
    warn_if_tool_fingerprint_changed(&new_session, &new_tool_registry)
        .map_err(|error| error.to_string())?;

    *context.provider = new_provider;
    *context.credential = new_credential;
    *context.model = new_session.model.clone();
    context.model_preferences.insert(
        runtime::model_preference_key(context.provider, context.credential),
        context.model.clone(),
    );
    runtime::save_current_preference(
        context.provider,
        context.credential,
        context.model_preferences,
    )
    .map_err(|error| error.to_string())?;
    *context.session = new_session;
    *context.tool_registry = new_tool_registry;
    *context.skill_catalog = new_skill_catalog;
    *context.hook_registry =
        HookRegistry::for_project(&project_path).map_err(|error| error.to_string())?;

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

fn warn_if_tool_fingerprint_changed(session: &Session, registry: &ToolRegistry) -> Result<()> {
    let current_fingerprint = registry.definition_fingerprint(session.mode)?;
    if let Some(warning) = runtime::tool_fingerprint_resume_warning(
        session.tool_definition_fingerprint.as_deref(),
        &current_fingerprint,
    ) {
        println!("{warning}");
    }

    Ok(())
}

#[derive(Default)]
struct TerminalRenderState {
    streaming_text: bool,
}

fn render_agent_event(event: AgentEvent, state: &mut TerminalRenderState) {
    if let Err(error) = render_agent_event_to_writer(event, &mut io::stdout(), state) {
        eprintln!("failed to render event: {error}");
    }
}

fn render_agent_event_to_writer(
    event: AgentEvent,
    writer: &mut impl Write,
    state: &mut TerminalRenderState,
) -> io::Result<()> {
    match event {
        AgentEvent::SessionStart { .. } | AgentEvent::SessionEnd { .. } => {}
        AgentEvent::UserPromptSubmit { prompt } => {
            let _ = prompt;
        }
        AgentEvent::ModelRequestStart {
            provider,
            model,
            tool_definition_fingerprint,
            active_skills,
        } => {
            let _ = (provider, model, tool_definition_fingerprint, active_skills);
        }
        AgentEvent::ModelRequestFinish {
            provider,
            model,
            metadata,
            tool_definition_fingerprint,
            active_skills,
        } => {
            finish_stream_line(writer, state)?;
            let observability = render_request_observability(
                &metadata,
                &tool_definition_fingerprint,
                &active_skills,
            );
            writeln!(
                writer,
                "model request from {provider}/{model}: {observability}"
            )?;
        }
        AgentEvent::PreToolUse { id, name, input } => {
            finish_stream_line(writer, state)?;
            let _ = input;
            writeln!(writer, "tool request: {} ({})", name, id)?;
        }
        AgentEvent::PostToolUse {
            name,
            output_len,
            is_error,
            error,
            ..
        } => {
            finish_stream_line(writer, state)?;
            if is_error {
                writeln!(
                    writer,
                    "tool error from {}: {}",
                    name,
                    error.unwrap_or_else(|| "unknown tool error".to_string())
                )?;
            } else if name == "exit_plan_mode" {
                writeln!(writer, "plan ready for approval")?;
            } else {
                writeln!(writer, "tool result from {}: {} bytes", name, output_len)?;
            }
        }
        AgentEvent::AssistantText { provider, text } => {
            if state.streaming_text {
                writeln!(writer)?;
                state.streaming_text = false;
            } else {
                writeln!(writer, "{}: {}", provider, text)?;
            }
        }
        AgentEvent::AssistantTextDelta { provider, text } => {
            if !state.streaming_text {
                write!(writer, "{}: ", provider)?;
                state.streaming_text = true;
            }
            write!(writer, "{text}")?;
            writer.flush()?;
        }
        AgentEvent::AssistantToolUseStart { provider, id, name } => {
            let _ = (provider, id, name);
        }
        AgentEvent::AssistantToolUseInputDelta {
            provider,
            id,
            partial_json,
        } => {
            let _ = (provider, id, partial_json);
        }
        AgentEvent::Stop { reason } => {
            finish_stream_line(writer, state)?;
            let _ = reason;
        }
        AgentEvent::StopFailure { message, .. } => {
            finish_stream_line(writer, state)?;
            let _ = message;
        }
    }

    Ok(())
}

fn finish_stream_line(writer: &mut impl Write, state: &mut TerminalRenderState) -> io::Result<()> {
    if state.streaming_text {
        writeln!(writer)?;
        state.streaming_text = false;
    }

    Ok(())
}

fn render_provider_metadata(metadata: &ProviderMetadata) -> Option<String> {
    if metadata.is_empty() {
        return None;
    }

    let usage = metadata.usage.as_ref()?;
    let mut parts = Vec::new();

    if let Some(tokens) = usage.input_tokens {
        parts.push(format!("input={tokens}"));
    }
    if let Some(tokens) = usage.output_tokens {
        parts.push(format!("output={tokens}"));
    }
    if let Some(tokens) = usage.cache_creation_input_tokens {
        parts.push(format!("cache_create={tokens}"));
    }
    if let Some(tokens) = usage.cache_read_input_tokens {
        parts.push(format!("cache_read={tokens}"));
    }

    (!parts.is_empty()).then(|| parts.join(", "))
}

fn render_request_observability(
    metadata: &ProviderMetadata,
    tool_fingerprint: &str,
    active_skills: &[String],
) -> String {
    let mut parts = render_provider_metadata(metadata)
        .map(|usage| vec![usage])
        .unwrap_or_default();
    parts.push(format!("tools={tool_fingerprint}"));
    if !active_skills.is_empty() {
        parts.push(format!("skills={}", active_skills.join(",")));
    }
    parts.join(", ")
}

fn approve_plan_interactively(plan_ready: &PlanReady) -> Result<bool> {
    println!();
    println!("proposed plan:");
    println!("{}", plan_ready.plan);
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
        let provider = runtime::provider_by_name(&name)
            .map_err(|message| Error::Env(format!("unknown provider value: {message}")))?;
        if let Some(credential) =
            runtime::try_credential_for_provider(&provider, preference, client).await?
        {
            return Ok((provider, credential));
        }
    }

    println!("No configured provider found.");
    let provider = prompt_provider()?;
    let credential = acquire_credential_for_provider(&provider, client).await?;
    Ok((provider, credential))
}

async fn credential_for_provider(
    provider: &impl Provider,
    preference: Option<&ProviderPreference>,
    client: &reqwest::Client,
) -> Result<ActiveCredential> {
    if let Some(credential) =
        runtime::try_credential_for_provider(provider, preference, client).await?
    {
        return Ok(credential);
    }

    println!("No configured credentials found for {}.", provider.name());
    acquire_credential_for_provider(provider, client).await
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

    let new_provider = runtime::provider_by_name(name)?;
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
    *model = runtime::model_for_provider(provider, active_credential, model_preferences);
    model_preferences.insert(
        runtime::model_preference_key(provider, active_credential),
        model.clone(),
    );
    runtime::save_current_preference(provider, active_credential, model_preferences)
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

        match runtime::provider_by_name(name) {
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
    model_preferences.insert(
        runtime::model_preference_key(provider, credential),
        model.clone(),
    );
    runtime::save_current_preference(provider, credential, model_preferences)
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
    for provider in runtime::available_providers() {
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
    use crate::provider::TokenUsage;
    use crate::session::is_resumable;
    use serde_json::json;
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn repl_test_path(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("cawir-repl-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{}-{}-{}", prefix, std::process::id(), unique))
    }

    #[test]
    fn model_preference_key_includes_provider_and_auth_option() {
        assert_eq!(
            runtime::model_preference_key_parts("openai", "api-key"),
            "openai:api-key"
        );
        assert_eq!(
            runtime::model_preference_key_parts("openai", "codex-oauth"),
            "openai:codex-oauth"
        );
        assert_eq!(
            runtime::model_preference_key_parts("ollama", "none"),
            "ollama:none"
        );
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
    fn command_registry_loads_plugin_slash_commands() {
        let project = repl_test_path("plugin-command");
        let plugin = project.join("plugins").join("hello");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::create_dir_all(project.join(".cawir")).unwrap();
        std::fs::write(
            project.join(".cawir").join("settings.json"),
            r#"{
                "plugins": {
                    "directories": ["plugins"]
                }
            }"#,
        )
        .unwrap();
        std::fs::write(
            plugin.join(crate::plugins::PLUGIN_MANIFEST_FILE),
            r#"{
                "name": "hello",
                "commands": [
                    {
                        "name": "/hello",
                        "command": "printf hello"
                    }
                ]
            }"#,
        )
        .unwrap();

        let registry = CommandRegistry::for_project(&project).unwrap();

        assert!(registry.names().contains(&"/hello"));

        std::fs::remove_dir_all(project).unwrap();
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
    fn provider_metadata_renders_token_usage_and_cache_counts() {
        let rendered = render_provider_metadata(&ProviderMetadata {
            usage: Some(TokenUsage {
                input_tokens: Some(100),
                output_tokens: Some(20),
                cache_creation_input_tokens: Some(30),
                cache_read_input_tokens: Some(40),
            }),
        });

        assert_eq!(
            rendered.as_deref(),
            Some("input=100, output=20, cache_create=30, cache_read=40")
        );
    }

    #[test]
    fn request_observability_includes_usage_and_tool_fingerprint() {
        let rendered = render_request_observability(
            &ProviderMetadata {
                usage: Some(TokenUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(20),
                    cache_creation_input_tokens: Some(30),
                    cache_read_input_tokens: Some(40),
                }),
            },
            "fnv1a64:abc123",
            &["rust-tutor".to_string()],
        );

        assert_eq!(
            rendered,
            "input=100, output=20, cache_create=30, cache_read=40, tools=fnv1a64:abc123, skills=rust-tutor"
        );
    }

    #[test]
    fn tool_fingerprint_warning_only_renders_on_resume_mismatch() {
        assert_eq!(
            runtime::tool_fingerprint_resume_warning(Some("fnv1a64:old"), "fnv1a64:new"),
            Some(
                "warning: tool definitions changed since this session was saved; previous fingerprint fnv1a64:old, current fingerprint fnv1a64:new. The next provider request may rebuild the prompt cache."
                    .to_string()
            )
        );
        assert_eq!(
            runtime::tool_fingerprint_resume_warning(Some("fnv1a64:same"), "fnv1a64:same"),
            None
        );
        assert_eq!(
            runtime::tool_fingerprint_resume_warning(None, "fnv1a64:new"),
            None
        );
    }

    #[test]
    fn render_streaming_text_delta_suppresses_duplicate_final_text() {
        let mut output = Vec::new();
        let mut state = TerminalRenderState::default();

        render_agent_event_to_writer(
            AgentEvent::AssistantTextDelta {
                provider: "anthropic".to_string(),
                text: "hel".to_string(),
            },
            &mut output,
            &mut state,
        )
        .unwrap();
        render_agent_event_to_writer(
            AgentEvent::AssistantTextDelta {
                provider: "anthropic".to_string(),
                text: "lo".to_string(),
            },
            &mut output,
            &mut state,
        )
        .unwrap();
        render_agent_event_to_writer(
            AgentEvent::AssistantText {
                provider: "anthropic".to_string(),
                text: "hello".to_string(),
            },
            &mut output,
            &mut state,
        )
        .unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), "anthropic: hello\n");
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
