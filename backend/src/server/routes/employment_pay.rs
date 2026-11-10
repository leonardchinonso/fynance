//! Employment-pay (PAYE) CRUD and the tax-year summary:
//!   GET    /api/employment-pay?tax_year=YYYY-YY
//!   POST   /api/employment-pay
//!   PATCH  /api/employment-pay/:id
//!   DELETE /api/employment-pay/:id
//!   GET    /api/tax/summary/:tax_year
//!
//! Validation lives here, at the HTTP boundary: every payload problem is a
//! 400 with a specific code, so a bad `tax_year` never silently computes an
//! empty year.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::model::{EmploymentPay, EmploymentPayPayload, PatchEmploymentPayBody, TaxYearSummary};
use crate::server::error::AppError;
use crate::server::routes::tax::validate_tax_year;
use crate::server::state::AppState;
use crate::util::fx::FxRateMap;

#[derive(Debug, Deserialize)]
pub struct ListEmploymentPayQuery {
    /// `YYYY-YY`. Absent returns every year.
    pub tax_year: Option<String>,
}

fn parse_iso_date(s: &str) -> Result<(), AppError> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
        AppError::bad_request(
            format!("expected a YYYY-MM-DD date, got {s:?}"),
            "invalid_date",
        )
    })?;
    Ok(())
}

fn require_non_negative(v: Option<Decimal>, field: &str) -> Result<(), AppError> {
    if let Some(v) = v.filter(|v| *v < Decimal::ZERO) {
        return Err(AppError::bad_request(
            format!("{field} must not be negative, got {v}"),
            "negative_amount",
        ));
    }
    Ok(())
}

/// Every rule a POST body must satisfy. `PATCH` applies the same rules
/// field-by-field in its handler, since it only receives what it changes.
fn validate_employment_payload(body: &EmploymentPayPayload) -> Result<(), AppError> {
    validate_tax_year(&body.tax_year)?;
    if body.employer.trim().is_empty() {
        return Err(AppError::bad_request(
            "employer must not be empty",
            "invalid_employer",
        ));
    }
    for d in [body.period_start.as_deref(), body.period_end.as_deref()]
        .into_iter()
        .flatten()
    {
        parse_iso_date(d)?;
    }
    require_non_negative(Some(body.gross_pay), "gross_pay")?;
    require_non_negative(Some(body.tax_deducted), "tax_deducted")?;
    require_non_negative(body.employees_ni, "employees_ni")?;
    require_non_negative(body.student_loan, "student_loan")?;
    require_non_negative(body.net_pay, "net_pay")?;
    Ok(())
}

fn ensure_transaction_exists(state: &AppState, tx_id: &str) -> Result<(), AppError> {
    let db = state.db();
    if !db.transaction_exists(tx_id)? {
        return Err(AppError::bad_request(
            format!("transaction {tx_id} not found"),
            "transaction_not_found",
        ));
    }
    Ok(())
}

// ── GET /api/employment-pay ──────────────────────────────────────────────────

pub async fn list_employment_pay(
    State(state): State<AppState>,
    Query(q): Query<ListEmploymentPayQuery>,
) -> Result<Json<Value>, AppError> {
    let items = {
        let db = state.db();
        match q.tax_year.as_deref().filter(|s| !s.is_empty()) {
            Some(year) => {
                validate_tax_year(year)?;
                db.list_employment_pay(Some(year))?
            }
            None => db.list_employment_pay(None)?,
        }
    };
    Ok(Json(json!({ "items": items })))
}

// ── POST /api/employment-pay ─────────────────────────────────────────────────

pub async fn create_employment_pay(
    State(state): State<AppState>,
    Json(body): Json<EmploymentPayPayload>,
) -> Result<(StatusCode, Json<EmploymentPay>), AppError> {
    validate_employment_payload(&body)?;
    if let Some(tx) = body.transaction_id.as_deref() {
        ensure_transaction_exists(&state, tx)?;
    }

    let row = {
        let db = state.db();
        db.create_employment_pay(&body)?
    };
    Ok((StatusCode::CREATED, Json(row)))
}

// ── PATCH /api/employment-pay/:id ────────────────────────────────────────────

pub async fn update_employment_pay(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchEmploymentPayBody>,
) -> Result<Json<EmploymentPay>, AppError> {
    if body.tax_year.is_none()
        && body.employer.is_none()
        && body.period_start.is_none()
        && body.period_end.is_none()
        && body.is_p60.is_none()
        && body.gross_pay.is_none()
        && body.tax_deducted.is_none()
        && body.employees_ni.is_none()
        && body.student_loan.is_none()
        && body.net_pay.is_none()
        && body.currency.is_none()
        && body.transaction_id.is_none()
        && body.document_ids.is_none()
        && body.notes.is_none()
    {
        return Err(AppError::bad_request(
            "at least one field must be provided",
            "empty_body",
        ));
    }

    if let Some(year) = body.tax_year.as_deref() {
        validate_tax_year(year)?;
    }
    if let Some(employer) = body.employer.as_deref() {
        if employer.trim().is_empty() {
            return Err(AppError::bad_request(
                "employer must not be empty",
                "invalid_employer",
            ));
        }
    }
    for d in [
        body.period_start.as_ref().and_then(|o| o.as_deref()),
        body.period_end.as_ref().and_then(|o| o.as_deref()),
    ]
    .into_iter()
    .flatten()
    {
        parse_iso_date(d)?;
    }
    require_non_negative(body.gross_pay, "gross_pay")?;
    require_non_negative(body.tax_deducted, "tax_deducted")?;
    require_non_negative(body.employees_ni.flatten(), "employees_ni")?;
    require_non_negative(body.student_loan.flatten(), "student_loan")?;
    require_non_negative(body.net_pay.flatten(), "net_pay")?;
    if let Some(currency) = body.currency.as_deref() {
        if currency.trim().is_empty() {
            return Err(AppError::bad_request(
                "currency must not be empty",
                "invalid_currency",
            ));
        }
    }
    if let Some(tx) = body.transaction_id.as_ref().and_then(|o| o.as_deref()) {
        ensure_transaction_exists(&state, tx)?;
    }

    let row = {
        let db = state.db();
        db.update_employment_pay(&id, &body)?
    };
    row.map(Json)
        .ok_or_else(|| AppError::NotFound(format!("employment pay record {id} not found")))
}

// ── DELETE /api/employment-pay/:id ───────────────────────────────────────────

pub async fn delete_employment_pay(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let deleted = {
        let db = state.db();
        db.delete_employment_pay(&id)?
    };
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound(format!(
            "employment pay record {id} not found"
        )))
    }
}

// ── GET /api/tax/summary/:tax_year ───────────────────────────────────────────

pub async fn get_tax_year_summary(
    State(state): State<AppState>,
    Path(tax_year): Path<String>,
) -> Result<Json<TaxYearSummary>, AppError> {
    validate_tax_year(&tax_year)?;
    let db = state.db();
    let fx = FxRateMap::new(db.get_currencies()?)?;
    let summary = db.compute_tax_year_summary(&tax_year, &fx)?;
    Ok(Json(summary))
}
