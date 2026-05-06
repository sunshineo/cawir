use crate::{
    Error, Result,
    auth::ActiveCredential,
    policy::PermissionMode,
    provider::{Provider, ProviderResponse},
    session::{Message, MessageContent},
    tools::{self, PlanReady},
};

const MAX_TOOL_ROUNDS: usize = 42;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TurnOutcome {
    Complete,
    PlanReady(PlanReady),
}

pub(crate) async fn run_turn<P: Provider>(
    provider: &P,
    client: &reqwest::Client,
    credential: &ActiveCredential,
    model: &str,
    mode: PermissionMode,
    history: &mut Vec<Message>,
) -> Result<TurnOutcome> {
    let mut tool_rounds = 0;

    loop {
        match provider
            .send(client, credential, model, history, tools::definitions(mode))
            .await?
        {
            ProviderResponse::Text(reply) => {
                println!("{}: {}", provider.name(), reply);
                history.push(Message::assistant(vec![MessageContent::Text {
                    text: reply.clone(),
                }]));
                if mode == PermissionMode::Plan {
                    return Ok(TurnOutcome::PlanReady(PlanReady {
                        tool_use_id: None,
                        plan: reply,
                    }));
                }
                return Ok(TurnOutcome::Complete);
            }
            ProviderResponse::ToolUse(blocks) => {
                tool_rounds += 1;
                if tool_rounds > MAX_TOOL_ROUNDS {
                    return Err(Error::ToolLoopLimitExceeded(MAX_TOOL_ROUNDS));
                }

                let tool_execution = tools::execute_tool_uses(&blocks, mode);
                if tool_execution.results.is_empty() && tool_execution.plan_ready.is_none() {
                    return Err(Error::EmptyContent(provider.name().to_string()));
                }

                history.push(Message::assistant(blocks));
                if let Some(plan_ready) = tool_execution.plan_ready {
                    return Ok(TurnOutcome::PlanReady(plan_ready));
                }
                history.push(Message::user_tool_results(tool_execution.results));
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
