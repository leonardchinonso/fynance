//! Small pure helpers shared by the importer, storage, and CLI layers.

use anyhow::{Context, Result, anyhow};
use chrono::NaiveDate;
use once_cell::sync::Lazy;
use regex::Regex;
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};

/// Regex that collapses any run of whitespace into a single space. Used by
/// `normalize_description` to produce a stable display string regardless of
/// how the source bank padded the merchant field.
static WHITESPACE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());

/// Noise tokens that banks tend to tack on to merchant descriptions and
/// that we want stripped before computing fingerprints. The list is
/// deliberately conservative: we'd rather keep a token than drop real
/// information.
static NOISE_TOKENS: &[&str] = &[
    "CARD PAYMENT TO ",
    "CARD PAYMENT ",
    "DIRECT DEBIT ",
    "BILL PAYMENT TO ",
    "BILL PAYMENT ",
    "PAYMENT TO ",
    "CONTACTLESS ",
    "POS ",
    "VIS ",
];

/// Clean a raw merchant description for display and fingerprinting.
///
/// The goal is stability across re-imports, not linguistic perfection: two
/// statements that show the same transaction must produce the same
/// normalized string so that fingerprint dedup catches them.
pub fn normalize_description(raw: &str) -> String {
    let mut s = raw.trim().to_string();

    let upper = s.to_uppercase();
    for token in NOISE_TOKENS {
        if let Some(stripped) = upper.strip_prefix(token) {
            // Preserve the original (post-token) substring rather than the
            // uppercased version, in case the merchant name has mixed case.
            let start = s.len() - stripped.len();
            s = s[start..].to_string();
            break;
        }
    }

    let collapsed = WHITESPACE.replace_all(s.trim(), " ");
    collapsed.to_lowercase()
}

/// Stable dedup fingerprint for a transaction.
///
/// We hash the four fields that together uniquely identify a transaction
/// across re-imports: the date, the signed amount as written by the bank,
/// the raw description, and the account it belongs to. Using the raw
/// description rather than the normalized one means a normalization tweak
/// in the future does not invalidate every previously-imported row.
pub fn fingerprint(date: &str, amount: &str, description: &str, account_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(date.as_bytes());
    hasher.update(b"|");
    hasher.update(amount.as_bytes());
    hasher.update(b"|");
    hasher.update(description.as_bytes());
    hasher.update(b"|");
    hasher.update(account_id.as_bytes());
    hex::encode(hasher.finalize())
}

/// Parse either the ISO form `YYYY-MM-DD` (Monzo, Revolut) or the UK form
/// `DD/MM/YYYY` (Lloyds). Anything else returns an error so bad data
/// cannot silently become an epoch or "today".
pub fn parse_date(s: &str) -> Result<NaiveDate> {
    let trimmed = s.trim();
    // Try ISO first because it is cheaper and what our own storage layer
    // writes back.
    if let Ok(d) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        return Ok(d);
    }
    // Fall back to the UK bank format used by Lloyds CSVs.
    if let Ok(d) = NaiveDate::parse_from_str(trimmed, "%d/%m/%Y") {
        return Ok(d);
    }
    // Some banks include a time component on the date field.
    if let Ok(d) =
        NaiveDate::parse_from_str(trimmed.split_whitespace().next().unwrap_or(""), "%Y-%m-%d")
    {
        return Ok(d);
    }
    Err(anyhow!("unrecognized date format: {s:?}"))
}

/// Parse an amount from a raw bank string. Strips commas (thousands
/// separators common on Lloyds exports) and optional currency symbols before
/// handing off to `Decimal::from_str_exact` which refuses to accept floats.
///
/// This is kept as a utility for the `Transaction::from_unified` path in case
/// the LLM returns an amount with a stray currency symbol despite being asked
/// not to.
pub fn parse_amount(raw: &str) -> Result<Decimal> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ',' && *c != '£' && *c != '$' && *c != '€')
        .collect();
    Decimal::from_str_exact(&cleaned).with_context(|| format!("parsing amount {raw:?}"))
}

/// Validate that a string looks like `YYYY-MM`. Used by budget CLI args.
pub fn parse_month(s: &str) -> Result<String> {
    let trimmed = s.trim();
    NaiveDate::parse_from_str(&format!("{trimmed}-01"), "%Y-%m-%d")
        .with_context(|| format!("month must be YYYY-MM, got {s:?}"))?;
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn normalize_strips_whitespace_and_lowercases() {
        assert_eq!(
            normalize_description("  LIDL   GB  LONDON "),
            "lidl gb london"
        );
    }

    #[test]
    fn normalize_strips_known_prefix() {
        assert_eq!(
            normalize_description("CARD PAYMENT TO LIDL GB LONDON"),
            "lidl gb london"
        );
    }

    #[test]
    fn normalize_is_stable_across_case() {
        assert_eq!(
            normalize_description("Lidl GB London"),
            normalize_description("LIDL GB LONDON")
        );
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let a = fingerprint("2026-03-10", "-5.50", "Lidl", "monzo-current");
        let b = fingerprint("2026-03-10", "-5.50", "Lidl", "monzo-current");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn fingerprint_changes_with_any_field() {
        let base = fingerprint("2026-03-10", "-5.50", "Lidl", "monzo-current");
        assert_ne!(
            base,
            fingerprint("2026-03-11", "-5.50", "Lidl", "monzo-current")
        );
        assert_ne!(
            base,
            fingerprint("2026-03-10", "-5.51", "Lidl", "monzo-current")
        );
        assert_ne!(
            base,
            fingerprint("2026-03-10", "-5.50", "Tesco", "monzo-current")
        );
        assert_ne!(
            base,
            fingerprint("2026-03-10", "-5.50", "Lidl", "revolut-main")
        );
    }

    #[test]
    fn parse_date_iso() {
        assert_eq!(
            parse_date("2026-03-10").unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 10).unwrap()
        );
    }

    #[test]
    fn parse_date_uk() {
        assert_eq!(
            parse_date("10/03/2026").unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 10).unwrap()
        );
    }

    #[test]
    fn parse_date_rejects_garbage() {
        assert!(parse_date("yesterday").is_err());
    }

    #[test]
    fn parse_month_accepts_valid() {
        assert_eq!(parse_month("2026-03").unwrap(), "2026-03");
    }

    #[test]
    fn parse_month_rejects_invalid() {
        assert!(parse_month("2026/03").is_err());
        assert!(parse_month("2026-13").is_err());
    }
}
