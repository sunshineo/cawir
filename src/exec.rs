use std::{
    env,
    io::{self, BufRead, BufReader, Write},
    process::{Command, Stdio},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{Error, Result, events::AgentEvent};

const PROTOCOL_VERSION: u32 = 1;
const CLIENT_NAME: &str = "cawir-exec";
const METHOD_INITIALIZE: &str = "initialize";
const METHOD_SHUTDOWN: &str = "shutdown";
const METHOD_SESSION_NEW: &str = "session/new";
const METHOD_SESSION_RESUME: &str = "session/resume";
const METHOD_TURN_SUBMIT: &str = "turn/submit";
const METHOD_APPROVAL_TOOL: &str = "approval/tool";
const METHOD_APPROVAL_PLAN: &str = "approval/plan";
const METHOD_EVENT: &str = "event";

pub(crate) struct ExecOptions {
    pub(crate) prompt: String,
    pub(crate) provider: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) resume: Option<String>,
    pub(crate) json_output: bool,
    pub(crate) approve: bool,
}

pub(crate) fn run(options: ExecOptions) -> Result<()> {
    let mut child = Command::new(env::current_exe()?)
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    let mut child_stdin = child
        .stdin
        .take()
        .ok_or_else(|| Error::Env("failed to open app-server stdin".to_string()))?;
    let child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Env("failed to open app-server stdout".to_string()))?;
    let mut child_stdout = BufReader::new(child_stdout);
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();

    let client_result = run_client_on_protocol(
        &mut child_stdout,
        &mut child_stdin,
        &mut stdout,
        &mut stderr,
        &options,
    );

    if client_result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait()?;
    client_result?;

    if !status.success() {
        return Err(Error::Env(format!("app-server exited with {status}")));
    }

    Ok(())
}

fn run_client_on_protocol(
    mut reader: impl BufRead,
    writer: &mut impl Write,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    options: &ExecOptions,
) -> Result<()> {
    let mut next_id = 1;
    let mut renderer = ExecRenderer::new(options.json_output);

    request_result(
        &mut reader,
        writer,
        stdout,
        stderr,
        options,
        &mut renderer,
        &mut next_id,
        METHOD_INITIALIZE,
        json!({
            "protocol_version": PROTOCOL_VERSION,
            "client_name": CLIENT_NAME,
            "client_version": env!("CARGO_PKG_VERSION")
        }),
    )?;

    let session_result = if let Some(session_id) = &options.resume {
        request_result(
            &mut reader,
            writer,
            stdout,
            stderr,
            options,
            &mut renderer,
            &mut next_id,
            METHOD_SESSION_RESUME,
            json!({ "session_id": session_id }),
        )?
    } else {
        request_result(
            &mut reader,
            writer,
            stdout,
            stderr,
            options,
            &mut renderer,
            &mut next_id,
            METHOD_SESSION_NEW,
            session_new_params(options),
        )?
    };

    let session_id = required_string(&session_result, "session_id")?;
    let turn_result = request_result(
        &mut reader,
        writer,
        stdout,
        stderr,
        options,
        &mut renderer,
        &mut next_id,
        METHOD_TURN_SUBMIT,
        json!({
            "session_id": session_id,
            "prompt": options.prompt
        }),
    )?;
    renderer.finish_turn(stdout, &turn_result)?;

    request_result(
        &mut reader,
        writer,
        stdout,
        stderr,
        options,
        &mut renderer,
        &mut next_id,
        METHOD_SHUTDOWN,
        json!({}),
    )?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn request_result(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    options: &ExecOptions,
    renderer: &mut ExecRenderer,
    next_id: &mut u64,
    method: &str,
    params: Value,
) -> Result<Value> {
    let id = *next_id;
    *next_id += 1;
    write_json_line(writer, &ClientRequest { id, method, params })?;
    read_until_response(reader, writer, stdout, stderr, options, renderer, id)
}

fn read_until_response(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    options: &ExecOptions,
    renderer: &mut ExecRenderer,
    expected_id: u64,
) -> Result<Value> {
    loop {
        match read_server_message(reader)? {
            ServerMessage::Response(response) => {
                if response.id() != &json!(expected_id) {
                    return Err(Error::Env(format!(
                        "unexpected app-server response id: expected {expected_id}, got {}",
                        compact_json(response.id())
                    )));
                }
                return response.into_result();
            }
            ServerMessage::Request(request) => {
                answer_server_request(writer, stdout, stderr, options, request)?
            }
            ServerMessage::Notification(notification) => {
                renderer.render_notification(stdout, stderr, notification)?
            }
        }
    }
}

fn answer_server_request(
    writer: &mut impl Write,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    options: &ExecOptions,
    request: ServerRequest,
) -> Result<()> {
    match request.method.as_str() {
        METHOD_APPROVAL_TOOL | METHOD_APPROVAL_PLAN => {
            if options.json_output {
                write_json_line(
                    stdout,
                    &json!({
                        "type": "approval",
                        "method": request.method,
                        "approved": options.approve,
                        "params": request.params
                    }),
                )?;
            } else {
                let label = approval_label(&request);
                let decision = if options.approve {
                    "approved"
                } else {
                    "denied"
                };
                writeln!(stderr, "{} {}: {}", request.method, label, decision)?;
            }
            write_json_line(
                writer,
                &ClientResponse {
                    id: request.id,
                    result: json!({ "approved": options.approve }),
                },
            )?;
            Ok(())
        }
        other => Err(Error::Env(format!(
            "app-server sent unsupported request method: {other}"
        ))),
    }
}

fn approval_label(request: &ServerRequest) -> String {
    request
        .params
        .get("tool_name")
        .or_else(|| request.params.get("tool_use_id"))
        .and_then(Value::as_str)
        .unwrap_or("request")
        .to_string()
}

fn session_new_params(options: &ExecOptions) -> Value {
    let mut params = Map::new();
    if let Some(provider) = &options.provider {
        params.insert("provider".to_string(), json!(provider));
    }
    if let Some(model) = &options.model {
        params.insert("model".to_string(), json!(model));
    }
    Value::Object(params)
}

fn required_string(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| Error::Env(format!("app-server result missing string field `{key}`")))
}

fn write_json_line<T: Serialize>(writer: &mut impl Write, value: &T) -> Result<()> {
    serde_json::to_writer(&mut *writer, value).map_err(|error| Error::Env(error.to_string()))?;
    writeln!(writer)?;
    writer.flush()?;
    Ok(())
}

fn read_server_message(reader: &mut impl BufRead) -> Result<ServerMessage> {
    let mut line = String::new();
    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            return Err(Error::Env(
                "app-server closed stdout before responding".to_string(),
            ));
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            continue;
        }

        return serde_json::from_str(trimmed).map_err(|error| Error::Env(error.to_string()));
    }
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

struct ExecRenderer {
    json_output: bool,
    saw_text_delta: bool,
    needs_plain_newline: bool,
}

impl ExecRenderer {
    fn new(json_output: bool) -> Self {
        Self {
            json_output,
            saw_text_delta: false,
            needs_plain_newline: false,
        }
    }

    fn render_notification(
        &mut self,
        stdout: &mut impl Write,
        stderr: &mut impl Write,
        notification: ServerNotification,
    ) -> Result<()> {
        if notification.method != METHOD_EVENT {
            return Ok(());
        }

        let params: EventNotificationParams = serde_json::from_value(notification.params)
            .map_err(|error| Error::Env(error.to_string()))?;
        if self.json_output {
            write_json_line(
                stdout,
                &json!({
                    "type": "event",
                    "session_id": params.session_id,
                    "event": params.event
                }),
            )?;
        } else {
            self.render_plain_event(stdout, stderr, params.event)?;
        }
        Ok(())
    }

    fn render_plain_event(
        &mut self,
        stdout: &mut impl Write,
        stderr: &mut impl Write,
        event: AgentEvent,
    ) -> Result<()> {
        match event {
            AgentEvent::AssistantTextDelta { text, .. } => {
                write!(stdout, "{text}")?;
                stdout.flush()?;
                self.saw_text_delta = true;
                self.needs_plain_newline = true;
            }
            AgentEvent::AssistantText { text, .. } => {
                if self.saw_text_delta {
                    self.saw_text_delta = false;
                } else {
                    writeln!(stdout, "{text}")?;
                    self.needs_plain_newline = false;
                }
            }
            AgentEvent::PreToolUse { name, .. } => {
                self.finish_plain_line(stdout)?;
                writeln!(stderr, "tool {name}: requested")?;
            }
            AgentEvent::PostToolUse {
                name,
                is_error,
                error,
                ..
            } => {
                self.finish_plain_line(stdout)?;
                if is_error {
                    writeln!(
                        stderr,
                        "tool {name}: error{}",
                        error
                            .as_deref()
                            .map(|message| format!(": {message}"))
                            .unwrap_or_default()
                    )?;
                } else {
                    writeln!(stderr, "tool {name}: ok")?;
                }
            }
            AgentEvent::StopFailure { message, .. } => {
                self.finish_plain_line(stdout)?;
                writeln!(stderr, "error: {message}")?;
            }
            _ => {}
        }

        Ok(())
    }

    fn finish_turn(&mut self, stdout: &mut impl Write, turn_result: &Value) -> Result<()> {
        if self.json_output {
            write_json_line(
                stdout,
                &json!({
                    "type": "turn_result",
                    "result": turn_result
                }),
            )
        } else {
            self.finish_plain_line(stdout)
        }
    }

    fn finish_plain_line(&mut self, stdout: &mut impl Write) -> Result<()> {
        if self.needs_plain_newline {
            writeln!(stdout)?;
            self.needs_plain_newline = false;
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct ClientRequest<'a> {
    id: u64,
    method: &'a str,
    params: Value,
}

#[derive(Serialize)]
struct ClientResponse {
    id: Value,
    result: Value,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
enum ServerMessage {
    Response(ServerResponse),
    Request(ServerRequest),
    Notification(ServerNotification),
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
enum ServerResponse {
    Success { id: Value, result: Value },
    Failure { id: Value, error: ProtocolError },
}

impl ServerResponse {
    fn id(&self) -> &Value {
        match self {
            Self::Success { id, .. } | Self::Failure { id, .. } => id,
        }
    }

    fn into_result(self) -> Result<Value> {
        match self {
            Self::Success { result, .. } => Ok(result),
            Self::Failure { error, .. } => Err(Error::Env(format!(
                "app-server error {}: {}",
                error.code, error.message
            ))),
        }
    }
}

#[derive(Debug, Deserialize, PartialEq)]
struct ServerRequest {
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize, PartialEq)]
struct ServerNotification {
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize, PartialEq)]
struct ProtocolError {
    code: i64,
    message: String,
}

#[derive(Debug, Deserialize, PartialEq)]
struct EventNotificationParams {
    session_id: String,
    event: AgentEvent,
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde_json::{Value, json};

    use super::*;

    fn parse_json_lines(output: &[u8]) -> Vec<Value> {
        String::from_utf8(output.to_vec())
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn exec_client_drives_app_server_and_renders_assistant_text() {
        let server_output = r#"{"id":1,"result":{"protocol_version":1,"server_name":"cawir","server_version":"0.1.0","capabilities":{"sessions":true,"turns":true,"approvals":true}}}
{"id":2,"result":{"session_id":"session-1","provider":"ollama","auth_option":"none","model":"qwen3:8b","mode":"default","project_path":"/tmp/cawir","tool_definition_fingerprint":"abc","message_count":0,"warnings":[]}}
{"method":"event","params":{"session_id":"session-1","event":{"type":"assistant_text_delta","provider":"ollama","text":"hello"}}}
{"method":"event","params":{"session_id":"session-1","event":{"type":"assistant_text_delta","provider":"ollama","text":" world"}}}
{"method":"event","params":{"session_id":"session-1","event":{"type":"assistant_text","provider":"ollama","text":"hello world"}}}
{"id":3,"result":{"session_id":"session-1","mode":"default","message_count":2}}
{"id":4,"result":{}}
"#;
        let options = ExecOptions {
            prompt: "hello".to_string(),
            provider: Some("ollama".to_string()),
            model: Some("qwen3:8b".to_string()),
            resume: None,
            json_output: false,
            approve: false,
        };
        let mut app_server_input = Vec::new();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        run_client_on_protocol(
            Cursor::new(server_output),
            &mut app_server_input,
            &mut stdout,
            &mut stderr,
            &options,
        )
        .unwrap();

        assert_eq!(String::from_utf8(stdout).unwrap(), "hello world\n");
        assert!(String::from_utf8(stderr).unwrap().is_empty());
        assert_eq!(
            parse_json_lines(&app_server_input),
            vec![
                json!({
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocol_version": 1,
                        "client_name": "cawir-exec",
                        "client_version": env!("CARGO_PKG_VERSION")
                    }
                }),
                json!({
                    "id": 2,
                    "method": "session/new",
                    "params": {
                        "provider": "ollama",
                        "model": "qwen3:8b"
                    }
                }),
                json!({
                    "id": 3,
                    "method": "turn/submit",
                    "params": {
                        "session_id": "session-1",
                        "prompt": "hello"
                    }
                }),
                json!({
                    "id": 4,
                    "method": "shutdown",
                    "params": {}
                })
            ]
        );
    }

    #[test]
    fn exec_client_answers_app_server_approval_requests() {
        let server_output = r#"{"id":1,"result":{"protocol_version":1,"server_name":"cawir","server_version":"0.1.0","capabilities":{"sessions":true,"turns":true,"approvals":true}}}
{"id":2,"result":{"session_id":"session-1","provider":"ollama","auth_option":"none","model":"qwen3:8b","mode":"default","project_path":"/tmp/cawir","tool_definition_fingerprint":"abc","message_count":0,"warnings":[]}}
{"id":"server-1","method":"approval/tool","params":{"session_id":"session-1","tool_name":"write_file","summary":"write src/main.rs"}}
{"id":3,"result":{"session_id":"session-1","mode":"default","message_count":4}}
{"id":4,"result":{}}
"#;
        let options = ExecOptions {
            prompt: "write a file".to_string(),
            provider: Some("ollama".to_string()),
            model: Some("qwen3:8b".to_string()),
            resume: None,
            json_output: false,
            approve: false,
        };
        let mut app_server_input = Vec::new();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        run_client_on_protocol(
            Cursor::new(server_output),
            &mut app_server_input,
            &mut stdout,
            &mut stderr,
            &options,
        )
        .unwrap();

        assert_eq!(
            parse_json_lines(&app_server_input)[3],
            json!({
                "id": "server-1",
                "result": {
                    "approved": false
                }
            })
        );
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "approval/tool write_file: denied\n"
        );
    }

    #[test]
    fn json_exec_output_keeps_events_and_turn_result_structured() {
        let server_output = r#"{"id":1,"result":{"protocol_version":1,"server_name":"cawir","server_version":"0.1.0","capabilities":{"sessions":true,"turns":true,"approvals":true}}}
{"id":2,"result":{"session_id":"session-1","provider":"ollama","auth_option":"none","model":"qwen3:8b","mode":"default","project_path":"/tmp/cawir","tool_definition_fingerprint":"abc","message_count":0,"warnings":[]}}
{"method":"event","params":{"session_id":"session-1","event":{"type":"assistant_text","provider":"ollama","text":"hello world"}}}
{"id":3,"result":{"session_id":"session-1","mode":"default","message_count":2}}
{"id":4,"result":{}}
"#;
        let options = ExecOptions {
            prompt: "hello".to_string(),
            provider: Some("ollama".to_string()),
            model: Some("qwen3:8b".to_string()),
            resume: None,
            json_output: true,
            approve: false,
        };
        let mut app_server_input = Vec::new();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        run_client_on_protocol(
            Cursor::new(server_output),
            &mut app_server_input,
            &mut stdout,
            &mut stderr,
            &options,
        )
        .unwrap();

        assert!(String::from_utf8(stderr).unwrap().is_empty());
        assert_eq!(
            parse_json_lines(&stdout),
            vec![
                json!({
                    "type": "event",
                    "session_id": "session-1",
                    "event": {
                        "type": "assistant_text",
                        "provider": "ollama",
                        "text": "hello world"
                    }
                }),
                json!({
                    "type": "turn_result",
                    "result": {
                        "session_id": "session-1",
                        "mode": "default",
                        "message_count": 2
                    }
                })
            ]
        );
    }
}
