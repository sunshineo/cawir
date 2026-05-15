use std::{
    fs::File,
    io::{ErrorKind, Read},
    path::{Component, Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use crate::{
    Error, Result,
    events::AgentEvent,
    policy::{PermissionDecision, PermissionMode, ToolKind, permission_decision},
    provider::ToolDefinition,
    session::{MessageContent, ToolResult},
};

const READ_FILE_MAX_BYTES: usize = 64 * 1024;
const LIST_FILES_MAX_ENTRIES: usize = 200;
const SHELL_OUTPUT_MAX_BYTES: usize = 32 * 1024;
const SHELL_TIMEOUT: Duration = Duration::from_secs(30);
const SHELL_POLL_INTERVAL: Duration = Duration::from_millis(10);

trait Tool {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> Value;
    fn kind(&self) -> ToolKind;

    fn is_available(&self, mode: PermissionMode) -> bool {
        mode == PermissionMode::Plan || self.name() != "exit_plan_mode"
    }

    fn prepare(&self, input: &Value, context: &ToolContext) -> Result<PreparedToolCall>;
    fn execute(&self, input: PreparedToolInput) -> Result<ToolOutput>;

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            input_schema: self.input_schema(),
        }
    }
}

pub(crate) struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    pub(crate) fn builtins() -> Self {
        Self {
            tools: vec![
                Box::new(ListFilesTool),
                Box::new(ReadFileTool),
                Box::new(WriteFileTool),
                Box::new(EditFileTool),
                Box::new(ShellTool),
                Box::new(ExitPlanModeTool),
            ],
        }
    }

    pub(crate) fn definitions(&self, mode: PermissionMode) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .filter(|tool| tool.is_available(mode))
            .map(|tool| tool.definition())
            .collect()
    }

    fn execute<F>(
        &self,
        name: &str,
        input: &Value,
        mode: PermissionMode,
        approve: &mut F,
    ) -> Result<ToolOutput>
    where
        F: FnMut(&ToolApprovalRequest) -> Result<bool>,
    {
        let tool = self
            .tools
            .iter()
            .find(|tool| tool.name() == name)
            .ok_or_else(|| Error::UnknownTool(name.to_string()))?;

        let context = ToolContext::current_project()?;
        let prepared = tool.prepare(input, &context)?;
        apply_permission_decision(mode, &prepared, approve)?;
        tool.execute(prepared.input)
    }
}

enum ToolOutput {
    Result(String),
    PlanReady(String),
}

struct ToolContext {
    project_root: PathBuf,
}

impl ToolContext {
    fn current_project() -> Result<Self> {
        Ok(Self {
            project_root: std::env::current_dir()?.canonicalize()?,
        })
    }
}

struct PreparedToolCall {
    tool_name: &'static str,
    kind: ToolKind,
    approval: Option<ToolApprovalRequest>,
    input: PreparedToolInput,
}

enum PreparedToolInput {
    ReadFile {
        path: PathBuf,
    },
    ListFiles {
        path: PathBuf,
    },
    WriteFile {
        path: PathBuf,
        content: String,
    },
    EditFile {
        path: PathBuf,
        old_string: String,
        new_string: String,
        replace_all: bool,
    },
    Shell {
        command: String,
    },
    ExitPlanMode {
        plan: String,
    },
}

pub(crate) struct ToolApprovalRequest {
    tool_name: &'static str,
    summary: String,
    denial_message: String,
}

impl ToolApprovalRequest {
    pub(crate) fn tool_name(&self) -> &'static str {
        self.tool_name
    }

    pub(crate) fn summary(&self) -> &str {
        &self.summary
    }
}

fn apply_permission_decision<F>(
    mode: PermissionMode,
    prepared: &PreparedToolCall,
    approve: &mut F,
) -> Result<()>
where
    F: FnMut(&ToolApprovalRequest) -> Result<bool>,
{
    match permission_decision(mode, prepared.kind) {
        PermissionDecision::Allow => Ok(()),
        PermissionDecision::AskUser => {
            let approval = prepared.approval.as_ref().ok_or_else(|| Error::ToolInput {
                tool: prepared.tool_name.to_string(),
                message: "tool requires approval but did not provide an approval request"
                    .to_string(),
            })?;

            if approve(approval)? {
                Ok(())
            } else {
                Err(Error::ToolDenied {
                    tool: prepared.tool_name.to_string(),
                    message: approval.denial_message.clone(),
                })
            }
        }
        PermissionDecision::Deny(reason) => Err(Error::ToolDenied {
            tool: prepared.tool_name.to_string(),
            message: reason.to_string(),
        }),
    }
}

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

struct ReadFileTool;

impl Tool for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn description(&self) -> &'static str {
        "Read the full contents of a UTF-8 text file from the current project. Use this when you need to inspect source code, configuration, documentation, or other text files before answering. Provide a path relative to the current working directory when possible. Do not use this for files outside the project unless the user clearly asks for them."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read, preferably relative to the current working directory."
                }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    fn kind(&self) -> ToolKind {
        ToolKind::ReadOnly
    }

    fn prepare(&self, input: &Value, context: &ToolContext) -> Result<PreparedToolCall> {
        let path = input_string(input, "read_file", "path")?;
        let path = resolve_existing_project_path("read_file", path, context)?;

        Ok(PreparedToolCall {
            tool_name: self.name(),
            kind: self.kind(),
            approval: None,
            input: PreparedToolInput::ReadFile { path },
        })
    }

    fn execute(&self, input: PreparedToolInput) -> Result<ToolOutput> {
        let PreparedToolInput::ReadFile { path } = input else {
            return Err(wrong_prepared_input(self.name()));
        };

        execute_read_file(&path).map(ToolOutput::Result)
    }
}

struct ListFilesTool;

impl Tool for ListFilesTool {
    fn name(&self) -> &'static str {
        "list_files"
    }

    fn description(&self) -> &'static str {
        "List the files and directories inside a folder from the current project. Use this before read_file when you need to discover repository structure or find likely files to inspect. Provide a path relative to the current working directory when possible."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to list, preferably relative to the current working directory."
                }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    fn kind(&self) -> ToolKind {
        ToolKind::ReadOnly
    }

    fn prepare(&self, input: &Value, context: &ToolContext) -> Result<PreparedToolCall> {
        let path = input_string(input, "list_files", "path")?;
        let path = resolve_existing_project_path("list_files", path, context)?;

        Ok(PreparedToolCall {
            tool_name: self.name(),
            kind: self.kind(),
            approval: None,
            input: PreparedToolInput::ListFiles { path },
        })
    }

    fn execute(&self, input: PreparedToolInput) -> Result<ToolOutput> {
        let PreparedToolInput::ListFiles { path } = input else {
            return Err(wrong_prepared_input(self.name()));
        };

        execute_list_files(&path).map(ToolOutput::Result)
    }
}

struct WriteFileTool;

impl Tool for WriteFileTool {
    fn name(&self) -> &'static str {
        "write_file"
    }

    fn description(&self) -> &'static str {
        "Write UTF-8 text content to a file in the current project. Use this only when the user asks you to create or replace a file. The write will require explicit user approval before it runs."
    }

    fn input_schema(&self) -> Value {
        json!({
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
        })
    }

    fn kind(&self) -> ToolKind {
        ToolKind::WriteFile
    }

    fn prepare(&self, input: &Value, context: &ToolContext) -> Result<PreparedToolCall> {
        let raw_path = input_string(input, "write_file", "path")?;
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
        let path = resolve_writable_project_path("write_file", raw_path, context)?;

        Ok(PreparedToolCall {
            tool_name: self.name(),
            kind: self.kind(),
            approval: Some(ToolApprovalRequest {
                tool_name: self.name(),
                summary: format!("write {} bytes to {}", content.len(), path.display()),
                denial_message: format!("user denied write to {}", path.display()),
            }),
            input: PreparedToolInput::WriteFile {
                path,
                content: content.to_string(),
            },
        })
    }

    fn execute(&self, input: PreparedToolInput) -> Result<ToolOutput> {
        let PreparedToolInput::WriteFile { path, content } = input else {
            return Err(wrong_prepared_input(self.name()));
        };

        execute_write_file(&path, &content).map(ToolOutput::Result)
    }
}

struct EditFileTool;

impl Tool for EditFileTool {
    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn description(&self) -> &'static str {
        "Edit an existing UTF-8 text file in the current project by replacing exact text. Use this for routine changes to existing files instead of write_file. The edit will require explicit user approval before it runs."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit, preferably relative to the current working directory."
                },
                "old_string": {
                    "type": "string",
                    "description": "Exact text currently in the file. By default this must match exactly once."
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text."
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace every occurrence of old_string instead of requiring a unique match. Defaults to false."
                }
            },
            "required": ["path", "old_string", "new_string"],
            "additionalProperties": false
        })
    }

    fn kind(&self) -> ToolKind {
        ToolKind::WriteFile
    }

    fn prepare(&self, input: &Value, context: &ToolContext) -> Result<PreparedToolCall> {
        let raw_path = input_string(input, "edit_file", "path")?;
        let old_string = input_string(input, "edit_file", "old_string")?;
        let new_string = input_string(input, "edit_file", "new_string")?;
        let replace_all = input_bool(input, "edit_file", "replace_all")?.unwrap_or(false);
        let path = resolve_existing_project_path("edit_file", raw_path, context)?;

        Ok(PreparedToolCall {
            tool_name: self.name(),
            kind: self.kind(),
            approval: Some(ToolApprovalRequest {
                tool_name: self.name(),
                summary: format!(
                    "replace {} bytes with {} bytes in {}",
                    old_string.len(),
                    new_string.len(),
                    path.display()
                ),
                denial_message: format!("user denied edit to {}", path.display()),
            }),
            input: PreparedToolInput::EditFile {
                path,
                old_string: old_string.to_string(),
                new_string: new_string.to_string(),
                replace_all,
            },
        })
    }

    fn execute(&self, input: PreparedToolInput) -> Result<ToolOutput> {
        let PreparedToolInput::EditFile {
            path,
            old_string,
            new_string,
            replace_all,
        } = input
        else {
            return Err(wrong_prepared_input(self.name()));
        };

        execute_edit_file(&path, &old_string, &new_string, replace_all).map(ToolOutput::Result)
    }
}

struct ShellTool;

impl Tool for ShellTool {
    fn name(&self) -> &'static str {
        "shell"
    }

    fn description(&self) -> &'static str {
        "Run a shell command in the current project. Do not use shell for directory listings, file reads, or file writes; use the dedicated list_files, read_file, and write_file tools for those. Use shell for commands that need a process, such as tests, formatters, builds, git commands, or search commands. The command will require explicit user approval before it runs."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to run from the current working directory."
                }
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Shell
    }

    fn prepare(&self, input: &Value, _context: &ToolContext) -> Result<PreparedToolCall> {
        let command = input_string(input, "shell", "command")?;

        if let Some(reason) = catastrophic_shell_denial(command) {
            return Err(Error::ToolDenied {
                tool: "shell".to_string(),
                message: reason.to_string(),
            });
        }

        Ok(PreparedToolCall {
            tool_name: self.name(),
            kind: self.kind(),
            approval: Some(ToolApprovalRequest {
                tool_name: self.name(),
                summary: format!("run shell command: {}", command),
                denial_message: format!("user denied shell command: {}", command),
            }),
            input: PreparedToolInput::Shell {
                command: command.to_string(),
            },
        })
    }

    fn execute(&self, input: PreparedToolInput) -> Result<ToolOutput> {
        let PreparedToolInput::Shell { command } = input else {
            return Err(wrong_prepared_input(self.name()));
        };

        execute_shell(&command).map(ToolOutput::Result)
    }
}

struct ExitPlanModeTool;

impl Tool for ExitPlanModeTool {
    fn name(&self) -> &'static str {
        "exit_plan_mode"
    }

    fn description(&self) -> &'static str {
        "Submit a concise implementation plan for user approval. Use this when you are in plan mode and ready to ask permission to make changes."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "plan": {
                    "type": "string",
                    "description": "The proposed plan to show to the user before making changes."
                }
            },
            "required": ["plan"],
            "additionalProperties": false
        })
    }

    fn kind(&self) -> ToolKind {
        ToolKind::ExitPlanMode
    }

    fn prepare(&self, input: &Value, _context: &ToolContext) -> Result<PreparedToolCall> {
        let plan = input_string(input, "exit_plan_mode", "plan")?;

        Ok(PreparedToolCall {
            tool_name: self.name(),
            kind: self.kind(),
            approval: None,
            input: PreparedToolInput::ExitPlanMode {
                plan: plan.to_string(),
            },
        })
    }

    fn execute(&self, input: PreparedToolInput) -> Result<ToolOutput> {
        let PreparedToolInput::ExitPlanMode { plan } = input else {
            return Err(wrong_prepared_input(self.name()));
        };

        Ok(ToolOutput::PlanReady(plan))
    }
}

#[cfg(test)]
pub(crate) fn execute_tool_uses<F>(
    registry: &ToolRegistry,
    blocks: &[MessageContent],
    mode: PermissionMode,
    emit: F,
) -> ToolExecution
where
    F: FnMut(AgentEvent),
{
    execute_tool_uses_with_approval(registry, blocks, mode, emit, |_| Ok(true))
}

pub(crate) fn execute_tool_uses_with_approval<F, A>(
    registry: &ToolRegistry,
    blocks: &[MessageContent],
    mode: PermissionMode,
    mut emit: F,
    mut approve: A,
) -> ToolExecution
where
    F: FnMut(AgentEvent),
    A: FnMut(&ToolApprovalRequest) -> Result<bool>,
{
    let mut results = Vec::new();
    let mut plan_ready = None;

    for block in blocks {
        match block {
            MessageContent::Text { .. } => {}
            MessageContent::ToolUse { id, name, input } => {
                emit(AgentEvent::ToolUseRequested {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                });
                match registry.execute(name, input, mode, &mut approve) {
                    Ok(ToolOutput::Result(content)) => {
                        emit(AgentEvent::ToolUseFinished {
                            id: id.clone(),
                            name: name.clone(),
                            output_len: content.len(),
                            is_error: false,
                            error: None,
                        });
                        results.push(ToolResult {
                            tool_use_id: id.clone(),
                            content,
                            is_error: false,
                        });
                    }
                    Ok(ToolOutput::PlanReady(plan)) => {
                        emit(AgentEvent::ToolUseFinished {
                            id: id.clone(),
                            name: name.clone(),
                            output_len: plan.len(),
                            is_error: false,
                            error: None,
                        });
                        plan_ready = Some(PlanReady {
                            tool_use_id: Some(id.clone()),
                            plan,
                        });
                    }
                    Err(error) => {
                        let content = error.to_string();
                        emit(AgentEvent::ToolUseFinished {
                            id: id.clone(),
                            name: name.clone(),
                            output_len: content.len(),
                            is_error: true,
                            error: Some(content.clone()),
                        });
                        results.push(ToolResult {
                            tool_use_id: id.clone(),
                            content,
                            is_error: true,
                        });
                    }
                }
            }
            MessageContent::ToolResult { .. } => {}
        }
    }

    ToolExecution {
        results,
        plan_ready,
    }
}

#[cfg(test)]
fn execute_tool_call(name: &str, input: &Value, mode: PermissionMode) -> Result<String> {
    execute_tool_call_with_approval(name, input, mode, |_| Ok(true))
}

#[cfg(test)]
fn execute_tool_call_with_approval<F>(
    name: &str,
    input: &Value,
    mode: PermissionMode,
    mut approve: F,
) -> Result<String>
where
    F: FnMut(&ToolApprovalRequest) -> Result<bool>,
{
    match ToolRegistry::builtins().execute(name, input, mode, &mut approve)? {
        ToolOutput::Result(content) => Ok(content),
        ToolOutput::PlanReady(_) => Err(Error::ToolInput {
            tool: name.to_string(),
            message: "expected ordinary tool result, received plan-ready output".to_string(),
        }),
    }
}

fn input_string<'a>(input: &'a Value, tool: &str, field: &str) -> Result<&'a str> {
    input
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::ToolInput {
            tool: tool.to_string(),
            message: format!("expected input.{field} to be a string"),
        })
}

fn input_bool(input: &Value, tool: &str, field: &str) -> Result<Option<bool>> {
    match input.get(field) {
        Some(value) => value.as_bool().map(Some).ok_or_else(|| Error::ToolInput {
            tool: tool.to_string(),
            message: format!("expected input.{field} to be a boolean"),
        }),
        None => Ok(None),
    }
}

fn wrong_prepared_input(tool: &str) -> Error {
    Error::ToolInput {
        tool: tool.to_string(),
        message: "tool received prepared input for a different tool".to_string(),
    }
}

fn resolve_existing_project_path(
    tool: &str,
    input_path: &str,
    context: &ToolContext,
) -> Result<PathBuf> {
    let lexical_path = lexical_project_path(tool, input_path, context)?;
    let canonical_path = lexical_path.canonicalize()?;

    if !canonical_path.starts_with(&context.project_root) {
        return Err(outside_project_error(tool, input_path, context));
    }

    Ok(canonical_path)
}

fn resolve_writable_project_path(
    tool: &str,
    input_path: &str,
    context: &ToolContext,
) -> Result<PathBuf> {
    let lexical_path = lexical_project_path(tool, input_path, context)?;

    if lexical_path.exists() {
        let canonical_path = lexical_path.canonicalize()?;
        if !canonical_path.starts_with(&context.project_root) {
            return Err(outside_project_error(tool, input_path, context));
        }
        return Ok(canonical_path);
    }

    let parent = lexical_path.parent().ok_or_else(|| Error::ToolInput {
        tool: tool.to_string(),
        message: format!("path has no parent directory: {input_path}"),
    })?;
    let file_name = lexical_path.file_name().ok_or_else(|| Error::ToolInput {
        tool: tool.to_string(),
        message: format!("path must name a file: {input_path}"),
    })?;
    let canonical_parent = parent.canonicalize()?;

    if !canonical_parent.starts_with(&context.project_root) {
        return Err(outside_project_error(tool, input_path, context));
    }

    Ok(canonical_parent.join(file_name))
}

fn lexical_project_path(tool: &str, input_path: &str, context: &ToolContext) -> Result<PathBuf> {
    let raw_path = Path::new(input_path);
    let joined = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        context.project_root.join(raw_path)
    };
    let normalized = normalize_path_lexically(&joined);

    if !normalized.starts_with(&context.project_root) {
        return Err(outside_project_error(tool, input_path, context));
    }

    Ok(normalized)
}

fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }

    normalized
}

fn outside_project_error(tool: &str, input_path: &str, context: &ToolContext) -> Error {
    Error::ToolDenied {
        tool: tool.to_string(),
        message: format!(
            "path {} is outside the current project {}",
            input_path,
            context.project_root.display()
        ),
    }
}

fn execute_list_files(path: &Path) -> Result<String> {
    let mut entries = Vec::new();
    let mut truncated = false;

    for entry in std::fs::read_dir(path)? {
        if entries.len() == LIST_FILES_MAX_ENTRIES {
            truncated = true;
            break;
        }

        let entry = entry?;
        let file_type = entry.file_type()?;
        let mut name = entry.file_name().to_string_lossy().into_owned();

        if file_type.is_dir() {
            name.push('/');
        }

        entries.push(name);
    }

    entries.sort();
    let mut output = entries.join("\n");

    if truncated {
        append_truncation_marker(
            &mut output,
            format!(
                "list_files returned the first {} entries from {}; directory has more entries",
                LIST_FILES_MAX_ENTRIES,
                path.display()
            ),
        );
    }

    Ok(output)
}

fn execute_read_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let total_bytes = file.metadata()?.len();
    let mut bytes = Vec::new();

    file.by_ref()
        .take((READ_FILE_MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;

    let truncated = bytes.len() > READ_FILE_MAX_BYTES;
    let visible_bytes = if truncated {
        &bytes[..READ_FILE_MAX_BYTES]
    } else {
        &bytes
    };
    let prefix_len = utf8_prefix_len(visible_bytes)?;
    let mut output = std::str::from_utf8(&visible_bytes[..prefix_len])
        .map_err(invalid_utf8_error)?
        .to_string();

    if truncated {
        append_truncation_marker(
            &mut output,
            format!(
                "read_file returned the first {} valid UTF-8 bytes of {} bytes from {}",
                prefix_len,
                total_bytes,
                path.display()
            ),
        );
    }

    Ok(output)
}

fn execute_write_file(path: &Path, content: &str) -> Result<String> {
    std::fs::write(path, content)?;

    Ok(format!(
        "wrote {} bytes to {}",
        content.len(),
        path.display()
    ))
}

fn execute_edit_file(
    path: &Path,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<String> {
    if old_string == new_string {
        return Err(Error::ToolInput {
            tool: "edit_file".to_string(),
            message: "old_string and new_string are identical".to_string(),
        });
    }

    if old_string.is_empty() {
        return Err(Error::ToolInput {
            tool: "edit_file".to_string(),
            message: "old_string must not be empty; use write_file for new files or full rewrites"
                .to_string(),
        });
    }

    let content = std::fs::read_to_string(path)?;
    let matches = content.matches(old_string).count();

    if matches == 0 {
        return Err(Error::ToolInput {
            tool: "edit_file".to_string(),
            message: "old_string was not found in the file".to_string(),
        });
    }

    if matches > 1 && !replace_all {
        return Err(Error::ToolInput {
            tool: "edit_file".to_string(),
            message: format!(
                "found {matches} matches for old_string; provide more context or set replace_all to true"
            ),
        });
    }

    let updated = if replace_all {
        content.replace(old_string, new_string)
    } else {
        content.replacen(old_string, new_string, 1)
    };

    std::fs::write(path, updated)?;

    let noun = if matches == 1 {
        "occurrence"
    } else {
        "occurrences"
    };
    Ok(format!(
        "edited {matches} {noun} in {}",
        path.to_string_lossy()
    ))
}

fn execute_shell(command: &str) -> Result<String> {
    execute_shell_with_timeout(command, SHELL_TIMEOUT)
}

fn execute_shell_with_timeout(command: &str, timeout: Duration) -> Result<String> {
    let mut process = Command::new("sh");
    process
        .arg("-c")
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        process.process_group(0);
    }

    let mut child = process.spawn()?;
    let start = Instant::now();

    loop {
        if child.try_wait()?.is_some() {
            let output = child.wait_with_output()?;
            return Ok(format_shell_output(ShellOutput::Finished(output)));
        }

        if start.elapsed() >= timeout {
            terminate_shell_process(&mut child)?;

            let output = child.wait_with_output()?;
            return Ok(format_shell_output(ShellOutput::TimedOut {
                output,
                timeout,
            }));
        }

        thread::sleep(SHELL_POLL_INTERVAL);
    }
}

#[cfg(unix)]
fn terminate_shell_process(child: &mut std::process::Child) -> Result<()> {
    let process_group = format!("-{}", child.id());

    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(&process_group)
        .status();
    thread::sleep(SHELL_POLL_INTERVAL);

    if child.try_wait()?.is_none() {
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(&process_group)
            .status();
    }

    kill_direct_child(child)
}

#[cfg(not(unix))]
fn terminate_shell_process(child: &mut std::process::Child) -> Result<()> {
    kill_direct_child(child)
}

fn kill_direct_child(child: &mut std::process::Child) -> Result<()> {
    match child.kill() {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::InvalidInput => Ok(()),
        Err(error) => Err(error.into()),
    }
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

enum ShellOutput {
    Finished(Output),
    TimedOut { output: Output, timeout: Duration },
}

fn format_shell_output(run: ShellOutput) -> String {
    let (status, output) = match run {
        ShellOutput::Finished(output) => {
            let status = match output.status.code() {
                Some(code) => format!("exit status: {}", code),
                None => "exit status: terminated by signal".to_string(),
            };
            (status, output)
        }
        ShellOutput::TimedOut { output, timeout } => (
            format!("exit status: timed out after {}", format_duration(timeout)),
            output,
        ),
    };
    let stdout = budgeted_lossy_output("stdout", &output.stdout, SHELL_OUTPUT_MAX_BYTES);
    let stderr = budgeted_lossy_output("stderr", &output.stderr, SHELL_OUTPUT_MAX_BYTES);

    format!("{}\nstdout:\n{}\nstderr:\n{}", status, stdout, stderr)
}

fn budgeted_lossy_output(label: &str, bytes: &[u8], max_bytes: usize) -> String {
    let truncated = bytes.len() > max_bytes;
    let visible_bytes = if truncated {
        let prefix_len = utf8_lossy_prefix_len(&bytes[..max_bytes]);
        &bytes[..prefix_len]
    } else {
        bytes
    };
    let mut output = String::from_utf8_lossy(visible_bytes).into_owned();

    if truncated {
        append_truncation_marker(
            &mut output,
            format!(
                "{} returned the first {} bytes of {} bytes",
                label,
                visible_bytes.len(),
                bytes.len()
            ),
        );
    }

    output
}

fn utf8_prefix_len(bytes: &[u8]) -> std::io::Result<usize> {
    match std::str::from_utf8(bytes) {
        Ok(_) => Ok(bytes.len()),
        Err(error) if error.error_len().is_none() => Ok(error.valid_up_to()),
        Err(error) => Err(invalid_utf8_error(error)),
    }
}

fn utf8_lossy_prefix_len(bytes: &[u8]) -> usize {
    match std::str::from_utf8(bytes) {
        Ok(_) => bytes.len(),
        Err(error) if error.error_len().is_none() => error.valid_up_to(),
        Err(_) => bytes.len(),
    }
}

fn invalid_utf8_error(error: std::str::Utf8Error) -> std::io::Error {
    std::io::Error::new(
        ErrorKind::InvalidData,
        format!("file is not valid UTF-8: {error}"),
    )
}

fn append_truncation_marker(output: &mut String, detail: String) {
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }

    output.push_str("[tool output truncated: ");
    output.push_str(&detail);
    output.push(']');
}

fn format_duration(duration: Duration) -> String {
    if duration.as_millis() < 1_000 {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{}s", duration.as_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn project_test_path(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("cawir-tool-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{}-{}-{}", prefix, std::process::id(), unique))
    }

    #[test]
    fn tool_execution_returns_results_for_all_tool_uses() {
        let path = project_test_path("multi-tool");
        let registry = ToolRegistry::builtins();

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

        let execution = execute_tool_uses(&registry, &blocks, PermissionMode::Default, |_| {});

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
        let registry = ToolRegistry::builtins();
        let blocks = vec![MessageContent::ToolUse {
            id: "toolu_missing_path".to_string(),
            name: "read_file".to_string(),
            input: json!({}),
        }];

        let execution = execute_tool_uses(&registry, &blocks, PermissionMode::Default, |_| {});

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
    fn tool_execution_emits_progress_events_separate_from_results() {
        let path = project_test_path("tool-event.txt");
        let registry = ToolRegistry::builtins();
        std::fs::write(&path, "event result").unwrap();

        let input = json!({ "path": path.to_string_lossy() });
        let blocks = vec![MessageContent::ToolUse {
            id: "toolu_event".to_string(),
            name: "read_file".to_string(),
            input: input.clone(),
        }];
        let mut events = Vec::new();

        let execution = execute_tool_uses(&registry, &blocks, PermissionMode::Default, |event| {
            events.push(event);
        });

        assert_eq!(
            events,
            vec![
                AgentEvent::ToolUseRequested {
                    id: "toolu_event".to_string(),
                    name: "read_file".to_string(),
                    input,
                },
                AgentEvent::ToolUseFinished {
                    id: "toolu_event".to_string(),
                    name: "read_file".to_string(),
                    output_len: "event result".len(),
                    is_error: false,
                    error: None,
                }
            ]
        );
        assert_eq!(
            execution.results,
            vec![ToolResult {
                tool_use_id: "toolu_event".to_string(),
                content: "event result".to_string(),
                is_error: false,
            }]
        );

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn plan_mode_advertises_exit_plan_mode() {
        let registry = ToolRegistry::builtins();
        let tool_names = registry
            .definitions(PermissionMode::Plan)
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();

        assert_eq!(
            tool_names,
            vec![
                "list_files",
                "read_file",
                "write_file",
                "edit_file",
                "shell",
                "exit_plan_mode",
            ]
        );
    }

    #[test]
    fn default_mode_advertises_builtins_in_deterministic_order() {
        let registry = ToolRegistry::builtins();
        let tool_names = registry
            .definitions(PermissionMode::Default)
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();

        assert_eq!(
            tool_names,
            vec![
                "list_files",
                "read_file",
                "write_file",
                "edit_file",
                "shell"
            ]
        );
    }

    #[test]
    fn exit_plan_mode_returns_plan_ready_instead_of_tool_result() {
        let registry = ToolRegistry::builtins();
        let blocks = vec![MessageContent::ToolUse {
            id: "toolu_plan".to_string(),
            name: "exit_plan_mode".to_string(),
            input: json!({ "plan": "1. Inspect\n2. Edit\n3. Test" }),
        }];

        let execution = execute_tool_uses(&registry, &blocks, PermissionMode::Plan, |_| {});

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
        let path = project_test_path("read-file.txt");

        std::fs::write(&path, "tokio = \"1\"\nserde = \"1\"\n").unwrap();

        let input = json!({ "path": path.to_string_lossy() });
        let result = execute_tool_call("read_file", &input, PermissionMode::Default).unwrap();

        assert_eq!(result, "tokio = \"1\"\nserde = \"1\"\n");

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn read_file_truncates_large_files_with_a_visible_marker() {
        let path = project_test_path("large-read-file.txt");
        let content = "a".repeat(READ_FILE_MAX_BYTES + 10);

        std::fs::write(&path, content).unwrap();

        let input = json!({ "path": path.to_string_lossy() });
        let result = execute_tool_call("read_file", &input, PermissionMode::Default).unwrap();

        assert!(result.starts_with(&"a".repeat(READ_FILE_MAX_BYTES)));
        assert!(result.contains("[tool output truncated: read_file returned the first"));
        assert!(result.contains("valid UTF-8 bytes"));

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn read_file_truncation_does_not_split_utf8_characters() {
        let path = project_test_path("utf8-boundary-read-file.txt");
        let content = format!("{}étail", "a".repeat(READ_FILE_MAX_BYTES - 1));

        std::fs::write(&path, content).unwrap();

        let input = json!({ "path": path.to_string_lossy() });
        let result = execute_tool_call("read_file", &input, PermissionMode::Default).unwrap();

        assert!(result.starts_with(&"a".repeat(READ_FILE_MAX_BYTES - 1)));
        assert!(!result.contains('é'));
        assert!(!result.contains('�'));
        assert!(result.contains("[tool output truncated: read_file returned the first"));

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
        let path = project_test_path("list-files");

        std::fs::create_dir(&path).unwrap();
        std::fs::create_dir(path.join("src")).unwrap();
        std::fs::write(path.join("Cargo.toml"), "[package]\nname = \"cawir\"\n").unwrap();

        let input = json!({ "path": path.to_string_lossy() });
        let result = execute_tool_call("list_files", &input, PermissionMode::Default).unwrap();

        assert_eq!(result, "Cargo.toml\nsrc/");

        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn list_files_truncates_large_directories_with_a_visible_marker() {
        let path = project_test_path("large-list-files");

        std::fs::create_dir(&path).unwrap();
        for index in 0..(LIST_FILES_MAX_ENTRIES + 5) {
            std::fs::write(path.join(format!("{index:03}.txt")), "").unwrap();
        }

        let input = json!({ "path": path.to_string_lossy() });
        let result = execute_tool_call("list_files", &input, PermissionMode::Default).unwrap();

        assert!(result.contains("[tool output truncated: list_files returned the first"));
        assert!(result.contains("directory has more entries"));

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
    fn read_file_denies_paths_outside_project() {
        let error = execute_tool_call(
            "read_file",
            &json!({ "path": "../Cargo.toml" }),
            PermissionMode::Default,
        )
        .unwrap_err();

        match error {
            Error::ToolDenied { tool, message } => {
                assert_eq!(tool, "read_file");
                assert!(message.contains("outside the current project"));
            }
            other => panic!("expected tool denied error, got {:?}", other),
        }
    }

    #[test]
    fn list_files_denies_absolute_paths_outside_project() {
        let outside = std::env::temp_dir();
        let error = execute_tool_call(
            "list_files",
            &json!({ "path": outside.to_string_lossy() }),
            PermissionMode::Default,
        )
        .unwrap_err();

        match error {
            Error::ToolDenied { tool, message } => {
                assert_eq!(tool, "list_files");
                assert!(message.contains("outside the current project"));
            }
            other => panic!("expected tool denied error, got {:?}", other),
        }
    }

    #[test]
    fn executes_write_file_tool_call_when_approved() {
        let path = project_test_path("write-file.txt");

        let input = json!({
            "path": path.to_string_lossy(),
            "content": "hello from cawir\n"
        });
        let result =
            execute_tool_call_with_approval("write_file", &input, PermissionMode::Default, |_| {
                Ok(true)
            })
            .unwrap();

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
    fn executes_edit_file_tool_call_when_approved() {
        let path = project_test_path("edit-file.txt");
        std::fs::write(&path, "alpha\nbeta\ngamma\n").unwrap();

        let input = json!({
            "path": path.to_string_lossy(),
            "old_string": "beta\n",
            "new_string": "delta\n"
        });
        let result =
            execute_tool_call_with_approval("edit_file", &input, PermissionMode::Default, |_| {
                Ok(true)
            })
            .unwrap();

        assert_eq!(
            result,
            format!("edited 1 occurrence in {}", path.to_string_lossy())
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "alpha\ndelta\ngamma\n"
        );

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn edit_file_replaces_all_matches_when_requested() {
        let path = project_test_path("edit-file-replace-all.txt");
        std::fs::write(&path, "name = old\nother = old\n").unwrap();

        let input = json!({
            "path": path.to_string_lossy(),
            "old_string": "old",
            "new_string": "new",
            "replace_all": true
        });
        let result =
            execute_tool_call_with_approval("edit_file", &input, PermissionMode::Default, |_| {
                Ok(true)
            })
            .unwrap();

        assert_eq!(
            result,
            format!("edited 2 occurrences in {}", path.to_string_lossy())
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "name = new\nother = new\n"
        );

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn edit_file_requires_unique_match_by_default() {
        let path = project_test_path("edit-file-ambiguous.txt");
        std::fs::write(&path, "old\nold\n").unwrap();

        let input = json!({
            "path": path.to_string_lossy(),
            "old_string": "old",
            "new_string": "new"
        });
        let error =
            execute_tool_call_with_approval("edit_file", &input, PermissionMode::Default, |_| {
                Ok(true)
            })
            .unwrap_err();

        match error {
            Error::ToolInput { tool, message } => {
                assert_eq!(tool, "edit_file");
                assert!(message.contains("found 2 matches"));
                assert!(message.contains("replace_all"));
            }
            other => panic!("expected tool input error, got {:?}", other),
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old\nold\n");

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn edit_file_errors_when_match_is_missing() {
        let path = project_test_path("edit-file-missing-match.txt");
        std::fs::write(&path, "alpha\nbeta\n").unwrap();

        let input = json!({
            "path": path.to_string_lossy(),
            "old_string": "gamma",
            "new_string": "delta"
        });
        let error =
            execute_tool_call_with_approval("edit_file", &input, PermissionMode::Default, |_| {
                Ok(true)
            })
            .unwrap_err();

        match error {
            Error::ToolInput { tool, message } => {
                assert_eq!(tool, "edit_file");
                assert!(message.contains("old_string was not found"));
            }
            other => panic!("expected tool input error, got {:?}", other),
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "alpha\nbeta\n");

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn edit_file_denies_paths_outside_project_before_approval() {
        let input = json!({
            "path": "../cawir-outside-edit.txt",
            "old_string": "old",
            "new_string": "new"
        });
        let mut asked = false;

        let error =
            execute_tool_call_with_approval("edit_file", &input, PermissionMode::Default, |_| {
                asked = true;
                Ok(true)
            })
            .unwrap_err();

        assert!(!asked);
        match error {
            Error::ToolDenied { tool, message } => {
                assert_eq!(tool, "edit_file");
                assert!(message.contains("outside the current project"));
            }
            other => panic!("expected tool denied error, got {:?}", other),
        }
    }

    #[test]
    fn plan_mode_denies_edit_file_without_asking() {
        let path = project_test_path("plan-denied-edit.txt");
        std::fs::write(&path, "old\n").unwrap();
        let input = json!({
            "path": path.to_string_lossy(),
            "old_string": "old",
            "new_string": "new"
        });
        let mut asked = false;

        let error =
            execute_tool_call_with_approval("edit_file", &input, PermissionMode::Plan, |_| {
                asked = true;
                Ok(true)
            })
            .unwrap_err();

        assert!(!asked);
        match error {
            Error::ToolDenied { tool, message } => {
                assert_eq!(tool, "edit_file");
                assert_eq!(message, "plan mode does not allow mutating tools");
            }
            other => panic!("expected tool denied error, got {:?}", other),
        }

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn write_file_denies_paths_outside_project_before_approval() {
        let input = json!({
            "path": "../cawir-outside-write.txt",
            "content": "hello from cawir\n"
        });
        let mut asked = false;

        let error =
            execute_tool_call_with_approval("write_file", &input, PermissionMode::Default, |_| {
                asked = true;
                Ok(true)
            })
            .unwrap_err();

        assert!(!asked);
        match error {
            Error::ToolDenied { tool, message } => {
                assert_eq!(tool, "write_file");
                assert!(message.contains("outside the current project"));
            }
            other => panic!("expected tool denied error, got {:?}", other),
        }
    }

    #[test]
    fn write_file_denial_returns_tool_error() {
        let path = project_test_path("denied-write.txt");
        let input = json!({
            "path": path.to_string_lossy(),
            "content": "hello from cawir\n"
        });

        let error =
            execute_tool_call_with_approval("write_file", &input, PermissionMode::Default, |_| {
                Ok(false)
            })
            .unwrap_err();

        match error {
            Error::ToolDenied { tool, message } => {
                assert_eq!(tool, "write_file");
                assert_eq!(message, format!("user denied write to {}", path.display()));
            }
            other => panic!("expected tool denied error, got {:?}", other),
        }
    }

    #[test]
    fn plan_mode_denies_write_file_without_asking() {
        let path = project_test_path("plan-denied-write.txt");
        let input = json!({
            "path": path.to_string_lossy(),
            "content": "hello from cawir\n"
        });
        let mut asked = false;

        let error =
            execute_tool_call_with_approval("write_file", &input, PermissionMode::Plan, |_| {
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

        let result =
            execute_tool_call_with_approval("shell", &input, PermissionMode::Default, |_| Ok(true))
                .unwrap();

        assert_eq!(
            result,
            "exit status: 0\nstdout:\nhello from stdout\nstderr:\nhello from stderr"
        );
    }

    #[test]
    fn shell_truncates_stdout_and_stderr_with_visible_markers() {
        let stdout = vec![b'a'; SHELL_OUTPUT_MAX_BYTES + 10];
        let stderr = vec![b'b'; SHELL_OUTPUT_MAX_BYTES + 20];
        let output = Output {
            status: Command::new("sh").arg("-c").arg("exit 0").status().unwrap(),
            stdout,
            stderr,
        };

        let result = format_shell_output(ShellOutput::Finished(output));

        assert!(result.contains("[tool output truncated: stdout returned the first"));
        assert!(result.contains("[tool output truncated: stderr returned the first"));
    }

    #[test]
    fn shell_timeout_kills_long_running_commands() {
        let result =
            execute_shell_with_timeout("while true; do :; done", Duration::from_millis(50))
                .unwrap();

        assert!(result.starts_with("exit status: timed out after 50ms"));
    }

    #[test]
    fn shell_denial_returns_tool_error() {
        let input = json!({
            "command": "cargo test"
        });

        let error =
            execute_tool_call_with_approval("shell", &input, PermissionMode::Default, |_| {
                Ok(false)
            })
            .unwrap_err();

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
            execute_tool_call_with_approval("shell", &input, PermissionMode::Bypass, |_| Ok(true))
                .unwrap_err();

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

        let result =
            execute_tool_call_with_approval("shell", &input, PermissionMode::Default, |_| Ok(true))
                .unwrap();

        assert_eq!(
            result,
            "exit status: 7\nstdout:\n\nstderr:\nfailure details"
        );
    }
}
