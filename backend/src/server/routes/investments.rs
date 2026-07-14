//! Investment event CRUD routes: POST, GET, PATCH, DELETE.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::Value;

use crate::model::{
    CreateInvestmentEventBody, InvestmentEvent, InvestmentImportError, InvestmentImportResult,
    InvestmentsImportPayload, ListInvestmentEventsQuery, PatchInvestmentEventBody,
};
use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::server::validation::{
    parse_date, parse_granularity, split_csv_param, validate_date_range,
};
use crate::util::fx::FxRateMap;

// ── POST /api/investments ────────────────────────────────────────────────────

pub async fn create_investment(
    State(state): State<AppState>,
    Json(body): Json<CreateInvestmentEventBody>,
) -> Result<Json<InvestmentEvent>, AppError> {
    if body.account_id.is_empty() {
        return Err(AppError::bad_request(
            "account_id must not be empty",
            "invalid_account_id",
        ));
    }
    if body.symbol.is_empty() {
        return Err(AppError::bad_request(
            "symbol must not be empty",
            "invalid_symbol",
        ));
    }
    if body.currency.is_empty() {
        return Err(AppError::bad_request(
            "currency must not be empty",
            "invalid_currency",
        ));
    }

    let event = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.create_investment_event(&body)
            .map_err(|e| AppError::bad_request(e.to_string(), "invalid_body"))?
    };

    Ok(Json(event))
}

// ── GET /api/investments ─────────────────────────────────────────────────────

pub async fn list_investments(
    State(state): State<AppState>,
    Query(q): Query<ListInvestmentEventsQuery>,
) -> Result<Json<Value>, AppError> {
    let events = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.list_investment_events(
            q.account_id.as_deref(),
            q.symbol.as_deref(),
            q.event_type.as_deref(),
            None,
        )?
    };
    Ok(Json(serde_json::to_value(events)?))
}

// ── GET /api/investments/history ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct InvestmentHistoryQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub granularity: Option<String>,
    pub profile_id: Option<String>,
    /// Comma-separated account ids. Empty means every investment + ISA account.
    pub accounts: Option<String>,
}

pub async fn get_investment_history(
    State(state): State<AppState>,
    Query(q): Query<InvestmentHistoryQuery>,
) -> Result<Json<Value>, AppError> {
    let start = q
        .start
        .as_deref()
        .ok_or_else(|| AppError::bad_request("start is required", "missing_parameter"))
        .and_then(parse_date)?;
    let end = q
        .end
        .as_deref()
        .ok_or_else(|| AppError::bad_request("end is required", "missing_parameter"))
        .and_then(parse_date)?;
    validate_date_range(start, end)?;

    let granularity = q
        .granularity
        .as_deref()
        .ok_or_else(|| AppError::bad_request("granularity is required", "missing_parameter"))
        .and_then(parse_granularity)?;

    let profile_id = q.profile_id.as_deref().filter(|s| !s.is_empty());
    let account_ids: Vec<String> = q
        .accounts
        .as_deref()
        .and_then(split_csv_param)
        .unwrap_or_default();

    let (rows, preferred_currency) = {
        let db = state.db.lock().expect("db mutex poisoned");
        let fx = FxRateMap::new(db.get_currencies()?)?;
        let rows =
            db.get_investment_history(start, end, &granularity, profile_id, &account_ids, &fx)?;
        (rows, fx.preferred().to_string())
    };

    Ok(Json(serde_json::json!({
        "preferred_currency": preferred_currency,
        "rows": rows,
    })))
}

// ── PATCH /api/investments/:id ───────────────────────────────────────────────

pub async fn update_investment(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchInvestmentEventBody>,
) -> Result<Json<InvestmentEvent>, AppError> {
    if body.event_type.is_none()
        && body.symbol.is_none()
        && body.date.is_none()
        && body.quantity.is_none()
        && body.price_per_share.is_none()
        && body.fee.is_none()
        && body.currency.is_none()
        && body.fee_currency.is_none()
        && body.notes.is_none()
    {
        return Err(AppError::bad_request(
            "at least one field must be provided",
            "empty_body",
        ));
    }

    let event = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.update_investment_event(&id, &body)
            .map_err(|e| AppError::bad_request(e.to_string(), "invalid_body"))?
    };

    event
        .map(Json)
        .ok_or_else(|| AppError::NotFound(format!("investment event {id} not found")))
}

// ── DELETE /api/investments/:id ──────────────────────────────────────────────

pub async fn delete_investment(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let deleted = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.delete_investment_event(&id)?
    };

    if deleted {
        Ok(Json(serde_json::json!({ "ok": true })))
    } else {
        Err(AppError::NotFound(format!(
            "investment event {id} not found"
        )))
    }
}

// ── POST /api/investments/import ────────────────────────────────────────────

pub async fn import_investments(
    State(state): State<AppState>,
    Json(payload): Json<InvestmentsImportPayload>,
) -> Result<Json<InvestmentImportResult>, AppError> {
    if payload.account_id.is_empty() {
        return Err(AppError::bad_request(
            "account_id must not be empty",
            "invalid_account_id",
        ));
    }

    // A payload with no provenance commits cleanly and reports success, so the
    // omission is invisible: a bulk import that rebuilt its rows by hand instead of
    // forwarding the /api/parse payload verbatim left hundreds of events with no
    // source link and no error. Warn rather than reject, since a manual or
    // corrective import legitimately has no source document.
    if !payload.events.is_empty()
        && payload
            .events
            .iter()
            .all(|e| e.source_document_ids.is_empty())
    {
        tracing::warn!(
            account_id = %payload.account_id,
            events = payload.events.len(),
            "investment import has no source_document_ids on any event; these rows will have no provenance. \
             Forward the /api/parse payload verbatim, or upload the source via POST /api/documents first."
        );
    }

    let mut inserted: usize = 0;
    let mut duplicates: usize = 0;
    let mut errors: Vec<InvestmentImportError> = Vec::new();

    {
        let db = state.db.lock().expect("db mutex poisoned");
        for (index, event) in payload.events.iter().enumerate() {
            let body = CreateInvestmentEventBody {
                account_id: payload.account_id.clone(),
                event_type: event.event_type.clone(),
                symbol: event.symbol.clone(),
                date: event.date.clone(),
                quantity: event.quantity.clone(),
                price_per_share: event.price_per_share.clone(),
                fee: event.fee.clone(),
                currency: event.currency.clone(),
                fee_currency: event.fee_currency.clone(),
                notes: event.notes.clone(),
                source_document_ids: event.source_document_ids.clone(),
            };

            match db.create_investment_event(&body) {
                Ok(_) => {
                    inserted += 1;
                }
                Err(e) => {
                    let reason = e.to_string();
                    if reason.contains("UNIQUE constraint") || reason.contains("duplicate") {
                        duplicates += 1;
                    } else {
                        errors.push(InvestmentImportError { index, reason });
                    }
                }
            }
        }
    }

    Ok(Json(InvestmentImportResult {
        total: payload.events.len(),
        inserted,
        duplicates,
        errors,
    }))
}
