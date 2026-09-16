//! The CGT matching engine.
//!
//! Implements the HMRC share identification rules in order: same-day matching,
//! the 30-day "bed and breakfast" rule, then the S104 average-cost pool.
//!
//! This module is pure. It takes an already-fetched `Vec<InvestmentEvent>` plus
//! an `&FxRateMap` and returns a value; it performs no I/O and never touches
//! `AppState`. That is load-bearing rather than incidental -- see the deadlock
//! note in `server::routes::capital_gains`.

use chrono::{Duration, NaiveDate};
use rust_decimal::Decimal;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::model::{InvestmentEvent, InvestmentEventType};
use crate::server::error::AppError;
use crate::util::fx::FxRateMap;

use super::guards::{check_required_exchange_rates, unreachable_missing_rate};
use super::models::{
    CalEvent, CapitalGainsResponse, CgtDisposalGroup, CgtMatchDetail, CgtRealizedEvent, CgtSummary,
    InternalMatch, S104PoolState,
};

/// Runs the HMRC matching rules over the event ledger.
///
/// Fallible on purpose: some ledger states have no honest answer, and the engine refuses them
/// rather than emitting a number that looks authoritative. A silently-wrong tax figure is the
/// failure mode this whole report exists to prevent, so ambiguity is surfaced as a 4xx the user
/// can act on. See plan 23 §0.2.
pub(crate) fn run_cgt_engine(
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

    // 3. For each symbol group, run the HMRC matching rules.
    //
    // Matching is done for EVERY symbol before any conversion happens, so the FX precheck
    // below sees the complete picture and can report every missing rate in one response.
    // Interleaving them would fail on the first symbol that needs an unstored rate and hide
    // the rest, turning a single pre-flight round-trip into one request per missing rate.
    let mut matched_groups: Vec<(String, Vec<CalEvent>)> = Vec::new();
    for (symbol, mut events) in symbol_groups {
        // Sort chronologically
        events.sort_by_key(|e| e.date);

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

        matched_groups.push((symbol, events));
    }

    // 3b. FX precheck — every rate the engine is about to need must already be stored.
    //
    // Runs here, after matching and before the first conversion, for two reasons: the 30-day
    // matches now exist so their acquisition-date requirements are known, and nothing has been
    // converted yet so no partially-computed figure can escape. Deliberately walks every event
    // in the ledger rather than just those in the requested window, because the S104 pool is
    // built from every acquisition ever — see `required_rate_pairs`.
    let all_matched_events: Vec<CalEvent> = matched_groups
        .iter()
        .flat_map(|(_, events)| events.iter().cloned())
        .collect();
    if let Err(missing) = check_required_exchange_rates(&all_matched_events, fx) {
        let count = missing.missing.len();
        let quote = missing.quote.clone();
        return Err(AppError::bad_request_with_details(
            format!(
                "This report needs {count} exchange rate{} that {} not been entered yet. \
                 Each disposal must be converted at its own date's rate, and each acquisition \
                 at the rate on the date it was acquired, so rates are needed for every \
                 acquisition in the pool — including those from earlier tax years. \
                 Supply the missing rates (quoted into {quote}) and generate the report again.",
                if count == 1 { "" } else { "s" },
                if count == 1 { "has" } else { "have" },
            ),
            "missing_exchange_rates",
            serde_json::to_value(&missing).unwrap_or_else(|_| serde_json::json!({})),
        ));
    }

    // 4. Replay the S104 pool and emit results, now that every rate is known to be present.
    for (symbol, mut events) in matched_groups {
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
                        let price_cost = fx
                            .convert_as_of(entering * e.price_per_share, &e.currency, e.date.date())
                            .map_err(unreachable_missing_rate)?;
                        let fee_cost = fx
                            .convert_as_of(prop_fee, &e.fee_currency, e.date.date())
                            .map_err(unreachable_missing_rate)?;
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
                let (m_proceeds, fee_prop_matched) = e
                    .calculate_matched_finance(m.quantity, fx)
                    .map_err(unreachable_missing_rate)?;

                let m_cost_raw = m.quantity * m.price;
                let m_cost = fx
                    .convert_as_of(m_cost_raw, &e.currency, e.date.date())
                    .map_err(unreachable_missing_rate)?;

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
                let (m_proceeds, fee_prop_matched) = e
                    .calculate_matched_finance(m.quantity, fx)
                    .map_err(unreachable_missing_rate)?;

                let acq_date = m
                    .acquisition_date
                    .map(|d| d.date())
                    .unwrap_or_else(|| e.date.date());

                let m_cost_raw = m.quantity * m.price;
                let m_cost = fx
                    .convert_as_of(m_cost_raw, &e.currency, acq_date)
                    .map_err(unreachable_missing_rate)?;

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
                let (m_proceeds, fee_prop_matched) = e
                    .calculate_matched_finance(m.quantity, fx)
                    .map_err(unreachable_missing_rate)?;

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
        tax: None,
    })
}

/// The calendar day of a `CgtRealizedEvent.disposal_date`, which is a
/// `NaiveDateTime` rendered by `to_string()` — i.e. `"YYYY-MM-DD HH:MM:SS"`.
///
/// Grouping is by *day*, never by instant: see [`group_disposals`]. Parsing the
/// date rather than slicing `[..10]` means a value that is not in the expected
/// shape cannot be silently truncated into a plausible-looking wrong day; a
/// malformed input falls back to the whole string, which cannot merge two
/// genuinely distinct days and is visible in the output rather than hidden.
pub(crate) fn disposal_day_of(disposal_date: &str) -> String {
    match NaiveDate::parse_from_str(
        &disposal_date.chars().take(10).collect::<String>(),
        "%Y-%m-%d",
    ) {
        Ok(d) => d.to_string(),
        Err(_) => disposal_date.to_string(),
    }
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
///
/// **The key is the calendar DAY, not the timestamp.** `CgtRealizedEvent.disposal_date`
/// carries a full `YYYY-MM-DD HH:MM:SS`, and `parse_iso_date` accepts `%Y-%m-%dT%H:%M:%S`,
/// so imported events can legitimately carry non-midnight times. Keying on the raw datetime
/// would split a morning and an afternoon sale of one holding into two groups and overstate
/// the SA108 disposal count — while the same-day matcher above, which keys on `e.date.date()`,
/// had already treated them as one. [`disposal_day_of`] normalises to the day so the grouper
/// and the matcher agree. `disposal_date` on the *group* is therefore date-only; the
/// per-event `CgtRealizedEvent.disposal_date` keeps its full timestamp.
pub(crate) fn group_disposals(realized_events: &[CgtRealizedEvent]) -> Vec<CgtDisposalGroup> {
    // BTreeMap keeps groups in (symbol, disposal_date) order without a separate sort pass.
    let mut groups: BTreeMap<(String, String), CgtDisposalGroup> = BTreeMap::new();

    for event in realized_events {
        let disposal_day = disposal_day_of(&event.disposal_date);
        let key = (event.symbol.clone(), disposal_day.clone());
        let group = groups.entry(key).or_insert_with(|| CgtDisposalGroup {
            symbol: event.symbol.clone(),
            disposal_date: disposal_day,
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
