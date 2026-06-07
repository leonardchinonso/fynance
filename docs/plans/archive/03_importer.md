# Statement Importer

> **Superseded for CSV ingestion by [`10_llm_csv_import.md`](10_llm_csv_import.md).** The bank-specific `BankMapping`, `AmountSign`, and `detect_format` paths documented below are preserved for historical context but must not be reimplemented. The CSV path is an LLM-driven unified parser that emits `UnifiedStatementRow` regardless of bank, and uses `BankFormat` only as a bookkeeping tag. See plan 10 for the confidence-threshold contract, the unknown-bank behaviour, and the new Phase 1 checklist.
>
> **Updated after Prompt 1.1.** Target banks are Monzo, Revolut, and Lloyds. The original Chase/BofA/Apple mappings from the first draft are preserved conceptually but replaced with UK bank formats.

## Supported Formats (MVP)

| Bank | Format | Amount Convention | Date Format |
|---|---|---|---|
| Monzo | CSV | Signed `Amount` column (negative = debit) | `YYYY-MM-DD` |
| Revolut | CSV | Signed `Amount` column (negative = debit) | `YYYY-MM-DD HH:MM:SS` |
| Lloyds | CSV | Split `Debit Amount` / `Credit Amount` columns | `DD/MM/YYYY` |

OFX/QFX and PDF importers are deferred to a later phase.

## Importer Trait (`src/importers/mod.rs`)

```rust
use crate::model::Transaction;
use std::path::Path;
use anyhow::Result;

pub trait Importer: Send + Sync {
    fn can_handle(&self, path: &Path, account_hint: Option<&str>) -> bool;
    fn parse(&self, path: &Path) -> Result<Vec<Transaction>>;
}

pub fn get_importer<'a>(
    path: &Path,
    account_hint: Option<&str>,
    importers: &'a [Box<dyn Importer>],
) -> Option<&'a dyn Importer> {
    importers.iter()
        .find(|i| i.can_handle(path, account_hint))
        .map(|i| i.as_ref())
}
```

The `account_hint` is the `--account` flag value, which encodes the institution (e.g. `monzo-current`, `revolut-main`, `lloyds-savings`). Importers use both the filename and the hint to decide whether to claim a file.

## Bank Mapping (`src/importers/csv_importer.rs`)

```rust
use crate::model::{Account, AccountType, Transaction, CategorySource};
use crate::util::{normalize_description, parse_date, fingerprint};
use anyhow::{anyhow, Context, Result};
use csv::ReaderBuilder;
use rust_decimal::Decimal;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::str::FromStr;
use uuid::Uuid;

#[derive(Clone)]
pub enum AmountSign {
    Signed,                                         // one column, sign already correct
    Negate,                                         // one column, must flip sign
    Split { debit: String, credit: String },        // two columns
}

#[derive(Clone)]
pub struct BankMapping {
    pub institution: &'static str,
    pub date_col: &'static str,
    pub desc_col: &'static str,
    pub amount_col: Option<&'static str>,
    pub amount_sign: AmountSign,
    pub fitid_col: Option<&'static str>,
    pub currency_col: Option<&'static str>,
    pub default_currency: &'static str,
    pub skip_rows: usize,                           // raw lines before the CSV header
    pub date_format: &'static str,
}

pub struct CsvImporter {
    pub account_id: String,
    pub mapping: BankMapping,
}

impl CsvImporter {
    /// Monzo: signed `Amount`, `Transaction ID` fitid, ISO dates.
    pub fn monzo(account_id: &str) -> Self {
        Self {
            account_id: account_id.to_string(),
            mapping: BankMapping {
                institution: "Monzo",
                date_col: "Date",
                desc_col: "Name",
                amount_col: Some("Amount"),
                amount_sign: AmountSign::Signed,
                fitid_col: Some("Transaction ID"),
                currency_col: Some("Currency"),
                default_currency: "GBP",
                skip_rows: 0,
                date_format: "%Y-%m-%d",
            },
        }
    }

    /// Revolut: signed `Amount`, combined datetime in `Completed Date`.
    pub fn revolut(account_id: &str) -> Self {
        Self {
            account_id: account_id.to_string(),
            mapping: BankMapping {
                institution: "Revolut",
                date_col: "Completed Date",
                desc_col: "Description",
                amount_col: Some("Amount"),
                amount_sign: AmountSign::Signed,
                fitid_col: None,
                currency_col: Some("Currency"),
                default_currency: "GBP",
                skip_rows: 0,
                date_format: "%Y-%m-%d %H:%M:%S",
            },
        }
    }

    /// Lloyds: split Debit/Credit, DD/MM/YYYY dates.
    pub fn lloyds(account_id: &str) -> Self {
        Self {
            account_id: account_id.to_string(),
            mapping: BankMapping {
                institution: "Lloyds",
                date_col: "Transaction Date",
                desc_col: "Transaction Description",
                amount_col: None,
                amount_sign: AmountSign::Split {
                    debit: "Debit Amount".into(),
                    credit: "Credit Amount".into(),
                },
                fitid_col: None,
                currency_col: None,
                default_currency: "GBP",
                skip_rows: 0,
                date_format: "%d/%m/%Y",
            },
        }
    }
}

impl super::Importer for CsvImporter {
    fn can_handle(&self, path: &Path, account_hint: Option<&str>) -> bool {
        let is_csv = path.extension().and_then(|e| e.to_str()) == Some("csv");
        if !is_csv { return false; }
        account_hint
            .map(|h| h.starts_with(&self.mapping.institution.to_lowercase()))
            .unwrap_or(false)
    }

    fn parse(&self, path: &Path) -> Result<Vec<Transaction>> {
        let file = File::open(path).with_context(|| format!("opening {:?}", path))?;
        let mut reader = BufReader::new(file);

        for _ in 0..self.mapping.skip_rows {
            let mut line = String::new();
            reader.read_line(&mut line)?;
        }

        let mut csv_rdr = ReaderBuilder::new()
            .has_headers(true)
            .flexible(true)
            .from_reader(reader);

        let headers = csv_rdr.headers()?.clone();
        let col_index = |name: &str| -> Result<usize> {
            headers.iter().position(|h| h.trim() == name)
                .ok_or_else(|| anyhow!("column '{}' not found in {:?}", name, path))
        };

        let date_idx = col_index(self.mapping.date_col)?;
        let desc_idx = col_index(self.mapping.desc_col)?;
        let amount_idx = self.mapping.amount_col.map(col_index).transpose()?;
        let fitid_idx = self.mapping.fitid_col.map(col_index).transpose()?;
        let currency_idx = self.mapping.currency_col.map(col_index).transpose()?;

        let (debit_idx, credit_idx) = match &self.mapping.amount_sign {
            AmountSign::Split { debit, credit } => {
                (Some(col_index(debit)?), Some(col_index(credit)?))
            }
            _ => (None, None),
        };

        let mut transactions = Vec::new();

        for result in csv_rdr.records() {
            let record = result?;
            let raw_date = record.get(date_idx).unwrap_or("").trim();
            let raw_desc = record.get(desc_idx).unwrap_or("").trim();
            if raw_date.is_empty() || raw_desc.is_empty() { continue; }

            let date = match parse_date(raw_date) {
                Some(d) => d,
                None => {
                    tracing::debug!("skipping row with unparseable date");
                    continue;
                }
            };

            let amount = match &self.mapping.amount_sign {
                AmountSign::Signed => parse_decimal(record.get(amount_idx.unwrap()).unwrap_or("0")),
                AmountSign::Negate => -parse_decimal(record.get(amount_idx.unwrap()).unwrap_or("0")),
                AmountSign::Split { .. } => {
                    let d = parse_decimal(record.get(debit_idx.unwrap()).unwrap_or("0"));
                    let c = parse_decimal(record.get(credit_idx.unwrap()).unwrap_or("0"));
                    c - d
                }
            };

            let currency = currency_idx
                .and_then(|i| record.get(i))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| self.mapping.default_currency.to_string());

            let fitid = fitid_idx
                .and_then(|i| record.get(i))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());

            let description = normalize_description(raw_desc);
            let fp = fingerprint(&date.to_string(), &amount, &description, &self.account_id);

            transactions.push(Transaction {
                id: Uuid::new_v4().to_string(),
                date,
                description,
                raw_description: raw_desc.to_string(),
                amount,
                currency,
                account_id: self.account_id.clone(),
                category: None,
                category_source: None,
                confidence: None,
                notes: None,
                fingerprint: fp,
                fitid,
            });
        }

        Ok(transactions)
    }
}

fn parse_decimal(s: &str) -> Decimal {
    let cleaned = s.replace(',', "").replace('£', "").replace('$', "").replace('€', "");
    Decimal::from_str(cleaned.trim()).unwrap_or_default()
}
```

## Import Command (`src/commands/import.rs`)

```rust
use crate::storage::db::{Db, InsertResult};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::Path;

pub fn run(
    path: &Path,
    account_id: &str,
    db: &Db,
    importers: &[Box<dyn crate::importers::Importer>],
) -> anyhow::Result<()> {
    let paths: Vec<_> = if path.is_dir() {
        std::fs::read_dir(path)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("csv"))
            .collect()
    } else {
        vec![path.to_path_buf()]
    };

    let pb = ProgressBar::new(paths.len() as u64);
    pb.set_style(ProgressStyle::default_bar().template("{bar:40} {pos}/{len} {msg}")?);

    for p in &paths {
        pb.set_message(p.file_name().unwrap_or_default().to_string_lossy().to_string());
        let Some(importer) = crate::importers::get_importer(p, Some(account_id), importers) else {
            pb.println(format!("  skip {}: no matching importer", p.display()));
            pb.inc(1);
            continue;
        };

        match importer.parse(p) {
            Ok(transactions) => {
                let mut inserted = 0usize;
                let mut dupes = 0usize;
                for txn in &transactions {
                    match db.insert_transaction(txn)? {
                        InsertResult::Inserted  => inserted += 1,
                        InsertResult::Duplicate => dupes += 1,
                    }
                }
                db.log_import(p, account_id, transactions.len(), inserted, dupes)?;
                pb.println(format!(
                    "  {}: {} new, {} duplicate",
                    p.file_name().unwrap_or_default().to_string_lossy(),
                    inserted, dupes,
                ));
            }
            Err(e) => {
                pb.println(format!("  error {}: {}", p.display(), e));
            }
        }
        pb.inc(1);
    }

    pb.finish_with_message("Done");
    Ok(())
}
```

## Upload Endpoint (`src/server/routes/import.rs`)

The UI can also upload a CSV through `POST /api/import` (multipart form, `file` + `account_id` fields). The handler writes the upload to a temp file, runs the same import pipeline, and returns a summary JSON.

```rust
#[derive(serde::Serialize)]
pub struct ImportSummary {
    filename: String,
    account_id: String,
    rows_total: usize,
    rows_inserted: usize,
    rows_duplicate: usize,
    errors: Vec<String>,
}
```
