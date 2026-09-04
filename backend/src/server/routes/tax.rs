//! Tax configuration and inputs routes.
//!
//! `GET/PUT /api/tax-config` is the statutory table — the law, identical for
//! every user. `GET/PUT /api/tax-inputs/:profile_id/:tax_year` is one
//! taxpayer's own situation. They are separate endpoints for the same reason
//! they are separate tables: a Budget changes one and never the other. See
//! `db/sql/schema.sql`.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::model::{PutTaxConfigPayload, PutTaxInputsPayload, TaxConfigEntry, TaxInputs};
use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::server::validation::validate_profile_id;

#[derive(Debug, Deserialize)]
pub struct TaxConfigQuery {
    /// `YYYY-YY`. Absent returns every year we hold.
    pub tax_year: Option<String>,
}

// ── GET /api/tax-config ───────────────────────────────────────────────────────

pub async fn get_tax_config(
    State(state): State<AppState>,
    Query(q): Query<TaxConfigQuery>,
) -> Result<Json<Value>, AppError> {
    let entries = {
        let db = state.db();
        match q.tax_year.as_deref().filter(|s| !s.is_empty()) {
            Some(year) => {
                validate_tax_year(year)?;
                db.get_tax_config(year)?
            }
            None => db.get_all_tax_config()?,
        }
    };
    Ok(Json(json!({ "entries": entries })))
}

// ── PUT /api/tax-config ───────────────────────────────────────────────────────

pub async fn put_tax_config(
    State(state): State<AppState>,
    Json(body): Json<PutTaxConfigPayload>,
) -> Result<Json<Value>, AppError> {
    validate_tax_year(&body.tax_year)?;

    let mut entries: Vec<TaxConfigEntry> = Vec::with_capacity(body.entries.len());
    for e in &body.entries {
        let rate_kind = e.rate_kind.clone().unwrap_or_default();
        match e.kind.as_str() {
            "aea" => {
                let amount = e.amount.ok_or_else(|| {
                    AppError::bad_request(
                        "an 'aea' entry must carry an amount",
                        "missing_aea_amount",
                    )
                })?;
                if amount < rust_decimal::Decimal::ZERO {
                    return Err(AppError::bad_request(
                        "the annual exempt amount must not be negative",
                        "negative_aea",
                    ));
                }
            }
            "rate" => {
                let rate = e.rate.ok_or_else(|| {
                    AppError::bad_request("a 'rate' entry must carry a rate", "missing_rate")
                })?;
                // A rate is a fraction, so 24% is 0.24. Rejecting anything above
                // 1 catches the obvious mistake of sending 24, which would
                // otherwise compute a tax bill 100x too large and still look
                // like a plausible number on the page.
                if rate < rust_decimal::Decimal::ZERO || rate > rust_decimal::Decimal::ONE {
                    return Err(AppError::bad_request(
                        "a rate must be a fraction between 0 and 1 (24% is 0.24, not 24)",
                        "rate_out_of_range",
                    ));
                }
                if rate_kind != "basic" && rate_kind != "higher" {
                    return Err(AppError::bad_request(
                        "a 'rate' entry must have rate_kind 'basic' or 'higher'",
                        "invalid_rate_kind",
                    ));
                }
            }
            other => {
                return Err(AppError::bad_request(
                    format!("unknown tax config kind {other:?}; expected 'aea' or 'rate'"),
                    "invalid_tax_config_kind",
                ));
            }
        }

        validate_date(&e.valid_from)?;
        validate_date(&e.valid_to)?;
        if e.valid_to < e.valid_from {
            return Err(AppError::bad_request(
                format!(
                    "valid_to ({}) is before valid_from ({})",
                    e.valid_to, e.valid_from
                ),
                "invalid_validity_range",
            ));
        }

        entries.push(TaxConfigEntry {
            tax_year: body.tax_year.clone(),
            kind: e.kind.clone(),
            rate_kind,
            valid_from: e.valid_from.clone(),
            valid_to: e.valid_to.clone(),
            amount: e.amount,
            rate: e.rate,
            updated_at: None,
        });
    }

    let written = {
        let db = state.db();
        db.put_tax_config(&body.tax_year, &entries)?
    };

    Ok(Json(
        json!({ "tax_year": body.tax_year, "written": written }),
    ))
}

// ── GET /api/tax-inputs/:profile_id/:tax_year ─────────────────────────────────

pub async fn get_tax_inputs(
    State(state): State<AppState>,
    Path((profile_id, tax_year)): Path<(String, String)>,
) -> Result<Json<TaxInputs>, AppError> {
    validate_profile_id(&profile_id)?;
    validate_tax_year(&tax_year)?;

    let inputs = {
        let db = state.db();
        db.get_tax_inputs(&profile_id, &tax_year)?
    };
    Ok(Json(inputs))
}

// ── PUT /api/tax-inputs/:profile_id/:tax_year ─────────────────────────────────

pub async fn put_tax_inputs(
    State(state): State<AppState>,
    Path((profile_id, tax_year)): Path<(String, String)>,
    Json(body): Json<PutTaxInputsPayload>,
) -> Result<Json<TaxInputs>, AppError> {
    validate_profile_id(&profile_id)?;
    validate_tax_year(&tax_year)?;

    let db = state.db();
    if !db.profile_exists(&profile_id)? {
        return Err(AppError::bad_request(
            format!("profile {profile_id} not found"),
            "profile_not_found",
        ));
    }

    // Read-modify-write against the stored row (or its defaults), so an absent
    // key means "leave it alone" rather than "reset it to zero". A PUT that
    // silently zeroed the brought-forward losses because the caller only meant
    // to toggle the AEA would change the tax due without saying so.
    let mut inputs = db.get_tax_inputs(&profile_id, &tax_year)?;

    if let Some(v) = body.brought_forward_losses {
        require_non_negative(v, "brought_forward_losses")?;
        inputs.brought_forward_losses = v;
    }
    if let Some(v) = body.allowable_income_remaining {
        require_non_negative(v, "allowable_income_remaining")?;
        inputs.allowable_income_remaining = v;
    }
    if let Some(v) = body.aea_claimed {
        inputs.aea_claimed = v;
    }

    db.put_tax_inputs(&inputs)?;
    Ok(Json(db.get_tax_inputs(&profile_id, &tax_year)?))
}

// ── Validation helpers ────────────────────────────────────────────────────────

/// A tax year is `YYYY-YY`, where the second half is the year after the first.
/// Validated at entry so a typo becomes a 400 rather than an empty config set
/// that silently computes no tax at all.
pub fn validate_tax_year(s: &str) -> Result<(), AppError> {
    let invalid = || {
        AppError::bad_request(
            format!("tax_year must look like '2024-25', got {s:?}"),
            "invalid_tax_year",
        )
    };

    let (start, end) = s.split_once('-').ok_or_else(invalid)?;
    if start.len() != 4 || end.len() != 2 {
        return Err(invalid());
    }
    let start_year: i32 = start.parse().map_err(|_| invalid())?;
    let end_year: i32 = end.parse().map_err(|_| invalid())?;
    if (start_year + 1) % 100 != end_year {
        return Err(invalid());
    }
    Ok(())
}

fn validate_date(s: &str) -> Result<(), AppError> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
        AppError::bad_request(
            format!("expected a YYYY-MM-DD date, got {s:?}"),
            "invalid_date",
        )
    })?;
    Ok(())
}

fn require_non_negative(v: rust_decimal::Decimal, field: &str) -> Result<(), AppError> {
    if v < rust_decimal::Decimal::ZERO {
        return Err(AppError::bad_request(
            format!("{field} must not be negative"),
            "negative_amount",
        ));
    }
    Ok(())
}
