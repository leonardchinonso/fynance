# Statement Importer

Only CSV is supported. OFX/QFX and PDF importers are deferred to a later phase.

## Importer Trait (`src/importers/mod.rs`)

```rust
use crate::model::Transaction;
use std::path::Path;
use anyhow::Result;

pub trait Importer: Send {
    fn can_handle(&self, path: &Path) -> bool;
    fn parse(&self, path: &Path) -> Result<Vec<Transaction>>;
}

pub fn get_importer<'a>(
    path: &Path,
    importers: &'a [Box<dyn Importer>],
) -> Option<&'a dyn Importer> {
    importers.iter().find(|i| i.can_handle(path)).map(|i| i.as_ref())
}
```

## CSV Importer (`src/importers/csv_importer.rs`)

Each bank exports CSV with different column names and amount conventions. `BankMapping` captures those differences; named constructors provide one per supported bank.

```rust
use crate::model::{SourceFormat, Transaction};
use crate::util::{normalize_description, parse_date};
use anyhow::{anyhow, Result};
use csv::ReaderBuilder;
use rust_decimal::Decimal;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::str::FromStr;
use std::fs::File;

#[derive(Clone)]
pub enum AmountSign {
    Signed,                              // One column, sign already correct (Chase)
    Negate,                              // One column, must negate (Apple Card)
    Split { debit: String, credit: String }, // Two columns (BofA)
}

#[derive(Clone)]
pub struct BankMapping {
    pub date_col: String,
    pub desc_col: String,
    pub amount_col: Option<String>,
    pub amount_sign: AmountSign,
    pub skip_rows: usize,       // Header rows before the CSV header (e.g. BofA has 6)
    pub date_format: &'static str,
}

pub struct CsvImporter {
    pub account_id: String,
    pub bank: String,
    pub mapping: BankMapping,
}

impl CsvImporter {
    /// Chase checking / credit: signed Amount column, MM/DD/YYYY dates.
    pub fn chase(account_id: &str) -> Self {
        Self {
            account_id: account_id.to_string(),
            bank: "chase".to_string(),
            mapping: BankMapping {
                date_col: "Transaction Date".to_string(),
                desc_col: "Description".to_string(),
                amount_col: Some("Amount".to_string()),
                amount_sign: AmountSign::Signed,
                skip_rows: 0,
                date_format: "%m/%d/%Y",
            },
        }
    }

    /// Bank of America: separate Debit/Credit columns, 6 metadata rows before the header.
    pub fn bofa(account_id: &str) -> Self {
        Self {
            account_id: account_id.to_string(),
            bank: "bofa".to_string(),
            mapping: BankMapping {
                date_col: "Date".to_string(),
                desc_col: "Description".to_string(),
                amount_col: None,
                amount_sign: AmountSign::Split {
                    debit: "Debit Amount".to_string(),
                    credit: "Credit Amount".to_string(),
                },
                skip_rows: 6,
                date_format: "%m/%d/%Y",
            },
        }
    }

    /// Apple Card: positive amounts for purchases, must negate.
    pub fn apple(account_id: &str) -> Self {
        Self {
            account_id: account_id.to_string(),
            bank: "apple".to_string(),
            mapping: BankMapping {
                date_col: "Transaction Date".to_string(),
                desc_col: "Merchant".to_string(),
                amount_col: Some("Amount (USD)".to_string()),
                amount_sign: AmountSign::Negate,
                skip_rows: 0,
                date_format: "%m/%d/%Y",
            },
        }
    }
}

impl super::Importer for CsvImporter {
    fn can_handle(&self, path: &Path) -> bool {
        path.extension().and_then(|e| e.to_str()) == Some("csv")
    }

    fn parse(&self, path: &Path) -> Result<Vec<Transaction>> {
        let file = File::open(path)?;
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

        let date_idx = col_index(&self.mapping.date_col)?;
        let desc_idx = col_index(&self.mapping.desc_col)?;
        let amount_idx = self.mapping.amount_col.as_deref()
            .map(|c| col_index(c))
            .transpose()?;

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
                    tracing::warn!("skipping row with unparseable date: {}", raw_date);
                    continue;
                }
            };

            let amount = match &self.mapping.amount_sign {
                AmountSign::Signed => {
                    let s = record.get(amount_idx.unwrap()).unwrap_or("0")
                        .replace(',', "").replace('$', "");
                    Decimal::from_str(&s).unwrap_or_default()
                }
                AmountSign::Negate => {
                    let s = record.get(amount_idx.unwrap()).unwrap_or("0")
                        .replace(',', "").replace('$', "");
                    -Decimal::from_str(&s).unwrap_or_default()
                }
                AmountSign::Split { .. } => {
                    let debit = record.get(debit_idx.unwrap()).unwrap_or("0")
                        .replace(',', "").replace('$', "");
                    let credit = record.get(credit_idx.unwrap()).unwrap_or("0")
                        .replace(',', "").replace('$', "");
                    let d = Decimal::from_str(&debit).unwrap_or_default();
                    let c = Decimal::from_str(&credit).unwrap_or_default();
                    c - d
                }
            };

            transactions.push(Transaction::new(
                date,
                normalize_description(raw_desc),
                raw_desc.to_string(),
                amount,
                self.account_id.clone(),
                self.bank.clone(),
                SourceFormat::Csv,
            ));
        }

        Ok(transactions)
    }
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
            .filter(|p| p.is_file())
            .collect()
    } else {
        vec![path.to_path_buf()]
    };

    let pb = ProgressBar::new(paths.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{bar:40} {pos}/{len} {msg}")?,
    );

    for p in &paths {
        pb.set_message(p.file_name().unwrap_or_default().to_string_lossy().to_string());
        let Some(importer) = crate::importers::get_importer(p, importers) else {
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
                db.log_import(p, account_id, inserted, dupes, 0)?;
                pb.println(format!(
                    "  {} -> {} inserted, {} duplicates",
                    p.file_name().unwrap_or_default().to_string_lossy(),
                    inserted, dupes,
                ));
            }
            Err(e) => {
                pb.println(format!("  ERROR {}: {}", p.display(), e));
                db.log_import(p, account_id, 0, 0, 1)?;
            }
        }
        pb.inc(1);
    }

    pb.finish_with_message("Done");
    Ok(())
}
```
