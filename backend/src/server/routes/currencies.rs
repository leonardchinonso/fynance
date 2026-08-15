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

/// Subset of ISO 4217 codes accepted by POST /api/currencies. Also includes a
/// short list of non-ISO sub-unit codes that brokers use as line-item currencies
/// (e.g. GBX = British pence on the LSE, ZAC = South African cent). Investment
/// statements regularly arrive denominated in these, so rejecting them at write
/// time means the CGT engine later sees events it can't convert.
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
    // Broker sub-units (GBX, USX, ZAC, ILA) are accepted alongside real ISO codes, sourced from
    // the one table that also drives conversion, so the two can never disagree about which codes
    // exist. They stop being accepted once plan 23 §0.2 (7.1) converts them at import.
    VALID_ISO_CODES.contains(&code) || crate::util::subunits::is_sub_unit(code)
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
