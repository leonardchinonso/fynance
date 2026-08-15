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

#[derive(Debug, Deserialize)]
pub struct CapitalGainsQuery {
    pub account_id: Option<String>,
    pub symbol: Option<String>,
    pub start_date: Option<String>,  // YYYY-MM-DD
    pub end_date: Option<String>,    // YYYY-MM-DD
    pub tax_year: Option<String>,    // e.g. "2024-25" -> 6 Apr 2024 to 5 Apr 2025
    pub as_at: Option<String>,       // YYYY-MM-DD (limit calculations to this date)
    pub profile_ids: Option<String>, // comma-separated; scope to accounts whose profile_ids JSON intersects this set
}

#[derive(Debug, Deserialize)]
pub struct S104PoolsQuery {
    pub as_at: Option<String>,       // YYYY-MM-DD
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

// ── API Response Models ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct CapitalGainsResponse {
    pub summary: CgtSummary,
    pub symbol_summaries: Vec<SymbolSummary>,
    pub realized_events: Vec<CgtRealizedEvent>,
    pub pools: Vec<S104PoolState>,
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
    /// Native currency of `total_allowable_expenditure` and `average_cost_per_share`, which are
    /// held in the symbol's own currency rather than the preferred one. Mandatory on purpose: a
    /// pool always has at least one event to read it from, and making it optional would push the
    /// ambiguity onto every consumer — which is the bug it exists to fix, since a symbol sitting
    /// in the pool with no disposals in the window would otherwise be rendered against the base
    /// currency and silently mislabelled.
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

// ── Helper functions for UK tax year date conversion ─────────────────────────

fn parse_tax_year(tax_year: &str) -> Option<(NaiveDate, NaiveDate)> {
    // Expected format YYYY-YY or YYYY-YYYY, e.g. "2024-25" or "2024-2025"
    let parts: Vec<&str> = tax_year.split('-').collect();
    if parts.len() != 2 {
        return None;
    }

    let start_year_str = parts[0];
    let start_year = start_year_str.parse::<i32>().ok()?;

    let end_year = if parts[1].len() == 2 {
        let prefix = start_year / 100;
        let suffix = parts[1].parse::<i32>().ok()?;
        prefix * 100 + suffix
    } else {
        parts[1].parse::<i32>().ok()?
    };

    if end_year != start_year + 1 {
        return None;
    }

    let start_date = NaiveDate::from_ymd_opt(start_year, 4, 6)?;
    let end_date = NaiveDate::from_ymd_opt(end_year, 4, 5)?;

    Some((start_date, end_date))
}

// ── S104 Pool state calculations ─────────────────────────────────────────────

pub async fn get_s104_pools(
    State(state): State<AppState>,
    Query(q): Query<S104PoolsQuery>,
) -> Result<Json<Vec<S104PoolState>>, AppError> {
    let as_at = q
        .as_at
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
    let pools = run_cgt_engine(events, &excluded_accounts, as_at, None, None, &fx)?;

    Ok(Json(pools.pools))
}

// ── Capital Gains Tax calculation endpoint ───────────────────────────────────

pub async fn get_capital_gains(
    State(state): State<AppState>,
    Query(q): Query<CapitalGainsQuery>,
) -> Result<Json<CapitalGainsResponse>, AppError> {
    let as_at = q
        .as_at
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(parse_date)
        .transpose()?;

    // Date range logic
    let (filter_start, filter_end) = match q.tax_year.as_deref().filter(|s| !s.is_empty()) {
        Some(ty) => parse_tax_year(ty)
            .map(|(s, e)| (Some(s), Some(e)))
            .ok_or_else(|| {
                AppError::bad_request(
                    format!("invalid tax_year format: {ty} (expected YYYY-YY, e.g. 2024-25)"),
                    "invalid_tax_year",
                )
            })?,
        None => {
            let start = q
                .start_date
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(parse_date)
                .transpose()?;
            let end = q
                .end_date
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(parse_date)
                .transpose()?;
            (start, end)
        }
    };

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

    let mut response = run_cgt_engine(
        events,
        &excluded_accounts,
        as_at,
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

        // Pool figures stay in the symbol's native currency, so record which one that is while
        // every event is still to hand. A symbol group is only created by pushing an event, so
        // `first()` is always populated; the fallback exists to keep this total rather than
        // introduce an unwrap. Events for one symbol are expected to share a currency — if that
        // ever stops holding, this reports the earliest and the mismatch belongs in the warnings
        // channel (plan 23 §7.8) rather than in a nullable field.
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

            // Process unmatched short sale/unmatched disposals
            let total_matched: Decimal = matches_list.iter().map(|m| m.quantity).sum();
            if total_matched < e.quantity {
                let unmatched_qty = e.quantity - total_matched;
                let (m_proceeds, fee_prop_matched) = e.calculate_matched_finance(unmatched_qty, fx);

                all_realized.push(CgtRealizedEvent {
                    symbol: symbol.clone(),
                    disposal_id: e.id.clone(),
                    disposal_date: e.date.to_string(),
                    quantity: unmatched_qty,
                    disposal_price: e.price_per_share,
                    proceeds: m_proceeds - fee_prop_matched,
                    cost_basis: Decimal::ZERO,
                    gain_loss: m_proceeds - fee_prop_matched,
                    rule_applied: "Unmatched".to_string(),
                    original_currency: e.currency.clone(),
                    matches: vec![CgtMatchDetail {
                        acquisition_id: None,
                        acquisition_date: None,
                        quantity: unmatched_qty,
                        price: Decimal::ZERO,
                    }],
                });
            }
        }
    }

    // Sort realized events chronologically
    all_realized.sort_by(|a, b| a.disposal_date.cmp(&b.disposal_date));

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
        pools: all_pools,
    })
}
