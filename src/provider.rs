use reqwest::{
    StatusCode,
    header::{HeaderMap, RETRY_AFTER},
};
use serde::{Deserialize, Serialize};

use crate::{
    Error, Result,
    auth::{ActiveCredential, AuthOption},
    error::RetryAfter,
    prompt::SystemPrompt,
    session::{Message, MessageContent},
};

#[derive(Serialize, Clone)]
pub(crate) struct ToolDefinition {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_schema: serde_json::Value,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ProviderMetadata {
    pub(crate) usage: Option<TokenUsage>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct TokenUsage {
    pub(crate) input_tokens: Option<u64>,
    pub(crate) output_tokens: Option<u64>,
    pub(crate) cache_creation_input_tokens: Option<u64>,
    pub(crate) cache_read_input_tokens: Option<u64>,
}

impl ProviderMetadata {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    pub(crate) fn from_usage(usage: TokenUsage) -> Self {
        if usage.is_empty() {
            Self::empty()
        } else {
            Self { usage: Some(usage) }
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.usage.is_none()
    }
}

impl TokenUsage {
    pub(crate) fn is_empty(&self) -> bool {
        self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.cache_creation_input_tokens.is_none()
            && self.cache_read_input_tokens.is_none()
    }
}

pub(crate) enum ProviderResponse {
    Text {
        text: String,
        metadata: ProviderMetadata,
    },
    ToolUse {
        blocks: Vec<MessageContent>,
        metadata: ProviderMetadata,
    },
}

impl ProviderResponse {
    pub(crate) fn text(text: String, metadata: ProviderMetadata) -> Self {
        Self::Text { text, metadata }
    }

    pub(crate) fn tool_use(blocks: Vec<MessageContent>, metadata: ProviderMetadata) -> Self {
        Self::ToolUse { blocks, metadata }
    }

    pub(crate) fn metadata(&self) -> &ProviderMetadata {
        match self {
            Self::Text { metadata, .. } | Self::ToolUse { metadata, .. } => metadata,
        }
    }
}

pub(crate) trait Provider {
    fn name(&self) -> &'static str;

    fn auth_options(&self) -> &'static [AuthOption];

    fn default_model(&self, credential: &ActiveCredential) -> &'static str;

    async fn available_models(
        &self,
        client: &reqwest::Client,
        credential: &ActiveCredential,
    ) -> Result<Vec<String>>;

    fn fallback_models(&self, credential: &ActiveCredential) -> &'static [&'static str];

    async fn send(
        &self,
        client: &reqwest::Client,
        credential: &ActiveCredential,
        model: &str,
        prompt: &SystemPrompt,
        messages: &[Message],
        tools: Vec<ToolDefinition>,
    ) -> Result<ProviderResponse>;
}

pub(crate) async fn api_error_from_response(provider: &str, response: reqwest::Response) -> Error {
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .text()
        .await
        .unwrap_or_else(|error| format!("failed to read error body: {error}"));

    api_error_for_status(provider, status, &headers, body)
}

pub(crate) fn api_error_for_status(
    provider: &str,
    status: StatusCode,
    headers: &HeaderMap,
    body: String,
) -> Error {
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Error::RateLimited {
            provider: provider.to_string(),
            status,
            retry_after: retry_after_from_headers(headers),
            body,
        };
    }

    Error::Api {
        provider: provider.to_string(),
        status,
        body,
    }
}

fn retry_after_from_headers(headers: &HeaderMap) -> RetryAfter {
    let Some(raw) = headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    else {
        return RetryAfter::default();
    };
    let seconds = raw.parse::<u64>().ok();

    RetryAfter {
        raw: Some(raw),
        seconds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Error, anthropic::Anthropic, auth::resolve_for_provider, ollama::Ollama, openai::OpenAi,
        session::Message,
    };
    use reqwest::{
        StatusCode,
        header::{HeaderMap, HeaderValue, RETRY_AFTER},
    };

    #[test]
    fn rate_limit_error_includes_retry_after_seconds() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("17"));

        let error = api_error_for_status(
            "anthropic",
            StatusCode::TOO_MANY_REQUESTS,
            &headers,
            "slow down".to_string(),
        );

        match &error {
            Error::RateLimited {
                provider,
                status,
                retry_after,
                body,
            } => {
                assert_eq!(provider, "anthropic");
                assert_eq!(*status, StatusCode::TOO_MANY_REQUESTS);
                assert_eq!(retry_after.raw.as_deref(), Some("17"));
                assert_eq!(retry_after.seconds, Some(17));
                assert_eq!(body, "slow down");
            }
            other => panic!("expected rate-limit error, got {other:?}"),
        }
        assert_eq!(
            error.to_string(),
            "anthropic rate limited 429 Too Many Requests; retry after 17s: slow down"
        );
    }

    #[test]
    fn non_rate_limit_status_stays_generic_api_error() {
        let error = api_error_for_status(
            "openai",
            StatusCode::BAD_REQUEST,
            &HeaderMap::new(),
            "bad request".to_string(),
        );

        match error {
            Error::Api {
                provider,
                status,
                body,
            } => {
                assert_eq!(provider, "openai");
                assert_eq!(status, StatusCode::BAD_REQUEST);
                assert_eq!(body, "bad request");
            }
            other => panic!("expected generic api error, got {other:?}"),
        }
    }

    #[tokio::test]
    #[ignore = "uses real provider credentials and network"]
    async fn live_smoke() -> Result<()> {
        let _ = dotenvy::dotenv();

        let (provider, auth_option) = live_route_from_env()?;
        let client = reqwest::Client::builder()
            .user_agent("cawir-live-test/0.1")
            .build()?;
        let messages = live_smoke_messages();
        let prompt = crate::prompt::assemble(&std::env::current_dir()?)?;

        let response = match provider.as_str() {
            "anthropic" => {
                let provider = Anthropic;
                let credential = resolve_for_provider(
                    provider.name(),
                    provider.auth_options(),
                    Some(&auth_option),
                    &client,
                )
                .await?;
                provider
                    .send(
                        &client,
                        &credential,
                        provider.default_model(&credential),
                        &prompt,
                        &messages,
                        Vec::new(),
                    )
                    .await?
            }
            "openai" => {
                let provider = OpenAi;
                let credential = resolve_for_provider(
                    provider.name(),
                    provider.auth_options(),
                    Some(&auth_option),
                    &client,
                )
                .await?;
                provider
                    .send(
                        &client,
                        &credential,
                        provider.default_model(&credential),
                        &prompt,
                        &messages,
                        Vec::new(),
                    )
                    .await?
            }
            "ollama" => {
                let provider = Ollama;
                let credential = resolve_for_provider(
                    provider.name(),
                    provider.auth_options(),
                    Some(&auth_option),
                    &client,
                )
                .await?;
                provider
                    .send(
                        &client,
                        &credential,
                        provider.default_model(&credential),
                        &prompt,
                        &messages,
                        Vec::new(),
                    )
                    .await?
            }
            other => {
                return Err(Error::Env(format!(
                    "unknown PROVIDER={other}; expected anthropic, openai, or ollama"
                )));
            }
        };

        assert_live_response(response);
        Ok(())
    }

    #[tokio::test]
    #[ignore = "uses real provider credentials, network, and provider quota"]
    async fn live_models_and_switch() -> Result<()> {
        let _ = dotenvy::dotenv();

        let (provider, auth_option) = live_route_from_env()?;
        let client = reqwest::Client::builder()
            .user_agent("cawir-live-test/0.1")
            .build()?;
        let messages = live_smoke_messages();
        let prompt = crate::prompt::assemble(&std::env::current_dir()?)?;

        let response = match provider.as_str() {
            "anthropic" => {
                let provider = Anthropic;
                let credential = resolve_for_provider(
                    provider.name(),
                    provider.auth_options(),
                    Some(&auth_option),
                    &client,
                )
                .await?;
                let model = selected_live_model(&provider, &client, &credential).await?;
                provider
                    .send(&client, &credential, &model, &prompt, &messages, Vec::new())
                    .await?
            }
            "openai" => {
                let provider = OpenAi;
                let credential = resolve_for_provider(
                    provider.name(),
                    provider.auth_options(),
                    Some(&auth_option),
                    &client,
                )
                .await?;
                let model = selected_live_model(&provider, &client, &credential).await?;
                provider
                    .send(&client, &credential, &model, &prompt, &messages, Vec::new())
                    .await?
            }
            "ollama" => {
                let provider = Ollama;
                let credential = resolve_for_provider(
                    provider.name(),
                    provider.auth_options(),
                    Some(&auth_option),
                    &client,
                )
                .await?;
                let model = selected_live_model(&provider, &client, &credential).await?;
                provider
                    .send(&client, &credential, &model, &prompt, &messages, Vec::new())
                    .await?
            }
            other => {
                return Err(Error::Env(format!(
                    "unknown PROVIDER={other}; expected anthropic, openai, or ollama"
                )));
            }
        };

        assert_live_response(response);
        Ok(())
    }

    fn live_route_from_env() -> Result<(String, String)> {
        let provider = std::env::var("PROVIDER")
            .map_err(|_| Error::Env("set PROVIDER=anthropic|openai|ollama".to_string()))?;
        let auth_option = std::env::var("AUTH_OPTION")
            .map_err(|_| Error::Env("set AUTH_OPTION=api-key|codex-oauth|none".to_string()))?;
        Ok((provider, auth_option))
    }

    fn live_smoke_messages() -> Vec<Message> {
        vec![Message::user_text(
            "Reply with a short sentence containing the text cawir-live-ok.",
        )]
    }

    async fn selected_live_model<P: Provider>(
        provider: &P,
        client: &reqwest::Client,
        credential: &ActiveCredential,
    ) -> Result<String> {
        let models = provider.available_models(client, credential).await?;
        assert!(!models.is_empty(), "{} returned no models", provider.name());

        let default = provider.default_model(credential);
        Ok(models
            .iter()
            .find(|model| model.as_str() == default)
            .cloned()
            .unwrap_or_else(|| models[0].clone()))
    }

    fn assert_live_response(response: ProviderResponse) {
        match response {
            ProviderResponse::Text { text, .. } => {
                assert!(
                    !text.trim().is_empty(),
                    "provider returned an empty text response"
                );
            }
            ProviderResponse::ToolUse { blocks, .. } => {
                assert!(
                    !blocks.is_empty(),
                    "provider returned empty tool-use blocks"
                );
            }
        }
    }
}
