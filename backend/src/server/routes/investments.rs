//! Investment event CRUD routes: POST, GET, PATCH, DELETE.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::Value;

use crate::model::{
    CreateInvestmentEventBody, InsertOutcome, InvestmentEvent, InvestmentImportError,
    InvestmentImportResult, InvestmentsImportPayload, ListInvestmentEventsQuery,
    PatchInvestmentEventBody,
};
use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::server::validation::{
    parse_date, parse_granularity, split_csv_param, validate_currency, validate_date_range,
};
use crate::storage::db::Db;
use crate::util::fx::FxRateMap;

/// Reject unconfigured trade / fee currencies before anything is written,
/// matching the transaction and holdings import paths.
///
/// A broker sub-unit code (GBX, USX, ZAC, ILA) is accepted here even though it
/// is never itself a "configured" currency: `create_investment_event` converts
/// it to its parent before the row is stored, so what actually needs to be
/// configured is the *parent* currency, not the sub-unit. This keeps sub-unit
/// codes usable as input without requiring them in the `currencies` table.
fn validate_event_currency(db: &Db, currency: &str) -> Result<(), AppError> {
    if let Some(unit) = crate::util::subunits::lookup(currency) {
        return validate_currency(db, unit.parent);
    }
    validate_currency(db, currency)
}

fn validate_event_currencies(
    db: &Db,
    currency: &str,
    fee_currency: Option<&str>,
) -> Result<(), AppError> {
    validate_event_currency(db, currency)?;
    if let Some(fc) = fee_currency.filter(|s| !s.is_empty()) {
        validate_event_currency(db, fc)?;
    }
    Ok(())
}

/// Reject an event whose symbol is already recorded under a different trade
/// currency.
///
/// The S104 pool for a symbol is a single running total of shares and cost. If
/// one symbol's events carry two currencies, that total is a sum of (say) pence
/// and pounds — a meaningless number that still prints as a confident cost
/// basis. The realistic cause is the same LSE holding reported as `GBX` by one
/// broker and `GBP` by another, but ticker collisions and plain data entry
/// produce it too.
///
/// Only the trade currency is checked. A fee charged in a different currency is
/// legitimate and already handled — the engine converts price and fee at their
/// own rates before summing into the pool.
///
/// `exclude_id` is the event being edited, so a PATCH does not conflict with the
/// row it is itself rewriting.
fn validate_symbol_currency(
    db: &Db,
    symbol: &str,
    currency: &str,
    exclude_id: Option<&str>,
) -> Result<(), AppError> {
    let existing = db.investment_currencies_for_symbol(symbol, exclude_id)?;
    let conflicting: Vec<&str> = existing
        .iter()
        .map(|c| c.as_str())
        .filter(|c| *c != currency)
        .collect();
    if conflicting.is_empty() {
        return Ok(());
    }
    let list = conflicting.join(", ");
    Err(AppError::bad_request(
        format!(
            "Symbol '{symbol}' is already recorded in {list}, but this event is in {currency}. \
             One symbol must use one currency, or its capital-gains pool becomes a sum of two \
             different units. Correct this event's currency, or if the existing rows are the \
             wrong ones, edit them to {currency} first."
        ),
        "symbol_currency_conflict",
    ))
}

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
        let db = state.db();
        validate_event_currencies(&db, &body.currency, body.fee_currency.as_deref())?;
        validate_symbol_currency(&db, &body.symbol, &body.currency, None)?;
        let (event, _outcome) = db
            .create_investment_event(&body)
            .map_err(|e| AppError::bad_request(e.to_string(), "invalid_body"))?;
        event
    };

    Ok(Json(event))
}

// ── GET /api/investments ─────────────────────────────────────────────────────

pub async fn list_investments(
    State(state): State<AppState>,
    Query(q): Query<ListInvestmentEventsQuery>,
) -> Result<Json<Value>, AppError> {
    let events = {
        let db = state.db();
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
        let db = state.db();
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
        let db = state.db();
        if let Some(ref c) = body.currency {
            validate_currency(&db, c)?;
        }
        if let Some(fc) = body.fee_currency.as_deref().filter(|s| !s.is_empty()) {
            validate_currency(&db, fc)?;
        }
        // A patch can move the event to another symbol, another currency, or both, so
        // the one-symbol-one-currency rule is checked against the pair the event will
        // END UP with — omitted fields keep their stored value. The event itself is
        // excluded from the comparison, or re-saving the only event of a symbol in a
        // new currency would conflict with the row it is replacing.
        if body.symbol.is_some() || body.currency.is_some() {
            if let Some((cur_symbol, cur_currency)) = db.investment_event_symbol_currency(&id)? {
                let next_symbol = body.symbol.as_deref().unwrap_or(&cur_symbol);
                let next_currency = body.currency.as_deref().unwrap_or(&cur_currency);
                validate_symbol_currency(&db, next_symbol, next_currency, Some(&id))?;
            }
        }
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
        let db = state.db();
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
        let db = state.db();

        // Validate currencies for the whole batch before writing anything.
        for event in &payload.events {
            validate_event_currencies(&db, &event.currency, event.fee_currency.as_deref())?;
        }

        // One symbol, one currency — checked against stored rows AND across the batch
        // itself, since an import is the most likely way two brokers' spellings of the
        // same holding (GBX vs GBP) arrive together. Checked up-front so a bad batch
        // writes nothing rather than half its rows.
        let mut batch_currency: std::collections::HashMap<&str, &str> =
            std::collections::HashMap::new();
        for event in &payload.events {
            validate_symbol_currency(&db, &event.symbol, &event.currency, None)?;
            match batch_currency.get(event.symbol.as_str()) {
                Some(seen) if *seen != event.currency.as_str() => {
                    return Err(AppError::bad_request(
                        format!(
                            "This import has symbol '{}' in both {seen} and {}. One symbol must \
                             use one currency, or its capital-gains pool becomes a sum of two \
                             different units. Correct the rows that use the wrong currency and \
                             re-import — nothing has been saved.",
                            event.symbol, event.currency
                        ),
                        "symbol_currency_conflict",
                    ));
                }
                _ => {
                    batch_currency.insert(event.symbol.as_str(), event.currency.as_str());
                }
            }
        }

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
                Ok((_, InsertOutcome::Inserted)) => inserted += 1,
                Ok((_, InsertOutcome::Duplicate)) => duplicates += 1,
                Err(e) => errors.push(InvestmentImportError {
                    index,
                    reason: e.to_string(),
                }),
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn test_db() -> (Db, NamedTempFile) {
        let file = NamedTempFile::new().expect("temp file");
        let db = Db::open(file.path()).expect("test db");
        (db, file)
    }

    #[test]
    fn sub_unit_currency_is_valid_input_when_its_parent_is_configured() {
        let (db, _file) = test_db();
        // GBP is seeded as the default preferred currency; GBX is never itself
        // configured, but should still be accepted as input because
        // create_investment_event converts it to GBP before writing.
        assert!(validate_event_currency(&db, "GBX").is_ok());
    }

    #[test]
    fn sub_unit_currency_is_rejected_when_its_parent_is_not_configured() {
        let (db, _file) = test_db();
        // ZAR (ZAC's parent) was never configured, so the sub-unit must be
        // rejected too — otherwise create_investment_event would silently
        // write an unconfigured currency once converted.
        assert!(validate_event_currency(&db, "ZAC").is_err());
    }

    #[test]
    fn ordinary_currency_validation_is_unaffected() {
        let (db, _file) = test_db();
        assert!(validate_event_currency(&db, "GBP").is_ok());
        assert!(validate_event_currency(&db, "XXX").is_err());
    }
}
