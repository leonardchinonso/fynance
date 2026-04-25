//! Budget routes: spending grid, per-month budget view, set standing budgets,
//! set per-month overrides.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::server::validation::{
    parse_date, parse_decimal, parse_granularity, parse_month, require_non_negative,
    validate_date_range,
};

// ── GET /api/budget/:month ────────────────────────────────────────────────────

pub async fn get_budget_for_month(
    State(state): State<AppState>,
    Path(month): Path<String>,
) -> Result<Json<Value>, AppError> {
    parse_month(&month)?;
    let rows = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_effective_budget(&month)?
    };
    Ok(Json(serde_json::to_value(rows)?))
}

// ── GET /api/budget/spending-grid ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SpendingGridQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub granularity: Option<String>,
    pub profile_id: Option<String>,
}

pub async fn get_spending_grid(
    State(state): State<AppState>,
    Query(q): Query<SpendingGridQuery>,
) -> Result<Json<Value>, AppError> {
    let start_str = q.start.as_deref().ok_or_else(|| {
        AppError::bad_request("missing required parameter: start", "missing_parameter")
    })?;
    let end_str = q.end.as_deref().ok_or_else(|| {
        AppError::bad_request("missing required parameter: end", "missing_parameter")
    })?;
    let gran_str = q.granularity.as_deref().ok_or_else(|| {
        AppError::bad_request(
            "missing required parameter: granularity",
            "missing_parameter",
        )
    })?;

    let start = parse_date(start_str)?;
    let end = parse_date(end_str)?;
    validate_date_range(start, end)?;
    let granularity = parse_granularity(gran_str)?;

    let profile_id = q.profile_id.as_deref().filter(|s| !s.is_empty());

    let rows = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_spending_grid(start, end, &granularity, profile_id)?
    };

    Ok(Json(serde_json::to_value(rows)?))
}

// ── POST /api/budget ──────────────────────────────────────────────────────────

/// Request body for `POST /api/budget`. Sets a standing monthly target
/// for one category that applies to every month unless overridden.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct SetStandingBudgetBody {
    pub category: String,
    pub amount: String,
}

pub async fn set_standing_budget(
    State(state): State<AppState>,
    Json(body): Json<SetStandingBudgetBody>,
) -> Result<Json<Value>, AppError> {
    if body.category.is_empty() {
        return Err(AppError::bad_request(
            "category must not be empty",
            "invalid_category",
        ));
    }
    let amount = parse_decimal(&body.amount)?;
    require_non_negative(amount)?;

    {
        let db = state.db.lock().expect("db mutex poisoned");
        db.set_standing_budget(&body.category, amount)?;
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── POST /api/budget/override ─────────────────────────────────────────────────

/// Request body for `POST /api/budget/override`. Sets a per-month override
/// on top of the standing budget for one category.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct SetBudgetOverrideBody {
    pub month: String,
    pub category: String,
    pub amount: String,
}

pub async fn set_budget_override(
    State(state): State<AppState>,
    Json(body): Json<SetBudgetOverrideBody>,
) -> Result<Json<Value>, AppError> {
    parse_month(&body.month)?;
    if body.category.is_empty() {
        return Err(AppError::bad_request(
            "category must not be empty",
            "invalid_category",
        ));
    }
    let amount = parse_decimal(&body.amount)?;
    require_non_negative(amount)?;

    {
        let db = state.db.lock().expect("db mutex poisoned");
        db.set_budget_override(&body.month, &body.category, amount)?;
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}
