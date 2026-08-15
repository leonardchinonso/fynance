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
    /// 400 carrying a structured payload alongside the message, merged into the
    /// response body. For errors the client must act on item-by-item rather than
    /// display as prose — the CGT pre-flight lists ~49 missing `(currency, date)`
    /// rates, which is a form to fill in, not a sentence to read.
    BadRequestWithDetails {
        message: String,
        code: &'static str,
        details: serde_json::Value,
    },
    /// 409: conflict (e.g. duplicate ID). Specific machine-readable code.
    Conflict { message: String, code: &'static str },
    /// 401: missing or invalid bearer token. code = "unauthorized"
    Unauthorized(String),
    /// 502: upstream service error with a specific machine-readable code.
    BadGateway { message: String, code: &'static str },
    /// 504: upstream service timed out.
    GatewayTimeout { message: String, code: &'static str },
    /// 429: rate limited by upstream service.
    TooManyRequests { message: String, code: &'static str },
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

    /// Construct a 400 error whose body also carries a structured `details` object,
    /// for clients that must act on the contents rather than render the message.
    pub fn bad_request_with_details(
        message: impl Into<String>,
        code: &'static str,
        details: serde_json::Value,
    ) -> Self {
        Self::BadRequestWithDetails {
            message: message.into(),
            code,
            details,
        }
    }

    /// Construct a 409 error (duplicate resource) with a specific code.
    pub fn conflict(message: impl Into<String>, code: &'static str) -> Self {
        Self::Conflict {
            message: message.into(),
            code,
        }
    }

    pub fn bad_gateway(message: impl Into<String>, code: &'static str) -> Self {
        Self::BadGateway {
            message: message.into(),
            code,
        }
    }

    pub fn gateway_timeout(message: impl Into<String>, code: &'static str) -> Self {
        Self::GatewayTimeout {
            message: message.into(),
            code,
        }
    }

    pub fn too_many_requests(message: impl Into<String>, code: &'static str) -> Self {
        Self::TooManyRequests {
            message: message.into(),
            code,
        }
    }

    fn status_code(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest { .. } | Self::BadRequestWithDetails { .. } => StatusCode::BAD_REQUEST,
            Self::Conflict { .. } => StatusCode::CONFLICT,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::BadGateway { .. } => StatusCode::BAD_GATEWAY,
            Self::GatewayTimeout { .. } => StatusCode::GATEWAY_TIMEOUT,
            Self::TooManyRequests { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code_str(&self) -> &str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::BadRequest { code, .. } => code,
            Self::BadRequestWithDetails { code, .. } => code,
            Self::Conflict { code, .. } => code,
            Self::Unauthorized(_) => "unauthorized",
            Self::BadGateway { code, .. } => code,
            Self::GatewayTimeout { code, .. } => code,
            Self::TooManyRequests { code, .. } => code,
            Self::Internal(_) => "internal",
        }
    }

    fn message(&self) -> String {
        match self {
            Self::NotFound(m) | Self::Unauthorized(m) => m.clone(),
            Self::BadRequest { message, .. }
            | Self::BadRequestWithDetails { message, .. }
            | Self::Conflict { message, .. }
            | Self::BadGateway { message, .. }
            | Self::GatewayTimeout { message, .. }
            | Self::TooManyRequests { message, .. } => message.clone(),
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
        let mut body = json!({
            "error": self.message(),
            "code":  self.code_str(),
        });
        // Merge the structured payload in at the top level, so a client reads
        // e.g. `missing` beside `error`/`code` rather than nested under a key it
        // has to know about. `error` and `code` are written first and the merge
        // skips them, so a details object can never shadow either.
        if let Self::BadRequestWithDetails { details, .. } = &self {
            if let (Some(obj), Some(extra)) = (body.as_object_mut(), details.as_object()) {
                for (k, v) in extra {
                    if k != "error" && k != "code" {
                        obj.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (status, Json(body)).into_response()
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
