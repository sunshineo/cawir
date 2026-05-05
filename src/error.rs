use thiserror::Error;

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

    #[error("{0} returned empty content")]
    EmptyContent(String),

    #[error("unknown tool requested: {0}")]
    UnknownTool(String),

    #[error("invalid input for tool {tool}: {message}")]
    ToolInput { tool: String, message: String },

    #[error("tool {tool} denied: {message}")]
    ToolDenied { tool: String, message: String },

    #[error("tool loop exceeded {0} rounds")]
    ToolLoopLimitExceeded(usize),
}

pub type Result<T> = std::result::Result<T, Error>;
