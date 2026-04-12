//! Unified error type returned by every Axum handler.
//!
//! Handlers return `Result<Json<T>, AppError>`. `AppError` implements
//! `IntoResponse`, so we can `?`-propagate `anyhow::Error`, `rusqlite`
//! errors, or explicit status-typed errors, and they all render as a
//! consistent `{ "error": "..." }` JSON body with the right HTTP code.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    NotFound(String),
    BadRequest(String),
    Unauthorized(String),
    Internal(anyhow::Error),
}

impl AppError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code_slug(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::BadRequest(_) => "bad_request",
            Self::Unauthorized(_) => "unauthorized",
            Self::Internal(_) => "internal",
        }
    }

    fn message(&self) -> String {
        match self {
            Self::NotFound(m) | Self::BadRequest(m) | Self::Unauthorized(m) => m.clone(),
            // Never leak low-level error chains to the network; log them
            // server-side and return a generic string instead.
            Self::Internal(_) => "internal server error".to_string(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if let Self::Internal(err) = &self {
            tracing::error!(error = ?err, "handler failed");
        }
        let status = self.status_code();
        let body = Json(json!({
            "error": self.message(),
            "code": self.code_slug(),
        }));
        (status, body).into_response()
    }
}

// Accept any `anyhow::Error` as an internal error so handlers can use `?`
// freely without wrapping every call site.
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
