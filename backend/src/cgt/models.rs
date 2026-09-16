//! Response models returned by the CGT endpoints -- the public API surface.
//!
//! Every type here derives `TS` and exports to `frontend/src/bindings/`; the
//! export path is crate-relative and the filename derives from the struct name,
//! so this module's location does not affect the generated output.
//!
//! The engine's internal calculation types live in [`super::internal`] and are
//! deliberately not part of this contract.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::TaxComputation;

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
    /// The tax computation, present only when the request named a `tax_year`.
    /// Absent means "not asked for", never "no tax due" — a nil tax bill is a
    /// present computation whose `tax_due` is zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tax: Option<TaxComputation>,
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
    /// Source metadata only — the currency the constituent trades were denominated
    /// in. **Not a formatting label**: `proceeds`, `cost_basis` and `gain_loss` above
    /// are all in the preferred base currency (GBP). See
    /// [`CgtRealizedEvent::original_currency`].
    ///
    /// Every event in a group provably shares one currency:
    /// `check_single_currency_per_symbol` rejects a symbol carrying more than one
    /// before the engine runs, and a group never spans symbols.
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
    /// Source metadata only — **not a formatting label**. Every total on this struct
    /// is in the preferred base currency (GBP). See
    /// [`CgtRealizedEvent::original_currency`].
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
    /// The traded price per share, in [`Self::original_currency`]. This is the one
    /// money field on this struct that is genuinely native — it is `price_per_share`
    /// straight off the event, never converted.
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub disposal_price: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub proceeds: Decimal, // in base currency (GBP)
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub cost_basis: Decimal, // in base currency (GBP)
    /// `proceeds - cost_basis`, and therefore in base currency (GBP) like both.
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub gain_loss: Decimal,
    pub rule_applied: String, // "Same-Day" | "30-Day Rule" | "S104 Pool" | "Unmatched"
    /// Source metadata only: the currency the underlying trade was denominated in.
    ///
    /// **It is NOT a formatting label for the money on this struct.** `proceeds`,
    /// `cost_basis` and `gain_loss` are all converted to the preferred base currency
    /// (GBP) before serialisation, so formatting them with this field renders a GBP
    /// amount behind a "$". That exact bug has now been found three separate times —
    /// here, on [`S104PoolState`], and latently on [`CgtDisposalGroup`] — because the
    /// name reads like a display currency. Only [`Self::disposal_price`] is native.
    ///
    /// Use it the way `cgt_per_symbol_table.tsx` does: as a non-formatting badge shown
    /// when it differs from base.
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
