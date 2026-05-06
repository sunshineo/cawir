use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const CODEX_AUTH_ISSUER: &str = "https://auth.openai.com";
const TOKEN_EXPIRY_SKEW_SECONDS: u64 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AuthOption {
    None,
    ApiKey(ApiKeyCredential),
    CodexOAuth(CodexOAuthCredential),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ApiKeyCredential {
    pub(crate) env_var: &'static str,
    pub(crate) storage_key: &'static str,
    pub(crate) attachment: RequestAuth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CodexOAuthCredential {
    pub(crate) env_var: &'static str,
    pub(crate) storage_key: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestAuth {
    None,
    Bearer,
    Header(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActiveCredential {
    option_name: &'static str,
    request_auth: RequestAuth,
    secret: String,
    source: CredentialSource,
    chatgpt_account_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CredentialSource {
    Local,
    ConfigFile,
    Environment,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct ProviderPreference {
    pub(crate) provider: String,
    pub(crate) auth_option: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
struct StoredCredentials {
    credentials: BTreeMap<String, StoredCredential>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StoredCredential {
    ApiKey { secret: String },
    CodexOAuth { tokens: CodexOAuthTokens },
}

#[derive(Debug, Deserialize)]
struct DeviceUserCodeResponse {
    device_auth_id: String,
    #[serde(alias = "user_code", alias = "usercode")]
    user_code: String,
    interval: String,
}

#[derive(Debug, Deserialize)]
struct DeviceTokenResponse {
    authorization_code: String,
    code_verifier: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
struct CodexOAuthTokens {
    id_token: String,
    access_token: String,
    refresh_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chatgpt_account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthExchangeResponse {
    id_token: String,
    access_token: String,
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct OAuthRefreshResponse {
    id_token: Option<String>,
    access_token: Option<String>,
    refresh_token: Option<String>,
}

impl AuthOption {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ApiKey(_) => "api-key",
            Self::CodexOAuth(_) => "codex-oauth",
        }
    }

    pub(crate) fn env_var(&self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::ApiKey(auth) => Some(auth.env_var),
            Self::CodexOAuth(auth) => Some(auth.env_var),
        }
    }

    pub(crate) fn is_acquirable(&self) -> bool {
        !matches!(self, Self::None)
    }

    fn storage_key(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ApiKey(auth) => auth.storage_key,
            Self::CodexOAuth(auth) => auth.storage_key,
        }
    }

    fn request_auth(&self) -> RequestAuth {
        match self {
            Self::None => RequestAuth::None,
            Self::ApiKey(auth) => auth.attachment,
            Self::CodexOAuth(_) => RequestAuth::Bearer,
        }
    }
}

impl RequestAuth {
    fn attach(self, request: reqwest::RequestBuilder, secret: &str) -> reqwest::RequestBuilder {
        match self {
            Self::None => request,
            Self::Bearer => request.bearer_auth(secret),
            Self::Header(name) => request.header(name, secret),
        }
    }
}

impl ActiveCredential {
    pub(crate) fn option_name(&self) -> &'static str {
        self.option_name
    }

    pub(crate) fn source_name(&self) -> &'static str {
        match self.source {
            CredentialSource::Local => "local",
            CredentialSource::ConfigFile => "config file",
            CredentialSource::Environment => "environment/.env",
        }
    }

    pub(crate) fn attach(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let request = self.request_auth.attach(request, &self.secret);

        if let Some(account_id) = &self.chatgpt_account_id {
            request.header("ChatGPT-Account-ID", account_id)
        } else {
            request
        }
    }
}

pub(crate) async fn resolve_for_provider(
    provider: &str,
    options: &[AuthOption],
    preferred_option: Option<&str>,
    client: &reqwest::Client,
) -> Result<ActiveCredential> {
    for option in ordered_options(options, preferred_option) {
        if let Some(credential) = resolve(option, client).await? {
            return Ok(credential);
        }
    }

    let accepted = options
        .iter()
        .map(AuthOption::name)
        .collect::<Vec<_>>()
        .join(", ");
    let env_vars = options
        .iter()
        .filter_map(AuthOption::env_var)
        .collect::<Vec<_>>()
        .join(", ");
    let checked_env_vars = if env_vars.is_empty() {
        "none".to_string()
    } else {
        env_vars
    };

    Err(Error::Env(format!(
        "no credentials found for {provider}. Checked credentials.json, then environment/.env vars: {checked_env_vars}. Accepted credential options: {accepted}."
    )))
}

pub(crate) fn find_option<'a>(options: &'a [AuthOption], name: &str) -> Option<&'a AuthOption> {
    options.iter().find(|option| option.name() == name)
}

pub(crate) fn save_api_key(option: &AuthOption, api_key: &str) -> Result<ActiveCredential> {
    let AuthOption::ApiKey(_) = option else {
        return Err(Error::Env(format!(
            "{} does not accept direct API-key setup",
            option.name()
        )));
    };
    if api_key.is_empty() {
        return Err(Error::Env("API key cannot be empty".to_string()));
    }

    write_stored_credential(
        option.storage_key(),
        StoredCredential::ApiKey {
            secret: api_key.to_string(),
        },
    )?;
    Ok(ActiveCredential {
        option_name: option.name(),
        request_auth: option.request_auth(),
        secret: api_key.to_string(),
        source: CredentialSource::ConfigFile,
        chatgpt_account_id: None,
    })
}

pub(crate) async fn acquire_codex_oauth(
    option: &AuthOption,
    client: &reqwest::Client,
) -> Result<ActiveCredential> {
    let AuthOption::CodexOAuth(_) = option else {
        return Err(Error::Env(format!(
            "{} does not use Codex OAuth setup",
            option.name()
        )));
    };

    let device = request_device_code(client).await?;
    println!();
    println!("Open this URL in your browser and sign in with ChatGPT:");
    println!("  {}/codex/device", CODEX_AUTH_ISSUER);
    println!();
    println!("Enter this one-time code:");
    println!("  {}", device.user_code);
    println!();
    println!("Waiting for browser sign-in...");

    let token_response = poll_device_token(client, &device).await?;
    let tokens = exchange_authorization_code(client, &token_response).await?;
    let tokens = CodexOAuthTokens {
        chatgpt_account_id: chatgpt_account_id(&tokens.id_token),
        id_token: tokens.id_token,
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
    };

    write_stored_credential(
        option.storage_key(),
        StoredCredential::CodexOAuth {
            tokens: tokens.clone(),
        },
    )?;

    Ok(active_oauth_credential(
        option,
        tokens,
        CredentialSource::ConfigFile,
    ))
}

pub(crate) fn load_provider_preference() -> Result<Option<ProviderPreference>> {
    let path = provider_config_path()?;
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Error::Io(error)),
    };

    serde_json::from_str(&contents)
        .map(Some)
        .map_err(|error| Error::Env(format!("failed to parse {}: {error}", path.display())))
}

pub(crate) fn save_provider_preference(provider: &str, auth_option: &str) -> Result<()> {
    let path = provider_config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let preference = ProviderPreference {
        provider: provider.to_string(),
        auth_option: auth_option.to_string(),
    };
    let contents =
        serde_json::to_string_pretty(&preference).map_err(|error| Error::Env(error.to_string()))?;
    fs::write(path, contents)?;
    Ok(())
}

pub(crate) fn project_config_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("dev", "cawir", "cawir")
        .ok_or_else(|| Error::Env("could not determine OS config directory".to_string()))?;

    Ok(dirs.config_dir().to_path_buf())
}

fn provider_config_path() -> Result<PathBuf> {
    Ok(project_config_dir()?.join("provider.json"))
}

fn credentials_config_path() -> Result<PathBuf> {
    Ok(project_config_dir()?.join("credentials.json"))
}

async fn resolve(
    option: &AuthOption,
    client: &reqwest::Client,
) -> Result<Option<ActiveCredential>> {
    if matches!(option, AuthOption::None) {
        return Ok(Some(ActiveCredential {
            option_name: option.name(),
            request_auth: option.request_auth(),
            secret: String::new(),
            source: CredentialSource::Local,
            chatgpt_account_id: None,
        }));
    }

    if let Some(stored) = read_stored_credential(option.storage_key())? {
        return match (option, stored) {
            (AuthOption::ApiKey(_), StoredCredential::ApiKey { secret }) => {
                Ok(Some(ActiveCredential {
                    option_name: option.name(),
                    request_auth: option.request_auth(),
                    secret,
                    source: CredentialSource::ConfigFile,
                    chatgpt_account_id: None,
                }))
            }
            (AuthOption::CodexOAuth(_), StoredCredential::CodexOAuth { mut tokens }) => {
                if token_needs_refresh(&tokens.access_token) {
                    tokens = refresh_codex_oauth(client, tokens).await?;
                    write_stored_credential(
                        option.storage_key(),
                        StoredCredential::CodexOAuth {
                            tokens: tokens.clone(),
                        },
                    )?;
                }
                Ok(Some(active_oauth_credential(
                    option,
                    tokens,
                    CredentialSource::ConfigFile,
                )))
            }
            _ => Ok(None),
        };
    }

    if let Some(env_var) = option.env_var()
        && let Ok(secret) = std::env::var(env_var)
        && !secret.is_empty()
    {
        return Ok(Some(ActiveCredential {
            option_name: option.name(),
            request_auth: option.request_auth(),
            secret,
            source: CredentialSource::Environment,
            chatgpt_account_id: None,
        }));
    }

    Ok(None)
}

fn active_oauth_credential(
    option: &AuthOption,
    tokens: CodexOAuthTokens,
    source: CredentialSource,
) -> ActiveCredential {
    ActiveCredential {
        option_name: option.name(),
        request_auth: option.request_auth(),
        secret: tokens.access_token,
        source,
        chatgpt_account_id: tokens.chatgpt_account_id,
    }
}

async fn refresh_codex_oauth(
    client: &reqwest::Client,
    mut tokens: CodexOAuthTokens,
) -> Result<CodexOAuthTokens> {
    let response = client
        .post(format!("{CODEX_AUTH_ISSUER}/oauth/token"))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "client_id": CODEX_CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": tokens.refresh_token,
        }))
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await?;
        return Err(Error::Env(format!(
            "saved Codex OAuth credential could not be refreshed: {status}: {body}"
        )));
    }

    let refreshed: OAuthRefreshResponse = response.json().await?;
    if let Some(id_token) = refreshed.id_token {
        tokens.chatgpt_account_id = chatgpt_account_id(&id_token);
        tokens.id_token = id_token;
    }
    if let Some(access_token) = refreshed.access_token {
        tokens.access_token = access_token;
    }
    if let Some(refresh_token) = refreshed.refresh_token {
        tokens.refresh_token = refresh_token;
    }
    Ok(tokens)
}

async fn request_device_code(client: &reqwest::Client) -> Result<DeviceUserCodeResponse> {
    let response = client
        .post(format!(
            "{CODEX_AUTH_ISSUER}/api/accounts/deviceauth/usercode"
        ))
        .json(&serde_json::json!({ "client_id": CODEX_CLIENT_ID }))
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await?;
        return Err(Error::Env(format!(
            "device-code request failed: {status}: {body}"
        )));
    }

    Ok(response.json().await?)
}

async fn poll_device_token(
    client: &reqwest::Client,
    device: &DeviceUserCodeResponse,
) -> Result<DeviceTokenResponse> {
    let interval = device.interval.trim().parse::<u64>().unwrap_or(5);
    let max_wait = Duration::from_secs(15 * 60);
    let started = SystemTime::now();

    loop {
        let response = client
            .post(format!("{CODEX_AUTH_ISSUER}/api/accounts/deviceauth/token"))
            .json(&serde_json::json!({
                "device_auth_id": device.device_auth_id,
                "user_code": device.user_code,
            }))
            .send()
            .await?;

        let status = response.status();
        if status.is_success() {
            return Ok(response.json().await?);
        }

        if status != reqwest::StatusCode::FORBIDDEN && status != reqwest::StatusCode::NOT_FOUND {
            let body = response.text().await?;
            return Err(Error::Env(format!(
                "device-code polling failed: {status}: {body}"
            )));
        }

        if started.elapsed().unwrap_or_default() >= max_wait {
            return Err(Error::Env(
                "device-code login timed out after 15 minutes".to_string(),
            ));
        }

        tokio::time::sleep(Duration::from_secs(interval)).await;
    }
}

async fn exchange_authorization_code(
    client: &reqwest::Client,
    token_response: &DeviceTokenResponse,
) -> Result<OAuthExchangeResponse> {
    let response = client
        .post(format!("{CODEX_AUTH_ISSUER}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", token_response.authorization_code.as_str()),
            (
                "redirect_uri",
                &format!("{CODEX_AUTH_ISSUER}/deviceauth/callback"),
            ),
            ("client_id", CODEX_CLIENT_ID),
            ("code_verifier", token_response.code_verifier.as_str()),
        ])
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await?;
        return Err(Error::Env(format!(
            "OAuth token exchange failed: {status}: {body}"
        )));
    }

    Ok(response.json().await?)
}

fn ordered_options<'a>(
    options: &'a [AuthOption],
    preferred_option: Option<&str>,
) -> Vec<&'a AuthOption> {
    let mut ordered = Vec::new();
    if let Some(preferred_option) = preferred_option
        && let Some(option) = find_option(options, preferred_option)
    {
        ordered.push(option);
    }

    for option in options {
        if !ordered
            .iter()
            .any(|existing| existing.name() == option.name())
        {
            ordered.push(option);
        }
    }

    ordered
}

fn read_stored_credential(key: &str) -> Result<Option<StoredCredential>> {
    Ok(load_stored_credentials()?.credentials.remove(key))
}

fn write_stored_credential(key: &str, credential: StoredCredential) -> Result<()> {
    let mut credentials = load_stored_credentials()?;
    credentials.credentials.insert(key.to_string(), credential);
    save_stored_credentials(&credentials)
}

fn load_stored_credentials() -> Result<StoredCredentials> {
    let path = credentials_config_path()?;
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(StoredCredentials::default());
        }
        Err(error) => return Err(Error::Io(error)),
    };

    serde_json::from_str(&contents)
        .map_err(|error| Error::Env(format!("failed to parse {}: {error}", path.display())))
}

fn save_stored_credentials(credentials: &StoredCredentials) -> Result<()> {
    let path = credentials_config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let contents =
        serde_json::to_string_pretty(credentials).map_err(|error| Error::Env(error.to_string()))?;

    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path)?;
    file.write_all(contents.as_bytes())?;
    file.flush()?;
    Ok(())
}

fn token_needs_refresh(token: &str) -> bool {
    let Some(expires_at) = jwt_expiration(token) else {
        return false;
    };

    let now = unix_now();
    expires_at <= now + TOKEN_EXPIRY_SKEW_SECONDS
}

fn jwt_expiration(token: &str) -> Option<u64> {
    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    claims.get("exp")?.as_u64()
}

fn chatgpt_account_id(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    claims
        .get("https://api.openai.com/auth")?
        .get("chatgpt_account_id")?
        .as_str()
        .map(str::to_string)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    const API_KEY: AuthOption = AuthOption::ApiKey(ApiKeyCredential {
        env_var: "CAWIR_TEST_API_KEY",
        storage_key: "test-api-key",
        attachment: RequestAuth::Bearer,
    });

    const CODEX_OAUTH: AuthOption = AuthOption::CodexOAuth(CodexOAuthCredential {
        env_var: "CAWIR_TEST_CODEX_TOKEN",
        storage_key: "test-codex-oauth",
    });

    #[test]
    fn auth_option_exposes_lookup_metadata() {
        assert_eq!(API_KEY.name(), "api-key");
        assert_eq!(API_KEY.env_var(), Some("CAWIR_TEST_API_KEY"));
        assert_eq!(API_KEY.storage_key(), "test-api-key");
    }

    #[test]
    fn preferred_option_is_tried_first() {
        let ordered = ordered_options(&[API_KEY, CODEX_OAUTH], Some("codex-oauth"));

        assert_eq!(ordered[0].name(), "codex-oauth");
        assert_eq!(ordered[1].name(), "api-key");
    }

    #[test]
    fn project_config_dir_uses_the_directories_crate() {
        let path = project_config_dir().unwrap();

        assert!(path.to_string_lossy().contains("cawir"));
    }
}
