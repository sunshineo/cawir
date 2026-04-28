use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Env(String),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("anthropic api error {status}: {body}")]
    Api {
        status: reqwest::StatusCode,
        body: String,
    },

    #[error("anthropic returned empty content")]
    EmptyContent,
}

pub type Result<T> = std::result::Result<T, Error>;
