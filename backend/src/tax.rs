//! UK Capital Gains Tax computation.
//!
//! This module turns the CGT engine's output — realized disposals, already
//! matched under HMRC's share-identification rules and already converted to the
//! base currency — into a tax figure. It is deliberately separate from
//! `server::routes::capital_gains`: that module answers "what did you dispose of
//! and what was the gain", which is a question about the ledger, and this one
//! answers "what is the tax on it", which is a question about the law. The split
//! also means everything here is a pure function over its inputs, with no
//! database, no FX and no HTTP, so it can be tested against HMRC's worked
//! examples directly.
//!
//! ## The two rules that matter
//!
//! **Gains are bucketed by disposal date against the rate bands in force.** A
//! tax year usually has one band per rate kind. 2024-25 has two, because the
//! Autumn Budget 2024 raised CGT on shares from 10%/20% to 18%/24% for disposals
//! on or after 30 October 2024. That is not special-cased here: the bands come
//! from `tax_config` as ordinary rows, and a disposal lands in whichever one
//! contains its date. The next Budget needs a row, not a code change.
//!
//! **Deductions come off the highest-rate band first.** Losses (brought forward
//! and current-year) and the Annual Exempt Amount are all set against the gains
//! that would otherwise be taxed most heavily. This is the taxpayer-favourable
//! ordering, it is what HMRC's own computation does, and it materially changes
//! the answer: with gains split across a 20% band and a 24% band, taking the
//! deductions off the 24% band first is worth 4% of the deducted amount.

use std::collections::HashMap;

use chrono::NaiveDate;
use rust_decimal::Decimal;

use crate::model::{TaxBandResult, TaxComputation, TaxConfigEntry, TaxInputs};

/// A rate band resolved from `tax_config`, ready to have gains assigned to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateBand {
    pub valid_from: NaiveDate,
    pub valid_to: NaiveDate,
    pub rate_kind: String,
    pub rate: Decimal,
}

/// A single disposal reduced to the only two things the tax computation needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisposalForTax {
    pub disposal_date: NaiveDate,
    /// Positive for a gain, negative for a loss, in the base currency.
    pub gain_loss: Decimal,
}

/// Why a computation could not be produced.
///
/// Every variant is a situation where continuing would mean inventing a number.
/// A tax figure that is quietly wrong is worse than no figure, because the user
/// cannot tell it apart from a right one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaxComputationError {
    /// No rate bands are configured for the tax year.
    NoRateBands { tax_year: String },
    /// A disposal fell outside every configured band, so no rate applies to it.
    /// Means the bands fail to tile the year.
    UncoveredDisposal { date: String, tax_year: String },
}

impl std::fmt::Display for TaxComputationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoRateBands { tax_year } => write!(
                f,
                "no capital gains rate bands are configured for tax year {tax_year}"
            ),
            Self::UncoveredDisposal { date, tax_year } => write!(
                f,
                "disposal on {date} falls outside every configured rate band for tax year \
                 {tax_year}; the bands must cover the whole year without gaps"
            ),
        }
    }
}

impl std::error::Error for TaxComputationError {}

/// Pull the rate bands for one `rate_kind` out of a `tax_config` entry set,
/// earliest first.
///
/// Entries with an unparseable date or a missing rate are skipped rather than
/// failing: `tax_config` holds both `aea` and `rate` kinds in one table, so a
/// non-rate row appearing here is expected, not corrupt.
pub fn rate_bands_for(entries: &[TaxConfigEntry], rate_kind: &str) -> Vec<RateBand> {
    let mut bands: Vec<RateBand> = entries
        .iter()
        .filter(|e| e.kind == "rate" && e.rate_kind == rate_kind)
        .filter_map(|e| {
            Some(RateBand {
                valid_from: NaiveDate::parse_from_str(&e.valid_from, "%Y-%m-%d").ok()?,
                valid_to: NaiveDate::parse_from_str(&e.valid_to, "%Y-%m-%d").ok()?,
                rate_kind: e.rate_kind.clone(),
                rate: e.rate?,
            })
        })
        .collect();
    bands.sort_by_key(|b| b.valid_from);
    bands
}

/// The Annual Exempt Amount for a tax year, if one is configured.
pub fn aea_for(entries: &[TaxConfigEntry]) -> Option<Decimal> {
    entries
        .iter()
        .find(|e| e.kind == "aea")
        .and_then(|e| e.amount)
}

/// Compute the Capital Gains Tax due for one tax year.
///
/// `disposals` are the realized events for the year, `entries` the statutory
/// configuration, `inputs` the taxpayer's own figures.
///
/// The band a gain is charged at depends on how much of the taxpayer's
/// basic-rate income band is unused (`inputs.allowable_income_remaining`). That
/// much gain is charged at the basic rate and the rest at the higher rate. The
/// default of 0 means everything is charged at the higher rate, which is the
/// safe assumption: this app cannot see PAYE income, and over-estimating the tax
/// is the failure direction that does not produce a surprise bill.
pub fn compute_tax(
    tax_year: &str,
    disposals: &[DisposalForTax],
    entries: &[TaxConfigEntry],
    inputs: &TaxInputs,
) -> Result<TaxComputation, TaxComputationError> {
    let higher_bands = rate_bands_for(entries, "higher");
    let basic_bands = rate_bands_for(entries, "basic");

    if higher_bands.is_empty() {
        return Err(TaxComputationError::NoRateBands {
            tax_year: tax_year.to_string(),
        });
    }

    // 1. Bucket disposals by period. Gains and losses are tracked separately per
    //    period: current-year losses are a deduction against total gains, not a
    //    negative gain in their own band, so netting them here would silently
    //    apply them to whichever period they happened to fall in rather than to
    //    the highest-rate band as the law requires.
    let mut gains_by_period: HashMap<NaiveDate, Decimal> = HashMap::new();
    let mut current_year_losses = Decimal::ZERO;

    for d in disposals {
        let band = higher_bands
            .iter()
            .find(|b| d.disposal_date >= b.valid_from && d.disposal_date <= b.valid_to)
            .ok_or_else(|| TaxComputationError::UncoveredDisposal {
                date: d.disposal_date.format("%Y-%m-%d").to_string(),
                tax_year: tax_year.to_string(),
            })?;

        if d.gain_loss > Decimal::ZERO {
            *gains_by_period.entry(band.valid_from).or_default() += d.gain_loss;
        } else {
            current_year_losses += d.gain_loss.abs();
        }
    }

    let total_gains: Decimal = gains_by_period.values().copied().sum();

    // 2. Split each period's gains between the basic and higher rate, using the
    //    taxpayer's remaining basic-rate income headroom.
    //
    //    The headroom is consumed EARLIEST period first. It is a single
    //    allowance shared across the whole year, so it has to be spent in some
    //    order, and chronological is the only order that is stable and
    //    explicable to a user reading the working. It is not the deduction
    //    ordering (that is highest-rate-first below) because this is not a
    //    deduction: it moves gain between bands rather than removing it.
    let mut periods: Vec<NaiveDate> = gains_by_period.keys().copied().collect();
    periods.sort_unstable();

    let mut headroom = inputs.allowable_income_remaining.max(Decimal::ZERO);
    // (band, gains) in the order they will be built into results.
    let mut band_gains: Vec<(RateBand, Decimal)> = Vec::new();

    for period_start in periods {
        let gains = gains_by_period[&period_start];
        let higher = higher_bands
            .iter()
            .find(|b| b.valid_from == period_start)
            .expect("period key came from a higher band");

        let at_basic = headroom.min(gains);
        headroom -= at_basic;
        let at_higher = gains - at_basic;

        if at_basic > Decimal::ZERO {
            // Fall back to the higher band's own dates if no basic band is
            // configured for this period, so a partial config still produces a
            // labelled row rather than dropping the gain.
            if let Some(basic) = basic_bands.iter().find(|b| b.valid_from == period_start) {
                band_gains.push((basic.clone(), at_basic));
            } else {
                band_gains.push((higher.clone(), at_basic));
            }
        }
        if at_higher > Decimal::ZERO {
            band_gains.push((higher.clone(), at_higher));
        }
    }

    // 3. Deduct, highest rate first.
    //
    //    Order: current-year losses, then brought-forward losses, then the AEA.
    //    Current-year losses are compulsory — they must be set against the
    //    year's gains whether or not that wastes the allowance — while
    //    brought-forward losses are only used down to the AEA, so using them in
    //    this order is what preserves the unused remainder for future years.
    let mut remaining_by_band: Vec<Decimal> = band_gains.iter().map(|(_, g)| *g).collect();

    // Highest rate first; ties broken by earliest period so the order is stable.
    let mut order: Vec<usize> = (0..band_gains.len()).collect();
    order.sort_by(|&a, &b| {
        band_gains[b]
            .0
            .rate
            .cmp(&band_gains[a].0.rate)
            .then(band_gains[a].0.valid_from.cmp(&band_gains[b].0.valid_from))
    });

    let mut deductions_by_band: Vec<Decimal> = vec![Decimal::ZERO; band_gains.len()];

    let apply = |pot: Decimal,
                 remaining_by_band: &mut Vec<Decimal>,
                 deductions_by_band: &mut Vec<Decimal>|
     -> Decimal {
        let mut left = pot;
        for &i in &order {
            if left <= Decimal::ZERO {
                break;
            }
            let take = left.min(remaining_by_band[i]);
            remaining_by_band[i] -= take;
            deductions_by_band[i] += take;
            left -= take;
        }
        pot - left
    };

    let current_year_losses_applied = apply(
        current_year_losses,
        &mut remaining_by_band,
        &mut deductions_by_band,
    );

    let gains_after_current_losses: Decimal = remaining_by_band.iter().copied().sum();

    // The AEA is applied after losses in the arithmetic, but brought-forward
    // losses are restricted to the excess over the allowance: you never use a
    // carried-forward loss to reduce a gain the allowance would have covered
    // for free. So compute how much of the allowance is available first, and
    // only spend brought-forward losses above it.
    let aea_available = if inputs.aea_claimed {
        aea_for(entries).unwrap_or(Decimal::ZERO).max(Decimal::ZERO)
    } else {
        Decimal::ZERO
    };

    let brought_forward_available = inputs.brought_forward_losses.max(Decimal::ZERO);
    let brought_forward_needed = (gains_after_current_losses - aea_available).max(Decimal::ZERO);
    let brought_forward_to_apply = brought_forward_available.min(brought_forward_needed);

    let brought_forward_losses_applied = apply(
        brought_forward_to_apply,
        &mut remaining_by_band,
        &mut deductions_by_band,
    );

    let aea_applied = apply(
        aea_available,
        &mut remaining_by_band,
        &mut deductions_by_band,
    );

    let bands: Vec<TaxBandResult> = band_gains
        .iter()
        .enumerate()
        .map(|(i, (band, gains))| {
            let taxable = remaining_by_band[i];
            TaxBandResult {
                valid_from: band.valid_from.format("%Y-%m-%d").to_string(),
                valid_to: band.valid_to.format("%Y-%m-%d").to_string(),
                rate_kind: band.rate_kind.clone(),
                rate: band.rate,
                gains: *gains,
                deductions: deductions_by_band[i],
                taxable,
                tax: (taxable * band.rate).round_dp(2),
            }
        })
        .collect();

    let taxable_gain: Decimal = bands.iter().map(|b| b.taxable).sum();
    let tax_due: Decimal = bands.iter().map(|b| b.tax).sum();

    Ok(TaxComputation {
        tax_year: tax_year.to_string(),
        bands,
        total_gains,
        current_year_losses_applied,
        brought_forward_losses_applied,
        brought_forward_losses_remaining: brought_forward_available
            - brought_forward_losses_applied,
        aea_applied,
        taxable_gain,
        tax_due,
        inputs: inputs.clone(),
    })
}
