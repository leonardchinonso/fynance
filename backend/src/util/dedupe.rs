//! Duplicate detection for imported transactions and investment events.
//!
//! A row's identity key is built from its calendar date (not the time of day,
//! which sources record inconsistently) and its canonical amounts, so `"500"`
//! and `"500.00"` or `T00:00:00` and `T14:32:11` key the same. Rows that share a
//! key within one import are numbered by occurrence, and the stored
//! fingerprint hashes the key with that number: re-importing a statement
//! reproduces every fingerprint and dedupes, while two genuine identical rows
//! in one statement (two £15.50 TfL fares) both land.

use std::collections::HashMap;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};

/// How many days apart two rows may be and still be flagged as a likely duplicate.
pub const NEAR_MATCH_DAYS: i64 = 3;

/// Identity key of a transaction, before its occurrence number is applied.
pub fn transaction_key(date: NaiveDate, amount: Decimal, account_id: &str) -> String {
    format!("tx2|{date}|{}|{account_id}", amount.normalize())
}

/// Identity key of an investment event, before its occurrence number is applied.
/// `event_type` must be the canonical `InvestmentEventType::as_str` form.
pub fn investment_key(
    account_id: &str,
    symbol: &str,
    date: NaiveDate,
    quantity: Decimal,
    price_per_share: Decimal,
    event_type: &str,
) -> String {
    format!(
        "inv2|{account_id}|{symbol}|{date}|{}|{}|{event_type}",
        quantity.normalize(),
        price_per_share.normalize()
    )
}

/// The stored fingerprint of the `occurrence`-th row (from 0) carrying `key`.
pub fn fingerprint(key: &str, occurrence: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hasher.update(b"|");
    hasher.update(occurrence.to_string().as_bytes());
    hex::encode(hasher.finalize())
}

/// Numbers the rows of one import that share an identity key, in input order.
#[derive(Debug, Default)]
pub struct Occurrences(HashMap<String, (u32, usize)>);

/// Where a row sits among the rows of its import that share its key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    pub fingerprint: String,
    /// Index of the first row in the import with the same key, when this is
    /// not that row.
    pub earlier_row: Option<usize>,
}

impl Occurrences {
    pub fn next(&mut self, key: &str, row_index: usize) -> Occurrence {
        let (count, first) = self.0.entry(key.to_string()).or_insert((0, row_index));
        let occurrence = Occurrence {
            fingerprint: fingerprint(key, *count),
            earlier_row: (*count > 0).then_some(*first),
        };
        *count += 1;
        occurrence
    }
}

/// Words that appear across unrelated merchants, so sharing one is no evidence
/// that two descriptions name the same thing.
const GENERIC_WORDS: &[&str] = &[
    "bank", "card", "credit", "debit", "direct", "faster", "from", "limited", "london", "online",
    "order", "payment", "purchase", "standing", "transfer", "with",
];

/// Whether two descriptions plausibly name the same merchant or counterparty:
/// one contains the other, or they share a distinctive word of four or more
/// characters. Inputs should already be `normalize_description`-ed.
pub fn descriptions_similar(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a.contains(b) || b.contains(a) {
        return true;
    }
    let distinctive = |s: &'_ str| -> Vec<String> {
        s.split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.chars().count() >= 4)
            .filter(|w| w.chars().any(char::is_alphabetic))
            .map(str::to_lowercase)
            .filter(|w| !GENERIC_WORDS.contains(&w.as_str()))
            .collect()
    };
    let words_a = distinctive(a);
    distinctive(b).iter().any(|w| words_a.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn transaction_key_ignores_amount_formatting() {
        let date = d("2026-07-03");
        assert_eq!(
            transaction_key(date, dec("500"), "monzo"),
            transaction_key(date, dec("500.00"), "monzo")
        );
        assert_eq!(
            transaction_key(date, dec("-15.5"), "amex"),
            transaction_key(date, dec("-15.50"), "amex")
        );
    }

    #[test]
    fn transaction_key_changes_with_any_field() {
        let base = transaction_key(d("2026-07-03"), dec("-5.50"), "monzo");
        assert_ne!(
            base,
            transaction_key(d("2026-07-04"), dec("-5.50"), "monzo")
        );
        assert_ne!(
            base,
            transaction_key(d("2026-07-03"), dec("-5.51"), "monzo")
        );
        assert_ne!(base, transaction_key(d("2026-07-03"), dec("5.50"), "monzo"));
        assert_ne!(
            base,
            transaction_key(d("2026-07-03"), dec("-5.50"), "revolut")
        );
    }

    #[test]
    fn investment_key_ignores_number_formatting() {
        let date = d("2026-03-13");
        assert_eq!(
            investment_key("schwab", "PLTR", date, dec("40"), dec("153.1"), "sell"),
            investment_key("schwab", "PLTR", date, dec("40.000"), dec("153.10"), "sell")
        );
    }

    #[test]
    fn fingerprint_is_a_sha256_hex_that_depends_on_the_occurrence() {
        let key = transaction_key(d("2026-07-03"), dec("-15.50"), "amex");
        assert_eq!(fingerprint(&key, 0).len(), 64);
        assert_eq!(fingerprint(&key, 0), fingerprint(&key, 0));
        assert_ne!(fingerprint(&key, 0), fingerprint(&key, 1));
    }

    #[test]
    fn occurrences_number_repeated_keys_and_point_at_the_first_row() {
        let tfl = transaction_key(d("2026-07-03"), dec("-15.50"), "amex");
        let pret = transaction_key(d("2026-07-03"), dec("-4.20"), "amex");
        let mut occ = Occurrences::default();

        let first = occ.next(&tfl, 0);
        let other = occ.next(&pret, 1);
        let second = occ.next(&tfl, 2);

        assert_eq!(first.fingerprint, fingerprint(&tfl, 0));
        assert_eq!(first.earlier_row, None);
        assert_eq!(other.earlier_row, None);
        assert_eq!(second.fingerprint, fingerprint(&tfl, 1));
        assert_eq!(second.earlier_row, Some(0));
    }

    #[test]
    fn similar_descriptions() {
        assert!(descriptions_similar("tfl travel ch", "tfl travel charge"));
        assert!(descriptions_similar(
            "pret a manger",
            "pret a manger london"
        ));
        assert!(descriptions_similar("to joda savings", "joda"));
        assert!(descriptions_similar(
            "sainsbury's s/mkts",
            "sainsburys sainsbury's"
        ));
    }

    #[test]
    fn dissimilar_descriptions() {
        assert!(!descriptions_similar("tesco stores 1234", "shell 1234"));
        assert!(!descriptions_similar(
            "card payment london",
            "online payment london"
        ));
        assert!(!descriptions_similar("", "tesco"));
        assert!(!descriptions_similar("pret", "costa"));
    }
}
