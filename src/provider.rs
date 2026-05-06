use serde::Serialize;

use crate::{Result, auth::AuthOption, session::Message};

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

    async fn send(
        &self,
        client: &reqwest::Client,
        credential: &crate::auth::ActiveCredential,
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

        let provider = std::env::var("PROVIDER")
            .map_err(|_| Error::Env("set PROVIDER=anthropic|openai|ollama".to_string()))?;
        let auth_option = std::env::var("AUTH_OPTION")
            .map_err(|_| Error::Env("set AUTH_OPTION=api-key|codex-oauth|none".to_string()))?;
        let client = reqwest::Client::builder()
            .user_agent("cawir-live-test/0.1")
            .build()?;
        let messages = vec![Message::user_text(
            "Reply with a short sentence containing the text cawir-live-ok.",
        )];

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
                    .send(&client, &credential, &messages, Vec::new())
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
                    .send(&client, &credential, &messages, Vec::new())
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
                    .send(&client, &credential, &messages, Vec::new())
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
