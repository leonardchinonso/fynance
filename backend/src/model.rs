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

/// Generic pagination envelope shared by list endpoints. `total` is the full
/// match count across all pages; `data` holds the current page.
#[derive(Debug, Serialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct Paginated<T> {
    pub data: Vec<T>,
    pub total: u32,
    pub page: u32,
    pub limit: u32,
}

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
    /// FK to categories.id; only leaf nodes are valid. The display name is
    /// resolved client-side from the categories list.
    pub category_id: Option<String>,
    pub category_source: Option<CategorySource>,
    pub confidence: Option<f64>,
    pub notes: Option<String>,
    pub is_recurring: bool,
    pub exclude_from_summary: bool,
    pub fingerprint: String,
    pub fitid: Option<String>,
    /// IDs of the source documents this transaction was extracted from
    /// (`documents.id`). Empty for manual / CSV / API imports with no document.
    #[serde(default)]
    pub source_document_ids: Vec<String>,
}

/// A hierarchical category entry.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct Category {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub display_order: i32,
    pub is_active: bool,
    /// Optional free-text description. Surfaced to LLM categorisation agents
    /// to disambiguate categories that share similar names (e.g. "Bills:
    /// utility bills like internet, water, electricity" vs "Subscriptions").
    pub description: Option<String>,
    pub category_type: CategoryType,
    pub created_at: String,
    pub updated_at: String,
}

/// Tree node used in `GET /api/categories` response.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct CategoryNode {
    pub id: String,
    pub name: String,
    /// Optional free-text description. Same role as on `Category`.
    pub description: Option<String>,
    pub category_type: CategoryType,
    pub children: Vec<CategoryNode>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateCategoryPayload {
    pub name: String,
    pub parent_id: Option<String>,
    pub display_order: Option<i32>,
    pub description: Option<String>,
    /// Required: every category must be classified explicitly on creation.
    pub category_type: CategoryType,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PatchCategoryPayload {
    pub name: Option<String>,
    pub parent_id: Option<String>,
    pub display_order: Option<i32>,
    pub is_active: Option<bool>,
    pub description: Option<String>,
    /// Omitted = leave unchanged (standard PATCH semantics).
    pub category_type: Option<CategoryType>,
}

/// Semantic classification of a category, used to drive filtering and the
/// income/spending/transfer logic across budgets, charts and the portfolio
/// card. Every category has exactly one;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
#[serde(rename_all = "snake_case")]
pub enum CategoryType {
    Spending,
    IncomeTaxable,
    IncomeNonTaxable,
    InterestTaxable,
    InterestNonTaxable,
    InternalTransfer,
    DonationTaxable,
    DonationNonTaxable,
}

impl CategoryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Spending => "spending",
            Self::IncomeTaxable => "income_taxable",
            Self::IncomeNonTaxable => "income_non_taxable",
            Self::InterestTaxable => "interest_taxable",
            Self::InterestNonTaxable => "interest_non_taxable",
            Self::InternalTransfer => "internal_transfer",
            Self::DonationTaxable => "donation_taxable",
            Self::DonationNonTaxable => "donation_non_taxable",
        }
    }

    /// Human-facing label (single source for the FE dropdowns/legends).
    pub fn label(&self) -> &'static str {
        match self {
            Self::Spending => "Spending",
            Self::IncomeTaxable => "Income (taxable)",
            Self::IncomeNonTaxable => "Income (non-taxable)",
            Self::InterestTaxable => "Interest (taxable)",
            Self::InterestNonTaxable => "Interest (non-taxable)",
            Self::InternalTransfer => "Internal transfer",
            Self::DonationTaxable => "Donation (taxable)",
            Self::DonationNonTaxable => "Donation (non-taxable)",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "spending" => Some(Self::Spending),
            "income_taxable" => Some(Self::IncomeTaxable),
            "income_non_taxable" => Some(Self::IncomeNonTaxable),
            "interest_taxable" => Some(Self::InterestTaxable),
            "interest_non_taxable" => Some(Self::InterestNonTaxable),
            "internal_transfer" => Some(Self::InternalTransfer),
            "donation_taxable" => Some(Self::DonationTaxable),
            "donation_non_taxable" => Some(Self::DonationNonTaxable),
            _ => None,
        }
    }

    /// All variants in declaration order (drives the generated TS binding set).
    pub const ALL: &'static [CategoryType] = &[
        Self::Spending,
        Self::IncomeTaxable,
        Self::IncomeNonTaxable,
        Self::InterestTaxable,
        Self::InterestNonTaxable,
        Self::InternalTransfer,
        Self::DonationTaxable,
        Self::DonationNonTaxable,
    ];

    /// Types summed into the portfolio card's "Income" headline.
    pub const INCOME: &'static [CategoryType] = &[Self::IncomeTaxable, Self::IncomeNonTaxable];

    /// Types summed into the portfolio card's "Spending" headline (outflows + donations).
    pub const SPENDING: &'static [CategoryType] = &[
        Self::Spending,
        Self::DonationTaxable,
        Self::DonationNonTaxable,
    ];

    /// Always excluded from charts and the portfolio cash card.
    pub const CHART_EXCLUDED: &'static [CategoryType] = &[Self::InternalTransfer];
}

#[cfg(test)]
mod category_type_groups_export {
    use super::CategoryType;

    fn ts_list(name: &str, items: &[CategoryType]) -> String {
        let body = items
            .iter()
            .map(|c| format!("\"{}\"", c.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        format!("export const {name}: readonly CategoryType[] = [{body}]\n")
    }

    /// Regenerates `frontend/src/bindings/category_type_groups.ts` from the
    /// Rust `CategoryType` constants so the frontend never hand-maintains the
    /// income/spending/excluded sets or the display labels. Runs with the other
    /// ts-rs exports under `cargo test`; the output must be committed.
    #[test]
    fn export_category_type_groups() {
        let mut out = String::new();
        out.push_str("// AUTO-GENERATED from backend CategoryType (cargo test). Do not edit.\n");
        out.push_str("import type { CategoryType } from \"./CategoryType\"\n\n");
        out.push_str(&ts_list("ALL_CATEGORY_TYPES", CategoryType::ALL));
        out.push_str(&ts_list("INCOME_TYPES", CategoryType::INCOME));
        out.push_str(&ts_list("SPENDING_TYPES", CategoryType::SPENDING));
        out.push_str(&ts_list(
            "CHART_EXCLUDED_TYPES",
            CategoryType::CHART_EXCLUDED,
        ));
        out.push_str("\nexport const CATEGORY_TYPE_LABELS: Record<CategoryType, string> = {\n");
        for c in CategoryType::ALL {
            out.push_str(&format!("  {}: \"{}\",\n", c.as_str(), c.label()));
        }
        out.push_str("}\n");

        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../frontend/src/bindings/category_type_groups.ts"
        );
        std::fs::write(path, out).expect("write category_type_groups.ts");
    }
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
    /// JSON array of profile IDs, e.g. `["alex", "sam"]`. Required: there is no
    /// implicit default profile, so this must name at least one existing profile.
    pub profile_ids: Vec<String>,
    /// Set in portfolio responses to indicate whether the carried-forward
    /// balance is stale (snapshot > 45 days before the query date).
    /// Absent (`None`) in non-portfolio contexts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_stale: Option<bool>,
    /// True for account types that count toward available (liquid) wealth.
    /// Derived from account_type at query time; never stored in the DB.
    #[serde(default)]
    pub is_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
#[serde(rename_all = "snake_case")]
pub enum AccountType {
    Checking,
    Savings,
    EmergencyFund,
    Investment,
    InvestmentIsa,
    Credit,
    Pension,
    Property,
}

impl AccountType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Checking => "checking",
            Self::Savings => "savings",
            Self::EmergencyFund => "emergency_fund",
            Self::Investment => "investment",
            Self::InvestmentIsa => "investment_isa",
            Self::Credit => "credit",
            Self::Pension => "pension",
            Self::Property => "property",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "checking" => Some(Self::Checking),
            "savings" => Some(Self::Savings),
            "emergency_fund" => Some(Self::EmergencyFund),
            "investment" => Some(Self::Investment),
            "investment_isa" => Some(Self::InvestmentIsa),
            "credit" => Some(Self::Credit),
            "pension" => Some(Self::Pension),
            "property" => Some(Self::Property),
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

/// Dimension the spending grid groups rows by. The spreadsheet uses
/// `LeafCategory` (per-leaf rows it can budget on); charts offer all four.
/// Not exposed to TS as a type (the frontend just sends the string param).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpendingGroupBy {
    #[default]
    LeafCategory,
    ParentCategory,
    CategoryType,
    Account,
}

impl SpendingGroupBy {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "leaf_category" | "subcategory" | "leaf" => Some(Self::LeafCategory),
            "parent_category" | "category" | "parent" => Some(Self::ParentCategory),
            "category_type" | "type" => Some(Self::CategoryType),
            "account" => Some(Self::Account),
            _ => None,
        }
    }
}

/// One row in the spending-grid response. `periods` maps period strings
/// (e.g. "2026-01", "2026-Q1", "2026") to the spending total as a Decimal
/// string, or null if there were no transactions in that period.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct SpendingGridRow {
    /// FK to categories.id (leaf). Set only when grouping by leaf category;
    /// display name resolved client-side. Budget editing keys on this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
    /// Parent category id of `category_id` (leaf grouping only). Lets the
    /// frontend group/order the spreadsheet by parent without string-splitting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// The grouping dimension's value when grouping by parent/type/account
    /// (the parent id, the category_type string, or the account id). Charts read
    /// `group_key ?? category_id` as the series key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_key: Option<String>,
    /// Period key -> decimal string (or null). Amounts are signed:
    /// negative = expense, positive = income/credit.
    #[ts(type = "Record<string, string | null>")]
    pub periods: HashMap<String, Option<String>>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    #[ts(type = "Record<string, DisplayCurrency>")]
    pub periods_display: HashMap<String, DisplayCurrency>,
    /// Average spend per period that had any transactions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_display: Option<DisplayCurrency>,
    /// Standing budget amount for this category (decimal string).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<String>,
    /// Sum across all periods.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_display: Option<DisplayCurrency>,
}

/// One row in the `GET /api/budget/:month` response.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct BudgetRow {
    /// FK to categories.id (leaf). Display name resolved client-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
    /// Effective budget for this month (standing or override). Null if not set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budgeted: Option<String>,
    /// Actual spend this month (absolute value of negative transactions).
    pub actual: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_display: Option<DisplayCurrency>,
    /// `actual / budgeted * 100`. Null when `budgeted` is null or zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
}

/// Aggregate spend per category, used by `GET /api/transactions/by-category`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct CategoryTotal {
    /// FK to categories.id (leaf), or null for uncategorized. Display name
    /// resolved client-side.
    pub category_id: Option<String>,
    /// When `direction` is unset the total is the signed net sum
    /// (negative = net spend). When `direction` is `outflow` or `income`
    /// the total is the sum of absolute values of matching transactions.
    pub total: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_currency: Option<DisplayCurrency>,
}

/// A currency in use in the app. Returned by GET /api/currencies.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct Currency {
    pub code: String,
    pub is_preferred: bool,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub fx_rate: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Attached to aggregated monetary values when all source amounts share the
/// same non-preferred currency. Lets the frontend show the original-currency
/// value alongside the converted preferred-currency value.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct DisplayCurrency {
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub value: Decimal,
    pub currency: String,
}

/// Request body for POST /api/currencies.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateCurrencyPayload {
    pub code: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub fx_rate: Decimal,
}

/// Request body for PATCH /api/currencies/:code.
#[derive(Debug, Clone, Deserialize)]
pub struct PatchCurrencyPayload {
    #[serde(with = "rust_decimal::serde::str_option", default)]
    pub fx_rate: Option<Decimal>,
    #[serde(default)]
    pub is_preferred: Option<bool>,
}

/// Cash-flow direction filter for aggregation endpoints.
/// `Outflow` sums absolute values of negative amounts (spending).
/// `Income` sums absolute values of positive amounts (credits).
/// When no direction is supplied the caller gets signed net sums.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum TransactionDirection {
    Outflow,
    Income,
}

impl TransactionDirection {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "outflow" => Some(Self::Outflow),
            "income" => Some(Self::Income),
            _ => None,
        }
    }
}

/// A standing monthly budget target for one category.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct StandingBudget {
    /// FK to categories.id (leaf)
    pub category_id: Option<String>,
    pub amount: String,
}

/// Per-month override for a standing budget.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct BudgetOverride {
    pub month: String,
    pub category_id: String,
    pub amount: String,
}

/// Input record for `POST /api/import` (structured JSON from external agents).
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
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
    /// FK to categories.id (leaf node)
    #[serde(default)]
    pub category_id: Option<String>,
    #[serde(default)]
    pub category_source: Option<CategorySource>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub is_recurring: Option<bool>,
    #[serde(default)]
    pub exclude_from_summary: Option<bool>,
    /// IDs of the source documents this row was extracted from (`documents.id`).
    /// The parse endpoint fills these in per row; manual rows leave it empty.
    #[serde(default)]
    pub source_document_ids: Vec<String>,
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

/// Point-in-time account balance derived from SUM(holdings.value).
/// Used by `GET /api/portfolio/balances` (non-summary mode).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct AccountSnapshot {
    #[serde(with = "serde_naive_datetime")]
    #[ts(type = "string")]
    pub as_of: NaiveDateTime,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_account: Option<String>,
    #[serde(default)]
    pub is_closed: bool,
    /// Import-time provenance: `true` when this snapshot was computed/inferred
    /// from other data (e.g. interpolated from transactions or neighbouring
    /// period balances), `false` when read directly from the source document.
    /// Not persisted to SQLite; surfaces in the import preview only and
    /// defaults to `false` when round-tripped through the API or read back.
    #[serde(default)]
    pub derived: bool,
    /// IDs of the source documents this snapshot was extracted from
    /// (`documents.id`). Persisted. Empty for manual / API imports.
    #[serde(default)]
    pub source_document_ids: Vec<String>,
    /// Transient: the filename this row was attributed to during a parse. Used
    /// only to resolve `source_document_ids` once documents are stored; never
    /// persisted or sent to clients.
    #[serde(default, skip_serializing)]
    #[ts(skip)]
    pub source_file: Option<String>,
}

/// Internal row type used only by `get_holdings_for_summary` -- not serialized.
#[derive(Debug, Clone)]
pub struct HoldingSummaryRow {
    pub holding: Holding,
    pub account_type: AccountType,
    pub institution: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct HoldingPreview {
    pub account_id: String,
    pub symbol: String,
    pub sub_account: Option<String>,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub value: Decimal,
    pub currency: String,
    pub as_of: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_value: Option<String>,
    /// True when the underlying snapshot was computed/inferred rather than
    /// read directly from the source document. Drives the "Source" badge in
    /// the import preview. Always `false` for previews that did not come from
    /// the LLM extraction pipeline (e.g. the user-driven /api/holdings preview).
    #[serde(default)]
    pub derived: bool,
    /// IDs of the source documents this snapshot was extracted from (resolve
    /// names via `IngestionPreview.documents`).
    #[serde(default)]
    pub source_document_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct HoldingsImportPayload {
    pub account_id: String,
    pub holdings: Vec<Holding>,
}

/// One holding in a write request. The amount is a presence-discriminated union:
/// supply either `value` (scalar: cash, property, loan, credit) OR both
/// `quantity` and `price_per_unit` (computed: stock, etf, fund, bond, crypto),
/// never both. The backend derives `value = quantity * price_per_unit` for the
/// computed arm. The response `Holding` stays flat so reads never branch.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct HoldingWrite {
    pub symbol: String,
    pub name: String,
    pub holding_type: HoldingType,
    pub currency: String,
    #[serde(with = "serde_naive_datetime")]
    #[ts(type = "string")]
    pub as_of: NaiveDateTime,
    #[serde(default)]
    pub sub_account: Option<String>,
    #[serde(default)]
    pub is_closed: bool,
    #[serde(default)]
    pub source_document_ids: Vec<String>,
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[ts(type = "string | null")]
    pub value: Option<Decimal>,
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[ts(type = "string | null")]
    pub quantity: Option<Decimal>,
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[ts(type = "string | null")]
    pub price_per_unit: Option<Decimal>,
}

impl HoldingWrite {
    /// Validate the amount union and flatten into a storable [`Holding`].
    /// Errors when both arms are supplied, neither is, or the computed arm is
    /// missing one of its fields.
    pub fn into_holding(self, account_id: &str) -> Result<Holding, String> {
        let has_value = self.value.is_some();
        let has_quantity = self.quantity.is_some();
        let has_price = self.price_per_unit.is_some();

        let (quantity, price_per_unit, value) = if has_quantity || has_price {
            if has_value {
                return Err(
                    "holding must set either `value` or `quantity`+`price_per_unit`, not both"
                        .to_string(),
                );
            }
            let quantity = self
                .quantity
                .ok_or_else(|| "computed holding requires `quantity`".to_string())?;
            let price = self
                .price_per_unit
                .ok_or_else(|| "computed holding requires `price_per_unit`".to_string())?;
            (quantity, Some(price), quantity * price)
        } else if has_value {
            (Decimal::ZERO, None, self.value.unwrap())
        } else {
            return Err(
                "holding requires either `value` or both `quantity` and `price_per_unit`"
                    .to_string(),
            );
        };

        // Sub-unit conversion (GBX/USX/ZAC/ILA -> parent currency) happens here,
        // the single flatten point every holdings write path (POST /import,
        // POST /:account_id, and their shared dry-run preview) goes through.
        // After this point no sub-unit code is ever persisted. `value` and
        // `price_per_unit` are both monetary amounts in the sub-unit currency
        // and scale identically under division; `quantity` (a share count) is
        // never touched.
        let (value, price_per_unit, currency) =
            match crate::util::subunits::to_parent(value, &self.currency) {
                Some((converted_value, parent)) => {
                    let converted_price = price_per_unit.map(|p| {
                        crate::util::subunits::to_parent(p, &self.currency)
                            .map(|(v, _)| v)
                            .unwrap_or(p)
                    });
                    (converted_value, converted_price, parent.to_string())
                }
                None => (value, price_per_unit, self.currency),
            };

        Ok(Holding {
            account_id: account_id.to_string(),
            symbol: self.symbol,
            name: self.name,
            holding_type: self.holding_type,
            quantity,
            price_per_unit,
            value,
            currency,
            as_of: self.as_of,
            short_name: None,
            sub_account: self.sub_account,
            is_closed: self.is_closed,
            derived: false,
            source_document_ids: self.source_document_ids,
            source_file: None,
        })
    }
}

/// Request body for the holdings write endpoints (`POST /api/holdings/import`
/// and `POST /api/holdings/:account_id`). See [`HoldingWrite`] for the union.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct HoldingsWritePayload {
    pub account_id: String,
    pub holdings: Vec<HoldingWrite>,
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
    Property,
    Debt,
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
            Self::Property => "property",
            Self::Debt => "debt",
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
            "property" => Some(Self::Property),
            "debt" => Some(Self::Debt),
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

/// Full holdings summary returned by `GET /api/holdings/summary`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct HoldingsSummaryResponse {
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub net_worth: Decimal,
    pub preferred_currency: String,
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

/// Broad asset class used for portfolio grouping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
#[serde(rename_all = "PascalCase")]
pub enum AssetClass {
    Investments,
    Pension,
    Property,
    Cash,
}

impl AssetClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Investments => "Investments",
            Self::Pension => "Pension",
            Self::Property => "Property",
            Self::Cash => "Cash",
        }
    }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_currency: Option<DisplayCurrency>,
}

/// One row in the `GET /api/holdings/history` response.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct HoldingsHistoryRow {
    /// Period label: "YYYY-MM" for monthly, "YYYY-Qn" for quarterly, "YYYY" for yearly.
    pub month: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub available_wealth: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_wealth_display: Option<DisplayCurrency>,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub unavailable_wealth: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_wealth_display: Option<DisplayCurrency>,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub total_wealth: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_wealth_display: Option<DisplayCurrency>,
}

/// One period in the `GET /api/investments/history` response. Both values are
/// `null` where the period has no underlying data (before the first
/// contribution event, or no active holdings) so the chart shows a gap rather
/// than a phantom zero. Values are decimal strings in the preferred currency.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct InvestmentHistoryRow {
    /// Period label: "YYYY-MM" / "YYYY-Qn" / "YYYY".
    pub period: String,
    /// Cumulative net contributions (buys/vests/transfers in, sells/withholds out, plus fees).
    pub net_invested: Option<String>,
    /// Market value of all active investment + ISA holdings.
    pub market_value: Option<String>,
}

/// Stable metadata for one holding line in an account's history chart.
/// Used by `GET /api/holdings/account-history`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct AccountHoldingSeries {
    pub symbol: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_name: Option<String>,
    pub holding_type: String,
}

/// Value of one holding at a period end, converted to the preferred currency.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct AccountHoldingValue {
    pub symbol: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub value: Decimal,
}

/// One period in the `GET /api/holdings/account-history` response: the account's
/// total value and the per-symbol breakdown at that period end (preferred currency).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct AccountHoldingHistoryRow {
    /// Period label: "YYYY-MM" for monthly, "YYYY-Qn" for quarterly, "YYYY" for yearly.
    pub period: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub total: Decimal,
    pub values: Vec<AccountHoldingValue>,
}

/// One row in the `GET /api/holdings/cash-flow` response.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct HoldingsCashFlowMonth {
    /// Period label: "YYYY-MM", "YYYY-Qn", or "YYYY".
    pub month: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub income: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub income_display: Option<DisplayCurrency>,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub spending: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spending_display: Option<DisplayCurrency>,
}

/// Start/end balance delta for one account. Used by `GET /api/portfolio/balances?summary=true`.
/// Balance is computed as SUM(holdings.value) per account.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct BalanceDelta {
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

/// Category-type-driven cash summary for the portfolio "Income, Spending &
/// Investments" card. Returned by `GET /api/budget/cash-summary`, range-aware.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct CashSummaryResponse {
    pub preferred_currency: String,
    /// Sum of income_taxable + income_non_taxable transactions in range.
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub income: Decimal,
    /// Sum of |amount| for spending + donation_taxable + donation_non_taxable.
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub spending: Decimal,
    /// Period-over-period growth of `savings`-type holdings.
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub savings_growth: Decimal,
    /// Sum of `buy` investment events in range.
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub new_cash_invested: Decimal,
    /// Range-aware investment performance (total/market growth, start/end value).
    pub investment_metrics: InvestmentMetrics,
}

// ── Investments ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum InvestmentEventType {
    Vest,
    Buy,
    Sell,
    Transfer,
    Withhold,
    Split,
}

impl InvestmentEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Vest => "vest",
            Self::Buy => "buy",
            Self::Sell => "sell",
            Self::Transfer => "transfer",
            Self::Withhold => "withhold",
            Self::Split => "split",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "vest" => Some(Self::Vest),
            "buy" => Some(Self::Buy),
            "sell" => Some(Self::Sell),
            "transfer" => Some(Self::Transfer),
            "withhold" => Some(Self::Withhold),
            "split" => Some(Self::Split),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct InvestmentEvent {
    pub id: String,
    pub account_id: String,
    pub event_type: InvestmentEventType,
    pub symbol: String,
    #[serde(with = "serde_naive_datetime")]
    #[ts(type = "string")]
    pub date: NaiveDateTime,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub quantity: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub price_per_share: Decimal,
    #[serde(with = "rust_decimal::serde::str_option", default)]
    #[ts(type = "string | null")]
    pub fee: Option<Decimal>,
    pub currency: String,
    /// ISO 4217 currency of the fee when it differs from `currency`. `None` means
    /// the fee is in the trade currency (or there is no fee).
    #[serde(default)]
    pub fee_currency: Option<String>,
    pub notes: Option<String>,
    pub fingerprint: String,
    #[serde(with = "serde_naive_datetime")]
    #[ts(type = "string")]
    pub created_at: NaiveDateTime,
    /// IDs of the source documents this event was extracted from
    /// (`documents.id`). Empty for manual / API imports.
    #[serde(default)]
    pub source_document_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct CreateInvestmentEventBody {
    pub account_id: String,
    pub event_type: String,
    pub symbol: String,
    pub date: String,
    pub quantity: String,
    pub price_per_share: String,
    pub fee: Option<String>,
    pub currency: String,
    /// ISO 4217 currency of the fee when it differs from `currency`. Omit / `None`
    /// to charge the fee in the trade currency; the server defaults it to
    /// `currency` whenever a fee is present.
    #[serde(default)]
    pub fee_currency: Option<String>,
    pub notes: Option<String>,
    /// IDs of the source documents this row was extracted from (`documents.id`).
    /// The parse endpoint fills these in per row; manual rows leave it empty.
    #[serde(default)]
    pub source_document_ids: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct PatchInvestmentEventBody {
    pub event_type: Option<String>,
    pub symbol: Option<String>,
    pub date: Option<String>,
    pub quantity: Option<String>,
    pub price_per_share: Option<String>,
    pub fee: Option<String>,
    pub currency: Option<String>,
    #[serde(default)]
    pub fee_currency: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListInvestmentEventsQuery {
    pub account_id: Option<String>,
    pub symbol: Option<String>,
    pub event_type: Option<String>,
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

/// Status of a single row in a dryrun preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum TransactionPreviewStatus {
    New,
    Duplicate,
    Error,
}

/// A single row in a transaction dryrun preview.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct TransactionPreviewRow {
    pub index: usize,
    #[serde(with = "serde_naive_datetime")]
    #[ts(type = "string")]
    pub date: NaiveDateTime,
    pub description: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub amount: Decimal,
    pub currency: String,
    pub status: TransactionPreviewStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_reason: Option<String>,
    /// FK to categories.id (leaf). Frontend resolves the display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
    /// LLM confidence in the category assignment, [0.0, 1.0]. Unified mode only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_confidence: Option<f32>,
    /// IDs of the source documents this row was extracted from (resolve names
    /// via `IngestionPreview.documents`). Empty for the user-driven JSON dryrun.
    #[serde(default)]
    pub source_document_ids: Vec<String>,
}

/// Full response from a transaction import dryrun.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct TransactionImportPreview {
    pub account_id: String,
    pub filename: String,
    pub detected_bank: BankFormat,
    pub detection_confidence: f32,
    pub rows: Vec<TransactionPreviewRow>,
    pub rows_total: u64,
    pub rows_new: u64,
    pub rows_duplicate: u64,
    pub rows_error: u64,
    /// Parsed payload ready for confirmation via `POST /api/import`.
    pub payload: ImportPayload,
}

// ── Parse cost / agent selection ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Haiku,
    Sonnet,
    Opus,
}

/// Which Anthropic credential the parse pipeline should use. The actual model,
/// prompts, tools, and output are identical across all variants; only the
/// authentication (and therefore billing) differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
#[serde(rename_all = "snake_case")]
pub enum AuthSource {
    /// Prefer the Claude subscription OAuth token when configured, otherwise
    /// fall back to the Console API key. This is the default.
    #[default]
    Auto,
    /// Force the Claude subscription OAuth token (`FYNANCE_CLAUDE_CODE_OAUTH_TOKEN`).
    Subscription,
    /// Force the Console API key (`FYNANCE_ANTHROPIC_API_KEY`).
    ApiKey,
}

/// Cost of a single LLM call made during a parse run.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct ParserCallCost {
    /// Parser id, e.g. "csv_transactions", "pdf_holdings", "unified".
    pub parser: String,
    pub agent: Agent,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub duration_ms: u64,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub amount: Decimal,
    /// Always "USD". Frontend converts for display.
    pub currency: String,
}

/// Per-call breakdown plus total for a parse run.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct EstimatedPrice {
    pub calls: Vec<ParserCallCost>,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub total: Decimal,
    pub currency: String,
}

impl Default for EstimatedPrice {
    fn default() -> Self {
        Self {
            calls: Vec::new(),
            total: Decimal::ZERO,
            currency: "USD".to_string(),
        }
    }
}

// ── Ingestion (2-stage import redesign) ──────────────────────────────────────

/// Top-level response from `POST /api/parse` (Stage 1).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct IngestionPreview {
    pub status: IngestionStatus,
    pub metadata: IngestionMetadata,
    pub transactions: TransactionIngestionResult,
    pub holdings: HoldingsIngestionResult,
    pub investments: InvestmentIngestionResult,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clarifications_needed: Vec<ClarificationRequest>,
    /// Documents stored for this parse, in upload order. Rows reference these
    /// by id via their `source_document_ids`; the frontend renders a numbered
    /// "Source" chip per id and resolves the name/date from this list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub documents: Vec<DocumentSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
#[serde(rename_all = "snake_case")]
pub enum IngestionStatus {
    Success,
    NeedsClarification,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct IngestionMetadata {
    pub files_processed: usize,
    pub institution_detected: Option<String>,
    pub detection_confidence: f32,
    pub processing_time_ms: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relationships_found: Vec<String>,
    #[serde(default)]
    pub estimated_price: EstimatedPrice,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct TransactionIngestionResult {
    pub count: usize,
    pub new: usize,
    pub duplicate: usize,
    pub errors: usize,
    pub rows: Vec<TransactionPreviewRow>,
    pub payload: Option<ImportPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct HoldingsIngestionResult {
    pub count: usize,
    pub new: usize,
    pub modify: usize,
    pub rows: Vec<HoldingPreview>,
    pub payload: Option<HoldingsImportPayload>,
    /// Latest open snapshot per distinct (symbol, sub_account) already on this
    /// account in the DB. Drives the Holdings preview's symbol picker
    /// (autocomplete vs free-text) and the "vs prev" diff column on each row.
    /// Empty when the account has no holdings yet.
    #[serde(default)]
    pub known_holdings: Vec<KnownHolding>,
}

/// Compact summary of an account's most recent open snapshot for a given
/// `(symbol, sub_account)`. Sent on the holdings preview response so the
/// frontend can offer existing symbols as suggestions and show diff-from-prev
/// without a second round-trip.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct KnownHolding {
    pub symbol: String,
    pub name: String,
    pub holding_type: HoldingType,
    pub currency: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_account: Option<String>,
    /// Last value for this holding as a decimal string.
    pub last_value: String,
    /// ISO date of the last snapshot (`YYYY-MM-DD`).
    pub last_as_of: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct InvestmentIngestionResult {
    pub count: usize,
    pub new: usize,
    pub duplicate: usize,
    pub rows: Vec<InvestmentPreviewRow>,
    pub payload: Option<InvestmentsImportPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct InvestmentPreviewRow {
    pub index: usize,
    pub event_type: String,
    pub symbol: String,
    pub date: String,
    pub quantity: String,
    pub price_per_share: String,
    pub currency: String,
    pub status: TransactionPreviewStatus,
    /// Why this row was rejected, when `status` is `Error` (e.g. low confidence,
    /// invalid date/quantity/price). `None` for new/duplicate rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_id: Option<String>,
    /// IDs of the source documents this row was extracted from (resolve names
    /// via `IngestionPreview.documents`).
    #[serde(default)]
    pub source_document_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct InvestmentsImportPayload {
    pub account_id: String,
    pub events: Vec<CreateInvestmentEventBody>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct InvestmentImportResult {
    pub total: usize,
    pub inserted: usize,
    pub duplicates: usize,
    pub errors: Vec<InvestmentImportError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct InvestmentImportError {
    pub index: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct ClarificationRequest {
    pub file: String,
    pub question: String,
    pub suggestions: Vec<String>,
}

// ── Documents (source-file storage & provenance) ─────────────────────────────

/// Full metadata for a stored source document. Internal: the API returns
/// [`DocumentSummary`], which adds the computed reference info. `file_path`
/// is a local absolute path and is never serialized to clients.
#[derive(Debug, Clone)]
pub struct Document {
    pub id: String,
    pub filename: String,
    pub file_path: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub content_hash: String,
    /// `"parse"` (auto-stored during a parse) or `"manual"` (standalone upload).
    pub origin: String,
    pub account_id: Option<String>,
    pub uploaded_at: String,
}

/// API view of a stored document: metadata plus how many rows reference it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct DocumentSummary {
    pub id: String,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: usize,
    pub origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    pub uploaded_at: String,
    /// Total rows across transactions + holdings + investments linking this doc.
    /// `null` in the list view: it's not computed there (too slow over the whole
    /// dataset). Fetch the real count via `GET /api/documents/:id`.
    pub reference_count: Option<usize>,
    /// True when zero rows reference this doc, regardless of `origin`. Computed
    /// cheaply in both the list and single-doc views.
    pub orphaned: bool,
}

/// Per-entity reference counts for one document. Used in the delete-conflict
/// body and the force-delete unlink summary.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct DocumentReferences {
    pub transactions: usize,
    pub holdings: usize,
    pub investments: usize,
}

impl DocumentReferences {
    pub fn total(&self) -> usize {
        self.transactions + self.holdings + self.investments
    }
}

/// Response from a successful `DELETE /api/documents/:id` (force or unreferenced).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct DocumentDeleteResult {
    pub deleted: bool,
    pub unlinked: DocumentReferences,
}
