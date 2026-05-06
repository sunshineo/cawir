use serde::Serialize;

use crate::{
    Result,
    auth::{ActiveCredential, AuthOption},
    session::Message,
};

#[derive(Serialize, Clone)]
pub(crate) struct ToolDefinition {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_schema: serde_json::Value,
}

pub(crate) enum ProviderResponse {
    Text(String),
    ToolUse(Vec<crate::session::MessageContent>),
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
        messages: &[Message],
        tools: Vec<ToolDefinition>,
    ) -> Result<ProviderResponse>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Error, anthropic::Anthropic, auth::resolve_for_provider, ollama::Ollama, openai::OpenAi,
        session::Message,
    };

    #[tokio::test]
    #[ignore = "uses real provider credentials and network"]
    async fn live_smoke() -> Result<()> {
        let _ = dotenvy::dotenv();

        let (provider, auth_option) = live_route_from_env()?;
        let client = reqwest::Client::builder()
            .user_agent("cawir-live-test/0.1")
            .build()?;
        let messages = live_smoke_messages();

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
                    .send(&client, &credential, &model, &messages, Vec::new())
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
                    .send(&client, &credential, &model, &messages, Vec::new())
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
                    .send(&client, &credential, &model, &messages, Vec::new())
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
            ProviderResponse::Text(text) => {
                assert!(
                    !text.trim().is_empty(),
                    "provider returned an empty text response"
                );
            }
            ProviderResponse::ToolUse(blocks) => {
                assert!(
                    !blocks.is_empty(),
                    "provider returned empty tool-use blocks"
                );
            }
        }
    }
}
