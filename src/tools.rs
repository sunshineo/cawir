use std::{
    io::{self, Write},
    process::Command,
};

use serde_json::{Value, json};

use crate::{
    Error, Result,
    policy::{PermissionDecision, PermissionMode, ToolKind, permission_decision},
    provider::ToolDefinition,
    session::{MessageContent, ToolResult},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlanReady {
    pub(crate) tool_use_id: Option<String>,
    pub(crate) plan: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ToolExecution {
    pub(crate) results: Vec<ToolResult>,
    pub(crate) plan_ready: Option<PlanReady>,
}

pub(crate) fn definitions(mode: PermissionMode) -> Vec<ToolDefinition> {
    let mut definitions = vec![
        list_files_tool(),
        read_file_tool(),
        write_file_tool(),
        shell_tool(),
    ];

    if mode == PermissionMode::Plan {
        definitions.push(exit_plan_mode_tool());
    }

    definitions
}

fn read_file_tool() -> ToolDefinition {
    ToolDefinition {
        name: "read_file".to_string(),
        description: "Read the full contents of a UTF-8 text file from the current project. Use this when you need to inspect source code, configuration, documentation, or other text files before answering. Provide a path relative to the current working directory when possible. Do not use this for files outside the project unless the user clearly asks for them.".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read, preferably relative to the current working directory."
                }
            },
            "required": ["path"],
            "additionalProperties": false
        }),
    }
}

fn list_files_tool() -> ToolDefinition {
    ToolDefinition {
        name: "list_files".to_string(),
        description: "List the files and directories inside a folder from the current project. Use this before read_file when you need to discover repository structure or find likely files to inspect. Provide a path relative to the current working directory when possible.".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to list, preferably relative to the current working directory."
                }
            },
            "required": ["path"],
            "additionalProperties": false
        }),
    }
}

fn write_file_tool() -> ToolDefinition {
    ToolDefinition {
        name: "write_file".to_string(),
        description: "Write UTF-8 text content to a file in the current project. Use this only when the user asks you to create or replace a file. The write will require explicit user approval before it runs.".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write, preferably relative to the current working directory."
                },
                "content": {
                    "type": "string",
                    "description": "The complete UTF-8 text content to write to the file."
                }
            },
            "required": ["path", "content"],
            "additionalProperties": false
        }),
    }
}

fn shell_tool() -> ToolDefinition {
    ToolDefinition {
        name: "shell".to_string(),
        description: "Run a shell command in the current project. Do not use shell for directory listings, file reads, or file writes; use the dedicated list_files, read_file, and write_file tools for those. Use shell for commands that need a process, such as tests, formatters, builds, git commands, or search commands. The command will require explicit user approval before it runs.".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to run from the current working directory."
                }
            },
            "required": ["command"],
            "additionalProperties": false
        }),
    }
}

fn exit_plan_mode_tool() -> ToolDefinition {
    ToolDefinition {
        name: "exit_plan_mode".to_string(),
        description: "Submit a concise implementation plan for user approval. Use this when you are in plan mode and ready to ask permission to make changes.".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "plan": {
                    "type": "string",
                    "description": "The proposed plan to show to the user before making changes."
                }
            },
            "required": ["plan"],
            "additionalProperties": false
        }),
    }
}

pub(crate) fn execute_tool_uses(blocks: &[MessageContent], mode: PermissionMode) -> ToolExecution {
    let mut results = Vec::new();
    let mut plan_ready = None;

    for block in blocks {
        match block {
            MessageContent::Text { text } => {
                println!("claude: {}", text);
            }
            MessageContent::ToolUse { id, name, input } => {
                println!("tool request: {} ({})", name, id);
                if name == "exit_plan_mode" {
                    match plan_from_exit_plan_mode(input) {
                        Ok(plan) => {
                            println!("plan ready for approval");
                            plan_ready = Some(PlanReady {
                                tool_use_id: Some(id.clone()),
                                plan,
                            });
                            continue;
                        }
                        Err(error) => {
                            let content = error.to_string();
                            print_tool_error(name, &content);
                            results.push(ToolResult {
                                tool_use_id: id.clone(),
                                content,
                                is_error: true,
                            });
                            continue;
                        }
                    }
                }

                let result = match execute_tool_call(name, input, mode) {
                    Ok(content) => {
                        print_tool_result(name, &content);
                        ToolResult {
                            tool_use_id: id.clone(),
                            content,
                            is_error: false,
                        }
                    }
                    Err(error) => {
                        let content = error.to_string();
                        print_tool_error(name, &content);
                        ToolResult {
                            tool_use_id: id.clone(),
                            content,
                            is_error: true,
                        }
                    }
                };
                results.push(result);
            }
            MessageContent::ToolResult { .. } => {}
        }
    }

    ToolExecution {
        results,
        plan_ready,
    }
}

fn execute_tool_call(name: &str, input: &Value, mode: PermissionMode) -> Result<String> {
    match name {
        "list_files" => {
            let _decision = permission_decision(mode, ToolKind::ReadOnly);
            execute_list_files(input)
        }
        "read_file" => {
            let _decision = permission_decision(mode, ToolKind::ReadOnly);
            execute_read_file(input)
        }
        "write_file" => execute_write_file(input, mode),
        "shell" => execute_shell(input, mode),
        _ => Err(Error::UnknownTool(name.to_string())),
    }
}

fn execute_list_files(input: &Value) -> Result<String> {
    let path = input
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::ToolInput {
            tool: "list_files".to_string(),
            message: "expected input.path to be a string".to_string(),
        })?;

    let mut entries = std::fs::read_dir(path)?
        .map(|entry| -> Result<String> {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let mut name = entry.file_name().to_string_lossy().into_owned();

            if file_type.is_dir() {
                name.push('/');
            }

            Ok(name)
        })
        .collect::<Result<Vec<_>>>()?;

    entries.sort();
    Ok(entries.join("\n"))
}

fn execute_read_file(input: &Value) -> Result<String> {
    let path = input
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::ToolInput {
            tool: "read_file".to_string(),
            message: "expected input.path to be a string".to_string(),
        })?;

    Ok(std::fs::read_to_string(path)?)
}

fn execute_write_file(input: &Value, mode: PermissionMode) -> Result<String> {
    execute_write_file_with_policy(input, mode, approve_write_interactively)
}

#[cfg(test)]
fn execute_write_file_with_approval<F>(input: &Value, approve: F) -> Result<String>
where
    F: FnMut(&str, &str) -> Result<bool>,
{
    execute_write_file_with_policy(input, PermissionMode::Default, approve)
}

fn execute_write_file_with_policy<F>(
    input: &Value,
    mode: PermissionMode,
    mut approve: F,
) -> Result<String>
where
    F: FnMut(&str, &str) -> Result<bool>,
{
    let path = input
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::ToolInput {
            tool: "write_file".to_string(),
            message: "expected input.path to be a string".to_string(),
        })?;

    let content = input
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::ToolInput {
            tool: "write_file".to_string(),
            message: format!(
                "expected input.content to be a string; received input: {}",
                input
            ),
        })?;

    match permission_decision(mode, ToolKind::WriteFile) {
        PermissionDecision::Allow => {}
        PermissionDecision::AskUser => {
            if !approve(path, content)? {
                return Err(Error::ToolDenied {
                    tool: "write_file".to_string(),
                    message: format!("user denied write to {}", path),
                });
            }
        }
        PermissionDecision::Deny(reason) => {
            return Err(Error::ToolDenied {
                tool: "write_file".to_string(),
                message: reason.to_string(),
            });
        }
    }

    std::fs::write(path, content)?;

    Ok(format!("wrote {} bytes to {}", content.len(), path))
}

fn approve_write_interactively(path: &str, content: &str) -> Result<bool> {
    println!(
        "write_file wants to write {} bytes to {}",
        content.len(),
        path
    );
    print!("approve write? [y/N] ");
    io::stdout().flush()?;

    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;

    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES"))
}

fn execute_shell(input: &Value, mode: PermissionMode) -> Result<String> {
    execute_shell_with_policy(input, mode, approve_shell_interactively)
}

#[cfg(test)]
fn execute_shell_with_approval<F>(input: &Value, approve: F) -> Result<String>
where
    F: FnMut(&str) -> Result<bool>,
{
    execute_shell_with_policy(input, PermissionMode::Default, approve)
}

fn execute_shell_with_policy<F>(
    input: &Value,
    mode: PermissionMode,
    mut approve: F,
) -> Result<String>
where
    F: FnMut(&str) -> Result<bool>,
{
    let command = input
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::ToolInput {
            tool: "shell".to_string(),
            message: "expected input.command to be a string".to_string(),
        })?;

    if let Some(reason) = catastrophic_shell_denial(command) {
        return Err(Error::ToolDenied {
            tool: "shell".to_string(),
            message: reason.to_string(),
        });
    }

    match permission_decision(mode, ToolKind::Shell) {
        PermissionDecision::Allow => {}
        PermissionDecision::AskUser => {
            if !approve(command)? {
                return Err(Error::ToolDenied {
                    tool: "shell".to_string(),
                    message: format!("user denied shell command: {}", command),
                });
            }
        }
        PermissionDecision::Deny(reason) => {
            return Err(Error::ToolDenied {
                tool: "shell".to_string(),
                message: reason.to_string(),
            });
        }
    }

    let output = Command::new("sh").arg("-c").arg(command).output()?;

    Ok(format_shell_output(&output))
}

fn plan_from_exit_plan_mode(input: &Value) -> Result<String> {
    let _decision = permission_decision(PermissionMode::Plan, ToolKind::ExitPlanMode);

    input
        .get("plan")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::ToolInput {
            tool: "exit_plan_mode".to_string(),
            message: "expected input.plan to be a string".to_string(),
        })
}

fn catastrophic_shell_denial(command: &str) -> Option<&'static str> {
    let normalized = command.split_whitespace().collect::<Vec<_>>().join(" ");
    let dangerous = [
        "rm -rf /",
        "rm -fr /",
        "rm -Rf /",
        "rm -fR /",
        "rm -rf /*",
        "rm -fr /*",
        "rm -rf ~",
        "rm -fr ~",
        "rm -rf $HOME",
        "rm -fr $HOME",
        "chmod -R 777 /",
        "chown -R",
        "mkfs",
        "dd if=",
        "shutdown",
        "reboot",
    ];

    dangerous
        .iter()
        .any(|pattern| normalized.starts_with(pattern))
        .then_some("refusing catastrophic shell command")
}

fn approve_shell_interactively(command: &str) -> Result<bool> {
    println!("shell wants to run: {}", command);
    print!("approve command? [y/N] ");
    io::stdout().flush()?;

    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;

    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES"))
}

fn format_shell_output(output: &std::process::Output) -> String {
    let status = match output.status.code() {
        Some(code) => format!("exit status: {}", code),
        None => "exit status: terminated by signal".to_string(),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    format!("{}\nstdout:\n{}\nstderr:\n{}", status, stdout, stderr)
}

fn print_tool_result(name: &str, result: &str) {
    println!("tool result from {}: {} bytes", name, result.len());
}

fn print_tool_error(name: &str, error: &str) {
    println!("tool error from {}: {}", name, error);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn tool_execution_returns_results_for_all_tool_uses() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cawir-multi-tool-test-{}-{}",
            std::process::id(),
            unique
        ));

        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("first.txt"), "first result").unwrap();

        let blocks = vec![
            MessageContent::Text {
                text: "I'll inspect one file.".to_string(),
            },
            MessageContent::ToolUse {
                id: "toolu_first".to_string(),
                name: "read_file".to_string(),
                input: json!({ "path": path.join("first.txt").to_string_lossy() }),
            },
            MessageContent::ToolUse {
                id: "toolu_second".to_string(),
                name: "list_files".to_string(),
                input: json!({ "path": path.to_string_lossy() }),
            },
        ];

        let execution = execute_tool_uses(&blocks, PermissionMode::Default);

        assert_eq!(
            execution.results,
            vec![
                ToolResult {
                    tool_use_id: "toolu_first".to_string(),
                    content: "first result".to_string(),
                    is_error: false,
                },
                ToolResult {
                    tool_use_id: "toolu_second".to_string(),
                    content: "first.txt".to_string(),
                    is_error: false,
                }
            ]
        );
        assert_eq!(execution.plan_ready, None);

        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn tool_execution_turns_failures_into_error_results() {
        let blocks = vec![MessageContent::ToolUse {
            id: "toolu_missing_path".to_string(),
            name: "read_file".to_string(),
            input: json!({}),
        }];

        let execution = execute_tool_uses(&blocks, PermissionMode::Default);

        assert_eq!(
            execution.results,
            vec![ToolResult {
                tool_use_id: "toolu_missing_path".to_string(),
                content: "invalid input for tool read_file: expected input.path to be a string"
                    .to_string(),
                is_error: true,
            }]
        );
    }

    #[test]
    fn plan_mode_advertises_exit_plan_mode() {
        let tool_names = definitions(PermissionMode::Plan)
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();

        assert!(tool_names.contains(&"exit_plan_mode".to_string()));
    }

    #[test]
    fn default_mode_does_not_advertise_exit_plan_mode() {
        let tool_names = definitions(PermissionMode::Default)
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();

        assert!(!tool_names.contains(&"exit_plan_mode".to_string()));
    }

    #[test]
    fn exit_plan_mode_returns_plan_ready_instead_of_tool_result() {
        let blocks = vec![MessageContent::ToolUse {
            id: "toolu_plan".to_string(),
            name: "exit_plan_mode".to_string(),
            input: json!({ "plan": "1. Inspect\n2. Edit\n3. Test" }),
        }];

        let execution = execute_tool_uses(&blocks, PermissionMode::Plan);

        assert_eq!(execution.results, Vec::new());
        assert_eq!(
            execution.plan_ready,
            Some(PlanReady {
                tool_use_id: Some("toolu_plan".to_string()),
                plan: "1. Inspect\n2. Edit\n3. Test".to_string(),
            })
        );
    }

    #[test]
    fn executes_read_file_tool_call() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cawir-read-file-test-{}-{}.txt",
            std::process::id(),
            unique
        ));

        std::fs::write(&path, "tokio = \"1\"\nserde = \"1\"\n").unwrap();

        let input = json!({ "path": path.to_string_lossy() });
        let result = execute_tool_call("read_file", &input, PermissionMode::Default).unwrap();

        assert_eq!(result, "tokio = \"1\"\nserde = \"1\"\n");

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn read_file_requires_a_string_path() {
        let error =
            execute_tool_call("read_file", &json!({}), PermissionMode::Default).unwrap_err();

        match error {
            Error::ToolInput { tool, message } => {
                assert_eq!(tool, "read_file");
                assert_eq!(message, "expected input.path to be a string");
            }
            other => panic!("expected tool input error, got {:?}", other),
        }
    }

    #[test]
    fn executes_list_files_tool_call() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cawir-list-files-test-{}-{}",
            std::process::id(),
            unique
        ));

        std::fs::create_dir(&path).unwrap();
        std::fs::create_dir(path.join("src")).unwrap();
        std::fs::write(path.join("Cargo.toml"), "[package]\nname = \"cawir\"\n").unwrap();

        let input = json!({ "path": path.to_string_lossy() });
        let result = execute_tool_call("list_files", &input, PermissionMode::Default).unwrap();

        assert_eq!(result, "Cargo.toml\nsrc/");

        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn list_files_requires_a_string_path() {
        let error =
            execute_tool_call("list_files", &json!({}), PermissionMode::Default).unwrap_err();

        match error {
            Error::ToolInput { tool, message } => {
                assert_eq!(tool, "list_files");
                assert_eq!(message, "expected input.path to be a string");
            }
            other => panic!("expected tool input error, got {:?}", other),
        }
    }

    #[test]
    fn executes_write_file_tool_call_when_approved() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cawir-write-file-test-{}-{}.txt",
            std::process::id(),
            unique
        ));

        let input = json!({
            "path": path.to_string_lossy(),
            "content": "hello from cawir\n"
        });
        let result = execute_write_file_with_approval(&input, |_, _| Ok(true)).unwrap();

        assert_eq!(
            result,
            format!("wrote 17 bytes to {}", path.to_string_lossy())
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "hello from cawir\n"
        );

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn write_file_denial_returns_tool_error() {
        let input = json!({
            "path": "scratch.txt",
            "content": "hello from cawir\n"
        });

        let error = execute_write_file_with_approval(&input, |_, _| Ok(false)).unwrap_err();

        match error {
            Error::ToolDenied { tool, message } => {
                assert_eq!(tool, "write_file");
                assert_eq!(message, "user denied write to scratch.txt");
            }
            other => panic!("expected tool denied error, got {:?}", other),
        }
    }

    #[test]
    fn plan_mode_denies_write_file_without_asking() {
        let input = json!({
            "path": "scratch.txt",
            "content": "hello from cawir\n"
        });
        let mut asked = false;

        let error = execute_write_file_with_policy(&input, PermissionMode::Plan, |_, _| {
            asked = true;
            Ok(true)
        })
        .unwrap_err();

        assert!(!asked);
        match error {
            Error::ToolDenied { tool, message } => {
                assert_eq!(tool, "write_file");
                assert_eq!(message, "plan mode does not allow mutating tools");
            }
            other => panic!("expected tool denied error, got {:?}", other),
        }
    }

    #[test]
    fn write_file_requires_string_content() {
        let error = execute_tool_call(
            "write_file",
            &json!({ "path": "scratch.txt" }),
            PermissionMode::Default,
        )
        .unwrap_err();

        match error {
            Error::ToolInput { tool, message } => {
                assert_eq!(tool, "write_file");
                assert!(message.contains("expected input.content to be a string"));
                assert!(message.contains("received input"));
                assert!(message.contains(r#""path":"scratch.txt""#));
            }
            other => panic!("expected tool input error, got {:?}", other),
        }
    }

    #[test]
    fn executes_shell_tool_call_when_approved() {
        let input = json!({
            "command": "printf 'hello from stdout'; printf 'hello from stderr' >&2"
        });

        let result = execute_shell_with_approval(&input, |_| Ok(true)).unwrap();

        assert_eq!(
            result,
            "exit status: 0\nstdout:\nhello from stdout\nstderr:\nhello from stderr"
        );
    }

    #[test]
    fn shell_denial_returns_tool_error() {
        let input = json!({
            "command": "cargo test"
        });

        let error = execute_shell_with_approval(&input, |_| Ok(false)).unwrap_err();

        match error {
            Error::ToolDenied { tool, message } => {
                assert_eq!(tool, "shell");
                assert_eq!(message, "user denied shell command: cargo test");
            }
            other => panic!("expected tool denied error, got {:?}", other),
        }
    }

    #[test]
    fn bypass_still_denies_catastrophic_shell_commands() {
        let input = json!({
            "command": "rm   -rf   /"
        });

        let error =
            execute_shell_with_policy(&input, PermissionMode::Bypass, |_| Ok(true)).unwrap_err();

        match error {
            Error::ToolDenied { tool, message } => {
                assert_eq!(tool, "shell");
                assert_eq!(message, "refusing catastrophic shell command");
            }
            other => panic!("expected tool denied error, got {:?}", other),
        }
    }

    #[test]
    fn shell_requires_string_command() {
        let error = execute_tool_call("shell", &json!({}), PermissionMode::Default).unwrap_err();

        match error {
            Error::ToolInput { tool, message } => {
                assert_eq!(tool, "shell");
                assert_eq!(message, "expected input.command to be a string");
            }
            other => panic!("expected tool input error, got {:?}", other),
        }
    }

    #[test]
    fn shell_returns_nonzero_status_and_stderr() {
        let input = json!({
            "command": "printf 'failure details' >&2; exit 7"
        });

        let result = execute_shell_with_approval(&input, |_| Ok(true)).unwrap();

        assert_eq!(
            result,
            "exit status: 7\nstdout:\n\nstderr:\nfailure details"
        );
    }
}
