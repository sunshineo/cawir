use std::{
    collections::BTreeMap,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use crate::{
    Error, Result, agent,
    anthropic::Anthropic,
    auth::{ActiveCredential, ProviderPreference, resolve_for_provider, save_provider_preference},
    events::AgentEvent,
    hooks::HookRegistry,
    mcp,
    ollama::Ollama,
    openai::OpenAi,
    plugins::PluginCatalog,
    policy::PermissionMode,
    provider::{Provider, ProviderRequest},
    session::{Message, Session, ToolResult, current_project_path, is_resumable, save_session},
    settings::SettingsResolver,
    skills::SkillCatalog,
    tools::{PlanReady, ToolApprovalRequest, ToolRegistry},
};

pub(crate) struct Runtime {
    pub(crate) provider: ActiveProvider,
    pub(crate) credential: ActiveCredential,
    pub(crate) model: String,
    pub(crate) model_preferences: BTreeMap<String, String>,
    pub(crate) client: reqwest::Client,
    pub(crate) tool_registry: ToolRegistry,
    pub(crate) hook_registry: HookRegistry,
    pub(crate) skill_catalog: SkillCatalog,
}

pub(crate) enum ActiveProvider {
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
        request: ProviderRequest<'_>,
    ) -> Result<crate::provider::ProviderResponse> {
        match self {
            Self::Anthropic(provider) => provider.send(request).await,
            Self::Ollama(provider) => provider.send(request).await,
            Self::OpenAi(provider) => provider.send(request).await,
        }
    }
}

pub(crate) fn load_dotenv() -> Result<()> {
    match dotenvy::dotenv() {
        Ok(_) => Ok(()),
        Err(dotenvy::Error::Io(error)) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Env(format!("failed to load .env: {}", error))),
    }
}

pub(crate) fn build_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("cawir/0.1")
        .build()
        .map_err(Error::Http)
}

pub(crate) fn provider_by_name(name: &str) -> std::result::Result<ActiveProvider, String> {
    match name {
        "anthropic" => Ok(ActiveProvider::Anthropic(Anthropic)),
        "ollama" => Ok(ActiveProvider::Ollama(Ollama)),
        "openai" => Ok(ActiveProvider::OpenAi(OpenAi)),
        other => Err(format!("{}. Expected anthropic, openai, or ollama.", other)),
    }
}

pub(crate) fn available_providers() -> [ActiveProvider; 3] {
    [
        ActiveProvider::Anthropic(Anthropic),
        ActiveProvider::Ollama(Ollama),
        ActiveProvider::OpenAi(OpenAi),
    ]
}

pub(crate) async fn configured_provider(
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

    Err(Error::Env(
        "no configured provider found for app-server; configure credentials before starting a protocol surface".to_string(),
    ))
}

pub(crate) async fn configured_provider_for_session(
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
    let credential =
        configured_credential_for_provider(&provider, Some(&preference), client).await?;

    Ok((provider, credential))
}

pub(crate) async fn configured_credential_for_provider(
    provider: &impl Provider,
    preference: Option<&ProviderPreference>,
    client: &reqwest::Client,
) -> Result<ActiveCredential> {
    let preferred_option = preference
        .filter(|preference| preference.provider == provider.name())
        .map(|preference| preference.auth_option.as_str());

    resolve_for_provider(
        provider.name(),
        provider.auth_options(),
        preferred_option,
        client,
    )
    .await
}

pub(crate) fn model_for_provider(
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

pub(crate) fn save_current_preference(
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

pub(crate) fn model_preference_key(
    provider: &impl Provider,
    credential: &ActiveCredential,
) -> String {
    model_preference_key_parts(provider.name(), credential.option_name())
}

pub(crate) fn model_preference_key_parts(provider: &str, auth_option: &str) -> String {
    format!("{provider}:{auth_option}")
}

pub(crate) async fn try_credential_for_provider(
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

pub(crate) fn sync_session_from_runtime(session: &mut Session, runtime: &Runtime) -> Result<()> {
    session.provider = runtime.provider.name().to_string();
    session.auth_option = runtime.credential.option_name().to_string();
    session.model = runtime.model.clone();
    if session.project_path.is_none() {
        session.project_path = current_project_path();
    }
    session.tool_definition_fingerprint =
        Some(runtime.tool_registry.definition_fingerprint(session.mode)?);

    Ok(())
}

pub(crate) fn load_project_context(runtime: &mut Runtime, session: &mut Session) -> Result<()> {
    let project_root = session_project_path(session)?;
    runtime.tool_registry = tool_registry_for_project(&project_root)?;
    runtime.skill_catalog = skill_catalog_for_project(&project_root)?;
    runtime.hook_registry = HookRegistry::for_project(&project_root)?;
    sync_session_from_runtime(session, runtime)
}

pub(crate) fn save_session_if_needed(
    session: &mut Session,
    was_loaded_from_disk: bool,
) -> Result<()> {
    if was_loaded_from_disk || is_resumable(session) {
        save_session(session)?;
    }

    Ok(())
}

pub(crate) struct SurfaceTurnHooks<'a, E, A, P> {
    pub(crate) emit: &'a mut E,
    pub(crate) approve_tool: &'a mut A,
    pub(crate) approve_plan: &'a mut P,
}

pub(crate) async fn run_agent_until_complete<E, A, P>(
    runtime: &Runtime,
    project_root: PathBuf,
    mode: &mut PermissionMode,
    history: &mut Vec<Message>,
    user_prompt: &str,
    surface_hooks: &mut SurfaceTurnHooks<'_, E, A, P>,
) -> Result<()>
where
    E: FnMut(AgentEvent),
    A: FnMut(&ToolApprovalRequest) -> Result<bool>,
    P: FnMut(&PlanReady) -> Result<bool>,
{
    let active_skills = runtime.skill_catalog.activate_for_prompt(user_prompt)?;
    loop {
        let mut agent_hooks = agent::TurnHooks {
            emit: &mut *surface_hooks.emit,
            approve: &mut *surface_hooks.approve_tool,
        };

        let context = agent::TurnContext {
            provider: &runtime.provider,
            client: &runtime.client,
            credential: &runtime.credential,
            model: &runtime.model,
            project_root: &project_root,
            mode: *mode,
            tool_registry: &runtime.tool_registry,
            hook_registry: &runtime.hook_registry,
            active_skills: &active_skills,
        };

        match agent::run_turn(context, history, &mut agent_hooks).await? {
            agent::TurnOutcome::Complete => return Ok(()),
            agent::TurnOutcome::PlanReady(plan_ready) => {
                if (surface_hooks.approve_plan)(&plan_ready)? {
                    *mode = PermissionMode::Default;
                    if let Some(tool_use_id) = plan_ready.tool_use_id {
                        history.push(Message::user_tool_result(
                            tool_use_id,
                            "plan approved; continue in default mode".to_string(),
                        ));
                    } else {
                        return Ok(());
                    }
                } else if let Some(tool_use_id) = plan_ready.tool_use_id {
                    history.push(Message::user_tool_results(vec![ToolResult {
                        tool_use_id,
                        content: "plan denied by user; stay in plan mode".to_string(),
                        is_error: true,
                    }]));
                } else {
                    return Ok(());
                }
            }
        }
    }
}

pub(crate) fn session_project_path(session: &Session) -> Result<PathBuf> {
    if let Some(project_path) = &session.project_path {
        return Ok(Path::new(project_path).to_path_buf());
    }

    std::env::current_dir().map_err(Error::Io)
}

pub(crate) fn tool_registry_for_project(project_root: &Path) -> Result<ToolRegistry> {
    let settings = SettingsResolver::for_project(project_root)?.load()?;
    let plugins = PluginCatalog::from_settings(&settings, project_root)?;
    let settings = plugins.merged_settings(settings);
    let mut registry = ToolRegistry::builtins();
    mcp::register_tools_from_settings(&mut registry, &settings, project_root)?;
    plugins.register_tools(&mut registry, project_root)?;
    Ok(registry)
}

pub(crate) fn skill_catalog_for_project(project_root: &Path) -> Result<SkillCatalog> {
    let settings = SettingsResolver::for_project(project_root)?.load()?;
    let plugins = PluginCatalog::from_settings(&settings, project_root)?;
    let plugin_skill_directories = plugins.skill_directories();
    let settings = plugins.merged_settings(settings);

    SkillCatalog::from_settings(&settings, project_root, plugin_skill_directories)
}

pub(crate) fn tool_fingerprint_resume_warning(
    saved_fingerprint: Option<&str>,
    current_fingerprint: &str,
) -> Option<String> {
    let saved_fingerprint = saved_fingerprint?;
    if saved_fingerprint == current_fingerprint {
        return None;
    }

    Some(format!(
        "warning: tool definitions changed since this session was saved; previous fingerprint {saved_fingerprint}, current fingerprint {current_fingerprint}. The next provider request may rebuild the prompt cache."
    ))
}
