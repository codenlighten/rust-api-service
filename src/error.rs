//! Typed API errors, rendered as RFC 9457 `application/problem+json`.

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FieldError {
    pub field: &'static str,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("request validation failed")]
    Validation(Vec<FieldError>),
    /// A body or query string the extractors could not parse; the status comes from axum.
    #[error("{detail}")]
    Rejected { status: StatusCode, detail: String },
    #[error("{0}")]
    Unauthorized(&'static str),
    #[error("resource not found")]
    NotFound,
    #[error("method not allowed")]
    MethodNotAllowed,
    #[error("{0}")]
    Conflict(&'static str),
    #[error("database unavailable")]
    Unavailable,
    #[error("internal error: {0}")]
    Internal(String),
}

impl ApiError {
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Rejected { status, .. } => *status,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// The text clients see. Internal errors are logged, never echoed.
    fn detail(&self) -> String {
        match self {
            Self::Internal(_) => {
                "an internal error occurred; retry, and report the request id if it persists".into()
            }
            Self::Validation(_) => "one or more fields are invalid; see 'errors'".into(),
            other => other.to_string(),
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed | sqlx::Error::Io(_) => {
                tracing::error!(error = %err, "database unavailable");
                Self::Unavailable
            }
            other => {
                tracing::error!(error = %other, "database error");
                Self::Internal(other.to_string())
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let mut body = json!({
            "type": "about:blank",
            "title": status.canonical_reason().unwrap_or("Error"),
            "status": status.as_u16(),
            "detail": self.detail(),
        });
        if let Self::Validation(errors) = &self {
            body["errors"] = json!(errors);
        }

        let mut response = (status, body.to_string()).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        if matches!(self, Self::Unauthorized(_)) {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}
