//! Shared validation helpers for route handlers.
//!
//! These functions parse and validate common query / body parameters and
//! return `AppError` with the specific machine-readable codes defined in
//! docs/plans/11_frontend_backend_consolidation.md §Validation & Error Handling.

use chrono::NaiveDate;
use rust_decimal::Decimal;

use crate::server::error::AppError;

/// Parse an ISO 8601 date string (`YYYY-MM-DD`). Returns `invalid_date` on failure.
pub fn parse_date(s: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| AppError::bad_request(format!("invalid date: {s}"), "invalid_date"))
}

/// Validate that a `YYYY-MM` month string is well-formed.
pub fn parse_month(s: &str) -> Result<String, AppError> {
    let valid = s.len() == 7
        && s.chars().nth(4) == Some('-')
        && s[..4].chars().all(|c| c.is_ascii_digit())
        && s[5..].chars().all(|c| c.is_ascii_digit());
    if valid {
        Ok(s.to_string())
    } else {
        Err(AppError::bad_request(
            format!("invalid month: {s} (expected YYYY-MM)"),
            "invalid_month",
        ))
    }
}

/// Parse a `YYYY` year string.
pub fn parse_year(s: &str) -> Result<String, AppError> {
    let valid = s.len() == 4 && s.chars().all(|c| c.is_ascii_digit());
    if valid {
        Ok(s.to_string())
    } else {
        Err(AppError::bad_request(
            format!("invalid year: {s} (expected YYYY)"),
            "invalid_year",
        ))
    }
}

/// Validate that `start <= end`.
pub fn validate_date_range(start: NaiveDate, end: NaiveDate) -> Result<(), AppError> {
    if start > end {
        return Err(AppError::bad_request(
            format!("start ({start}) must not be after end ({end})"),
            "invalid_date_range",
        ));
    }
    Ok(())
}

/// Validate pagination parameters.
pub fn validate_pagination(page: u32, limit: u32) -> Result<(), AppError> {
    if page < 1 {
        return Err(AppError::bad_request(
            "page must be >= 1",
            "invalid_pagination",
        ));
    }
    if limit < 1 || limit > 200 {
        return Err(AppError::bad_request(
            "limit must be between 1 and 200",
            "invalid_pagination",
        ));
    }
    Ok(())
}

/// Parse and validate a Decimal string. Returns `invalid_decimal` on failure.
pub fn parse_decimal(s: &str) -> Result<Decimal, AppError> {
    s.parse::<Decimal>()
        .map_err(|_| AppError::bad_request(format!("invalid decimal value: {s}"), "invalid_decimal"))
}

/// Ensure a Decimal amount is non-negative. Returns `negative_amount` otherwise.
pub fn require_non_negative(amount: Decimal) -> Result<(), AppError> {
    if amount < Decimal::ZERO {
        return Err(AppError::bad_request(
            "amount must not be negative",
            "negative_amount",
        ));
    }
    Ok(())
}

/// Validate a profile ID: lowercase alphanumeric + hyphens, non-empty.
pub fn validate_profile_id(id: &str) -> Result<(), AppError> {
    if id.is_empty()
        || id.contains(char::is_whitespace)
        || !id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(AppError::bad_request(
            format!(
                "invalid profile id {id:?}: must be non-empty lowercase alphanumeric + hyphens"
            ),
            "invalid_profile_id",
        ));
    }
    Ok(())
}

/// Validate a granularity query string.
pub fn parse_granularity(s: &str) -> Result<crate::model::Granularity, AppError> {
    crate::model::Granularity::parse(s).ok_or_else(|| {
        AppError::bad_request(
            format!("invalid granularity: {s} (expected monthly|quarterly|yearly)"),
            "invalid_granularity",
        )
    })
}

/// Split a comma-separated parameter into a `Vec<String>`, returning `None`
/// if the input is empty or all-whitespace.
pub fn split_csv_param(s: &str) -> Option<Vec<String>> {
    let parts: Vec<String> = s
        .split(',')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts)
    }
}
