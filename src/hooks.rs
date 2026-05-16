use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use serde::Deserialize;
use serde_json::Value;

use crate::{Error, Result, events::AgentEvent, settings::SettingsResolver};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum HookAction {
    Continue,
    Deny(String),
    ModifyInput(Value),
}

pub(crate) struct HookRegistry {
    handlers: BTreeMap<HookEventKind, Vec<Box<dyn HookHandler>>>,
}

impl HookRegistry {
    pub(crate) fn empty() -> Self {
        Self {
            handlers: BTreeMap::new(),
        }
    }

    pub(crate) fn for_project(project_root: &Path) -> Result<Self> {
        let settings = SettingsResolver::for_project(project_root)?.load()?;
        Self::from_settings(&settings, project_root)
    }

    pub(crate) fn from_settings(settings: &Value, project_root: &Path) -> Result<Self> {
        let settings = serde_json::from_value::<HookSettings>(settings.clone())
            .map_err(|error| Error::Env(format!("failed to parse hook settings: {error}")))?;
        let mut registry = Self::empty();

        for (event_name, hooks) in settings.hooks {
            let kind = HookEventKind::parse(&event_name).ok_or_else(|| {
                Error::Env(format!("unknown hook event in settings: {event_name}"))
            })?;

            for hook in hooks {
                match hook {
                    HookConfig::Command {
                        command,
                        tool,
                        path_suffix,
                    } => registry.register(
                        kind,
                        CommandHook {
                            command,
                            tool,
                            path_suffix,
                            project_root: project_root.to_path_buf(),
                        },
                    ),
                }
            }
        }

        Ok(registry)
    }

    pub(crate) fn dispatch(&self, event: &AgentEvent) -> Result<HookAction> {
        let kind = HookEventKind::from_event(event);
        let Some(handlers) = self.handlers.get(&kind) else {
            return Ok(HookAction::Continue);
        };

        let mut current_event = event.clone();
        let mut action = HookAction::Continue;
        for handler in handlers {
            match handler.on_event(&current_event)? {
                HookAction::Continue => {}
                HookAction::Deny(message) => return Ok(HookAction::Deny(message)),
                HookAction::ModifyInput(input) => {
                    if let AgentEvent::PreToolUse {
                        input: current_input,
                        ..
                    } = &mut current_event
                    {
                        *current_input = input.clone();
                    }
                    action = HookAction::ModifyInput(input);
                }
            }
        }

        Ok(action)
    }

    fn register(&mut self, event: HookEventKind, handler: impl HookHandler + 'static) {
        self.handlers
            .entry(event)
            .or_default()
            .push(Box::new(handler));
    }
}

trait HookHandler {
    fn on_event(&self, event: &AgentEvent) -> Result<HookAction>;
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum HookEventKind {
    SessionStart,
    SessionEnd,
    UserPromptSubmit,
    ModelRequestStart,
    ModelRequestFinish,
    PreToolUse,
    PostToolUse,
    AssistantText,
    AssistantTextDelta,
    AssistantToolUseStart,
    AssistantToolUseInputDelta,
    Stop,
    StopFailure,
}

impl HookEventKind {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "session_start" => Some(Self::SessionStart),
            "session_end" => Some(Self::SessionEnd),
            "user_prompt_submit" => Some(Self::UserPromptSubmit),
            "model_request_start" => Some(Self::ModelRequestStart),
            "model_request_finish" => Some(Self::ModelRequestFinish),
            "pre_tool_use" => Some(Self::PreToolUse),
            "post_tool_use" => Some(Self::PostToolUse),
            "assistant_text" => Some(Self::AssistantText),
            "assistant_text_delta" => Some(Self::AssistantTextDelta),
            "assistant_tool_use_start" => Some(Self::AssistantToolUseStart),
            "assistant_tool_use_input_delta" => Some(Self::AssistantToolUseInputDelta),
            "stop" => Some(Self::Stop),
            "stop_failure" => Some(Self::StopFailure),
            _ => None,
        }
    }

    fn from_event(event: &AgentEvent) -> Self {
        match event {
            AgentEvent::SessionStart { .. } => Self::SessionStart,
            AgentEvent::SessionEnd { .. } => Self::SessionEnd,
            AgentEvent::UserPromptSubmit { .. } => Self::UserPromptSubmit,
            AgentEvent::ModelRequestStart { .. } => Self::ModelRequestStart,
            AgentEvent::ModelRequestFinish { .. } => Self::ModelRequestFinish,
            AgentEvent::PreToolUse { .. } => Self::PreToolUse,
            AgentEvent::PostToolUse { .. } => Self::PostToolUse,
            AgentEvent::AssistantText { .. } => Self::AssistantText,
            AgentEvent::AssistantTextDelta { .. } => Self::AssistantTextDelta,
            AgentEvent::AssistantToolUseStart { .. } => Self::AssistantToolUseStart,
            AgentEvent::AssistantToolUseInputDelta { .. } => Self::AssistantToolUseInputDelta,
            AgentEvent::Stop { .. } => Self::Stop,
            AgentEvent::StopFailure { .. } => Self::StopFailure,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct HookSettings {
    #[serde(default)]
    hooks: BTreeMap<String, Vec<HookConfig>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum HookConfig {
    Command {
        command: String,
        #[serde(default)]
        tool: Option<String>,
        #[serde(default)]
        path_suffix: Option<String>,
    },
}

struct CommandHook {
    command: String,
    tool: Option<String>,
    path_suffix: Option<String>,
    project_root: PathBuf,
}

impl HookHandler for CommandHook {
    fn on_event(&self, event: &AgentEvent) -> Result<HookAction> {
        if !self.matches_event(event) {
            return Ok(HookAction::Continue);
        }

        let event_json = serde_json::to_vec(event).map_err(|error| Error::Hook {
            hook: self.command.clone(),
            message: format!("failed to serialize event: {error}"),
        })?;
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(&self.command)
            .current_dir(&self.project_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdin = child.stdin.take().ok_or_else(|| Error::Hook {
            hook: self.command.clone(),
            message: "failed to open hook stdin".to_string(),
        })?;
        stdin.write_all(&event_json)?;
        drop(stdin);

        let output = child.wait_with_output()?;
        parse_command_output(&self.command, output)
    }
}

impl CommandHook {
    fn matches_event(&self, event: &AgentEvent) -> bool {
        if let Some(expected_tool) = &self.tool
            && tool_name(event) != Some(expected_tool.as_str())
        {
            return false;
        }

        if let Some(suffix) = &self.path_suffix
            && !input_path(event).is_some_and(|path| path.ends_with(suffix))
        {
            return false;
        }

        true
    }
}

fn tool_name(event: &AgentEvent) -> Option<&str> {
    match event {
        AgentEvent::PreToolUse { name, .. } | AgentEvent::PostToolUse { name, .. } => Some(name),
        _ => None,
    }
}

fn input_path(event: &AgentEvent) -> Option<&str> {
    match event {
        AgentEvent::PreToolUse { input, .. } | AgentEvent::PostToolUse { input, .. } => {
            input.get("path").and_then(Value::as_str)
        }
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum CommandHookOutput {
    Allow,
    Deny { message: String },
    Modify { input: Value },
}

fn parse_command_output(command: &str, output: std::process::Output) -> Result<HookAction> {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        let message = if stderr.is_empty() {
            if stdout.is_empty() {
                format!("hook command exited with {}", output.status)
            } else {
                stdout
            }
        } else {
            stderr
        };
        return Ok(HookAction::Deny(message));
    }

    if stdout.is_empty() {
        return Ok(HookAction::Continue);
    }

    match serde_json::from_str::<CommandHookOutput>(&stdout).map_err(|error| Error::Hook {
        hook: command.to_string(),
        message: format!("failed to parse hook stdout as action JSON: {error}: {stdout}"),
    })? {
        CommandHookOutput::Allow => Ok(HookAction::Continue),
        CommandHookOutput::Deny { message } => Ok(HookAction::Deny(message)),
        CommandHookOutput::Modify { input } => Ok(HookAction::ModifyInput(input)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        events::AgentEvent,
        policy::PermissionMode,
        session::{MessageContent, ToolResult},
        tools::{self, ToolRegistry},
    };
    use serde_json::json;
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn hook_test_path(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("cawir-hook-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{}-{}-{}", prefix, std::process::id(), unique))
    }

    #[test]
    fn command_hook_receives_event_json_on_stdin() {
        let project = hook_test_path("stdin-project");
        let captured = project.join("event.json");
        std::fs::create_dir(&project).unwrap();
        let settings = json!({
            "hooks": {
                "pre_tool_use": [
                    {
                        "type": "command",
                        "command": format!("cat > {}; printf '{{\"action\":\"allow\"}}'", captured.display())
                    }
                ]
            }
        });
        let registry = HookRegistry::from_settings(&settings, &project).unwrap();
        let event = AgentEvent::PreToolUse {
            id: "toolu_read".to_string(),
            name: "read_file".to_string(),
            input: json!({ "path": "Cargo.toml" }),
        };

        let action = registry.dispatch(&event).unwrap();

        assert_eq!(action, HookAction::Continue);
        let captured_event: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(captured).unwrap()).unwrap();
        assert_eq!(
            captured_event,
            json!({
                "type": "pre_tool_use",
                "id": "toolu_read",
                "name": "read_file",
                "input": { "path": "Cargo.toml" }
            })
        );

        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn command_hook_stdout_can_deny_pre_tool_use() {
        let project = hook_test_path("deny-project");
        std::fs::create_dir(&project).unwrap();
        let settings = json!({
            "hooks": {
                "pre_tool_use": [
                    {
                        "type": "command",
                        "command": "printf '{\"action\":\"deny\",\"message\":\"blocked by hook\"}'"
                    }
                ]
            }
        });
        let registry = HookRegistry::from_settings(&settings, &project).unwrap();

        let action = registry
            .dispatch(&AgentEvent::PreToolUse {
                id: "toolu_read".to_string(),
                name: "read_file".to_string(),
                input: json!({ "path": "Cargo.toml" }),
            })
            .unwrap();

        assert_eq!(action, HookAction::Deny("blocked by hook".to_string()));

        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn command_hook_stdout_can_modify_pre_tool_input() {
        let project = hook_test_path("modify-project");
        std::fs::create_dir(&project).unwrap();
        let settings = json!({
            "hooks": {
                "pre_tool_use": [
                    {
                        "type": "command",
                        "command": "printf '{\"action\":\"modify\",\"input\":{\"path\":\"README.md\"}}'"
                    }
                ]
            }
        });
        let registry = HookRegistry::from_settings(&settings, &project).unwrap();

        let action = registry
            .dispatch(&AgentEvent::PreToolUse {
                id: "toolu_read".to_string(),
                name: "read_file".to_string(),
                input: json!({ "path": "Cargo.toml" }),
            })
            .unwrap();

        assert_eq!(
            action,
            HookAction::ModifyInput(json!({ "path": "README.md" }))
        );

        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn pre_tool_deny_hook_blocks_execution_as_tool_result() {
        let project = hook_test_path("pre-deny-tool");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("Cargo.toml"), "should not be read").unwrap();
        let settings = json!({
            "hooks": {
                "pre_tool_use": [
                    {
                        "type": "command",
                        "tool": "read_file",
                        "command": "printf '{\"action\":\"deny\",\"message\":\"reads are disabled\"}'"
                    }
                ]
            }
        });
        let hook_registry = HookRegistry::from_settings(&settings, &project).unwrap();
        let tool_registry = ToolRegistry::builtins();
        let blocks = vec![MessageContent::ToolUse {
            id: "toolu_read".to_string(),
            name: "read_file".to_string(),
            input: json!({ "path": "Cargo.toml" }),
        }];

        let execution = tools::execute_tool_uses_in_project_with_hooks(
            &tool_registry,
            &hook_registry,
            &project,
            &blocks,
            PermissionMode::Default,
            |_| {},
        );

        assert_eq!(
            execution.results,
            vec![ToolResult {
                tool_use_id: "toolu_read".to_string(),
                content: "tool read_file denied: reads are disabled".to_string(),
                is_error: true,
            }]
        );

        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn post_write_hook_can_format_rust_files() {
        let project = hook_test_path("post-write-format");
        let src = project.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"hook_fmt_test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        let settings = json!({
            "hooks": {
                "post_tool_use": [
                    {
                        "type": "command",
                        "tool": "write_file",
                        "path_suffix": ".rs",
                        "command": format!("cargo fmt --manifest-path {}", project.join("Cargo.toml").display())
                    }
                ]
            }
        });
        let hook_registry = HookRegistry::from_settings(&settings, &project).unwrap();
        let tool_registry = ToolRegistry::builtins();
        let blocks = vec![MessageContent::ToolUse {
            id: "toolu_write".to_string(),
            name: "write_file".to_string(),
            input: json!({
                "path": "src/lib.rs",
                "content": "pub fn value()->i32{1}\n"
            }),
        }];

        let execution = tools::execute_tool_uses_in_project_with_hooks(
            &tool_registry,
            &hook_registry,
            &project,
            &blocks,
            PermissionMode::Bypass,
            |_| {},
        );

        assert_eq!(execution.results.len(), 1);
        assert!(!execution.results[0].is_error);
        assert_eq!(
            std::fs::read_to_string(src.join("lib.rs")).unwrap(),
            "pub fn value() -> i32 {\n    1\n}\n"
        );

        std::fs::remove_dir_all(project).unwrap();
    }
}
