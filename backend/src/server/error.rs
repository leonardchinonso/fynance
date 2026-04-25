//! Unified error type returned by every Axum handler.
//!
//! Handlers return `Result<Json<T>, AppError>`. `AppError` implements
//! `IntoResponse`, so we can `?`-propagate `anyhow::Error`, `rusqlite`
//! errors, or explicit status-typed errors, and they all render as a
//! consistent `{ "error": "...", "code": "..." }` JSON body with the right
//! HTTP status code.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    /// 404: resource not found. code = "not_found"
    NotFound(String),
    /// 400: bad input with a specific machine-readable code.
    BadRequest { message: String, code: &'static str },
    /// 409: conflict (e.g. duplicate ID). Specific machine-readable code.
    Conflict { message: String, code: &'static str },
    /// 401: missing or invalid bearer token. code = "unauthorized"
    Unauthorized(String),
    /// 500: unexpected internal failure. Message is NOT forwarded to clients.
    Internal(anyhow::Error),
}

impl AppError {
    /// Construct a 400 error with the given human-readable message and a
    /// specific machine-readable code (e.g. "invalid_date", "invalid_decimal").
    pub fn bad_request(message: impl Into<String>, code: &'static str) -> Self {
        Self::BadRequest {
            message: message.into(),
            code,
        }
    }

    /// Construct a 409 error (duplicate resource) with a specific code.
    pub fn conflict(message: impl Into<String>, code: &'static str) -> Self {
        Self::Conflict {
            message: message.into(),
            code,
        }
    }

    fn status_code(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest { .. } => StatusCode::BAD_REQUEST,
            Self::Conflict { .. } => StatusCode::CONFLICT,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code_str(&self) -> &str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::BadRequest { code, .. } => code,
            Self::Conflict { code, .. } => code,
            Self::Unauthorized(_) => "unauthorized",
            Self::Internal(_) => "internal",
        }
    }

    fn message(&self) -> String {
        match self {
            Self::NotFound(m) | Self::Unauthorized(m) => m.clone(),
            Self::BadRequest { message, .. } | Self::Conflict { message, .. } => message.clone(),
            // Never leak low-level error chains to the network.
            Self::Internal(_) => "internal server error".to_string(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if let Self::Internal(ref err) = self {
            tracing::error!(error = ?err, "handler failed");
        }
        let status = self.status_code();
        let body = Json(json!({
            "error": self.message(),
            "code":  self.code_str(),
        }));
        (status, body).into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        Self::Internal(err)
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Internal(err.into())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::Internal(err.into())
    }
}
