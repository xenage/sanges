use std::io;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, SandboxError>;

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("{context}: {source}")]
    Io { context: String, source: io::Error },
    #[error("{context}: {source}")]
    Json {
        context: String,
        source: serde_json::Error,
    },
    #[error("{context}: unexpected HTTP status {status}: {body}")]
    HttpStatus {
        context: String,
        status: u16,
        body: String,
    },
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("backend failure: {0}")]
    Backend(String),
    #[error("unsupported host: {0}")]
    UnsupportedHost(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("timeout: {0}")]
    Timeout(String),
}

impl SandboxError {
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    pub fn json(context: impl Into<String>, source: serde_json::Error) -> Self {
        Self::Json {
            context: context.into(),
            source,
        }
    }

    pub fn http_status(context: impl Into<String>, status: u16, body: impl Into<String>) -> Self {
        Self::HttpStatus {
            context: context.into(),
            status,
            body: body.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidConfig(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }

    pub fn backend(message: impl Into<String>) -> Self {
        Self::Backend(message.into())
    }

    pub fn protocol(message: impl Into<String>) -> Self {
        Self::Protocol(message.into())
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::Timeout(message.into())
    }
}
