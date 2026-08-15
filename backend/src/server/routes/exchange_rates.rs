//! Date-keyed exchange rate routes:
//!   GET    /api/exchange-rates
//!   POST   /api/exchange-rates            (bulk upsert)
//!   DELETE /api/exchange-rates/:base/:quote/:date
//!
//! These rates are **user-owned**. Nothing here fetches, interpolates or infers a rate: HMRC
//! mandates no particular source, only that the chosen basis is applied consistently, and the
//! primary use case is reproducing the rates a previously-filed return was computed with. An
//! auto-fill button in the UI may *suggest* a value, but what gets stored is always what the
//! user committed — which is what `source` records.
//!
//! RATE DIRECTION: `rate` is quote-units per ONE base unit, i.e.
//! `amount_in_quote = amount_in_base * rate`. `(USD, GBP, 0.7862)` means $1 = £0.7862.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::model::{CreateExchangeRatesPayload, ExchangeRate};
use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::server::validation::parse_date;

#[derive(Debug, Deserialize)]
pub struct ListExchangeRatesQuery {
    pub base: Option<String>,
    pub quote: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

/// Normalise and validate a date string to `YYYY-MM-DD`.
///
/// Stored dates are compared as strings (both in the `(base, quote, date)` primary key and in
/// the engine's lookups), so an unpadded `2024-4-6` would create a second row that never
/// matches the padded form the engine asks for — a stored rate that silently fails to apply.
fn normalize_date(s: &str) -> Result<String, AppError> {
    Ok(parse_date(s)?.format("%Y-%m-%d").to_string())
}

// ── GET /api/exchange-rates ─────────────────────────────────────────────────

pub async fn list_exchange_rates(
    State(state): State<AppState>,
    Query(q): Query<ListExchangeRatesQuery>,
) -> Result<Json<Vec<ExchangeRate>>, AppError> {
    let start = q
        .start_date
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(parse_date)
        .transpose()?;
    let end = q
        .end_date
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(parse_date)
        .transpose()?;

    let base = q.base.as_deref().map(|s| s.to_uppercase());
    let quote = q.quote.as_deref().map(|s| s.to_uppercase());

    let db = state.db();
    let rates = db.list_exchange_rates(base.as_deref(), quote.as_deref(), start, end)?;
    Ok(Json(rates))
}

// ── POST /api/exchange-rates ────────────────────────────────────────────────

/// Bulk upsert. A single tax-year report needs ~49 rates, so a batch is the normal case and a
/// single rate is just a batch of one. Upsert rather than insert-only because correcting a
/// mistyped rate is routine and the pre-flight screen resubmits the set it was given.
///
/// The whole batch is validated before anything is written, so a typo in the last row does not
/// leave the first forty-eight applied — a half-saved set is worse than a rejected one, because
/// the user cannot see which half landed.
pub async fn create_exchange_rates(
    State(state): State<AppState>,
    Json(payload): Json<CreateExchangeRatesPayload>,
) -> Result<(StatusCode, Json<Vec<ExchangeRate>>), AppError> {
    if payload.rates.is_empty() {
        return Err(AppError::bad_request(
            "at least one rate is required",
            "empty_batch",
        ));
    }

    let db = state.db();
    let currencies = db.get_currencies()?;
    let preferred = currencies
        .iter()
        .find(|c| c.is_preferred)
        .map(|c| c.code.clone())
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("no preferred currency configured")))?;
    let known: std::collections::HashSet<String> =
        currencies.iter().map(|c| c.code.clone()).collect();

    let mut prepared: Vec<ExchangeRate> = Vec::with_capacity(payload.rates.len());
    for input in &payload.rates {
        let base = input.base.to_uppercase();
        let quote = input
            .quote
            .as_deref()
            .map(|q| q.to_uppercase())
            .unwrap_or_else(|| preferred.clone());
        let date = normalize_date(&input.date)?;

        if input.rate <= Decimal::ZERO {
            return Err(AppError::bad_request(
                format!("exchange rate for {base}->{quote} on {date} must be positive"),
                "invalid_rate",
            ));
        }

        // A rate against itself is definitionally 1, and storing anything else would let the
        // engine "convert" GBP into a different number of GBP.
        if base == quote {
            return Err(AppError::bad_request(
                format!(
                    "cannot store an exchange rate from {base} to itself; \
                     a currency always converts to itself at 1"
                ),
                "invalid_rate_pair",
            ));
        }

        // Both sides must be currencies the app knows about, so a typo ('USDD') is rejected on
        // entry rather than being stored as a rate that no lookup will ever match.
        for code in [&base, &quote] {
            if !known.contains(code) {
                return Err(AppError::bad_request(
                    format!(
                        "'{code}' is not a configured currency; \
                         add it under Settings → Currencies first"
                    ),
                    "unknown_currency",
                ));
            }
        }

        let source = match input.source.as_deref() {
            None => "user".to_string(),
            Some(s) if s == "user" || s == "suggested" => s.to_string(),
            Some(other) => {
                return Err(AppError::bad_request(
                    format!("unknown rate source '{other}'; expected 'user' or 'suggested'"),
                    "invalid_source",
                ));
            }
        };

        prepared.push(ExchangeRate {
            base,
            quote,
            date,
            rate: input.rate,
            source,
            updated_at: None,
        });
    }

    db.upsert_exchange_rates(&prepared)?;

    // Read back so the response carries the stored `updated_at` values.
    let stored = db.list_exchange_rates(None, None, None, None)?;
    let echoed: Vec<ExchangeRate> = stored
        .into_iter()
        .filter(|s| {
            prepared
                .iter()
                .any(|p| p.base == s.base && p.quote == s.quote && p.date == s.date)
        })
        .collect();

    Ok((StatusCode::CREATED, Json(echoed)))
}

// ── DELETE /api/exchange-rates/:base/:quote/:date ───────────────────────────

pub async fn delete_exchange_rate(
    State(state): State<AppState>,
    Path((base, quote, date)): Path<(String, String, String)>,
) -> Result<StatusCode, AppError> {
    let base = base.to_uppercase();
    let quote = quote.to_uppercase();
    let date = normalize_date(&date)?;

    let db = state.db();
    if !db.delete_exchange_rate(&base, &quote, &date)? {
        return Err(AppError::NotFound(format!(
            "no exchange rate stored for {base}->{quote} on {date}"
        )));
    }
    Ok(StatusCode::NO_CONTENT)
}
