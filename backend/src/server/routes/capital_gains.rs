//! Axum handlers for the Capital Gains Tax endpoints.
//!
//! This module is a thin adapter, not the engine. Its job is HTTP: parse and
//! validate query parameters, resolve profiles to accounts, fetch the rows the
//! calculation needs, hand them to [`crate::cgt`], and serialise the result.
//!
//! The tax mathematics -- HMRC same-day matching, the 30-day "bed and
//! breakfast" rule, and S104 average-cost pooling -- lives in
//! [`crate::cgt::engine`], with its pre-flight refusals in
//! [`crate::cgt::guards`] and its response types in [`crate::cgt::models`].
//!
//! DEADLOCK INVARIANT (see #109): take `state.db()` exactly ONCE per request
//! and reuse that guard. It is a non-reentrant `std::sync::Mutex`, so a second
//! acquisition on the same thread while the first guard is alive self-deadlocks
//! the request and wedges the whole server. This is also why the engine in
//! `crate::cgt` takes already-fetched data and holds no handle to `AppState`:
//! it has no way to re-acquire the lock, so the invariant cannot be broken from
//! inside the calculation.

use axum::Json;
use axum::extract::{Query, State};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

use crate::cgt::engine::{disposal_day_of, run_cgt_engine};
use crate::cgt::guards::{
    check_required_currencies, check_single_currency_per_symbol, check_single_owner_accounts,
};
use crate::cgt::models::{CapitalGainsResponse, CgtSummary, S104PoolState, SymbolSummary};
use crate::model::{Account, AccountType, DerivedBroughtForwardLosses};
use crate::server::error::AppError;
use crate::server::routes::tax::validate_tax_year;
use crate::server::state::AppState;
use crate::server::validation::{parse_date, split_csv_param, validate_date_range};
use crate::tax::{DisposalForTax, compute_tax};
use crate::util::fx::FxRateMap;
// ── Query Parameters ─────────────────────────────────────────────────────────

// `tax_year` and `as_at` were removed from the wire format in favour of
// `start_date` / `end_date` alone — see plan 23 §0.2 (decision 7.3). The two
// dropped params looked interchangeable and were not: `as_at` truncated the
// *event ledger* before matching, so the 30-day rule could not reach forward
// to a later acquisition, while `end_date` only ever filtered which
// disposals were *emitted* — the pool still replayed through every later
// event, so the 30-day rule *could* reach forward. The same disposal got a
// different cost basis depending on which param the caller used, and
// nothing about the names told you that. "Tax year" is now frontend
// arithmetic (`start = YYYY-04-06`, `end = (YYYY+1)-04-05`) and "as at a
// date" is `end_date` alone — an absent `start_date` means "from time
// zero", which reproduces the old `as_at` semantic for the report use case.
#[derive(Debug, Deserialize)]
pub struct CapitalGainsQuery {
    pub account_id: Option<String>,
    pub symbol: Option<String>,
    pub start_date: Option<String>, // YYYY-MM-DD; absent = from time zero
    pub end_date: Option<String>,   // YYYY-MM-DD; absent = no upper bound
    pub profile_ids: Option<String>, // comma-separated; scope to accounts whose profile_ids JSON intersects this set
    /// `YYYY-YY`. When set, the response carries a `tax` computation for that
    /// year, using the statutory config and the profile's stored inputs.
    ///
    /// Separate from `start_date`/`end_date` on purpose: those bound which
    /// disposals are *reported*, and a caller is free to report a window that
    /// is not a tax year at all. Tax is only defined for a whole tax year, so
    /// asking for it is a distinct request rather than something inferred from
    /// a date range that happens to look like one.
    pub tax_year: Option<String>,
}

/// Query for `GET /api/investments/brought-forward-losses`.
#[derive(Debug, Deserialize)]
pub struct DerivedLossesQuery {
    /// The tax year the losses would be brought forward INTO. Only years
    /// strictly before it contribute.
    pub tax_year: String,
    /// Comma-separated profile IDs; same semantics as on `/capital-gains`.
    pub profile_ids: Option<String>,
    /// How many prior tax years to look back over. Defaults to 4.
    pub years: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct S104PoolsQuery {
    pub end_date: Option<String>, // YYYY-MM-DD; replay events up to and including this date only
    pub profile_ids: Option<String>, // comma-separated; same semantics as on /capital-gains
}

/// Resolve a comma-separated `profile_ids` query param into the matching set
/// of account IDs. Returns `None` when the filter is absent or empty, meaning
/// "all accounts" (engine behaviour unchanged). Returns `Some([])` when the
/// filter is set but matches no accounts (engine returns empty result).
fn resolve_profile_ids_to_account_ids(
    accounts: &[Account],
    profile_ids: Option<&str>,
) -> Option<Vec<String>> {
    let ids = profile_ids.and_then(split_csv_param)?;
    let pid_set: HashSet<String> = ids.into_iter().collect();
    let scoped: Vec<String> = accounts
        .iter()
        .filter(|a| a.profile_ids.iter().any(|p| pid_set.contains(p)))
        .map(|a| a.id.clone())
        .collect();
    Some(scoped)
}

// ── S104 Pool state calculations ─────────────────────────────────────────────

pub async fn get_s104_pools(
    State(state): State<AppState>,
    Query(q): Query<S104PoolsQuery>,
) -> Result<Json<Vec<S104PoolState>>, AppError> {
    // A pool snapshot has no "start" — it is a point-in-time replay of the
    // whole ledger, so `end_date` here truncates the ledger itself (the old
    // `as_at` behaviour), not merely which disposals are emitted. There is
    // nothing to emit-filter: this endpoint returns pool state, not disposals.
    let as_at = q
        .end_date
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(parse_date)
        .transpose()?;

    let db = state.db();

    // Fetch all accounts once; derive both the profile scope and the ISA/Pension exclusion from it.
    let accounts = db.get_accounts(None)?;
    let included_account_ids =
        resolve_profile_ids_to_account_ids(&accounts, q.profile_ids.as_deref());
    let excluded_accounts: HashSet<String> = accounts
        .iter()
        .filter(|a| {
            matches!(
                a.account_type,
                AccountType::InvestmentIsa | AccountType::Pension
            )
        })
        .map(|a| a.id.clone())
        .collect();

    let events = db.list_investment_events(None, None, None, included_account_ids.as_deref())?;

    // Compute pools. The pool's cost basis is built from acquisitions converted at their own
    // dates, so it needs the same date-keyed rates the full report does.
    let currencies = db.get_currencies()?;
    let fx = FxRateMap::new(currencies)?;
    let historical = db.get_exchange_rates_for_quote(fx.preferred())?;
    let fx = fx.with_historical(historical);
    check_required_currencies(&events, &fx)?;
    // Same `as_at` the engine truncates the ledger with, so the guard's scope is
    // exactly the events that will actually build the pools it is protecting.
    check_single_owner_accounts(&accounts, &events, &excluded_accounts, as_at)?;
    check_single_currency_per_symbol(&events)?;
    let pools = run_cgt_engine(events, &excluded_accounts, as_at, None, None, &fx)?;

    Ok(Json(pools.pools))
}

// ── Derived brought-forward losses ───────────────────────────────────────────

/// Suggest a brought-forward loss figure from prior years' disposals.
///
/// This is a PREFILL for a field the user confirms, never a value stored on
/// their behalf, and the response type says so: it carries the years it was
/// built from and an `is_upper_bound` flag that is always true.
///
/// It can only overstate. A UK capital loss carries forward only if it was
/// CLAIMED within four years of the end of the tax year it arose in, and only
/// the excess left after setting it against that same year's gains carries at
/// all. The ledger records neither the claim nor any disposal made outside this
/// app, so the honest thing to return is a bound with its working shown.
///
/// The default four-year lookback mirrors that claim window: a loss older than
/// that cannot now be claimed, so offering it would suggest something the user
/// cannot actually do.
pub async fn get_brought_forward_losses(
    State(state): State<AppState>,
    Query(q): Query<DerivedLossesQuery>,
) -> Result<Json<DerivedBroughtForwardLosses>, AppError> {
    validate_tax_year(&q.tax_year)?;

    let start_year: i32 = q.tax_year[..4].parse().map_err(|_| {
        AppError::bad_request(
            format!("tax_year must look like '2024-25', got {:?}", q.tax_year),
            "invalid_tax_year",
        )
    })?;
    let lookback = q.years.unwrap_or(4).min(20) as i32;

    // The UK tax year runs 6 April to 5 April. Build the boundaries for each
    // prior year in the window, newest last.
    let mut boundaries: Vec<(String, NaiveDate, NaiveDate)> = Vec::new();
    for offset in 1..=lookback {
        let y = start_year - offset;
        let label = format!("{y}-{:02}", (y + 1) % 100);
        let from = NaiveDate::from_ymd_opt(y, 4, 6)
            .ok_or_else(|| AppError::bad_request("tax year out of range", "invalid_tax_year"))?;
        let to = NaiveDate::from_ymd_opt(y + 1, 4, 5)
            .ok_or_else(|| AppError::bad_request("tax year out of range", "invalid_tax_year"))?;
        boundaries.push((label, from, to));
    }
    boundaries.reverse();

    let db = state.db();

    let accounts = db.get_accounts(None)?;
    let included_account_ids =
        resolve_profile_ids_to_account_ids(&accounts, q.profile_ids.as_deref());
    let excluded_accounts: HashSet<String> = accounts
        .iter()
        .filter(|a| {
            matches!(
                a.account_type,
                AccountType::InvestmentIsa | AccountType::Pension
            )
        })
        .map(|a| a.id.clone())
        .collect();

    let events = db.list_investment_events(None, None, None, included_account_ids.as_deref())?;

    let currencies = db.get_currencies()?;
    let fx = FxRateMap::new(currencies)?;
    let historical = db.get_exchange_rates_for_quote(fx.preferred())?;
    let fx = fx.with_historical(historical);
    check_required_currencies(&events, &fx)?;
    check_single_owner_accounts(&accounts, &events, &excluded_accounts, None)?;
    check_single_currency_per_symbol(&events)?;

    // The ledger is never truncated: the S104 pool and the 30-day rule must see
    // the full history for a prior year's gain to be computed correctly, exactly
    // as on the main report.
    let report = run_cgt_engine(events, &excluded_accounts, None, None, None, &fx)?;

    let realized: Vec<(String, Decimal)> = report
        .realized_events
        .iter()
        .map(|e| (disposal_day_of(&e.disposal_date), e.gain_loss))
        .collect();

    Ok(Json(
        db.derive_brought_forward_losses(&realized, &boundaries)?,
    ))
}

// ── Capital Gains Tax calculation endpoint ───────────────────────────────────

pub async fn get_capital_gains(
    State(state): State<AppState>,
    Query(q): Query<CapitalGainsQuery>,
) -> Result<Json<CapitalGainsResponse>, AppError> {
    // `tax_year` and `as_at` are gone from the wire format (plan 23 §0.2,
    // decision 7.3). The engine never truncates the event ledger here — the
    // S104 pool always replays every event regardless of `end_date`, so the
    // 30-day rule can always reach forward to a later acquisition. That is
    // now the *only* behaviour, rather than one of two depending on which
    // param the caller happened to use. Absent `start_date` means "from time
    // zero", which reproduces the old `as_at` semantic for the report use
    // case (a period with no lower bound).
    let filter_start = q
        .start_date
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(parse_date)
        .transpose()?;
    let filter_end = q
        .end_date
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(parse_date)
        .transpose()?;

    if let (Some(s), Some(e)) = (filter_start, filter_end) {
        validate_date_range(s, e)?;
    }

    let db = state.db();

    // Fetch all accounts once; derive both the profile scope and the ISA/Pension exclusion from it.
    let accounts = db.get_accounts(None)?;
    let included_account_ids =
        resolve_profile_ids_to_account_ids(&accounts, q.profile_ids.as_deref());
    let excluded_accounts: HashSet<String> = accounts
        .iter()
        .filter(|a| {
            matches!(
                a.account_type,
                AccountType::InvestmentIsa | AccountType::Pension
            )
        })
        .map(|a| a.id.clone())
        .collect();

    // Fetch investment events. account_ids (when set) narrows the SQL scope.
    // The global S104 pool is still per-symbol — the engine handles that — but its
    // input is now scoped to the requested profile set.
    let events = db.list_investment_events(
        q.account_id.as_deref(),
        q.symbol.as_deref(),
        None,
        included_account_ids.as_deref(),
    )?;

    // Load currency exchange rates for final base-currency summary normalization, plus the
    // date-keyed rates the engine converts each leg with.
    let currencies = db.get_currencies()?;
    let fx = FxRateMap::new(currencies)?;
    let base_currency = fx.preferred().to_string();
    let historical = db.get_exchange_rates_for_quote(&base_currency)?;
    let fx = fx.with_historical(historical);

    check_required_currencies(&events, &fx)?;
    // `None`, mirroring the engine call below: this endpoint never truncates the
    // ledger, so every event contributes to the pool and can make a figure ambiguous.
    check_single_owner_accounts(&accounts, &events, &excluded_accounts, None)?;
    check_single_currency_per_symbol(&events)?;

    let mut response = run_cgt_engine(
        events,
        &excluded_accounts,
        None, // no ledger truncation — see the `filter_start`/`filter_end` comment above
        filter_start,
        filter_end,
        &fx,
    )?;

    // Aggregate the per-event figures (already converted to the preferred base
    // currency by run_cgt_engine) into the summary and per-symbol totals.
    let mut total_proceeds = Decimal::ZERO;
    let mut total_allowable_costs = Decimal::ZERO;
    let mut total_gains = Decimal::ZERO;
    let mut total_losses = Decimal::ZERO;

    let mut symbol_map: HashMap<String, SymbolSummary> = HashMap::new();

    for event in &response.realized_events {
        let p_converted = event.proceeds;
        let c_converted = event.cost_basis;
        let g_converted = event.gain_loss;

        total_proceeds += p_converted;
        total_allowable_costs += c_converted;

        if g_converted > Decimal::ZERO {
            total_gains += g_converted;
        } else {
            total_losses += g_converted.abs();
        }

        let entry = symbol_map
            .entry(event.symbol.clone())
            .or_insert_with(|| SymbolSummary {
                symbol: event.symbol.clone(),
                total_proceeds: Decimal::ZERO,
                total_allowable_costs: Decimal::ZERO,
                total_gains: Decimal::ZERO,
                total_losses: Decimal::ZERO,
                net_gain_loss: Decimal::ZERO,
                original_currency: event.original_currency.clone(),
            });

        entry.total_proceeds += p_converted;
        entry.total_allowable_costs += c_converted;
        if g_converted > Decimal::ZERO {
            entry.total_gains += g_converted;
        } else {
            entry.total_losses += g_converted.abs();
        }
    }

    let net_gain_loss = total_gains - total_losses;

    response.summary = CgtSummary {
        total_proceeds,
        total_allowable_costs,
        total_gains,
        total_losses,
        net_gain_loss,
        base_currency,
    };

    let mut symbol_summaries = Vec::new();
    for (_, mut sym_sum) in symbol_map {
        sym_sum.net_gain_loss = sym_sum.total_gains - sym_sum.total_losses;
        symbol_summaries.push(sym_sum);
    }
    symbol_summaries.sort_by(|a, b| a.symbol.cmp(&b.symbol));

    response.symbol_summaries = symbol_summaries;

    // Tax, only when the caller asked for a specific tax year.
    //
    // Computed from `realized_events` rather than from `summary`: the bands are
    // keyed on disposal DATE, and the summary has already collapsed the dates
    // away. The rate that applies to a gain depends on when it was realized —
    // that is the whole point of the 30 October 2024 split — so a single netted
    // total cannot be bucketed after the fact.
    if let Some(tax_year) = q.tax_year.as_deref().filter(|s| !s.is_empty()) {
        validate_tax_year(tax_year)?;

        // Reuses the `db` guard taken at the top of this function. Do NOT call
        // `state.db()` here: it is a non-reentrant `std::sync::Mutex`, so a
        // second acquisition on this thread while the outer guard is still
        // alive self-deadlocks the request and wedges the whole server.
        let (entries, inputs) = {
            let entries = db.get_tax_config(tax_year)?;
            // Tax inputs are per profile. A request scoped to exactly one
            // profile uses that profile's stored figures; anything else (no
            // scope, or several profiles at once) has no single taxpayer to
            // read them from, so the documented defaults apply and the caller
            // gets a computation with no losses and the AEA claimed. Silently
            // borrowing one profile's losses for a multi-profile report would
            // understate somebody's tax.
            let profile_ids = q.profile_ids.as_deref().and_then(split_csv_param);
            let inputs = match profile_ids.as_deref() {
                Some([only]) => db.get_tax_inputs(only, tax_year)?,
                _ => db.get_tax_inputs("", tax_year)?,
            };
            (entries, inputs)
        };

        let disposals: Vec<DisposalForTax> = response
            .realized_events
            .iter()
            .map(|e| {
                let day = disposal_day_of(&e.disposal_date);
                NaiveDate::parse_from_str(&day, "%Y-%m-%d")
                    .map(|disposal_date| DisposalForTax {
                        disposal_date,
                        gain_loss: e.gain_loss,
                    })
                    .map_err(|_| {
                        AppError::bad_request(
                            format!("unparseable disposal date {:?}", e.disposal_date),
                            "invalid_disposal_date",
                        )
                    })
            })
            .collect::<Result<_, _>>()?;

        let computed = compute_tax(tax_year, &disposals, &entries, &inputs)
            .map_err(|e| AppError::bad_request(e.to_string(), "tax_computation_failed"))?;
        response.tax = Some(computed);
    }

    Ok(Json(response))
}
