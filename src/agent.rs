use crate::{
    Error, Result,
    auth::ActiveCredential,
    events::{AgentEvent, StopReason},
    policy::PermissionMode,
    provider::{Provider, ProviderResponse},
    session::{Message, MessageContent},
    tools::{self, PlanReady, ToolApprovalRequest},
};

const MAX_TOOL_ROUNDS: usize = 42;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TurnOutcome {
    Complete,
    PlanReady(PlanReady),
}

pub(crate) struct TurnHooks<'a, E, A>
where
    E: FnMut(AgentEvent),
    A: FnMut(&ToolApprovalRequest) -> Result<bool>,
{
    pub(crate) emit: &'a mut E,
    pub(crate) approve: &'a mut A,
}

pub(crate) fn submit_user_prompt(
    prompt: &str,
    history: &mut Vec<Message>,
    emit: &mut impl FnMut(AgentEvent),
) {
    emit(AgentEvent::UserPromptSubmit {
        prompt: prompt.to_string(),
    });
    history.push(Message::user_text(prompt));
}

pub(crate) async fn run_turn<P, E, A>(
    provider: &P,
    client: &reqwest::Client,
    credential: &ActiveCredential,
    model: &str,
    mode: PermissionMode,
    history: &mut Vec<Message>,
    hooks: &mut TurnHooks<'_, E, A>,
) -> Result<TurnOutcome>
where
    P: Provider,
    E: FnMut(AgentEvent),
    A: FnMut(&ToolApprovalRequest) -> Result<bool>,
{
    let mut tool_rounds = 0;

    loop {
        (hooks.emit)(AgentEvent::ModelRequestStart {
            provider: provider.name().to_string(),
            model: model.to_string(),
        });

        let response = match provider
            .send(client, credential, model, history, tools::definitions(mode))
            .await
        {
            Ok(response) => response,
            Err(error) => {
                (hooks.emit)(AgentEvent::StopFailure {
                    message: error.to_string(),
                });
                return Err(error);
            }
        };

        match response {
            ProviderResponse::Text(reply) => {
                (hooks.emit)(AgentEvent::AssistantText {
                    provider: provider.name().to_string(),
                    text: reply.clone(),
                });
                history.push(Message::assistant(vec![MessageContent::Text {
                    text: reply.clone(),
                }]));
                if mode == PermissionMode::Plan {
                    (hooks.emit)(AgentEvent::Stop {
                        reason: StopReason::PlanReady,
                    });
                    return Ok(TurnOutcome::PlanReady(PlanReady {
                        tool_use_id: None,
                        plan: reply,
                    }));
                }
                (hooks.emit)(AgentEvent::Stop {
                    reason: StopReason::Complete,
                });
                return Ok(TurnOutcome::Complete);
            }
            ProviderResponse::ToolUse(blocks) => {
                tool_rounds += 1;
                if tool_rounds > MAX_TOOL_ROUNDS {
                    let error = Error::ToolLoopLimitExceeded(MAX_TOOL_ROUNDS);
                    (hooks.emit)(AgentEvent::StopFailure {
                        message: error.to_string(),
                    });
                    return Err(error);
                }

                for block in &blocks {
                    if let MessageContent::Text { text } = block {
                        (hooks.emit)(AgentEvent::AssistantText {
                            provider: provider.name().to_string(),
                            text: text.clone(),
                        });
                    }
                }

                let tool_execution = tools::execute_tool_uses_with_approval(
                    &blocks,
                    mode,
                    &mut *hooks.emit,
                    &mut *hooks.approve,
                );
                if tool_execution.results.is_empty() && tool_execution.plan_ready.is_none() {
                    let error = Error::EmptyContent(provider.name().to_string());
                    (hooks.emit)(AgentEvent::StopFailure {
                        message: error.to_string(),
                    });
                    return Err(error);
                }

                history.push(Message::assistant(blocks));
                if let Some(plan_ready) = tool_execution.plan_ready {
                    (hooks.emit)(AgentEvent::Stop {
                        reason: StopReason::PlanReady,
                    });
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
