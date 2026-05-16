use std::fmt;

use thiserror::Error;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RetryAfter {
    pub raw: Option<String>,
    pub seconds: Option<u64>,
}

impl fmt::Display for RetryAfter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.seconds, self.raw.as_deref()) {
            (Some(seconds), _) => write!(formatter, "{seconds}s"),
            (None, Some(raw)) => formatter.write_str(raw),
            (None, None) => formatter.write_str("unknown"),
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Env(String),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("{provider} api error {status}: {body}")]
    Api {
        provider: String,
        status: reqwest::StatusCode,
        body: String,
    },

    #[error("{provider} rate limited {status}; retry after {retry_after}: {body}")]
    RateLimited {
        provider: String,
        status: reqwest::StatusCode,
        retry_after: RetryAfter,
        body: String,
    },

    #[error("{0} returned empty content")]
    EmptyContent(String),

    #[error("unknown tool requested: {0}")]
    UnknownTool(String),

    #[error("invalid input for tool {tool}: {message}")]
    ToolInput { tool: String, message: String },

    #[error("tool {tool} denied: {message}")]
    ToolDenied { tool: String, message: String },

    #[error("hook {hook} failed: {message}")]
    Hook { hook: String, message: String },

    #[error("mcp server {server} failed: {message}")]
    Mcp { server: String, message: String },

    #[error("tool loop exceeded {0} rounds")]
    ToolLoopLimitExceeded(usize),
}

pub type Result<T> = std::result::Result<T, Error>;
