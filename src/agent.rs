use crate::{
    Error, Result,
    auth::ActiveCredential,
    events::{AgentEvent, StopReason},
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

pub(crate) async fn run_turn<P: Provider>(
    provider: &P,
    client: &reqwest::Client,
    credential: &ActiveCredential,
    model: &str,
    mode: PermissionMode,
    history: &mut Vec<Message>,
    emit: &mut impl FnMut(AgentEvent),
) -> Result<TurnOutcome> {
    let mut tool_rounds = 0;

    loop {
        emit(AgentEvent::ModelRequestStart {
            provider: provider.name().to_string(),
            model: model.to_string(),
        });

        let response = match provider
            .send(client, credential, model, history, tools::definitions(mode))
            .await
        {
            Ok(response) => response,
            Err(error) => {
                emit(AgentEvent::StopFailure {
                    message: error.to_string(),
                });
                return Err(error);
            }
        };

        match response {
            ProviderResponse::Text(reply) => {
                emit(AgentEvent::AssistantText {
                    provider: provider.name().to_string(),
                    text: reply.clone(),
                });
                history.push(Message::assistant(vec![MessageContent::Text {
                    text: reply.clone(),
                }]));
                if mode == PermissionMode::Plan {
                    emit(AgentEvent::Stop {
                        reason: StopReason::PlanReady,
                    });
                    return Ok(TurnOutcome::PlanReady(PlanReady {
                        tool_use_id: None,
                        plan: reply,
                    }));
                }
                emit(AgentEvent::Stop {
                    reason: StopReason::Complete,
                });
                return Ok(TurnOutcome::Complete);
            }
            ProviderResponse::ToolUse(blocks) => {
                tool_rounds += 1;
                if tool_rounds > MAX_TOOL_ROUNDS {
                    let error = Error::ToolLoopLimitExceeded(MAX_TOOL_ROUNDS);
                    emit(AgentEvent::StopFailure {
                        message: error.to_string(),
                    });
                    return Err(error);
                }

                for block in &blocks {
                    if let MessageContent::Text { text } = block {
                        emit(AgentEvent::AssistantText {
                            provider: provider.name().to_string(),
                            text: text.clone(),
                        });
                    }
                }

                let tool_execution = tools::execute_tool_uses(&blocks, mode, &mut *emit);
                if tool_execution.results.is_empty() && tool_execution.plan_ready.is_none() {
                    let error = Error::EmptyContent(provider.name().to_string());
                    emit(AgentEvent::StopFailure {
                        message: error.to_string(),
                    });
                    return Err(error);
                }

                history.push(Message::assistant(blocks));
                if let Some(plan_ready) = tool_execution.plan_ready {
                    emit(AgentEvent::Stop {
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
