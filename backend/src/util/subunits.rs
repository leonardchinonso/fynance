//! Non-ISO sub-unit currency codes, and their parent currencies.
//!
//! Brokers quote some instruments in a currency's minor unit rather than the currency itself —
//! most commonly LSE equities priced in pence rather than pounds. The codes they use for this
//! (`GBX`, `USX`, `ZAC`, `ILA`) are **market conventions, not ISO 4217**: ISO defines a minor-unit
//! *exponent* for each currency but never assigns the minor unit its own code. There is therefore
//! no authoritative list to fetch and no standard to track — which is precisely why this table is
//! hardcoded. It is short and effectively closed; other portfolio tools hardcode the same set.
//!
//! # Why this exists rather than an FX rate
//!
//! Sub-units were originally handled by adding `GBX` to the `currencies` table with a rate of
//! 0.01. That only works while the preferred currency is GBP: switch it to USD and `GBX` needs
//! 0.0074, maintained by hand alongside `GBP`'s own rate, and the two silently drift apart. The
//! relationship between a sub-unit and its parent is a fixed property of the denomination, not an
//! exchange rate that moves — so it belongs here, applied once at import, rather than in a rate
//! table.
//!
//! # Lifecycle
//!
//! Plan 23 §0.2 (7.1) converts sub-units to their parent at import time, after which no sub-unit
//! code is ever written to storage and these codes are dropped from the accepted-currency list.
//! Until that lands, the codes must stay accepted at write time or statements denominated in them
//! cannot be imported at all. This module is the single source of truth for both phases and can be
//! deleted wholesale once conversion-at-import is complete and existing rows are migrated.

use rust_decimal::Decimal;

/// A broker sub-unit code, the currency it is a fraction of, and how many of it make one unit.
pub struct SubUnit {
    /// The non-ISO code as brokers write it, e.g. `GBX`.
    pub code: &'static str,
    /// The ISO 4217 code of the parent currency, e.g. `GBP`.
    pub parent: &'static str,
    /// How many sub-units make one parent unit. 100 for every currently known case; kept explicit
    /// rather than assumed so a 1000-to-one minor unit would not need a code change.
    pub per_unit: u32,
}

/// Every sub-unit code known to be quoted by brokers.
///
/// - `GBX` — British pence. London Stock Exchange; by far the most common.
/// - `USX` — US cents. Occasionally used for low-priced US instruments.
/// - `ZAC` — South African cents. Johannesburg Stock Exchange.
/// - `ILA` — Israeli agorot. Tel Aviv Stock Exchange.
pub const SUB_UNITS: &[SubUnit] = &[
    SubUnit {
        code: "GBX",
        parent: "GBP",
        per_unit: 100,
    },
    SubUnit {
        code: "USX",
        parent: "USD",
        per_unit: 100,
    },
    SubUnit {
        code: "ZAC",
        parent: "ZAR",
        per_unit: 100,
    },
    SubUnit {
        code: "ILA",
        parent: "ILS",
        per_unit: 100,
    },
];

/// Look up a sub-unit by its code. Returns `None` for ordinary ISO currencies.
pub fn lookup(code: &str) -> Option<&'static SubUnit> {
    SUB_UNITS.iter().find(|s| s.code == code)
}

/// True if `code` is a broker sub-unit rather than a currency in its own right.
pub fn is_sub_unit(code: &str) -> bool {
    lookup(code).is_some()
}

/// Convert an amount denominated in a sub-unit into its parent currency.
///
/// Returns `None` when `code` is not a sub-unit, so callers can pass any currency through and act
/// only on a `Some`. Division is exact on `Decimal`, so no precision is lost: 1234 GBX becomes
/// exactly 12.34 GBP.
pub fn to_parent(amount: Decimal, code: &str) -> Option<(Decimal, &'static str)> {
    let unit = lookup(code)?;
    Some((amount / Decimal::from(unit.per_unit), unit.parent))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn known_sub_units_resolve_to_their_parent() {
        assert_eq!(lookup("GBX").unwrap().parent, "GBP");
        assert_eq!(lookup("USX").unwrap().parent, "USD");
        assert_eq!(lookup("ZAC").unwrap().parent, "ZAR");
        assert_eq!(lookup("ILA").unwrap().parent, "ILS");
    }

    #[test]
    fn ordinary_currencies_are_not_sub_units() {
        assert!(!is_sub_unit("GBP"));
        assert!(!is_sub_unit("USD"));
        assert!(lookup("EUR").is_none());
    }

    #[test]
    fn conversion_to_parent_is_exact() {
        let (amount, parent) = to_parent(Decimal::from(1234), "GBX").unwrap();
        assert_eq!(amount, Decimal::from_str_exact("12.34").unwrap());
        assert_eq!(parent, "GBP");
    }

    #[test]
    fn conversion_returns_none_for_non_sub_unit() {
        assert!(to_parent(Decimal::from(100), "GBP").is_none());
    }

    #[test]
    fn every_parent_is_a_distinct_real_currency() {
        // A sub-unit whose parent is itself, or a duplicated code, would loop or shadow.
        for s in SUB_UNITS {
            assert_ne!(s.code, s.parent, "{} is its own parent", s.code);
            assert!(!is_sub_unit(s.parent), "{} is itself a sub-unit", s.parent);
            assert!(s.per_unit > 1, "{} has a meaningless ratio", s.code);
        }
        let codes: Vec<_> = SUB_UNITS.iter().map(|s| s.code).collect();
        let mut deduped = codes.clone();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(codes.len(), deduped.len(), "duplicate sub-unit code");
    }
}
