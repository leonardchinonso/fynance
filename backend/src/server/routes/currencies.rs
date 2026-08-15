//! Currency management routes:
//!   GET    /api/currencies
//!   POST   /api/currencies
//!   PATCH  /api/currencies/:code
//!   DELETE /api/currencies/:code

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use rust_decimal::Decimal;

use crate::model::{CreateCurrencyPayload, Currency, PatchCurrencyPayload};
use crate::server::error::AppError;
use crate::server::state::AppState;

/// Subset of ISO 4217 codes accepted by POST /api/currencies.
///
/// Broker sub-unit codes (GBX, USX, ZAC, ILA) are deliberately NOT in this
/// list: every write path now converts a sub-unit price/amount to its parent
/// currency at import/write time (`create_investment_event`,
/// `HoldingWrite::into_holding`, `insert_transactions_bulk`,
/// `Transaction::from_unified`), so a sub-unit code is never itself persisted
/// and never needs to be a "configured" currency. See `util::subunits` for
/// the conversion table.
const VALID_ISO_CODES: &[&str] = &[
    "AED", "AFN", "ALL", "AMD", "ANG", "AOA", "ARS", "AUD", "AWG", "AZN", "BAM", "BBD", "BDT",
    "BGN", "BHD", "BIF", "BMD", "BND", "BOB", "BRL", "BSD", "BTN", "BWP", "BYN", "BZD", "CAD",
    "CDF", "CHF", "CLP", "CNY", "COP", "CRC", "CUP", "CVE", "CZK", "DJF", "DKK", "DOP", "DZD",
    "EGP", "ERN", "ETB", "EUR", "FJD", "FKP", "GBP", "GEL", "GHS", "GIP", "GMD", "GNF", "GTQ",
    "GYD", "HKD", "HNL", "HTG", "HUF", "IDR", "ILS", "INR", "IQD", "IRR", "ISK", "JMD", "JOD",
    "JPY", "KES", "KGS", "KHR", "KMF", "KPW", "KRW", "KWD", "KYD", "KZT", "LAK", "LBP", "LKR",
    "LRD", "LSL", "LYD", "MAD", "MDL", "MGA", "MKD", "MMK", "MNT", "MOP", "MRU", "MUR", "MVR",
    "MWK", "MXN", "MYR", "MZN", "NAD", "NGN", "NIO", "NOK", "NPR", "NZD", "OMR", "PAB", "PEN",
    "PGK", "PHP", "PKR", "PLN", "PYG", "QAR", "RON", "RSD", "RUB", "RWF", "SAR", "SBD", "SCR",
    "SDG", "SEK", "SGD", "SHP", "SLE", "SOS", "SRD", "SSP", "STN", "SYP", "SZL", "THB", "TJS",
    "TMT", "TND", "TOP", "TRY", "TTD", "TWD", "TZS", "UAH", "UGX", "USD", "UYU", "UZS", "VES",
    "VND", "VUV", "WST", "XAF", "XCD", "XOF", "XPF", "YER", "ZAR", "ZMW", "ZWL",
];

fn is_valid_iso_code(code: &str) -> bool {
    VALID_ISO_CODES.contains(&code)
}

// ── GET /api/currencies ─────────────────────────────────────────────────────

pub async fn list_currencies(
    State(state): State<AppState>,
) -> Result<Json<Vec<Currency>>, AppError> {
    let db = state.db();
    let currencies = db.get_currencies()?;
    Ok(Json(currencies))
}

// ── POST /api/currencies ────────────────────────────────────────────────────

pub async fn create_currency(
    State(state): State<AppState>,
    Json(payload): Json<CreateCurrencyPayload>,
) -> Result<(StatusCode, Json<Currency>), AppError> {
    let code = payload.code.to_uppercase();

    if !is_valid_iso_code(&code) {
        return Err(AppError::bad_request(
            format!("'{code}' is not a valid ISO 4217 currency code"),
            "invalid_currency_code",
        ));
    }

    if payload.fx_rate <= Decimal::ZERO {
        return Err(AppError::bad_request(
            "exchange rate must be positive",
            "invalid_rate",
        ));
    }

    let db = state.db();

    if db.currency_exists(&code)? {
        return Err(AppError::conflict(
            format!("currency '{code}' already exists"),
            "currency_exists",
        ));
    }

    let currency = db.create_currency(&code, payload.fx_rate)?;
    Ok((StatusCode::CREATED, Json(currency)))
}

// ── PATCH /api/currencies/:code ─────────────────────────────────────────────

pub async fn update_currency(
    State(state): State<AppState>,
    Path(code): Path<String>,
    Json(payload): Json<PatchCurrencyPayload>,
) -> Result<Json<Currency>, AppError> {
    if payload.fx_rate.is_none() && payload.is_preferred.is_none() {
        return Err(AppError::bad_request(
            "at least one of 'fx_rate' or 'is_preferred' is required",
            "empty_patch",
        ));
    }

    if let Some(ref rate) = payload.fx_rate {
        if *rate <= Decimal::ZERO {
            return Err(AppError::bad_request(
                "exchange rate must be positive",
                "invalid_rate",
            ));
        }
    }

    let db = state.db();

    // Transfer preferred status first (if requested).
    if let Some(true) = payload.is_preferred {
        db.set_preferred_currency(&code).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("not found") {
                AppError::NotFound(msg)
            } else {
                AppError::Internal(e)
            }
        })?;
    }

    // Update rate (if provided). Rejected for the preferred currency.
    if let Some(rate) = payload.fx_rate {
        db.update_currency_rate(&code, rate).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("preferred") {
                AppError::bad_request(msg, "cannot_update_preferred_rate")
            } else if msg.contains("not found") {
                AppError::NotFound(msg)
            } else {
                AppError::Internal(e)
            }
        })?;
    }

    // Return the current state of the currency.
    let currencies = db.get_currencies()?;
    let currency = currencies
        .into_iter()
        .find(|c| c.code == code)
        .ok_or_else(|| AppError::NotFound(format!("currency '{code}' not found")))?;

    Ok(Json(currency))
}

// ── DELETE /api/currencies/:code ────────────────────────────────────────────

pub async fn delete_currency(
    State(state): State<AppState>,
    Path(code): Path<String>,
) -> Result<StatusCode, AppError> {
    let db = state.db();
    db.delete_currency(&code).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("not found") {
            AppError::NotFound(msg)
        } else if msg.contains("preferred") {
            AppError::bad_request(msg, "cannot_delete_preferred")
        } else if msg.contains("in use") {
            AppError::conflict(msg, "currency_in_use")
        } else {
            AppError::Internal(e)
        }
    })?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_iso_codes_are_valid() {
        assert!(is_valid_iso_code("GBP"));
        assert!(is_valid_iso_code("USD"));
    }

    /// Regression test for the sub-unit migration: broker sub-unit codes must
    /// no longer be creatable as their own configured currency now that every
    /// write path converts them to their parent at import time.
    #[test]
    fn broker_sub_unit_codes_are_no_longer_valid() {
        assert!(!is_valid_iso_code("GBX"));
        assert!(!is_valid_iso_code("USX"));
        assert!(!is_valid_iso_code("ZAC"));
        assert!(!is_valid_iso_code("ILA"));
    }

    #[test]
    fn unknown_code_is_invalid() {
        assert!(!is_valid_iso_code("XXX"));
    }
}
