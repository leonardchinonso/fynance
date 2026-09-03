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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TaxConfigEntry;
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).expect("decimal literal")
    }

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("date literal")
    }

    fn aea_entry(tax_year: &str, from: &str, to: &str, amount: &str) -> TaxConfigEntry {
        TaxConfigEntry {
            tax_year: tax_year.to_string(),
            kind: "aea".to_string(),
            rate_kind: String::new(),
            valid_from: from.to_string(),
            valid_to: to.to_string(),
            amount: Some(d(amount)),
            rate: None,
            updated_at: None,
        }
    }

    fn rate_entry(
        tax_year: &str,
        rate_kind: &str,
        from: &str,
        to: &str,
        rate: &str,
    ) -> TaxConfigEntry {
        TaxConfigEntry {
            tax_year: tax_year.to_string(),
            kind: "rate".to_string(),
            rate_kind: rate_kind.to_string(),
            valid_from: from.to_string(),
            valid_to: to.to_string(),
            amount: None,
            rate: Some(d(rate)),
            updated_at: None,
        }
    }

    /// 2024-25 as it actually is: the Autumn Budget 2024 split, plus an AEA.
    /// The statutory figures are real; every disposal in these tests is invented.
    fn config_2024_25() -> Vec<TaxConfigEntry> {
        vec![
            aea_entry("2024-25", "2024-04-06", "2025-04-05", "3000"),
            rate_entry("2024-25", "basic", "2024-04-06", "2024-10-29", "0.10"),
            rate_entry("2024-25", "higher", "2024-04-06", "2024-10-29", "0.20"),
            rate_entry("2024-25", "basic", "2024-10-30", "2025-04-05", "0.18"),
            rate_entry("2024-25", "higher", "2024-10-30", "2025-04-05", "0.24"),
        ]
    }

    fn inputs(brought_forward: &str, headroom: &str, aea_claimed: bool) -> TaxInputs {
        TaxInputs {
            profile_id: "test".to_string(),
            tax_year: "2024-25".to_string(),
            brought_forward_losses: d(brought_forward),
            allowable_income_remaining: d(headroom),
            aea_claimed,
            updated_at: None,
        }
    }

    fn disposal(on: &str, gain_loss: &str) -> DisposalForTax {
        DisposalForTax {
            disposal_date: date(on),
            gain_loss: d(gain_loss),
        }
    }

    /// The rate split is the point of the 2024-25 modelling: two identical
    /// disposals either side of 30 October 2024 are charged at 20% and 24%.
    #[test]
    fn splits_gains_across_the_30_october_2024_rate_change() {
        let disposals = vec![
            disposal("2024-06-01", "10000"),
            disposal("2024-12-01", "10000"),
        ];
        // No AEA and no losses, so each band's tax is purely its own rate.
        let result = compute_tax(
            "2024-25",
            &disposals,
            &config_2024_25(),
            &inputs("0", "0", false),
        )
        .expect("computes");

        assert_eq!(result.bands.len(), 2, "one band per side of the split");

        let pre = &result.bands[0];
        assert_eq!(pre.valid_from, "2024-04-06");
        assert_eq!(pre.rate, d("0.20"));
        assert_eq!(pre.gains, d("10000"));
        assert_eq!(pre.tax, d("2000.00"));

        let post = &result.bands[1];
        assert_eq!(post.valid_from, "2024-10-30");
        assert_eq!(post.rate, d("0.24"));
        assert_eq!(post.gains, d("10000"));
        assert_eq!(post.tax, d("2400.00"));

        assert_eq!(result.tax_due, d("4400.00"));
    }

    /// The boundary is inclusive of the new rate: a disposal made *on* 30
    /// October is charged at 24%. An off-by-one here is a real filing error.
    #[test]
    fn the_30_october_boundary_is_inclusive_of_the_new_rate() {
        let on_the_day = compute_tax(
            "2024-25",
            &[disposal("2024-10-30", "1000")],
            &config_2024_25(),
            &inputs("0", "0", false),
        )
        .expect("computes");
        assert_eq!(on_the_day.bands[0].rate, d("0.24"));
        assert_eq!(on_the_day.tax_due, d("240.00"));

        let day_before = compute_tax(
            "2024-25",
            &[disposal("2024-10-29", "1000")],
            &config_2024_25(),
            &inputs("0", "0", false),
        )
        .expect("computes");
        assert_eq!(day_before.bands[0].rate, d("0.20"));
        assert_eq!(day_before.tax_due, d("200.00"));
    }

    /// The ordering rule: losses and the AEA come off the HIGHEST-rate band
    /// first, leaving the lower-rate band untouched while the higher can absorb.
    #[test]
    fn deducts_losses_and_aea_from_the_highest_rate_band_first() {
        let disposals = vec![
            disposal("2024-06-01", "20000"), // 20% band
            disposal("2024-12-01", "12000"), // 24% band
        ];
        // 1,000 brought-forward losses + the 3,000 AEA = 4,000 of deductions,
        // all of which must land on the 24% band.
        let result = compute_tax(
            "2024-25",
            &disposals,
            &config_2024_25(),
            &inputs("1000", "0", true),
        )
        .expect("computes");

        let pre = &result.bands[0];
        assert_eq!(pre.rate, d("0.20"));
        assert_eq!(
            pre.deductions,
            Decimal::ZERO,
            "the lower-rate band must be untouched while the higher one can absorb"
        );
        assert_eq!(pre.taxable, d("20000"));
        assert_eq!(pre.tax, d("4000.00"));

        let post = &result.bands[1];
        assert_eq!(post.rate, d("0.24"));
        assert_eq!(post.deductions, d("4000"), "1000 losses + 3000 AEA");
        assert_eq!(post.taxable, d("8000"));
        assert_eq!(post.tax, d("1920.00"));

        assert_eq!(result.brought_forward_losses_applied, d("1000"));
        assert_eq!(result.aea_applied, d("3000"));
        assert_eq!(result.tax_due, d("5920.00"));
    }

    /// Current-year losses net against gains before anything else, and they too
    /// come off the highest-rate band.
    #[test]
    fn current_year_losses_reduce_the_highest_band() {
        let disposals = vec![
            disposal("2024-06-01", "10000"), // 20% band gain
            disposal("2024-12-01", "10000"), // 24% band gain
            disposal("2024-12-15", "-4000"), // a loss in the year
        ];
        let result = compute_tax(
            "2024-25",
            &disposals,
            &config_2024_25(),
            &inputs("0", "0", false),
        )
        .expect("computes");

        assert_eq!(result.current_year_losses_applied, d("4000"));
        assert_eq!(result.bands[0].taxable, d("10000"), "20% band untouched");
        assert_eq!(
            result.bands[1].taxable,
            d("6000"),
            "24% band absorbs the loss"
        );
        assert_eq!(result.tax_due, d("3440.00")); // 2000 + 1440
    }

    /// Brought-forward losses are spent only down to the AEA: the allowance
    /// covers the rest for free and the unused losses stay carried forward.
    #[test]
    fn brought_forward_losses_are_preserved_behind_the_aea() {
        // One 5,000 gain with 10,000 of losses available. Only 2,000 should be
        // used (5,000 - 3,000 AEA), leaving 8,000 to carry.
        let result = compute_tax(
            "2024-25",
            &[disposal("2024-12-01", "5000")],
            &config_2024_25(),
            &inputs("10000", "0", true),
        )
        .expect("computes");

        assert_eq!(result.brought_forward_losses_applied, d("2000"));
        assert_eq!(result.brought_forward_losses_remaining, d("8000"));
        assert_eq!(result.aea_applied, d("3000"));
        assert_eq!(result.taxable_gain, Decimal::ZERO);
        assert_eq!(result.tax_due, Decimal::ZERO);
    }

    /// `allowable_income_remaining` moves gain into the basic band rather than
    /// removing it.
    #[test]
    fn income_headroom_moves_gain_into_the_basic_band() {
        // 10,000 of post-30-Oct gain with 4,000 of unused basic-rate income
        // band: 4,000 at 18% and 6,000 at 24%.
        let result = compute_tax(
            "2024-25",
            &[disposal("2024-12-01", "10000")],
            &config_2024_25(),
            &inputs("0", "4000", false),
        )
        .expect("computes");

        assert_eq!(result.bands.len(), 2);

        let basic = result
            .bands
            .iter()
            .find(|b| b.rate_kind == "basic")
            .expect("a basic band");
        assert_eq!(basic.rate, d("0.18"));
        assert_eq!(basic.gains, d("4000"));

        let higher = result
            .bands
            .iter()
            .find(|b| b.rate_kind == "higher")
            .expect("a higher band");
        assert_eq!(higher.rate, d("0.24"));
        assert_eq!(higher.gains, d("6000"));

        // 4000*0.18 = 720, 6000*0.24 = 1440.
        assert_eq!(result.tax_due, d("2160.00"));
    }

    /// With no headroom (the default), everything is charged at the higher rate.
    #[test]
    fn no_headroom_charges_everything_at_the_higher_rate() {
        let result = compute_tax(
            "2024-25",
            &[disposal("2024-12-01", "10000")],
            &config_2024_25(),
            &inputs("0", "0", false),
        )
        .expect("computes");

        assert_eq!(result.bands.len(), 1);
        assert_eq!(result.bands[0].rate_kind, "higher");
        assert_eq!(result.bands[0].rate, d("0.24"));
        assert_eq!(result.tax_due, d("2400.00"));
    }

    /// Declining the AEA must actually change the answer.
    #[test]
    fn declining_the_aea_leaves_the_gain_chargeable() {
        let claimed = compute_tax(
            "2024-25",
            &[disposal("2024-12-01", "10000")],
            &config_2024_25(),
            &inputs("0", "0", true),
        )
        .expect("computes");
        assert_eq!(claimed.aea_applied, d("3000"));
        assert_eq!(claimed.taxable_gain, d("7000"));

        let declined = compute_tax(
            "2024-25",
            &[disposal("2024-12-01", "10000")],
            &config_2024_25(),
            &inputs("0", "0", false),
        )
        .expect("computes");
        assert_eq!(declined.aea_applied, Decimal::ZERO);
        assert_eq!(declined.taxable_gain, d("10000"));
    }

    /// The AEA is capped at the gains available: a small gain must not produce a
    /// negative taxable figure.
    #[test]
    fn aea_is_capped_at_the_gains_available() {
        let result = compute_tax(
            "2024-25",
            &[disposal("2024-12-01", "500")],
            &config_2024_25(),
            &inputs("0", "0", true),
        )
        .expect("computes");

        assert_eq!(result.aea_applied, d("500"), "capped, not the full 3000");
        assert_eq!(result.taxable_gain, Decimal::ZERO);
        assert_eq!(result.tax_due, Decimal::ZERO);
    }

    /// A disposal outside every configured band is refused rather than charged
    /// at no rate. A gap in the config must never silently untax a disposal.
    #[test]
    fn refuses_a_disposal_outside_every_band() {
        let err = compute_tax(
            "2024-25",
            &[disposal("2025-06-01", "10000")], // the next tax year
            &config_2024_25(),
            &inputs("0", "0", false),
        )
        .expect_err("must refuse");

        assert_eq!(
            err,
            TaxComputationError::UncoveredDisposal {
                date: "2025-06-01".to_string(),
                tax_year: "2024-25".to_string(),
            }
        );
    }

    /// A year with no rate bands is refused rather than returning a confident
    /// zero.
    #[test]
    fn refuses_a_year_with_no_rate_bands() {
        let only_an_aea = vec![aea_entry("2029-30", "2029-04-06", "2030-04-05", "3000")];
        let err = compute_tax(
            "2029-30",
            &[disposal("2029-06-01", "10000")],
            &only_an_aea,
            &inputs("0", "0", true),
        )
        .expect_err("must refuse");

        assert_eq!(
            err,
            TaxComputationError::NoRateBands {
                tax_year: "2029-30".to_string(),
            }
        );
    }

    /// A year that nets to a loss owes nothing and reports no negative figures.
    #[test]
    fn a_losing_year_owes_nothing() {
        let disposals = vec![
            disposal("2024-06-01", "1000"),
            disposal("2024-12-01", "-5000"),
        ];
        let result = compute_tax(
            "2024-25",
            &disposals,
            &config_2024_25(),
            &inputs("0", "0", true),
        )
        .expect("computes");

        assert_eq!(result.tax_due, Decimal::ZERO);
        assert_eq!(result.taxable_gain, Decimal::ZERO);
        assert!(
            result.bands.iter().all(|b| b.taxable >= Decimal::ZERO),
            "no band may report a negative taxable amount"
        );
    }

    /// A year without a mid-year change produces one band.
    #[test]
    fn a_year_without_a_mid_year_change_has_one_band() {
        let config = vec![
            aea_entry("2025-26", "2025-04-06", "2026-04-05", "3000"),
            rate_entry("2025-26", "basic", "2025-04-06", "2026-04-05", "0.18"),
            rate_entry("2025-26", "higher", "2025-04-06", "2026-04-05", "0.24"),
        ];
        let mut ins = inputs("0", "0", true);
        ins.tax_year = "2025-26".to_string();

        let result = compute_tax("2025-26", &[disposal("2025-09-01", "13000")], &config, &ins)
            .expect("computes");

        assert_eq!(result.bands.len(), 1);
        assert_eq!(result.bands[0].rate, d("0.24"));
        assert_eq!(result.taxable_gain, d("10000")); // 13000 - 3000 AEA
        assert_eq!(result.tax_due, d("2400.00"));
    }
}
