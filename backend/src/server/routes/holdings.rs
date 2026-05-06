//! Holdings routes:
//!   GET  /api/holdings
//!   GET  /api/holdings/summary
//!   GET  /api/holdings/history
//!   GET  /api/holdings/balances
//!   GET  /api/holdings/cash-flow
//!   POST /api/holdings/import
//!   POST /api/holdings/:account_id
//!   PATCH /api/holdings/:account_id/:symbol

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use chrono::Local;
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::model::{
    AccountSnapshot, BalanceDelta, BreakdownItem, Holding, HoldingsImportPayload,
    HoldingsSummaryResponse,
};
use crate::server::auth::AuthContext;
use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::server::validation::{
    parse_date, parse_granularity, split_csv_param, validate_date_range, validate_currency,
};
use crate::storage::db::{account_type_to_asset_class, is_available_account};
use crate::util::fx::{FxRateMap, CurrencyAggregator};

// ── Auth helper ─────────────────────────────────────────────────────────────

fn require_token_if_remote(state: &AppState, auth: &AuthContext) -> Result<(), AppError> {
    if !state.loopback_only && !matches!(auth, AuthContext::Token { .. }) {
        return Err(AppError::Unauthorized(
            "Bearer token required for holdings endpoints in non-loopback mode".to_string(),
        ));
    }
    Ok(())
}

// ── GET /api/holdings ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct HoldingsQuery {
    pub account_id: Option<String>,
    pub account_ids: Option<String>,
    pub profile_id: Option<String>,
}

pub async fn list_holdings(
    State(state): State<AppState>,
    Query(q): Query<HoldingsQuery>,
) -> Result<Json<Vec<Holding>>, AppError> {
    let db = state.db.lock().expect("db mutex poisoned");

    let account_ids: Vec<String> = if let Some(ref id) = q.account_id {
        if !id.is_empty() {
            vec![id.clone()]
        } else {
            vec![]
        }
    } else if let Some(ref ids) = q.account_ids {
        split_csv_param(ids).unwrap_or_default()
    } else if let Some(ref pid) = q.profile_id {
        if !pid.is_empty() {
            db.get_accounts(Some(pid))?
                .into_iter()
                .filter(|a| {
                    matches!(
                        a.account_type,
                        crate::model::AccountType::Investment
                            | crate::model::AccountType::InvestmentIsa
                            | crate::model::AccountType::Pension
                    )
                })
                .map(|a| a.id)
                .collect()
        } else {
            vec![]
        }
    } else {
        return Err(AppError::bad_request(
            "must provide one of: account_id, account_ids, profile_id",
            "missing_parameter",
        ));
    };

    if account_ids.is_empty() {
        return Ok(Json(vec![]));
    }

    let holdings = db.get_holdings_batch(&account_ids)?;
    Ok(Json(holdings))
}

// ── GET /api/holdings/summary ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct HoldingsSummaryQuery {
    pub profile_id: Option<String>,
    pub as_of: Option<String>,
}

pub async fn get_holdings_summary(
    State(state): State<AppState>,
    Query(q): Query<HoldingsSummaryQuery>,
) -> Result<Json<HoldingsSummaryResponse>, AppError> {
    let as_of = match q.as_of.as_deref() {
        Some(s) => parse_date(s)?.min(Local::now().date_naive()),
        None => Local::now().date_naive(),
    };
    let profile_id = q.profile_id.as_deref().filter(|s| !s.is_empty());

    let metrics_start = as_of
        .checked_sub_months(chrono::Months::new(12))
        .unwrap_or(as_of);

    let (accounts, holding_rows, investment_metrics, fx) = {
        let db = state.db.lock().expect("db mutex poisoned");
        let accounts = db.get_portfolio_as_of(as_of, profile_id)?;
        let holding_rows = db.get_holdings_for_summary(as_of, profile_id)?;
        let metrics = db.compute_investment_metrics(metrics_start, as_of, profile_id)?;
        let currencies = db.get_currencies()?;
        let fx = FxRateMap::new(currencies)?;
        (accounts, holding_rows, metrics, fx)
    };

    let mut total_assets = Decimal::ZERO;
    let mut total_liabilities = Decimal::ZERO;
    let mut available_wealth = Decimal::ZERO;
    let mut unavailable_wealth = Decimal::ZERO;

    let mut by_type_map: HashMap<String, CurrencyAggregator> = HashMap::new();
    let mut by_institution_map: HashMap<String, CurrencyAggregator> = HashMap::new();
    let mut by_asset_class_map: HashMap<String, CurrencyAggregator> = HashMap::new();

    for row in &holding_rows {
        let h = &row.holding;
        let converted = fx.convert(h.value, &h.currency);

        if converted >= Decimal::ZERO {
            total_assets += converted;
        } else {
            total_liabilities += converted;
        }

        if is_available_account(&row.account_type) {
            available_wealth += converted;
        } else {
            unavailable_wealth += converted;
        }

        by_type_map
            .entry(row.account_type.as_str().to_string())
            .or_default()
            .add(h.value, &h.currency, &fx);
        by_institution_map
            .entry(row.institution.clone())
            .or_default()
            .add(h.value, &h.currency, &fx);
        by_asset_class_map
            .entry(account_type_to_asset_class(&row.account_type).to_string())
            .or_default()
            .add(h.value, &h.currency, &fx);
    }

    let net_worth = total_assets + total_liabilities;

    let total_abs = net_worth;
    let to_breakdown = |map: HashMap<String, CurrencyAggregator>| -> Vec<BreakdownItem> {
        let mut items: Vec<BreakdownItem> = map
            .into_iter()
            .map(|(label, agg)| {
                let value = agg.converted_sum();
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
                    display_currency: agg.display_currency(fx.preferred()),
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

    let preferred_currency = fx.preferred().to_string();

    Ok(Json(HoldingsSummaryResponse {
        net_worth,
        preferred_currency,
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

// ── GET /api/holdings/history ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct HoldingsHistoryQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub granularity: Option<String>,
    pub profile_id: Option<String>,
}

pub async fn get_holdings_history(
    State(state): State<AppState>,
    Query(q): Query<HoldingsHistoryQuery>,
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

    let granularity = q
        .granularity
        .as_deref()
        .ok_or_else(|| AppError::bad_request("granularity is required", "missing_parameter"))
        .and_then(parse_granularity)?;

    let profile_id = q.profile_id.as_deref().filter(|s| !s.is_empty());

    let (rows, preferred_currency) = {
        let db = state.db.lock().expect("db mutex poisoned");
        let currencies = db.get_currencies()?;
        let fx = FxRateMap::new(currencies)?;
        let rows = db.get_monthly_net_worth(start, end, &granularity, profile_id, &fx)?;
        let preferred = fx.preferred().to_string();
        (rows, preferred)
    };

    Ok(Json(serde_json::json!({
        "preferred_currency": preferred_currency,
        "rows": rows
    })))
}

// ── GET /api/holdings/balances ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct HoldingsBalancesQuery {
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

pub async fn get_holdings_balances(
    State(state): State<AppState>,
    Query(q): Query<HoldingsBalancesQuery>,
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

// ── GET /api/holdings/cash-flow ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct HoldingsCashFlowQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub granularity: Option<String>,
    pub profile_id: Option<String>,
}

pub async fn get_holdings_cash_flow(
    State(state): State<AppState>,
    Query(q): Query<HoldingsCashFlowQuery>,
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

    let granularity = q
        .granularity
        .as_deref()
        .ok_or_else(|| AppError::bad_request("granularity is required", "missing_parameter"))
        .and_then(parse_granularity)?;

    let profile_id = q.profile_id.as_deref().filter(|s| !s.is_empty());

    let (rows, preferred_currency) = {
        let db = state.db.lock().expect("db mutex poisoned");
        let currencies = db.get_currencies()?;
        let fx = FxRateMap::new(currencies)?;
        let rows = db.get_cash_flow(start, end, profile_id, &granularity, &fx)?;
        let preferred = fx.preferred().to_string();
        (rows, preferred)
    };

    Ok(Json(serde_json::json!({
        "preferred_currency": preferred_currency,
        "rows": rows
    })))
}

// ── POST /api/holdings/import ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct HoldingsImportQuery {
    pub dry_run: Option<bool>,
}

pub async fn import_holdings(
    State(state): State<AppState>,
    auth: Extension<AuthContext>,
    Query(q): Query<HoldingsImportQuery>,
    Json(payload): Json<HoldingsImportPayload>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_token_if_remote(&state, &auth)?;
    let db = state.db.lock().expect("db mutex poisoned");

    if !db.account_exists(&payload.account_id)? {
        return Err(AppError::bad_request(
            format!("account {} not found", payload.account_id),
            "account_not_found",
        ));
    }

    // Validate currencies for all holdings.
    for holding in &payload.holdings {
        validate_currency(&db, &holding.currency)?;
    }

    if q.dry_run.unwrap_or(false) {
        let previews = db.dry_run_holdings(&payload.account_id, &payload.holdings)?;
        return Ok(Json(serde_json::json!({
            "dry_run": true,
            "preview": { "total": previews.len(), "snapshots": previews },
            "commit_payload": { "account_id": payload.account_id, "holdings": payload.holdings }
        })));
    }

    db.upsert_holdings(&payload.account_id, &payload.holdings)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "holdings_imported": payload.holdings.len()
    })))
}

// ── POST /api/holdings/:account_id ────────────────────────────────────────────

pub async fn post_holdings(
    State(state): State<AppState>,
    auth: Extension<AuthContext>,
    Path(account_id): Path<String>,
    Json(body): Json<Vec<Holding>>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_token_if_remote(&state, &auth)?;

    let holdings_updated = {
        let db = state.db.lock().expect("db mutex poisoned");
        if db.get_account_by_id(&account_id)?.is_none() {
            return Err(AppError::NotFound(format!(
                "account {account_id} not found"
            )));
        }

        // Validate currencies for all holdings.
        for holding in &body {
            validate_currency(&db, &holding.currency)?;
        }

        db.replace_holdings(&account_id, &body)?
    };

    Ok(Json(serde_json::json!({
        "ok": true,
        "holdings_updated": holdings_updated
    })))
}

// ── PATCH /api/holdings/:account_id/:symbol ──────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PatchHoldingRequest {
    pub is_closed: Option<bool>,
    pub sub_account: Option<String>,
    pub as_of: String,
}

pub async fn patch_holding(
    State(state): State<AppState>,
    Path((account_id, symbol)): Path<(String, String)>,
    Json(body): Json<PatchHoldingRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().expect("db mutex poisoned");
    let as_of = parse_date(&body.as_of)?
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| AppError::bad_request("invalid date", "bad_date"))?;

    if let Some(close) = body.is_closed {
        let rows = if close {
            db.close_holding(&account_id, &symbol, body.sub_account.as_deref(), as_of)?
        } else {
            db.reopen_holding(&account_id, &symbol, body.sub_account.as_deref(), as_of)?
        };
        Ok(Json(
            serde_json::json!({ "ok": true, "rows_updated": rows }),
        ))
    } else {
        Err(AppError::bad_request("nothing to update", "empty_patch"))
    }
}
