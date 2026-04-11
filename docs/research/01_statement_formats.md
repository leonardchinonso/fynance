# Statement Formats

## Overview

Banks export statements in four main formats, none of which share a universal schema:

| Format | Extension | Readability | Availability |
|---|---|---|---|
| CSV | .csv | Human-readable | Most common, varies by bank |
| OFX | .ofx | XML-based | Major US banks |
| QFX | .qfx | Quicken OFX variant | Chase, BofA, Wells Fargo |
| PDF | .pdf | Visual only | All banks; requires extraction |
| QBO | .qbo | QuickBooks variant | Rare, enterprise-focused |

## Bank-Specific Schemas

### Chase

- **Checking/Savings CSV fields**: Transaction Date, Description, Amount, Running Balance
- **Credit Card CSV fields**: Transaction Date, Post Date, Description, Category, Type, Amount, Memo
- Also exports QFX (Quicken) and OFX
- CSV limited to last few years; older data may require in-person branch request

### Bank of America

- **CSV fields**: Date, Description, Debit Amount, Credit Amount, Balance (separate debit/credit columns)
- Exports CSV and QFX
- Date format: MM/DD/YYYY

### Wells Fargo

- **CSV fields**: Date, Description, Amount, Running Balance, Check Number
- PDF-heavy; CSV available for checking
- Checking date format: MM/DD/YYYY

### Apple Card

- **CSV fields**: Transaction Date, Clearing Date, Merchant, Category, Type, Amount (USD)
- Apple uses its own category taxonomy (purchases, returns, payments)
- No OFX/QFX support

### Common Challenges

- **Amount sign conventions**: Some banks use negative for debits (Chase), others use separate debit/credit columns (BofA)
- **Date formats**: MM/DD/YYYY, YYYY-MM-DD, and DD/MM/YYYY all appear in the wild
- **Description normalization**: "STARBUCKS #1234 SEATTLE WA" vs. "SBUX*COFFEE" - same merchant, different strings
- **Encoding**: UTF-8 usually, but some banks export latin-1 or Windows-1252
- **Headers**: Some CSVs have bank name, account number, and date range as header rows before the column row

## Recommended Internal Schema

Normalize all inputs to this schema before storage:

```rust
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,                    // UUID, generated on import
    pub date: NaiveDate,               // Transaction date
    pub post_date: Option<NaiveDate>,  // Settlement date (may differ)
    pub description: String,           // Normalized merchant string
    pub raw_description: String,       // Unmodified original description
    pub amount: Decimal,               // Negative = debit, positive = credit
    pub account: String,               // Account identifier
    pub bank: String,                  // Bank name
    pub category: Option<String>,      // Assigned category
    pub confidence: Option<f64>,       // Categorization confidence
    pub tags: Vec<String>,             // User-defined tags
    pub memo: Option<String>,          // Notes
    pub source: SourceFormat,          // csv | ofx | qfx | pdf
    pub fitid: Option<String>,         // Bank's unique ID from OFX
    pub fingerprint: String,           // Hash for dedup when FITID missing
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceFormat { Csv, Ofx, Qfx, Pdf }
```

## OFX/QFX Format

Standard XML-based format. A transaction looks like:

```xml
<STMTTRN>
  <TRNTYPE>DEBIT</TRNTYPE>
  <DTPOSTED>20260411120000.000[-5:EST]</DTPOSTED>
  <TRNAMT>-45.99</TRNAMT>
  <FITID>20260411-001</FITID>
  <NAME>WHOLE FOODS MARKET</NAME>
  <MEMO>GROCERY</MEMO>
</STMTTRN>
```

Key fields:
- `TRNTYPE`: DEBIT, CREDIT, CHECK, INT (interest), DIV (dividend), FEE, ATM
- `DTPOSTED`: Date in `YYYYMMDDHHMMSS.mmm[tz]` format
- `TRNAMT`: Signed decimal (negative = debit)
- `FITID`: Bank's unique transaction ID (use for deduplication)
- `NAME`: Merchant name
- `MEMO`: Additional description (not always present)

## Handling Multi-Year Imports

When ingesting 2-3 years of statements:
1. Collect all files first, sort by account and date range
2. Identify and resolve overlapping date ranges (deduplication by FITID or date+amount+description hash)
3. Normalize descriptions (strip store numbers, state codes, etc.)
4. Flag transactions that appear in multiple exports as potential duplicates
