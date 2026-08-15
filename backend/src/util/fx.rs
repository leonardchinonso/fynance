//! Currency conversion helpers.

use crate::model::{Currency, DisplayCurrency};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet};

/// A `(currency, date)` pair the caller asked to convert but for which no rate is stored.
///
/// Carried out of `convert_as_of` so the caller can report precisely which rate is missing.
/// The CGT precheck collects these across the whole report and returns them in one response,
/// because discovering ~49 missing rates one request at a time would be unusable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MissingRate {
    pub currency: String,
    pub date: NaiveDate,
}

impl std::fmt::Display for MissingRate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} on {}", self.currency, self.date)
    }
}

/// In-memory map of FX rates, loaded once per aggregation request.
///
/// Holds two distinct things that must not be confused:
///   * `rates` — the single flat present-day rate per currency, from the `currencies` table.
///     Correct for "what is my portfolio worth today" aggregates, wrong for tax.
///   * `historical` — date-keyed user-owned rates from the `exchange_rates` table, keyed
///     `(currency, date)`. The only thing CGT may use.
pub struct FxRateMap {
    preferred: String,
    rates: HashMap<String, Decimal>,
    historical: HashMap<(String, NaiveDate), Decimal>,
}

impl FxRateMap {
    /// Build from the full list of currencies fetched via `Db::get_currencies()`.
    ///
    /// The historical map starts empty, so `convert_as_of` reports every non-preferred
    /// pair as missing. Callers that need date-specific conversion (the CGT engine) must
    /// use [`FxRateMap::with_historical`]; every other caller uses only `convert` and is
    /// unaffected.
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

        Ok(FxRateMap {
            preferred,
            rates,
            historical: HashMap::new(),
        })
    }

    /// Attach date-keyed rates, as loaded from the `exchange_rates` table.
    ///
    /// `rows` are `(base_currency, date, rate)` where `rate` is quote-units-per-base-unit
    /// and the quote is assumed to be the preferred currency — the loader filters on that,
    /// so a row quoting into some other currency can never leak in here and be applied in
    /// the wrong direction.
    pub fn with_historical(mut self, rows: Vec<(String, NaiveDate, Decimal)>) -> Self {
        self.historical = rows
            .into_iter()
            .map(|(currency, date, rate)| ((currency, date), rate))
            .collect();
        self
    }

    /// Convert an amount from `source_currency` to the preferred currency.
    /// Every write path (transactions, holdings, accounts, investment events)
    /// validates currencies at write time, so a currency missing from the FX
    /// table can only come from a row written before that validation existed.
    /// Log a warning and return the amount unchanged: poisoning the DB mutex
    /// with a panic here brings down the whole server for a single bad row,
    /// and that trade-off is not worth it.
    pub fn convert(&self, amount: Decimal, source_currency: &str) -> Decimal {
        if source_currency == self.preferred {
            return amount;
        }
        let Some(rate) = self.rates.get(source_currency) else {
            tracing::warn!(
                source_currency,
                "currency missing from FxRateMap; returning amount unchanged. Add it under Settings → Currencies to fix aggregates."
            );
            return amount;
        };
        amount * rate
    }

    /// Convert an amount from `source_currency` to the preferred currency using the rate
    /// stored for that specific `date`.
    ///
    /// Returns `Err(MissingRate)` when no rate is stored for that `(currency, date)` pair
    /// rather than falling back to the flat `currencies.fx_rate`. That fallback is exactly
    /// the bug this exists to fix: applying one present-day rate to every event regardless
    /// of date produced a ~6% error against the owner's filed return. A silent fallback
    /// would reintroduce it while looking correct, so the absence has to be representable
    /// in the type — hence `Result` rather than the bare `Decimal` this used to return.
    ///
    /// Callers are expected to have run the precheck (see `check_required_exchange_rates`)
    /// so that every pair the report needs is present before the engine starts; this error
    /// is the backstop that makes forgetting to do so impossible to miss.
    pub fn convert_as_of(
        &self,
        amount: Decimal,
        source_currency: &str,
        date: NaiveDate,
    ) -> Result<Decimal, MissingRate> {
        // GBP -> GBP is a no-op and must never require a stored rate.
        if source_currency == self.preferred {
            return Ok(amount);
        }
        match self.historical.get(&(source_currency.to_string(), date)) {
            Some(rate) => Ok(amount * rate),
            None => Err(MissingRate {
                currency: source_currency.to_string(),
                date,
            }),
        }
    }

    /// True when a rate is stored for this `(currency, date)` pair, or when the currency is
    /// the preferred one (which never needs a rate). Used by the precheck to enumerate what
    /// is missing without performing a conversion.
    pub fn has_rate_as_of(&self, source_currency: &str, date: NaiveDate) -> bool {
        source_currency == self.preferred
            || self
                .historical
                .contains_key(&(source_currency.to_string(), date))
    }

    pub fn preferred(&self) -> &str {
        &self.preferred
    }

    /// Look up the stored conversion rate for a currency, if any. Used by callers
    /// that need to validate their inputs before invoking `convert`.
    pub fn rate(&self, source_currency: &str) -> Option<&Decimal> {
        self.rates.get(source_currency)
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
