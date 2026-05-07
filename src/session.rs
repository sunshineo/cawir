use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{Error, Result, policy::PermissionMode};

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct Session {
    pub(crate) schema_version: u32,
    pub(crate) id: String,
    pub(crate) provider: String,
    pub(crate) auth_option: String,
    pub(crate) model: String,
    pub(crate) mode: PermissionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) project_path: Option<String>,
    pub(crate) created_at: u64,
    pub(crate) updated_at: u64,
    pub(crate) messages: Vec<Message>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message {
    pub role: String,
    pub content: Vec<MessageContent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type")]
pub enum MessageContent {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "is_false")]
        is_error: bool,
    },
}

impl Message {
    pub fn user_text(text: &str) -> Self {
        Self {
            role: "user".to_string(),
            content: vec![MessageContent::Text {
                text: text.to_string(),
            }],
        }
    }

    pub fn assistant(content: Vec<MessageContent>) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
        }
    }

    pub fn user_tool_result(tool_use_id: String, content: String) -> Self {
        Self {
            role: "user".to_string(),
            content: vec![MessageContent::ToolResult {
                tool_use_id,
                content,
                is_error: false,
            }],
        }
    }

    pub fn user_tool_results(results: Vec<ToolResult>) -> Self {
        Self {
            role: "user".to_string(),
            content: results
                .into_iter()
                .map(
                    |ToolResult {
                         tool_use_id,
                         content,
                         is_error,
                     }| MessageContent::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    },
                )
                .collect(),
        }
    }
}

impl Session {
    pub(crate) fn new(provider: &str, auth_option: &str, model: &str) -> Self {
        let now = unix_now();

        Self {
            schema_version: 1,
            id: new_session_id(),
            provider: provider.to_string(),
            auth_option: auth_option.to_string(),
            model: model.to_string(),
            mode: PermissionMode::Default,
            project_path: current_project_path(),
            created_at: now,
            updated_at: now,
            messages: Vec::new(),
        }
    }
}

pub(crate) fn save_session(session: &mut Session) -> Result<()> {
    session.updated_at = unix_now();
    if session.project_path.is_none() {
        session.project_path = current_project_path();
    }

    let path = session_path(&session.id)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let contents =
        serde_json::to_string_pretty(session).map_err(|error| Error::Env(error.to_string()))?;
    fs::write(path, contents)?;
    Ok(())
}

pub(crate) fn load_session(id: &str) -> Result<Session> {
    reject_path_like_id(id)?;

    let path = session_path(id)?;
    let contents = fs::read_to_string(&path)?;
    serde_json::from_str(&contents)
        .map_err(|error| Error::Env(format!("failed to parse {}: {error}", path.display())))
}

pub(crate) fn load_most_recent_session() -> Result<Option<Session>> {
    Ok(list_resumable_project_sessions()?.into_iter().next())
}

pub(crate) fn list_sessions() -> Result<Vec<Session>> {
    let dir = sessions_dir()?;
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(Error::Io(error)),
    };

    let mut sessions = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("json")
        {
            continue;
        }

        let contents = fs::read_to_string(entry.path())?;
        let session: Session = serde_json::from_str(&contents).map_err(|error| {
            Error::Env(format!(
                "failed to parse session {}: {error}",
                entry.path().display()
            ))
        })?;
        sessions.push(session);
    }

    sessions.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(sessions)
}

pub(crate) fn list_project_sessions() -> Result<Vec<Session>> {
    let Some(project_path) = current_project_path() else {
        return Ok(Vec::new());
    };

    Ok(list_sessions()?
        .into_iter()
        .filter(|session| session.project_path.as_deref() == Some(project_path.as_str()))
        .collect())
}

pub(crate) fn list_resumable_project_sessions() -> Result<Vec<Session>> {
    Ok(list_project_sessions()?
        .into_iter()
        .filter(is_resumable)
        .collect())
}

pub(crate) fn is_resumable(session: &Session) -> bool {
    !session.messages.is_empty()
}

pub(crate) fn sessions_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("dev", "cawir", "cawir")
        .ok_or_else(|| Error::Env("could not determine OS data directory".to_string()))?;

    Ok(dirs.data_dir().join("sessions"))
}

fn session_path(id: &str) -> Result<PathBuf> {
    reject_path_like_id(id)?;
    Ok(sessions_dir()?.join(format!("{id}.json")))
}

fn reject_path_like_id(id: &str) -> Result<()> {
    let path = Path::new(id);
    if id.is_empty() || path.components().count() != 1 {
        return Err(Error::Env(format!("invalid session id: {id}")));
    }

    Ok(())
}

fn new_session_id() -> String {
    Uuid::new_v4().to_string()
}

pub(crate) fn current_project_path() -> Option<String> {
    let path = std::env::current_dir().ok()?;
    Some(
        path.canonicalize()
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned(),
    )
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_tool_result_message_for_anthropic() {
        let message = Message::user_tool_result("toolu_123".to_string(), "Cargo.toml".to_string());
        let serialized = serde_json::to_value(message).unwrap();

        assert_eq!(
            serialized,
            json!({
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_123",
                        "content": "Cargo.toml"
                    }
                ]
            })
        );
    }

    #[test]
    fn serializes_error_tool_result_message_for_anthropic() {
        let message = Message::user_tool_results(vec![ToolResult {
            tool_use_id: "toolu_123".to_string(),
            content: "io error: No such file or directory".to_string(),
            is_error: true,
        }]);
        let serialized = serde_json::to_value(message).unwrap();

        assert_eq!(
            serialized,
            json!({
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_123",
                        "content": "io error: No such file or directory",
                        "is_error": true
                    }
                ]
            })
        );
    }

    #[test]
    fn session_serializes_provider_model_mode_and_messages() {
        let mut session = Session::new("ollama", "none", "qwen3:8b");
        session.id = "session-test".to_string();
        session.project_path = Some("/tmp/cawir".to_string());
        session.messages.push(Message::user_text("hello"));

        let serialized = serde_json::to_value(session).unwrap();

        assert_eq!(serialized["schema_version"], 1);
        assert_eq!(serialized["id"], "session-test");
        assert_eq!(serialized["provider"], "ollama");
        assert_eq!(serialized["auth_option"], "none");
        assert_eq!(serialized["model"], "qwen3:8b");
        assert_eq!(serialized["mode"], "default");
        assert_eq!(serialized["project_path"], "/tmp/cawir");
        assert_eq!(serialized["messages"][0]["role"], "user");
    }

    #[test]
    fn session_deserializes_without_project_path_for_backward_compatibility() {
        let session: Session = serde_json::from_str(
            r#"{
                "schema_version": 1,
                "id": "session-test",
                "provider": "ollama",
                "auth_option": "none",
                "model": "qwen3:8b",
                "mode": "default",
                "created_at": 1,
                "updated_at": 2,
                "messages": []
            }"#,
        )
        .unwrap();

        assert_eq!(session.project_path, None);
    }
}
