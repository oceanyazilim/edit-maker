//! Structured errors with a stable machine code and a user-facing message.
//! Technical details are kept separate so the UI can hide them by default.

use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NotFound,
    UnsupportedFormat,
    InvalidMedia,
    InvalidInput,
    ToolMissing,
    EncoderUnavailable,
    ProcessFailed,
    Cancelled,
    Io,
    Validation,
    Storage,
    Internal,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error, Serialize, Deserialize)]
#[error("{message}")]
pub struct StudioError {
    pub code: ErrorCode,
    /// Human readable, safe to show to end users.
    pub message: String,
    /// Optional technical detail for the advanced diagnostics view.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

pub type StudioResult<T> = Result<T, StudioError>;

impl StudioError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }
    pub fn with_details(mut self, d: impl Into<String>) -> Self {
        self.details = Some(d.into());
        self
    }
    pub fn not_found(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, m)
    }
    pub fn invalid_input(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, m)
    }
    pub fn invalid_media(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidMedia, m)
    }
    pub fn validation(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Validation, m)
    }
    pub fn internal(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, m)
    }
    pub fn cancelled() -> Self {
        Self::new(ErrorCode::Cancelled, "The operation was cancelled.")
    }
    pub fn is_cancelled(&self) -> bool {
        self.code == ErrorCode::Cancelled
    }
}

impl From<std::io::Error> for StudioError {
    fn from(e: std::io::Error) -> Self {
        let code = if e.kind() == std::io::ErrorKind::NotFound {
            ErrorCode::NotFound
        } else {
            ErrorCode::Io
        };
        StudioError::new(code, "A file system operation failed.").with_details(e.to_string())
    }
}

impl From<serde_json::Error> for StudioError {
    fn from(e: serde_json::Error) -> Self {
        StudioError::new(ErrorCode::Validation, "Data could not be parsed.")
            .with_details(e.to_string())
    }
}
