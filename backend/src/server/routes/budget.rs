//! Budget routes: spending grid, per-month budget view, set standing budgets,
//! set per-month overrides.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::model::{CashSummaryResponse, SpendingGroupBy};
use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::server::validation::{
    parse_date, parse_decimal, parse_granularity, parse_month, require_non_negative,
    split_csv_param, validate_date_range,
};
use crate::util::fx::FxRateMap;

// ── GET /api/budget/:month ────────────────────────────────────────────────────

pub async fn get_budget_for_month(
    State(state): State<AppState>,
    Path(month): Path<String>,
) -> Result<Json<Value>, AppError> {
    parse_month(&month)?;
    let (rows, preferred_currency) = {
        let db = state.db();
        let currencies = db.get_currencies()?;
        let fx = FxRateMap::new(currencies)?;
        let rows = db.get_effective_budget(&month, &fx)?;
        let preferred = fx.preferred().to_string();
        (rows, preferred)
    };
    Ok(Json(serde_json::json!({
        "preferred_currency": preferred_currency,
        "rows": rows
    })))
}

// ── GET /api/budget/spending-grid ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SpendingGridQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub granularity: Option<String>,
    pub profile_id: Option<String>,
    /// Comma-separated account IDs.
    pub accounts: Option<String>,
    /// Comma-separated category IDs.
    pub categories: Option<String>,
    /// Comma-separated category_type values.
    pub category_types: Option<String>,
    /// Grouping dimension: parent_category | leaf_category | category_type |
    /// account. Defaults to leaf_category (per-leaf rows for the spreadsheet).
    pub group_by: Option<String>,
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
    let accounts = q
        .accounts
        .as_deref()
        .and_then(split_csv_param)
        .unwrap_or_default();
    let categories = q
        .categories
        .as_deref()
        .and_then(split_csv_param)
        .unwrap_or_default();
    let category_types = q
        .category_types
        .as_deref()
        .and_then(split_csv_param)
        .unwrap_or_default();
    let group_by = q
        .group_by
        .as_deref()
        .map(|s| {
            SpendingGroupBy::parse(s)
                .ok_or_else(|| AppError::bad_request("invalid group_by", "invalid_parameter"))
        })
        .transpose()?
        .unwrap_or_default();

    let (rows, preferred_currency) = {
        let db = state.db();
        let currencies = db.get_currencies()?;
        let fx = FxRateMap::new(currencies)?;
        let rows = db.get_spending_grid(
            start,
            end,
            &granularity,
            profile_id,
            &accounts,
            &categories,
            &category_types,
            group_by,
            &fx,
        )?;
        let preferred = fx.preferred().to_string();
        (rows, preferred)
    };

    Ok(Json(serde_json::json!({
        "preferred_currency": preferred_currency,
        "rows": rows
    })))
}

// ── GET /api/budget/cash-summary ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CashSummaryQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub profile_id: Option<String>,
}

pub async fn get_cash_summary(
    State(state): State<AppState>,
    Query(q): Query<CashSummaryQuery>,
) -> Result<Json<CashSummaryResponse>, AppError> {
    let start = parse_date(q.start.as_deref().ok_or_else(|| {
        AppError::bad_request("missing required parameter: start", "missing_parameter")
    })?)?;
    let end = parse_date(q.end.as_deref().ok_or_else(|| {
        AppError::bad_request("missing required parameter: end", "missing_parameter")
    })?)?;
    validate_date_range(start, end)?;
    let profile_id = q.profile_id.as_deref().filter(|s| !s.is_empty());

    let resp = {
        let db = state.db();
        let currencies = db.get_currencies()?;
        let fx = FxRateMap::new(currencies)?;
        let (income, spending) = db.compute_category_type_cash(start, end, profile_id, &fx)?;
        let savings_growth = db.compute_savings_growth(start, end, profile_id, &fx)?;
        let new_cash_invested = db.compute_new_cash_invested(start, end, profile_id, &fx)?;
        let investment_metrics =
            db.compute_investment_metrics_with(start, end, profile_id, &fx, new_cash_invested)?;
        CashSummaryResponse {
            preferred_currency: fx.preferred().to_string(),
            income,
            spending,
            savings_growth,
            new_cash_invested,
            investment_metrics,
        }
    };

    Ok(Json(resp))
}

// ── POST /api/budget ──────────────────────────────────────────────────────────

/// Request body for `POST /api/budget`. Sets a standing monthly target
/// for one category that applies to every month unless overridden.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct SetStandingBudgetBody {
    /// FK to categories.id (leaf)
    pub category_id: String,
    pub amount: String,
}

pub async fn set_standing_budget(
    State(state): State<AppState>,
    Json(body): Json<SetStandingBudgetBody>,
) -> Result<Json<Value>, AppError> {
    let amount = parse_decimal(&body.amount)?;
    require_non_negative(amount)?;

    let db = state.db();

    if body.category_id.is_empty() {
        return Err(AppError::bad_request(
            "category_id is required",
            "invalid_category",
        ));
    }

    db.set_standing_budget(&body.category_id, amount)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── POST /api/budget/override ─────────────────────────────────────────────────

/// Request body for `POST /api/budget/override`. Sets a per-month override
/// on top of the standing budget for one category.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct SetBudgetOverrideBody {
    pub month: String,
    /// FK to categories.id (leaf)
    pub category_id: String,
    pub amount: String,
}

pub async fn set_budget_override(
    State(state): State<AppState>,
    Json(body): Json<SetBudgetOverrideBody>,
) -> Result<Json<Value>, AppError> {
    parse_month(&body.month)?;
    let amount = parse_decimal(&body.amount)?;
    require_non_negative(amount)?;

    let db = state.db();

    if body.category_id.is_empty() {
        return Err(AppError::bad_request(
            "category_id is required",
            "invalid_category",
        ));
    }

    db.set_budget_override(&body.month, &body.category_id, amount)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
