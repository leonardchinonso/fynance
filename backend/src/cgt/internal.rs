//! The engine's internal calculation types.
//!
//! These are implementation detail of the matching rules, not part of the API
//! surface: nothing here derives `TS` and nothing here is visible outside the
//! crate. `CalEvent` is the mutable working copy of an [`InvestmentEvent`] that
//! the engine consumes quantity from as it matches, and `InternalMatch` is a
//! matched bucket before it is converted to the public [`CgtMatchDetail`].
//!
//! Split out of [`super::models`] so that module holds only the public response
//! types a reader needs to understand the API contract.

use chrono::NaiveDateTime;
use rust_decimal::Decimal;

use crate::model::{InvestmentEvent, InvestmentEventType};
use crate::util::fx::{FxRateMap, MissingRate};

use super::models::CgtMatchDetail;

#[derive(Debug, Clone)]
pub(crate) struct InternalMatch {
    pub acquisition_id: Option<String>,
    pub acquisition_date: Option<NaiveDateTime>,
    pub quantity: Decimal,
    pub price: Decimal,
    pub is_s104: bool,
}

impl InternalMatch {
    pub(crate) fn to_cgt_match_detail(&self) -> CgtMatchDetail {
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
pub(crate) struct CalEvent {
    pub(crate) id: String,
    pub(crate) event_type: InvestmentEventType,
    pub(crate) date: NaiveDateTime,
    pub(crate) quantity: Decimal,
    pub(crate) price_per_share: Decimal,
    pub(crate) fee: Decimal,
    pub(crate) currency: String,
    /// Currency the fee is denominated in; may differ from `currency`.
    pub(crate) fee_currency: String,

    // Tracking for matching algorithm
    pub(crate) remaining_qty: Decimal,
    pub(crate) same_day_matches: Vec<InternalMatch>,
    pub(crate) thirty_day_matches: Vec<InternalMatch>,
    pub(crate) pool_matches: Vec<InternalMatch>,
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
    /// Calculates the normalized proceeds and proportional fee in preferred base currency (GBP)
    /// for a matched quantity, each converted at this event's own date.
    ///
    /// Fallible only because a rate could be absent, which the precheck has already ruled out
    /// by the time the engine reaches here -- see `unreachable_missing_rate`.
    pub(crate) fn calculate_matched_finance(
        &self,
        match_qty: Decimal,
        fx: &FxRateMap,
    ) -> Result<(Decimal, Decimal), MissingRate> {
        let proceeds_raw = match_qty * self.price_per_share;
        let fee_raw = if self.quantity > Decimal::ZERO {
            self.fee * (match_qty / self.quantity)
        } else {
            Decimal::ZERO
        };

        let proceeds = fx.convert_as_of(proceeds_raw, &self.currency, self.date.date())?;
        let fee = fx.convert_as_of(fee_raw, &self.fee_currency, self.date.date())?;
        Ok((proceeds, fee))
    }
}
