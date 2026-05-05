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
