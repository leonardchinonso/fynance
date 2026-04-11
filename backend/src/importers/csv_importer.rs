//! CSV importer for Monzo, Revolut, and Lloyds exports.
//!
//! The three banks disagree on almost every detail: column names, date
//! format, whether debit and credit live in one column or two, whether
//! categories are pre-assigned. We detect the bank from the header row,
//! then dispatch to a format-specific row mapper that produces a
//! `Transaction` ready to go through `Db::insert_transaction`.

use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use csv::{ReaderBuilder, StringRecord};
use indicatif::{ProgressBar, ProgressStyle};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::importers::Importer;
use crate::model::{CategorySource, ImportResult, InsertOutcome, Transaction};
use crate::storage::Db;
use crate::util::{fingerprint, normalize_description, parse_date};

/// Which bank's dialect we think this file is. The same `CsvImporter`
/// handles all three because the only things that vary are the column
/// indices and how amount signs are encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BankFormat {
    Monzo,
    Revolut,
    Lloyds,
    Unknown,
}

pub struct CsvImporter;

impl Importer for CsvImporter {
    fn import(&self, path: &Path, account_id: &str, db: &Db) -> Result<ImportResult> {
        let file = File::open(path).with_context(|| format!("opening {path:?}"))?;
        let mut reader = ReaderBuilder::new().has_headers(true).from_reader(file);

        // `headers()` reads lazily on first call; `clone()` copies the
        // record so we can still iterate rows after detection.
        let headers = reader.headers().context("reading csv headers")?.clone();
        let format = detect_format(&headers);
        if format == BankFormat::Unknown {
            return Err(anyhow!(
                "could not detect bank format from headers: {:?}",
                headers.iter().collect::<Vec<_>>()
            ));
        }

        let col_idx = ColumnIndex::build(&headers, format)?;

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("<unknown>")
            .to_string();

        let mut result = ImportResult {
            filename: filename.clone(),
            account_id: account_id.to_string(),
            ..ImportResult::default()
        };

        let progress = ProgressBar::new_spinner();
        progress.set_style(
            ProgressStyle::with_template("{spinner} {pos} rows — {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        progress.set_message(filename);

        for record in reader.records() {
            let record = record.context("reading csv row")?;
            result.rows_total += 1;
            progress.inc(1);

            // Skip rows that don't parse rather than failing the whole file.
            // Phase 6 will upgrade this to return per-row errors; for Phase
            // 1 we log and keep going so a single bad line never blocks an
            // import.
            let parsed = match map_row(format, &col_idx, &record, account_id) {
                Ok(tx) => tx,
                Err(err) => {
                    tracing::warn!("skipping row in {}: {err}", result.filename);
                    continue;
                }
            };

            match db.insert_transaction(&parsed)? {
                InsertOutcome::Inserted => result.rows_inserted += 1,
                InsertOutcome::Duplicate => result.rows_duplicate += 1,
            }
        }

        progress.finish_and_clear();
        Ok(result)
    }
}

/// Inspect headers to figure out which bank exported this file. The order
/// of checks matters a little: Monzo and Revolut both have an `Amount`
/// column, but only Monzo has `Transaction ID`, and only Revolut has
/// `Completed Date`. Lloyds is the odd one out with `Debit Amount` +
/// `Credit Amount` as separate columns.
pub fn detect_format(headers: &StringRecord) -> BankFormat {
    let names: Vec<String> = headers.iter().map(|h| h.trim().to_string()).collect();
    let has = |needle: &str| names.iter().any(|n| n.eq_ignore_ascii_case(needle));

    if has("Transaction ID") && has("Amount") && has("Date") {
        BankFormat::Monzo
    } else if has("Completed Date") && has("Amount") && has("Description") {
        BankFormat::Revolut
    } else if has("Debit Amount") && has("Credit Amount") && has("Transaction Description") {
        BankFormat::Lloyds
    } else {
        BankFormat::Unknown
    }
}

/// Pre-computed column indices for the dialect in play. We look these up
/// once per file instead of every row to save time on large statements.
struct ColumnIndex {
    date: usize,
    description: usize,
    amount: Option<usize>,
    debit: Option<usize>,
    credit: Option<usize>,
    category: Option<usize>,
    fitid: Option<usize>,
}

impl ColumnIndex {
    fn build(headers: &StringRecord, format: BankFormat) -> Result<Self> {
        let find = |name: &str| -> Option<usize> {
            headers
                .iter()
                .position(|h| h.trim().eq_ignore_ascii_case(name))
        };
        let required = |name: &str| -> Result<usize> {
            find(name).ok_or_else(|| anyhow!("missing required column: {name}"))
        };

        Ok(match format {
            BankFormat::Monzo => Self {
                date: required("Date")?,
                description: required("Name")?,
                amount: Some(required("Amount")?),
                debit: None,
                credit: None,
                category: find("Category"),
                fitid: find("Transaction ID"),
            },
            BankFormat::Revolut => Self {
                date: required("Completed Date")?,
                description: required("Description")?,
                amount: Some(required("Amount")?),
                debit: None,
                credit: None,
                category: None,
                fitid: None,
            },
            BankFormat::Lloyds => Self {
                date: required("Transaction Date")?,
                description: required("Transaction Description")?,
                amount: None,
                debit: Some(required("Debit Amount")?),
                credit: Some(required("Credit Amount")?),
                category: None,
                fitid: None,
            },
            BankFormat::Unknown => return Err(anyhow!("unknown format")),
        })
    }
}

/// Map a single CSV row to a `Transaction`.
///
/// The tricky bit is amount signs: Monzo and Revolut already sign the
/// amount (negative = debit), but Lloyds splits debit and credit into
/// separate columns where the value is always positive, so we have to
/// flip the sign on debits ourselves.
fn map_row(
    format: BankFormat,
    idx: &ColumnIndex,
    record: &StringRecord,
    account_id: &str,
) -> Result<Transaction> {
    let get = |i: usize| -> &str { record.get(i).unwrap_or("").trim() };

    let date_raw = get(idx.date);
    let date = parse_date(date_raw)?;
    let date_iso = date.format("%Y-%m-%d").to_string();

    let description = get(idx.description).to_string();
    if description.is_empty() {
        return Err(anyhow!("empty description"));
    }

    let amount: Decimal = match format {
        BankFormat::Monzo | BankFormat::Revolut => {
            let raw = get(idx.amount.expect("amount col required for monzo/revolut"));
            parse_amount(raw)?
        }
        BankFormat::Lloyds => {
            // Lloyds gives debit and credit as two separate positive
            // columns. Exactly one of them is populated per row; the other
            // is empty. Debits must be flipped to negative to match the
            // rest of the app's "negative = money out" convention.
            let debit_raw = get(idx.debit.expect("debit col required for lloyds"));
            let credit_raw = get(idx.credit.expect("credit col required for lloyds"));
            if !debit_raw.is_empty() {
                -parse_amount(debit_raw)?
            } else if !credit_raw.is_empty() {
                parse_amount(credit_raw)?
            } else {
                return Err(anyhow!("lloyds row with no debit or credit value"));
            }
        }
        BankFormat::Unknown => unreachable!("unknown format reached map_row"),
    };

    let normalized = normalize_description(&description);

    // Fingerprint deliberately uses the raw (not normalized) description
    // so a future change to normalization rules does not invalidate old
    // fingerprints. See util::fingerprint.
    let fp = fingerprint(&date_iso, &amount.to_string(), &description, account_id);

    let category = idx
        .category
        .and_then(|i| record.get(i))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let category_source = category.as_ref().map(|_| CategorySource::Rule);

    let fitid = idx
        .fitid
        .and_then(|i| record.get(i))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Ok(Transaction {
        id: Uuid::new_v4().to_string(),
        date,
        description,
        normalized,
        amount,
        currency: "GBP".to_string(),
        account_id: account_id.to_string(),
        category,
        category_source,
        confidence: None,
        notes: None,
        is_recurring: false,
        fingerprint: fp,
        fitid,
    })
}

/// Parse an amount from a bank CSV. Strips commas (thousands separators,
/// common on Lloyds exports) and optional currency symbols before handing
/// off to `Decimal::from_str_exact` which refuses to accept floats.
fn parse_amount(raw: &str) -> Result<Decimal> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ',' && *c != '£' && *c != '$' && *c != '€')
        .collect();
    Decimal::from_str_exact(&cleaned).with_context(|| format!("parsing amount {raw:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(names: &[&str]) -> StringRecord {
        StringRecord::from(names.to_vec())
    }

    #[test]
    fn detects_monzo() {
        let h = headers(&["Transaction ID", "Date", "Name", "Amount"]);
        assert_eq!(detect_format(&h), BankFormat::Monzo);
    }

    #[test]
    fn detects_revolut() {
        let h = headers(&["Type", "Completed Date", "Description", "Amount"]);
        assert_eq!(detect_format(&h), BankFormat::Revolut);
    }

    #[test]
    fn detects_lloyds() {
        let h = headers(&[
            "Transaction Date",
            "Transaction Description",
            "Debit Amount",
            "Credit Amount",
        ]);
        assert_eq!(detect_format(&h), BankFormat::Lloyds);
    }

    #[test]
    fn parse_amount_strips_commas_and_symbols() {
        assert_eq!(parse_amount("1,234.56").unwrap(), Decimal::new(123456, 2));
        assert_eq!(parse_amount("£5.50").unwrap(), Decimal::new(550, 2));
        assert_eq!(parse_amount("-5.50").unwrap(), Decimal::new(-550, 2));
    }
}
