//! Central error type for the LLooM core.

use thiserror::Error;

/// Provider 调用的结构化错误（L-3）：保留上游状态码与类别。
/// 网关语义：上游 429 → 客户端 429（可退避）、上游 400 → 客户端 400（请求本身问题）、
/// 上游 401/403/5xx 与连接类故障 → 502（我方到上游的链路/凭证问题，非客户端责任）。
#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider authentication failed ({status}): {message}")]
    Auth { status: u16, message: String },
    #[error("provider rate limited ({status}): {message}")]
    RateLimit { status: u16, message: String },
    #[error("provider timeout: {message}")]
    Timeout { message: String },
    #[error("provider connection error: {message}")]
    Connection { message: String },
    #[error("provider invalid request ({status}): {message}")]
    InvalidRequest { status: u16, message: String },
    #[error("provider upstream error ({status}): {message}")]
    Upstream { status: u16, message: String },
    #[error("provider error: {message}")]
    Other { message: String },
}

impl ProviderError {
    /// 上游 HTTP 状态 → 错误类别。
    pub fn from_status(status: u16, message: String) -> Self {
        match status {
            401 | 403 => ProviderError::Auth { status, message },
            408 => ProviderError::Timeout { message },
            429 => ProviderError::RateLimit { status, message },
            400..=499 => ProviderError::InvalidRequest { status, message },
            _ => ProviderError::Upstream { status, message },
        }
    }

    /// 网关转发给下游的状态码。
    pub fn gateway_status(&self) -> u16 {
        match self {
            ProviderError::RateLimit { .. } => 429,
            ProviderError::InvalidRequest { .. } => 400,
            ProviderError::Timeout { .. } => 504,
            ProviderError::Auth { .. }
            | ProviderError::Connection { .. }
            | ProviderError::Upstream { .. }
            | ProviderError::Other { .. } => 502,
        }
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("connection pool error: {0}")]
    Pool(#[from] r2d2::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("AI service error: {0}")]
    AiService(String),

    #[error("{0}")]
    Provider(#[from] ProviderError),

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
            AppError::Provider(e) => e.gateway_status(),
            AppError::Db(_)
            | AppError::Pool(_)
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

#[cfg(test)]
mod provider_error_tests {
    use super::ProviderError;

    #[test]
    fn upstream_status_maps_to_gateway_status() {
        // 429 → 429（客户端可退避）；400 → 400；401/403/5xx → 502；408 → 504。
        let cases: [(u16, u16); 6] = [
            (429, 429),
            (400, 400),
            (401, 502),
            (403, 502),
            (500, 502),
            (503, 502),
        ];
        for (upstream, expected) in cases {
            let e = ProviderError::from_status(upstream, "x".into());
            assert_eq!(e.gateway_status(), expected, "upstream {upstream}");
        }
        assert_eq!(
            ProviderError::Timeout {
                message: "t".into()
            }
            .gateway_status(),
            504
        );
    }

    #[test]
    fn variants_carry_semantics() {
        assert!(matches!(
            ProviderError::from_status(429, "limited".into()),
            ProviderError::RateLimit { .. }
        ));
        assert!(matches!(
            ProviderError::from_status(401, "bad key".into()),
            ProviderError::Auth { .. }
        ));
        assert!(matches!(
            ProviderError::from_status(408, "slow".into()),
            ProviderError::Timeout { .. }
        ));
    }
}
