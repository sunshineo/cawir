use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    Error, Result,
    policy::ToolKind,
    settings::SettingsResolver,
    tools::{
        PreparedToolCall, PreparedToolInput, Tool, ToolApprovalRequest, ToolContext, ToolOutput,
        ToolRegistry,
    },
};

const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
const MCP_SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &[
    "2024-11-05",
    "2025-03-26",
    "2025-06-18",
    MCP_PROTOCOL_VERSION,
];
const MCP_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MCP_SHUTDOWN_WAIT: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, PartialEq, Eq)]
struct McpServerConfig {
    name: String,
    command: String,
    args: Vec<String>,
    env: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct RawMcpServerConfig {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

impl McpServerConfig {
    fn from_settings(settings: &Value) -> Result<Vec<Self>> {
        let Some(servers) = settings
            .get("mcp_servers")
            .or_else(|| settings.get("mcpServers"))
        else {
            return Ok(Vec::new());
        };

        let servers = servers.as_object().ok_or_else(|| {
            Error::Env("settings.mcp_servers must be an object keyed by server name".to_string())
        })?;
        let mut configs = Vec::new();

        for (name, value) in servers {
            let raw =
                serde_json::from_value::<RawMcpServerConfig>(value.clone()).map_err(|error| {
                    Error::Env(format!("invalid MCP server config for {name}: {error}"))
                })?;
            configs.push(Self {
                name: name.clone(),
                command: raw.command,
                args: raw.args,
                env: raw.env,
            });
        }

        configs.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(configs)
    }
}

pub(crate) fn register_configured_tools(
    registry: &mut ToolRegistry,
    project_root: &Path,
) -> Result<()> {
    let settings = SettingsResolver::for_project(project_root)?.load()?;
    for config in McpServerConfig::from_settings(&settings)? {
        let session = McpServerSession::start(config, project_root)?;
        register_session_tools(registry, Arc::new(Mutex::new(session)))?;
    }

    Ok(())
}

trait RpcConnection: Send {
    fn request(&mut self, method: &str, params: Value) -> Result<Value>;
    fn notify(&mut self, method: &str, params: Option<Value>) -> Result<()>;
}

struct McpServerSession {
    server_name: String,
    connection: Box<dyn RpcConnection>,
}

impl McpServerSession {
    fn start(config: McpServerConfig, project_root: &Path) -> Result<Self> {
        let server_name = config.name.clone();
        let connection = StdioRpcConnection::start(config, project_root)?;
        let mut session = Self::with_connection(&server_name, Box::new(connection));
        session.initialize()?;
        Ok(session)
    }

    fn with_connection(server_name: &str, connection: Box<dyn RpcConnection>) -> Self {
        Self {
            server_name: server_name.to_string(),
            connection,
        }
    }

    fn initialize(&mut self) -> Result<()> {
        let result = self.connection.request(
            "initialize",
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "cawir",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        )?;
        let initialization =
            serde_json::from_value::<InitializeResult>(result).map_err(|error| Error::Mcp {
                server: self.server_name.clone(),
                message: format!("invalid initialize result: {error}"),
            })?;

        if !MCP_SUPPORTED_PROTOCOL_VERSIONS.contains(&initialization.protocol_version.as_str()) {
            return Err(Error::Mcp {
                server: self.server_name.clone(),
                message: format!(
                    "unsupported protocol version {}; supported versions: {}",
                    initialization.protocol_version,
                    MCP_SUPPORTED_PROTOCOL_VERSIONS.join(", ")
                ),
            });
        }

        self.connection.notify("notifications/initialized", None)?;
        Ok(())
    }

    fn list_tools(&mut self) -> Result<Vec<McpToolMetadata>> {
        let mut tools = Vec::new();
        let mut cursor = None;

        loop {
            let params = cursor
                .as_ref()
                .map(|cursor| json!({ "cursor": cursor }))
                .unwrap_or_else(|| json!({}));
            let result = self.connection.request("tools/list", params)?;
            let page =
                serde_json::from_value::<ListToolsResult>(result).map_err(|error| Error::Mcp {
                    server: self.server_name.clone(),
                    message: format!("invalid tools/list result: {error}"),
                })?;
            tools.extend(page.tools);

            let Some(next_cursor) = page.next_cursor else {
                break;
            };
            cursor = Some(next_cursor);
        }

        Ok(tools)
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> Result<McpToolCallOutput> {
        let result = self.connection.request(
            "tools/call",
            json!({
                "name": name,
                "arguments": arguments
            }),
        )?;
        let result =
            serde_json::from_value::<CallToolResult>(result).map_err(|error| Error::Mcp {
                server: self.server_name.clone(),
                message: format!("invalid tools/call result for {name}: {error}"),
            })?;

        Ok(McpToolCallOutput {
            content: format_tool_call_result(&result),
            is_error: result.is_error,
        })
    }
}

#[derive(Deserialize)]
struct InitializeResult {
    #[serde(rename = "protocolVersion")]
    protocol_version: String,
}

#[derive(Clone, Debug, Deserialize)]
struct McpToolMetadata {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default = "default_input_schema", rename = "inputSchema")]
    input_schema: Value,
}

#[derive(Deserialize)]
struct ListToolsResult {
    tools: Vec<McpToolMetadata>,
    #[serde(default, rename = "nextCursor")]
    next_cursor: Option<String>,
}

#[derive(Deserialize)]
struct CallToolResult {
    content: Vec<Value>,
    #[serde(default, rename = "structuredContent")]
    structured_content: Option<Value>,
    #[serde(default, rename = "isError")]
    is_error: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct McpToolCallOutput {
    content: String,
    is_error: bool,
}

fn default_input_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false
    })
}

fn format_tool_call_result(result: &CallToolResult) -> String {
    let mut parts = result
        .content
        .iter()
        .map(format_content_block)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if let Some(structured_content) = &result.structured_content {
        parts.push(format!(
            "structured_content:\n{}",
            compact_json(structured_content)
        ));
    }

    if parts.is_empty() {
        "(empty MCP tool result)".to_string()
    } else {
        parts.join("\n")
    }
}

fn format_content_block(block: &Value) -> String {
    if block.get("type").and_then(Value::as_str) == Some("text")
        && let Some(text) = block.get("text").and_then(Value::as_str)
    {
        return text.to_string();
    }

    compact_json(block)
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

struct StdioRpcConnection {
    server_name: String,
    child: Child,
    stdin: Option<ChildStdin>,
    messages: mpsc::Receiver<Result<Value>>,
    reader_thread: Option<JoinHandle<()>>,
    next_id: u64,
}

impl StdioRpcConnection {
    fn start(config: McpServerConfig, project_root: &Path) -> Result<Self> {
        let mut command = Command::new(&config.command);
        command
            .args(&config.args)
            .envs(&config.env)
            .current_dir(project_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        let mut child = command.spawn().map_err(|error| Error::Mcp {
            server: config.name.clone(),
            message: format!("failed to start `{}`: {error}", config.command),
        })?;
        let stdin = child.stdin.take().ok_or_else(|| Error::Mcp {
            server: config.name.clone(),
            message: "failed to capture server stdin".to_string(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| Error::Mcp {
            server: config.name.clone(),
            message: "failed to capture server stdout".to_string(),
        })?;
        let (sender, messages) = mpsc::channel();
        let server_name = config.name.clone();
        let reader_name = server_name.clone();
        let reader_thread = thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let line = match line {
                    Ok(line) => line,
                    Err(error) => {
                        let _ = sender.send(Err(Error::Mcp {
                            server: reader_name.clone(),
                            message: format!("failed to read stdio response: {error}"),
                        }));
                        break;
                    }
                };
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let message = serde_json::from_str::<Value>(trimmed).map_err(|error| Error::Mcp {
                    server: reader_name.clone(),
                    message: format!("server wrote invalid JSON-RPC message: {error}"),
                });
                if sender.send(message).is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            server_name,
            child,
            stdin: Some(stdin),
            messages,
            reader_thread: Some(reader_thread),
            next_id: 1,
        })
    }

    fn write_message(&mut self, message: &Value) -> Result<()> {
        let server = self.server_name.clone();
        let line = serde_json::to_string(message).map_err(|error| Error::Mcp {
            server: server.clone(),
            message: format!("failed to serialize JSON-RPC message: {error}"),
        })?;
        let stdin = self.stdin.as_mut().ok_or_else(|| Error::Mcp {
            server: server.clone(),
            message: "server stdin is closed".to_string(),
        })?;

        writeln!(stdin, "{line}").map_err(|error| Error::Mcp {
            server: server.clone(),
            message: format!("failed to write JSON-RPC message: {error}"),
        })?;
        stdin.flush().map_err(|error| Error::Mcp {
            server,
            message: format!("failed to flush JSON-RPC message: {error}"),
        })
    }

    fn write_response(&mut self, id: Value, result: Value) -> Result<()> {
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }))
    }

    fn write_error_response(&mut self, id: Value, code: i64, message: &str) -> Result<()> {
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message
            }
        }))
    }

    fn handle_server_request(&mut self, message: &Value) -> Result<()> {
        let Some(id) = message.get("id").cloned() else {
            return Ok(());
        };
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return Ok(());
        };

        if method == "ping" {
            self.write_response(id, json!({}))
        } else {
            self.write_error_response(id, -32601, "method not found")
        }
    }

    fn response_result(&self, message: Value) -> Result<Value> {
        if let Some(error) = message.get("error") {
            return Err(Error::Mcp {
                server: self.server_name.clone(),
                message: format!("JSON-RPC error: {}", compact_json(error)),
            });
        }

        Ok(message.get("result").cloned().unwrap_or(Value::Null))
    }
}

impl RpcConnection for StdioRpcConnection {
    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))?;

        let start = Instant::now();
        loop {
            let elapsed = start.elapsed();
            if elapsed >= MCP_REQUEST_TIMEOUT {
                return Err(Error::Mcp {
                    server: self.server_name.clone(),
                    message: format!("timed out waiting for response to {method}"),
                });
            }

            let remaining = MCP_REQUEST_TIMEOUT - elapsed;
            match self.messages.recv_timeout(remaining) {
                Ok(Ok(message)) => {
                    if message.get("id") == Some(&json!(id)) {
                        return self.response_result(message);
                    }
                    self.handle_server_request(&message)?;
                }
                Ok(Err(error)) => return Err(error),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(Error::Mcp {
                        server: self.server_name.clone(),
                        message: format!("timed out waiting for response to {method}"),
                    });
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(Error::Mcp {
                        server: self.server_name.clone(),
                        message: format!("server closed stdout while waiting for {method}"),
                    });
                }
            }
        }
    }

    fn notify(&mut self, method: &str, params: Option<Value>) -> Result<()> {
        let mut message = json!({
            "jsonrpc": "2.0",
            "method": method
        });
        if let Some(params) = params {
            message["params"] = params;
        }

        self.write_message(&message)
    }
}

impl Drop for StdioRpcConnection {
    fn drop(&mut self) {
        self.stdin.take();

        let start = Instant::now();
        while start.elapsed() < MCP_SHUTDOWN_WAIT {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(_) => break,
            }
        }

        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }

        if let Some(reader_thread) = self.reader_thread.take() {
            let _ = reader_thread.join();
        }
    }
}

struct McpTool {
    registered_name: String,
    server_name: String,
    original_name: String,
    description: String,
    input_schema: Value,
    session: Arc<Mutex<McpServerSession>>,
}

impl McpTool {
    fn new(session: Arc<Mutex<McpServerSession>>, metadata: McpToolMetadata) -> Result<Self> {
        let server_name = lock_session(&session)?.server_name.clone();
        let registered_name = registered_tool_name(&server_name, &metadata.name)?;
        let description = match metadata.description {
            Some(description) if !description.trim().is_empty() => {
                format!(
                    "MCP tool {}::{}. {}",
                    server_name,
                    metadata.name,
                    description.trim()
                )
            }
            _ => format!("MCP tool {}::{}.", server_name, metadata.name),
        };

        Ok(Self {
            registered_name,
            server_name,
            original_name: metadata.name,
            description,
            input_schema: metadata.input_schema,
            session,
        })
    }
}

impl Tool for McpTool {
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
                    "call MCP tool {}::{} with input {}",
                    self.server_name,
                    self.original_name,
                    truncate_summary(&compact_json(input))
                ),
                format!(
                    "user denied MCP tool call {}::{}",
                    self.server_name, self.original_name
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

        let output = lock_session(&self.session)?.call_tool(&self.original_name, input)?;
        Ok(ToolOutput::Result {
            content: output.content,
            is_error: output.is_error,
        })
    }
}

fn register_session_tools(
    registry: &mut ToolRegistry,
    session: Arc<Mutex<McpServerSession>>,
) -> Result<()> {
    let tools = lock_session(&session)?.list_tools()?;
    let mut tools = tools
        .into_iter()
        .map(|metadata| McpTool::new(session.clone(), metadata))
        .collect::<Result<Vec<_>>>()?;
    tools.sort_by(|left, right| left.name().cmp(right.name()));

    for metadata in tools {
        registry.register(Box::new(metadata))?;
    }

    Ok(())
}

fn lock_session(
    session: &Arc<Mutex<McpServerSession>>,
) -> Result<std::sync::MutexGuard<'_, McpServerSession>> {
    session.lock().map_err(|_| Error::Mcp {
        server: "unknown".to_string(),
        message: "MCP session lock poisoned".to_string(),
    })
}

fn registered_tool_name(server_name: &str, tool_name: &str) -> Result<String> {
    let server = sanitize_tool_segment(server_name);
    let tool = sanitize_tool_segment(tool_name);
    if server.is_empty() || tool.is_empty() {
        return Err(Error::Env(format!(
            "MCP server {server_name} exposed an unusable tool name: {tool_name}"
        )));
    }

    Ok(format!("mcp__{server}__{tool}"))
}

fn sanitize_tool_segment(value: &str) -> String {
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
    use crate::{policy::PermissionMode, provider::ToolDefinition, tools::ToolRegistry};
    use serde_json::{Value, json};
    use std::collections::{BTreeMap, VecDeque};

    struct ScriptedConnection {
        requests: Vec<(String, Value)>,
        results: VecDeque<Value>,
        notifications: Vec<String>,
    }

    impl ScriptedConnection {
        fn new(results: Vec<Value>) -> Self {
            Self {
                requests: Vec::new(),
                results: VecDeque::from(results),
                notifications: Vec::new(),
            }
        }
    }

    impl RpcConnection for ScriptedConnection {
        fn request(&mut self, method: &str, params: Value) -> crate::Result<Value> {
            self.requests.push((method.to_string(), params));
            self.results
                .pop_front()
                .ok_or_else(|| crate::Error::Env(format!("missing scripted result for {method}")))
        }

        fn notify(&mut self, method: &str, _params: Option<Value>) -> crate::Result<()> {
            self.notifications.push(method.to_string());
            Ok(())
        }
    }

    #[test]
    fn settings_parse_configured_stdio_servers() {
        let settings = json!({
            "mcp_servers": {
                "local-tools": {
                    "command": "node",
                    "args": ["server.js"],
                    "env": {
                        "TOKEN": "present"
                    }
                }
            }
        });

        let configs = McpServerConfig::from_settings(&settings).unwrap();

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "local-tools");
        assert_eq!(configs[0].command, "node");
        assert_eq!(configs[0].args, vec!["server.js"]);
        assert_eq!(
            configs[0].env.get("TOKEN").map(String::as_str),
            Some("present")
        );
    }

    #[test]
    fn mcp_session_discovers_tools_and_registers_namespaced_definitions() {
        let connection = ScriptedConnection::new(vec![
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "fake", "version": "0.1.0" }
            }),
            json!({
                "tools": [
                    {
                        "name": "zeta",
                        "description": "Last alphabetically",
                        "inputSchema": {
                            "type": "object",
                            "additionalProperties": false
                        }
                    },
                    {
                        "name": "echo",
                        "description": "Echo a value",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "value": { "type": "string" }
                            },
                            "required": ["value"]
                        }
                    }
                ]
            }),
        ]);
        let mut session = McpServerSession::with_connection("fake-server", Box::new(connection));
        session.initialize().unwrap();
        let session = std::sync::Arc::new(std::sync::Mutex::new(session));

        let mut registry = ToolRegistry::builtins();
        register_session_tools(&mut registry, session).unwrap();

        let definitions = registry.definitions(PermissionMode::Default);
        let definition = find_definition(&definitions, "mcp__fake_server__echo");
        assert_eq!(
            definition.description,
            "MCP tool fake-server::echo. Echo a value"
        );
        assert_eq!(
            definition.input_schema,
            json!({
                "type": "object",
                "properties": {
                    "value": { "type": "string" }
                },
                "required": ["value"]
            })
        );

        let mcp_tool_names = definitions
            .iter()
            .map(|definition| definition.name.as_str())
            .filter(|name| name.starts_with("mcp__fake_server__"))
            .collect::<Vec<_>>();
        assert_eq!(
            mcp_tool_names,
            vec!["mcp__fake_server__echo", "mcp__fake_server__zeta"]
        );
    }

    #[test]
    fn mcp_tool_call_formats_result_for_normal_tool_loop() {
        let connection = ScriptedConnection::new(vec![
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "fake", "version": "0.1.0" }
            }),
            json!({
                "content": [
                    { "type": "text", "text": "hello from mcp" }
                ],
                "structuredContent": {
                    "ok": true
                },
                "isError": false
            }),
        ]);
        let mut session = McpServerSession::with_connection("fake-server", Box::new(connection));
        session.initialize().unwrap();
        let output = session
            .call_tool("echo", json!({ "value": "hello" }))
            .unwrap();

        assert!(!output.is_error);
        assert_eq!(
            output.content,
            "hello from mcp\nstructured_content:\n{\"ok\":true}"
        );
    }

    #[test]
    fn initialize_accepts_older_supported_protocol_versions() {
        let connection = ScriptedConnection::new(vec![json!({
            "protocolVersion": "2025-06-18",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "fake", "version": "0.1.0" }
        })]);
        let mut session = McpServerSession::with_connection("fake-server", Box::new(connection));

        session.initialize().unwrap();
    }

    #[test]
    fn stdio_connection_starts_process_and_exchanges_json_rpc_lines() {
        let script = r#"i=0
while IFS= read -r line; do
  i=$((i + 1))
  case "$i" in
    1) printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"0.1.0"}}}' ;;
    3) printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"stdio_echo","description":"Echo over stdio","inputSchema":{"type":"object","properties":{"value":{"type":"string"}},"required":["value"]}}]}}' ;;
    4) printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"stdio ok"}],"isError":false}}' ;;
  esac
done"#;
        let config = McpServerConfig {
            name: "stdio-test".to_string(),
            command: "sh".to_string(),
            args: vec!["-c".to_string(), script.to_string()],
            env: BTreeMap::new(),
        };
        let project_root = std::env::current_dir().unwrap();
        let mut session = McpServerSession::start(config, &project_root).unwrap();

        let tools = session.list_tools().unwrap();
        let output = session
            .call_tool("stdio_echo", json!({ "value": "hello" }))
            .unwrap();

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "stdio_echo");
        assert_eq!(output.content, "stdio ok");
        assert!(!output.is_error);
    }

    fn find_definition<'a>(definitions: &'a [ToolDefinition], name: &str) -> &'a ToolDefinition {
        definitions
            .iter()
            .find(|definition| definition.name == name)
            .unwrap_or_else(|| panic!("missing tool definition {name}"))
    }
}
