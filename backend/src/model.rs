//! Core domain types for fynance.
//!
//! Everything a caller might serialize out of the API lives here. All public
//! structs derive `ts_rs::TS` so a future `cargo test`-driven step can emit
//! matching TypeScript interfaces into `frontend/src/bindings/`.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A single bank transaction.
///
/// `amount` follows the bank convention: negative = money out, positive =
/// money in. Money is parsed into `Decimal` on the way in and serialized as
/// a string on the way out so we never touch floats.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct Transaction {
    pub id: String,
    #[ts(type = "string")]
    pub date: NaiveDate,
    pub description: String,
    pub normalized: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub amount: Decimal,
    pub currency: String,
    pub account_id: String,
    pub category: Option<String>,
    pub category_source: Option<CategorySource>,
    pub confidence: Option<f64>,
    pub notes: Option<String>,
    pub is_recurring: bool,
    pub fingerprint: String,
    pub fitid: Option<String>,
}

/// Where a transaction's category came from. Phase 1 only writes `Rule`
/// from CSV imports and `Manual` from CLI category edits; `Agent` is
/// reserved for data pushed in by external AI agents in later phases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum CategorySource {
    Rule,
    Agent,
    Manual,
}

impl CategorySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rule => "rule",
            Self::Agent => "agent",
            Self::Manual => "manual",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "rule" => Some(Self::Rule),
            "agent" | "claude" => Some(Self::Agent),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct Account {
    pub id: String,
    pub name: String,
    pub institution: String,
    #[serde(rename = "type")]
    pub account_type: AccountType,
    pub currency: String,
    #[serde(with = "rust_decimal::serde::str_option", default)]
    #[ts(type = "string | null")]
    pub balance: Option<Decimal>,
    #[ts(type = "string | null")]
    pub balance_date: Option<NaiveDate>,
    pub is_active: bool,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum AccountType {
    Checking,
    Savings,
    Investment,
    Credit,
    Cash,
    Pension,
}

impl AccountType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Checking => "checking",
            Self::Savings => "savings",
            Self::Investment => "investment",
            Self::Credit => "credit",
            Self::Cash => "cash",
            Self::Pension => "pension",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "checking" => Some(Self::Checking),
            "savings" => Some(Self::Savings),
            "investment" => Some(Self::Investment),
            "credit" => Some(Self::Credit),
            "cash" => Some(Self::Cash),
            "pension" => Some(Self::Pension),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct Budget {
    pub month: String,
    pub category: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub amount: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct PortfolioSnapshot {
    #[ts(type = "string")]
    pub snapshot_date: NaiveDate,
    pub account_id: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub balance: Decimal,
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct Holding {
    pub account_id: String,
    pub symbol: String,
    pub name: String,
    pub holding_type: HoldingType,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub quantity: Decimal,
    #[serde(with = "rust_decimal::serde::str_option", default)]
    #[ts(type = "string | null")]
    pub price_per_unit: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub value: Decimal,
    pub currency: String,
    #[ts(type = "string")]
    pub as_of: NaiveDate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum HoldingType {
    Stock,
    Etf,
    Fund,
    Bond,
    Crypto,
    Cash,
}

impl HoldingType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stock => "stock",
            Self::Etf => "etf",
            Self::Fund => "fund",
            Self::Bond => "bond",
            Self::Crypto => "crypto",
            Self::Cash => "cash",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "stock" => Some(Self::Stock),
            "etf" => Some(Self::Etf),
            "fund" => Some(Self::Fund),
            "bond" => Some(Self::Bond),
            "crypto" => Some(Self::Crypto),
            "cash" => Some(Self::Cash),
            _ => None,
        }
    }
}

/// Aggregated result of one file or payload import.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct ImportResult {
    pub rows_total: u64,
    pub rows_inserted: u64,
    pub rows_duplicate: u64,
    pub filename: String,
    pub account_id: String,
}

/// Result of a single `INSERT OR IGNORE`. Lets the CSV importer count new
/// rows vs. duplicates without a second query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertOutcome {
    Inserted,
    Duplicate,
}

/// Input record for writing an `import_log` row.
#[derive(Debug, Clone)]
pub struct ImportLog {
    pub filename: String,
    pub account_id: String,
    pub rows_total: u64,
    pub rows_inserted: u64,
    pub rows_duplicate: u64,
    pub source: String,
}
