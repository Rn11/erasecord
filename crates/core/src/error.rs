use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("the token is empty or contains invalid characters")]
    InvalidToken,

    #[error("Discord rejected the token (401): it is wrong or has expired")]
    Unauthorized,

    #[error("Discord answered {status}: {message}")]
    Api {
        status: u16,
        /// Discord's JSON error code, e.g. 50013 for "Missing Permissions".
        code: Option<u64>,
        message: String,
    },

    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("unexpected response from Discord: {0}")]
    Decode(#[from] serde_json::Error),

    #[error("{0}")]
    InvalidFilter(String),

    #[error("cancelled")]
    Cancelled,
}
