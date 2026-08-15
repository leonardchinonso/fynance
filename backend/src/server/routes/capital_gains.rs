//! UK Capital Gains Tax (CGT) calculation engine and Axum handlers.
//!
//! Implements HMRC rules: same-day matching, 30-day Bed & Breakfast rule,
//! and global S104 average cost pooling, with multi-account pooling and
//! ISA/pension exclusions.

use axum::Json;
use axum::extract::{Query, State};
use chrono::{Duration, NaiveDate, NaiveDateTime};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use ts_rs::TS;

use crate::model::{Account, AccountType, InvestmentEvent, InvestmentEventType};
use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::server::validation::{parse_date, split_csv_param, validate_date_range};
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

/// Reject the request up-front if any in-scope investment event references a
/// currency that isn't configured. Without this check the engine still runs,
/// `FxRateMap::convert` returns the amount unchanged, and totals quietly skew —
/// surfacing it as an actionable 400 lets the user add the missing rows under
/// Settings → Currencies before they look at numbers that pretend to be correct.
fn check_required_currencies(events: &[InvestmentEvent], fx: &FxRateMap) -> Result<(), AppError> {
    let preferred = fx.preferred();
    let mut missing: Vec<String> = events
        .iter()
        .flat_map(|e| {
            let mut codes: Vec<&str> = vec![e.currency.as_str()];
            // A non-zero fee in its own currency adds a second requirement.
            if e.fee.is_some_and(|f| !f.is_zero()) {
                if let Some(fc) = e.fee_currency.as_deref() {
                    codes.push(fc);
                }
            }
            codes
        })
        .filter(|c| *c != preferred && fx.rate(c).is_none())
        .map(|c| c.to_string())
        .collect();
    missing.sort();
    missing.dedup();
    if missing.is_empty() {
        return Ok(());
    }
    let list = missing.join(", ");
    Err(AppError::bad_request(
        format!(
            "Some investment events use currencies not yet configured: {list}. \
             Add them under Settings → Currencies before generating this report."
        ),
        "missing_currencies",
    ))
}

/// Refuse the report when any in-scope account has more than one owner.
///
/// The S104 pool has no concept of shared ownership: it pools every event for a
/// symbol into one running cost, with no share of it attributable to a
/// particular person. So a joint account today returns 100% of the gain when
/// asked for owner A's figures, and the same 100% when asked for owner B's —
/// the same gain declared on two tax returns.
///
/// This refuses only the CGT computation, deliberately. Joint accounts are
/// lawful and stay fully representable: the data model, the account write path
/// and every other endpoint are untouched. Failing here scopes the breakage to
/// the one consumer for which the answer is genuinely ambiguous.
/// See docs/plans/23_capital_gains_post_v0.md §0.2 decision 7.2.
///
/// Scope is the accounts that actually contribute events to this computation —
/// derived from the events themselves, after profile filtering and after the
/// ISA/pension exclusion. Ope's joint *current* account, and a joint ISA whose
/// gains are tax-free anyway, are therefore not grounds to refuse a report they
/// contribute nothing to.
fn check_single_owner_accounts(
    accounts: &[Account],
    events: &[InvestmentEvent],
    excluded_accounts: &HashSet<String>,
) -> Result<(), AppError> {
    let contributing: HashSet<&str> = events
        .iter()
        .map(|e| e.account_id.as_str())
        .filter(|id| !excluded_accounts.contains(*id))
        .collect();

    let mut shared: Vec<String> = accounts
        .iter()
        .filter(|a| contributing.contains(a.id.as_str()) && a.profile_ids.len() > 1)
        .map(|a| format!("{} ({} owners)", a.name, a.profile_ids.len()))
        .collect();
    if shared.is_empty() {
        return Ok(());
    }
    shared.sort();
    let list = shared.join(", ");
    Err(AppError::bad_request(
        format!(
            "Cannot calculate capital gains for an investment account with multiple owners: \
             {list}. The S104 pool cannot split a gain between owners, so each owner would be \
             reported the full gain and it would be declared twice. Either narrow the report to \
             accounts with a single owner, or split the joint account into one account per owner \
             before generating this report."
        ),
        "multi_owner_account",
    ))
}

/// Refuse the report when one symbol's events carry more than one currency.
///
/// The S104 pool for a symbol is a single running total. Two currencies under
/// one symbol make that total a sum of, say, pence and pounds — a meaningless
/// figure that still renders as a confident cost basis. The write-time guard in
/// `routes::investments` stops new rows creating this; this precheck covers
/// rows written before that guard existed.
fn check_single_currency_per_symbol(events: &[InvestmentEvent]) -> Result<(), AppError> {
    let mut by_symbol: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for e in events {
        let seen = by_symbol.entry(e.symbol.as_str()).or_default();
        if !seen.contains(&e.currency.as_str()) {
            seen.push(e.currency.as_str());
        }
    }
    let conflicts: Vec<String> = by_symbol
        .into_iter()
        .filter(|(_, currencies)| currencies.len() > 1)
        .map(|(symbol, mut currencies)| {
            currencies.sort_unstable();
            format!("{symbol} ({})", currencies.join(", "))
        })
        .collect();
    if conflicts.is_empty() {
        return Ok(());
    }
    let list = conflicts.join("; ");
    Err(AppError::bad_request(
        format!(
            "These symbols have investment events in more than one currency: {list}. A symbol's \
             S104 pool is a single running total, so mixing currencies makes its cost basis \
             meaningless. Edit the affected events under Investments so each symbol uses one \
             currency — a holding priced in pence (GBX) and pounds (GBP) is the usual cause."
        ),
        "mixed_symbol_currency",
    ))
}

// ── API Response Models ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct CapitalGainsResponse {
    pub summary: CgtSummary,
    pub symbol_summaries: Vec<SymbolSummary>,
    pub realized_events: Vec<CgtRealizedEvent>,
    /// One row per actual sale — `realized_events` rolled up by `(symbol,
    /// disposal_date)`. See [`CgtDisposalGroup`] for why this exists
    /// alongside, not instead of, the granular rows.
    pub disposal_groups: Vec<CgtDisposalGroup>,
    pub pools: Vec<S104PoolState>,
}

/// A single real-world disposal, with HMRC's matching-rule buckets rolled back up.
///
/// `realized_events` emits one row per **matched bucket** — a sale of 500 shares that matches
/// 100 same-day + 50 under the 30-day rule + 350 from the S104 pool becomes three rows, because
/// the matching rules force three different cost-basis calculations. But nobody sold three
/// times, and SA108 box 23 ("number of disposals") wants the honest count. This groups by
/// `(symbol, disposal_date)` — the actual sale — and sums the constituent matches back together.
///
/// Deliberately NOT grouped by `rule_applied` (that's what `realized_events` already gives you —
/// it would just re-introduce the same artifact) and NOT by rate band (a tax-computation concern,
/// out of scope here — see plan 23 §7.7).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct CgtDisposalGroup {
    pub symbol: String,
    pub disposal_date: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub quantity: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub proceeds: Decimal, // in base currency, summed across matches
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub cost_basis: Decimal, // in base currency, summed across matches
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub gain_loss: Decimal, // proceeds - cost_basis
    pub original_currency: String,
    /// The individual matched-bucket rows this group rolls up. Same objects as in
    /// `realized_events` (by `disposal_id` + `rule_applied`), repeated here so a
    /// consumer that only fetched `disposal_groups` can still show the breakdown.
    pub events: Vec<CgtRealizedEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct SymbolSummary {
    pub symbol: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub total_proceeds: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub total_allowable_costs: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub total_gains: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub total_losses: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub net_gain_loss: Decimal,
    pub original_currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct CgtSummary {
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub total_proceeds: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub total_allowable_costs: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub total_gains: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub total_losses: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub net_gain_loss: Decimal,
    pub base_currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct CgtRealizedEvent {
    pub symbol: String,
    pub disposal_id: String,
    pub disposal_date: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub quantity: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub disposal_price: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub proceeds: Decimal, // in base currency (GBP)
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub cost_basis: Decimal, // in base currency (GBP)
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub gain_loss: Decimal,
    pub rule_applied: String, // "Same-Day" | "30-Day Rule" | "S104 Pool" | "Unmatched"
    pub original_currency: String,
    pub matches: Vec<CgtMatchDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct CgtMatchDetail {
    pub acquisition_id: Option<String>,
    pub acquisition_date: Option<String>,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub quantity: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub price: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct S104PoolState {
    pub symbol: String,
    /// Source metadata only: the currency the underlying trades were originally
    /// denominated in. It does NOT describe the currency of `total_allowable_expenditure`
    /// or `average_cost_per_share` — both of those are always in the preferred base
    /// currency (GBP), converted via `fx.convert_as_of` as each event enters the pool.
    /// Mirrors `CgtRealizedEvent.original_currency`, which is source metadata for the
    /// same reason: `proceeds`/`cost_basis` there are base-currency too. Mandatory on
    /// purpose: a pool always has at least one event to read it from, and making it
    /// optional would push the ambiguity onto every consumer — which is the bug it
    /// exists to fix, since a symbol sitting in the pool with no disposals in the
    /// window would otherwise have no source currency to report at all.
    pub original_currency: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub current_shares: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub total_allowable_expenditure: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub average_cost_per_share: Decimal,
}

// ── Internal Calculation Types ───────────────────────────────────────────────

#[derive(Debug, Clone)]
struct InternalMatch {
    pub acquisition_id: Option<String>,
    pub acquisition_date: Option<NaiveDateTime>,
    pub quantity: Decimal,
    pub price: Decimal,
    pub is_s104: bool,
}

impl InternalMatch {
    fn to_cgt_match_detail(&self) -> CgtMatchDetail {
        CgtMatchDetail {
            acquisition_id: self.acquisition_id.clone(),
            acquisition_date: if self.is_s104 {
                Some("S104 Pool".to_string())
            } else {
                self.acquisition_date.map(|d| d.date().to_string())
            },
            quantity: self.quantity,
            price: self.price,
        }
    }
}

#[derive(Debug, Clone)]
struct CalEvent {
    id: String,
    event_type: InvestmentEventType,
    date: NaiveDateTime,
    quantity: Decimal,
    price_per_share: Decimal,
    fee: Decimal,
    currency: String,
    /// Currency the fee is denominated in; may differ from `currency`.
    fee_currency: String,

    // Tracking for matching algorithm
    remaining_qty: Decimal,
    same_day_matches: Vec<InternalMatch>,
    thirty_day_matches: Vec<InternalMatch>,
    pool_matches: Vec<InternalMatch>,
}

impl From<InvestmentEvent> for CalEvent {
    fn from(e: InvestmentEvent) -> Self {
        CalEvent {
            id: e.id,
            event_type: e.event_type,
            date: e.date,
            quantity: e.quantity,
            price_per_share: e.price_per_share,
            fee: e.fee.unwrap_or(Decimal::ZERO),
            // A null fee_currency means the fee is in the trade currency. This is
            // only a defensive fallback; new rows carry a concrete value (defaulted
            // at write time and backfilled by migration).
            fee_currency: e.fee_currency.unwrap_or_else(|| e.currency.clone()),
            currency: e.currency,
            remaining_qty: e.quantity,
            same_day_matches: Vec::new(),
            thirty_day_matches: Vec::new(),
            pool_matches: Vec::new(),
        }
    }
}

impl CalEvent {
    /// Calculates the normalized proceeds and proportional fee in preferred base currency (GBP) for a matched quantity.
    fn calculate_matched_finance(&self, match_qty: Decimal, fx: &FxRateMap) -> (Decimal, Decimal) {
        let proceeds_raw = match_qty * self.price_per_share;
        let fee_raw = if self.quantity > Decimal::ZERO {
            self.fee * (match_qty / self.quantity)
        } else {
            Decimal::ZERO
        };

        let proceeds = fx.convert_as_of(proceeds_raw, &self.currency, self.date.date());
        let fee = fx.convert_as_of(fee_raw, &self.fee_currency, self.date.date());
        (proceeds, fee)
    }
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

    // Compute pools
    let currencies = db.get_currencies()?;
    let fx = FxRateMap::new(currencies)?;
    check_required_currencies(&events, &fx)?;
    check_single_owner_accounts(&accounts, &events, &excluded_accounts)?;
    check_single_currency_per_symbol(&events)?;
    let pools = run_cgt_engine(events, &excluded_accounts, as_at, None, None, &fx)?;

    Ok(Json(pools.pools))
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

    // Load currency exchange rates for final base-currency summary normalization
    let currencies = db.get_currencies()?;
    let fx = FxRateMap::new(currencies)?;
    let base_currency = fx.preferred().to_string();

    check_required_currencies(&events, &fx)?;
    check_single_owner_accounts(&accounts, &events, &excluded_accounts)?;
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

    Ok(Json(response))
}

// ── Core Engine Replay ────────────────────────────────────────────────────────

/// Runs the HMRC matching rules over the event ledger.
///
/// Fallible on purpose: some ledger states have no honest answer, and the engine refuses them
/// rather than emitting a number that looks authoritative. A silently-wrong tax figure is the
/// failure mode this whole report exists to prevent, so ambiguity is surfaced as a 4xx the user
/// can act on. See plan 23 §0.2.
fn run_cgt_engine(
    raw_events: Vec<InvestmentEvent>,
    excluded_accounts: &HashSet<String>,
    as_at: Option<NaiveDate>,
    filter_start: Option<NaiveDate>,
    filter_end: Option<NaiveDate>,
    fx: &FxRateMap,
) -> Result<CapitalGainsResponse, AppError> {
    // 1. Filter out sheltered/excluded accounts and respect `as_at`
    let filtered_events: Vec<InvestmentEvent> = raw_events
        .into_iter()
        .filter(|e| {
            if excluded_accounts.contains(&e.account_id) {
                return false;
            }
            if let Some(limit_date) = as_at {
                if e.date.date() > limit_date {
                    return false;
                }
            }
            true
        })
        .collect();

    // 2. Group events by symbol
    let mut symbol_groups: HashMap<String, Vec<CalEvent>> = HashMap::new();
    for event in filtered_events {
        symbol_groups
            .entry(event.symbol.clone())
            .or_default()
            .push(event.into());
    }

    let mut all_realized: Vec<CgtRealizedEvent> = Vec::new();
    let mut all_pools: Vec<S104PoolState> = Vec::new();

    // 3. For each symbol group, run the HMRC matching rules
    for (symbol, mut events) in symbol_groups {
        // Sort chronologically
        events.sort_by_key(|e| e.date);

        // Record the symbol's source trade currency as metadata while every event is
        // still to hand — pool_cost itself is always converted to base currency (GBP)
        // below, this is not the currency it is held in. A symbol group is only created
        // by pushing an event, so `first()` is always populated; the fallback exists to
        // keep this total rather than introduce an unwrap. Events for one symbol are
        // expected to share a currency — if that ever stops holding, this reports the
        // earliest and the mismatch belongs in the warnings channel (plan 23 §7.8)
        // rather than in a nullable field.
        let pool_currency = events
            .first()
            .map(|e| e.currency.clone())
            .unwrap_or_else(|| fx.preferred().to_string());

        // -- Same-Day Rule matching --
        // Find Same-Day pairs: disposals matched against acquisitions on the same calendar date.
        // We group events of the day and match them FIFO.
        #[derive(Default)]
        struct EventIndices {
            incoming: Vec<usize>, // Buy/Vest
            outgoing: Vec<usize>, // Sell/Withhold
        }

        let mut daily_groups: BTreeMap<NaiveDate, EventIndices> = BTreeMap::new();

        for (idx, e) in events.iter().enumerate() {
            let date = e.date.date();
            let group = daily_groups.entry(date).or_default();

            match e.event_type {
                InvestmentEventType::Buy | InvestmentEventType::Vest => group.incoming.push(idx),
                // Sell and Withhold are both treated as disposals. Withhold (sell-to-cover or net
                // settlement) represents shares sold at vest to cover income tax, which is a
                // disposal under UK CGT.
                //
                // DELIBERATE DIVERGENCE: some practitioners leave sell-to-cover out of the disposal
                // schedule entirely, reasoning that same-day matching nets the gain to ~zero so the
                // tax due is unchanged either way. We include them: they are disposals in law, and
                // omitting them understates both the disposal count and total proceeds. Reports
                // generated here will therefore not tie to a return prepared the other way — that
                // difference is intentional and is not a bug to be "fixed".
                // See docs/design/08_cgt_engine.md § Deliberate Divergences from Common Practice.
                InvestmentEventType::Sell | InvestmentEventType::Withhold => {
                    group.outgoing.push(idx)
                }
                _ => {}
            }
        }

        for date in daily_groups.keys() {
            if let Some(group) = daily_groups.get(date) {
                // Match same-day FIFO
                for &d_idx in &group.outgoing {
                    for &a_idx in &group.incoming {
                        let d_rem = events[d_idx].remaining_qty;
                        let a_rem = events[a_idx].remaining_qty;
                        if d_rem > Decimal::ZERO && a_rem > Decimal::ZERO {
                            let matched = d_rem.min(a_rem);
                            events[d_idx].remaining_qty -= matched;
                            events[a_idx].remaining_qty -= matched;

                            let match_detail = InternalMatch {
                                acquisition_id: Some(events[a_idx].id.clone()),
                                acquisition_date: Some(events[a_idx].date),
                                quantity: matched,
                                price: events[a_idx].price_per_share,
                                is_s104: false,
                            };
                            events[d_idx].same_day_matches.push(match_detail);
                        }
                    }
                }
            }
        }

        // -- 30-Day Rule matching (Bed & Breakfast) --
        // Match disposal D against acquisitions occurring in the 30 days *after* D (days D+1 to D+30).
        for idx in 0..events.len() {
            if !matches!(
                events[idx].event_type,
                InvestmentEventType::Sell | InvestmentEventType::Withhold
            ) {
                continue;
            }
            if events[idx].remaining_qty == Decimal::ZERO {
                continue;
            }

            let disposal_date = events[idx].date.date();
            let max_acq_date = disposal_date + Duration::days(30);

            // Search ahead for acquisitions
            for acq_idx in (idx + 1)..events.len() {
                if events[idx].remaining_qty == Decimal::ZERO {
                    break;
                }

                let acq_date = events[acq_idx].date.date();
                if acq_date <= disposal_date {
                    continue;
                }
                if acq_date > max_acq_date {
                    // Out of 30-day range
                    break;
                }

                if !matches!(
                    events[acq_idx].event_type,
                    InvestmentEventType::Buy | InvestmentEventType::Vest
                ) {
                    continue;
                }

                let d_rem = events[idx].remaining_qty;
                let a_rem = events[acq_idx].remaining_qty;
                if d_rem > Decimal::ZERO && a_rem > Decimal::ZERO {
                    let matched = d_rem.min(a_rem);
                    events[idx].remaining_qty -= matched;
                    events[acq_idx].remaining_qty -= matched;

                    let match_detail = InternalMatch {
                        acquisition_id: Some(events[acq_idx].id.clone()),
                        acquisition_date: Some(events[acq_idx].date),
                        quantity: matched,
                        price: events[acq_idx].price_per_share,
                        is_s104: false,
                    };
                    events[idx].thirty_day_matches.push(match_detail);
                }
            }
        }

        // -- S104 Pool Replay --
        // Chronological replay to maintain S104 state and complete matches
        let mut pool_shares = Decimal::ZERO;
        let mut pool_cost = Decimal::ZERO; // in preferred base currency (GBP)

        for e in &mut events {
            match e.event_type {
                InvestmentEventType::Buy | InvestmentEventType::Vest => {
                    // Unmatched quantity entering the pool
                    let entering = e.remaining_qty;
                    if entering > Decimal::ZERO {
                        let prop_fee = if e.quantity > Decimal::ZERO {
                            e.fee * (entering / e.quantity)
                        } else {
                            Decimal::ZERO
                        };
                        // Price and fee can be in different currencies, so convert
                        // each at its own rate before summing into the pool cost.
                        let price_cost = fx.convert_as_of(
                            entering * e.price_per_share,
                            &e.currency,
                            e.date.date(),
                        );
                        let fee_cost = fx.convert_as_of(prop_fee, &e.fee_currency, e.date.date());
                        let acq_cost = price_cost + fee_cost;

                        pool_shares += entering;
                        pool_cost += acq_cost;

                        e.remaining_qty = Decimal::ZERO;
                    }
                }
                InvestmentEventType::Sell | InvestmentEventType::Withhold => {
                    let exiting = e.remaining_qty;
                    if exiting > Decimal::ZERO {
                        let matched = exiting.min(pool_shares);
                        if matched > Decimal::ZERO {
                            let avg_cost = if pool_shares > Decimal::ZERO {
                                pool_cost / pool_shares
                            } else {
                                Decimal::ZERO
                            };

                            let pool_cost_basis = matched * avg_cost;

                            let match_detail = InternalMatch {
                                acquisition_id: None,
                                acquisition_date: None,
                                quantity: matched,
                                price: avg_cost,
                                is_s104: true,
                            };
                            e.pool_matches.push(match_detail);

                            pool_shares -= matched;
                            pool_cost -= pool_cost_basis;
                            if pool_shares == Decimal::ZERO {
                                pool_cost = Decimal::ZERO;
                            }

                            e.remaining_qty -= matched;
                        }
                    }
                }
                InvestmentEventType::Split => {
                    // `quantity` is the CHANGE in share count, not a ratio: a 10-for-1
                    // forward split on 1.72827619 shares is stored as +15.55448571, the
                    // shares added. Multiplying by it would inflate the pool (here:
                    // 25.17 shares instead of 17.28), understating average cost and
                    // overstating every later gain.
                    //
                    // A split or consolidation is a reorganisation (TCGA 1992 s.126-131):
                    // the new holding is the same asset, acquired at the same time and for
                    // the same cost. pool_cost is therefore never touched — only the share
                    // count moves, and average cost per share falls (split) or rises
                    // (consolidation) as a consequence.
                    //
                    // A consolidation removes shares, so `quantity` is negative: a 1-for-5
                    // on 100 shares is stored as -80. This branch used to require
                    // `quantity > 0`, so consolidations were silently dropped and the pool
                    // kept too many shares.
                    //
                    // Removing more shares than the pool holds is impossible, so it means
                    // the ledger is wrong — a mistyped quantity, a missing acquisition, or
                    // an event filed against the wrong symbol. Refuse rather than absorb
                    // it: clamping at zero would leave a pool of no shares but non-zero
                    // cost, making average cost zero and reporting 100% of the proceeds of
                    // every later disposal as gain. That overstates tax while looking
                    // perfectly ordinary.
                    let after = pool_shares + e.quantity;
                    if after < Decimal::ZERO {
                        return Err(AppError::bad_request(
                            format!(
                                "{symbol}: a share consolidation on {} removes {} shares but \
                                 the pool holds only {pool_shares}. Check the event quantity \
                                 (it is the change in share count, negative for a \
                                 consolidation) and that every acquisition has been imported.",
                                e.date.date(),
                                e.quantity.abs(),
                            ),
                            "consolidation_exceeds_pool",
                        ));
                    }
                    pool_shares = after;
                    // A consolidation to exactly zero is legitimate data (a holding fully
                    // wound up), not an error — but it must clear pool_cost the same way
                    // the Sell branch does above. Left unhandled, the orphaned cost sits on
                    // a pool of zero shares; the next Buy restarts pool_shares from zero,
                    // so avg_cost carries that stale cost over the new shares instead of
                    // zero, and the following Sell reports the entire stale cost as
                    // allowable expenditure against a disposal that never earned it.
                    if pool_shares == Decimal::ZERO {
                        pool_cost = Decimal::ZERO;
                    }
                }
                InvestmentEventType::Transfer => {
                    // Internal transfers are neutral within a single S104 pool scope
                }
            }
        }

        // Save current pool state
        let final_avg = if pool_shares > Decimal::ZERO {
            pool_cost / pool_shares
        } else {
            Decimal::ZERO
        };
        all_pools.push(S104PoolState {
            symbol: symbol.clone(),
            original_currency: pool_currency,
            current_shares: pool_shares,
            total_allowable_expenditure: pool_cost,
            average_cost_per_share: final_avg,
        });

        // 4. Assemble realized events inside the filter range
        for e in events {
            if !matches!(
                e.event_type,
                InvestmentEventType::Sell | InvestmentEventType::Withhold
            ) {
                continue;
            }

            let date_check = e.date.date();
            if let Some(start) = filter_start {
                if date_check < start {
                    continue;
                }
            }
            if let Some(end) = filter_end {
                if date_check > end {
                    continue;
                }
            }

            // We compile all matches for this disposal
            let mut matches_list: Vec<CgtMatchDetail> = Vec::new();

            // Process same-day matches
            for m in &e.same_day_matches {
                let (m_proceeds, fee_prop_matched) = e.calculate_matched_finance(m.quantity, fx);

                let m_cost_raw = m.quantity * m.price;
                let m_cost = fx.convert_as_of(m_cost_raw, &e.currency, e.date.date());

                let api_match = m.to_cgt_match_detail();
                matches_list.push(api_match.clone());

                all_realized.push(CgtRealizedEvent {
                    symbol: symbol.clone(),
                    disposal_id: e.id.clone(),
                    disposal_date: e.date.to_string(),
                    quantity: m.quantity,
                    disposal_price: e.price_per_share,
                    proceeds: m_proceeds - fee_prop_matched,
                    cost_basis: m_cost,
                    gain_loss: (m_proceeds - fee_prop_matched) - m_cost,
                    rule_applied: "Same-Day".to_string(),
                    original_currency: e.currency.clone(),
                    matches: vec![api_match],
                });
            }

            // Process 30-day matches
            for m in &e.thirty_day_matches {
                let (m_proceeds, fee_prop_matched) = e.calculate_matched_finance(m.quantity, fx);

                let acq_date = m
                    .acquisition_date
                    .map(|d| d.date())
                    .unwrap_or_else(|| e.date.date());

                let m_cost_raw = m.quantity * m.price;
                let m_cost = fx.convert_as_of(m_cost_raw, &e.currency, acq_date);

                let api_match = m.to_cgt_match_detail();
                matches_list.push(api_match.clone());

                all_realized.push(CgtRealizedEvent {
                    symbol: symbol.clone(),
                    disposal_id: e.id.clone(),
                    disposal_date: e.date.to_string(),
                    quantity: m.quantity,
                    disposal_price: e.price_per_share,
                    proceeds: m_proceeds - fee_prop_matched,
                    cost_basis: m_cost,
                    gain_loss: (m_proceeds - fee_prop_matched) - m_cost,
                    rule_applied: "30-Day Rule".to_string(),
                    original_currency: e.currency.clone(),
                    matches: vec![api_match],
                });
            }

            // Process S104 pool matches
            for m in &e.pool_matches {
                let (m_proceeds, fee_prop_matched) = e.calculate_matched_finance(m.quantity, fx);

                // m.price is avg_cost which is ALREADY in preferred base currency (GBP)
                let m_cost = m.quantity * m.price;

                let api_match = m.to_cgt_match_detail();
                matches_list.push(api_match.clone());

                all_realized.push(CgtRealizedEvent {
                    symbol: symbol.clone(),
                    disposal_id: e.id.clone(),
                    disposal_date: e.date.to_string(),
                    quantity: m.quantity,
                    disposal_price: e.price_per_share,
                    proceeds: m_proceeds - fee_prop_matched,
                    cost_basis: m_cost,
                    gain_loss: (m_proceeds - fee_prop_matched) - m_cost,
                    rule_applied: "S104 Pool".to_string(),
                    original_currency: e.currency.clone(),
                    matches: vec![api_match],
                });
            }

            // A disposal that exhausts all three HMRC matching rules — same-day,
            // 30-day, and the S104 pool — has no acquisition to draw a cost from.
            //
            // This used to be emitted as a row with `cost_basis = 0` and
            // rule_applied "Unmatched", which counts 100% of the proceeds as gain
            // and OVERSTATES the tax due. It looks like an ordinary line on the
            // report, so nothing signals that the number is wrong.
            //
            // In law this shape is a short sale, but a retail portfolio effectively
            // never contains one: in practice it always means acquisition data is
            // missing — an un-imported statement, a wrong symbol, or a transfer-in
            // recorded without its original cost. Refuse, and name the symbol, date
            // and quantity so the missing acquisition can actually be found.
            let total_matched: Decimal = matches_list.iter().map(|m| m.quantity).sum();
            if total_matched < e.quantity {
                let unmatched_qty = e.quantity - total_matched;
                return Err(AppError::bad_request(
                    format!(
                        "{symbol}: the disposal of {unmatched_qty} shares on {} has no matching \
                         acquisition, so there is no cost to set against it. Counting it as \
                         all-gain would overstate the tax due. Import or add the missing \
                         acquisition for {symbol} before this date, and check the disposal is \
                         filed under the right symbol.",
                        e.date.date(),
                    ),
                    "unmatched_disposal",
                ));
            }
        }
    }

    // Sort realized events chronologically
    all_realized.sort_by(|a, b| a.disposal_date.cmp(&b.disposal_date));

    let disposal_groups = group_disposals(&all_realized);

    Ok(CapitalGainsResponse {
        summary: CgtSummary {
            total_proceeds: Decimal::ZERO,
            total_allowable_costs: Decimal::ZERO,
            total_gains: Decimal::ZERO,
            total_losses: Decimal::ZERO,
            net_gain_loss: Decimal::ZERO,
            base_currency: "GBP".to_string(),
        },
        symbol_summaries: Vec::new(),
        realized_events: all_realized,
        disposal_groups,
        pools: all_pools,
    })
}

/// Rolls `realized_events` up by `(symbol, disposal_date)` into one row per actual sale.
///
/// `disposal_id` is deliberately NOT part of the grouping key: it identifies the underlying
/// investment event, and a single disposal event is exactly what we're collapsing multiple
/// matched-bucket rows back into — grouping by it would just reproduce `realized_events`
/// one-for-one. `(symbol, disposal_date)` is what HMRC treats as one same-day disposal in
/// aggregate; two distinct sell events for the same symbol on the same date are the same
/// disposal for reporting purposes (see the same-day FIFO matching above, which already
/// treats them jointly). Two disposals of the same symbol on *different* dates stay separate
/// groups — the date is part of the key precisely so they don't collapse into each other.
fn group_disposals(realized_events: &[CgtRealizedEvent]) -> Vec<CgtDisposalGroup> {
    // BTreeMap keeps groups in (symbol, disposal_date) order without a separate sort pass.
    let mut groups: BTreeMap<(String, String), CgtDisposalGroup> = BTreeMap::new();

    for event in realized_events {
        let key = (event.symbol.clone(), event.disposal_date.clone());
        let group = groups.entry(key).or_insert_with(|| CgtDisposalGroup {
            symbol: event.symbol.clone(),
            disposal_date: event.disposal_date.clone(),
            quantity: Decimal::ZERO,
            proceeds: Decimal::ZERO,
            cost_basis: Decimal::ZERO,
            gain_loss: Decimal::ZERO,
            original_currency: event.original_currency.clone(),
            events: Vec::new(),
        });
        group.quantity += event.quantity;
        group.proceeds += event.proceeds;
        group.cost_basis += event.cost_basis;
        group.gain_loss += event.gain_loss;
        group.events.push(event.clone());
    }

    groups.into_values().collect()
}
