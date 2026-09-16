//! Pre-flight refusals for a CGT report.
//!
//! Every check here runs BEFORE the engine and turns a condition that would
//! otherwise produce quietly-wrong numbers into an actionable 4xx. The engine
//! itself assumes these have already passed.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use ts_rs::TS;

use crate::model::{Account, InvestmentEvent, InvestmentEventType};
use crate::server::error::AppError;
use crate::util::fx::{FxRateMap, MissingRate};

use super::internal::CalEvent;

/// Reject the request up-front if any in-scope investment event references a
/// currency that isn't configured. Without this check the engine still runs,
/// `FxRateMap::convert` returns the amount unchanged, and totals quietly skew —
/// surfacing it as an actionable 400 lets the user add the missing rows under
/// Settings → Currencies before they look at numbers that pretend to be correct.
pub(crate) fn check_required_currencies(
    events: &[InvestmentEvent],
    fx: &FxRateMap,
) -> Result<(), AppError> {
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

/// Every `(currency, date)` pair the engine will need a rate for, given the event set it is
/// about to process.
///
/// **This walks the events, not the requested window, and that distinction is the whole
/// point.** The S104 pool is built from *every* acquisition ever made, so the cost basis of a
/// disposal in 2024-25 depends on rates going back as far as the ledger goes. Collecting only
/// the dates inside the requested date range would leave the pool built at the wrong rates and
/// silently produce a wrong cost basis — the report would look complete and be wrong, which is
/// the failure mode this whole feature exists to eliminate. Measured on the real ledger: a
/// 2024-25 report needs 49 pairs, of which only 17 are disposal dates in the year; the other 32
/// are cumulative acquisitions from earlier years.
///
/// `events` must therefore be the post-exclusion, post-`as_at` set — the same one
/// `run_cgt_engine` iterates — but must NOT have been narrowed to `filter_start`/`filter_end`,
/// which only govern which disposals are *emitted*.
///
/// Mirrors the conversion sites in the engine exactly:
///   * every *converting* event's trade currency at its own date (acquisitions into the pool,
///     disposal proceeds, and same-day cost). `Split` and `Transfer` are skipped: neither
///     performs a conversion, so a rate for their dates would never be applied.
///   * a non-zero fee's currency at that same date, which may differ from the trade currency
///   * for a 30-day match, the *acquisition* date rather than the disposal date, because HMRC
///     matches that leg at its own acquisition-date rate
pub(crate) fn required_rate_pairs(
    events: &[CalEvent],
    fx: &FxRateMap,
) -> BTreeSet<(String, NaiveDate)> {
    let preferred = fx.preferred();
    let mut pairs: BTreeSet<(String, NaiveDate)> = BTreeSet::new();

    for e in events {
        // `Split` and `Transfer` never reach a conversion, so demanding a rate for their dates
        // blocks a legitimate ledger on a date with no economic meaning. A reorganisation
        // (TCGA 1992 s.126-131) only moves the share count — the engine's `Split` branch touches
        // `pool_shares` and never `pool_cost` — and `Transfer` is a no-op within one pool scope.
        // Neither is ever a match leg either: same-day grouping sorts only Buy/Vest into
        // `incoming` and Sell/Withhold into `outgoing`, and the 30-day loop guards both sides,
        // so `calculate_matched_finance` cannot be reached with one.
        //
        // Skipping them keeps this set a superset of the engine's real lookups — the property
        // `unreachable_missing_rate` depends on — while dropping only pairs that provably cannot
        // be looked up. Over-collecting was safe for the numbers but not for the user: it
        // refused the report with a 400 naming a date whose rate would never have been applied.
        if matches!(
            e.event_type,
            InvestmentEventType::Split | InvestmentEventType::Transfer
        ) {
            continue;
        }
        let date = e.date.date();
        if e.currency != preferred {
            pairs.insert((e.currency.clone(), date));
        }
        if !e.fee.is_zero() && e.fee_currency != preferred {
            pairs.insert((e.fee_currency.clone(), date));
        }
    }

    // 30-day matches convert the acquisition leg at the acquisition date. That date always
    // belongs to another event in this same set, so its trade currency is already covered
    // above — but the *disposal's* currency is what the engine converts at the acquisition
    // date (see the `m_cost` call in the 30-day branch), and that pair may not otherwise
    // exist. Enumerate it explicitly rather than relying on the two currencies matching.
    for e in events {
        if !matches!(
            e.event_type,
            InvestmentEventType::Sell | InvestmentEventType::Withhold
        ) {
            continue;
        }
        for m in &e.thirty_day_matches {
            if let Some(acq) = m.acquisition_date {
                if e.currency != preferred {
                    pairs.insert((e.currency.clone(), acq.date()));
                }
            }
        }
    }

    pairs
}

/// Reject the request up-front when any rate the report needs is not stored, listing **every**
/// missing pair in one response.
///
/// One round-trip has to tell the user everything they need to supply: making them discover ~49
/// missing rates one 400 at a time would be unusable, and it is what the pre-flight screen
/// renders. Deliberately distinct from `missing_currencies`, which means something else — a
/// currency with no row in the `currencies` table at all.
///
/// The backend never invents a rate here. HMRC mandates no particular source, only that the
/// chosen basis is applied consistently, so a user-entered rate is fully legitimate and
/// auto-fetching would actively defeat the main use case (reproducing the rates a
/// previously-filed return was computed with).
pub(crate) fn check_required_exchange_rates(
    events: &[CalEvent],
    fx: &FxRateMap,
) -> Result<(), MissingExchangeRates> {
    let missing: Vec<MissingRatePair> = required_rate_pairs(events, fx)
        .into_iter()
        .filter(|(currency, date)| !fx.has_rate_as_of(currency, *date))
        .map(|(currency, date)| MissingRatePair {
            currency,
            date: date.to_string(),
        })
        .collect();

    if missing.is_empty() {
        return Ok(());
    }
    Err(MissingExchangeRates {
        quote: fx.preferred().to_string(),
        missing,
    })
}

/// A rate went missing *after* the precheck said every one was present.
///
/// Not reachable through the HTTP surface: `check_required_exchange_rates` enumerates the same
/// conversion sites the engine uses, so anything it clears cannot then fail. If this ever fires
/// the two have drifted apart, which is a bug in this file and not something the user can fix by
/// entering a rate — hence a 500 rather than the actionable 400 the precheck raises.
pub(crate) fn unreachable_missing_rate(m: MissingRate) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "internal error: no exchange rate for {m} during CGT calculation, but the pre-check \
         reported none missing. required_rate_pairs() and the engine's conversion sites have \
         diverged."
    ))
}

/// One `(currency, date)` pair with no stored rate.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct MissingRatePair {
    pub currency: String,
    /// YYYY-MM-DD.
    pub date: String,
}

/// The structured payload behind a `missing_exchange_rates` error.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct MissingExchangeRates {
    /// The currency every missing rate must be quoted into — the preferred currency.
    pub quote: String,
    pub missing: Vec<MissingRatePair>,
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
/// ISA/pension exclusion. A joint *current* account, and a joint ISA whose
/// gains are tax-free anyway, are therefore not grounds to refuse a report they
/// contribute nothing to.
///
/// `as_at` must mirror the ledger truncation the engine will apply (see
/// [`run_cgt_engine`]): `/pools` replays events only up to and including that
/// date, so an account whose events all fall after it contributes nothing and
/// must not block the request. Pass `None` where the engine does — `/capital-gains`
/// never truncates the ledger, because the S104 pool is built from all history.
///
/// Note this is deliberately NOT narrowed by `filter_start`/`filter_end`: those
/// govern only which disposals are *emitted*, while every event still enters the
/// pool and can therefore make a returned figure ambiguous.
pub(crate) fn check_single_owner_accounts(
    accounts: &[Account],
    events: &[InvestmentEvent],
    excluded_accounts: &HashSet<String>,
    as_at: Option<NaiveDate>,
) -> Result<(), AppError> {
    let contributing: HashSet<&str> = events
        .iter()
        .filter(|e| match as_at {
            Some(limit_date) => e.date.date() <= limit_date,
            None => true,
        })
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
pub(crate) fn check_single_currency_per_symbol(events: &[InvestmentEvent]) -> Result<(), AppError> {
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
