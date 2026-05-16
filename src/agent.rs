use std::path::Path;

use crate::{
    Error, Result,
    auth::ActiveCredential,
    events::{AgentEvent, FailureKind, StopReason},
    hooks::HookRegistry,
    policy::PermissionMode,
    prompt,
    provider::{Provider, ProviderResponse},
    session::{Message, MessageContent},
    tools::{self, PlanReady, ToolApprovalRequest, ToolRegistry},
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

pub(crate) struct TurnContext<'a, P>
where
    P: Provider,
{
    pub(crate) provider: &'a P,
    pub(crate) client: &'a reqwest::Client,
    pub(crate) credential: &'a ActiveCredential,
    pub(crate) model: &'a str,
    pub(crate) project_root: &'a Path,
    pub(crate) mode: PermissionMode,
    pub(crate) tool_registry: &'a ToolRegistry,
    pub(crate) hook_registry: &'a HookRegistry,
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
    context: TurnContext<'_, P>,
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
            provider: context.provider.name().to_string(),
            model: context.model.to_string(),
        });

        let prompt = match prompt::assemble(context.project_root) {
            Ok(prompt) => prompt,
            Err(error) => {
                (hooks.emit)(AgentEvent::stop_failure(
                    FailureKind::PromptAssembly,
                    &error,
                ));
                return Err(error);
            }
        };

        let response = match context
            .provider
            .send(
                context.client,
                context.credential,
                context.model,
                &prompt,
                history,
                context.tool_registry.definitions(context.mode),
            )
            .await
        {
            Ok(response) => response,
            Err(error) => {
                (hooks.emit)(AgentEvent::stop_failure(
                    FailureKind::ProviderRequest,
                    &error,
                ));
                return Err(error);
            }
        };
        let metadata = response.metadata().clone();
        (hooks.emit)(AgentEvent::ModelRequestFinish {
            provider: context.provider.name().to_string(),
            model: context.model.to_string(),
            metadata,
        });

        match response {
            ProviderResponse::Text { text: reply, .. } => {
                (hooks.emit)(AgentEvent::AssistantText {
                    provider: context.provider.name().to_string(),
                    text: reply.clone(),
                });
                history.push(Message::assistant(vec![MessageContent::Text {
                    text: reply.clone(),
                }]));
                if context.mode == PermissionMode::Plan {
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
            ProviderResponse::ToolUse { blocks, .. } => {
                tool_rounds += 1;
                if tool_rounds > MAX_TOOL_ROUNDS {
                    let error = Error::ToolLoopLimitExceeded(MAX_TOOL_ROUNDS);
                    (hooks.emit)(AgentEvent::stop_failure(FailureKind::ToolLoopLimit, &error));
                    return Err(error);
                }

                for block in &blocks {
                    if let MessageContent::Text { text } = block {
                        (hooks.emit)(AgentEvent::AssistantText {
                            provider: context.provider.name().to_string(),
                            text: text.clone(),
                        });
                    }
                }

                let tool_execution = tools::execute_tool_uses_with_approval(
                    context.tool_registry,
                    context.hook_registry,
                    context.project_root,
                    &blocks,
                    context.mode,
                    &mut *hooks.emit,
                    &mut *hooks.approve,
                );
                if tool_execution.results.is_empty() && tool_execution.plan_ready.is_none() {
                    let error = Error::EmptyContent(context.provider.name().to_string());
                    (hooks.emit)(AgentEvent::stop_failure(FailureKind::EmptyContent, &error));
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
