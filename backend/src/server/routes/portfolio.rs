//! Portfolio routes:
//!   GET  /api/portfolio
//!   GET  /api/portfolio/history
//!   GET  /api/portfolio/balances
//!   GET  /api/cash-flow

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Query, State};
use chrono::Local;
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::model::{
    AccountSnapshot, BalanceDelta, BreakdownItem, CashFlowMonth, PortfolioHistoryRow,
    PortfolioResponse,
};
use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::server::validation::{parse_date, parse_granularity, validate_date_range};
use crate::storage::db::{account_type_to_asset_class, is_available_account};

// ── GET /api/portfolio ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PortfolioQuery {
    pub profile_id: Option<String>,
    pub as_of: Option<String>,
}

pub async fn get_portfolio(
    State(state): State<AppState>,
    Query(q): Query<PortfolioQuery>,
) -> Result<Json<PortfolioResponse>, AppError> {
    let as_of = match q.as_of.as_deref() {
        Some(s) => parse_date(s)?.min(Local::now().date_naive()),
        None => Local::now().date_naive(),
    };
    let profile_id = q.profile_id.as_deref().filter(|s| !s.is_empty());

    // Compute investment metrics over the trailing year.
    let metrics_start = as_of
        .checked_sub_months(chrono::Months::new(12))
        .unwrap_or(as_of);

    let (accounts, investment_metrics) = {
        let db = state.db.lock().expect("db mutex poisoned");
        let accounts = db.get_portfolio_as_of(as_of, profile_id)?;
        let metrics = db.compute_investment_metrics(metrics_start, as_of, profile_id)?;
        (accounts, metrics)
    };

    // Aggregate totals.
    let mut total_assets = Decimal::ZERO;
    let mut total_liabilities = Decimal::ZERO;
    let mut available_wealth = Decimal::ZERO;
    let mut unavailable_wealth = Decimal::ZERO;

    // Maps for breakdown aggregations.
    let mut by_type_map: HashMap<String, Decimal> = HashMap::new();
    let mut by_institution_map: HashMap<String, Decimal> = HashMap::new();
    let mut by_asset_class_map: HashMap<String, Decimal> = HashMap::new();

    for account in &accounts {
        let balance = account.balance.unwrap_or(Decimal::ZERO);

        if balance >= Decimal::ZERO {
            total_assets += balance;
        } else {
            total_liabilities += balance;
        }

        if is_available_account(&account.account_type) {
            available_wealth += balance;
        } else {
            unavailable_wealth += balance;
        }

        // Breakdowns use absolute values (liabilities show as positive for charting).
        let abs_balance = balance.abs();
        *by_type_map
            .entry(account.account_type.as_str().to_string())
            .or_default() += abs_balance;
        *by_institution_map
            .entry(account.institution.clone())
            .or_default() += abs_balance;
        *by_asset_class_map
            .entry(account_type_to_asset_class(&account.account_type).to_string())
            .or_default() += abs_balance;
    }

    let net_worth = total_assets + total_liabilities;

    // Build breakdown items with percentages.
    let total_abs = total_assets.abs() + total_liabilities.abs();
    let to_breakdown = |map: HashMap<String, Decimal>| -> Vec<BreakdownItem> {
        let mut items: Vec<BreakdownItem> = map
            .into_iter()
            .map(|(label, value)| {
                let percentage = if total_abs.is_zero() {
                    0.0
                } else {
                    (value / total_abs * Decimal::ONE_HUNDRED)
                        .try_into()
                        .unwrap_or(0.0_f64)
                };
                BreakdownItem {
                    label,
                    value,
                    percentage,
                }
            })
            .collect();
        items.sort_by(|a, b| {
            b.value
                .partial_cmp(&a.value)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        items
    };

    let by_type = to_breakdown(by_type_map);
    let by_institution = to_breakdown(by_institution_map);
    let by_asset_class = to_breakdown(by_asset_class_map);

    // Determine currency from first account, default GBP.
    let currency = accounts
        .first()
        .map(|a| a.currency.clone())
        .unwrap_or_else(|| "GBP".to_string());

    Ok(Json(PortfolioResponse {
        net_worth,
        currency,
        as_of: as_of.format("%Y-%m-%d").to_string(),
        total_assets,
        total_liabilities,
        available_wealth,
        unavailable_wealth,
        accounts,
        by_type,
        by_institution,
        by_asset_class,
        investment_metrics,
    }))
}

// ── GET /api/portfolio/history ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PortfolioHistoryQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub granularity: Option<String>,
    pub profile_id: Option<String>,
}

pub async fn get_portfolio_history(
    State(state): State<AppState>,
    Query(q): Query<PortfolioHistoryQuery>,
) -> Result<Json<Vec<PortfolioHistoryRow>>, AppError> {
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

    let rows = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_monthly_net_worth(start, end, &granularity, profile_id)?
    };

    Ok(Json(rows))
}

// ── GET /api/portfolio/balances ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PortfolioBalancesQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub summary: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(untagged)]
pub enum BalancesResponse {
    Full(Vec<AccountSnapshot>),
    Summary(Vec<BalanceDelta>),
}

pub async fn get_portfolio_balances(
    State(state): State<AppState>,
    Query(q): Query<PortfolioBalancesQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
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

    let summary = q.summary.as_deref().unwrap_or("false") == "true";

    let db = state.db.lock().expect("db mutex poisoned");
    if summary {
        let deltas = db.get_balance_summary(start, end)?;
        Ok(Json(serde_json::to_value(deltas)?))
    } else {
        let balances = db.get_balances_in_range(start, end)?;
        Ok(Json(serde_json::to_value(balances)?))
    }
}

// ── GET /api/cash-flow ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CashFlowQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub granularity: Option<String>,
    pub profile_id: Option<String>,
}

pub async fn get_cash_flow(
    State(state): State<AppState>,
    Query(q): Query<CashFlowQuery>,
) -> Result<Json<Vec<CashFlowMonth>>, AppError> {
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

    let rows = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_cash_flow(start, end, profile_id, &granularity)?
    };

    Ok(Json(rows))
}
