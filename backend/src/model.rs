//! Core domain types for fynance.
//!
//! Everything a caller might serialize out of the API lives here. All public
//! structs derive `ts_rs::TS` so a future `cargo test`-driven step can emit
//! matching TypeScript interfaces into `frontend/src/bindings/`.

use std::collections::HashMap;

use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::util::{serde_naive_datetime, serde_naive_datetime_option};

/// A single bank transaction.
///
/// `amount` follows the bank convention: negative = money out, positive =
/// money in. Money is parsed into `Decimal` on the way in and serialized as
/// a string on the way out so we never touch floats.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct Transaction {
    pub id: String,
    #[serde(with = "serde_naive_datetime")]
    #[ts(type = "string")]
    pub date: NaiveDateTime,
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
    #[serde(with = "serde_naive_datetime_option", default)]
    #[ts(type = "string | null")]
    pub balance_date: Option<NaiveDateTime>,
    pub is_active: bool,
    pub notes: Option<String>,
    /// JSON array of profile IDs, e.g. `["alex", "sam"]`.
    /// Defaults to `["default"]` when not specified.
    #[serde(default = "default_profile_ids")]
    pub profile_ids: Vec<String>,
    /// Set in portfolio responses to indicate whether the carried-forward
    /// balance is stale (snapshot > 45 days before the query date).
    /// Absent (`None`) in non-portfolio contexts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_stale: Option<bool>,
}

fn default_profile_ids() -> Vec<String> {
    vec!["default".to_string()]
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

/// A profile represents one person in a multi-person household.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct Profile {
    pub id: String,
    pub name: String,
}

/// Time granularity used by spending-grid and portfolio-history endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum Granularity {
    Monthly,
    Quarterly,
    Yearly,
}

impl Granularity {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "monthly" => Some(Self::Monthly),
            "quarterly" => Some(Self::Quarterly),
            "yearly" => Some(Self::Yearly),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Monthly => "monthly",
            Self::Quarterly => "quarterly",
            Self::Yearly => "yearly",
        }
    }
}

/// One row in the spending-grid response. `periods` maps period strings
/// (e.g. "2026-01", "2026-Q1", "2026") to the spending total as a Decimal
/// string, or null if there were no transactions in that period.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct SpendingGridRow {
    pub category: String,
    pub section: String,
    /// Period key -> decimal string (or null). Amounts are signed:
    /// negative = expense, positive = income/credit.
    #[ts(type = "Record<string, string | null>")]
    pub periods: HashMap<String, Option<String>>,
    /// Average spend per period that had any transactions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average: Option<String>,
    /// Standing budget amount for this category (decimal string).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<String>,
    /// Sum across all periods.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<String>,
}

/// One row in the `GET /api/budget/:month` response.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct BudgetRow {
    pub category: String,
    /// Effective budget for this month (standing or override). Null if not set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budgeted: Option<String>,
    /// Actual spend this month (absolute value of negative transactions).
    pub actual: String,
    /// `actual / budgeted * 100`. Null when `budgeted` is null or zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
}

/// Aggregate spend per category, used by `GET /api/transactions/by-category`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct CategoryTotal {
    pub category: String,
    /// Signed sum of `amount` for this category (negative = net spend).
    pub total: String,
}

/// Maps one budget category to a spending-grid section.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct SectionMapping {
    /// One of: Income | Bills | Spending | Irregular | Transfers
    pub section: String,
    pub category: String,
}

/// A standing monthly budget target for one category.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct StandingBudget {
    pub category: String,
    pub amount: String,
}

/// Per-month override for a standing budget.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct BudgetOverride {
    pub month: String,
    pub category: String,
    pub amount: String,
}

/// Input record for `POST /api/import` (structured JSON from external agents).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct ImportTransaction {
    #[serde(with = "serde_naive_datetime")]
    #[ts(type = "string")]
    pub date: NaiveDateTime,
    pub description: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub amount: Decimal,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub category_source: Option<CategorySource>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub is_recurring: Option<bool>,
}

/// Request body for `POST /api/import`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct ImportPayload {
    pub account_id: String,
    pub transactions: Vec<ImportTransaction>,
}

/// Per-row error detail in a partial-success import response.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct ImportRowError {
    /// Zero-based index into the `transactions` array.
    pub index: usize,
    pub reason: String,
}

/// A standing budget (old model). Kept for CLI backward compatibility.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct Budget {
    pub month: String,
    pub category: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub amount: Decimal,
}

/// One row in the ingestion checklist for a given month.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct ChecklistItem {
    pub account_id: String,
    pub account_name: String,
    pub status: ChecklistStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum ChecklistStatus {
    Pending,
    Complete,
}

impl ChecklistStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Complete => "complete",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "complete" => Self::Complete,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct PortfolioSnapshot {
    #[serde(with = "serde_naive_datetime")]
    #[ts(type = "string")]
    pub snapshot_date: NaiveDateTime,
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
    #[serde(with = "serde_naive_datetime")]
    #[ts(type = "string")]
    pub as_of: NaiveDateTime,
    /// Optional short display name (e.g. ticker alias).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_name: Option<String>,
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

/// Which bank's dialect a CSV file came from. Used for bookkeeping / display
/// only; the import path never branches on this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum BankFormat {
    Monzo,
    Revolut,
    Lloyds,
    /// LLM could not confidently identify the bank, or it is an unrecognised
    /// institution. The import still proceeds as long as detection_confidence
    /// is above the threshold.
    #[default]
    Unknown,
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
    /// Bank detected by the LLM parser. `Unknown` if unrecognised or not yet
    /// set (e.g. in the aggregated totals object in the CLI).
    pub detected_bank: BankFormat,
    /// LLM's own confidence in `detected_bank` [0.0, 1.0]. Zero for the
    /// synthetic totals row produced by the CLI.
    pub detection_confidence: f32,
    /// Per-row errors from a partial-success import (API path only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ImportRowError>,
}

impl BankFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Monzo => "monzo",
            Self::Revolut => "revolut",
            Self::Lloyds => "lloyds",
            Self::Unknown => "unknown",
        }
    }
}

/// Full portfolio snapshot returned by `GET /api/portfolio`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct PortfolioResponse {
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub net_worth: Decimal,
    pub currency: String,
    /// ISO 8601 date the response was computed for (the `as_of` parameter or today).
    pub as_of: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub total_assets: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub total_liabilities: Decimal,
    /// Sum of checking, savings, investment, cash, and credit accounts.
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub available_wealth: Decimal,
    /// Sum of pension (and future: property) accounts.
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub unavailable_wealth: Decimal,
    /// All accounts with carry-forward balances. `is_stale` set per account.
    pub accounts: Vec<Account>,
    pub by_type: Vec<BreakdownItem>,
    pub by_institution: Vec<BreakdownItem>,
    pub by_asset_class: Vec<BreakdownItem>,
    pub investment_metrics: InvestmentMetrics,
}

/// One slice of a portfolio breakdown (by type, institution, or asset class).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct BreakdownItem {
    pub label: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub value: Decimal,
    pub percentage: f64,
}

/// One row in the `GET /api/portfolio/history` response.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct PortfolioHistoryRow {
    /// Period label: "YYYY-MM" for monthly, "YYYY-Qn" for quarterly, "YYYY" for yearly.
    pub month: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub available_wealth: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub unavailable_wealth: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub total_wealth: Decimal,
}

/// One row in the `GET /api/cash-flow` response.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct CashFlowMonth {
    /// Period label: "YYYY-MM", "YYYY-Qn", or "YYYY".
    pub month: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub income: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub spending: Decimal,
}

/// Start/end balance delta for one account. Used by `GET /api/portfolio/snapshots?summary=true`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct SnapshotDelta {
    pub account_id: String,
    #[serde(with = "rust_decimal::serde::str_option", default)]
    #[ts(type = "string | null")]
    pub start_balance: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::str_option", default)]
    #[ts(type = "string | null")]
    pub end_balance: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::str_option", default)]
    #[ts(type = "string | null")]
    pub delta: Option<Decimal>,
}

/// Investment performance metrics included in `GET /api/portfolio`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct InvestmentMetrics {
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub start_value: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub end_value: Decimal,
    /// `end_value - start_value`
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub total_growth: Decimal,
    /// Net cash moved into investment accounts over the period (positive = in).
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub new_cash_invested: Decimal,
    /// `total_growth - new_cash_invested` (pure price appreciation).
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub market_growth: Decimal,
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
    pub detected_bank: BankFormat,
    pub detection_confidence: f32,
}
