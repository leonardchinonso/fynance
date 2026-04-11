# Rust Crates for Statement Parsing

## Workspace Dependencies

A representative `Cargo.toml` for the project:

```toml
[package]
name = "fynance"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"

[dependencies]
# CLI
clap = { version = "4.5", features = ["derive"] }
indicatif = "0.17"
owo-colors = "4"

# Async runtime and HTTP
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"

# Storage
rusqlite = { version = "0.32", features = ["bundled", "chrono", "serde_json"] }

# Parsing
csv = "1.3"
regex = "1.11"
pdf-extract = "0.7"
lopdf = "0.34"

# Domain types
chrono = { version = "0.4", features = ["serde"] }
rust_decimal = { version = "1.36", features = ["serde"] }
rust_decimal_macros = "1.36"
uuid = { version = "1", features = ["v4"] }

# Error handling and logging
anyhow = "1"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

[dev-dependencies]
tempfile = "3"
pretty_assertions = "1"
```

## CSV Parsing

The `csv` crate is fast and mature. Combine with `serde` for struct deserialization. Bank CSVs often have header metadata rows; skip those manually before handing the reader to `csv`.

```rust
use csv::ReaderBuilder;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Deserialize)]
struct ChaseRow {
    #[serde(rename = "Transaction Date")]
    transaction_date: String,
    #[serde(rename = "Description")]
    description: String,
    #[serde(rename = "Amount")]
    amount: String,
}

pub fn parse_chase_csv(path: &Path) -> anyhow::Result<Vec<ChaseRow>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    // Skip metadata lines until we find the header
    let mut line = String::new();
    let mut header_pos = 0u64;
    loop {
        let pos = reader.stream_position()?;
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        if line.contains("Transaction Date") {
            header_pos = pos;
            break;
        }
    }
    reader.seek(SeekFrom::Start(header_pos))?;

    let mut csv_reader = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(reader);

    let mut rows = Vec::new();
    for result in csv_reader.deserialize() {
        rows.push(result?);
    }
    Ok(rows)
}
```

### Bank-Specific Notes

- **Chase**: Clean CSV, signed `Amount` column, MM/DD/YYYY dates
- **Bank of America**: Separate `Debit Amount` and `Credit Amount` columns, 6 metadata lines above the header
- **Apple Card**: Positive amounts for purchases (negate for internal representation)
- **Wells Fargo**: No header row; positional columns (date, amount, asterisk, check_num, description)

## OFX / QFX Parsing

Rust's OFX ecosystem is thinner than Python's. Options:

### Option 1: `sgmlish` or manual SGML/XML parsing

OFX is technically SGML, but QFX files are usually XML-compliant. Parse with `roxmltree` (lightweight, read-only):

```toml
roxmltree = "0.20"
```

```rust
use roxmltree::Document;
use rust_decimal::Decimal;
use std::str::FromStr;

#[derive(Debug)]
pub struct OfxTransaction {
    pub date: chrono::NaiveDate,
    pub amount: Decimal,
    pub name: String,
    pub memo: Option<String>,
    pub fitid: String,
}

pub fn parse_ofx(content: &str) -> anyhow::Result<Vec<OfxTransaction>> {
    // Strip the OFX SGML header if present
    let xml_start = content.find("<OFX>").unwrap_or(0);
    let xml = &content[xml_start..];

    let doc = Document::parse(xml)?;
    let mut transactions = Vec::new();

    for stmttrn in doc.descendants().filter(|n| n.has_tag_name("STMTTRN")) {
        let get = |tag: &str| stmttrn.descendants()
            .find(|n| n.has_tag_name(tag))
            .and_then(|n| n.text())
            .map(str::trim);

        let Some(dt) = get("DTPOSTED") else { continue };
        let Some(amt) = get("TRNAMT") else { continue };
        let Some(fitid) = get("FITID") else { continue };

        // DTPOSTED format: YYYYMMDDHHMMSS.mmm[tz] or just YYYYMMDD
        let date_str = &dt[..8];
        let date = chrono::NaiveDate::parse_from_str(date_str, "%Y%m%d")?;
        let amount = Decimal::from_str(amt)?;
        let name = get("NAME").unwrap_or("").to_string();
        let memo = get("MEMO").map(str::to_string);

        transactions.push(OfxTransaction {
            date,
            amount,
            name,
            memo,
            fitid: fitid.to_string(),
        });
    }
    Ok(transactions)
}
```

### Option 2: Pre-process with `sgmlish`

```toml
sgmlish = "0.4"
```

`sgmlish` normalizes OFX SGML to XML, which can then be fed to `roxmltree` or `quick-xml`. Use this when encountering old-style SGML OFX files (common from older bank exports).

### Option 3: Shell out

If OFX parsing becomes a maintenance burden, the Python `ofxtools` library is more mature. An acceptable middle ground is a tiny Python helper invoked from Rust via `std::process::Command` for OFX only, while keeping everything else native. This is a pragmatic fallback, not a first choice.

## PDF Parsing

PDF is the weakest area of Rust's parsing ecosystem compared to Python.

### `pdf-extract` (Primary for text)

```toml
pdf-extract = "0.7"
```

```rust
use pdf_extract::extract_text;
use std::path::Path;

pub fn extract_pdf_text(path: &Path) -> anyhow::Result<String> {
    let text = extract_text(path)?;
    Ok(text)
}
```

Extracts raw text from the PDF. Good for statements where transactions are listed as plain lines. You then need regex to pull out dates, descriptions, and amounts.

```rust
use regex::Regex;
use rust_decimal::Decimal;
use std::str::FromStr;

// Matches: "04/11/2026 WHOLE FOODS MKT #123 $87.23" or similar
pub fn parse_transaction_lines(text: &str) -> Vec<(String, String, Decimal)> {
    let re = Regex::new(
        r"(?m)^(\d{2}/\d{2}/\d{4})\s+(.+?)\s+\$?(-?[\d,]+\.\d{2})$"
    ).unwrap();

    re.captures_iter(text)
        .filter_map(|cap| {
            let date = cap.get(1)?.as_str().to_string();
            let desc = cap.get(2)?.as_str().trim().to_string();
            let amt_str = cap.get(3)?.as_str().replace(',', "");
            let amount = Decimal::from_str(&amt_str).ok()?;
            Some((date, desc, amount))
        })
        .collect()
}
```

### `lopdf` (Lower level, more control)

```toml
lopdf = "0.34"
```

Provides object-level PDF access. Useful when you need to inspect specific pages, fonts, or structural elements. Heavier to use than `pdf-extract` but more flexible.

### Claude Vision Fallback (Recommended for bank statements)

Rust lacks an equivalent to Python's `pdfplumber` for sophisticated table extraction. For any PDF where regex parsing fails, fall back to Claude's document input feature. This is not only easier than building a table parser, it is also more accurate and handles variable bank layouts automatically.

```rust
use base64::{engine::general_purpose, Engine};
use reqwest::Client;
use serde_json::json;
use std::fs;
use std::path::Path;

pub async fn extract_pdf_claude(
    client: &Client,
    api_key: &str,
    path: &Path,
) -> anyhow::Result<serde_json::Value> {
    let pdf_bytes = fs::read(path)?;
    let pdf_b64 = general_purpose::STANDARD.encode(&pdf_bytes);

    let body = json!({
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
                    "text": "Extract all transactions from this bank statement as JSON: {\"transactions\": [{\"date\": \"YYYY-MM-DD\", \"description\": \"...\", \"amount\": -0.00}]}. Use negative amounts for debits/charges, positive for credits/deposits. Return ONLY valid JSON."
                }
            ]
        }]
    });

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json::<serde_json::Value>()
        .await?;

    let text = resp["content"][0]["text"].as_str().unwrap_or("{}");
    let parsed: serde_json::Value = serde_json::from_str(text)?;
    Ok(parsed)
}
```

## Date Parsing

```rust
use chrono::NaiveDate;

pub fn parse_flexible_date(s: &str) -> Option<NaiveDate> {
    const FORMATS: &[&str] = &[
        "%m/%d/%Y", "%Y-%m-%d", "%m/%d/%y",
        "%B %d, %Y", "%b %d, %Y", "%d/%m/%Y",
    ];
    FORMATS.iter().find_map(|fmt| NaiveDate::parse_from_str(s, fmt).ok())
}
```

## Deduplication

```rust
use sha2::{Digest, Sha256};

pub fn fingerprint(date: &str, amount: &rust_decimal::Decimal, description: &str) -> String {
    let key = format!("{}|{:.2}|{}", date, amount, &description.chars().take(50).collect::<String>());
    let hash = Sha256::digest(key.as_bytes());
    hex::encode(&hash[..8])
}
```

```toml
sha2 = "0.10"
hex = "0.4"
```

Prefer the bank-provided OFX `FITID` when available; fall back to the fingerprint hash for CSV and PDF imports.

## Crate Decision Matrix

| Format | Primary | Fallback | Notes |
|---|---|---|---|
| CSV | `csv` + `serde` | `std` manual | Fast, mature, well-documented |
| OFX/QFX | `roxmltree` + manual | `sgmlish` preprocess | Rust OFX ecosystem is thin |
| PDF (text) | `pdf-extract` + `regex` | `lopdf` | Works for simple statements |
| PDF (complex) | Claude vision | manual PDF coding | Easier and more accurate than table extraction |
