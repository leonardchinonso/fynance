//! Unified statement row schema.
//!
//! `UnifiedStatementRow` is the single target struct that the LLM parser
//! produces. It is a union of every field Monzo, Revolut, Lloyds, and
//! "reasonable unknown UK bank" might expose, with `Option<T>` for anything
//! that is not guaranteed. All three bank dialects that Phase 1 previously
//! handled with separate column-index mappers now converge here.
//!
//! `Transaction::from_unified` converts one row into the storage-ready
//! `Transaction` type.

use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::model::{CategorySource, Transaction};
use crate::util::{fingerprint, normalize_description, serde_naive_datetime};

/// One row of a bank statement after the LLM has normalised it.
///
/// `amount` follows the signed convention used everywhere in fynance:
/// negative = money out (debit), positive = money in (credit).
/// Fields not present in the source file are `None`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct UnifiedStatementRow {
    // -- Always required --
    #[serde(with = "serde_naive_datetime")]
    #[ts(type = "string")]
    pub date: NaiveDateTime,
    pub description: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub amount: Decimal,
    pub currency: String,

    // -- Optional, bank-dependent --
    /// Unique transaction ID from the bank (Monzo "Transaction ID", Lloyds ref).
    pub fitid: Option<String>,
    /// Spending category if the bank provides one (Monzo ships these).
    pub category: Option<String>,
    /// Merchant name when distinct from `description` (Monzo "Name" column).
    pub merchant: Option<String>,
    /// Counterparty name for peer-to-peer transfers (Revolut "Description").
    pub counterparty: Option<String>,
    /// Transaction type as the bank names it (e.g. Revolut "Type": CARD_PAYMENT).
    pub transaction_type: Option<String>,
    /// Running balance after this transaction, when the bank includes it.
    #[serde(with = "rust_decimal::serde::str_option", default)]
    #[ts(type = "string | null")]
    pub balance_after: Option<Decimal>,
    /// Any notes or tags on the transaction (Monzo "Notes and #tags").
    pub notes: Option<String>,
    /// Payment reference when present (Lloyds "Transaction Reference").
    pub reference: Option<String>,

    // -- Per-row confidence from the LLM --
    /// LLM's confidence that this row was correctly extracted [0.0, 1.0].
    pub row_confidence: f32,
}

impl Transaction {
    /// Build a `Transaction` from a normalised LLM statement row.
    ///
    /// The fingerprint is computed from `(datetime, amount, account_id)`.
    /// Description is excluded so that the same transaction imported via
    /// different channels (CSV vs agent) deduplicates correctly.
    pub fn from_unified(row: UnifiedStatementRow, account_id: &str) -> Self {
        let date_iso = row.date.format("%Y-%m-%dT%H:%M:%S").to_string();
        let amount_str = row.amount.to_string();

        // Prefer the explicit merchant field when present (it is more stable
        // than a generic description that may include payment-method noise).
        let description = row
            .merchant
            .filter(|m| !m.is_empty())
            .unwrap_or(row.description);

        let normalized = normalize_description(&description);
        let fp = fingerprint(&date_iso, &amount_str, account_id);

        let category = row.category.filter(|s| !s.is_empty());
        let category_source = category.as_ref().map(|_| CategorySource::Rule);

        Transaction {
            id: Uuid::new_v4().to_string(),
            date: row.date,
            description,
            normalized,
            amount: row.amount,
            currency: row.currency,
            account_id: account_id.to_string(),
            category,
            category_source,
            confidence: None,
            notes: row.notes,
            is_recurring: false,
            fingerprint: fp,
            fitid: row.fitid,
        }
    }
}
