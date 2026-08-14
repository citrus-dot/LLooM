//! Central error type for the LLooM core.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("AI service error: {0}")]
    AiService(String),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("already exists: {0}")]
    Conflict(String),

    #[error("process error: {0}")]
    Process(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    /// HTTP status code appropriate for this error.
    pub fn status_code(&self) -> u16 {
        match self {
            AppError::NotFound(_) => 404,
            AppError::Conflict(_) => 409,
            AppError::InvalidRequest(_) => 400,
            AppError::Db(_)
            | AppError::Io(_)
            | AppError::Json(_)
            | AppError::AiService(_)
            | AppError::Process(_)
            | AppError::Internal(_) => 500,
        }
    }
}

impl From<String> for AppError {
    fn from(s: String) -> Self {
        AppError::Internal(s)
    }
}

impl From<&str> for AppError {
    fn from(s: &str) -> Self {
        AppError::Internal(s.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
