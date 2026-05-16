use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    Error, Result,
    policy::ToolKind,
    settings::deep_merge,
    tools::{
        PreparedToolCall, PreparedToolInput, Tool, ToolApprovalRequest, ToolContext, ToolOutput,
        ToolRegistry,
    },
};

pub(crate) const PLUGIN_MANIFEST_FILE: &str = "cawir-plugin.json";

#[derive(Debug)]
pub(crate) struct PluginCatalog {
    plugins: Vec<PluginPackage>,
}

impl PluginCatalog {
    pub(crate) fn from_settings(settings: &Value, project_root: &Path) -> Result<Self> {
        let mut plugins = Vec::new();
        for directory in configured_plugin_directories(settings, project_root)? {
            plugins.extend(discover_plugins_in_directory(&directory)?);
        }
        plugins.sort_by(|left, right| {
            left.manifest
                .name
                .cmp(&right.manifest.name)
                .then_with(|| left.root.cmp(&right.root))
        });

        Ok(Self { plugins })
    }

    pub(crate) fn merged_settings(&self, mut settings: Value) -> Value {
        for plugin in &self.plugins {
            deep_merge(&mut settings, plugin.manifest.settings.clone());
            append_plugin_hooks(&mut settings, &plugin.manifest.hooks);
        }

        settings
    }

    pub(crate) fn register_tools(
        &self,
        registry: &mut ToolRegistry,
        project_root: &Path,
    ) -> Result<()> {
        let mut tools = Vec::new();
        for plugin in &self.plugins {
            let plugin_name = plugin.manifest.name.clone();
            for tool in &plugin.manifest.tools {
                tools.push(PluginCommandTool::new(
                    plugin_name.clone(),
                    plugin.root.clone(),
                    project_root.to_path_buf(),
                    tool.clone(),
                )?);
            }
        }
        tools.sort_by(|left, right| left.name().cmp(right.name()));

        for tool in tools {
            registry.register(Box::new(tool))?;
        }

        Ok(())
    }

    pub(crate) fn commands(&self, project_root: &Path) -> Result<Vec<PluginCommandContribution>> {
        let mut commands = Vec::new();
        for plugin in &self.plugins {
            for command in &plugin.manifest.commands {
                commands.push(PluginCommandContribution::new(
                    plugin.manifest.name.clone(),
                    plugin.root.clone(),
                    project_root.to_path_buf(),
                    command.clone(),
                )?);
            }
        }
        commands.sort_by(|left, right| left.name.cmp(&right.name));

        Ok(commands)
    }

    pub(crate) fn skill_directories(&self) -> Vec<PathBuf> {
        let mut directories = Vec::new();
        for plugin in &self.plugins {
            directories.extend(plugin.skill_directories());
        }
        directories.sort();
        directories
    }
}

#[derive(Debug)]
pub(crate) struct PluginPackage {
    root: PathBuf,
    manifest: PluginManifest,
}

#[derive(Clone, Debug)]
pub(crate) struct PluginCommandContribution {
    pub(crate) name: String,
    command: String,
    plugin_name: String,
    plugin_root: PathBuf,
    project_root: PathBuf,
}

impl PluginCommandContribution {
    fn new(
        plugin_name: String,
        plugin_root: PathBuf,
        project_root: PathBuf,
        raw: RawPluginCommand,
    ) -> Result<Self> {
        validate_slash_command_name(&raw.name)?;
        Ok(Self {
            name: raw.name,
            command: raw.command,
            plugin_name,
            plugin_root,
            project_root,
        })
    }

    pub(crate) fn run(&self, args: &str) -> Result<String> {
        let output = run_external_command(
            &self.command,
            &self.project_root,
            &self.plugin_root,
            &self.plugin_name,
            Some(("CAWIR_COMMAND_ARGS", args)),
            None,
        )?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(Error::Env(format!(
                "plugin command {} failed: {}",
                self.name,
                command_failure_message(output)
            )))
        }
    }
}

#[derive(Debug, Deserialize)]
struct PluginManifest {
    name: String,
    #[serde(default, rename = "version")]
    _version: Option<String>,
    #[serde(default, rename = "description")]
    _description: Option<String>,
    #[serde(default)]
    commands: Vec<RawPluginCommand>,
    #[serde(default)]
    tools: Vec<RawPluginTool>,
    #[serde(default)]
    hooks: Value,
    #[serde(default = "empty_object")]
    settings: Value,
    #[serde(default)]
    skill_dirs: Vec<String>,
}

impl PluginPackage {
    fn skill_directories(&self) -> Vec<PathBuf> {
        let mut directories = Vec::new();

        let conventional = self.root.join("skills");
        if conventional.is_dir() {
            directories.push(conventional);
        }

        for path in &self.manifest.skill_dirs {
            let path = resolve_configured_path(path, &self.root);
            if let Ok(path) = path.canonicalize()
                && path.is_dir()
            {
                directories.push(path);
            }
        }

        directories.sort();
        directories.dedup();
        directories
    }
}

#[derive(Clone, Debug, Deserialize)]
struct RawPluginCommand {
    name: String,
    #[serde(default, rename = "description")]
    _description: Option<String>,
    command: String,
}

#[derive(Clone, Debug, Deserialize)]
struct RawPluginTool {
    name: String,
    #[serde(default)]
    description: Option<String>,
    command: String,
    #[serde(default = "default_input_schema")]
    input_schema: Value,
}

struct PluginCommandTool {
    registered_name: String,
    plugin_name: String,
    plugin_root: PathBuf,
    project_root: PathBuf,
    original_name: String,
    description: String,
    command: String,
    input_schema: Value,
}

impl PluginCommandTool {
    fn new(
        plugin_name: String,
        plugin_root: PathBuf,
        project_root: PathBuf,
        raw: RawPluginTool,
    ) -> Result<Self> {
        let registered_name = registered_tool_name(&plugin_name, &raw.name)?;
        let description = match raw.description {
            Some(description) if !description.trim().is_empty() => {
                format!(
                    "Plugin tool {}::{}. {}",
                    plugin_name,
                    raw.name,
                    description.trim()
                )
            }
            _ => format!("Plugin tool {}::{}.", plugin_name, raw.name),
        };

        Ok(Self {
            registered_name,
            plugin_name,
            plugin_root,
            project_root,
            original_name: raw.name,
            description,
            command: raw.command,
            input_schema: raw.input_schema,
        })
    }
}

impl Tool for PluginCommandTool {
    fn name(&self) -> &str {
        &self.registered_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::External
    }

    fn prepare(&self, input: &Value, _context: &ToolContext) -> Result<PreparedToolCall> {
        Ok(PreparedToolCall {
            tool_name: self.registered_name.clone(),
            kind: self.kind(),
            approval: Some(ToolApprovalRequest::new(
                self.name(),
                format!(
                    "call plugin tool {}::{} with input {}",
                    self.plugin_name,
                    self.original_name,
                    truncate_summary(&compact_json(input))
                ),
                format!(
                    "user denied plugin tool call {}::{}",
                    self.plugin_name, self.original_name
                ),
            )),
            input: PreparedToolInput::External {
                input: input.clone(),
            },
        })
    }

    fn execute(&self, input: PreparedToolInput) -> Result<ToolOutput> {
        let PreparedToolInput::External { input } = input else {
            return Err(Error::ToolInput {
                tool: self.registered_name.clone(),
                message: "tool received prepared input for a different tool".to_string(),
            });
        };

        let stdin = serde_json::to_vec(&input).map_err(|error| Error::ToolInput {
            tool: self.registered_name.clone(),
            message: format!("failed to serialize plugin tool input: {error}"),
        })?;
        let output = run_external_command(
            &self.command,
            &self.project_root,
            &self.plugin_root,
            &self.plugin_name,
            None,
            Some(&stdin),
        )?;
        let success = output.status.success();
        let content = if success {
            String::from_utf8_lossy(&output.stdout).to_string()
        } else {
            command_failure_message(output)
        };

        Ok(ToolOutput::Result {
            content,
            is_error: !success,
        })
    }
}

fn configured_plugin_directories(settings: &Value, project_root: &Path) -> Result<Vec<PathBuf>> {
    let mut directories = Vec::new();
    if let Some(value) = settings
        .get("plugin_dirs")
        .or_else(|| settings.get("pluginDirs"))
    {
        append_configured_directories(value, project_root, &mut directories)?;
    }
    if let Some(value) = settings
        .get("plugins")
        .and_then(|plugins| plugins.get("directories").or_else(|| plugins.get("dirs")))
    {
        append_configured_directories(value, project_root, &mut directories)?;
    }

    Ok(directories)
}

fn append_configured_directories(
    value: &Value,
    project_root: &Path,
    directories: &mut Vec<PathBuf>,
) -> Result<()> {
    let array = value.as_array().ok_or_else(|| {
        Error::Env("settings.plugins.directories must be an array of paths".to_string())
    })?;

    for value in array {
        let path = value.as_str().ok_or_else(|| {
            Error::Env("settings.plugins.directories entries must be strings".to_string())
        })?;
        let path = resolve_configured_path(path, project_root);
        let path = path.canonicalize().map_err(|error| {
            Error::Env(format!(
                "failed to resolve plugin directory {}: {error}",
                path.display()
            ))
        })?;
        directories.push(path);
    }

    Ok(())
}

fn resolve_configured_path(path: &str, project_root: &Path) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    }
}

fn discover_plugins_in_directory(directory: &Path) -> Result<Vec<PluginPackage>> {
    if directory.join(PLUGIN_MANIFEST_FILE).is_file() {
        return Ok(vec![load_plugin(directory)?]);
    }

    let mut children = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.path().join(PLUGIN_MANIFEST_FILE).is_file() {
            children.push(entry.path());
        }
    }
    children.sort();

    children
        .iter()
        .map(|child| load_plugin(child))
        .collect::<Result<Vec<_>>>()
}

fn load_plugin(root: &Path) -> Result<PluginPackage> {
    let root = root.canonicalize()?;
    let manifest_path = root.join(PLUGIN_MANIFEST_FILE);
    let contents = std::fs::read_to_string(&manifest_path)?;
    let manifest = serde_json::from_str::<PluginManifest>(&contents).map_err(|error| {
        Error::Env(format!(
            "failed to parse plugin manifest {}: {error}",
            manifest_path.display()
        ))
    })?;
    validate_plugin_manifest(&manifest, &manifest_path)?;

    Ok(PluginPackage { root, manifest })
}

fn validate_plugin_manifest(manifest: &PluginManifest, manifest_path: &Path) -> Result<()> {
    if sanitize_segment(&manifest.name).is_empty() {
        return Err(Error::Env(format!(
            "plugin manifest {} has an unusable name: {}",
            manifest_path.display(),
            manifest.name
        )));
    }

    if !manifest.settings.is_object() {
        return Err(Error::Env(format!(
            "plugin manifest {} settings must be an object",
            manifest_path.display()
        )));
    }

    if !manifest.hooks.is_null() && !manifest.hooks.is_object() {
        return Err(Error::Env(format!(
            "plugin manifest {} hooks must be an object keyed by event name",
            manifest_path.display()
        )));
    }

    for command in &manifest.commands {
        validate_slash_command_name(&command.name)?;
    }

    for tool in &manifest.tools {
        if sanitize_segment(&tool.name).is_empty() {
            return Err(Error::Env(format!(
                "plugin {} has an unusable tool name: {}",
                manifest.name, tool.name
            )));
        }
    }

    Ok(())
}

fn validate_slash_command_name(name: &str) -> Result<()> {
    if !name.starts_with('/') || name.split_whitespace().count() != 1 || name == "/" {
        return Err(Error::Env(format!(
            "plugin command name must be a single slash command such as /hello: {name}"
        )));
    }

    Ok(())
}

fn append_plugin_hooks(settings: &mut Value, hooks: &Value) {
    let Some(plugin_hooks) = hooks.as_object() else {
        return;
    };
    let settings = settings
        .as_object_mut()
        .expect("settings starts as an object");
    let hooks_value = settings
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let Some(target_hooks) = hooks_value.as_object_mut() else {
        *hooks_value = Value::Object(serde_json::Map::new());
        let Some(target_hooks) = hooks_value.as_object_mut() else {
            return;
        };
        append_hook_entries(target_hooks, plugin_hooks);
        return;
    };

    append_hook_entries(target_hooks, plugin_hooks);
}

fn append_hook_entries(
    target_hooks: &mut serde_json::Map<String, Value>,
    plugin_hooks: &serde_json::Map<String, Value>,
) {
    for (event_name, plugin_entries) in plugin_hooks {
        let target_entries = target_hooks
            .entry(event_name.clone())
            .or_insert_with(|| Value::Array(Vec::new()));
        match (target_entries.as_array_mut(), plugin_entries.as_array()) {
            (Some(target_entries), Some(plugin_entries)) => {
                target_entries.extend(plugin_entries.iter().cloned());
            }
            _ => {
                *target_entries = plugin_entries.clone();
            }
        }
    }
}

fn run_external_command(
    command_line: &str,
    project_root: &Path,
    plugin_root: &Path,
    plugin_name: &str,
    env_var: Option<(&str, &str)>,
    stdin: Option<&[u8]>,
) -> Result<Output> {
    let mut process = Command::new("sh");
    process
        .arg("-c")
        .arg(command_line)
        .current_dir(project_root)
        .env("CAWIR_PLUGIN_DIR", plugin_root)
        .env("CAWIR_PLUGIN_NAME", plugin_name)
        .env("CAWIR_PROJECT_ROOT", project_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some((name, value)) = env_var {
        process.env(name, value);
    }

    let mut child = process.spawn()?;

    if let Some(stdin_bytes) = stdin {
        let mut child_stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Env("failed to open plugin command stdin".to_string()))?;
        child_stdin.write_all(stdin_bytes)?;
    }
    drop(child.stdin.take());

    child.wait_with_output().map_err(Error::Io)
}

fn command_failure_message(output: Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("command exited with {}", output.status)
    }
}

fn registered_tool_name(plugin_name: &str, tool_name: &str) -> Result<String> {
    let plugin = sanitize_segment(plugin_name);
    let tool = sanitize_segment(tool_name);
    if plugin.is_empty() || tool.is_empty() {
        return Err(Error::Env(format!(
            "plugin {plugin_name} exposed an unusable tool name: {tool_name}"
        )));
    }

    Ok(format!("plugin__{plugin}__{tool}"))
}

fn sanitize_segment(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '_' => character,
            _ => '_',
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

fn default_input_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false
    })
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

fn truncate_summary(value: &str) -> String {
    const MAX_CHARS: usize = 160;
    let mut chars = value.chars();
    let prefix = chars.by_ref().take(MAX_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}...")
    } else {
        prefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        hooks::HookRegistry,
        policy::PermissionMode,
        session::{MessageContent, ToolResult},
        tools::{self, ToolRegistry},
    };
    use serde_json::json;
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn plugin_test_path(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("cawir-plugin-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{}-{}-{}", prefix, std::process::id(), unique))
    }

    fn write_manifest(plugin_root: &std::path::Path, manifest: &str) {
        std::fs::create_dir_all(plugin_root).unwrap();
        std::fs::write(plugin_root.join(PLUGIN_MANIFEST_FILE), manifest).unwrap();
    }

    #[test]
    fn discovers_plugin_manifests_from_configured_directories() {
        let project = plugin_test_path("discover");
        let plugin = project.join("plugins").join("hello");
        write_manifest(
            &plugin,
            r#"{
                "name": "hello",
                "version": "0.1.0",
                "description": "Test plugin",
                "commands": [
                    {
                        "name": "/hello",
                        "description": "Print hello",
                        "command": "printf hello"
                    }
                ]
            }"#,
        );
        let settings = json!({
            "plugins": {
                "directories": ["plugins"]
            }
        });

        let catalog = PluginCatalog::from_settings(&settings, &project).unwrap();

        assert_eq!(catalog.plugins.len(), 1);
        assert_eq!(catalog.plugins[0].manifest.name, "hello");
        assert_eq!(catalog.plugins[0].root, plugin.canonicalize().unwrap());

        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn plugin_settings_merge_snippets_and_append_hooks() {
        let project = plugin_test_path("settings");
        let plugin = project.join("plugins").join("hooks");
        write_manifest(
            &plugin,
            r#"{
                "name": "hooks",
                "settings": {
                    "nested": {
                        "from_plugin": true
                    }
                },
                "hooks": {
                    "pre_tool_use": [
                        {
                            "type": "command",
                            "command": "printf '{\"action\":\"allow\"}'"
                        }
                    ]
                }
            }"#,
        );
        let settings = json!({
            "plugins": {
                "directories": ["plugins"]
            },
            "hooks": {
                "pre_tool_use": [
                    {
                        "type": "command",
                        "command": "printf '{\"action\":\"allow\"}'"
                    }
                ]
            },
            "nested": {
                "from_project": true
            }
        });
        let catalog = PluginCatalog::from_settings(&settings, &project).unwrap();

        let merged = catalog.merged_settings(settings);

        assert_eq!(
            merged,
            json!({
                "plugins": {
                    "directories": ["plugins"]
                },
                "hooks": {
                    "pre_tool_use": [
                        {
                            "type": "command",
                            "command": "printf '{\"action\":\"allow\"}'"
                        },
                        {
                            "type": "command",
                            "command": "printf '{\"action\":\"allow\"}'"
                        }
                    ]
                },
                "nested": {
                    "from_project": true,
                    "from_plugin": true
                }
            })
        );

        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn plugin_command_tool_runs_shell_command_with_json_input() {
        let project = plugin_test_path("tool");
        let plugin = project.join("plugins").join("echo");
        write_manifest(
            &plugin,
            r#"{
                "name": "echo-plugin",
                "tools": [
                    {
                        "name": "echo",
                        "description": "Echo the JSON input",
                        "command": "input=$(cat); printf 'plugin saw %s' \"$input\"",
                        "input_schema": {
                            "type": "object",
                            "properties": {
                                "value": { "type": "string" }
                            },
                            "required": ["value"],
                            "additionalProperties": false
                        }
                    }
                ]
            }"#,
        );
        let settings = json!({
            "plugins": {
                "directories": ["plugins"]
            }
        });
        let catalog = PluginCatalog::from_settings(&settings, &project).unwrap();
        let mut registry = ToolRegistry::builtins();
        catalog.register_tools(&mut registry, &project).unwrap();
        let definitions = registry.definitions(PermissionMode::Default);
        assert!(definitions.iter().any(|definition| {
            definition.name == "plugin__echo_plugin__echo"
                && definition.description == "Plugin tool echo-plugin::echo. Echo the JSON input"
        }));
        let blocks = vec![MessageContent::ToolUse {
            id: "toolu_plugin".to_string(),
            name: "plugin__echo_plugin__echo".to_string(),
            input: json!({ "value": "hello" }),
        }];

        let execution = tools::execute_tool_uses_in_project_with_hooks(
            &registry,
            &HookRegistry::empty(),
            &project,
            &blocks,
            PermissionMode::Bypass,
            |_| {},
        );

        assert_eq!(
            execution.results,
            vec![ToolResult {
                tool_use_id: "toolu_plugin".to_string(),
                content: "plugin saw {\"value\":\"hello\"}".to_string(),
                is_error: false,
            }]
        );

        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn plugin_tools_register_in_stable_name_order() {
        let project = plugin_test_path("tool-order");
        let plugin = project.join("plugins").join("ordered");
        write_manifest(
            &plugin,
            r#"{
                "name": "ordered-plugin",
                "tools": [
                    {
                        "name": "zeta",
                        "command": "printf zeta"
                    },
                    {
                        "name": "alpha",
                        "command": "printf alpha"
                    }
                ]
            }"#,
        );
        let settings = json!({
            "plugins": {
                "directories": ["plugins"]
            }
        });
        let catalog = PluginCatalog::from_settings(&settings, &project).unwrap();
        let mut registry = ToolRegistry::builtins();
        catalog.register_tools(&mut registry, &project).unwrap();

        let plugin_tool_names = registry
            .definitions(PermissionMode::Default)
            .into_iter()
            .map(|definition| definition.name)
            .filter(|name| name.starts_with("plugin__ordered_plugin__"))
            .collect::<Vec<_>>();

        assert_eq!(
            plugin_tool_names,
            vec![
                "plugin__ordered_plugin__alpha".to_string(),
                "plugin__ordered_plugin__zeta".to_string(),
            ]
        );

        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn plugin_catalog_exposes_conventional_skill_directories() {
        let project = plugin_test_path("plugin-skills");
        let plugin = project.join("plugins").join("teacher");
        write_manifest(
            &plugin,
            r#"{
                "name": "teacher"
            }"#,
        );
        std::fs::create_dir_all(plugin.join("skills")).unwrap();
        let settings = json!({
            "plugins": {
                "directories": ["plugins"]
            }
        });
        let catalog = PluginCatalog::from_settings(&settings, &project).unwrap();

        assert_eq!(
            catalog.skill_directories(),
            vec![plugin.join("skills").canonicalize().unwrap()]
        );

        std::fs::remove_dir_all(project).unwrap();
    }
}
