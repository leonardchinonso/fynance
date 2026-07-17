//! Holdings routes:
//!   GET  /api/holdings
//!   GET  /api/holdings/summary
//!   GET  /api/holdings/history
//!   GET  /api/holdings/account-history
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
    AccountSnapshot, BalanceDelta, BreakdownItem, Holding, HoldingWrite, HoldingsSummaryResponse,
    HoldingsWritePayload,
};
use crate::server::auth::AuthContext;
use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::server::validation::{
    parse_date, parse_granularity, parse_naive_datetime, split_csv_param, validate_currency,
    validate_date_range,
};
use crate::storage::db::{account_type_to_asset_class, is_available_account};
use crate::util::fx::{CurrencyAggregator, FxRateMap};

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
    pub include_closed: Option<bool>,
}

pub async fn list_holdings(
    State(state): State<AppState>,
    Query(q): Query<HoldingsQuery>,
) -> Result<Json<Vec<Holding>>, AppError> {
    let db = state.db();
    let include_closed = q.include_closed.unwrap_or(false);

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

    let holdings = db.get_holdings_batch(&account_ids, include_closed)?;
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
        let db = state.db();
        let accounts = db.accounts_as_of(as_of, profile_id)?;
        let holding_rows = db.get_holdings_for_summary(as_of, profile_id)?;
        let currencies = db.get_currencies()?;
        let fx = FxRateMap::new(currencies)?;
        let metrics = db.compute_investment_metrics(metrics_start, as_of, profile_id, &fx)?;
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
            .entry(
                account_type_to_asset_class(&row.account_type)
                    .as_str()
                    .to_string(),
            )
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
        let db = state.db();
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

// ── GET /api/holdings/account-history ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AccountHistoryQuery {
    pub account_id: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub granularity: Option<String>,
}

pub async fn get_account_holdings_history(
    State(state): State<AppState>,
    Query(q): Query<AccountHistoryQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let account_id = q
        .account_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::bad_request("account_id is required", "missing_parameter"))?;

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

    let (symbols, rows, preferred_currency) = {
        let db = state.db();
        let currencies = db.get_currencies()?;
        let fx = FxRateMap::new(currencies)?;
        let (symbols, rows) =
            db.get_account_holdings_history(account_id, start, end, &granularity, &fx)?;
        let preferred = fx.preferred().to_string();
        (symbols, rows, preferred)
    };

    Ok(Json(serde_json::json!({
        "preferred_currency": preferred_currency,
        "symbols": symbols,
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

    let db = state.db();
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
    /// Comma-separated leaf category IDs to exclude from income/spending totals.
    pub exclude_category_ids: Option<String>,
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
    let exclude_category_ids: Vec<String> = q
        .exclude_category_ids
        .as_deref()
        .and_then(split_csv_param)
        .unwrap_or_default();

    let (rows, preferred_currency) = {
        let db = state.db();
        let currencies = db.get_currencies()?;
        let fx = FxRateMap::new(currencies)?;
        let rows = db.get_cash_flow(
            start,
            end,
            profile_id,
            &granularity,
            &exclude_category_ids,
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

// ── POST /api/holdings/import ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct HoldingsImportQuery {
    pub dry_run: Option<bool>,
}

/// Validate each write-union holding and flatten into storable `Holding`s,
/// surfacing the first validation failure as a 400.
fn holdings_from_writes(
    writes: Vec<HoldingWrite>,
    account_id: &str,
) -> Result<Vec<Holding>, AppError> {
    writes
        .into_iter()
        .map(|w| w.into_holding(account_id))
        .collect::<Result<Vec<_>, String>>()
        .map_err(|e| AppError::bad_request(e, "invalid_holding"))
}

pub async fn import_holdings(
    State(state): State<AppState>,
    auth: Extension<AuthContext>,
    Query(q): Query<HoldingsImportQuery>,
    Json(payload): Json<HoldingsWritePayload>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_token_if_remote(&state, &auth)?;
    let db = state.db();

    let account_id = payload.account_id;
    if !db.account_exists(&account_id)? {
        return Err(AppError::bad_request(
            format!("account {account_id} not found"),
            "account_not_found",
        ));
    }

    let holdings = holdings_from_writes(payload.holdings, &account_id)?;

    // Validate currencies for all holdings.
    for holding in &holdings {
        validate_currency(&db, &holding.currency)?;
    }

    if q.dry_run.unwrap_or(false) {
        let previews = db.dry_run_holdings(&account_id, &holdings)?;
        return Ok(Json(serde_json::json!({
            "dry_run": true,
            "preview": { "total": previews.len(), "snapshots": previews },
            "commit_payload": { "account_id": account_id, "holdings": holdings }
        })));
    }

    db.upsert_holdings(&account_id, &holdings)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "holdings_imported": holdings.len()
    })))
}

// ── POST /api/holdings/:account_id ────────────────────────────────────────────

pub async fn post_holdings(
    State(state): State<AppState>,
    auth: Extension<AuthContext>,
    Path(account_id): Path<String>,
    Json(body): Json<Vec<HoldingWrite>>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_token_if_remote(&state, &auth)?;

    let holdings = holdings_from_writes(body, &account_id)?;

    let holdings_updated = {
        let db = state.db();
        if db.get_account_by_id(&account_id)?.is_none() {
            return Err(AppError::NotFound(format!(
                "account {account_id} not found"
            )));
        }

        // Validate currencies for all holdings.
        for holding in &holdings {
            validate_currency(&db, &holding.currency)?;
        }

        db.replace_holdings(&account_id, &holdings)?
    };

    Ok(Json(serde_json::json!({
        "ok": true,
        "holdings_updated": holdings_updated
    })))
}

// ── GET /api/holdings/:account_id/:symbol ────────────────────────────────────

pub async fn get_holding_history(
    State(state): State<AppState>,
    Path((account_id, symbol)): Path<(String, String)>,
) -> Result<Json<Vec<Holding>>, AppError> {
    let db = state.db();
    let snapshots = db.get_holding_snapshots(&account_id, &symbol)?;
    Ok(Json(snapshots))
}

// ── DELETE /api/holdings/:account_id/:symbol ─────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DeleteHoldingQuery {
    pub as_of: String,
    pub sub_account: Option<String>,
}

pub async fn delete_holding_handler(
    State(state): State<AppState>,
    auth: Extension<AuthContext>,
    Path((account_id, symbol)): Path<(String, String)>,
    Query(q): Query<DeleteHoldingQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_token_if_remote(&state, &auth)?;
    let as_of = parse_naive_datetime(&q.as_of)?
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string();
    let db = state.db();
    let rows = db.delete_holding(&account_id, &symbol, &as_of, q.sub_account.as_deref())?;
    if rows == 0 {
        return Err(AppError::NotFound(format!(
            "no holding found for account={account_id} symbol={symbol} as_of={as_of}"
        )));
    }
    Ok(Json(
        serde_json::json!({ "ok": true, "rows_deleted": rows }),
    ))
}

// ── PATCH /api/holdings/:account_id/:symbol ──────────────────────────────────
//
// Scope (which row to update) is supplied via the body's `as_of` and the
// optional `sub_account` field. New field values use the `new_*` prefix so
// they don't collide with the scoping `sub_account`. To rename a holding's
// sub-account in place, pass the current value in `sub_account` and the
// desired value in `new_sub_account` (empty string means "set to null").

#[derive(Debug, Deserialize)]
pub struct PatchHoldingRequest {
    /// Snapshot timestamp identifying the row to update.
    pub as_of: String,
    /// Current sub-account label for the row (used together with `as_of` to scope).
    pub sub_account: Option<String>,
    /// If set, close (true) or reopen (false) the row.
    pub is_closed: Option<bool>,
    /// New scalar value for the holding.
    #[serde(with = "rust_decimal::serde::str_option", default)]
    pub value: Option<Decimal>,
    /// New currency code for the holding.
    pub currency: Option<String>,
    /// New sub-account label. Empty string sets it to NULL.
    pub new_sub_account: Option<String>,
}

pub async fn patch_holding(
    State(state): State<AppState>,
    Path((account_id, symbol)): Path<(String, String)>,
    Json(body): Json<PatchHoldingRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let as_of = parse_naive_datetime(&body.as_of)?;

    if body.is_closed.is_none()
        && body.value.is_none()
        && body.currency.is_none()
        && body.new_sub_account.is_none()
    {
        return Err(AppError::bad_request("nothing to update", "empty_patch"));
    }

    let db = state.db();

    if let Some(ref c) = body.currency {
        validate_currency(&db, c)?;
    }

    let new_sub_account = body
        .new_sub_account
        .as_deref()
        .map(|s| if s.is_empty() { None } else { Some(s) });

    let mut rows_updated: u64 = 0;
    rows_updated += db.update_holding_fields(
        &account_id,
        &symbol,
        body.sub_account.as_deref(),
        as_of,
        body.value,
        body.currency.as_deref(),
        new_sub_account,
    )?;

    if let Some(close) = body.is_closed {
        // After a sub_account rename, scope for close/reopen follows the new label.
        let scope_sub = match new_sub_account {
            Some(opt) => opt,
            None => body.sub_account.as_deref(),
        };
        let n = if close {
            db.close_holding(&account_id, &symbol, scope_sub, as_of)?
        } else {
            db.reopen_holding(&account_id, &symbol, scope_sub, as_of)?
        };
        rows_updated = rows_updated.max(n);
    }

    if rows_updated == 0 {
        return Err(AppError::NotFound(format!(
            "no holding found for account={account_id} symbol={symbol} as_of={}",
            as_of.format("%Y-%m-%dT%H:%M:%S")
        )));
    }

    Ok(Json(
        serde_json::json!({ "ok": true, "rows_updated": rows_updated }),
    ))
}
