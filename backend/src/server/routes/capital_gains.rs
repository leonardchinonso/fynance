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
use std::collections::{HashMap, HashSet};
use ts_rs::TS;

use crate::model::{AccountType, InvestmentEvent, InvestmentEventType};
use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::server::validation::{parse_date, validate_date_range};
use crate::util::fx::FxRateMap;

// ── Query Parameters ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CapitalGainsQuery {
    pub account_id: Option<String>,
    pub symbol: Option<String>,
    pub start_date: Option<String>, // YYYY-MM-DD
    pub end_date: Option<String>,   // YYYY-MM-DD
    pub tax_year: Option<String>,   // e.g. "2024-25" -> 6 Apr 2024 to 5 Apr 2025
    pub as_at: Option<String>,      // YYYY-MM-DD (limit calculations to this date)
}

#[derive(Debug, Deserialize)]
pub struct S104PoolsQuery {
    pub as_at: Option<String>, // YYYY-MM-DD
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
struct CalEvent {
    id: String,
    event_type: InvestmentEventType,
    date: NaiveDateTime,
    quantity: Decimal,
    price_per_share: Decimal,
    fee: Decimal,
    currency: String,

    // Tracking for matching algorithm
    remaining_qty: Decimal,
    same_day_matches: Vec<CgtMatchDetail>,
    thirty_day_matches: Vec<CgtMatchDetail>,
    pool_matches: Vec<CgtMatchDetail>,
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
            currency: e.currency,
            remaining_qty: e.quantity,
            same_day_matches: Vec::new(),
            thirty_day_matches: Vec::new(),
            pool_matches: Vec::new(),
        }
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
    let as_at = if let Some(ref d) = q.as_at {
        if !d.is_empty() {
            Some(parse_date(d)?)
        } else {
            None
        }
    } else {
        None
    };

    let db = state.db.lock().expect("db mutex poisoned");

    // Fetch all accounts to build ISA/pension filter
    let accounts = db.get_accounts(None)?;
    let excluded_accounts: HashSet<String> = accounts
        .into_iter()
        .filter(|a| {
            matches!(
                a.account_type,
                AccountType::InvestmentIsa | AccountType::Pension
            )
        })
        .map(|a| a.id)
        .collect();

    // Fetch all investment events
    let events = db.list_investment_events(None, None, None)?;

    // Compute pools
    let pools = run_cgt_engine(events, &excluded_accounts, as_at, None, None);

    Ok(Json(pools.pools))
}

// ── Capital Gains Tax calculation endpoint ───────────────────────────────────

pub async fn get_capital_gains(
    State(state): State<AppState>,
    Query(q): Query<CapitalGainsQuery>,
) -> Result<Json<CapitalGainsResponse>, AppError> {
    let as_at = if let Some(ref d) = q.as_at {
        if !d.is_empty() {
            Some(parse_date(d)?)
        } else {
            None
        }
    } else {
        None
    };

    // Date range logic
    let mut filter_start = None;
    let mut filter_end = None;

    if let Some(ref ty) = q.tax_year {
        if !ty.is_empty() {
            if let Some((start, end)) = parse_tax_year(ty) {
                filter_start = Some(start);
                filter_end = Some(end);
            } else {
                return Err(AppError::bad_request(
                    format!("invalid tax_year format: {ty} (expected YYYY-YY, e.g. 2024-25)"),
                    "invalid_tax_year",
                ));
            }
        }
    }

    if filter_start.is_none() {
        if let Some(ref s) = q.start_date {
            if !s.is_empty() {
                filter_start = Some(parse_date(s)?);
            }
        }
        if let Some(ref e) = q.end_date {
            if !e.is_empty() {
                filter_end = Some(parse_date(e)?);
            }
        }
    }

    if let (Some(s), Some(e)) = (filter_start, filter_end) {
        validate_date_range(s, e)?;
    }

    let db = state.db.lock().expect("db mutex poisoned");

    // Fetch all accounts to build ISA/pension filter
    let accounts = db.get_accounts(None)?;
    let excluded_accounts: HashSet<String> = accounts
        .into_iter()
        .filter(|a| {
            matches!(
                a.account_type,
                AccountType::InvestmentIsa | AccountType::Pension
            )
        })
        .map(|a| a.id)
        .collect();

    // Fetch investment events
    // If a specific symbol was requested, we could theoretically filter here,
    // but the global pools are per-symbol anyway, so we fetch all and calculate.
    let events = db.list_investment_events(q.account_id.as_deref(), q.symbol.as_deref(), None)?;

    // Load currency exchange rates for final base-currency summary normalization
    let currencies = db.get_currencies()?;
    let fx = FxRateMap::new(currencies)?;
    let base_currency = fx.preferred().to_string();

    let mut response = run_cgt_engine(events, &excluded_accounts, as_at, filter_start, filter_end);

    // Apply currency conversions to normalize summary totals to the preferred base currency
    let mut total_proceeds = Decimal::ZERO;
    let mut total_allowable_costs = Decimal::ZERO;
    let mut total_gains = Decimal::ZERO;
    let mut total_losses = Decimal::ZERO;

    let mut symbol_map: HashMap<String, SymbolSummary> = HashMap::new();

    for event in &response.realized_events {
        let p_converted = fx.convert(event.proceeds, &event.original_currency);
        let c_converted = fx.convert(event.cost_basis, &event.original_currency);
        let g_converted = fx.convert(event.gain_loss, &event.original_currency);

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

fn run_cgt_engine(
    raw_events: Vec<InvestmentEvent>,
    excluded_accounts: &HashSet<String>,
    as_at: Option<NaiveDate>,
    filter_start: Option<NaiveDate>,
    filter_end: Option<NaiveDate>,
) -> CapitalGainsResponse {
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

        // -- Same-Day Rule matching --
        // Find Same-Day pairs: disposals matched against acquisitions on the same calendar date.
        // We group events of the day and match them FIFO.
        let dates: HashSet<NaiveDate> = events.iter().map(|e| e.date.date()).collect();
        let mut sorted_dates: Vec<NaiveDate> = dates.into_iter().collect();
        sorted_dates.sort();

        for date in sorted_dates {
            let mut acquisitions: Vec<usize> = Vec::new();
            let mut disposals: Vec<usize> = Vec::new();

            for (idx, e) in events.iter().enumerate() {
                if e.date.date() == date {
                    if matches!(
                        e.event_type,
                        InvestmentEventType::Buy | InvestmentEventType::Vest
                    ) {
                        acquisitions.push(idx);
                    } else if matches!(
                        e.event_type,
                        InvestmentEventType::Sell | InvestmentEventType::Withhold
                    ) {
                        disposals.push(idx);
                    }
                }
            }

            // Match same-day FIFO
            for &d_idx in &disposals {
                for &a_idx in &acquisitions {
                    let d_rem = events[d_idx].remaining_qty;
                    let a_rem = events[a_idx].remaining_qty;
                    if d_rem > Decimal::ZERO && a_rem > Decimal::ZERO {
                        let matched = d_rem.min(a_rem);
                        events[d_idx].remaining_qty -= matched;
                        events[a_idx].remaining_qty -= matched;

                        let match_detail = CgtMatchDetail {
                            acquisition_id: Some(events[a_idx].id.clone()),
                            acquisition_date: Some(events[a_idx].date.to_string()),
                            quantity: matched,
                            price: events[a_idx].price_per_share,
                        };
                        events[d_idx].same_day_matches.push(match_detail);
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

                    let match_detail = CgtMatchDetail {
                        acquisition_id: Some(events[acq_idx].id.clone()),
                        acquisition_date: Some(events[acq_idx].date.to_string()),
                        quantity: matched,
                        price: events[acq_idx].price_per_share,
                    };
                    events[idx].thirty_day_matches.push(match_detail);
                }
            }
        }

        // -- S104 Pool Replay --
        // Chronological replay to maintain S104 state and complete matches
        let mut pool_shares = Decimal::ZERO;
        let mut pool_cost = Decimal::ZERO; // in original/native currency

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
                        let acq_cost = entering * e.price_per_share + prop_fee;

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

                            let match_detail = CgtMatchDetail {
                                acquisition_id: None,
                                acquisition_date: Some("S104 Pool".to_string()),
                                quantity: matched,
                                price: avg_cost,
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
                    let split_ratio = e.quantity;
                    if split_ratio > Decimal::ZERO {
                        pool_shares *= split_ratio;
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
            let mut proceeds_raw = Decimal::ZERO;
            let mut cost_basis_raw = Decimal::ZERO;
            let mut matches_list: Vec<CgtMatchDetail> = Vec::new();

            // Proportional fee on disposal
            let fee_prop = e.fee; // full disposal fee

            // Process same-day matches
            for m in e.same_day_matches {
                let m_proceeds = m.quantity * e.price_per_share;
                let m_cost = m.quantity * m.price;
                proceeds_raw += m_proceeds;
                cost_basis_raw += m_cost;

                matches_list.push(m.clone());

                all_realized.push(CgtRealizedEvent {
                    symbol: symbol.clone(),
                    disposal_id: e.id.clone(),
                    disposal_date: e.date.to_string(),
                    quantity: m.quantity,
                    disposal_price: e.price_per_share,
                    proceeds: m_proceeds - (fee_prop * (m.quantity / e.quantity)),
                    cost_basis: m_cost,
                    gain_loss: (m_proceeds - (fee_prop * (m.quantity / e.quantity))) - m_cost,
                    rule_applied: "Same-Day".to_string(),
                    original_currency: e.currency.clone(),
                    matches: vec![m],
                });
            }

            // Process 30-day matches
            for m in e.thirty_day_matches {
                let m_proceeds = m.quantity * e.price_per_share;
                let m_cost = m.quantity * m.price;
                proceeds_raw += m_proceeds;
                cost_basis_raw += m_cost;

                matches_list.push(m.clone());

                all_realized.push(CgtRealizedEvent {
                    symbol: symbol.clone(),
                    disposal_id: e.id.clone(),
                    disposal_date: e.date.to_string(),
                    quantity: m.quantity,
                    disposal_price: e.price_per_share,
                    proceeds: m_proceeds - (fee_prop * (m.quantity / e.quantity)),
                    cost_basis: m_cost,
                    gain_loss: (m_proceeds - (fee_prop * (m.quantity / e.quantity))) - m_cost,
                    rule_applied: "30-Day Rule".to_string(),
                    original_currency: e.currency.clone(),
                    matches: vec![m],
                });
            }

            // Process S104 pool matches
            for m in e.pool_matches {
                let m_proceeds = m.quantity * e.price_per_share;
                let m_cost = m.quantity * m.price;
                proceeds_raw += m_proceeds;
                cost_basis_raw += m_cost;

                matches_list.push(m.clone());

                all_realized.push(CgtRealizedEvent {
                    symbol: symbol.clone(),
                    disposal_id: e.id.clone(),
                    disposal_date: e.date.to_string(),
                    quantity: m.quantity,
                    disposal_price: e.price_per_share,
                    proceeds: m_proceeds - (fee_prop * (m.quantity / e.quantity)),
                    cost_basis: m_cost,
                    gain_loss: (m_proceeds - (fee_prop * (m.quantity / e.quantity))) - m_cost,
                    rule_applied: "S104 Pool".to_string(),
                    original_currency: e.currency.clone(),
                    matches: vec![m],
                });
            }

            // Process unmatched short sale/unmatched disposals
            let total_matched: Decimal = matches_list.iter().map(|m| m.quantity).sum();
            if total_matched < e.quantity {
                let unmatched_qty = e.quantity - total_matched;
                let m_proceeds = unmatched_qty * e.price_per_share;
                proceeds_raw += m_proceeds;

                all_realized.push(CgtRealizedEvent {
                    symbol: symbol.clone(),
                    disposal_id: e.id.clone(),
                    disposal_date: e.date.to_string(),
                    quantity: unmatched_qty,
                    disposal_price: e.price_per_share,
                    proceeds: m_proceeds - (fee_prop * (unmatched_qty / e.quantity)),
                    cost_basis: Decimal::ZERO,
                    gain_loss: m_proceeds - (fee_prop * (unmatched_qty / e.quantity)),
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

    CapitalGainsResponse {
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
    }
}
