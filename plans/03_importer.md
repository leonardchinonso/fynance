# Statement Importer

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

```rust
use crate::model::{SourceFormat, Transaction};
use crate::util::{normalize_description, parse_date};
use anyhow::{anyhow, Context, Result};
use csv::ReaderBuilder;
use rust_decimal::Decimal;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;
use std::str::FromStr;
use std::{collections::HashMap, fs::File};

#[derive(Clone)]
pub enum AmountSign { Signed, Negate, Split { debit: String, credit: String } }

#[derive(Clone)]
pub struct BankMapping {
    pub date_col: String,
    pub desc_col: String,
    pub amount_col: Option<String>,
    pub amount_sign: AmountSign,
    pub skip_rows: usize,
    pub date_format: &'static str,
}

pub struct CsvImporter {
    pub account_id: String,
    pub bank: String,
    pub mapping: BankMapping,
}

impl CsvImporter {
    /// Build a Chase checking/credit importer.
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

    /// Build a Bank of America importer (separate debit/credit columns).
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

    /// Build an Apple Card importer (positive amounts for purchases).
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

        // Skip configured metadata rows (e.g. BofA has 6)
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
                .ok_or_else(|| anyhow!("column '{}' not found", name))
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
                None => { continue; }
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

## OFX / QFX Importer (`src/importers/ofx_importer.rs`)

```rust
use crate::model::{SourceFormat, Transaction};
use crate::util::{normalize_description, parse_date};
use anyhow::Result;
use roxmltree::Document;
use rust_decimal::Decimal;
use std::fs;
use std::path::Path;
use std::str::FromStr;

pub struct OfxImporter {
    pub account_id: String,
    pub bank: String,
}

impl super::Importer for OfxImporter {
    fn can_handle(&self, path: &Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("ofx") | Some("qfx") | Some("qbo")
        )
    }

    fn parse(&self, path: &Path) -> Result<Vec<Transaction>> {
        let raw = fs::read_to_string(path)?;

        // OFX files may have an SGML header before the XML body.
        // Find <OFX> or <ofx> and parse from there.
        let xml_start = raw.find("<OFX>")
            .or_else(|| raw.find("<ofx>"))
            .unwrap_or(0);
        let xml = &raw[xml_start..];

        let doc = Document::parse(xml)?;
        let mut transactions = Vec::new();

        for stmttrn in doc.descendants().filter(|n| n.has_tag_name("STMTTRN")) {
            let get = |tag: &str| -> Option<&str> {
                stmttrn.descendants()
                    .find(|n| n.has_tag_name(tag))
                    .and_then(|n| n.text())
                    .map(str::trim)
            };

            let dt_raw = get("DTPOSTED").unwrap_or("");
            let amt_raw = get("TRNAMT").unwrap_or("0");
            let fitid = get("FITID").unwrap_or("").to_string();
            let name = get("NAME").unwrap_or("");
            let memo = get("MEMO");

            // DTPOSTED: YYYYMMDD or YYYYMMDDHHMMSS.mmm[tz]
            let date_str = &dt_raw[..dt_raw.len().min(8)];
            let date = match chrono::NaiveDate::parse_from_str(date_str, "%Y%m%d") {
                Ok(d) => d,
                Err(_) => continue,
            };
            let amount = Decimal::from_str(amt_raw).unwrap_or_default();

            let raw_desc = if memo.is_some_and(|m| m != name) {
                format!("{} {}", name, memo.unwrap_or("")).trim().to_string()
            } else {
                name.to_string()
            };

            let mut txn = Transaction::new(
                date,
                normalize_description(&raw_desc),
                raw_desc,
                amount,
                self.account_id.clone(),
                self.bank.clone(),
                SourceFormat::Ofx,
            );
            if !fitid.is_empty() {
                txn.fitid = Some(fitid);
            }

            transactions.push(txn);
        }

        Ok(transactions)
    }
}
```

## PDF Importer (`src/importers/pdf_importer.rs`)

```rust
use crate::model::{SourceFormat, Transaction};
use crate::util::{normalize_description, parse_date};
use anyhow::Result;
use regex::Regex;
use rust_decimal::Decimal;
use std::path::Path;
use std::str::FromStr;

pub struct PdfImporter {
    pub account_id: String,
    pub bank: String,
    pub http: reqwest::Client,
    pub api_key: String,
}

impl super::Importer for PdfImporter {
    fn can_handle(&self, path: &Path) -> bool {
        path.extension().and_then(|e| e.to_str()) == Some("pdf")
    }

    fn parse(&self, path: &Path) -> Result<Vec<Transaction>> {
        // Try text extraction first (fast, free)
        let text = pdf_extract::extract_text(path)?;
        let mut txns = parse_pdf_text(&text, &self.account_id, &self.bank);

        if txns.len() < 3 {
            // Fallback to Claude vision (async, block here)
            tracing::info!("pdf-extract found {} transactions, using Claude vision", txns.len());
            txns = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(
                    extract_via_claude(&self.http, &self.api_key, path, &self.account_id, &self.bank)
                )
            })?;
        }

        Ok(txns)
    }
}

fn parse_pdf_text(text: &str, account: &str, bank: &str) -> Vec<Transaction> {
    // Matches: "04/11/2026  WHOLE FOODS MKT  -87.23" or "04/11/2026  DEPOSIT  1000.00"
    let re = Regex::new(
        r"(?m)(\d{2}/\d{2}/\d{4})\s{2,}(.+?)\s{2,}(-?[\d,]+\.\d{2})"
    ).unwrap();

    re.captures_iter(text).filter_map(|cap| {
        let raw_date = cap.get(1)?.as_str();
        let raw_desc = cap.get(2)?.as_str().trim();
        let raw_amt  = cap.get(3)?.as_str().replace(',', "");

        let date   = parse_date(raw_date)?;
        let amount = Decimal::from_str(&raw_amt).ok()?;

        Some(Transaction::new(
            date,
            normalize_description(raw_desc),
            raw_desc.to_string(),
            amount,
            account.to_string(),
            bank.to_string(),
            SourceFormat::Pdf,
        ))
    }).collect()
}

async fn extract_via_claude(
    client: &reqwest::Client,
    api_key: &str,
    path: &Path,
    account: &str,
    bank: &str,
) -> Result<Vec<Transaction>> {
    use base64::{engine::general_purpose, Engine};
    let pdf_b64 = general_purpose::STANDARD.encode(std::fs::read(path)?);

    let body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 4096,
        "messages": [{
            "role": "user",
            "content": [
                {
                    "type": "document",
                    "source": {
                        "type": "base64",
                        "media_type": "application/pdf",
                        "data": pdf_b64
                    }
                },
                {
                    "type": "text",
                    "text": "Extract all transactions as JSON: {\"transactions\": [{\"date\": \"YYYY-MM-DD\", \"description\": \"...\", \"amount\": -0.00}]}. Negative = debit, positive = credit. Return ONLY valid JSON."
                }
            ]
        }]
    });

    let resp: serde_json::Value = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send().await?
        .error_for_status()?
        .json().await?;

    let text = resp["content"][0]["text"].as_str().unwrap_or("{}");
    let parsed: serde_json::Value = serde_json::from_str(text)?;

    let txns = parsed["transactions"].as_array()
        .map(|arr| arr.iter().filter_map(|t| {
            let date = parse_date(t["date"].as_str()?)?;
            let raw_desc = t["description"].as_str()?.to_string();
            let amount = Decimal::from_str(
                &t["amount"].to_string()
            ).ok()?;
            Some(Transaction::new(
                date,
                normalize_description(&raw_desc),
                raw_desc,
                amount,
                account.to_string(),
                bank.to_string(),
                SourceFormat::Pdf,
            ))
        }).collect())
        .unwrap_or_default();

    Ok(txns)
}
```

## Import Command (`src/commands/import.rs`)

```rust
use crate::storage::db::{Db, InsertResult};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::Path;

pub fn run(path: &Path, account_id: &str, db: &Db, importers: &[Box<dyn crate::importers::Importer>]) -> anyhow::Result<()> {
    let paths: Vec<_> = if path.is_dir() {
        glob::glob(&format!("{}/**/*", path.display()))?
            .filter_map(|e| e.ok())
            .filter(|p| p.is_file())
            .collect()
    } else {
        vec![path.to_path_buf()]
    };

    let pb = ProgressBar::new(paths.len() as u64);
    pb.set_style(ProgressStyle::default_bar()
        .template("{bar:40} {pos}/{len} {msg}")?);

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
                    inserted, dupes
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
