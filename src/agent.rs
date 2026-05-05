use crate::{
    Error, Result,
    auth::ActiveCredential,
    provider::{Provider, ProviderResponse},
    session::{Message, MessageContent},
    tools,
};

const MAX_TOOL_ROUNDS: usize = 42;

pub(crate) async fn run_turn<P: Provider>(
    provider: &P,
    client: &reqwest::Client,
    credential: &ActiveCredential,
    history: &mut Vec<Message>,
) -> Result<()> {
    let mut tool_rounds = 0;

    loop {
        match provider
            .send(client, credential, history, tools::definitions())
            .await?
        {
            ProviderResponse::Text(reply) => {
                println!("{}: {}", provider.name(), reply);
                history.push(Message::assistant(vec![MessageContent::Text {
                    text: reply,
                }]));
                return Ok(());
            }
            ProviderResponse::ToolUse(blocks) => {
                tool_rounds += 1;
                if tool_rounds > MAX_TOOL_ROUNDS {
                    return Err(Error::ToolLoopLimitExceeded(MAX_TOOL_ROUNDS));
                }

                let tool_results = tools::execute_tool_uses(&blocks);
                if tool_results.is_empty() {
                    return Err(Error::EmptyContent(provider.name().to_string()));
                }

                history.push(Message::assistant(blocks));
                history.push(Message::user_tool_results(tool_results));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_loop_limit_error_names_the_limit() {
        let error = Error::ToolLoopLimitExceeded(MAX_TOOL_ROUNDS);

        assert_eq!(error.to_string(), "tool loop exceeded 42 rounds");
    }
}
