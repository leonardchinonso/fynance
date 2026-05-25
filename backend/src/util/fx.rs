//! Currency conversion helpers.

use crate::model::{Currency, DisplayCurrency};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet};

/// In-memory map of FX rates, loaded once per aggregation request.
pub struct FxRateMap {
    preferred: String,
    rates: HashMap<String, Decimal>,
}

impl FxRateMap {
    /// Build from the full list of currencies fetched via `Db::get_currencies()`.
    pub fn new(currencies: Vec<Currency>) -> anyhow::Result<Self> {
        let preferred = currencies
            .iter()
            .find(|c| c.is_preferred)
            .ok_or_else(|| anyhow::anyhow!("no preferred currency configured"))?
            .code
            .clone();

        let rates = currencies
            .into_iter()
            .map(|c| (c.code, c.fx_rate))
            .collect();

        Ok(FxRateMap { preferred, rates })
    }

    /// Convert an amount from `source_currency` to the preferred currency.
    /// Panics if the currency is not in the map (should never happen due to
    /// write-time validation).
    pub fn convert(&self, amount: Decimal, source_currency: &str) -> Decimal {
        if source_currency == self.preferred {
            return amount;
        }
        let rate = self.rates.get(source_currency).unwrap_or_else(|| {
            panic!(
                "currency {source_currency} missing from FxRateMap; write-time validation failed"
            )
        });
        amount * rate
    }

    /// Convert an amount from `source_currency` to the preferred currency as of a specific date.
    /// Currently, it falls back to the current/static rate, ignoring the date.
    pub fn convert_as_of(
        &self,
        amount: Decimal,
        source_currency: &str,
        _date: NaiveDate,
    ) -> Decimal {
        self.convert(amount, source_currency)
    }

    pub fn preferred(&self) -> &str {
        &self.preferred
    }
}

/// Aggregation accumulator that tracks both the converted sum and whether all
/// contributing values share the same non-preferred currency.
#[derive(Default, Clone)]
pub struct CurrencyAggregator {
    converted_sum: Decimal,
    raw_sum: Decimal,
    seen_currencies: HashSet<String>,
}

impl CurrencyAggregator {
    pub fn add(&mut self, amount: Decimal, currency: &str, fx: &FxRateMap) {
        self.converted_sum += fx.convert(amount, currency);
        self.raw_sum += amount;
        self.seen_currencies.insert(currency.to_string());
    }

    pub fn converted_sum(&self) -> Decimal {
        self.converted_sum
    }

    /// Returns `Some(DisplayCurrency)` if every value added was in the same
    /// non-preferred currency. Returns `None` if mixed or all-preferred.
    pub fn display_currency(&self, preferred: &str) -> Option<DisplayCurrency> {
        if self.seen_currencies.len() == 1 {
            let currency = self.seen_currencies.iter().next().unwrap();
            if currency != preferred {
                return Some(DisplayCurrency {
                    value: self.raw_sum,
                    currency: currency.clone(),
                });
            }
        }
        None
    }
}
