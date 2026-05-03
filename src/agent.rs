use crate::{
    Error, Result,
    anthropic::{ClaudeResponse, ask_claude},
    session::{Message, MessageContent},
    tools,
};

const MAX_TOOL_ROUNDS: usize = 42;

pub(crate) async fn run_turn(
    client: &reqwest::Client,
    api_key: &str,
    history: &mut Vec<Message>,
) -> Result<()> {
    let mut tool_rounds = 0;

    loop {
        match ask_claude(client, api_key, history, tools::definitions()).await? {
            ClaudeResponse::Text(reply) => {
                println!("claude: {}", reply);
                history.push(Message::assistant(vec![MessageContent::Text {
                    text: reply,
                }]));
                return Ok(());
            }
            ClaudeResponse::ToolUse(blocks) => {
                tool_rounds += 1;
                if tool_rounds > MAX_TOOL_ROUNDS {
                    return Err(Error::ToolLoopLimitExceeded(MAX_TOOL_ROUNDS));
                }

                let tool_results = tools::execute_tool_uses(&blocks);
                if tool_results.is_empty() {
                    return Err(Error::EmptyContent);
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
