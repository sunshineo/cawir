use std::{
    env,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result};

pub(crate) const PROTOCOL_VERSION: u32 = 1;
pub(crate) const METHOD_INITIALIZE: &str = "initialize";
pub(crate) const METHOD_SHUTDOWN: &str = "shutdown";
pub(crate) const METHOD_SESSION_NEW: &str = "session/new";
pub(crate) const METHOD_SESSION_RESUME: &str = "session/resume";
pub(crate) const METHOD_TURN_SUBMIT: &str = "turn/submit";
pub(crate) const METHOD_APPROVAL_TOOL: &str = "approval/tool";
pub(crate) const METHOD_APPROVAL_PLAN: &str = "approval/plan";
pub(crate) const METHOD_EVENT: &str = "event";

pub(crate) struct AppServerProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
}

impl AppServerProcess {
    pub(crate) fn spawn() -> Result<Self> {
        let mut child = Command::new(env::current_exe()?)
            .arg("app-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Env("failed to open app-server stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Env("failed to open app-server stdout".to_string()))?;

        Ok(Self {
            child,
            stdin: Some(stdin),
            stdout: Some(BufReader::new(stdout)),
        })
    }

    pub(crate) fn io_mut(&mut self) -> Result<(&mut BufReader<ChildStdout>, &mut ChildStdin)> {
        let stdout = self
            .stdout
            .as_mut()
            .ok_or_else(|| Error::Env("app-server stdout already taken".to_string()))?;
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| Error::Env("app-server stdin already taken".to_string()))?;
        Ok((stdout, stdin))
    }

    pub(crate) fn take_io(&mut self) -> Result<(BufReader<ChildStdout>, ChildStdin)> {
        let stdout = self
            .stdout
            .take()
            .ok_or_else(|| Error::Env("app-server stdout already taken".to_string()))?;
        let stdin = self
            .stdin
            .take()
            .ok_or_else(|| Error::Env("app-server stdin already taken".to_string()))?;
        Ok((stdout, stdin))
    }

    pub(crate) fn kill(&mut self) -> std::io::Result<()> {
        self.child.kill()
    }

    pub(crate) fn wait(&mut self) -> std::io::Result<ExitStatus> {
        self.child.wait()
    }
}

pub(crate) fn write_request(
    writer: &mut impl Write,
    id: u64,
    method: &str,
    params: Value,
) -> Result<()> {
    write_json_line(writer, &ClientRequest { id, method, params })
}

pub(crate) fn write_response(writer: &mut impl Write, id: Value, result: Value) -> Result<()> {
    write_json_line(writer, &ClientResponse { id, result })
}

pub(crate) fn write_json_line<T: Serialize>(writer: &mut impl Write, value: &T) -> Result<()> {
    serde_json::to_writer(&mut *writer, value).map_err(|error| Error::Env(error.to_string()))?;
    writeln!(writer)?;
    writer.flush()?;
    Ok(())
}

pub(crate) fn read_server_message(reader: &mut impl BufRead) -> Result<ServerMessage> {
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

pub(crate) fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
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
pub(crate) enum ServerMessage {
    Response(ServerResponse),
    Request(ServerRequest),
    Notification(ServerNotification),
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub(crate) enum ServerResponse {
    Success { id: Value, result: Value },
    Failure { id: Value, error: ProtocolError },
}

impl ServerResponse {
    pub(crate) fn id(&self) -> &Value {
        match self {
            Self::Success { id, .. } | Self::Failure { id, .. } => id,
        }
    }

    pub(crate) fn into_result(self) -> Result<Value> {
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
pub(crate) struct ServerRequest {
    pub(crate) id: Value,
    pub(crate) method: String,
    #[serde(default)]
    pub(crate) params: Value,
}

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct ServerNotification {
    pub(crate) method: String,
    #[serde(default)]
    pub(crate) params: Value,
}

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct ProtocolError {
    pub(crate) code: i64,
    pub(crate) message: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct EventNotificationParams {
    pub(crate) session_id: String,
    pub(crate) event: Value,
}
