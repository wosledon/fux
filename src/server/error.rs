use actix_web::{HttpResponse, ResponseError, http::StatusCode};
use std::fmt;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub msg: String,
}

impl ApiError {
    pub fn new(status: StatusCode, msg: impl Into<String>) -> Self {
        Self {
            status,
            msg: msg.into(),
        }
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, msg)
    }
    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, msg)
    }
    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, msg)
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, msg)
    }
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, msg)
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl std::error::Error for ApiError {}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        self.status
    }
    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status).json(serde_json::json!({ "error": self.msg }))
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => ApiError::not_found("资源不存在"),
            other => {
                tracing::error!("db error: {other}");
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("数据库错误: {other}"),
                )
            }
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!("error: {e:#}");
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
    }
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("IO 错误: {e}"))
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
