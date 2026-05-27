use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    io::{self, BufRead, Write},
    rc::Rc,
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{
    Error, Result,
    auth::{ActiveCredential, load_provider_preference},
    events::AgentEvent,
    hooks::HookRegistry,
    provider::Provider,
    runtime::{self, Runtime, SurfaceTurnHooks},
    session::{Session, load_session},
    tools::{PlanReady, ToolApprovalRequest, ToolRegistry},
};

const PROTOCOL_VERSION: u32 = 1;
const SERVER_NAME: &str = "cawir";
const METHOD_INITIALIZE: &str = "initialize";
const METHOD_SHUTDOWN: &str = "shutdown";
const METHOD_SESSION_NEW: &str = "session/new";
const METHOD_SESSION_RESUME: &str = "session/resume";
const METHOD_TURN_SUBMIT: &str = "turn/submit";
const METHOD_APPROVAL_TOOL: &str = "approval/tool";
const METHOD_APPROVAL_PLAN: &str = "approval/plan";
const METHOD_EVENT: &str = "event";

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32000;
const SERVER_ERROR: i64 = -32001;
const UNEXPECTED_RESPONSE: i64 = -32002;
const SESSION_NOT_FOUND: i64 = -32020;
const SESSION_MISMATCH: i64 = -32021;

pub(crate) async fn run_stdio() -> Result<()> {
    runtime::load_dotenv()?;
    let client = runtime::build_http_client()?;
    let stdin = io::stdin();
    let stdout = io::stdout();
    AppServer::new(client)
        .run_stdio_loop(stdin.lock(), stdout.lock())
        .await?;
    Ok(())
}

struct AppServer {
    client: reqwest::Client,
    runtime: Option<Runtime>,
    session: Option<Session>,
    was_loaded_from_disk: bool,
    next_server_request_id: u64,
}

impl AppServer {
    fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            runtime: None,
            session: None,
            was_loaded_from_disk: false,
            next_server_request_id: 1,
        }
    }

    async fn run_stdio_loop(
        mut self,
        mut reader: impl BufRead,
        mut writer: impl Write,
    ) -> io::Result<()> {
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line)?;
            if bytes_read == 0 {
                break;
            }

            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                continue;
            }

            let outcome = self.handle_line(trimmed, &mut reader, &mut writer).await?;
            if let Some(message) = outcome.message {
                write_json_line(&mut writer, &message)?;
            }
            if outcome.exit {
                break;
            }
        }

        Ok(())
    }

    async fn handle_line(
        &mut self,
        line: &str,
        reader: &mut impl BufRead,
        writer: &mut impl Write,
    ) -> io::Result<HandleOutcome> {
        let value = match serde_json::from_str::<Value>(line) {
            Ok(value) => value,
            Err(error) => {
                return Ok(HandleOutcome {
                    message: Some(ServerMessage::error(
                        Value::Null,
                        ProtocolError::parse_error(error),
                    )),
                    exit: false,
                });
            }
        };

        match serde_json::from_value::<ClientMessage>(value.clone()) {
            Ok(ClientMessage::Request(request)) => {
                self.handle_request(request, reader, writer).await
            }
            Ok(ClientMessage::Notification(notification)) => Ok(handle_notification(notification)),
            Ok(ClientMessage::Response(response)) => Ok(HandleOutcome {
                message: Some(ServerMessage::error(
                    response.id().clone(),
                    ProtocolError::unexpected_response(response.id().clone()),
                )),
                exit: false,
            }),
            Err(error) => {
                let id = parse_request_id(&value).unwrap_or(Value::Null);
                Ok(HandleOutcome {
                    message: Some(ServerMessage::error(
                        id,
                        ProtocolError::invalid_request(error),
                    )),
                    exit: false,
                })
            }
        }
    }

    async fn handle_request(
        &mut self,
        request: ClientRequest,
        reader: &mut impl BufRead,
        writer: &mut impl Write,
    ) -> io::Result<HandleOutcome> {
        let exit = request.method == METHOD_SHUTDOWN;
        let message = match request.method.as_str() {
            METHOD_INITIALIZE => initialize_response(request.id, request.params),
            METHOD_SHUTDOWN => ServerMessage::result(request.id, json!({})),
            METHOD_SESSION_NEW => self.session_new_response(request.id, request.params).await,
            METHOD_SESSION_RESUME => {
                self.session_resume_response(request.id, request.params)
                    .await
            }
            METHOD_TURN_SUBMIT => {
                self.turn_submit_response(request.id, request.params, reader, writer)
                    .await?
            }
            _ => ServerMessage::error(request.id, ProtocolError::method_not_found(&request.method)),
        };

        Ok(HandleOutcome {
            message: Some(message),
            exit,
        })
    }

    async fn session_new_response(&mut self, id: Value, params: Value) -> ServerMessage {
        let params = match params_from_value::<SessionNewParams>(params) {
            Ok(params) => params,
            Err(error) => return ServerMessage::error(id, error),
        };

        match self.create_session(params).await {
            Ok(result) => ServerMessage::result(id, result),
            Err(error) => ServerMessage::error(id, ProtocolError::server_error(error)),
        }
    }

    async fn session_resume_response(&mut self, id: Value, params: Value) -> ServerMessage {
        let params = match params_from_value::<SessionResumeParams>(params) {
            Ok(params) => params,
            Err(error) => return ServerMessage::error(id, error),
        };

        match self.resume_session(params).await {
            Ok(result) => ServerMessage::result(id, result),
            Err(error) => ServerMessage::error(id, ProtocolError::server_error(error)),
        }
    }

    async fn turn_submit_response(
        &mut self,
        id: Value,
        params: Value,
        reader: &mut impl BufRead,
        writer: &mut impl Write,
    ) -> io::Result<ServerMessage> {
        let params = match params_from_value::<TurnSubmitParams>(params) {
            Ok(params) => params,
            Err(error) => return Ok(ServerMessage::error(id, error)),
        };

        self.submit_turn(id, params, reader, writer).await
    }
}

fn write_json_line(writer: &mut impl Write, message: &ServerMessage) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, message).map_err(io::Error::other)?;
    writeln!(writer)?;
    writer.flush()
}

impl AppServer {
    async fn create_session(&mut self, params: SessionNewParams) -> Result<Value> {
        let preference = load_provider_preference()?;
        let (provider, credential) = if let Some(provider_name) = params.provider.as_deref() {
            let provider = runtime::provider_by_name(provider_name).map_err(Error::Env)?;
            let credential = runtime::configured_credential_for_provider(
                &provider,
                preference.as_ref(),
                &self.client,
            )
            .await?;
            (provider, credential)
        } else {
            runtime::configured_provider(preference.as_ref(), &self.client).await?
        };

        let model_preferences = preference
            .as_ref()
            .map(|preference| preference.models.clone())
            .unwrap_or_default();
        let model = params.model.unwrap_or_else(|| {
            runtime::model_for_provider(&provider, &credential, &model_preferences)
        });

        let mut runtime =
            build_runtime(provider, credential, model, model_preferences, &self.client);
        let mut session = Session::new(
            runtime.provider.name(),
            runtime.credential.option_name(),
            &runtime.model,
        );
        runtime::load_project_context(&mut runtime, &mut session)?;
        let result = session_result(&session, &runtime, Vec::new());

        self.runtime = Some(runtime);
        self.session = Some(session);
        self.was_loaded_from_disk = false;

        Ok(result)
    }

    async fn resume_session(&mut self, params: SessionResumeParams) -> Result<Value> {
        let mut session = load_session(&params.session_id)?;
        let (provider, credential) =
            runtime::configured_provider_for_session(&session, &self.client).await?;
        let model_preferences = BTreeMap::from([(
            runtime::model_preference_key_parts(&session.provider, &session.auth_option),
            session.model.clone(),
        )]);
        let model = session.model.clone();
        let saved_tool_fingerprint = session.tool_definition_fingerprint.clone();
        let mut runtime =
            build_runtime(provider, credential, model, model_preferences, &self.client);
        runtime::load_project_context(&mut runtime, &mut session)?;
        let warnings = runtime::tool_fingerprint_resume_warning(
            saved_tool_fingerprint.as_deref(),
            &runtime.tool_registry.definition_fingerprint(session.mode)?,
        )
        .into_iter()
        .collect::<Vec<_>>();
        let result = session_result(&session, &runtime, warnings);

        self.runtime = Some(runtime);
        self.session = Some(session);
        self.was_loaded_from_disk = true;

        Ok(result)
    }

    async fn submit_turn(
        &mut self,
        id: Value,
        params: TurnSubmitParams,
        reader: &mut impl BufRead,
        writer: &mut impl Write,
    ) -> io::Result<ServerMessage> {
        let Some(runtime) = self.runtime.as_ref() else {
            return Ok(ServerMessage::error(id, ProtocolError::session_not_found()));
        };
        let Some(session) = self.session.as_mut() else {
            return Ok(ServerMessage::error(id, ProtocolError::session_not_found()));
        };
        if session.id != params.session_id {
            return Ok(ServerMessage::error(
                id,
                ProtocolError::session_mismatch(&params.session_id, &session.id),
            ));
        }

        let session_id = session.id.clone();
        let project_root = match runtime::session_project_path(session) {
            Ok(project_root) => project_root,
            Err(error) => return Ok(ServerMessage::error(id, ProtocolError::server_error(error))),
        };
        let history_len_before_turn = session.messages.len();

        let writer_cell = RefCell::new(writer);
        let reader_cell = RefCell::new(reader);
        let emit_error = Rc::new(RefCell::new(None));
        let next_request_id = Cell::new(self.next_server_request_id);

        let mut emit = {
            let emit_error = Rc::clone(&emit_error);
            let session_id = session_id.clone();
            let writer_cell = &writer_cell;
            move |event: AgentEvent| {
                let message = ServerMessage::notification(
                    METHOD_EVENT,
                    json!({
                        "session_id": session_id,
                        "event": event
                    }),
                );
                if let Err(error) = write_json_line(&mut **writer_cell.borrow_mut(), &message) {
                    *emit_error.borrow_mut() = Some(error);
                }
            }
        };

        crate::agent::submit_user_prompt(&params.prompt, &mut session.messages, &mut emit);

        let mut approve_tool = {
            let reader_cell = &reader_cell;
            let writer_cell = &writer_cell;
            |request: &ToolApprovalRequest| {
                request_approval(
                    reader_cell,
                    writer_cell,
                    &next_request_id,
                    METHOD_APPROVAL_TOOL,
                    json!({
                        "session_id": session_id,
                        "tool_name": request.tool_name(),
                        "summary": request.summary()
                    }),
                )
            }
        };
        let mut approve_plan = {
            let reader_cell = &reader_cell;
            let writer_cell = &writer_cell;
            |plan_ready: &PlanReady| {
                request_approval(
                    reader_cell,
                    writer_cell,
                    &next_request_id,
                    METHOD_APPROVAL_PLAN,
                    json!({
                        "session_id": session_id,
                        "tool_use_id": plan_ready.tool_use_id.clone(),
                        "plan": plan_ready.plan
                    }),
                )
            }
        };
        let mut hooks = SurfaceTurnHooks {
            emit: &mut emit,
            approve_tool: &mut approve_tool,
            approve_plan: &mut approve_plan,
        };

        let turn_result = runtime::run_agent_until_complete(
            runtime,
            project_root,
            &mut session.mode,
            &mut session.messages,
            &params.prompt,
            &mut hooks,
        )
        .await;

        self.next_server_request_id = next_request_id.get();

        if let Some(error) = emit_error.borrow_mut().take() {
            return Err(error);
        }

        match turn_result {
            Ok(()) => {
                if let Err(error) =
                    runtime::sync_session_from_runtime(session, runtime).and_then(|_| {
                        runtime::save_session_if_needed(session, self.was_loaded_from_disk)
                    })
                {
                    return Ok(ServerMessage::error(id, ProtocolError::server_error(error)));
                }
                Ok(ServerMessage::result(
                    id,
                    json!({
                        "session_id": session.id.clone(),
                        "mode": session.mode,
                        "message_count": session.messages.len()
                    }),
                ))
            }
            Err(error) => {
                session.messages.truncate(history_len_before_turn);
                Ok(ServerMessage::error(id, ProtocolError::server_error(error)))
            }
        }
    }
}

fn handle_notification(notification: ClientNotification) -> HandleOutcome {
    let ClientNotification { method, params } = notification;
    let _ = params;
    let exit = method == METHOD_SHUTDOWN;
    HandleOutcome {
        message: None,
        exit,
    }
}

fn build_runtime(
    provider: runtime::ActiveProvider,
    credential: ActiveCredential,
    model: String,
    model_preferences: BTreeMap<String, String>,
    client: &reqwest::Client,
) -> Runtime {
    Runtime {
        provider,
        credential,
        model,
        model_preferences,
        client: client.clone(),
        tool_registry: ToolRegistry::builtins(),
        hook_registry: HookRegistry::empty(),
        skill_catalog: crate::skills::SkillCatalog::empty(),
    }
}

fn session_result(session: &Session, runtime: &Runtime, warnings: Vec<String>) -> Value {
    json!({
        "session_id": session.id.clone(),
        "provider": runtime.provider.name(),
        "auth_option": runtime.credential.option_name(),
        "model": runtime.model.clone(),
        "mode": session.mode,
        "project_path": session.project_path.clone(),
        "tool_definition_fingerprint": session.tool_definition_fingerprint.clone(),
        "message_count": session.messages.len(),
        "warnings": warnings
    })
}

fn request_approval(
    reader_cell: &RefCell<&mut impl BufRead>,
    writer_cell: &RefCell<&mut impl Write>,
    next_request_id: &Cell<u64>,
    method: &str,
    params: Value,
) -> Result<bool> {
    let id = format!("server-{}", next_request_id.get());
    next_request_id.set(next_request_id.get() + 1);
    let message = ServerMessage::request(json!(id), method, params);
    write_json_line(&mut **writer_cell.borrow_mut(), &message)?;

    let response = read_client_response(&mut **reader_cell.borrow_mut())?;
    if response.id() != &json!(id) {
        return Err(Error::Env(format!(
            "approval response id mismatch: expected {id}, got {}",
            compact_json(response.id())
        )));
    }

    let result = response.into_result().map_err(|error| {
        Error::Env(format!("approval request failed: {}", compact_json(&error)))
    })?;
    let approval =
        params_from_value::<ApprovalResult>(result).map_err(|error| Error::Env(error.message))?;
    Ok(approval.approved)
}

fn read_client_response(reader: &mut impl BufRead) -> Result<ClientResponse> {
    let mut line = String::new();
    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            return Err(Error::Env(
                "client closed stdin while app-server waited for approval response".to_string(),
            ));
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            continue;
        }

        let value: Value =
            serde_json::from_str(trimmed).map_err(|error| Error::Env(error.to_string()))?;
        return match serde_json::from_value::<ClientMessage>(value) {
            Ok(ClientMessage::Response(response)) => Ok(response),
            Ok(_) => Err(Error::Env(
                "expected approval response, got request or notification".to_string(),
            )),
            Err(error) => Err(Error::Env(error.to_string())),
        };
    }
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

fn initialize_response(id: Value, params: Value) -> ServerMessage {
    let params = match params_from_value::<InitializeParams>(params) {
        Ok(params) => params,
        Err(error) => return ServerMessage::error(id, error),
    };

    if params.protocol_version != PROTOCOL_VERSION {
        return ServerMessage::error(
            id,
            ProtocolError::unsupported_protocol_version(params.protocol_version),
        );
    }

    let _ = (params.client_name, params.client_version);
    ServerMessage::result(
        id,
        json!({
            "protocol_version": PROTOCOL_VERSION,
            "server_name": SERVER_NAME,
            "server_version": env!("CARGO_PKG_VERSION"),
            "capabilities": {
                "sessions": true,
                "turns": true,
                "approvals": true
            }
        }),
    )
}

fn params_from_value<T>(value: Value) -> std::result::Result<T, ProtocolError>
where
    T: DeserializeOwned,
{
    serde_json::from_value(value).map_err(ProtocolError::invalid_params)
}

fn parse_request_id(value: &Value) -> Option<Value> {
    let object = value.as_object()?;
    object.get("id").cloned()
}

struct HandleOutcome {
    message: Option<ServerMessage>,
    exit: bool,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
enum ClientMessage {
    Request(ClientRequest),
    Notification(ClientNotification),
    Response(ClientResponse),
}

#[derive(Debug, Deserialize, PartialEq)]
struct ClientRequest {
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize, PartialEq)]
struct ClientNotification {
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
enum ClientResponse {
    Success { id: Value, result: Value },
    Failure { id: Value, error: Value },
}

impl ClientResponse {
    fn id(&self) -> &Value {
        match self {
            Self::Success { id, .. } | Self::Failure { id, .. } => id,
        }
    }

    fn into_result(self) -> std::result::Result<Value, Value> {
        match self {
            Self::Success { result, .. } => Ok(result),
            Self::Failure { error, .. } => Err(error),
        }
    }
}

#[derive(Debug, Deserialize, PartialEq)]
struct InitializeParams {
    protocol_version: u32,
    client_name: Option<String>,
    client_version: Option<String>,
}

#[derive(Debug, Default, Deserialize, PartialEq)]
struct SessionNewParams {
    provider: Option<String>,
    model: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct SessionResumeParams {
    session_id: String,
}

#[derive(Debug, Deserialize, PartialEq)]
struct TurnSubmitParams {
    session_id: String,
    prompt: String,
}

#[derive(Debug, Deserialize, PartialEq)]
struct ApprovalResult {
    approved: bool,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(untagged)]
enum ServerMessage {
    Response(ServerResponse),
    Request(ServerRequest),
    Notification(ServerNotification),
}

impl ServerMessage {
    fn result(id: Value, result: Value) -> Self {
        Self::Response(ServerResponse {
            id,
            result: Some(result),
            error: None,
        })
    }

    fn error(id: Value, error: ProtocolError) -> Self {
        Self::Response(ServerResponse {
            id,
            result: None,
            error: Some(error),
        })
    }

    fn request(id: Value, method: impl Into<String>, params: Value) -> Self {
        Self::Request(ServerRequest {
            id,
            method: method.into(),
            params,
        })
    }

    fn notification(method: impl Into<String>, params: Value) -> Self {
        Self::Notification(ServerNotification {
            method: method.into(),
            params,
        })
    }
}

#[derive(Debug, PartialEq, Serialize)]
struct ServerResponse {
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ProtocolError>,
}

#[derive(Debug, PartialEq, Serialize)]
struct ServerRequest {
    id: Value,
    method: String,
    params: Value,
}

#[derive(Debug, PartialEq, Serialize)]
struct ServerNotification {
    method: String,
    params: Value,
}

#[derive(Debug, PartialEq, Serialize)]
struct ProtocolError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl ProtocolError {
    fn parse_error(error: serde_json::Error) -> Self {
        Self {
            code: PARSE_ERROR,
            message: "parse error".to_string(),
            data: Some(json!({ "message": error.to_string() })),
        }
    }

    fn invalid_request(error: serde_json::Error) -> Self {
        Self {
            code: INVALID_REQUEST,
            message: "invalid request".to_string(),
            data: Some(json!({ "message": error.to_string() })),
        }
    }

    fn invalid_params(error: serde_json::Error) -> Self {
        Self {
            code: INVALID_PARAMS,
            message: "invalid params".to_string(),
            data: Some(json!({ "message": error.to_string() })),
        }
    }

    fn method_not_found(method: &str) -> Self {
        Self {
            code: METHOD_NOT_FOUND,
            message: "method not found".to_string(),
            data: Some(json!({ "method": method })),
        }
    }

    fn unsupported_protocol_version(actual: u32) -> Self {
        Self {
            code: UNSUPPORTED_PROTOCOL_VERSION,
            message: "unsupported protocol version".to_string(),
            data: Some(json!({
                "actual": actual,
                "supported": PROTOCOL_VERSION
            })),
        }
    }

    fn server_error(error: Error) -> Self {
        Self {
            code: SERVER_ERROR,
            message: error.to_string(),
            data: None,
        }
    }

    fn unexpected_response(id: Value) -> Self {
        Self {
            code: UNEXPECTED_RESPONSE,
            message: "unexpected client response".to_string(),
            data: Some(json!({ "id": id })),
        }
    }

    fn session_not_found() -> Self {
        Self {
            code: SESSION_NOT_FOUND,
            message: "no active session".to_string(),
            data: None,
        }
    }

    fn session_mismatch(requested: &str, active: &str) -> Self {
        Self {
            code: SESSION_MISMATCH,
            message: "request session_id does not match active session".to_string(),
            data: Some(json!({
                "requested": requested,
                "active": active
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    async fn run_protocol(input: &str) -> Vec<Value> {
        let mut output = Vec::new();
        AppServer::new(reqwest::Client::new())
            .run_stdio_loop(Cursor::new(input), &mut output)
            .await
            .unwrap();
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn initialize_returns_protocol_capabilities() {
        let messages = run_protocol(
            r#"{"id":1,"method":"initialize","params":{"protocol_version":1,"client_name":"test","client_version":"0.1"}}"#,
        )
        .await;

        assert_eq!(
            messages,
            vec![json!({
                "id": 1,
                "result": {
                    "protocol_version": 1,
                    "server_name": "cawir",
                    "server_version": env!("CARGO_PKG_VERSION"),
                    "capabilities": {
                        "sessions": true,
                        "turns": true,
                        "approvals": true
                    }
                }
            })]
        );
    }

    #[tokio::test]
    async fn unsupported_protocol_version_is_structured_error() {
        let messages = run_protocol(
            r#"{"id":"init-1","method":"initialize","params":{"protocol_version":2}}"#,
        )
        .await;

        assert_eq!(
            messages,
            vec![json!({
                "id": "init-1",
                "error": {
                    "code": -32000,
                    "message": "unsupported protocol version",
                    "data": {
                        "actual": 2,
                        "supported": 1
                    }
                }
            })]
        );
    }

    #[tokio::test]
    async fn notifications_do_not_write_responses() {
        let messages = run_protocol(r#"{"method":"initialized","params":{}}"#).await;

        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn unknown_method_returns_method_not_found() {
        let messages = run_protocol(r#"{"id":7,"method":"unknown","params":{}}"#).await;

        assert_eq!(
            messages,
            vec![json!({
                "id": 7,
                "error": {
                    "code": -32601,
                    "message": "method not found",
                    "data": {
                        "method": "unknown"
                    }
                }
            })]
        );
    }

    #[tokio::test]
    async fn malformed_json_returns_parse_error_with_null_id() {
        let messages = run_protocol(r#"{"id":"bad","method":"initialize","params":"#).await;

        assert_eq!(messages[0]["id"], json!(null));
        assert_eq!(messages[0]["error"]["code"], json!(-32700));
    }

    #[tokio::test]
    async fn invalid_request_preserves_request_id_when_possible() {
        let messages = run_protocol(r#"{"id":"bad","params":{}}"#).await;

        assert_eq!(messages[0]["id"], json!("bad"));
        assert_eq!(messages[0]["error"]["code"], json!(-32600));
    }

    #[tokio::test]
    async fn session_new_can_create_ollama_session_without_credentials() {
        let messages = run_protocol(
            r#"{"id":1,"method":"session/new","params":{"provider":"ollama","model":"qwen3:8b"}}"#,
        )
        .await;

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["id"], json!(1));
        assert!(messages[0]["result"]["session_id"].is_string());
        assert_eq!(messages[0]["result"]["provider"], json!("ollama"));
        assert_eq!(messages[0]["result"]["auth_option"], json!("none"));
        assert_eq!(messages[0]["result"]["model"], json!("qwen3:8b"));
        assert_eq!(messages[0]["result"]["message_count"], json!(0));
    }

    #[tokio::test]
    async fn turn_submit_requires_an_active_session() {
        let messages = run_protocol(
            r#"{"id":3,"method":"turn/submit","params":{"session_id":"s","prompt":"hi"}}"#,
        )
        .await;

        assert_eq!(
            messages,
            vec![json!({
                "id": 3,
                "error": {
                    "code": -32020,
                    "message": "no active session"
                }
            })]
        );
    }

    #[tokio::test]
    async fn turn_submit_requires_the_active_session_id() {
        let messages = run_protocol(
            r#"{"id":1,"method":"session/new","params":{"provider":"ollama","model":"qwen3:8b"}}
{"id":2,"method":"turn/submit","params":{"session_id":"wrong","prompt":"hi"}}"#,
        )
        .await;

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1]["id"], json!(2));
        assert_eq!(messages[1]["error"]["code"], json!(-32021));
        assert_eq!(messages[1]["error"]["data"]["requested"], json!("wrong"));
        assert_eq!(
            messages[1]["error"]["data"]["active"],
            messages[0]["result"]["session_id"]
        );
    }

    #[tokio::test]
    async fn client_response_outside_approval_is_structured_error() {
        let messages = run_protocol(r#"{"id":"server-1","result":{"approved":true}}"#).await;

        assert_eq!(
            messages,
            vec![json!({
                "id": "server-1",
                "error": {
                    "code": -32002,
                    "message": "unexpected client response",
                    "data": {
                        "id": "server-1"
                    }
                }
            })]
        );
    }

    #[tokio::test]
    async fn shutdown_returns_response_then_stops_reading() {
        let messages = run_protocol(
            r#"{"id":1,"method":"shutdown","params":{}}
{"id":2,"method":"unknown","params":{}}"#,
        )
        .await;

        assert_eq!(
            messages,
            vec![json!({
                "id": 1,
                "result": {}
            })]
        );
    }
}
