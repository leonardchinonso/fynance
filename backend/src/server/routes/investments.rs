//! Investment event CRUD routes: POST, GET, PATCH, DELETE.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde_json::Value;

use crate::model::{
    CreateInvestmentEventBody, InvestmentEvent, ListInvestmentEventsQuery,
    PatchInvestmentEventBody,
};
use crate::server::error::AppError;
use crate::server::state::AppState;

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
        )?
    };
    Ok(Json(serde_json::to_value(events)?))
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
