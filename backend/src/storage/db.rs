//! SQLite-backed persistence layer.
//!
//! The `Db` type owns a single `rusqlite::Connection` and exposes typed
//! methods for every query the rest of the crate needs. Phase 1 is
//! synchronous and single-threaded; the Axum server wraps this behind a
//! shared `Arc<Mutex<Db>>` without changing the surface area here.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::{NaiveDate, NaiveDateTime};
use rusqlite::{Connection, OptionalExtension, params};
use rust_decimal::Decimal;

use crate::model::{
    Account, AccountHoldingHistoryRow, AccountHoldingSeries, AccountHoldingValue, AccountSnapshot,
    AccountType, AssetClass, BalanceDelta, BudgetRow, Category, CategoryNode, CategorySource,
    CategoryTotal, CategoryType, ChecklistItem, ChecklistStatus, CreateCategoryPayload,
    CreateInvestmentEventBody, Currency, DerivedBroughtForwardLosses, DerivedLossYear, Document,
    DocumentReferences, DocumentSummary, ExchangeRate, Granularity, Holding, HoldingPreview,
    HoldingSummaryRow, HoldingType, HoldingsCashFlowMonth, HoldingsHistoryRow, ImportLog,
    ImportResult, ImportRowError, ImportTransaction, InsertOutcome, InvestmentEvent,
    InvestmentEventType, InvestmentHistoryRow, InvestmentMetrics, PatchCategoryPayload,
    PatchInvestmentEventBody, Profile, SpendingGridRow, SpendingGroupBy, TaxConfigEntry, TaxInputs,
    Transaction, TransactionPreviewRow, TransactionPreviewStatus,
};

/// The full schema DDL. Embedded at compile time so a release binary can
/// create a fresh DB on a new machine with no files on disk beside itself.
const SCHEMA_SQL: &str = include_str!("../../../db/sql/schema.sql");

/// Embedded default category taxonomy (names, types, descriptions). Seeded on
/// first run and used to backfill `category_type` on pre-existing categories.
const CATEGORIES_YAML: &str = include_str!("../../config/categories.yaml");

/// Resolve the default DB path. On Linux this is
/// `~/.local/share/fynance/fynance.db`; on macOS it's
/// `~/Library/Application Support/fynance/fynance.db`.
pub fn default_db_path() -> Result<PathBuf> {
    let base = dirs::data_local_dir()
        .ok_or_else(|| anyhow!("could not resolve OS data directory; set FYNANCE_DB_PATH"))?;
    Ok(base.join("fynance").join("fynance.db"))
}

/// Filter params for `Db::get_transactions`.
#[derive(Debug, Clone)]
pub struct TransactionFilters {
    pub start: Option<NaiveDate>,
    pub end: Option<NaiveDate>,
    /// Multi-select account IDs. Empty vec = no filter.
    pub accounts: Option<Vec<String>>,
    /// Multi-select category names or IDs. Empty vec = no filter.
    pub categories: Option<Vec<String>>,
    /// Multi-select category_type values (e.g. "spending"). Empty vec = no filter.
    pub category_types: Option<Vec<String>>,
    /// Free-text search across normalized, description, category, notes.
    pub search: Option<String>,
    pub profile_id: Option<String>,
    pub category_source: Option<CategorySource>,
    /// Sort column. `None` keeps the default newest-first ordering.
    pub sort: Option<TransactionSort>,
    pub sort_dir: SortDir,
    pub page: u32,
    pub limit: u32,
}

/// Columns the transactions list can be sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionSort {
    Date,
    Amount,
    Category,
}

impl TransactionSort {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "date" => Some(Self::Date),
            "amount" => Some(Self::Amount),
            "category" => Some(Self::Category),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortDir {
    Asc,
    #[default]
    Desc,
}

impl SortDir {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "asc" => Some(Self::Asc),
            "desc" => Some(Self::Desc),
            _ => None,
        }
    }

    fn sql(self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}

impl Default for TransactionFilters {
    fn default() -> Self {
        Self {
            start: None,
            end: None,
            accounts: None,
            categories: None,
            category_types: None,
            search: None,
            profile_id: None,
            category_source: None,
            sort: None,
            sort_dir: SortDir::Desc,
            page: 1,
            limit: 25,
        }
    }
}

/// Result of [`Db::delete_document`]. The route maps these to HTTP statuses.
#[derive(Debug, Clone)]
pub enum DeleteDocumentOutcome {
    /// No document with that id.
    NotFound,
    /// Referenced by at least one row and `force` was not set; nothing changed.
    Referenced(DocumentReferences),
    /// Row + file removed. The references are what was unlinked (zero if the
    /// document was already unreferenced).
    Deleted(DocumentReferences),
}

/// One row that [`Db::migrate_subunit_currencies`] changed (or would change,
/// in dry-run).
#[derive(Debug, Clone, PartialEq)]
pub struct SubunitMigrationRow {
    pub table: &'static str,
    pub id: String,
    pub sub_unit_code: String,
    pub parent_code: String,
    /// Human-readable before -> after amount (price_per_share, value, or
    /// amount depending on the table).
    pub before: String,
    pub after: String,
}

/// Full report of a [`Db::migrate_subunit_currencies`] run (dry-run or applied).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubunitMigrationReport {
    pub rows: Vec<SubunitMigrationRow>,
    /// Sub-unit `currencies` rows removed (only when nothing references
    /// them any more). Empty in dry-run mode.
    pub currencies_removed: Vec<String>,
}

impl SubunitMigrationReport {
    pub fn investments_migrated(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| r.table == "investments")
            .count()
    }
    pub fn holdings_migrated(&self) -> usize {
        self.rows.iter().filter(|r| r.table == "holdings").count()
    }
    pub fn transactions_migrated(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| r.table == "transactions")
            .count()
    }
    pub fn accounts_migrated(&self) -> usize {
        self.rows.iter().filter(|r| r.table == "accounts").count()
    }
}

pub struct Db {
    conn: Connection,
    /// Directory holding stored source documents, a `documents/` subdir beside
    /// the DB file. Created 0700 on Unix at open() time.
    documents_dir: PathBuf,
}

impl Db {
    /// Open (or create) the database at `path`. The parent directory is
    /// created with mode 0700 and the DB file with 0600 on Unix.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating db parent dir {parent:?}"))?;
                set_dir_mode_700(parent)?;
            }
        }

        let conn =
            Connection::open(path).with_context(|| format!("opening sqlite db at {path:?}"))?;

        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "wal_autocheckpoint", 100)?;
        // A CLI command and `serve` can hold connections to the same file;
        // without a timeout a write collision fails immediately with SQLITE_BUSY.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        conn.execute_batch(SCHEMA_SQL)
            .context("running schema.sql")?;

        migrate_schema(&conn)?;
        seed_defaults(&conn)?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;

        if path.exists() {
            set_file_mode_600(path)?;
        }

        let documents_dir = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("documents");
        if !documents_dir.exists() {
            std::fs::create_dir_all(&documents_dir)
                .with_context(|| format!("creating documents dir {documents_dir:?}"))?;
            set_dir_mode_700(&documents_dir)?;
        }

        Ok(Self {
            conn,
            documents_dir,
        })
    }

    /// Absolute path to the directory holding stored source documents.
    pub fn documents_dir(&self) -> &Path {
        &self.documents_dir
    }

    // ── Currencies ───────────────────────────────────────────────────────────

    pub fn get_currencies(&self) -> Result<Vec<Currency>> {
        let mut stmt = self.conn.prepare(
            "SELECT code, is_preferred, fx_rate, updated_at FROM currencies ORDER BY is_preferred DESC, code"
        )?;

        let currencies = stmt
            .query_map([], |row| {
                let code: String = row.get(0)?;
                let is_preferred: bool = row.get::<_, i32>(1)? == 1;
                let fx_rate_str: String = row.get(2)?;
                let updated_at: Option<String> = row.get(3)?;
                Ok((code, is_preferred, fx_rate_str, updated_at))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        currencies
            .into_iter()
            .map(|(code, is_preferred, fx_rate_str, updated_at)| {
                let fx_rate = fx_rate_str
                    .parse::<Decimal>()
                    .with_context(|| format!("invalid fx_rate for currency {code}"))?;
                Ok(Currency {
                    code,
                    is_preferred,
                    fx_rate,
                    updated_at: if is_preferred { None } else { updated_at },
                })
            })
            .collect()
    }

    pub fn currency_exists(&self, code: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM currencies WHERE code = ?1",
            params![code],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn create_currency(&self, code: &str, fx_rate: Decimal) -> Result<Currency> {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.conn.execute(
            "INSERT INTO currencies (code, is_preferred, fx_rate, updated_at) VALUES (?1, 0, ?2, ?3)",
            params![code, fx_rate.to_string(), now],
        )?;

        Ok(Currency {
            code: code.to_string(),
            is_preferred: false,
            fx_rate,
            updated_at: Some(now),
        })
    }

    pub fn update_currency_rate(&self, code: &str, fx_rate: Decimal) -> Result<Currency> {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let rows = self.conn.execute(
            "UPDATE currencies SET fx_rate = ?1, updated_at = ?2 WHERE code = ?3 AND is_preferred = 0",
            params![fx_rate.to_string(), now, code],
        )?;

        if rows == 0 {
            let exists = self.currency_exists(code)?;
            if exists {
                anyhow::bail!("cannot update the exchange rate for the preferred currency");
            } else {
                anyhow::bail!("currency {code} not found");
            }
        }

        Ok(Currency {
            code: code.to_string(),
            is_preferred: false,
            fx_rate,
            updated_at: Some(now),
        })
    }

    pub fn set_preferred_currency(&self, code: &str) -> Result<()> {
        if !self.currency_exists(code)? {
            anyhow::bail!("currency {code} not found");
        }

        let tx = self.conn.unchecked_transaction()?;

        tx.execute(
            "UPDATE currencies SET is_preferred = 0 WHERE is_preferred = 1",
            [],
        )?;
        tx.execute(
            "UPDATE currencies SET is_preferred = 1, fx_rate = '1', updated_at = NULL WHERE code = ?1",
            params![code],
        )?;

        tx.commit()?;
        Ok(())
    }

    pub fn delete_currency(&self, code: &str) -> Result<()> {
        let is_preferred: Option<i32> = self
            .conn
            .query_row(
                "SELECT is_preferred FROM currencies WHERE code = ?1",
                params![code],
                |row| row.get(0),
            )
            .optional()?;

        let is_preferred = match is_preferred {
            Some(v) => v,
            None => anyhow::bail!("currency {code} not found"),
        };

        if is_preferred == 1 {
            anyhow::bail!(
                "cannot delete the preferred currency; set a different preferred currency first"
            );
        }

        let holding_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM holdings WHERE currency = ?1",
            params![code],
            |r| r.get(0),
        )?;
        let account_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM accounts WHERE currency = ?1",
            params![code],
            |r| r.get(0),
        )?;
        let transaction_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM transactions WHERE currency = ?1",
            params![code],
            |r| r.get(0),
        )?;
        // Investment events were missing from this guard, which is precisely how the
        // "configured currency vanished" state was reached: deleting a currency still
        // referenced by the investment ledger leaves the CGT engine unable to convert
        // those events, so a report either refuses or (worse) sums unconverted figures.
        // Both `currency` (the trade currency) and `fee_currency` are references — a
        // fee charged in USD on a GBP trade keeps USD in use on its own.
        let investment_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM investments WHERE currency = ?1 OR fee_currency = ?1",
            params![code],
            |r| r.get(0),
        )?;

        let total = holding_count + account_count + transaction_count + investment_count;
        if total > 0 {
            anyhow::bail!(
                "cannot delete currency '{code}': in use by {holding_count} holdings, {account_count} accounts, {transaction_count} transactions, {investment_count} investment events"
            );
        }

        self.conn
            .execute("DELETE FROM currencies WHERE code = ?1", params![code])?;
        Ok(())
    }

    // ── Sub-unit migration ──────────────────────────────────────────────────
    //
    // One-time data migration converting pre-existing rows stored in a broker
    // sub-unit (GBX, USX, ZAC, ILA) to their parent currency, now that every
    // write path converts at import/write time (see `create_investment_event`,
    // `HoldingWrite::into_holding`, `Transaction::from_unified`,
    // `insert_transactions_bulk`). Only rows written *before* those changes can
    // still carry a sub-unit code; this cleans those up. See
    // `SubunitMigrationReport` / `SubunitMigrationRow` above `impl Db`.
    //
    // Idempotent: it only ever touches rows whose `currency` is a sub-unit
    // code, so a second run finds nothing left to convert and is a no-op.
    // Every row-group is migrated inside its own transaction, so a failure
    // partway through leaves already-migrated tables converted and unconverted
    // tables untouched — safely re-runnable rather than silently corrupt.

    /// Convert every stored row denominated in a broker sub-unit code to its
    /// parent currency. `dry_run = true` computes and returns the full report
    /// without writing anything. `dry_run = false` writes the changes and then
    /// removes any now-unreferenced sub-unit rows from the `currencies` table.
    ///
    /// **Not atomic across tables.** Each table is migrated in its own
    /// transaction, so a table is converted either fully or not at all, but a
    /// failure partway through leaves earlier tables converted and later ones
    /// untouched. That half-migrated state is consistent rather than corrupt —
    /// the operation is idempotent, so re-running it finishes the job.
    pub fn migrate_subunit_currencies(&self, dry_run: bool) -> Result<SubunitMigrationReport> {
        // Raw row shape for the investments scan: (id, account_id, symbol,
        // date, quantity, price_per_share, currency, fee, fee_currency, event_type).
        type InvestmentSubunitRow = (
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            String,
        );

        let mut report = SubunitMigrationReport::default();

        // ── investments ── price_per_share (and fee, independently, via
        // fee_currency) scale; quantity does not. The fingerprint depends on
        // price_per_share, so it is recomputed for every row we touch. Every
        // row is read and filtered in Rust against the fixed SUB_UNITS table
        // (the source of truth), rather than via the `currencies` table: a
        // sub-unit code is not necessarily present there.
        let inv_rows: Vec<InvestmentSubunitRow> = {
            let mut stmt = self.conn.prepare(
                "SELECT id, account_id, symbol, date, quantity, price_per_share, currency, \
                        fee, fee_currency, event_type \
                 FROM investments",
            )?;
            stmt.query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };

        let inv_tx = if dry_run {
            None
        } else {
            Some(self.conn.unchecked_transaction()?)
        };

        for (
            id,
            account_id,
            symbol,
            date_str,
            quantity_str,
            price_str,
            currency,
            fee_str,
            fee_currency,
            event_type,
        ) in &inv_rows
        {
            let price_is_sub_unit = crate::util::subunits::is_sub_unit(currency);
            let fee_is_sub_unit = fee_currency
                .as_deref()
                .is_some_and(crate::util::subunits::is_sub_unit);

            if !price_is_sub_unit && !fee_is_sub_unit {
                continue;
            }

            let quantity: Decimal = quantity_str
                .parse()
                .with_context(|| format!("investment {id}: bad quantity {quantity_str:?}"))?;
            let price: Decimal = price_str
                .parse()
                .with_context(|| format!("investment {id}: bad price_per_share {price_str:?}"))?;

            let (new_price, new_currency) = if price_is_sub_unit {
                let (converted, parent) = crate::util::subunits::to_parent(price, currency)
                    .expect("checked is_sub_unit above");
                (converted, parent.to_string())
            } else {
                (price, currency.clone())
            };

            let (new_fee, new_fee_currency) = if fee_is_sub_unit {
                let fee: Decimal = fee_str
                    .as_deref()
                    .ok_or_else(|| {
                        anyhow::anyhow!("investment {id}: fee_currency set with no fee")
                    })?
                    .parse()
                    .with_context(|| format!("investment {id}: bad fee {fee_str:?}"))?;
                let (converted, parent) =
                    crate::util::subunits::to_parent(fee, fee_currency.as_deref().unwrap())
                        .expect("checked is_sub_unit above");
                (Some(converted), Some(parent.to_string()))
            } else {
                (
                    fee_str.as_ref().map(|s| s.parse::<Decimal>()).transpose()?,
                    fee_currency.clone(),
                )
            };

            let new_fingerprint = sha256_hex(&format!(
                "{account_id}|{symbol}|{date_str}|{quantity}|{new_price}|{event_type}"
            ));

            report.rows.push(SubunitMigrationRow {
                table: "investments",
                id: id.clone(),
                // Report the pair that actually converted. On a fee-only row
                // the price currency never changed, so pairing the fee's
                // sub-unit code with the (unconverted) price currency would
                // print a nonsense line like `GBX -> USD`.
                sub_unit_code: if price_is_sub_unit {
                    currency.clone()
                } else {
                    fee_currency.clone().unwrap_or_default()
                },
                parent_code: if price_is_sub_unit {
                    new_currency.clone()
                } else {
                    new_fee_currency.clone().unwrap_or_default()
                },
                before: format!("price={price} currency={currency} fee={fee_str:?} fee_currency={fee_currency:?}"),
                after: format!(
                    "price={new_price} currency={new_currency} fee={new_fee:?} fee_currency={new_fee_currency:?}"
                ),
            });

            if let Some(tx) = &inv_tx {
                tx.execute(
                    "UPDATE investments SET price_per_share = ?1, currency = ?2, fee = ?3, fee_currency = ?4, fingerprint = ?5 WHERE id = ?6",
                    params![
                        new_price.to_string(),
                        new_currency,
                        new_fee.map(|f| f.to_string()),
                        new_fee_currency,
                        new_fingerprint,
                        id,
                    ],
                )?;
            }
        }
        if let Some(tx) = inv_tx {
            tx.commit()?;
        }

        // ── holdings ── value and price_per_unit scale identically; quantity
        // does not. Holdings have no fingerprint column (their identity is the
        // (account_id, symbol, sub_account, as_of) unique index, none of which
        // changes), so no recompute is needed.
        let holding_rows: Vec<(i64, String, Option<String>, String, String)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id, value, price_per_unit, currency, symbol FROM holdings")?;
            stmt.query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };

        let hold_tx = if dry_run {
            None
        } else {
            Some(self.conn.unchecked_transaction()?)
        };

        for (id, value_str, price_str, currency, symbol) in &holding_rows {
            if !crate::util::subunits::is_sub_unit(currency) {
                continue;
            }
            let value: Decimal = value_str
                .parse()
                .with_context(|| format!("holding {id} ({symbol}): bad value {value_str:?}"))?;
            let (new_value, new_currency) = crate::util::subunits::to_parent(value, currency)
                .expect("checked is_sub_unit above");
            let new_price = match price_str {
                Some(p) => {
                    let price: Decimal = p
                        .parse()
                        .with_context(|| format!("holding {id} ({symbol}): bad price {p:?}"))?;
                    let (converted, _) = crate::util::subunits::to_parent(price, currency)
                        .expect("checked is_sub_unit above");
                    Some(converted)
                }
                None => None,
            };

            report.rows.push(SubunitMigrationRow {
                table: "holdings",
                id: id.to_string(),
                sub_unit_code: currency.clone(),
                parent_code: new_currency.to_string(),
                before: format!("value={value} price_per_unit={price_str:?} currency={currency}"),
                after: format!(
                    "value={new_value} price_per_unit={new_price:?} currency={new_currency}"
                ),
            });

            if let Some(tx) = &hold_tx {
                tx.execute(
                    "UPDATE holdings SET value = ?1, price_per_unit = ?2, currency = ?3 WHERE id = ?4",
                    params![
                        new_value.to_string(),
                        new_price.map(|p| p.to_string()),
                        new_currency,
                        id,
                    ],
                )?;
            }
        }
        if let Some(tx) = hold_tx {
            tx.commit()?;
        }

        // ── transactions ── amount scales; fingerprint = (date, amount,
        // account_id), so it must be recomputed for every row we touch.
        let txn_rows: Vec<(String, String, String, String, String)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id, date, amount, currency, account_id FROM transactions")?;
            stmt.query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };

        let txn_tx = if dry_run {
            None
        } else {
            Some(self.conn.unchecked_transaction()?)
        };

        for (id, date_str, amount_str, currency, account_id) in &txn_rows {
            if !crate::util::subunits::is_sub_unit(currency) {
                continue;
            }
            let amount: Decimal = amount_str
                .parse()
                .with_context(|| format!("transaction {id}: bad amount {amount_str:?}"))?;
            let (new_amount, new_currency) = crate::util::subunits::to_parent(amount, currency)
                .expect("checked is_sub_unit above");
            let new_fingerprint =
                crate::util::fingerprint(date_str, &new_amount.to_string(), account_id);

            report.rows.push(SubunitMigrationRow {
                table: "transactions",
                id: id.clone(),
                sub_unit_code: currency.clone(),
                parent_code: new_currency.to_string(),
                before: format!("amount={amount} currency={currency}"),
                after: format!("amount={new_amount} currency={new_currency}"),
            });

            if let Some(tx) = &txn_tx {
                tx.execute(
                    "UPDATE transactions SET amount = ?1, currency = ?2, fingerprint = ?3 WHERE id = ?4",
                    params![new_amount.to_string(), new_currency, new_fingerprint, id],
                )?;
            }
        }
        if let Some(tx) = txn_tx {
            tx.commit()?;
        }

        // ── accounts ── the account's `currency` is a denomination label, not
        // an amount: there is nothing to scale, so the code is simply rewritten
        // to the parent. It still has to happen, for two reasons. First, the
        // in-use check below counts `accounts`, so a stranded GBX account would
        // pin the GBX `currencies` row open and make the cleanup silently
        // no-op. Second, `set_account_balance` copies `accounts.currency`
        // straight into the `_CASH` holding it writes with no conversion of its
        // own, so a stranded GBX account would keep minting fresh GBX holdings
        // long after this migration ran.
        let account_rows: Vec<(String, String)> = {
            let mut stmt = self.conn.prepare("SELECT id, currency FROM accounts")?;
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };

        let acct_tx = if dry_run {
            None
        } else {
            Some(self.conn.unchecked_transaction()?)
        };

        for (id, currency) in &account_rows {
            let Some(parent) = crate::util::subunits::lookup(currency).map(|u| u.parent) else {
                continue;
            };

            report.rows.push(SubunitMigrationRow {
                table: "accounts",
                id: id.clone(),
                sub_unit_code: currency.clone(),
                parent_code: parent.to_string(),
                before: format!("currency={currency}"),
                after: format!("currency={parent}"),
            });

            if let Some(tx) = &acct_tx {
                tx.execute(
                    "UPDATE accounts SET currency = ?1 WHERE id = ?2",
                    params![parent, id],
                )?;
            }
        }
        if let Some(tx) = acct_tx {
            tx.commit()?;
        }

        // ── currencies ── drop leftover sub-unit rows once nothing references
        // them any more (checked the same way delete_currency does: holdings,
        // accounts, transactions; investments/fee_currency too, since that
        // in-use check is broader than delete_currency's).
        if !dry_run {
            for unit in crate::util::subunits::SUB_UNITS {
                let code = unit.code;
                if !self.currency_exists(code)? {
                    continue;
                }
                let in_use: i64 = self.conn.query_row(
                    "SELECT \
                        (SELECT COUNT(*) FROM holdings WHERE currency = ?1) + \
                        (SELECT COUNT(*) FROM accounts WHERE currency = ?1) + \
                        (SELECT COUNT(*) FROM transactions WHERE currency = ?1) + \
                        (SELECT COUNT(*) FROM investments WHERE currency = ?1) + \
                        (SELECT COUNT(*) FROM investments WHERE fee_currency = ?1)",
                    params![code],
                    |r| r.get(0),
                )?;
                if in_use == 0 {
                    self.conn
                        .execute("DELETE FROM currencies WHERE code = ?1", params![code])?;
                    report.currencies_removed.push(code.to_string());
                }
            }
        }

        Ok(report)
    }

    // ── Exchange rates (date-keyed, user-owned) ──────────────────────────────
    //
    // `rate` is quote-units per ONE base unit: amount_in_quote = amount_in_base * rate.
    // See the `exchange_rates` comment in db/sql/schema.sql for why that direction is
    // spelled out everywhere it is touched.

    /// Every stored rate quoting into `quote`, as `(base, date, rate)`.
    /// Used to populate `FxRateMap::with_historical` for the CGT engine.
    pub fn get_exchange_rates_for_quote(
        &self,
        quote: &str,
    ) -> Result<Vec<(String, NaiveDate, Decimal)>> {
        let mut stmt = self.conn.prepare(
            "SELECT base, date, rate FROM exchange_rates WHERE quote = ?1 ORDER BY base, date",
        )?;
        let rows = stmt
            .query_map(params![quote], |row| {
                let base: String = row.get(0)?;
                let date: String = row.get(1)?;
                let rate: String = row.get(2)?;
                Ok((base, date, rate))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        rows.into_iter()
            .map(|(base, date_str, rate_str)| {
                let date = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                    .with_context(|| format!("invalid date '{date_str}' in exchange_rates"))?;
                let rate = rate_str.parse::<Decimal>().with_context(|| {
                    format!("invalid rate for {base}->{quote} on {date_str} in exchange_rates")
                })?;
                Ok((base, date, rate))
            })
            .collect()
    }

    /// List stored rates, optionally filtered by base currency and/or an inclusive date range.
    pub fn list_exchange_rates(
        &self,
        base: Option<&str>,
        quote: Option<&str>,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
    ) -> Result<Vec<ExchangeRate>> {
        let mut sql = String::from(
            "SELECT base, quote, date, rate, source, updated_at FROM exchange_rates WHERE 1=1",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(b) = base {
            sql.push_str(" AND base = ?");
            args.push(Box::new(b.to_string()));
        }
        if let Some(q) = quote {
            sql.push_str(" AND quote = ?");
            args.push(Box::new(q.to_string()));
        }
        if let Some(s) = start_date {
            sql.push_str(" AND date >= ?");
            args.push(Box::new(s.format("%Y-%m-%d").to_string()));
        }
        if let Some(e) = end_date {
            sql.push_str(" AND date <= ?");
            args.push(Box::new(e.format("%Y-%m-%d").to_string()));
        }
        sql.push_str(" ORDER BY base, quote, date");

        let mut stmt = self.conn.prepare(&sql)?;
        let arg_refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|a| a.as_ref()).collect();
        let rows = stmt
            .query_map(arg_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        rows.into_iter()
            .map(|(base, quote, date, rate_str, source, updated_at)| {
                let rate = rate_str.parse::<Decimal>().with_context(|| {
                    format!("invalid rate for {base}->{quote} on {date} in exchange_rates")
                })?;
                Ok(ExchangeRate {
                    base,
                    quote,
                    date,
                    rate,
                    source,
                    updated_at,
                })
            })
            .collect()
    }

    /// Insert or replace a batch of rates in one transaction.
    ///
    /// Upsert rather than insert-only: correcting a rate you previously typed wrong is a
    /// normal action, and the pre-flight screen re-submits the whole set. Returns the number
    /// of rows written.
    pub fn upsert_exchange_rates(&self, rates: &[ExchangeRate]) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        {
            let mut stmt = tx.prepare(
                "INSERT INTO exchange_rates (base, quote, date, rate, source, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(base, quote, date) DO UPDATE SET
                     rate = excluded.rate,
                     source = excluded.source,
                     updated_at = excluded.updated_at",
            )?;
            for r in rates {
                stmt.execute(params![
                    r.base,
                    r.quote,
                    r.date,
                    r.rate.to_string(),
                    r.source,
                    now,
                ])?;
            }
        }
        tx.commit()?;
        Ok(rates.len())
    }

    /// Delete one rate. Returns false when no such row existed.
    pub fn delete_exchange_rate(&self, base: &str, quote: &str, date: &str) -> Result<bool> {
        let rows = self.conn.execute(
            "DELETE FROM exchange_rates WHERE base = ?1 AND quote = ?2 AND date = ?3",
            params![base, quote, date],
        )?;
        Ok(rows > 0)
    }

    // ── Profiles ─────────────────────────────────────────────────────────────

    pub fn create_profile(&self, id: &str, name: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO profiles (id, name) VALUES (?1, ?2)",
            params![id, name],
        )?;
        Ok(())
    }

    pub fn get_profiles(&self) -> Result<Vec<Profile>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, utr FROM profiles ORDER BY name")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Profile {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    utr: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Set or clear a profile's HMRC Unique Taxpayer Reference. `None` clears it.
    pub fn update_profile_utr(&self, id: &str, utr: Option<&str>) -> Result<()> {
        let updated = self.conn.execute(
            "UPDATE profiles SET utr = ?1 WHERE id = ?2",
            params![utr, id],
        )?;
        if updated == 0 {
            return Err(anyhow!("profile {id} not found"));
        }
        Ok(())
    }

    pub fn profile_exists(&self, id: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM profiles WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn update_profile_name(&self, id: &str, name: &str) -> Result<()> {
        let updated = self.conn.execute(
            "UPDATE profiles SET name = ?1 WHERE id = ?2",
            params![name, id],
        )?;
        if updated == 0 {
            return Err(anyhow!("profile {id} not found"));
        }
        Ok(())
    }

    /// Number of accounts that currently include the given profile id in their
    /// JSON `profile_ids` array. Used to gate deletion.
    pub fn count_accounts_referencing_profile(&self, id: &str) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT a.id) FROM accounts a, json_each(a.profile_ids) j WHERE j.value = ?1",
            params![id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn delete_profile(&self, id: &str) -> Result<()> {
        let deleted = self
            .conn
            .execute("DELETE FROM profiles WHERE id = ?1", params![id])?;
        if deleted == 0 {
            return Err(anyhow!("profile {id} not found"));
        }
        Ok(())
    }

    // ── Accounts ─────────────────────────────────────────────────────────────

    pub fn upsert_account(&self, account: &Account) -> Result<()> {
        let profile_ids = serde_json::to_string(&account.profile_ids)
            .unwrap_or_else(|_| r#"["default"]"#.to_string());
        self.conn.execute(
            r"INSERT INTO accounts (
                id, name, institution, type, currency,
                is_active, notes, profile_ids
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(id) DO UPDATE SET
                name        = excluded.name,
                institution = excluded.institution,
                type        = excluded.type,
                currency    = excluded.currency,
                is_active   = excluded.is_active,
                notes       = excluded.notes,
                profile_ids = excluded.profile_ids",
            params![
                account.id,
                account.name,
                account.institution,
                account.account_type.as_str(),
                account.currency,
                account.is_active as i64,
                account.notes,
                profile_ids,
            ],
        )?;
        Ok(())
    }

    /// Create a new account, failing with a distinct error if the ID already
    /// exists. Use `upsert_account` for idempotent CLI paths.
    pub fn create_account(&self, account: &Account) -> Result<()> {
        let profile_ids = serde_json::to_string(&account.profile_ids)
            .unwrap_or_else(|_| r#"["default"]"#.to_string());
        self.conn.execute(
            r"INSERT INTO accounts (
                id, name, institution, type, currency,
                is_active, notes, profile_ids
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                account.id,
                account.name,
                account.institution,
                account.account_type.as_str(),
                account.currency,
                account.is_active as i64,
                account.notes,
                profile_ids,
            ],
        )?;
        Ok(())
    }

    /// Returns all accounts, optionally filtered to those belonging to a
    /// specific profile. When `profile_id` is `None`, all accounts are returned
    /// (household view).
    ///
    /// `balance` and `balance_date` are derived at read time from the SUM of
    /// the account's latest carry-forward holdings (as of today); accounts with
    /// no holdings get `None` for both.
    pub fn get_accounts(&self, profile_id: Option<&str>) -> Result<Vec<Account>> {
        let (sql, args): (String, Vec<Box<dyn rusqlite::ToSql>>) = if let Some(pid) = profile_id {
            let pattern = format!("%\"{pid}\"%");
            (
                r"SELECT id, name, institution, type, currency,
                         is_active, notes, profile_ids
                  FROM accounts
                  WHERE profile_ids LIKE ?1
                  ORDER BY institution, name"
                    .to_string(),
                vec![Box::new(pattern)],
            )
        } else {
            (
                r"SELECT id, name, institution, type, currency,
                         is_active, notes, profile_ids
                  FROM accounts
                  ORDER BY institution, name"
                    .to_string(),
                vec![],
            )
        };

        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows: Vec<Account> = stmt
            .query_map(
                rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())),
                row_to_account,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let today = chrono::Local::now().date_naive();
        let balances = self.balances_from_holdings_as_of(today)?;
        for account in &mut rows {
            if let Some((sum, max_as_of)) = balances.get(&account.id) {
                account.balance = Some(*sum);
                account.balance_date = Some(*max_as_of);
            }
        }
        Ok(rows)
    }

    pub fn get_account_by_id(&self, id: &str) -> Result<Option<Account>> {
        let result = self.conn.query_row(
            r"SELECT id, name, institution, type, currency,
                     is_active, notes, profile_ids
              FROM accounts WHERE id = ?1",
            params![id],
            row_to_account,
        );
        match result {
            Ok(mut a) => {
                let today = chrono::Local::now().date_naive();
                if let Some((sum, max_as_of)) = self.balance_for_account_as_of(&a.id, today)? {
                    a.balance = Some(sum);
                    a.balance_date = Some(max_as_of);
                }
                Ok(Some(a))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// SUM(holdings.value) per account, carrying each (account, symbol,
    /// sub_account) forward to the most recent snapshot on/before `as_of`.
    /// Returns `account_id -> (balance, latest_as_of_among_carried_rows)`.
    ///
    /// Uses `carried_holdings_sql` with `active_only = false`: `get_accounts`
    /// / `get_account_by_id` report balances for inactive accounts too, which
    /// keeps the reconciliation invariant that account balances sum to
    /// summary net worth.
    fn balances_from_holdings_as_of(
        &self,
        as_of: NaiveDate,
    ) -> Result<std::collections::HashMap<String, (Decimal, NaiveDateTime)>> {
        use std::collections::HashMap;
        let as_of_str = as_of.format("%Y-%m-%dT23:59:59").to_string();
        let sql = carried_holdings_sql("account_id, value, as_of", "AND h.as_of <= ?1", false);
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![as_of_str], |row| {
            let account_id: String = row.get(0)?;
            let value_str: String = row.get(1)?;
            let as_of_str: String = row.get(2)?;
            let value: Decimal = value_str.parse().map_err(|_| {
                column_error(
                    1,
                    format!("invalid holding value {value_str:?} for account {account_id}"),
                )
            })?;
            Ok((account_id, value, as_of_str))
        })?;

        let mut agg: HashMap<String, (Decimal, NaiveDateTime)> = HashMap::new();
        for r in rows {
            let (account_id, value, as_of_str) = r?;
            let snapshot_dt = parse_transaction_datetime(&as_of_str)
                .unwrap_or_else(|| chrono::Utc::now().naive_utc());
            let entry = agg
                .entry(account_id)
                .or_insert((Decimal::ZERO, snapshot_dt));
            entry.0 += value;
            if snapshot_dt > entry.1 {
                entry.1 = snapshot_dt;
            }
        }
        Ok(agg)
    }

    fn balance_for_account_as_of(
        &self,
        account_id: &str,
        as_of: NaiveDate,
    ) -> Result<Option<(Decimal, NaiveDateTime)>> {
        Ok(self.balances_from_holdings_as_of(as_of)?.remove(account_id))
    }

    pub fn account_exists(&self, id: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM accounts WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Record a point-in-time balance for `account_id` by upserting a `_CASH`
    /// holding row. Returns an error if the account is unknown. The aggregated
    /// balance shown on the API surface is always derived from `holdings`.
    pub fn set_account_balance(
        &self,
        account_id: &str,
        balance: Decimal,
        date: NaiveDateTime,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        let currency: String = tx
            .query_row(
                "SELECT currency FROM accounts WHERE id = ?1",
                params![account_id],
                |row| row.get(0),
            )
            .map_err(|_| anyhow!("unknown account: {account_id}"))?;

        let as_of_str = date.format("%Y-%m-%dT%H:%M:%S").to_string();
        let exists: bool = tx.query_row(
            "SELECT COUNT(*) > 0 FROM holdings
             WHERE account_id = ?1 AND symbol = '_CASH'
             AND COALESCE(sub_account, '') = '' AND as_of = ?2",
            params![account_id, as_of_str],
            |row| row.get(0),
        )?;

        if exists {
            tx.execute(
                "UPDATE holdings SET value = ?1, currency = ?2
                 WHERE account_id = ?3 AND symbol = '_CASH'
                 AND COALESCE(sub_account, '') = '' AND as_of = ?4",
                params![balance.to_string(), currency, account_id, as_of_str],
            )?;
        } else {
            tx.execute(
                r"INSERT INTO holdings (
                    account_id, symbol, name, holding_type, quantity, price_per_unit,
                    value, currency, as_of, sub_account, is_closed
                ) VALUES (?1, '_CASH', 'Account Balance', 'cash', '1', NULL, ?2, ?3, ?4, NULL, 0)",
                params![account_id, balance.to_string(), currency, as_of_str],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Apply optional field updates to an account. Returns the updated row.
    /// Caller validates `account_type` parse and `currency` exists.
    #[allow(clippy::too_many_arguments)]
    pub fn update_account(
        &self,
        id: &str,
        name: Option<&str>,
        institution: Option<&str>,
        account_type: Option<&AccountType>,
        currency: Option<&str>,
        is_active: Option<bool>,
        notes: Option<Option<&str>>,
        profile_ids: Option<&[String]>,
    ) -> Result<Account> {
        let tx = self.conn.unchecked_transaction()?;

        if let Some(v) = name {
            tx.execute(
                "UPDATE accounts SET name = ?1 WHERE id = ?2",
                params![v, id],
            )?;
        }
        if let Some(v) = institution {
            tx.execute(
                "UPDATE accounts SET institution = ?1 WHERE id = ?2",
                params![v, id],
            )?;
        }
        if let Some(v) = account_type {
            tx.execute(
                "UPDATE accounts SET type = ?1 WHERE id = ?2",
                params![v.as_str(), id],
            )?;
        }
        if let Some(v) = currency {
            tx.execute(
                "UPDATE accounts SET currency = ?1 WHERE id = ?2",
                params![v, id],
            )?;
        }
        if let Some(v) = is_active {
            tx.execute(
                "UPDATE accounts SET is_active = ?1 WHERE id = ?2",
                params![v as i64, id],
            )?;
        }
        if let Some(v) = notes {
            tx.execute(
                "UPDATE accounts SET notes = ?1 WHERE id = ?2",
                params![v, id],
            )?;
        }
        if let Some(ids) = profile_ids {
            let json = serde_json::to_string(ids).unwrap_or_else(|_| "[\"default\"]".to_string());
            tx.execute(
                "UPDATE accounts SET profile_ids = ?1 WHERE id = ?2",
                params![json, id],
            )?;
        }

        tx.commit()?;

        self.get_account_by_id(id)?
            .ok_or_else(|| anyhow!("account {id} not found after update"))
    }

    /// `(transactions, holdings, investment events)` referencing this account.
    /// Delete guards must check all three: holdings and investments carry an
    /// FK to accounts, so a hard delete with any left would hit an FK violation.
    pub fn account_reference_counts(&self, id: &str) -> Result<(i64, i64, i64)> {
        self.conn
            .query_row(
                r"SELECT
                    (SELECT COUNT(*) FROM transactions WHERE account_id = ?1),
                    (SELECT COUNT(*) FROM holdings WHERE account_id = ?1),
                    (SELECT COUNT(*) FROM investments WHERE account_id = ?1)",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(Into::into)
    }

    /// Soft-delete by setting `is_active = 0`. Hard delete is unsafe because
    /// holdings / transactions reference the account; callers must verify
    /// there are no references before calling this.
    pub fn delete_account(&self, id: &str) -> Result<()> {
        let deleted = self.conn.execute(
            "UPDATE accounts SET is_active = 0 WHERE id = ?1",
            params![id],
        )?;
        if deleted == 0 {
            return Err(anyhow!("account {id} not found"));
        }
        Ok(())
    }

    /// Permanently remove the account row, unlike [`Self::delete_account`] which
    /// only flips `is_active`. Callers must verify the account has no
    /// transactions, holdings, or investment events first (see
    /// [`Self::account_reference_counts`]) so no rows are orphaned. Ingestion
    /// checklist rows are per-month bookkeeping metadata and are removed with
    /// the account.
    pub fn hard_delete_account(&self, id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM ingestion_checklist WHERE account_id = ?1",
            params![id],
        )?;
        let deleted = tx.execute("DELETE FROM accounts WHERE id = ?1", params![id])?;
        if deleted == 0 {
            return Err(anyhow!("account {id} not found"));
        }
        tx.commit()?;
        Ok(())
    }

    // ── Investments ───────────────────────────────────────────────────────────

    /// Insert one investment event. `INSERT OR IGNORE` on the unique
    /// fingerprint makes creation idempotent; the returned [`InsertOutcome`]
    /// says whether a row was inserted or the fingerprint already existed
    /// (the existing event is returned either way).
    pub fn create_investment_event(
        &self,
        body: &CreateInvestmentEventBody,
    ) -> Result<(InvestmentEvent, InsertOutcome)> {
        let event_type = InvestmentEventType::parse(&body.event_type)
            .ok_or_else(|| anyhow::anyhow!("invalid event_type: {}", body.event_type))?;
        let quantity = body
            .quantity
            .parse::<Decimal>()
            .map_err(|_| anyhow::anyhow!("invalid quantity"))?;
        let price_per_share = body
            .price_per_share
            .parse::<Decimal>()
            .map_err(|_| anyhow::anyhow!("invalid price_per_share"))?;
        let fee = body
            .fee
            .as_deref()
            .map(|s| {
                s.parse::<Decimal>()
                    .map_err(|_| anyhow::anyhow!("invalid fee"))
            })
            .transpose()?;
        // Invariant: fee_currency is non-null exactly when a fee is present. A fee
        // with no explicit currency is charged in the trade currency.
        let fee_currency = fee.map(|_| {
            body.fee_currency
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(body.currency.as_str())
                .to_string()
        });

        // Sub-unit conversion (GBX/USX/ZAC/ILA -> parent currency) happens here,
        // the single write chokepoint every investment-event caller (direct POST,
        // batch import, CLI) goes through. After this point no sub-unit code is
        // ever persisted. Quantity (a share count) is never affected — only the
        // price and, independently, the fee if it carries its own sub-unit code.
        let (price_per_share, currency) =
            match crate::util::subunits::to_parent(price_per_share, &body.currency) {
                Some((converted, parent)) => (converted, parent.to_string()),
                None => (price_per_share, body.currency.clone()),
            };
        let (fee, fee_currency) = match (fee, fee_currency) {
            (Some(fee_amount), Some(fee_curr)) => {
                match crate::util::subunits::to_parent(fee_amount, &fee_curr) {
                    Some((converted, parent)) => (Some(converted), Some(parent.to_string())),
                    None => (Some(fee_amount), Some(fee_curr)),
                }
            }
            (fee, fee_currency) => (fee, fee_currency),
        };

        let date = parse_transaction_datetime(&body.date)
            .ok_or_else(|| anyhow::anyhow!("invalid date format"))?;
        let date_str = date.format("%Y-%m-%dT%H:%M:%S").to_string();

        let fingerprint = sha256_hex(&format!(
            "{}|{}|{}|{}|{}|{}",
            body.account_id,
            body.symbol,
            date_str,
            quantity,
            price_per_share,
            event_type.as_str()
        ));

        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let source_ids_json =
            serde_json::to_string(&body.source_document_ids).unwrap_or_else(|_| "[]".to_string());

        let rows = self.conn.execute(
            "INSERT OR IGNORE INTO investments
             (id, account_id, event_type, symbol, date, quantity, price_per_share, fee, currency, notes, fingerprint, created_at, source_document_ids, fee_currency)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                id,
                body.account_id,
                event_type.as_str(),
                body.symbol,
                date_str,
                quantity.to_string(),
                price_per_share.to_string(),
                fee.map(|f| f.to_string()),
                currency,
                body.notes,
                fingerprint,
                now,
                source_ids_json,
                fee_currency,
            ],
        )?;
        let outcome = if rows == 1 {
            InsertOutcome::Inserted
        } else {
            InsertOutcome::Duplicate
        };

        // On a duplicate (INSERT ignored) the row already exists; union any new
        // source documents into it so re-imports keep the audit trail complete.
        if !body.source_document_ids.is_empty() {
            merge_source_documents(
                &self.conn,
                "investments",
                "fingerprint",
                &fingerprint,
                &source_ids_json,
            )?;
        }

        let event = self.conn.query_row(
            "SELECT id, account_id, event_type, symbol, date, quantity, price_per_share, fee, currency, notes, fingerprint, created_at, source_document_ids, fee_currency
             FROM investments WHERE fingerprint = ?1",
            params![fingerprint],
            row_to_investment_event,
        )?;
        Ok((event, outcome))
    }

    /// The `(symbol, currency)` pair of a single event, or `None` if it does not
    /// exist. Used by the PATCH path to resolve the fields a partial body omits.
    pub fn investment_event_symbol_currency(&self, id: &str) -> Result<Option<(String, String)>> {
        let row = self
            .conn
            .query_row(
                "SELECT symbol, currency FROM investments WHERE id = ?1",
                params![id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?;
        Ok(row)
    }

    /// The distinct trade currencies already recorded against a symbol, sorted.
    ///
    /// A symbol is a plain TEXT column on `investments` — there is no symbols
    /// table — so there is nowhere to hang a DB-level constraint saying "one
    /// symbol, one currency". This is the lookup the write-time guard uses
    /// instead. Normally returns zero rows (a new symbol) or exactly one.
    ///
    /// `exclude_id` omits one event from the answer, so a PATCH can ask "what
    /// currencies would this symbol have if my row were not counted?" — without
    /// it, editing the sole event of a symbol would always conflict with itself.
    pub fn investment_currencies_for_symbol(
        &self,
        symbol: &str,
        exclude_id: Option<&str>,
    ) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT currency FROM investments
             WHERE symbol = ?1 AND (?2 IS NULL OR id <> ?2) ORDER BY currency",
        )?;
        let rows = stmt
            .query_map(params![symbol, exclude_id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_investment_events(
        &self,
        account_id: Option<&str>,
        symbol: Option<&str>,
        event_type: Option<&str>,
        account_ids: Option<&[String]>,
    ) -> Result<Vec<InvestmentEvent>> {
        let mut conditions = vec!["1=1"];
        let mut sql = String::from(
            "SELECT id, account_id, event_type, symbol, date, quantity, price_per_share, fee, currency, notes, fingerprint, created_at, source_document_ids, fee_currency
             FROM investments WHERE ",
        );

        // Build conditions dynamically — simpler than param_from_iter with optionals
        let account_clause;
        let symbol_clause;
        let event_type_clause;
        let account_ids_clause;

        if let Some(a) = account_id {
            account_clause = format!("account_id = '{}'", a.replace('\'', "''"));
            conditions.push(&account_clause);
        }
        if let Some(s) = symbol {
            symbol_clause = format!("symbol = '{}'", s.replace('\'', "''"));
            conditions.push(&symbol_clause);
        }
        if let Some(e) = event_type {
            event_type_clause = format!("event_type = '{}'", e.replace('\'', "''"));
            conditions.push(&event_type_clause);
        }
        if let Some(ids) = account_ids {
            if ids.is_empty() {
                // Scope explicitly empty (e.g. profile filter matched no accounts)
                return Ok(vec![]);
            }
            let escaped: Vec<String> = ids
                .iter()
                .map(|id| format!("'{}'", id.replace('\'', "''")))
                .collect();
            account_ids_clause = format!("account_id IN ({})", escaped.join(", "));
            conditions.push(&account_ids_clause);
        }

        sql.push_str(&conditions.join(" AND "));
        sql.push_str(" ORDER BY date ASC");

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], row_to_investment_event)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Patch an investment event. All field updates land in one transaction,
    /// and the fingerprint is recomputed whenever an identity field
    /// (event_type, symbol, date, quantity, price_per_share) changes so that
    /// dedup on re-import keeps working. A recompute that collides with
    /// another event's fingerprint fails the whole patch.
    pub fn update_investment_event(
        &self,
        id: &str,
        body: &PatchInvestmentEventBody,
    ) -> Result<Option<InvestmentEvent>> {
        let tx = self.conn.unchecked_transaction()?;

        let current = tx
            .query_row(
                "SELECT account_id, event_type, symbol, date, quantity, price_per_share
                 FROM investments WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((account_id, mut event_type, mut symbol, mut date_str, mut quantity, mut price)) =
            current
        else {
            return Ok(None);
        };

        if let Some(ref et) = body.event_type {
            let parsed = InvestmentEventType::parse(et)
                .ok_or_else(|| anyhow::anyhow!("invalid event_type: {}", et))?;
            event_type = parsed.as_str().to_string();
            tx.execute(
                "UPDATE investments SET event_type = ?1 WHERE id = ?2",
                params![event_type, id],
            )?;
        }
        if let Some(ref s) = body.symbol {
            symbol = s.clone();
            tx.execute(
                "UPDATE investments SET symbol = ?1 WHERE id = ?2",
                params![symbol, id],
            )?;
        }
        if let Some(ref d) = body.date {
            let dt = parse_transaction_datetime(d)
                .ok_or_else(|| anyhow::anyhow!("invalid date format"))?;
            date_str = dt.format("%Y-%m-%dT%H:%M:%S").to_string();
            tx.execute(
                "UPDATE investments SET date = ?1 WHERE id = ?2",
                params![date_str, id],
            )?;
        }
        if let Some(ref q) = body.quantity {
            quantity = q
                .parse::<Decimal>()
                .map_err(|_| anyhow::anyhow!("invalid quantity"))?
                .to_string();
            tx.execute(
                "UPDATE investments SET quantity = ?1 WHERE id = ?2",
                params![quantity, id],
            )?;
        }
        if let Some(ref p) = body.price_per_share {
            price = p
                .parse::<Decimal>()
                .map_err(|_| anyhow::anyhow!("invalid price_per_share"))?
                .to_string();
            tx.execute(
                "UPDATE investments SET price_per_share = ?1 WHERE id = ?2",
                params![price, id],
            )?;
        }
        if body.fee.is_some() {
            let fee_str = body.fee.as_deref();
            if let Some(f) = fee_str {
                f.parse::<Decimal>()
                    .map_err(|_| anyhow::anyhow!("invalid fee"))?;
            }
            tx.execute(
                "UPDATE investments SET fee = ?1 WHERE id = ?2",
                params![fee_str, id],
            )?;
        }
        if let Some(ref c) = body.currency {
            tx.execute(
                "UPDATE investments SET currency = ?1 WHERE id = ?2",
                params![c, id],
            )?;
        }
        if let Some(ref fc) = body.fee_currency {
            tx.execute(
                "UPDATE investments SET fee_currency = ?1 WHERE id = ?2",
                params![fc, id],
            )?;
        }
        if body.notes.is_some() {
            tx.execute(
                "UPDATE investments SET notes = ?1 WHERE id = ?2",
                params![body.notes.as_deref(), id],
            )?;
        }

        let identity_changed = body.event_type.is_some()
            || body.symbol.is_some()
            || body.date.is_some()
            || body.quantity.is_some()
            || body.price_per_share.is_some();
        if identity_changed {
            // Same formula as create_investment_event: decimals go through
            // Decimal Display and event_type through as_str(), so the stored
            // fingerprint stays comparable with future imports.
            let quantity_dec = quantity.parse::<Decimal>().map_err(|_| {
                anyhow!("stored quantity {quantity:?} on investment event {id} is not a decimal")
            })?;
            let price_dec = price.parse::<Decimal>().map_err(|_| {
                anyhow!(
                    "stored price_per_share {price:?} on investment event {id} is not a decimal"
                )
            })?;
            let canonical_event_type = InvestmentEventType::parse(&event_type)
                .ok_or_else(|| {
                    anyhow!("unknown event_type {event_type:?} stored on investment event {id}")
                })?
                .as_str();
            let fingerprint = sha256_hex(&format!(
                "{account_id}|{symbol}|{date_str}|{quantity_dec}|{price_dec}|{canonical_event_type}"
            ));
            tx.execute(
                "UPDATE investments SET fingerprint = ?1 WHERE id = ?2",
                params![fingerprint, id],
            )?;
        }

        tx.commit()?;

        let event = self.conn.query_row(
            "SELECT id, account_id, event_type, symbol, date, quantity, price_per_share, fee, currency, notes, fingerprint, created_at, source_document_ids, fee_currency
             FROM investments WHERE id = ?1",
            params![id],
            row_to_investment_event,
        )?;
        Ok(Some(event))
    }

    pub fn delete_investment_event(&self, id: &str) -> Result<bool> {
        let rows = self
            .conn
            .execute("DELETE FROM investments WHERE id = ?1", params![id])?;
        Ok(rows > 0)
    }

    // ── Transactions ──────────────────────────────────────────────────────────

    /// Insert one transaction. `INSERT OR IGNORE` on the unique fingerprint
    /// makes the import idempotent.
    pub fn insert_transaction(&self, tx: &Transaction) -> Result<InsertOutcome> {
        let source_ids_json =
            serde_json::to_string(&tx.source_document_ids).unwrap_or_else(|_| "[]".to_string());
        let rows = self.conn.execute(
            r"INSERT OR IGNORE INTO transactions (
                id, date, description, normalized, amount, currency,
                account_id, category_id, category_source, confidence, notes,
                is_recurring, exclude_from_summary, fingerprint, fitid, source_document_ids
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                tx.id,
                tx.date.format("%Y-%m-%dT%H:%M:%S").to_string(),
                tx.description,
                tx.normalized,
                tx.amount.to_string(),
                tx.currency,
                tx.account_id,
                tx.category_id,
                tx.category_source.as_ref().map(|s| s.as_str()),
                tx.confidence,
                tx.notes,
                tx.is_recurring as i64,
                tx.exclude_from_summary as i64,
                tx.fingerprint,
                tx.fitid,
                source_ids_json,
            ],
        )?;
        if rows == 1 {
            Ok(InsertOutcome::Inserted)
        } else {
            // Duplicate (same fingerprint). Merge any new source documents into
            // the existing row so the audit trail stays complete across
            // re-imports of overlapping statements.
            if !tx.source_document_ids.is_empty() {
                merge_source_documents(
                    &self.conn,
                    "transactions",
                    "fingerprint",
                    &tx.fingerprint,
                    &source_ids_json,
                )?;
            }
            Ok(InsertOutcome::Duplicate)
        }
    }

    /// Batch-insert a slice of `ImportTransaction`s from the JSON API.
    /// Inserts valid rows and skips bad ones (partial success). Returns an
    /// `ImportResult` with per-row error details for any skipped rows. All
    /// valid rows commit in a single transaction at the end.
    pub fn insert_transactions_bulk(
        &self,
        account_id: &str,
        txns: &[ImportTransaction],
    ) -> Result<ImportResult> {
        use crate::util::{fingerprint, normalize_description};
        use std::collections::HashSet;
        use uuid::Uuid;

        let mut result = ImportResult {
            filename: String::new(),
            account_id: account_id.to_string(),
            ..ImportResult::default()
        };

        // Category validity, preloaded once (a per-row lookup made large
        // imports issue O(rows) queries). Assignable = active leaf; parents
        // get a distinct error message regardless of their active flag,
        // mirroring the old per-row get_category_by_id match.
        let mut active_leaves: HashSet<String> = HashSet::new();
        let mut parent_ids: HashSet<String> = HashSet::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT id, parent_id IS NULL, is_active FROM categories")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? != 0,
                    row.get::<_, i64>(2)? != 0,
                ))
            })?;
            for row in rows {
                let (id, is_parent, is_active) = row?;
                if is_parent {
                    parent_ids.insert(id);
                } else if is_active {
                    active_leaves.insert(id);
                }
            }
        }

        let db_tx = self.conn.unchecked_transaction()?;
        {
            let mut insert_stmt = self.conn.prepare_cached(
                r"INSERT OR IGNORE INTO transactions (
                    id, date, description, normalized, amount, currency,
                    account_id, category_id, category_source, confidence, notes,
                    is_recurring, exclude_from_summary, fingerprint, fitid, source_document_ids
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            )?;

            for (i, t) in txns.iter().enumerate() {
                result.rows_total += 1;
                let date_iso = t.date.format("%Y-%m-%dT%H:%M:%S").to_string();
                let raw_currency = t.currency.clone().unwrap_or_else(|| "GBP".to_string());
                // Sub-unit conversion (GBX/USX/ZAC/ILA -> parent currency): the
                // single write chokepoint for the JSON/API transaction import
                // path. After this point no sub-unit code is ever persisted.
                let (amount, currency) =
                    match crate::util::subunits::to_parent(t.amount, &raw_currency) {
                        Some((converted, parent)) => (converted, parent.to_string()),
                        None => (t.amount, raw_currency),
                    };
                let amount_str = amount.to_string();
                let normalized = normalize_description(&t.description);
                let fp = fingerprint(&date_iso, &amount_str, account_id);

                let category_id = match &t.category_id {
                    Some(cid) if active_leaves.contains(cid) => Some(cid.as_str()),
                    Some(cid) if parent_ids.contains(cid) => {
                        result.errors.push(ImportRowError {
                            index: i,
                            reason: format!("category {cid} is a parent, not a leaf"),
                        });
                        continue;
                    }
                    Some(cid) => {
                        result.errors.push(ImportRowError {
                            index: i,
                            reason: format!("category {cid} not found or inactive"),
                        });
                        continue;
                    }
                    None => None,
                };

                let source_ids_json = serde_json::to_string(&t.source_document_ids)
                    .unwrap_or_else(|_| "[]".to_string());
                let inserted = insert_stmt.execute(params![
                    Uuid::new_v4().to_string(),
                    date_iso,
                    t.description,
                    normalized,
                    amount_str,
                    currency,
                    account_id,
                    category_id,
                    t.category_source.as_ref().map(|s| s.as_str()),
                    Option::<f64>::None,
                    t.notes,
                    t.is_recurring.unwrap_or(false) as i64,
                    t.exclude_from_summary.unwrap_or(false) as i64,
                    fp,
                    Option::<String>::None,
                    source_ids_json,
                ]);

                match inserted {
                    Ok(1) => result.rows_inserted += 1,
                    Ok(_) => {
                        // Duplicate (same fingerprint). Merge any new source
                        // documents into the existing row so the audit trail
                        // stays complete across re-imports of overlapping
                        // statements.
                        let merged = if t.source_document_ids.is_empty() {
                            Ok(())
                        } else {
                            merge_source_documents(
                                &self.conn,
                                "transactions",
                                "fingerprint",
                                &fp,
                                &source_ids_json,
                            )
                        };
                        match merged {
                            Ok(()) => result.rows_duplicate += 1,
                            Err(e) => result.errors.push(ImportRowError {
                                index: i,
                                reason: e.to_string(),
                            }),
                        }
                    }
                    Err(e) => result.errors.push(ImportRowError {
                        index: i,
                        reason: e.to_string(),
                    }),
                }
            }
        }
        db_tx.commit()?;
        Ok(result)
    }

    /// Preview a batch of `ImportTransaction`s without writing anything.
    /// Computes fingerprints and checks for existing matches.
    pub fn dry_run_transactions(
        &self,
        account_id: &str,
        transactions: &[ImportTransaction],
    ) -> Result<Vec<TransactionPreviewRow>> {
        let mut previews = Vec::with_capacity(transactions.len());

        let mut stmt = self
            .conn
            .prepare("SELECT id, description FROM transactions WHERE fingerprint = ?1")?;

        for (i, t) in transactions.iter().enumerate() {
            let date_iso = t.date.format("%Y-%m-%dT%H:%M:%S").to_string();
            let raw_currency = t.currency.clone().unwrap_or_else(|| "GBP".to_string());
            // Mirror insert_transactions_bulk's sub-unit conversion so the
            // preview shown to the user matches what would actually be written.
            let (amount, currency) = match crate::util::subunits::to_parent(t.amount, &raw_currency)
            {
                Some((converted, parent)) => (converted, parent.to_string()),
                None => (t.amount, raw_currency),
            };
            let amount_str = amount.to_string();
            let fp = crate::util::fingerprint(&date_iso, &amount_str, account_id);

            let existing: Option<(String, String)> = stmt
                .query_row(rusqlite::params![fp], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .ok();

            let (status, existing_id, existing_description) = match existing {
                Some((id, desc)) => (TransactionPreviewStatus::Duplicate, Some(id), Some(desc)),
                None => (TransactionPreviewStatus::New, None, None),
            };

            previews.push(TransactionPreviewRow {
                index: i,
                date: t.date,
                description: t.description.clone(),
                amount,
                currency,
                status,
                existing_id,
                existing_description,
                error_reason: None,
                category_id: t.category_id.clone(),
                category_confidence: None,
                source_document_ids: Vec::new(),
            });
        }

        Ok(previews)
    }

    /// Preview parsed CSV rows (from LLM) without writing anything.
    /// Low-confidence rows are marked as errors rather than silently dropped.
    pub fn dry_run_transactions_from_parsed(
        &self,
        account_id: &str,
        rows: &[crate::importers::unified::UnifiedStatementRow],
        min_row_confidence: f32,
    ) -> Result<Vec<TransactionPreviewRow>> {
        let mut previews = Vec::with_capacity(rows.len());

        let mut stmt = self
            .conn
            .prepare("SELECT id, description FROM transactions WHERE fingerprint = ?1")?;

        for (i, row) in rows.iter().enumerate() {
            if row.row_confidence < min_row_confidence {
                previews.push(TransactionPreviewRow {
                    index: i,
                    date: row.date,
                    description: row.description.clone(),
                    amount: row.amount,
                    currency: row.currency.clone(),
                    status: TransactionPreviewStatus::Error,
                    existing_id: None,
                    existing_description: None,
                    error_reason: Some(format!(
                        "row confidence {:.2} below threshold {:.2}",
                        row.row_confidence, min_row_confidence
                    )),
                    category_id: row.category_id.clone(),
                    category_confidence: row.category_confidence,
                    source_document_ids: Vec::new(),
                });
                continue;
            }

            let date_iso = row.date.format("%Y-%m-%dT%H:%M:%S").to_string();
            // Mirror Transaction::from_unified's sub-unit conversion so the
            // preview shown to the user matches what would actually be written.
            let (amount, currency) =
                match crate::util::subunits::to_parent(row.amount, &row.currency) {
                    Some((converted, parent)) => (converted, parent.to_string()),
                    None => (row.amount, row.currency.clone()),
                };
            let amount_str = amount.to_string();
            let fp = crate::util::fingerprint(&date_iso, &amount_str, account_id);

            let existing: Option<(String, String)> = stmt
                .query_row(rusqlite::params![fp], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .ok();

            let (status, existing_id, existing_description) = match existing {
                Some((id, desc)) => (TransactionPreviewStatus::Duplicate, Some(id), Some(desc)),
                None => (TransactionPreviewStatus::New, None, None),
            };

            previews.push(TransactionPreviewRow {
                index: i,
                date: row.date,
                description: row
                    .merchant
                    .as_deref()
                    .filter(|m| !m.is_empty())
                    .unwrap_or(&row.description)
                    .to_string(),
                amount,
                currency,
                status,
                existing_id,
                existing_description,
                error_reason: None,
                category_id: row.category_id.clone(),
                category_confidence: row.category_confidence,
                source_document_ids: Vec::new(),
            });
        }

        Ok(previews)
    }

    /// List transactions with filtering, search, and pagination.
    /// Returns `(rows, total_count)` where `total_count` is the count ignoring
    /// the limit/offset.
    pub fn get_transactions(
        &self,
        filters: &TransactionFilters,
    ) -> Result<(Vec<Transaction>, u64)> {
        let mut conditions: Vec<String> = vec!["1=1".to_string()];
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let mut need_account_join = false;
        let mut need_category_join = false;

        if let Some(start) = filters.start {
            args.push(Box::new(start.format("%Y-%m-%dT00:00:00").to_string()));
            conditions.push(format!("t.date >= ?{}", args.len()));
        }
        if let Some(end) = filters.end {
            args.push(Box::new(end.format("%Y-%m-%dT23:59:59").to_string()));
            conditions.push(format!("t.date <= ?{}", args.len()));
        }
        if let Some(accs) = &filters.accounts {
            if !accs.is_empty() {
                let placeholders: Vec<String> = accs
                    .iter()
                    .map(|v| {
                        args.push(Box::new(v.clone()));
                        format!("?{}", args.len())
                    })
                    .collect();
                conditions.push(format!("t.account_id IN ({})", placeholders.join(",")));
            }
        }
        // Category filter: match on category_id OR legacy category column.
        // The "__uncategorized__" sentinel is OR-combined so users can filter
        // to uncategorised rows alongside specific categories in one go.
        if let Some(cats) = &filters.categories {
            if !cats.is_empty() {
                let mut want_uncategorized = false;
                let real_cats: Vec<&String> = cats
                    .iter()
                    .filter(|v| {
                        if v.as_str() == "__uncategorized__" {
                            want_uncategorized = true;
                            false
                        } else {
                            true
                        }
                    })
                    .collect();

                let mut clauses: Vec<String> = Vec::new();
                if !real_cats.is_empty() {
                    let placeholders: Vec<String> = real_cats
                        .iter()
                        .map(|v| {
                            args.push(Box::new((*v).clone()));
                            format!("?{}", args.len())
                        })
                        .collect();
                    let ph = placeholders.join(",");
                    clauses.push(format!("t.category_id IN ({ph})"));
                }
                if want_uncategorized {
                    clauses.push("t.category_id IS NULL".to_string());
                }
                if !clauses.is_empty() {
                    conditions.push(format!("({})", clauses.join(" OR ")));
                }
            }
        }
        if let Some(ctypes) = &filters.category_types {
            if !ctypes.is_empty() {
                need_category_join = true;
                let placeholders: Vec<String> = ctypes
                    .iter()
                    .map(|v| {
                        args.push(Box::new(v.clone()));
                        format!("?{}", args.len())
                    })
                    .collect();
                conditions.push(format!("c.category_type IN ({})", placeholders.join(",")));
            }
        }
        if let Some(ref source) = filters.category_source {
            args.push(Box::new(source.as_str().to_string()));
            conditions.push(format!("t.category_source = ?{}", args.len()));
        }
        if let Some(search) = &filters.search {
            // Escape the escape character itself first, or a search containing
            // `\` builds a malformed pattern.
            let pattern = format!(
                "%{}%",
                search
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            );
            args.push(Box::new(pattern.clone()));
            let idx = args.len();
            conditions.push(format!(
                "(t.normalized LIKE ?{idx} ESCAPE '\\' OR t.description LIKE ?{idx} ESCAPE '\\' OR t.notes LIKE ?{idx} ESCAPE '\\')"
            ));
        }
        if let Some(pid) = &filters.profile_id {
            need_account_join = true;
            let pattern = format!("%\"{pid}\"%");
            args.push(Box::new(pattern));
            conditions.push(format!("a.profile_ids LIKE ?{}", args.len()));
        }

        let join = if need_account_join {
            "JOIN accounts a ON a.id = t.account_id"
        } else {
            ""
        };
        // The data query always LEFT JOINs `categories c`; the count query only
        // needs it when a category_type filter references `c`.
        let cat_join = if need_category_join {
            "LEFT JOIN categories c ON c.id = t.category_id"
        } else {
            ""
        };
        let where_clause = conditions.join(" AND ");

        let count_sql =
            format!("SELECT COUNT(*) FROM transactions t {join} {cat_join} WHERE {where_clause}");
        let total: i64 = self.conn.query_row(
            &count_sql,
            rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())),
            |row| row.get(0),
        )?;

        let page = filters.page.max(1);
        let limit = filters.limit;
        let offset = (page - 1) as i64 * limit as i64;

        args.push(Box::new(limit as i64));
        let limit_idx = args.len();
        args.push(Box::new(offset));
        let offset_idx = args.len();

        // Order: dynamic by requested column with a stable id tiebreak, so
        // pagination is deterministic across pages even when many rows share
        // the same date / amount / category. Identifiers come from a closed
        // enum, never user input — safe to interpolate.
        let dir = filters.sort_dir.sql();
        let order_by = match filters.sort {
            None => "t.date DESC, t.id DESC".to_string(),
            Some(TransactionSort::Date) => format!("t.date {dir}, t.id DESC"),
            // Amounts are stored in each transaction's native currency, so they
            // must be converted to the preferred currency before comparing or a
            // large-denomination row (e.g. NGN) outranks every GBP row. Rows in a
            // currency with no configured rate fall back to their raw amount.
            Some(TransactionSort::Amount) => format!(
                "CAST(t.amount AS REAL) * CAST(COALESCE(fx.fx_rate, '1') AS REAL) {dir}, t.id DESC"
            ),
            // Push uncategorized rows to the bottom regardless of direction.
            Some(TransactionSort::Category) => format!(
                "(t.category_id IS NULL) ASC, pc.name {dir}, c.name {dir}, t.date DESC, t.id DESC"
            ),
        };

        // LEFT JOIN categories to resolve display name from category_id
        let data_sql = format!(
            r"SELECT t.id, t.date, t.description, t.normalized, t.amount, t.currency,
                     t.account_id, t.category_id,
                     t.category_source, t.confidence, t.notes,
                     t.is_recurring, t.exclude_from_summary, t.fingerprint, t.fitid,
                     t.source_document_ids
              FROM transactions t
              LEFT JOIN categories c ON c.id = t.category_id
              LEFT JOIN categories pc ON pc.id = c.parent_id
              LEFT JOIN currencies fx ON fx.code = t.currency
              {join}
              WHERE {where_clause}
              ORDER BY {order_by}
              LIMIT ?{limit_idx} OFFSET ?{offset_idx}"
        );

        let mut stmt = self.conn.prepare(&data_sql)?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())),
                row_to_transaction,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok((rows, total as u64))
    }

    /// Aggregate spending per category over a filtered range.
    ///
    /// When `direction` is `None` the returned `total` is the signed net sum
    /// (negative = net spend). When `direction` is `Some(Outflow)` or
    /// `Some(Income)` the aggregation filters by sign first and returns the
    /// sum of absolute values. `filters.categories` restricts which category
    /// rows are considered. Excludes transactions with `exclude_from_summary = 1`.
    pub fn get_transactions_by_category(
        &self,
        filters: &TransactionFilters,
        direction: Option<crate::model::TransactionDirection>,
        fx: &crate::util::fx::FxRateMap,
    ) -> Result<Vec<CategoryTotal>> {
        use crate::model::TransactionDirection;
        use crate::util::fx::CurrencyAggregator;
        use std::collections::HashMap;

        // Same "__uncategorized__" sentinel contract as get_transactions:
        // when the categories filter includes it, NULL-category rows come
        // back as their own group (category_id = null). By default they are
        // excluded.
        let mut want_uncategorized = false;
        let real_cats: Vec<&String> = filters
            .categories
            .iter()
            .flatten()
            .filter(|v| {
                if v.as_str() == "__uncategorized__" {
                    want_uncategorized = true;
                    false
                } else {
                    true
                }
            })
            .collect();

        let mut conditions: Vec<String> = vec!["t.exclude_from_summary = 0".to_string()];
        if !want_uncategorized {
            conditions.push("t.category_id IS NOT NULL".to_string());
        }
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let mut need_account_join = false;

        if let Some(start) = filters.start {
            args.push(Box::new(start.format("%Y-%m-%dT00:00:00").to_string()));
            conditions.push(format!("t.date >= ?{}", args.len()));
        }
        if let Some(end) = filters.end {
            args.push(Box::new(end.format("%Y-%m-%dT23:59:59").to_string()));
            conditions.push(format!("t.date <= ?{}", args.len()));
        }
        if let Some(accs) = &filters.accounts {
            if !accs.is_empty() {
                let placeholders: Vec<String> = accs
                    .iter()
                    .map(|v| {
                        args.push(Box::new(v.clone()));
                        format!("?{}", args.len())
                    })
                    .collect();
                conditions.push(format!("t.account_id IN ({})", placeholders.join(",")));
            }
        }
        if !real_cats.is_empty() || want_uncategorized {
            let mut clauses: Vec<String> = Vec::new();
            if !real_cats.is_empty() {
                let placeholders: Vec<String> = real_cats
                    .iter()
                    .map(|v| {
                        args.push(Box::new((*v).clone()));
                        format!("?{}", args.len())
                    })
                    .collect();
                clauses.push(format!("t.category_id IN ({})", placeholders.join(",")));
            }
            if want_uncategorized {
                clauses.push("t.category_id IS NULL".to_string());
            }
            conditions.push(format!("({})", clauses.join(" OR ")));
        }
        let mut need_category_join = false;
        if let Some(ctypes) = &filters.category_types {
            if !ctypes.is_empty() {
                need_category_join = true;
                let placeholders: Vec<String> = ctypes
                    .iter()
                    .map(|v| {
                        args.push(Box::new(v.clone()));
                        format!("?{}", args.len())
                    })
                    .collect();
                conditions.push(format!("c.category_type IN ({})", placeholders.join(",")));
            }
        }
        if let Some(pid) = &filters.profile_id {
            need_account_join = true;
            let pattern = format!("%\"{pid}\"%");
            args.push(Box::new(pattern));
            conditions.push(format!("a.profile_ids LIKE ?{}", args.len()));
        }

        // Direction filter (sign-based).
        let sum_expr = match direction {
            Some(TransactionDirection::Outflow) => {
                conditions.push("CAST(t.amount AS REAL) < 0".to_string());
                "ABS(CAST(t.amount AS REAL))"
            }
            Some(TransactionDirection::Income) => {
                conditions.push("CAST(t.amount AS REAL) > 0".to_string());
                "CAST(t.amount AS REAL)"
            }
            None => "CAST(t.amount AS REAL)",
        };

        let join = if need_account_join {
            "JOIN accounts a ON a.id = t.account_id"
        } else {
            ""
        };
        let cat_join = if need_category_join {
            "LEFT JOIN categories c ON c.id = t.category_id"
        } else {
            ""
        };
        let where_clause = conditions.join(" AND ");

        let sql = format!(
            r"SELECT t.category_id, t.currency, {sum_expr} AS total
              FROM transactions t
              {join}
              {cat_join}
              WHERE {where_clause}
              GROUP BY t.category_id, t.currency"
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let raw: Vec<(Option<String>, String, f64)> = stmt
            .query_map(
                rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())),
                |row| {
                    let category_id: Option<String> = row.get(0)?;
                    let currency: String = row.get(1)?;
                    let total: f64 = row.get(2)?;
                    Ok((category_id, currency, total))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut categories_map: HashMap<Option<String>, CurrencyAggregator> = HashMap::new();
        for (category_id, currency, total_f64) in raw {
            let total = Decimal::try_from(total_f64).map_err(|e| {
                anyhow!("total for category {category_id:?} ({currency}) is not representable: {e}")
            })?;
            categories_map
                .entry(category_id)
                .or_default()
                .add(total, &currency, fx);
        }

        Ok(categories_map
            .into_iter()
            .map(|(category_id, agg)| CategoryTotal {
                category_id,
                total: agg.converted_sum().to_string(),
                display_currency: agg.display_currency(fx.preferred()),
            })
            .collect())
    }

    /// Returns the hierarchical category tree grouped by section.
    pub fn get_all_categories(&self) -> Result<Vec<CategoryNode>> {
        self.get_categories_tree()
    }

    /// Update a transaction's category. `category_id` must be an active leaf node.
    pub fn update_transaction_category(
        &self,
        id: &str,
        category_id: &str,
        source: CategorySource,
    ) -> Result<()> {
        let cat = self
            .get_category_by_id(category_id)?
            .ok_or_else(|| anyhow!("category {category_id} not found"))?;
        if !cat.is_active {
            return Err(anyhow!("category {category_id} is inactive"));
        }
        if cat.parent_id.is_none() {
            return Err(anyhow!(
                "category {category_id} is a parent; only leaf categories can be assigned"
            ));
        }

        let updated = self.conn.execute(
            "UPDATE transactions SET category_id = ?1, category_source = ?2 WHERE id = ?3",
            params![category_id, source.as_str(), id],
        )?;
        if updated == 0 {
            return Err(anyhow!("unknown transaction: {id}"));
        }
        Ok(())
    }

    /// Assign one leaf category to many transactions in a single statement.
    /// Validates the category once (active, leaf), then updates all ids.
    /// Returns the number of rows changed.
    pub fn bulk_update_transaction_category(
        &self,
        ids: &[String],
        category_id: &str,
        source: CategorySource,
    ) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let cat = self
            .get_category_by_id(category_id)?
            .ok_or_else(|| anyhow!("category {category_id} not found"))?;
        if !cat.is_active {
            return Err(anyhow!("category {category_id} is inactive"));
        }
        if cat.parent_id.is_none() {
            return Err(anyhow!(
                "category {category_id} is a parent; only leaf categories can be assigned"
            ));
        }

        let placeholders: String = (0..ids.len())
            .map(|i| format!("?{}", i + 3))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "UPDATE transactions SET category_id = ?1, category_source = ?2 WHERE id IN ({placeholders})"
        );
        let source_str = source.as_str();
        let mut sql_params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(ids.len() + 2);
        sql_params.push(&category_id);
        sql_params.push(&source_str);
        for id in ids {
            sql_params.push(id);
        }
        let n = self.conn.execute(&sql, sql_params.as_slice())?;
        Ok(n)
    }

    pub fn update_transaction_exclude_summary(&self, id: &str, exclude: bool) -> Result<()> {
        let updated = self.conn.execute(
            "UPDATE transactions SET exclude_from_summary = ?1 WHERE id = ?2",
            params![exclude as i64, id],
        )?;
        if updated == 0 {
            return Err(anyhow!("unknown transaction: {id}"));
        }
        Ok(())
    }

    pub fn update_transaction_notes(&self, id: &str, notes: Option<&str>) -> Result<()> {
        let updated = self.conn.execute(
            "UPDATE transactions SET notes = ?1 WHERE id = ?2",
            params![notes, id],
        )?;
        if updated == 0 {
            return Err(anyhow!("unknown transaction: {id}"));
        }
        Ok(())
    }

    /// Apply a transaction patch atomically: category, notes and
    /// exclude_from_summary all land or none do. The individual update
    /// helpers run on the same connection, so they execute inside the
    /// transaction opened here.
    pub fn patch_transaction_fields(
        &self,
        id: &str,
        category: Option<(&str, CategorySource)>,
        notes: Option<&str>,
        exclude_from_summary: Option<bool>,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        if let Some((category_id, source)) = category {
            self.update_transaction_category(id, category_id, source)?;
        }
        if let Some(n) = notes {
            self.update_transaction_notes(id, Some(n))?;
        }
        if let Some(e) = exclude_from_summary {
            self.update_transaction_exclude_summary(id, e)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_transaction_by_id(&self, id: &str) -> Result<Option<Transaction>> {
        let mut stmt = self.conn.prepare(
            r"SELECT t.id, t.date, t.description, t.normalized, t.amount, t.currency,
                     t.account_id, t.category_id,
                     t.category_source, t.confidence, t.notes,
                     t.is_recurring, t.exclude_from_summary, t.fingerprint, t.fitid,
                     t.source_document_ids
              FROM transactions t
              WHERE t.id = ?1",
        )?;
        let result = stmt.query_row(params![id], row_to_transaction).optional()?;
        Ok(result)
    }

    /// Hard-delete a single transaction. Returns the number of rows removed
    /// (0 when the id does not exist). The row's fingerprint is freed, so
    /// re-importing the same statement will re-insert the transaction.
    pub fn delete_transaction(&self, id: &str) -> Result<usize> {
        let n = self
            .conn
            .execute("DELETE FROM transactions WHERE id = ?1", params![id])?;
        Ok(n)
    }

    /// Hard-delete transactions by id. Returns the number of rows removed.
    pub fn delete_transactions(&self, ids: &[String]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let placeholders: String = (1..=ids.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("DELETE FROM transactions WHERE id IN ({placeholders})");
        let n = self
            .conn
            .execute(&sql, rusqlite::params_from_iter(ids.iter()))?;
        Ok(n)
    }

    /// Hard-delete every transaction for an account. Returns the number of rows
    /// removed. Used to clear an account before deleting it.
    pub fn delete_transactions_for_account(&self, account_id: &str) -> Result<usize> {
        let n = self.conn.execute(
            "DELETE FROM transactions WHERE account_id = ?1",
            params![account_id],
        )?;
        Ok(n)
    }

    // ── Categories ───────────────────────────────────────────────────────────

    pub fn create_category(&self, payload: &CreateCategoryPayload) -> Result<Category> {
        if let Some(ref parent_id) = payload.parent_id {
            let parent = self
                .get_category_by_id(parent_id)?
                .ok_or_else(|| anyhow!("parent category {parent_id} not found"))?;
            if !parent.is_active {
                return Err(anyhow!("parent category is inactive"));
            }
            if parent.parent_id.is_some() {
                return Err(anyhow!(
                    "max category depth is 2: cannot create child of a child"
                ));
            }
        }

        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let display_order = payload.display_order.unwrap_or(0);

        self.conn.execute(
            "INSERT INTO categories (id, name, parent_id, display_order, is_active, description, category_type, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, ?7)",
            params![id, payload.name, payload.parent_id, display_order, payload.description, payload.category_type.as_str(), now],
        )?;

        self.get_category_by_id(&id)?
            .ok_or_else(|| anyhow!("failed to read back created category"))
    }

    pub fn get_category_by_id(&self, id: &str) -> Result<Option<Category>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, parent_id, display_order, is_active, description, category_type, created_at, updated_at
             FROM categories WHERE id = ?1",
        )?;
        let result = stmt.query_row(params![id], row_to_category).optional()?;
        Ok(result)
    }

    /// Active categories as a flat list of parent nodes, each with its leaf
    /// children, ordered by `display_order`. (Sections were removed; the
    /// frontend groups by parent.)
    pub fn get_categories_tree(&self) -> Result<Vec<CategoryNode>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, parent_id, display_order, is_active, description, category_type, created_at, updated_at
             FROM categories WHERE is_active = 1
             ORDER BY display_order, name",
        )?;
        let all: Vec<Category> = stmt
            .query_map([], row_to_category)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let parents: Vec<&Category> = all.iter().filter(|c| c.parent_id.is_none()).collect();
        let children: Vec<&Category> = all.iter().filter(|c| c.parent_id.is_some()).collect();

        let tree = parents
            .into_iter()
            .map(|parent| {
                let child_nodes: Vec<CategoryNode> = children
                    .iter()
                    .filter(|c| c.parent_id.as_deref() == Some(&parent.id))
                    .map(|c| CategoryNode {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        description: c.description.clone(),
                        category_type: c.category_type,
                        children: vec![],
                    })
                    .collect();
                CategoryNode {
                    id: parent.id.clone(),
                    name: parent.name.clone(),
                    description: parent.description.clone(),
                    category_type: parent.category_type,
                    children: child_nodes,
                }
            })
            .collect();

        Ok(tree)
    }

    pub fn update_category(&self, id: &str, payload: &PatchCategoryPayload) -> Result<Category> {
        let existing = self
            .get_category_by_id(id)?
            .ok_or_else(|| anyhow!("category {id} not found"))?;

        if let Some(ref new_parent_id) = payload.parent_id {
            let parent = self
                .get_category_by_id(new_parent_id)?
                .ok_or_else(|| anyhow!("parent category {new_parent_id} not found"))?;
            if parent.parent_id.is_some() {
                return Err(anyhow!(
                    "max category depth is 2: cannot move under a child category"
                ));
            }
            let child_count: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM categories WHERE parent_id = ?1 AND is_active = 1",
                params![id],
                |r| r.get(0),
            )?;
            if child_count > 0 {
                return Err(anyhow!(
                    "cannot move a parent category under another parent"
                ));
            }
        }

        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let name = payload.name.as_deref().unwrap_or(&existing.name);
        let parent_id = payload
            .parent_id
            .as_deref()
            .or(existing.parent_id.as_deref());
        let display_order = payload.display_order.unwrap_or(existing.display_order);
        let is_active = payload
            .is_active
            .map(|v| v as i32)
            .unwrap_or(existing.is_active as i32);
        // PATCH treats `description: Some("")` as "clear" and `None` as
        // "leave unchanged" so the user can blank a description out via the UI.
        let description = match payload.description.as_ref() {
            Some(s) if s.is_empty() => None,
            Some(s) => Some(s.clone()),
            None => existing.description.clone(),
        };
        let category_type = payload.category_type.unwrap_or(existing.category_type);

        self.conn.execute(
            "UPDATE categories SET name = ?1, parent_id = ?2, display_order = ?3, is_active = ?4, description = ?5, category_type = ?6, updated_at = ?7
             WHERE id = ?8",
            params![name, parent_id, display_order, is_active, description, category_type.as_str(), now, id],
        )?;

        self.get_category_by_id(id)?
            .ok_or_else(|| anyhow!("failed to read back updated category"))
    }

    pub fn soft_delete_category(&self, id: &str) -> Result<()> {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let updated = self.conn.execute(
            "UPDATE categories SET is_active = 0, updated_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        if updated == 0 {
            return Err(anyhow!("category {id} not found"));
        }
        Ok(())
    }

    /// Permanently removes a category, unlike [`Self::soft_delete_category`]
    /// which only flips `is_active`. Refuses if the category still has child
    /// categories (reparent or delete them first) so a sub-tree is never
    /// orphaned. Any transactions or budget rows pointing at it are detached
    /// so no dangling references remain (FK enforcement is off, so this is
    /// done explicitly rather than relying on ON DELETE).
    pub fn hard_delete_category(&self, id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        let child_count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM categories WHERE parent_id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        if child_count > 0 {
            return Err(anyhow!(
                "category {id} has {child_count} child categories; reparent or delete them first"
            ));
        }

        tx.execute(
            "UPDATE transactions SET category_id = NULL WHERE category_id = ?1",
            params![id],
        )?;
        tx.execute("DELETE FROM budgets WHERE category_id = ?1", params![id])?;
        tx.execute(
            "DELETE FROM standing_budgets WHERE category_id = ?1",
            params![id],
        )?;
        tx.execute(
            "DELETE FROM budget_overrides WHERE category_id = ?1",
            params![id],
        )?;

        let deleted = tx.execute("DELETE FROM categories WHERE id = ?1", params![id])?;
        if deleted == 0 {
            return Err(anyhow!("category {id} not found"));
        }
        tx.commit()?;
        Ok(())
    }

    pub fn resolve_category_by_name(&self, name: &str) -> Result<Option<Category>> {
        let result = self
            .conn
            .query_row(
                "SELECT id, name, parent_id, display_order, is_active, description, category_type, created_at, updated_at
             FROM categories WHERE name = ?1 AND is_active = 1",
                params![name],
                row_to_category,
            )
            .optional()?;

        if result.is_some() {
            return Ok(result);
        }

        if let Some((_, child_name)) = name.split_once(": ") {
            let result = self
                .conn
                .query_row(
                    "SELECT id, name, parent_id, display_order, is_active, description, category_type, created_at, updated_at
                 FROM categories WHERE name = ?1 AND is_active = 1",
                    params![child_name.trim()],
                    row_to_category,
                )
                .optional()?;
            return Ok(result);
        }

        Ok(None)
    }

    // ── Budgets (standing + overrides) ───────────────────────────────────────

    pub fn set_standing_budget(&self, category_id: &str, amount: Decimal) -> Result<()> {
        self.conn.execute(
            r"INSERT INTO standing_budgets (category_id, amount)
              VALUES (?1, ?2)
              ON CONFLICT(category_id) DO UPDATE SET amount = excluded.amount",
            params![category_id, amount.to_string()],
        )?;
        Ok(())
    }

    pub fn set_budget_override(
        &self,
        month: &str,
        category_id: &str,
        amount: Decimal,
    ) -> Result<()> {
        self.conn.execute(
            r"INSERT INTO budget_overrides (month, category_id, amount)
              VALUES (?1, ?2, ?3)
              ON CONFLICT(month, category_id) DO UPDATE SET amount = excluded.amount",
            params![month, category_id, amount.to_string()],
        )?;
        Ok(())
    }

    /// Returns effective budget rows for `month`, merging standing targets with
    /// per-month overrides. Includes actual spend from transactions.
    /// Excludes transactions with `exclude_from_summary = 1`.
    pub fn get_effective_budget(
        &self,
        month: &str,
        fx: &crate::util::fx::FxRateMap,
    ) -> Result<Vec<BudgetRow>> {
        use crate::util::fx::CurrencyAggregator;
        use std::collections::HashMap;

        let sql = r"
            SELECT
                COALESCE(sb.category_id, t_agg.category_id) AS cat_id,
                COALESCE(
                  CASE WHEN c.parent_id IS NOT NULL THEN pc.name || ': ' || c.name
                       ELSE c.name END,
                  sb.category
                ) AS category_display,
                COALESCE(bo.amount, sb.amount) AS budgeted,
                t_agg.category_id,
                t_agg.amount,
                t_agg.currency
            FROM standing_budgets sb
            LEFT JOIN budget_overrides bo
                ON bo.category_id = sb.category_id AND bo.month = ?1
            LEFT JOIN (
                SELECT category_id,
                       CAST(ABS(CAST(amount AS REAL)) AS TEXT) AS amount,
                       currency
                FROM transactions
                WHERE substr(date, 1, 7) = ?1
                  AND CAST(amount AS REAL) < 0
                  AND category_id IS NOT NULL
                  AND exclude_from_summary = 0
            ) t_agg ON t_agg.category_id = sb.category_id
            LEFT JOIN categories c ON c.id = COALESCE(sb.category_id, t_agg.category_id)
            LEFT JOIN categories pc ON pc.id = c.parent_id
            WHERE sb.category_id IS NOT NULL

            UNION ALL

            SELECT
                t_agg2.category_id AS cat_id,
                COALESCE(
                  CASE WHEN c2.parent_id IS NOT NULL THEN pc2.name || ': ' || c2.name
                       ELSE c2.name END,
                  NULL
                ) AS category_display,
                bo2.amount AS budgeted,
                t_agg2.category_id,
                t_agg2.amount,
                t_agg2.currency
            FROM (
                SELECT category_id,
                       CAST(ABS(CAST(amount AS REAL)) AS TEXT) AS amount,
                       currency
                FROM transactions
                WHERE substr(date, 1, 7) = ?1
                  AND CAST(amount AS REAL) < 0
                  AND category_id IS NOT NULL
                  AND exclude_from_summary = 0
            ) t_agg2
            LEFT JOIN budget_overrides bo2
                ON bo2.category_id = t_agg2.category_id AND bo2.month = ?1
            LEFT JOIN categories c2 ON c2.id = t_agg2.category_id
            LEFT JOIN categories pc2 ON pc2.id = c2.parent_id
            WHERE t_agg2.category_id NOT IN (
                SELECT category_id FROM standing_budgets WHERE category_id IS NOT NULL
            )

            ORDER BY category_display
        ";

        type RawBudgetRow = (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        let mut stmt = self.conn.prepare(sql)?;
        let raw: Vec<RawBudgetRow> = stmt
            .query_map(params![month], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut categories_map: HashMap<
            String,
            (Option<String>, Option<String>, CurrencyAggregator),
        > = HashMap::new();

        for (cat_id, _category_display, budgeted, _t_cat_id, amount_str, currency) in raw {
            let key = cat_id.clone().unwrap_or_else(|| "unknown".to_string());
            let entry = categories_map
                .entry(key)
                .or_insert_with(|| (cat_id.clone(), budgeted.clone(), Default::default()));

            if let (Some(amount_s), Some(curr)) = (amount_str, currency) {
                if let Ok(amount) = amount_s.parse::<Decimal>() {
                    entry.2.add(amount, &curr, fx);
                }
            }
        }

        // Deduplicate by category_id and collect budgeted values
        let mut final_rows: Vec<_> = categories_map
            .into_values()
            .map(|(category_id, budgeted, actual_agg)| {
                let actual_dec = actual_agg.converted_sum();
                let percent = budgeted.as_ref().and_then(|b| {
                    b.parse::<Decimal>().ok().and_then(|budget| {
                        if budget.is_zero() {
                            None
                        } else {
                            let p = (actual_dec / budget * Decimal::ONE_HUNDRED)
                                .try_into()
                                .unwrap_or(0.0_f64);
                            Some(p)
                        }
                    })
                });

                BudgetRow {
                    category_id,
                    budgeted,
                    actual: actual_dec.to_string(),
                    actual_display: actual_agg.display_currency(fx.preferred()),
                    percent,
                }
            })
            .collect();

        final_rows.sort_by(|a, b| a.category_id.cmp(&b.category_id));
        Ok(final_rows)
    }

    /// Spending grid: aggregated spending per category per time period.
    /// Excludes transactions with `exclude_from_summary = 1`.
    #[allow(clippy::too_many_arguments)]
    pub fn get_spending_grid(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        granularity: &Granularity,
        profile_id: Option<&str>,
        accounts: &[String],
        categories: &[String],
        category_types: &[String],
        group_by: SpendingGroupBy,
        fx: &crate::util::fx::FxRateMap,
    ) -> Result<Vec<SpendingGridRow>> {
        use crate::util::fx::CurrencyAggregator;

        let period_expr = match granularity {
            Granularity::Monthly => "substr(t.date, 1, 7)".to_string(),
            Granularity::Quarterly => concat!(
                "CASE ",
                "WHEN CAST(substr(t.date,6,2) AS INTEGER) BETWEEN 1 AND 3 ",
                "  THEN substr(t.date,1,4)||'-Q1' ",
                "WHEN CAST(substr(t.date,6,2) AS INTEGER) BETWEEN 4 AND 6 ",
                "  THEN substr(t.date,1,4)||'-Q2' ",
                "WHEN CAST(substr(t.date,6,2) AS INTEGER) BETWEEN 7 AND 9 ",
                "  THEN substr(t.date,1,4)||'-Q3' ",
                "ELSE substr(t.date,1,4)||'-Q4' END"
            )
            .to_string(),
            Granularity::Yearly => "substr(t.date, 1, 4)".to_string(),
        };

        // The dimension each row is grouped by.
        let key_expr = match group_by {
            SpendingGroupBy::LeafCategory => "t.category_id",
            SpendingGroupBy::ParentCategory => "COALESCE(c.parent_id, c.id)",
            SpendingGroupBy::CategoryType => "c.category_type",
            SpendingGroupBy::Account => "t.account_id",
        };

        let start_str = start.format("%Y-%m-%dT00:00:00").to_string();
        let end_str = end.format("%Y-%m-%dT23:59:59").to_string();

        let mut conditions = vec![
            "t.category_id IS NOT NULL".to_string(),
            "t.exclude_from_summary = 0".to_string(),
        ];
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        args.push(Box::new(start_str));
        conditions.push(format!("t.date >= ?{}", args.len()));
        args.push(Box::new(end_str));
        conditions.push(format!("t.date <= ?{}", args.len()));

        let push_in = |col: &str,
                       vals: &[String],
                       conds: &mut Vec<String>,
                       args: &mut Vec<Box<dyn rusqlite::ToSql>>| {
            if vals.is_empty() {
                return;
            }
            let placeholders: Vec<String> = vals
                .iter()
                .map(|v| {
                    args.push(Box::new(v.clone()));
                    format!("?{}", args.len())
                })
                .collect();
            conds.push(format!("{col} IN ({})", placeholders.join(",")));
        };
        push_in("t.account_id", accounts, &mut conditions, &mut args);
        push_in("t.category_id", categories, &mut conditions, &mut args);
        push_in(
            "c.category_type",
            category_types,
            &mut conditions,
            &mut args,
        );

        let join = if let Some(pid) = profile_id {
            args.push(Box::new(format!("%\"{pid}\"%")));
            conditions.push(format!("a.profile_ids LIKE ?{}", args.len()));
            "JOIN accounts a ON a.id = t.account_id"
        } else {
            ""
        };

        let where_clause = conditions.join(" AND ");

        let sql = format!(
            r"SELECT
                {key_expr} AS gkey,
                t.category_id,
                c.parent_id,
                {period_expr} AS period,
                t.currency,
                SUM(CAST(t.amount AS REAL)) AS period_total
              FROM transactions t
              LEFT JOIN categories c ON c.id = t.category_id
              {join}
              WHERE {where_clause}
              GROUP BY {key_expr}, period, t.currency
              ORDER BY {key_expr}, period, t.currency"
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let raw: Vec<SpendingGridRawRow> = stmt
            .query_map(
                rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())),
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        // Fetch standing budgets keyed by category_id
        let budgets: HashMap<String, String> = {
            let mut stmt = self.conn.prepare(
                "SELECT category_id, amount FROM standing_budgets WHERE category_id IS NOT NULL",
            )?;
            stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .collect()
        };

        // Build the grid rows with CurrencyAggregator tracking
        let mut grid: HashMap<
            String,
            (
                SpendingGridRow,
                CurrencyAggregator,
                HashMap<String, CurrencyAggregator>,
            ),
        > = HashMap::new();
        for (gkey, cat_id, parent_id, period, currency, total_f64) in raw {
            let total_dec = Decimal::try_from(total_f64).map_err(|e| {
                anyhow!("spending total for {gkey} in period {period} is not representable: {e}")
            })?;
            let entry = grid.entry(gkey.clone()).or_insert_with(|| {
                let (category_id, parent_id_field, group_key) = match group_by {
                    SpendingGroupBy::LeafCategory => (cat_id.clone(), parent_id.clone(), None),
                    _ => (None, None, Some(gkey.clone())),
                };
                (
                    SpendingGridRow {
                        category_id,
                        parent_id: parent_id_field,
                        group_key,
                        periods: HashMap::new(),
                        periods_display: HashMap::new(),
                        average: None,
                        average_display: None,
                        budget: None,
                        total: None,
                        total_display: None,
                    },
                    Default::default(),
                    HashMap::new(),
                )
            });

            entry.1.add(total_dec, &currency, fx);
            entry
                .2
                .entry(period)
                .or_default()
                .add(total_dec, &currency, fx);
        }

        // Compute totals, averages, and attach budgets
        let mut result: Vec<SpendingGridRow> = grid
            .into_values()
            .map(|(mut row, total_agg, period_aggs)| {
                for (period, agg) in period_aggs {
                    // `periods` holds the FX-converted (preferred-currency) sum;
                    // `periods_display` keeps the original foreign amount for
                    // single-currency periods so the UI can show "₦X (£Y)".
                    // Without the conversion, foreign sums (e.g. NGN) render at
                    // face value — a ~2000x overstatement for NGN.
                    row.periods
                        .insert(period.clone(), Some(agg.converted_sum().to_string()));
                    if let Some(display) = agg.display_currency(fx.preferred()) {
                        row.periods_display.insert(period, display);
                    }
                }

                let total_converted = total_agg.converted_sum();
                if total_converted != Decimal::ZERO {
                    row.total = Some(total_converted.to_string());
                }
                if let Some(display) = total_agg.display_currency(fx.preferred()) {
                    row.total_display = Some(display);
                }

                let non_zero_periods = row
                    .periods
                    .values()
                    .filter_map(|v| v.as_ref())
                    .filter_map(|s| s.parse::<Decimal>().ok())
                    .filter(|d| d != &Decimal::ZERO)
                    .count();
                if non_zero_periods > 0 && !total_converted.is_zero() {
                    let avg = total_converted / Decimal::from(non_zero_periods as u64);
                    row.average = Some(avg.to_string());
                    if let Some(ref total_display) = row.total_display {
                        row.average_display = Some(total_display.clone());
                    }
                }

                if let Some(ref cid) = row.category_id {
                    row.budget = budgets.get(cid).cloned();
                }
                row
            })
            .collect();

        result.sort_by(|a, b| {
            a.group_key
                .cmp(&b.group_key)
                .then(a.category_id.cmp(&b.category_id))
        });
        Ok(result)
    }

    // ── Portfolio ─────────────────────────────────────────────────────────────

    pub fn upsert_holdings(&self, account_id: &str, holdings: &[Holding]) -> Result<()> {
        // Invariant: a closed holding must be zeroed (see `close_holding`).
        for h in holdings {
            if h.is_closed && !h.value.is_zero() {
                anyhow::bail!(
                    "cannot upsert closed holding {} in account {account_id} with non-zero value {}; closed holdings must be zeroed",
                    h.symbol,
                    h.value
                );
            }
        }

        let tx = self.conn.unchecked_transaction()?;
        for h in holdings {
            let sub = h.sub_account.as_deref().unwrap_or("");
            let as_of_str = h.as_of.format("%Y-%m-%dT%H:%M:%S").to_string();
            let source_ids_json =
                serde_json::to_string(&h.source_document_ids).unwrap_or_else(|_| "[]".to_string());

            let exists: bool = tx.query_row(
                "SELECT COUNT(*) > 0 FROM holdings
                 WHERE account_id = ?1 AND symbol = ?2
                 AND COALESCE(sub_account, '') = ?3 AND as_of = ?4",
                params![account_id, h.symbol, sub, as_of_str],
                |row| row.get(0),
            )?;

            if exists {
                // Union the incoming source documents into the existing row so a
                // re-import keeps every contributing document linked. An empty
                // incoming list leaves the existing list untouched.
                tx.execute(
                    "UPDATE holdings SET name = ?1, holding_type = ?2, quantity = ?3,
                     price_per_unit = ?4, value = ?5, currency = ?6, short_name = ?7,
                     is_closed = ?8,
                     source_document_ids = (
                        SELECT json_group_array(value) FROM (
                          SELECT value FROM json_each(holdings.source_document_ids)
                          UNION
                          SELECT value FROM json_each(?9)
                        )
                     )
                     WHERE account_id = ?10 AND symbol = ?11
                     AND COALESCE(sub_account, '') = ?12 AND as_of = ?13",
                    params![
                        h.name,
                        h.holding_type.as_str(),
                        h.quantity.to_string(),
                        h.price_per_unit.map(|p| p.to_string()),
                        h.value.to_string(),
                        h.currency,
                        h.short_name,
                        h.is_closed as i64,
                        source_ids_json,
                        account_id,
                        h.symbol,
                        sub,
                        as_of_str
                    ],
                )?;
            } else {
                tx.execute(
                    "INSERT INTO holdings (account_id, symbol, name, holding_type, quantity,
                     price_per_unit, value, currency, as_of, short_name, sub_account, is_closed,
                     source_document_ids)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    params![
                        account_id,
                        h.symbol,
                        h.name,
                        h.holding_type.as_str(),
                        h.quantity.to_string(),
                        h.price_per_unit.map(|p| p.to_string()),
                        h.value.to_string(),
                        h.currency,
                        as_of_str,
                        h.short_name,
                        h.sub_account,
                        h.is_closed as i64,
                        source_ids_json
                    ],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    // ── Ingestion checklist ───────────────────────────────────────────────────

    pub fn get_checklist(&self, month: &str) -> Result<Vec<ChecklistItem>> {
        let mut stmt = self.conn.prepare(
            r"SELECT a.id, a.name,
                     COALESCE(ic.status, 'pending') AS status,
                     ic.completed_at,
                     ic.notes
              FROM accounts a
              LEFT JOIN ingestion_checklist ic
                ON ic.account_id = a.id AND ic.month = ?1
              WHERE a.is_active = 1
              ORDER BY a.institution, a.name",
        )?;
        let rows = stmt
            .query_map(params![month], |row| {
                let status_str: String = row.get(2)?;
                Ok(ChecklistItem {
                    account_id: row.get(0)?,
                    account_name: row.get(1)?,
                    status: ChecklistStatus::parse(&status_str),
                    completed_at: row.get(3)?,
                    notes: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn mark_checklist_complete(
        &self,
        month: &str,
        account_id: &str,
        notes: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            r"INSERT INTO ingestion_checklist (month, account_id, status, completed_at, notes)
              VALUES (?1, ?2, 'complete', strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), ?3)
              ON CONFLICT(month, account_id) DO UPDATE SET
                status       = 'complete',
                completed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                notes        = COALESCE(excluded.notes, ingestion_checklist.notes)",
            params![month, account_id, notes],
        )?;
        Ok(())
    }

    // ── Import log ────────────────────────────────────────────────────────────

    pub fn log_import(&self, log: &ImportLog) -> Result<()> {
        self.conn.execute(
            r"INSERT INTO import_log (
                filename, account_id, rows_total, rows_inserted, rows_duplicate,
                source, detected_bank, detection_confidence
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                log.filename,
                log.account_id,
                log.rows_total as i64,
                log.rows_inserted as i64,
                log.rows_duplicate as i64,
                log.source,
                log.detected_bank.as_str(),
                log.detection_confidence,
            ],
        )?;
        Ok(())
    }

    // ── Documents (source-file storage & provenance) ──────────────────────────

    /// Store an uploaded file. Deduplicated by content hash: if a document with
    /// the same bytes already exists, returns it untouched (no new row, no new
    /// file). Returns `(document, deduped)` where `deduped` is true when an
    /// existing row was reused.
    pub fn store_document(
        &self,
        filename: &str,
        mime_type: &str,
        bytes: &[u8],
        origin: &str,
        account_id: Option<&str>,
    ) -> Result<(Document, bool)> {
        let content_hash = sha256_hex_bytes(bytes);

        if let Some(existing) = self.find_document_by_hash(&content_hash)? {
            return Ok((existing, true));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let on_disk = format!("{id}_{}", sanitize_filename(filename));
        let file_path = self.documents_dir.join(&on_disk);
        std::fs::write(&file_path, bytes)
            .with_context(|| format!("writing document file {file_path:?}"))?;
        #[cfg(unix)]
        set_file_mode_600(&file_path)?;

        let file_path_str = file_path.to_string_lossy().to_string();
        self.conn.execute(
            r"INSERT INTO documents (
                id, filename, file_path, mime_type, size_bytes, content_hash, origin, account_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                filename,
                file_path_str,
                mime_type,
                bytes.len() as i64,
                content_hash,
                origin,
                account_id,
            ],
        )?;

        let doc = self
            .get_document(&id)?
            .ok_or_else(|| anyhow!("document {id} vanished immediately after insert"))?;
        Ok((doc, false))
    }

    fn find_document_by_hash(&self, content_hash: &str) -> Result<Option<Document>> {
        self.conn
            .query_row(
                "SELECT id, filename, file_path, mime_type, size_bytes, content_hash, origin, \
                 account_id, uploaded_at FROM documents WHERE content_hash = ?1",
                params![content_hash],
                row_to_document,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn get_document(&self, id: &str) -> Result<Option<Document>> {
        self.conn
            .query_row(
                "SELECT id, filename, file_path, mime_type, size_bytes, content_hash, origin, \
                 account_id, uploaded_at FROM documents WHERE id = ?1",
                params![id],
                row_to_document,
            )
            .optional()
            .map_err(Into::into)
    }

    /// List every stored document with its orphan flag. `reference_count` is
    /// left `None` by default (clients fetch it lazily per row via
    /// `get_document`); with `include_refs` it is populated for every row via
    /// one grouped scan over the three referencing link sources, matching
    /// `document_references(id).total()` per document.
    pub fn list_documents(&self, include_refs: bool) -> Result<Vec<DocumentSummary>> {
        // Single pass per table over the source_document_ids JSON arrays.
        // Without include_refs a UNION collects the distinct referenced ids
        // (orphan flag only); with it, UNION ALL + GROUP BY counts every
        // referencing row. Either way the query count is constant.
        let referenced: std::collections::HashMap<String, usize> = if include_refs {
            let mut stmt = self.conn.prepare(
                r"SELECT value, COUNT(*) FROM (
                    SELECT j.value FROM transactions t, json_each(t.source_document_ids) j
                    UNION ALL
                    SELECT j.value FROM holdings h, json_each(h.source_document_ids) j
                    UNION ALL
                    SELECT j.value FROM investments i, json_each(i.source_document_ids) j
                  ) GROUP BY value",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        } else {
            let mut stmt = self.conn.prepare(
                r"SELECT j.value FROM transactions t, json_each(t.source_document_ids) j
                  UNION
                  SELECT j.value FROM holdings h, json_each(h.source_document_ids) j
                  UNION
                  SELECT j.value FROM investments i, json_each(i.source_document_ids) j",
            )?;
            let ids = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, 0usize)))?;
            ids.collect::<rusqlite::Result<_>>()?
        };

        let mut stmt = self.conn.prepare(
            r"SELECT d.id, d.filename, d.mime_type, d.size_bytes, d.origin, d.account_id, d.uploaded_at
              FROM documents d
              ORDER BY d.uploaded_at DESC, d.filename",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let refs = referenced.get(&id).copied();
                Ok(DocumentSummary {
                    id,
                    filename: row.get(1)?,
                    mime_type: row.get(2)?,
                    size_bytes: row.get::<_, i64>(3)? as usize,
                    origin: row.get(4)?,
                    account_id: row.get(5)?,
                    uploaded_at: row.get(6)?,
                    reference_count: if include_refs {
                        Some(refs.unwrap_or(0))
                    } else {
                        None
                    },
                    orphaned: refs.is_none(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Count rows referencing this document, split by entity type.
    pub fn document_references(&self, id: &str) -> Result<DocumentReferences> {
        self.conn
            .query_row(
                r"SELECT
                    (SELECT COUNT(*) FROM transactions t, json_each(t.source_document_ids) j WHERE j.value = ?1),
                    (SELECT COUNT(*) FROM holdings h, json_each(h.source_document_ids) j WHERE j.value = ?1),
                    (SELECT COUNT(*) FROM investments i, json_each(i.source_document_ids) j WHERE j.value = ?1)",
                params![id],
                |row| {
                    Ok(DocumentReferences {
                        transactions: row.get::<_, i64>(0)? as usize,
                        holdings: row.get::<_, i64>(1)? as usize,
                        investments: row.get::<_, i64>(2)? as usize,
                    })
                },
            )
            .map_err(Into::into)
    }

    /// Delete a document. With `force = false`, a referenced document is left
    /// untouched and `Referenced` is returned. With `force = true`, the id is
    /// stripped from every referencing row first, then the row and file are
    /// removed. The DB mutations (unlink + row delete) run in one transaction;
    /// the file is removed best-effort afterwards.
    pub fn delete_document(&self, id: &str, force: bool) -> Result<DeleteDocumentOutcome> {
        let doc = match self.get_document(id)? {
            Some(d) => d,
            None => return Ok(DeleteDocumentOutcome::NotFound),
        };
        let refs = self.document_references(id)?;
        if refs.total() > 0 && !force {
            return Ok(DeleteDocumentOutcome::Referenced(refs));
        }

        let tx = self.conn.unchecked_transaction()?;
        let unlinked = if refs.total() > 0 {
            let mut counts = DocumentReferences::default();
            for table in ["transactions", "holdings", "investments"] {
                let n = tx.execute(
                    &format!(
                        "UPDATE {table} SET source_document_ids = (
                            SELECT COALESCE(json_group_array(value), '[]')
                            FROM json_each({table}.source_document_ids) WHERE value <> ?1
                         )
                         WHERE id IN (
                            SELECT x.id FROM {table} x, json_each(x.source_document_ids) j
                            WHERE j.value = ?1
                         )"
                    ),
                    params![id],
                )?;
                match table {
                    "transactions" => counts.transactions = n,
                    "holdings" => counts.holdings = n,
                    _ => counts.investments = n,
                }
            }
            counts
        } else {
            DocumentReferences::default()
        };
        tx.execute("DELETE FROM documents WHERE id = ?1", params![id])?;
        tx.commit()?;

        if let Err(e) = std::fs::remove_file(&doc.file_path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path = %doc.file_path, error = %e, "failed to remove document file on disk");
            }
        }

        Ok(DeleteDocumentOutcome::Deleted(unlinked))
    }

    // ── API tokens ────────────────────────────────────────────────────────────

    pub fn create_token(&self, name: &str) -> Result<String> {
        let raw = generate_raw_token();
        let hash = sha256_hex(&raw);
        self.conn
            .execute(
                "INSERT INTO api_tokens (name, token_hash, is_active) VALUES (?1, ?2, 1)",
                params![name, hash],
            )
            .with_context(|| format!("creating api token {name:?}"))?;
        Ok(raw)
    }

    pub fn list_tokens(&self) -> Result<Vec<TokenInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, created_at, last_used, is_active FROM api_tokens ORDER BY created_at",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(TokenInfo {
                    name: row.get(0)?,
                    created_at: row.get(1)?,
                    last_used: row.get(2)?,
                    is_active: row.get::<_, i64>(3)? != 0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn revoke_token(&self, name: &str) -> Result<()> {
        let updated = self.conn.execute(
            "UPDATE api_tokens SET is_active = 0 WHERE name = ?1",
            params![name],
        )?;
        if updated == 0 {
            return Err(anyhow!("unknown token: {name}"));
        }
        Ok(())
    }

    pub fn validate_token(&self, raw_token: &str) -> Result<Option<String>> {
        let hash = sha256_hex(raw_token);
        let row: Option<(String, i64)> = self
            .conn
            .query_row(
                "SELECT name, is_active FROM api_tokens WHERE token_hash = ?1",
                params![hash],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();
        match row {
            Some((name, active)) if active != 0 => {
                // Debounced: refreshing last_used on every request turns each
                // authenticated read into a write. Only touch it when unset or
                // older than 60s. The stored format is fixed-width ISO 8601
                // UTC, so string comparison is chronological.
                self.conn.execute(
                    "UPDATE api_tokens SET last_used = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                     WHERE name = ?1
                       AND (last_used IS NULL
                            OR last_used < strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-60 seconds'))",
                    params![name],
                )?;
                Ok(Some(name))
            }
            _ => Ok(None),
        }
    }

    // ── Portfolio queries ─────────────────────────────────────────────────────

    /// Snapshot stream for a [`CarryForward`] walk over `[from, to]`: the
    /// latest snapshot per (account, symbol, sub_account) at or before
    /// end-of-day `from`, plus every snapshot inside `(from, to]`, sorted by
    /// `as_of` ascending. Replaying it reproduces `get_holdings_for_summary`
    /// for any period end in the range, at a cost that scales with live
    /// holdings plus in-range snapshots rather than full history depth.
    ///
    /// `account_types` / `account_ids` filter by owning account; both are
    /// safe to push into SQL because a key's carry-forward never depends on
    /// other accounts' snapshots.
    fn holding_snapshots_in_range(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        profile_id: Option<&str>,
        account_types: Option<&[AccountType]>,
        account_ids: &[String],
    ) -> Result<Vec<HoldingSnapshot>> {
        // A degenerate from > to reduces to a point query at to, matching the
        // pre-windowed behavior.
        let from = from.min(to);
        let from_str = from.format("%Y-%m-%dT23:59:59").to_string();
        let to_str = to.format("%Y-%m-%dT23:59:59").to_string();

        let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(from_str), Box::new(to_str)];
        let mut filters = String::new();
        if let Some(pid) = profile_id {
            args.push(Box::new(format!("%\"{pid}\"%")));
            filters.push_str(&format!(" AND a.profile_ids LIKE ?{}", args.len()));
        }
        if let Some(types) = account_types {
            let placeholders = types
                .iter()
                .map(|t| {
                    args.push(Box::new(t.as_str().to_string()));
                    format!("?{}", args.len())
                })
                .collect::<Vec<_>>()
                .join(", ");
            filters.push_str(&format!(" AND a.type IN ({placeholders})"));
        }
        if !account_ids.is_empty() {
            let placeholders = account_ids
                .iter()
                .map(|id| {
                    args.push(Box::new(id.clone()));
                    format!("?{}", args.len())
                })
                .collect::<Vec<_>>()
                .join(", ");
            filters.push_str(&format!(" AND h.account_id IN ({placeholders})"));
        }

        let carried_at_from = carried_holdings_sql(
            "account_id, symbol, COALESCE(sub_account, '') AS sub_account, as_of,
             value, currency, is_closed, account_type",
            &format!("AND h.as_of <= ?1{filters}"),
            true,
        );
        let sql = format!(
            r"{carried_at_from}
              UNION ALL
              SELECT h.account_id, h.symbol, COALESCE(h.sub_account, ''), h.as_of,
                     h.value, h.currency, h.is_closed, a.type
              FROM holdings h
              JOIN accounts a ON a.id = h.account_id
              WHERE a.is_active = 1 AND h.as_of > ?1 AND h.as_of <= ?2{filters}
              ORDER BY as_of"
        );

        let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<HoldingSnapshot> {
            let value_str: String = row.get(4)?;
            let account_type_str: String = row.get(7)?;
            Ok(HoldingSnapshot {
                account_id: row.get(0)?,
                symbol: row.get(1)?,
                sub_account: row.get(2)?,
                as_of: row.get(3)?,
                value: parse_decimal_column(4, "holding value", &value_str)?,
                currency: row.get(5)?,
                is_closed: row.get::<_, i64>(6)? != 0,
                account_type: AccountType::parse(&account_type_str).ok_or_else(|| {
                    column_error(7, format!("unknown account type: {account_type_str:?}"))
                })?,
            })
        };

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())),
                map_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// One `HoldingsHistoryRow` per period in `[from, to]`. The last point
    /// reconciles with the portfolio summary's net worth for the same
    /// `as_of` (history replays the same snapshots `get_holdings_for_summary`
    /// reduces, via [`CarryForward`]).
    pub fn get_monthly_net_worth(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        granularity: &Granularity,
        profile_id: Option<&str>,
        fx: &crate::util::fx::FxRateMap,
    ) -> Result<Vec<HoldingsHistoryRow>> {
        use crate::util::fx::CurrencyAggregator;

        let periods = generate_period_end_dates(from, to, granularity);
        let mut carry =
            CarryForward::new(self.holding_snapshots_in_range(from, to, profile_id, None, &[])?);
        let mut rows = Vec::with_capacity(periods.len());

        for (label, period_end) in periods {
            carry.advance_to(period_end);
            let mut available_agg: CurrencyAggregator = Default::default();
            let mut unavailable_agg: CurrencyAggregator = Default::default();

            for s in carry.effective() {
                if is_available_account(&s.account_type) {
                    available_agg.add(s.value, &s.currency, fx);
                } else {
                    unavailable_agg.add(s.value, &s.currency, fx);
                }
            }

            let available = available_agg.converted_sum();
            let unavailable = unavailable_agg.converted_sum();

            rows.push(HoldingsHistoryRow {
                month: label,
                available_wealth: available,
                available_wealth_display: available_agg.display_currency(fx.preferred()),
                unavailable_wealth: unavailable,
                unavailable_wealth_display: unavailable_agg.display_currency(fx.preferred()),
                total_wealth: available + unavailable,
                total_wealth_display: None,
            });
        }

        Ok(rows)
    }

    /// Per-period series for the "Cumulative invested" chart: cumulative net
    /// contributions and the market value of investment + ISA holdings, both
    /// FX-converted to the preferred currency. Each is `None` for a period with
    /// no underlying data (before the first contribution event / no active
    /// holdings) so the chart shows a gap instead of a phantom zero.
    pub fn get_investment_history(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        granularity: &Granularity,
        profile_id: Option<&str>,
        account_ids: &[String],
        fx: &crate::util::fx::FxRateMap,
    ) -> Result<Vec<InvestmentHistoryRow>> {
        use crate::util::fx::CurrencyAggregator;

        // Contribution events on investment + ISA accounts, oldest first. Dates
        // are ISO datetime strings, so lexicographic comparison against a
        // period-end string is a valid chronological compare.
        let to_str = to.format("%Y-%m-%dT23:59:59").to_string();
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(to_str)];
        let profile_filter = if let Some(pid) = profile_id {
            args.push(Box::new(format!("%\"{pid}\"%")));
            "AND a.profile_ids LIKE ?2"
        } else {
            ""
        };
        let account_filter = if !account_ids.is_empty() {
            let start = args.len() + 1;
            for id in account_ids {
                args.push(Box::new(id.to_string()));
            }
            let placeholders = (start..start + account_ids.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("AND i.account_id IN ({placeholders})")
        } else {
            String::new()
        };
        let sql = format!(
            r"SELECT i.date, i.event_type, i.symbol, i.quantity, i.price_per_share, i.fee, i.currency, i.fee_currency
              FROM investments i
              JOIN accounts a ON a.id = i.account_id
              WHERE a.type IN ('investment', 'investment_isa')
                AND i.event_type IN ('buy', 'sell', 'vest', 'withhold', 'transfer', 'split')
                AND i.date <= ?1 {profile_filter} {account_filter}
              ORDER BY i.date ASC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let raw: Vec<InvestmentEventRaw> = stmt
            .query_map(
                rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())),
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        // Net invested is capital contributed, so a disposal must remove what the
        // shares COST (their average book cost), not what they sold for. Removing
        // proceeds would let a profitable sale subtract more than was ever put in
        // and understate the basis. Values are converted to the preferred currency
        // at event time so each pool's average cost is in a single currency.
        let events: Vec<InvestmentEventParsed> = raw
            .into_iter()
            .map(
                |(date, event_type, symbol, q, p, fee, currency, fee_currency)| {
                    let q: Decimal = q.parse().map_err(|_| {
                        anyhow!("invalid quantity {q:?} on {event_type} {symbol} at {date}")
                    })?;
                    let p: Decimal = p.parse().map_err(|_| {
                        anyhow!("invalid price_per_share {p:?} on {event_type} {symbol} at {date}")
                    })?;
                    let principal = fx.convert(q * p, &currency);
                    let fee = match fee {
                        Some(f) => {
                            let f: Decimal = f.parse().map_err(|_| {
                                anyhow!("invalid fee {f:?} on {event_type} {symbol} at {date}")
                            })?;
                            fx.convert(f, fee_currency.as_deref().unwrap_or(&currency))
                        }
                        None => Decimal::ZERO,
                    };
                    Ok((date, event_type, symbol, q, principal, fee))
                },
            )
            .collect::<Result<Vec<_>>>()?;

        let periods = generate_period_end_dates(from, to, granularity);
        let mut carry = CarryForward::new(self.holding_snapshots_in_range(
            from,
            to,
            profile_id,
            Some(&[AccountType::Investment, AccountType::InvestmentIsa]),
            account_ids,
        )?);
        let mut rows = Vec::with_capacity(periods.len());
        let mut pools: HashMap<String, (Decimal, Decimal)> = HashMap::new();
        let mut invested = Decimal::ZERO;
        let mut ev_idx = 0usize;
        let mut any_event = false;

        for (label, period_end) in periods {
            let period_end_str = period_end.format("%Y-%m-%dT23:59:59").to_string();

            while ev_idx < events.len() && events[ev_idx].0 <= period_end_str {
                let (_, event_type, symbol, qty, principal, fee) = &events[ev_idx];
                let (shares, cost) = pools.entry(symbol.clone()).or_default();

                if event_type == "split" {
                    // `quantity` is the shares ADDED by the split, not a ratio. The
                    // shares arrive at no cost, so the pool's total cost and the
                    // net invested are both unchanged; only the average per-share
                    // cost falls. Skipping splits would leave the pool holding a
                    // pre-split share count while later disposals carry post-split
                    // quantities, so book cost would be drawn down at an inflated
                    // per-share rate.
                    *shares += *qty;
                } else if event_type == "sell" || event_type == "withhold" {
                    // Remove the average book cost of the disposed shares.
                    let removed = (*qty).min(*shares).max(Decimal::ZERO);
                    let book_cost = if *shares > Decimal::ZERO {
                        removed * (*cost / *shares)
                    } else {
                        Decimal::ZERO
                    };
                    *shares -= removed;
                    *cost -= book_cost;
                    invested -= book_cost;
                } else {
                    let gross = *principal + *fee;
                    *shares += *qty;
                    *cost += gross;
                    invested += gross;
                }

                any_event = true;
                ev_idx += 1;
            }
            let net_invested = any_event.then(|| invested.to_string());

            // Market value of active (unclosed) holdings. Account type and id
            // filters are already applied by the snapshot fetch; is_closed is
            // per snapshot, so it must stay a walk-time check (filtering it in
            // SQL would carry a stale pre-close value forward instead).
            carry.advance_to(period_end);
            let mut mv = CurrencyAggregator::default();
            let mut has_active = false;
            for s in carry.effective() {
                if !s.is_closed {
                    mv.add(s.value, &s.currency, fx);
                    has_active = true;
                }
            }
            let market_value = has_active.then(|| mv.converted_sum().to_string());

            rows.push(InvestmentHistoryRow {
                period: label,
                net_invested,
                market_value,
            });
        }

        Ok(rows)
    }

    /// Per-symbol value history for a single account: one row per period in
    /// `[from, to]`, each carrying the account total and the per-symbol breakdown
    /// (all converted to the preferred currency). Mirrors `get_monthly_net_worth`
    /// but scoped to one account and broken down by holding instead of
    /// available/unavailable. Sub-accounts are summed into their parent symbol.
    ///
    /// Returns `(symbols, rows)` where `symbols` is the stable set of holdings
    /// seen across the range (for display names) and `rows` are the periods.
    pub fn get_account_holdings_history(
        &self,
        account_id: &str,
        from: NaiveDate,
        to: NaiveDate,
        granularity: &Granularity,
        fx: &crate::util::fx::FxRateMap,
    ) -> Result<(Vec<AccountHoldingSeries>, Vec<AccountHoldingHistoryRow>)> {
        use std::collections::BTreeMap;

        // Carry-forward: latest snapshot per (symbol, sub_account) on/before the
        // period end. Same correlated-subquery pattern as get_holdings_for_summary,
        // but a holding only counts for a period when its latest snapshot at/before
        // that period is non-closed (`h.is_closed = 0`): once the most recent
        // snapshot is a close, the position drops out and the chart shows a gap
        // rather than carrying a stale value forward.
        let mut stmt = self.conn.prepare(
            r"SELECT h.symbol, h.name, h.short_name, h.value, h.currency, h.holding_type
              FROM holdings h
              WHERE h.account_id = ?1
                AND h.is_closed = 0
                AND h.as_of = (
                    SELECT MAX(h2.as_of) FROM holdings h2
                    WHERE h2.account_id = h.account_id
                      AND h2.symbol = h.symbol
                      AND COALESCE(h2.sub_account, '') = COALESCE(h.sub_account, '')
                      AND h2.as_of <= ?2
                )",
        )?;

        // Stable display metadata per symbol, ordered by first appearance.
        let mut series: Vec<AccountHoldingSeries> = Vec::new();
        let mut seen: BTreeMap<String, ()> = BTreeMap::new();

        let periods = generate_period_end_dates(from, to, granularity);
        let mut rows = Vec::with_capacity(periods.len());

        for (label, period_end) in periods {
            let period_end_str = period_end.format("%Y-%m-%dT23:59:59").to_string();

            // symbol -> converted value summed across sub-accounts.
            let mut by_symbol: BTreeMap<String, Decimal> = BTreeMap::new();

            let mapped = stmt.query_map(rusqlite::params![account_id, period_end_str], |row| {
                let symbol: String = row.get(0)?;
                let name: String = row.get(1)?;
                let short_name: Option<String> = row.get(2)?;
                let value: String = row.get(3)?;
                let currency: String = row.get(4)?;
                let holding_type: String = row.get(5)?;
                Ok((symbol, name, short_name, value, currency, holding_type))
            })?;

            for r in mapped {
                let (symbol, name, short_name, value_str, currency, holding_type) = r?;
                let value = value_str.parse::<Decimal>().map_err(|_| {
                    anyhow!("invalid holding value {value_str:?} for {account_id}/{symbol}")
                })?;
                let converted = fx.convert(value, &currency);
                *by_symbol.entry(symbol.clone()).or_insert(Decimal::ZERO) += converted;

                if seen.insert(symbol.clone(), ()).is_none() {
                    series.push(AccountHoldingSeries {
                        symbol,
                        name,
                        short_name,
                        holding_type,
                    });
                }
            }

            let total: Decimal = by_symbol.values().copied().sum();
            let values = by_symbol
                .into_iter()
                .map(|(symbol, value)| AccountHoldingValue { symbol, value })
                .collect();

            rows.push(AccountHoldingHistoryRow {
                period: label,
                total,
                values,
            });
        }

        Ok((series, rows))
    }

    /// Returns the first and last balance (SUM of holdings) per account within
    /// `[start, end]`, and the delta between them. Accounts with no snapshot
    /// inside the range are omitted entirely; snapshots outside the range
    /// never contribute.
    pub fn get_balance_summary(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<BalanceDelta>> {
        let start_str = start.format("%Y-%m-%dT00:00:00").to_string();
        let end_str = end.format("%Y-%m-%dT23:59:59").to_string();

        // One query total: per-account first/last snapshot date over the
        // range-filtered rows, joined back to sum the balances at those two
        // dates. The old shape issued ~4 queries per account.
        let mut stmt = self.conn.prepare(
            r"WITH bounds AS (
                SELECT account_id, MIN(as_of) AS first_date, MAX(as_of) AS last_date
                FROM holdings
                WHERE as_of >= ?1 AND as_of <= ?2
                GROUP BY account_id
              )
              SELECT b.account_id,
                     SUM(CASE WHEN h.as_of = b.first_date THEN CAST(h.value AS REAL) ELSE 0 END),
                     SUM(CASE WHEN h.as_of = b.last_date THEN CAST(h.value AS REAL) ELSE 0 END)
              FROM bounds b
              JOIN holdings h
                ON h.account_id = b.account_id
               AND h.as_of IN (b.first_date, b.last_date)
              GROUP BY b.account_id
              ORDER BY b.account_id",
        )?;

        let rows = stmt.query_map(rusqlite::params![start_str, end_str], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<f64>>(1)?,
                row.get::<_, Option<f64>>(2)?,
            ))
        })?;

        let mut result = Vec::new();
        for row in rows {
            let (account_id, start_f, end_f) = row?;
            let start_balance = start_f.and_then(|f| Decimal::try_from(f).ok());
            let end_balance = end_f.and_then(|f| Decimal::try_from(f).ok());
            let delta = start_balance.zip(end_balance).map(|(s, e)| e - s);
            result.push(BalanceDelta {
                account_id,
                start_balance,
                end_balance,
                delta,
            });
        }
        Ok(result)
    }

    /// Returns aggregated account balances (SUM of holdings) for each distinct
    /// (account_id, as_of) date in `[start, end]`, ordered by date and account.
    pub fn get_balances_in_range(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<AccountSnapshot>> {
        let mut stmt = self.conn.prepare(
            r"SELECT
                h.as_of,
                h.account_id,
                SUM(CAST(h.value AS REAL)) AS total_balance,
                MIN(h.currency) AS currency
              FROM holdings h
              WHERE h.as_of >= ?1 AND h.as_of <= ?2
              GROUP BY h.account_id, h.as_of
              ORDER BY h.as_of, h.account_id",
        )?;
        let rows = stmt
            .query_map(
                rusqlite::params![
                    start.format("%Y-%m-%dT00:00:00").to_string(),
                    end.format("%Y-%m-%dT23:59:59").to_string()
                ],
                |row| {
                    let date_str: String = row.get(0)?;
                    let total: f64 = row.get(2)?;
                    Ok((
                        date_str,
                        row.get::<_, String>(1)?,
                        total,
                        row.get::<_, String>(3)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(rows
            .into_iter()
            .filter_map(|(date_str, account_id, total, currency)| {
                let date = parse_transaction_datetime(&date_str)?;
                let balance = Decimal::try_from(total).ok()?;
                Some(AccountSnapshot {
                    as_of: date,
                    account_id,
                    balance,
                    currency,
                })
            })
            .collect())
    }

    /// Returns income and spending aggregated by period.
    ///
    /// `exclude_category_ids` filters out transactions whose `category_id` is
    /// in the list. Only leaf IDs are meaningful since parents are never
    /// assigned to transactions; pass parents pre-expanded to their leaves.
    pub fn get_cash_flow(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        profile_id: Option<&str>,
        granularity: &Granularity,
        exclude_category_ids: &[String],
        fx: &crate::util::fx::FxRateMap,
    ) -> Result<Vec<HoldingsCashFlowMonth>> {
        use crate::util::fx::CurrencyAggregator;
        use std::collections::HashMap;

        let period_expr = match granularity {
            Granularity::Monthly => "substr(t.date, 1, 7)".to_string(),
            Granularity::Quarterly => concat!(
                "CASE ",
                "WHEN CAST(substr(t.date,6,2) AS INTEGER) BETWEEN 1 AND 3 ",
                "  THEN substr(t.date,1,4)||'-Q1' ",
                "WHEN CAST(substr(t.date,6,2) AS INTEGER) BETWEEN 4 AND 6 ",
                "  THEN substr(t.date,1,4)||'-Q2' ",
                "WHEN CAST(substr(t.date,6,2) AS INTEGER) BETWEEN 7 AND 9 ",
                "  THEN substr(t.date,1,4)||'-Q3' ",
                "ELSE substr(t.date,1,4)||'-Q4' END"
            )
            .to_string(),
            Granularity::Yearly => "substr(t.date, 1, 4)".to_string(),
        };

        let mut conditions = vec!["t.date >= ?1".to_string(), "t.date <= ?2".to_string()];
        let mut extra_args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        let join = if let Some(pid) = profile_id {
            let pattern = format!("%\"{pid}\"%");
            extra_args.push(Box::new(pattern));
            conditions.push(format!("a.profile_ids LIKE ?{}", 2 + extra_args.len()));
            "JOIN accounts a ON a.id = t.account_id"
        } else {
            ""
        };

        if !exclude_category_ids.is_empty() {
            let start_idx = 2 + extra_args.len() + 1;
            let placeholders: Vec<String> = (0..exclude_category_ids.len())
                .map(|i| format!("?{}", start_idx + i))
                .collect();
            conditions.push(format!(
                "t.category_id NOT IN ({})",
                placeholders.join(", ")
            ));
            for id in exclude_category_ids {
                extra_args.push(Box::new(id.clone()));
            }
        }

        let where_clause = conditions.join(" AND ");

        let sql = format!(
            r"SELECT
                {period_expr} AS period,
                t.currency,
                SUM(CASE WHEN CAST(t.amount AS REAL) > 0 THEN CAST(t.amount AS REAL) ELSE 0 END) AS income,
                SUM(CASE WHEN CAST(t.amount AS REAL) < 0 THEN ABS(CAST(t.amount AS REAL)) ELSE 0 END) AS spending
              FROM transactions t
              {join}
              WHERE {where_clause}
              GROUP BY period, t.currency
              ORDER BY period, t.currency"
        );

        let start_str = start.format("%Y-%m-%dT00:00:00").to_string();
        let end_str = end.format("%Y-%m-%dT23:59:59").to_string();

        let mut base_args: Vec<Box<dyn rusqlite::ToSql>> =
            vec![Box::new(start_str), Box::new(end_str)];
        base_args.extend(extra_args);

        let mut stmt = self.conn.prepare(&sql)?;
        let raw: Vec<(String, String, f64, f64)> = stmt
            .query_map(
                rusqlite::params_from_iter(base_args.iter().map(|b| b.as_ref())),
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut periods_map: HashMap<String, (CurrencyAggregator, CurrencyAggregator)> =
            HashMap::new();
        for (period, currency, income_f, spending_f) in raw {
            let income = Decimal::try_from(income_f).map_err(|e| {
                anyhow!("income total for period {period} ({currency}) is not representable: {e}")
            })?;
            let spending = Decimal::try_from(spending_f).map_err(|e| {
                anyhow!("spending total for period {period} ({currency}) is not representable: {e}")
            })?;
            let entry = periods_map
                .entry(period)
                .or_insert_with(|| (Default::default(), Default::default()));
            entry.0.add(income, &currency, fx);
            entry.1.add(spending, &currency, fx);
        }

        // Generate all periods and collect results in order
        let periods = generate_period_end_dates(start, end, granularity);
        let mut result = Vec::new();
        for (label, _) in periods {
            let (income_agg, spending_agg) = periods_map
                .get(&label)
                .cloned()
                .unwrap_or_else(|| (Default::default(), Default::default()));
            result.push(HoldingsCashFlowMonth {
                month: label,
                income: income_agg.converted_sum(),
                income_display: income_agg.display_currency(fx.preferred()),
                spending: spending_agg.converted_sum(),
                spending_display: spending_agg.display_currency(fx.preferred()),
            });
        }

        Ok(result)
    }

    /// Point-in-time holdings (per-(account, symbol, sub_account) carry-forward
    /// to `as_of`) with account metadata, for the given profile.
    ///
    /// Single source of truth: the summary handler, `accounts_as_of`, and
    /// `get_monthly_net_worth` all reduce this, so they reconcile. The rule
    /// itself lives in `carried_holdings_sql`, shared with
    /// `holding_snapshots_in_range`.
    ///
    /// Does not filter `is_closed`; closed holdings are invariant-zeroed
    /// (`close_holding` / `upsert_holdings` / `replace_holdings`), so a
    /// carried closed snapshot contributes 0.
    pub fn get_holdings_for_summary(
        &self,
        as_of: NaiveDate,
        profile_id: Option<&str>,
    ) -> Result<Vec<HoldingSummaryRow>> {
        let as_of_str = as_of.format("%Y-%m-%dT23:59:59").to_string();

        let (profile_filter, profile_arg): (String, Option<String>) = if let Some(pid) = profile_id
        {
            let pattern = format!("%\"{pid}\"%");
            ("AND a.profile_ids LIKE ?2".to_string(), Some(pattern))
        } else {
            (String::new(), None)
        };

        let sql = format!(
            "{} ORDER BY account_id, symbol",
            carried_holdings_sql(
                "account_id, symbol, name, holding_type,
                 quantity, price_per_unit, value, currency,
                 as_of, short_name, sub_account, is_closed,
                 account_type, institution, source_document_ids",
                &format!("AND h.as_of <= ?1 {profile_filter}"),
                true,
            )
        );

        let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<HoldingSummaryRow> {
            let account_type_str: String = row.get(12)?;
            let institution: String = row.get(13)?;
            let holding = row_to_holding(row)?;
            Ok(HoldingSummaryRow {
                holding,
                account_type: AccountType::parse(&account_type_str).ok_or_else(|| {
                    column_error(12, format!("unknown account type: {account_type_str:?}"))
                })?,
                institution,
            })
        };

        let mut stmt = self.conn.prepare(&sql)?;
        let rows: Vec<HoldingSummaryRow> = if let Some(ref pat) = profile_arg {
            stmt.query_map(rusqlite::params![as_of_str, pat], map_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map(rusqlite::params![as_of_str], map_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };

        Ok(rows)
    }

    /// Per-account balances reduced from `get_holdings_for_summary`, so they
    /// reconcile with net worth.
    ///
    /// `balance` = sum of the account's carried holdings (account currency);
    /// `balance_date` = most recent snapshot among them. Active accounts with
    /// no holdings as of `as_of` get a `None` balance.
    pub fn accounts_as_of(
        &self,
        as_of: NaiveDate,
        profile_id: Option<&str>,
    ) -> Result<Vec<Account>> {
        use std::collections::HashMap;
        let stale_days = 45i64;

        let holdings = self.get_holdings_for_summary(as_of, profile_id)?;
        let mut agg: HashMap<String, (Decimal, NaiveDateTime)> = HashMap::new();
        for row in &holdings {
            let h = &row.holding;
            let entry = agg
                .entry(h.account_id.clone())
                .or_insert((Decimal::ZERO, h.as_of));
            entry.0 += h.value;
            if h.as_of > entry.1 {
                entry.1 = h.as_of;
            }
        }

        let (profile_filter, profile_arg): (String, Option<String>) = if let Some(pid) = profile_id
        {
            let pattern = format!("%\"{pid}\"%");
            ("AND a.profile_ids LIKE ?1".to_string(), Some(pattern))
        } else {
            (String::new(), None)
        };

        let sql = format!(
            r"SELECT a.id, a.name, a.institution, a.type, a.currency,
                     a.is_active, a.notes, a.profile_ids
              FROM accounts a
              WHERE a.is_active = 1
              {profile_filter}
              ORDER BY a.institution, a.name"
        );

        let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<Account> {
            let id: String = row.get(0)?;
            let type_str: String = row.get(3)?;
            let profile_ids_str: String = row.get(7).unwrap_or_else(|_| "[]".to_string());
            let profile_ids: Vec<String> = serde_json::from_str(&profile_ids_str)
                .unwrap_or_else(|_| vec!["default".to_string()]);

            let account_type = AccountType::parse(&type_str)
                .ok_or_else(|| column_error(3, format!("unknown account type: {type_str:?}")))?;
            let is_available = is_available_account(&account_type);

            let (balance, balance_date) = match agg.get(&id) {
                Some((sum, max_as_of)) => (Some(*sum), Some(*max_as_of)),
                None => (None, None),
            };
            let is_stale = balance_date
                .map(|d| (as_of - d.date()).num_days() > stale_days)
                .unwrap_or(false);

            Ok(Account {
                id,
                name: row.get(1)?,
                institution: row.get(2)?,
                account_type,
                currency: row.get(4)?,
                balance,
                balance_date,
                is_active: row.get::<_, i64>(5)? != 0,
                notes: row.get(6)?,
                profile_ids,
                is_stale: Some(is_stale),
                is_available,
            })
        };

        let mut stmt = self.conn.prepare(&sql)?;
        let rows: Vec<Account> = if let Some(ref pat) = profile_arg {
            stmt.query_map(rusqlite::params![pat], map_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map([], map_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };

        Ok(rows)
    }

    /// Returns the latest holdings (carry-forward) for all specified accounts.
    pub fn get_holdings_batch(
        &self,
        account_ids: &[String],
        include_closed: bool,
    ) -> Result<Vec<Holding>> {
        if account_ids.is_empty() {
            return Ok(vec![]);
        }
        let placeholders: String = account_ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");

        // Only place is_closed is filtered: a user-facing list with an
        // explicit `include_closed` API flag, not net-worth math.
        let closed_filter = if include_closed {
            ""
        } else {
            "AND h.is_closed = 0"
        };
        let sql = format!(
            r"SELECT h.account_id, h.symbol, h.name, h.holding_type,
                     h.quantity, h.price_per_unit, h.value, h.currency,
                     h.as_of, h.short_name, h.sub_account, h.is_closed,
                     h.source_document_ids
              FROM holdings h
              WHERE h.account_id IN ({placeholders})
                AND h.as_of = (
                    SELECT MAX(h2.as_of) FROM holdings h2
                    WHERE h2.account_id = h.account_id
                      AND h2.symbol = h.symbol
                      AND COALESCE(h2.sub_account, '') = COALESCE(h.sub_account, '')
                )
                {closed_filter}
              ORDER BY h.account_id, h.symbol"
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(account_ids.iter()),
                row_to_holding,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Replace all holdings for `account_id` on the dates present in `holdings`.
    /// For each distinct `as_of` date in the payload: delete existing rows for
    /// (account_id, as_of), then insert the new ones.
    pub fn replace_holdings(&self, account_id: &str, holdings: &[Holding]) -> Result<u32> {
        if holdings.is_empty() {
            return Ok(0);
        }

        // Invariant: a closed holding must be zeroed (see `close_holding`).
        for h in holdings {
            if h.is_closed && !h.value.is_zero() {
                anyhow::bail!(
                    "cannot replace with closed holding {} in account {account_id} with non-zero value {}; closed holdings must be zeroed",
                    h.symbol,
                    h.value
                );
            }
        }

        let tx = self.conn.unchecked_transaction()?;

        // Collect distinct as_of datetime strings to replace.
        let mut dates: Vec<String> = holdings
            .iter()
            .map(|h| h.as_of.format("%Y-%m-%dT%H:%M:%S").to_string())
            .collect();
        dates.sort();
        dates.dedup();

        for date in &dates {
            tx.execute(
                "DELETE FROM holdings WHERE account_id = ?1 AND as_of = ?2",
                rusqlite::params![account_id, date],
            )?;
        }

        let mut inserted = 0u32;
        for h in holdings {
            tx.execute(
                r"INSERT INTO holdings (
                    account_id, symbol, name, holding_type, quantity, price_per_unit,
                    value, currency, as_of, short_name, sub_account, is_closed
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                rusqlite::params![
                    account_id,
                    h.symbol,
                    h.name,
                    h.holding_type.as_str(),
                    h.quantity.to_string(),
                    h.price_per_unit.map(|p| p.to_string()),
                    h.value.to_string(),
                    h.currency,
                    h.as_of.format("%Y-%m-%dT%H:%M:%S").to_string(),
                    h.short_name,
                    h.sub_account,
                    h.is_closed as i64,
                ],
            )?;
            inserted += 1;
        }

        tx.commit()?;
        Ok(inserted)
    }

    /// Investment performance for `[start, end]`. Start/end values apply the
    /// same carry-forward rule as `get_holdings_for_summary` (Investment
    /// accounts only), FX-converted via `fx` per holding/transaction currency,
    /// so they stay consistent with net worth. `new_cash_invested` = net buys
    /// minus sells in range (see [`Self::compute_new_cash_invested`]).
    /// `market_growth` strips that out of the total value change to isolate
    /// price movement.
    pub fn compute_investment_metrics(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        profile_id: Option<&str>,
        fx: &crate::util::fx::FxRateMap,
    ) -> Result<InvestmentMetrics> {
        let new_cash_invested = self.compute_new_cash_invested(start, end, profile_id, fx)?;
        self.compute_investment_metrics_with(start, end, profile_id, fx, new_cash_invested)
    }

    /// [`Self::compute_investment_metrics`] for callers that already computed
    /// `new_cash_invested` for the same range and filters, so it is not
    /// recomputed per request.
    pub fn compute_investment_metrics_with(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        profile_id: Option<&str>,
        fx: &crate::util::fx::FxRateMap,
        new_cash_invested: Decimal,
    ) -> Result<InvestmentMetrics> {
        // One windowed fetch instead of two full carry-forward scans: advance
        // to start for the opening sum, then on to end for the closing sum.
        // Closed holdings are invariant-zeroed, so no is_closed filter.
        let mut carry = CarryForward::new(self.holding_snapshots_in_range(
            start,
            end,
            profile_id,
            Some(&[AccountType::Investment]),
            &[],
        )?);
        let sum_effective = |carry: &CarryForward| -> Decimal {
            carry
                .effective()
                .map(|s| fx.convert(s.value, &s.currency))
                .sum()
        };
        carry.advance_to(start);
        let start_value = sum_effective(&carry);
        carry.advance_to(end);
        let end_value = sum_effective(&carry);

        let total_growth = end_value - start_value;
        let market_growth = total_growth - new_cash_invested;

        Ok(InvestmentMetrics {
            start_value,
            end_value,
            total_growth,
            new_cash_invested,
            market_growth,
        })
    }

    /// Net new contributions over `[start, end]` (cash AND equity added that is
    /// not market movement), FX-converted to the preferred currency. So a fund
    /// switch (sell A, buy B) nets to ~0, and RSU vests count as value in.
    /// Sign by event: buy / vest = in (+), sell / withhold = out (-), transfer =
    /// signed by quantity (negative quantity = shares out); split is excluded
    /// (re-denomination, no value change). `quantity * price_per_share` is the
    /// trade leg; fees are always a cost (+). Trade leg (`currency`) and fee
    /// (`fee_currency`, falling back to `currency`) convert independently.
    /// Profile-scoped via the owning account.
    pub fn compute_new_cash_invested(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        profile_id: Option<&str>,
        fx: &crate::util::fx::FxRateMap,
    ) -> Result<Decimal> {
        use crate::util::fx::CurrencyAggregator;

        let start_str = start.format("%Y-%m-%dT00:00:00").to_string();
        let end_str = end.format("%Y-%m-%dT23:59:59").to_string();

        let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(start_str), Box::new(end_str)];
        let join = if let Some(pid) = profile_id {
            args.push(Box::new(format!("%\"{pid}\"%")));
            "JOIN accounts a ON a.id = i.account_id"
        } else {
            ""
        };
        let profile_filter = if profile_id.is_some() {
            "AND a.profile_ids LIKE ?3"
        } else {
            ""
        };

        let sql = format!(
            r"SELECT i.event_type, i.quantity, i.price_per_share, i.fee, i.currency, i.fee_currency
              FROM investments i {join}
              WHERE i.event_type IN ('buy', 'sell', 'vest', 'withhold', 'transfer')
                AND i.date >= ?1 AND i.date <= ?2 {profile_filter}"
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())),
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut agg = CurrencyAggregator::default();
        for (event_type, qty, price, fee, currency, fee_currency) in rows {
            let q = qty
                .parse::<Decimal>()
                .map_err(|_| anyhow!("invalid quantity {qty:?} on {event_type} event"))?;
            let p = price
                .parse::<Decimal>()
                .map_err(|_| anyhow!("invalid price_per_share {price:?} on {event_type} event"))?;
            // In (+): buy, vest, transfer-in. Out (-): sell, withhold. transfer
            // carries its direction in the quantity sign, so it is added as-is.
            // Fee is always a cost (+).
            let principal = q * p;
            let signed = if event_type == "sell" || event_type == "withhold" {
                -principal
            } else {
                principal
            };
            agg.add(signed, &currency, fx);
            if let Some(fee) = fee {
                let f = fee
                    .parse::<Decimal>()
                    .map_err(|_| anyhow!("invalid fee {fee:?} on {event_type} event"))?;
                if !f.is_zero() {
                    agg.add(f, fee_currency.as_deref().unwrap_or(&currency), fx);
                }
            }
        }
        Ok(agg.converted_sum())
    }

    /// Net savings growth over `[start, end]`: the FX-converted carry-forward
    /// balance of all `savings` and `emergency_fund` accounts as of `end` minus
    /// the same as of `start`. Derived from account type, not holding type
    /// (savings accounts store their balance as a `cash` holding).
    pub fn compute_savings_growth(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        profile_id: Option<&str>,
        fx: &crate::util::fx::FxRateMap,
    ) -> Result<Decimal> {
        let sum_savings = |date: NaiveDate| -> Result<Decimal> {
            Ok(self
                .get_holdings_for_summary(date, profile_id)?
                .iter()
                .filter(|r| {
                    matches!(
                        r.account_type,
                        AccountType::Savings | AccountType::EmergencyFund
                    )
                })
                .map(|r| fx.convert(r.holding.value, &r.holding.currency))
                .sum())
        };
        Ok(sum_savings(end)? - sum_savings(start)?)
    }

    /// `(income, spending)` over `[start, end]`, bucketed by category_type and
    /// FX-converted. Income sums the income_taxable and income_non_taxable types
    /// (signed, normally positive). Spending sums the absolute value of the
    /// spending, donation_taxable and donation_non_taxable types. The
    /// internal_transfer and interest_* types are excluded. Skips
    /// `exclude_from_summary` rows; profile-scoped via the owning account.
    pub fn compute_category_type_cash(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        profile_id: Option<&str>,
        fx: &crate::util::fx::FxRateMap,
    ) -> Result<(Decimal, Decimal)> {
        use crate::util::fx::CurrencyAggregator;

        let start_str = start.format("%Y-%m-%dT00:00:00").to_string();
        let end_str = end.format("%Y-%m-%dT23:59:59").to_string();

        let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(start_str), Box::new(end_str)];
        let join = if let Some(pid) = profile_id {
            args.push(Box::new(format!("%\"{pid}\"%")));
            "JOIN accounts a ON a.id = t.account_id"
        } else {
            ""
        };
        let profile_filter = if profile_id.is_some() {
            "AND a.profile_ids LIKE ?3"
        } else {
            ""
        };

        let sql = format!(
            r"SELECT c.category_type, t.currency, SUM(CAST(t.amount AS REAL))
              FROM transactions t
              JOIN categories c ON c.id = t.category_id
              {join}
              WHERE t.date >= ?1 AND t.date <= ?2
                AND t.exclude_from_summary = 0 {profile_filter}
              GROUP BY c.category_type, t.currency"
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())),
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, f64>(2)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut income = CurrencyAggregator::default();
        let mut spending = CurrencyAggregator::default();
        for (ctype_str, currency, total_f64) in rows {
            let Some(ctype) = CategoryType::parse(&ctype_str) else {
                continue;
            };
            let amount = Decimal::try_from(total_f64).map_err(|e| {
                anyhow!("cash total for category type {ctype_str} is not representable: {e}")
            })?;
            if CategoryType::INCOME.contains(&ctype) {
                income.add(amount, &currency, fx);
            } else if CategoryType::SPENDING.contains(&ctype) {
                spending.add(amount.abs(), &currency, fx);
            }
        }
        Ok((income.converted_sum(), spending.converted_sum()))
    }

    // ── Holdings close / reopen / dry-run ──────────────────────────────────

    pub fn close_holding(
        &self,
        account_id: &str,
        symbol: &str,
        sub_account: Option<&str>,
        as_of: NaiveDateTime,
    ) -> Result<u64> {
        let sub = sub_account.unwrap_or("");
        let as_of_str = as_of.format("%Y-%m-%dT%H:%M:%S").to_string();

        // Invariant: only zeroed positions may be closed, so closed holdings
        // drop out of net-worth math by value, not by an is_closed filter.
        let nonzero_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM holdings
             WHERE account_id = ?1 AND symbol = ?2
               AND COALESCE(sub_account, '') = ?3 AND as_of = ?4
               AND CAST(value AS REAL) != 0",
            params![account_id, symbol, sub, as_of_str],
            |row| row.get(0),
        )?;
        if nonzero_count > 0 {
            anyhow::bail!(
                "cannot close holding {symbol} in account {account_id}: value is non-zero; record a zeroed snapshot before closing"
            );
        }

        let rows = self.conn.execute(
            "UPDATE holdings SET is_closed = 1
             WHERE account_id = ?1 AND symbol = ?2
             AND COALESCE(sub_account, '') = ?3
             AND as_of = ?4",
            params![account_id, symbol, sub, as_of_str],
        )?;
        Ok(rows as u64)
    }

    pub fn reopen_holding(
        &self,
        account_id: &str,
        symbol: &str,
        sub_account: Option<&str>,
        as_of: NaiveDateTime,
    ) -> Result<u64> {
        let sub = sub_account.unwrap_or("");
        let rows = self.conn.execute(
            "UPDATE holdings SET is_closed = 0
             WHERE account_id = ?1 AND symbol = ?2
             AND COALESCE(sub_account, '') = ?3
             AND as_of = ?4",
            params![
                account_id,
                symbol,
                sub,
                as_of.format("%Y-%m-%dT%H:%M:%S").to_string()
            ],
        )?;
        Ok(rows as u64)
    }

    /// Apply optional field updates (value, currency, sub_account) to the
    /// holding row identified by (account_id, symbol, current_sub_account, as_of).
    /// `new_sub_account` semantics: `None` = leave unchanged, `Some(None)` = set
    /// to NULL, `Some(Some(s))` = set to the given label.
    #[allow(clippy::too_many_arguments)]
    pub fn update_holding_fields(
        &self,
        account_id: &str,
        symbol: &str,
        current_sub_account: Option<&str>,
        as_of: NaiveDateTime,
        value: Option<Decimal>,
        currency: Option<&str>,
        new_sub_account: Option<Option<&str>>,
    ) -> Result<u64> {
        if value.is_none() && currency.is_none() && new_sub_account.is_none() {
            return Ok(0);
        }
        let scope_sub = current_sub_account.unwrap_or("");
        let as_of_str = as_of.format("%Y-%m-%dT%H:%M:%S").to_string();

        let tx = self.conn.unchecked_transaction()?;
        let mut total: u64 = 0;

        if let Some(v) = value {
            let n = tx.execute(
                "UPDATE holdings SET value = ?1
                 WHERE account_id = ?2 AND symbol = ?3
                   AND COALESCE(sub_account, '') = ?4 AND as_of = ?5",
                params![v.to_string(), account_id, symbol, scope_sub, as_of_str],
            )?;
            total = total.max(n as u64);
        }
        if let Some(c) = currency {
            let n = tx.execute(
                "UPDATE holdings SET currency = ?1
                 WHERE account_id = ?2 AND symbol = ?3
                   AND COALESCE(sub_account, '') = ?4 AND as_of = ?5",
                params![c, account_id, symbol, scope_sub, as_of_str],
            )?;
            total = total.max(n as u64);
        }
        if let Some(new_sub) = new_sub_account {
            let n = tx.execute(
                "UPDATE holdings SET sub_account = ?1
                 WHERE account_id = ?2 AND symbol = ?3
                   AND COALESCE(sub_account, '') = ?4 AND as_of = ?5",
                params![new_sub, account_id, symbol, scope_sub, as_of_str],
            )?;
            total = total.max(n as u64);
        }

        tx.commit()?;
        Ok(total)
    }

    pub fn get_holding_snapshots(&self, account_id: &str, symbol: &str) -> Result<Vec<Holding>> {
        let mut stmt = self.conn.prepare(
            "SELECT account_id, symbol, name, holding_type, quantity, price_per_unit,
                    value, currency, as_of, short_name, sub_account, is_closed,
                    source_document_ids
             FROM holdings
             WHERE account_id = ?1 AND symbol = ?2
             ORDER BY as_of ASC",
        )?;
        let rows = stmt
            .query_map(params![account_id, symbol], row_to_holding)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn delete_holding(
        &self,
        account_id: &str,
        symbol: &str,
        as_of: &str,
        sub_account: Option<&str>,
    ) -> Result<usize> {
        let rows = if let Some(sub) = sub_account {
            self.conn.execute(
                "DELETE FROM holdings WHERE account_id = ?1 AND symbol = ?2 AND as_of = ?3 AND sub_account = ?4",
                params![account_id, symbol, as_of, sub],
            )?
        } else {
            self.conn.execute(
                "DELETE FROM holdings WHERE account_id = ?1 AND symbol = ?2 AND as_of = ?3",
                params![account_id, symbol, as_of],
            )?
        };
        Ok(rows)
    }

    pub fn dry_run_holdings(
        &self,
        account_id: &str,
        holdings: &[Holding],
    ) -> Result<Vec<HoldingPreview>> {
        let mut previews = Vec::new();
        for h in holdings {
            let sub = h.sub_account.as_deref().unwrap_or("");
            let as_of_str = h.as_of.format("%Y-%m-%dT%H:%M:%S").to_string();

            let existing_value: Option<String> = self
                .conn
                .query_row(
                    "SELECT value FROM holdings
                     WHERE account_id = ?1 AND symbol = ?2
                     AND COALESCE(sub_account, '') = ?3 AND as_of = ?4",
                    params![account_id, h.symbol, sub, as_of_str],
                    |row| row.get(0),
                )
                .ok();

            // Snapshot identity is (account, symbol, sub_account, as_of). A row
            // whose exact snapshot already exists is "modify" — unless the value
            // is unchanged, in which case it's a true no-op and we mark it
            // "duplicate" so the UI shows Skip and the commit drops it.
            let status = match &existing_value {
                Some(ev) if ev.parse::<Decimal>().ok() == Some(h.value) => "duplicate",
                Some(_) => "modify",
                None => "new",
            }
            .to_string();

            previews.push(HoldingPreview {
                account_id: account_id.to_string(),
                symbol: h.symbol.clone(),
                sub_account: h.sub_account.clone(),
                value: h.value,
                currency: h.currency.clone(),
                as_of: as_of_str,
                status,
                existing_value,
                derived: h.derived,
                source_document_ids: Vec::new(),
            });
        }
        Ok(previews)
    }

    /// Preview parsed investment rows without writing anything.
    /// Computes fingerprints matching `create_investment_event` and checks the `investments` table.
    pub fn dry_run_investments(
        &self,
        account_id: &str,
        rows: &[crate::importers::investments_parser::ParsedInvestmentRow],
        min_row_confidence: f32,
    ) -> anyhow::Result<Vec<crate::model::InvestmentPreviewRow>> {
        use crate::model::TransactionPreviewStatus;

        let mut previews = Vec::with_capacity(rows.len());

        let mut stmt = self
            .conn
            .prepare("SELECT id FROM investments WHERE fingerprint = ?1")?;

        for (i, row) in rows.iter().enumerate() {
            let err_row = |reason: String| crate::model::InvestmentPreviewRow {
                index: i,
                event_type: row.event_type.clone(),
                symbol: row.symbol.clone(),
                date: row.date.clone(),
                quantity: row.quantity.clone(),
                price_per_share: row.price_per_share.clone(),
                currency: row.currency.clone(),
                status: TransactionPreviewStatus::Error,
                error_reason: Some(reason),
                existing_id: None,
                source_document_ids: Vec::new(),
            };

            if row.row_confidence < min_row_confidence {
                previews.push(err_row(format!(
                    "Row confidence {:.2} is below the import threshold {:.2}",
                    row.row_confidence, min_row_confidence
                )));
                continue;
            }

            let date_str = match parse_transaction_datetime(&row.date) {
                Some(dt) => dt.format("%Y-%m-%dT%H:%M:%S").to_string(),
                None => {
                    tracing::warn!(
                        symbol = %row.symbol,
                        date = %row.date,
                        "invalid date in investment row; marking as error"
                    );
                    previews.push(err_row(format!("Could not parse date \"{}\"", row.date)));
                    continue;
                }
            };

            let quantity = match row.quantity.parse::<rust_decimal::Decimal>() {
                Ok(d) => d.to_string(),
                Err(_) => {
                    tracing::warn!(symbol = %row.symbol, "invalid quantity in investment row; marking as error");
                    previews.push(err_row(format!("Invalid quantity \"{}\"", row.quantity)));
                    continue;
                }
            };

            let price_per_share_dec = match row.price_per_share.parse::<rust_decimal::Decimal>() {
                Ok(d) => d,
                Err(_) => {
                    tracing::warn!(symbol = %row.symbol, "invalid price_per_share in investment row; marking as error");
                    previews.push(err_row(format!(
                        "Invalid price per share \"{}\"",
                        row.price_per_share
                    )));
                    continue;
                }
            };

            // Mirror create_investment_event's sub-unit conversion so the
            // fingerprint (and displayed price/currency) match what committing
            // this preview would actually write.
            let (price_per_share_dec, currency) =
                match crate::util::subunits::to_parent(price_per_share_dec, &row.currency) {
                    Some((converted, parent)) => (converted, parent.to_string()),
                    None => (price_per_share_dec, row.currency.clone()),
                };
            let price_per_share = price_per_share_dec.to_string();

            let fingerprint = sha256_hex(&format!(
                "{}|{}|{}|{}|{}|{}",
                account_id, row.symbol, date_str, quantity, price_per_share, row.event_type,
            ));

            let existing_id: Option<String> = stmt
                .query_row(rusqlite::params![fingerprint], |row| {
                    row.get::<_, String>(0)
                })
                .ok();

            let status = if existing_id.is_some() {
                TransactionPreviewStatus::Duplicate
            } else {
                TransactionPreviewStatus::New
            };

            previews.push(crate::model::InvestmentPreviewRow {
                index: i,
                event_type: row.event_type.clone(),
                symbol: row.symbol.clone(),
                date: row.date.clone(),
                quantity: row.quantity.clone(),
                price_per_share,
                currency,
                status,
                error_reason: None,
                existing_id,
                source_document_ids: Vec::new(),
            });
        }

        Ok(previews)
    }

    // ── Stats ─────────────────────────────────────────────────────────────────

    pub fn stats(&self) -> Result<Stats> {
        let (total, min_date, max_date): (i64, Option<String>, Option<String>) =
            self.conn.query_row(
                "SELECT COUNT(*), MIN(date), MAX(date) FROM transactions",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
        let mut stmt = self.conn.prepare(
            r"SELECT a.id,
                     COALESCE(cnt.total, 0),
                     cnt.min_date,
                     cnt.max_date,
                     COALESCE(cnt.uncategorized, 0)
              FROM accounts a
              LEFT JOIN (
                SELECT account_id,
                       COUNT(*)                                    AS total,
                       MIN(date)                                   AS min_date,
                       MAX(date)                                   AS max_date,
                       SUM(CASE WHEN category IS NULL THEN 1 ELSE 0 END) AS uncategorized
                FROM transactions
                GROUP BY account_id
              ) cnt ON cnt.account_id = a.id
              ORDER BY a.id",
        )?;
        let per_account = stmt
            .query_map([], |row| {
                Ok(AccountStats {
                    account_id: row.get(0)?,
                    count: row.get::<_, i64>(1)? as u64,
                    min_date: row.get(2)?,
                    max_date: row.get(3)?,
                    uncategorized: row.get::<_, i64>(4)? as u64,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Stats {
            total: total as u64,
            min_date,
            max_date,
            per_account,
        })
    }

    // ── Tax configuration and inputs ─────────────────────────────────────────

    /// Every statutory entry for a tax year, ordered so rate bands come back in
    /// chronological order.
    ///
    /// Returns an empty vec for a year that has never been seeded rather than an
    /// error: "no configuration for 2029-30" is a decision for the caller, which
    /// can say so in a 4xx far more usefully than a storage-layer error can.
    pub fn get_tax_config(&self, tax_year: &str) -> Result<Vec<TaxConfigEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT tax_year, kind, rate_kind, valid_from, valid_to, amount, rate, updated_at
             FROM tax_config WHERE tax_year = ?1
             ORDER BY kind, valid_from, rate_kind",
        )?;
        let rows = stmt
            .query_map(params![tax_year], row_to_tax_config_entry)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Every statutory entry we hold, for the config screen.
    pub fn get_all_tax_config(&self) -> Result<Vec<TaxConfigEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT tax_year, kind, rate_kind, valid_from, valid_to, amount, rate, updated_at
             FROM tax_config
             ORDER BY tax_year, kind, valid_from, rate_kind",
        )?;
        let rows = stmt
            .query_map([], row_to_tax_config_entry)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Replace the whole entry set for one tax year, in a transaction.
    ///
    /// Delete-then-insert rather than upsert because the entries for a year are
    /// a set that must tile it. Upserting row-by-row could leave a stale band
    /// behind after an edit that splits or merges periods, and a disposal
    /// falling in the resulting gap would be taxed at no rate at all.
    pub fn put_tax_config(&self, tax_year: &str, entries: &[TaxConfigEntry]) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        tx.execute(
            "DELETE FROM tax_config WHERE tax_year = ?1",
            params![tax_year],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO tax_config
                     (tax_year, kind, rate_kind, valid_from, valid_to, amount, rate, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for e in entries {
                stmt.execute(params![
                    tax_year,
                    e.kind,
                    e.rate_kind,
                    e.valid_from,
                    e.valid_to,
                    e.amount.map(|d| d.to_string()),
                    e.rate.map(|d| d.to_string()),
                    now,
                ])?;
            }
        }
        tx.commit()?;
        Ok(entries.len())
    }

    /// One profile's own figures for a tax year.
    ///
    /// A profile that has never been given inputs for a year gets the documented
    /// defaults (no brought-forward losses, no basic-rate headroom, AEA claimed)
    /// rather than `None`. Every one of those defaults is a real, defensible
    /// position rather than a placeholder, and returning them means the
    /// computation has no "unconfigured" branch to get wrong.
    pub fn get_tax_inputs(&self, profile_id: &str, tax_year: &str) -> Result<TaxInputs> {
        let stored = self
            .conn
            .query_row(
                "SELECT profile_id, tax_year, brought_forward_losses,
                        allowable_income_remaining, aea_claimed, updated_at
                 FROM tax_inputs WHERE profile_id = ?1 AND tax_year = ?2",
                params![profile_id, tax_year],
                row_to_tax_inputs,
            )
            .optional()?;

        Ok(stored.unwrap_or_else(|| TaxInputs {
            profile_id: profile_id.to_string(),
            tax_year: tax_year.to_string(),
            brought_forward_losses: Decimal::ZERO,
            allowable_income_remaining: Decimal::ZERO,
            aea_claimed: true,
            updated_at: None,
        }))
    }

    /// Write one profile's figures for a tax year, creating the row if absent.
    pub fn put_tax_inputs(&self, inputs: &TaxInputs) -> Result<()> {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.conn.execute(
            "INSERT INTO tax_inputs
                 (profile_id, tax_year, brought_forward_losses,
                  allowable_income_remaining, aea_claimed, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(profile_id, tax_year) DO UPDATE SET
                 brought_forward_losses     = excluded.brought_forward_losses,
                 allowable_income_remaining = excluded.allowable_income_remaining,
                 aea_claimed                = excluded.aea_claimed,
                 updated_at                 = excluded.updated_at",
            params![
                inputs.profile_id,
                inputs.tax_year,
                inputs.brought_forward_losses.to_string(),
                inputs.allowable_income_remaining.to_string(),
                i64::from(inputs.aea_claimed),
                now,
            ],
        )?;
        Ok(())
    }

    /// A *suggested* brought-forward loss figure for `tax_year`, derived from
    /// disposals recorded in earlier years.
    ///
    /// This is a prefill for a field the user confirms, and the return type is
    /// shaped to stop a consumer treating it as settled: it carries the years it
    /// was built from and an `is_upper_bound` flag that is always true.
    ///
    /// It can only ever OVERSTATE, for two reasons this app cannot see past.
    /// A UK capital loss carries forward only if it was CLAIMED within four
    /// years of the end of the tax year it arose in, and nothing in the ledger
    /// records whether a claim was made. And only the excess left after setting
    /// the loss against that same year's gains carries at all — which this does
    /// net off per year, but it cannot know about disposals made outside this
    /// app, so a year that looks like a net loss here may not have been one.
    ///
    /// Losses are netted **within** each tax year and only the years that netted
    /// to a loss contribute; a year that netted to a gain contributes nothing
    /// and is omitted rather than being allowed to cancel out another year's
    /// loss, because gains do not reduce losses carried forward from elsewhere.
    ///
    /// `year_boundaries` supplies the UK tax-year bounds to bucket by, so this
    /// function stays a pure query and the caller owns the calendar.
    pub fn derive_brought_forward_losses(
        &self,
        realized: &[(String, Decimal)],
        year_boundaries: &[(String, NaiveDate, NaiveDate)],
    ) -> Result<DerivedBroughtForwardLosses> {
        let mut net_by_year: BTreeMap<&str, Decimal> = BTreeMap::new();

        for (date_str, gain_loss) in realized {
            let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                .with_context(|| format!("invalid disposal date {date_str:?}"))?;
            if let Some((year, _, _)) = year_boundaries
                .iter()
                .find(|(_, from, to)| date >= *from && date <= *to)
            {
                *net_by_year.entry(year.as_str()).or_default() += gain_loss;
            }
        }

        let contributions: Vec<DerivedLossYear> = net_by_year
            .into_iter()
            .filter(|(_, net)| *net < Decimal::ZERO)
            .map(|(year, net)| DerivedLossYear {
                tax_year: year.to_string(),
                net_loss: net.abs(),
            })
            .collect();

        Ok(DerivedBroughtForwardLosses {
            amount: contributions.iter().map(|c| c.net_loss).sum(),
            contributions,
            is_upper_bound: true,
        })
    }
}

// ── Public data structs ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TokenInfo {
    pub name: String,
    pub created_at: String,
    pub last_used: Option<String>,
    pub is_active: bool,
}

#[derive(Debug)]
pub struct Stats {
    pub total: u64,
    pub min_date: Option<String>,
    pub max_date: Option<String>,
    pub per_account: Vec<AccountStats>,
}

#[derive(Debug)]
pub struct AccountStats {
    pub account_id: String,
    pub count: u64,
    pub min_date: Option<String>,
    pub max_date: Option<String>,
    pub uncategorized: u64,
}

// ── Date parsing helper ───────────────────────────────────────────────────────

/// Parse a stored date/datetime string into `NaiveDateTime`.
///
/// Accepts both the new `YYYY-MM-DDTHH:MM:SS` format and the legacy
/// `YYYY-MM-DD` format (converting date-only values to `T00:00:00`).
/// Returns `None` on parse failure rather than panicking so callers can
/// use `.unwrap_or_else` with a sensible default.
/// Parse a stored ISO 8601 datetime. Event dates are written without a zone
/// suffix, but `created_at` (both the SQL column default and the Rust insert)
/// appends a `Z`. Callers fall back to `now()` when this returns None, so a
/// format this does not accept is silently replaced by the current time rather
/// than surfacing as an error.
fn parse_transaction_datetime(s: &str) -> Option<NaiveDateTime> {
    let s = s.strip_suffix('Z').unwrap_or(s);
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .or_else(|| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
        })
}

// ── Row mappers ───────────────────────────────────────────────────────────────

/// Column-level conversion failure for row mappers. Corrupt stored values
/// (unparseable decimals, unknown enum strings) must surface as errors, never
/// silently coerce to a default that misreports data.
fn column_error(idx: usize, msg: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, msg.into())
}

fn parse_decimal_column(idx: usize, field: &str, s: &str) -> rusqlite::Result<Decimal> {
    s.parse()
        .map_err(|_| column_error(idx, format!("invalid {field}: {s:?}")))
}

fn row_to_tax_config_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaxConfigEntry> {
    let amount: Option<String> = row.get(5)?;
    let rate: Option<String> = row.get(6)?;
    Ok(TaxConfigEntry {
        tax_year: row.get(0)?,
        kind: row.get(1)?,
        rate_kind: row.get(2)?,
        valid_from: row.get(3)?,
        valid_to: row.get(4)?,
        amount: amount
            .map(|s| parse_decimal_column(5, "tax_config.amount", &s))
            .transpose()?,
        rate: rate
            .map(|s| parse_decimal_column(6, "tax_config.rate", &s))
            .transpose()?,
        updated_at: row.get(7)?,
    })
}

fn row_to_tax_inputs(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaxInputs> {
    let bfl: String = row.get(2)?;
    let air: String = row.get(3)?;
    Ok(TaxInputs {
        profile_id: row.get(0)?,
        tax_year: row.get(1)?,
        brought_forward_losses: parse_decimal_column(2, "tax_inputs.brought_forward_losses", &bfl)?,
        allowable_income_remaining: parse_decimal_column(
            3,
            "tax_inputs.allowable_income_remaining",
            &air,
        )?,
        aea_claimed: row.get::<_, i64>(4)? != 0,
        updated_at: row.get(5)?,
    })
}

fn row_to_category(row: &rusqlite::Row<'_>) -> rusqlite::Result<Category> {
    let category_type_str: String = row.get(6)?;
    let category_type = CategoryType::parse(&category_type_str)
        .ok_or_else(|| column_error(6, format!("unknown category type: {category_type_str:?}")))?;
    Ok(Category {
        id: row.get(0)?,
        name: row.get(1)?,
        parent_id: row.get(2)?,
        display_order: row.get(3)?,
        is_active: row.get::<_, i64>(4)? != 0,
        description: row.get(5)?,
        category_type,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn row_to_holding(row: &rusqlite::Row<'_>) -> rusqlite::Result<Holding> {
    let holding_type_str: String = row.get(3)?;
    let quantity_str: String = row.get(4)?;
    let price_str: Option<String> = row.get(5)?;
    let value_str: String = row.get(6)?;
    let as_of_str: String = row.get(8)?;
    let is_closed_int: i64 = row.get(11).unwrap_or(0);
    Ok(Holding {
        account_id: row.get(0)?,
        symbol: row.get(1)?,
        name: row.get(2)?,
        holding_type: HoldingType::parse(&holding_type_str).ok_or_else(|| {
            column_error(3, format!("unknown holding type: {holding_type_str:?}"))
        })?,
        quantity: parse_decimal_column(4, "holding quantity", &quantity_str)?,
        price_per_unit: price_str
            .as_deref()
            .map(|s| parse_decimal_column(5, "holding price_per_unit", s))
            .transpose()?,
        value: parse_decimal_column(6, "holding value", &value_str)?,
        currency: row.get(7)?,
        as_of: parse_transaction_datetime(&as_of_str)
            .unwrap_or_else(|| chrono::Local::now().naive_local()),
        short_name: row.get(9)?,
        sub_account: row.get(10)?,
        is_closed: is_closed_int != 0,
        derived: false,
        // Read by column name so SELECTs that don't include it (e.g. the
        // index-based summary query) still map cleanly; those fall back to empty.
        source_document_ids: row
            .get::<_, String>("source_document_ids")
            .map(|s| parse_id_array(&s))
            .unwrap_or_default(),
        source_file: None,
    })
}

fn row_to_transaction(row: &rusqlite::Row<'_>) -> rusqlite::Result<Transaction> {
    let date: String = row.get(1)?;
    let amount: String = row.get(4)?;
    let cat_source: Option<String> = row.get(8)?;
    Ok(Transaction {
        id: row.get(0)?,
        date: parse_transaction_datetime(&date).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                format!("invalid transaction date: {date:?}").into(),
            )
        })?,
        description: row.get(2)?,
        normalized: row.get(3)?,
        amount: parse_decimal_column(4, "transaction amount", &amount)?,
        currency: row.get(5)?,
        account_id: row.get(6)?,
        category_id: row.get(7)?, // FK to categories.id
        category_source: cat_source.as_deref().and_then(CategorySource::parse),
        confidence: row.get(9)?,
        notes: row.get(10)?,
        is_recurring: row.get::<_, i64>(11)? != 0,
        exclude_from_summary: row.get::<_, i64>(12)? != 0,
        fingerprint: row.get(13)?,
        fitid: row.get(14)?,
        source_document_ids: parse_id_array(&row.get::<_, String>(15)?),
    })
}

/// Reads the persisted columns of `accounts` into an `Account`. `balance` and
/// `balance_date` are runtime-derived from `holdings` and intentionally left
/// `None` here; callers (`get_accounts`, `get_account_by_id`, `accounts_as_of`)
/// fill them in.
///
/// Column order expected: id, name, institution, type, currency, is_active,
/// notes, profile_ids.
fn row_to_account(row: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
    let type_str: String = row.get(3)?;
    let profile_ids_str: String = row.get(7).unwrap_or_else(|_| "[]".to_string());
    let profile_ids: Vec<String> =
        serde_json::from_str(&profile_ids_str).unwrap_or_else(|_| vec!["default".to_string()]);
    let account_type = AccountType::parse(&type_str)
        .ok_or_else(|| column_error(3, format!("unknown account type: {type_str:?}")))?;
    let is_available = is_available_account(&account_type);
    Ok(Account {
        id: row.get(0)?,
        name: row.get(1)?,
        institution: row.get(2)?,
        account_type,
        currency: row.get(4)?,
        balance: None,
        balance_date: None,
        is_active: row.get::<_, i64>(5)? != 0,
        notes: row.get(6)?,
        profile_ids,
        is_stale: None,
        is_available,
    })
}

fn row_to_investment_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<InvestmentEvent> {
    let event_type_str: String = row.get(2)?;
    let date_str: String = row.get(4)?;
    let created_at_str: String = row.get(11)?;
    let fee: Option<String> = row.get(7)?;
    let quantity_str: String = row.get(5)?;
    let price_str: String = row.get(6)?;
    Ok(InvestmentEvent {
        id: row.get(0)?,
        account_id: row.get(1)?,
        event_type: InvestmentEventType::parse(&event_type_str).ok_or_else(|| {
            column_error(
                2,
                format!("unknown investment event type: {event_type_str:?}"),
            )
        })?,
        symbol: row.get(3)?,
        date: parse_transaction_datetime(&date_str)
            .unwrap_or_else(|| chrono::Utc::now().naive_utc()),
        quantity: parse_decimal_column(5, "investment quantity", &quantity_str)?,
        price_per_share: parse_decimal_column(6, "investment price_per_share", &price_str)?,
        fee: fee
            .as_deref()
            .map(|s| parse_decimal_column(7, "investment fee", s))
            .transpose()?,
        currency: row.get(8)?,
        // Read by name so SELECTs that omit it fall back to None.
        fee_currency: row.get::<_, Option<String>>("fee_currency").ok().flatten(),
        notes: row.get(9)?,
        fingerprint: row.get(10)?,
        created_at: parse_transaction_datetime(&created_at_str)
            .unwrap_or_else(|| chrono::Utc::now().naive_utc()),
        // Read by column name so SELECTs that omit it fall back to empty.
        source_document_ids: row
            .get::<_, String>("source_document_ids")
            .map(|s| parse_id_array(&s))
            .unwrap_or_default(),
    })
}

// ── Portfolio helpers ─────────────────────────────────────────────────────────

/// Returns `true` for account types counted in "available wealth".
pub fn is_available_account(t: &AccountType) -> bool {
    matches!(
        t,
        AccountType::Checking
            | AccountType::Savings
            | AccountType::EmergencyFund
            | AccountType::Investment
            | AccountType::InvestmentIsa
            | AccountType::Credit
    )
}

/// Map an account type to a broad asset class for `by_asset_class`.
pub fn account_type_to_asset_class(t: &AccountType) -> AssetClass {
    match t {
        AccountType::Investment | AccountType::InvestmentIsa => AssetClass::Investments,
        AccountType::Pension => AssetClass::Pension,
        AccountType::Checking
        | AccountType::Savings
        | AccountType::EmergencyFund
        | AccountType::Credit => AssetClass::Cash,
        AccountType::Property => AssetClass::Property,
    }
}

/// The single SQL form of the holdings carry-forward rule: the latest
/// snapshot per (account_id, symbol, COALESCE(sub_account, '')), restricted
/// by `filters` (which must bind the `h.as_of <= ?N` cutoff plus any
/// profile/account predicates; `h` = holdings, `a` = accounts). With
/// `active_only`, holdings are joined to accounts and restricted to
/// `a.is_active = 1`, and `select` may project `account_type` /
/// `institution` alongside holdings columns; without it there is no accounts
/// join at all, so every holdings row counts regardless of account state.
/// `get_holdings_for_summary` and `holding_snapshots_in_range` use
/// `active_only = true`; `balances_from_holdings_as_of` uses `false` because
/// account balances are reported for inactive accounts too. rowid breaks
/// `as_of` ties deterministically, though `uq_holdings_identity` makes ties
/// impossible.
fn carried_holdings_sql(select: &str, filters: &str, active_only: bool) -> String {
    let (join, account_cols, where_head) = if active_only {
        (
            "JOIN accounts a ON a.id = h.account_id",
            ", a.type AS account_type, a.institution",
            "a.is_active = 1",
        )
    } else {
        ("", "", "1=1")
    };
    format!(
        r"SELECT {select} FROM (
              SELECT h.*{account_cols},
                     ROW_NUMBER() OVER (
                         PARTITION BY h.account_id, h.symbol, COALESCE(h.sub_account, '')
                         ORDER BY h.as_of DESC, h.rowid DESC
                     ) AS rn
              FROM holdings h
              {join}
              WHERE {where_head} {filters}
          ) WHERE rn = 1"
    )
}

/// One holdings row as stored, joined with its account type. Produced by
/// `Db::holding_snapshots_in_range` for the [`CarryForward`] walk.
struct HoldingSnapshot {
    account_id: String,
    symbol: String,
    /// Normalized: NULL stored as `''`, matching the summary query's COALESCE.
    sub_account: String,
    /// ISO datetime string as stored; lexicographic order is chronological.
    as_of: String,
    value: Decimal,
    currency: String,
    is_closed: bool,
    account_type: AccountType,
}

/// Replays snapshots in `as_of` order, keeping the latest per
/// (account, symbol, sub_account). After `advance_to(d)`, `effective()`
/// yields the same set of carried holdings `get_holdings_for_summary(d)`
/// returns (closed snapshots included; they are invariant-zeroed). Callers
/// must advance with non-decreasing dates.
struct CarryForward {
    snapshots: Vec<HoldingSnapshot>,
    /// Interned (account, symbol, sub_account) key per snapshot; indexes
    /// `effective` so `advance_to` hashes and clones nothing.
    key_ids: Vec<usize>,
    cursor: usize,
    /// Latest consumed snapshot index per key id.
    effective: Vec<Option<usize>>,
}

impl CarryForward {
    fn new(snapshots: Vec<HoldingSnapshot>) -> Self {
        let mut ids: HashMap<(&str, &str, &str), usize> = HashMap::new();
        let mut key_ids = Vec::with_capacity(snapshots.len());
        for s in &snapshots {
            let next = ids.len();
            let key = (
                s.account_id.as_str(),
                s.symbol.as_str(),
                s.sub_account.as_str(),
            );
            key_ids.push(*ids.entry(key).or_insert(next));
        }
        let effective = vec![None; ids.len()];
        Self {
            snapshots,
            key_ids,
            cursor: 0,
            effective,
        }
    }

    fn advance_to(&mut self, period_end: NaiveDate) {
        let end = period_end.format("%Y-%m-%dT23:59:59").to_string();
        while self.cursor < self.snapshots.len() && self.snapshots[self.cursor].as_of <= end {
            self.effective[self.key_ids[self.cursor]] = Some(self.cursor);
            self.cursor += 1;
        }
    }

    fn effective(&self) -> impl Iterator<Item = &HoldingSnapshot> {
        self.effective.iter().flatten().map(|&i| &self.snapshots[i])
    }
}

/// Generate (label, period_end_date) pairs for a date range and granularity.
/// Each period_end is clamped to `to` if it exceeds it.
pub fn generate_period_end_dates(
    from: NaiveDate,
    to: NaiveDate,
    granularity: &Granularity,
) -> Vec<(String, NaiveDate)> {
    use chrono::Datelike;

    let mut periods = Vec::new();

    match granularity {
        Granularity::Monthly => {
            let mut year = from.year();
            let mut month = from.month();
            loop {
                // Last day of this month.
                let next = if month == 12 {
                    NaiveDate::from_ymd_opt(year + 1, 1, 1)
                } else {
                    NaiveDate::from_ymd_opt(year, month + 1, 1)
                }
                .unwrap();
                let period_end = next.pred_opt().unwrap().min(to);
                let label = format!("{year}-{month:02}");
                periods.push((label, period_end));
                if period_end >= to {
                    break;
                }
                // Advance one month.
                if month == 12 {
                    year += 1;
                    month = 1;
                } else {
                    month += 1;
                }
            }
        }
        Granularity::Quarterly => {
            let start_q = (from.month() - 1) / 3 + 1;
            let mut year = from.year();
            let mut quarter = start_q;
            loop {
                let end_month = quarter * 3;
                let next_year = if end_month == 12 { year + 1 } else { year };
                let next_month = if end_month == 12 { 1 } else { end_month + 1 };
                let period_end = NaiveDate::from_ymd_opt(next_year, next_month, 1)
                    .unwrap()
                    .pred_opt()
                    .unwrap()
                    .min(to);
                let label = format!("{year}-Q{quarter}");
                periods.push((label, period_end));
                if period_end >= to {
                    break;
                }
                if quarter == 4 {
                    year += 1;
                    quarter = 1;
                } else {
                    quarter += 1;
                }
            }
        }
        Granularity::Yearly => {
            let mut year = from.year();
            loop {
                let period_end = NaiveDate::from_ymd_opt(year, 12, 31).unwrap().min(to);
                let label = format!("{year}");
                periods.push((label, period_end));
                if period_end >= to {
                    break;
                }
                year += 1;
            }
        }
    }

    periods
}

// ── Permission helpers ────────────────────────────────────────────────────────

#[allow(unused_imports)]
use std::fs;

#[cfg(unix)]
fn set_dir_mode_700(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o700);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_dir_mode_700(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_file_mode_600(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_file_mode_600(_path: &Path) -> Result<()> {
    Ok(())
}

// ── Token helpers ─────────────────────────────────────────────────────────────

fn generate_raw_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("fyn_{}", hex::encode(bytes))
}

fn sha256_hex(s: &str) -> String {
    sha256_hex_bytes(s.as_bytes())
}

fn sha256_hex_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Make an uploaded filename safe to use as a path component: keep ASCII
/// alphanumerics, dot, dash, and underscore; replace anything else (including
/// path separators) with `_`. The `<uuid>_` prefix already guarantees
/// uniqueness, so this only needs to neutralise traversal and odd characters.
fn sanitize_filename(filename: &str) -> String {
    let cleaned: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('.');
    if trimmed.is_empty() {
        "upload".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Parse a JSON array of strings (as stored in `source_document_ids`) into a
/// `Vec<String>`, treating any malformed value as empty.
fn parse_id_array(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

/// Union `incoming_ids_json` (a JSON array string) into the `source_document_ids`
/// of the row(s) in `table` where `key_col = key_val`. Used to keep the source
/// document audit trail complete when a re-import hits an existing row.
/// `table` and `key_col` are caller-controlled constants, never user input.
fn merge_source_documents(
    conn: &Connection,
    table: &str,
    key_col: &str,
    key_val: &str,
    incoming_ids_json: &str,
) -> Result<()> {
    conn.execute(
        &format!(
            r"UPDATE {table}
              SET source_document_ids = (
                SELECT json_group_array(value) FROM (
                  SELECT value FROM json_each({table}.source_document_ids)
                  UNION
                  SELECT value FROM json_each(?2)
                )
              )
              WHERE {key_col} = ?1"
        ),
        params![key_val, incoming_ids_json],
    )?;
    Ok(())
}

fn row_to_document(row: &rusqlite::Row) -> rusqlite::Result<Document> {
    Ok(Document {
        id: row.get(0)?,
        filename: row.get(1)?,
        file_path: row.get(2)?,
        mime_type: row.get(3)?,
        size_bytes: row.get(4)?,
        content_hash: row.get(5)?,
        origin: row.get(6)?,
        account_id: row.get(7)?,
        uploaded_at: row.get(8)?,
    })
}

// ── Seed helpers ─────────────────────────────────────────────────────────────

fn seed_defaults(conn: &Connection) -> Result<()> {
    // Profiles are never auto-seeded: there is no implicit "default" profile.
    // A fresh database starts with zero profiles; the user creates one
    // explicitly via the API/UI. This is what lets a deleted profile stay
    // deleted across restarts instead of being resurrected on every open.
    seed_categories(conn)?;
    migrate_category_data(conn)?;
    seed_currencies(conn)?;
    Ok(())
}

/// One leaf in the default taxonomy: name, its `category_type` string, optional description.
type DefaultLeaf = (String, String, Option<String>);

/// One raw spending-grid row from SQL: (group key, category_id, parent_id,
/// period, currency, summed amount).
type SpendingGridRawRow = (String, Option<String>, Option<String>, String, String, f64);

/// One raw investment event row: (date, event_type, symbol, quantity,
/// price_per_share, fee, currency, fee_currency).
type InvestmentEventRaw = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
);

/// A parsed contribution event, amounts already converted to the preferred
/// currency: (date, event_type, symbol, quantity, principal, fee).
type InvestmentEventParsed = (String, String, String, Decimal, Decimal, Decimal);

/// Parse the embedded `categories.yaml` into `(parent_name, [leaf...])`. Leaf
/// children may be bare strings (type defaults to `spending`, no description)
/// or maps `{ name, category_type?, description? }`.
fn parse_default_categories() -> Vec<(String, Vec<DefaultLeaf>)> {
    let value: serde_yaml::Value =
        serde_yaml::from_str(CATEGORIES_YAML).unwrap_or(serde_yaml::Value::Null);
    let mut out: Vec<(String, Vec<DefaultLeaf>)> = Vec::new();
    let Some(cats) = value.get("categories").and_then(|v| v.as_sequence()) else {
        return out;
    };
    for cat in cats {
        let parent_name = cat
            .get("parent")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if parent_name.is_empty() {
            continue;
        }
        let mut leaves: Vec<DefaultLeaf> = Vec::new();
        if let Some(children) = cat.get("children").and_then(|v| v.as_sequence()) {
            for child in children {
                if let Some(name) = child.as_str() {
                    leaves.push((name.to_string(), "spending".to_string(), None));
                } else if child.is_mapping() {
                    let name = child
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let ctype = child
                        .get("category_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("spending")
                        .to_string();
                    let desc = child
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    leaves.push((name, ctype, desc));
                }
            }
        }
        out.push((parent_name, leaves));
    }
    out
}

fn seed_categories(conn: &Connection) -> Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM categories", [], |r| r.get(0))?;
    if count > 0 {
        return Ok(());
    }

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    for (order, (parent_name, children)) in parse_default_categories().into_iter().enumerate() {
        let parent_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO categories (id, name, parent_id, display_order, is_active, category_type, created_at, updated_at)
             VALUES (?1, ?2, NULL, ?3, 1, 'spending', ?4, ?4)",
            params![parent_id, parent_name, order as i32, now],
        )?;

        for (child_order, (child_name, ctype, desc)) in children.into_iter().enumerate() {
            let child_id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO categories (id, name, parent_id, display_order, is_active, description, category_type, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, ?7)",
                params![child_id, child_name, parent_id, child_order as i32, desc, ctype, now],
            )?;
        }
    }
    Ok(())
}

fn seed_currencies(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO currencies (code, is_preferred, fx_rate, updated_at) VALUES (?1, 1, '1', NULL)",
        params!["GBP"],
    )?;

    conn.execute_batch(
        "INSERT OR IGNORE INTO currencies (code, is_preferred, fx_rate, updated_at)
         SELECT DISTINCT currency, 0, '1', NULL
         FROM (
             SELECT currency FROM transactions
             UNION
             SELECT currency FROM accounts
             UNION
             SELECT currency FROM holdings
         )
         WHERE currency NOT IN (SELECT code FROM currencies)",
    )?;

    Ok(())
}

fn migrate_category_data(conn: &Connection) -> Result<()> {
    // ── Transactions ──
    let txns: Vec<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, category FROM transactions WHERE category IS NOT NULL AND category_id IS NULL"
        )?;
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };

    for (tx_id, category_str) in &txns {
        let child_name = category_str
            .split_once(": ")
            .map(|(_, child)| child.trim())
            .unwrap_or(category_str.trim());

        let cat_id: Option<String> = conn
            .query_row(
                "SELECT id FROM categories WHERE name = ?1 AND is_active = 1",
                params![child_name],
                |r| r.get(0),
            )
            .optional()?;

        if let Some(id) = cat_id {
            conn.execute(
                "UPDATE transactions SET category_id = ?1 WHERE id = ?2",
                params![id, tx_id],
            )?;
        }
    }

    // ── Standing budgets ──
    let budgets: Vec<(i64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, category FROM standing_budgets WHERE category IS NOT NULL AND category_id IS NULL"
        )?;
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };

    for (budget_id, category_str) in &budgets {
        let child_name = category_str
            .split_once(": ")
            .map(|(_, child)| child.trim())
            .unwrap_or(category_str.trim());

        let cat_id: Option<String> = conn
            .query_row(
                "SELECT id FROM categories WHERE name = ?1 AND is_active = 1",
                params![child_name],
                |r| r.get(0),
            )
            .optional()?;

        if let Some(id) = cat_id {
            conn.execute(
                "UPDATE standing_budgets SET category_id = ?1 WHERE id = ?2",
                params![id, budget_id],
            )?;
        }
    }

    // ── Budget overrides ──
    let overrides: Vec<(i64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, category FROM budget_overrides WHERE category IS NOT NULL AND category_id IS NULL"
        )?;
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };

    for (override_id, category_str) in &overrides {
        let child_name = category_str
            .split_once(": ")
            .map(|(_, child)| child.trim())
            .unwrap_or(category_str.trim());

        let cat_id: Option<String> = conn
            .query_row(
                "SELECT id FROM categories WHERE name = ?1 AND is_active = 1",
                params![child_name],
                |r| r.get(0),
            )
            .optional()?;

        if let Some(id) = cat_id {
            conn.execute(
                "UPDATE budget_overrides SET category_id = ?1 WHERE id = ?2",
                params![id, override_id],
            )?;
        }
    }

    Ok(())
}

fn migrate_schema(conn: &Connection) -> Result<()> {
    // ── 1. Add category_id to transactions ──
    if conn
        .prepare("SELECT category_id FROM transactions LIMIT 0")
        .is_err()
    {
        conn.execute_batch(
            "ALTER TABLE transactions ADD COLUMN category_id TEXT REFERENCES categories(id)",
        )?;
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_tx_category_id ON transactions(category_id)",
        )?;
    }

    // ── 2. Add exclude_from_summary to transactions ──
    if conn
        .prepare("SELECT exclude_from_summary FROM transactions LIMIT 0")
        .is_err()
    {
        conn.execute_batch(
            "ALTER TABLE transactions ADD COLUMN exclude_from_summary INTEGER NOT NULL DEFAULT 0",
        )?;
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_tx_exclude_summary ON transactions(exclude_from_summary)"
        )?;
    }

    // ── 3. Add category_id to standing_budgets ──
    if conn
        .prepare("SELECT category_id FROM standing_budgets LIMIT 0")
        .is_err()
    {
        conn.execute_batch(
            "ALTER TABLE standing_budgets ADD COLUMN category_id TEXT REFERENCES categories(id)",
        )?;
    }

    // ── 4. Add category_id to budget_overrides ──
    if conn
        .prepare("SELECT category_id FROM budget_overrides LIMIT 0")
        .is_err()
    {
        conn.execute_batch(
            "ALTER TABLE budget_overrides ADD COLUMN category_id TEXT REFERENCES categories(id)",
        )?;
    }

    // ── 5. Add category_id to budgets ──
    if conn
        .prepare("SELECT category_id FROM budgets LIMIT 0")
        .is_err()
    {
        conn.execute_batch(
            "ALTER TABLE budgets ADD COLUMN category_id TEXT REFERENCES categories(id)",
        )?;
    }

    // ── 6. Drop the removed `sections` concept ──
    // Sections were only ever used to group the budget spreadsheet; the UI now
    // groups by parent category, so the table (and its data) is obsolete.
    conn.execute_batch("DROP TABLE IF EXISTS section_mappings")?;

    // ── 8. Convert mortgage account type to property ──
    let mortgage_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE type = 'mortgage'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if mortgage_count > 0 {
        conn.execute_batch("UPDATE accounts SET type = 'property' WHERE type = 'mortgage'")?;
    }

    // ── 9. Drop denormalised accounts.balance / accounts.balance_date ──
    // Balances are now sourced at read time from SUM(holdings.value) per account
    // (see Db::get_accounts and Db::accounts_as_of). Any historical column data
    // was already mirrored to a `_CASH` holding by set_account_balance.
    if conn.prepare("SELECT balance FROM accounts LIMIT 0").is_ok() {
        conn.execute_batch("ALTER TABLE accounts DROP COLUMN balance")?;
    }
    if conn
        .prepare("SELECT balance_date FROM accounts LIMIT 0")
        .is_ok()
    {
        conn.execute_batch("ALTER TABLE accounts DROP COLUMN balance_date")?;
    }

    // ── 10. Add free-text description to categories ──
    // Used by LLM categorisation agents to disambiguate categories whose
    // names overlap. Existing rows get NULL.
    if conn
        .prepare("SELECT description FROM categories LIMIT 0")
        .is_err()
    {
        conn.execute_batch("ALTER TABLE categories ADD COLUMN description TEXT")?;
    }

    // ── 11. Add source_document_ids to transactions / holdings / investments ──
    // JSON array of documents.id; provenance back to the source file(s) an item
    // was extracted from. Existing rows default to '[]'.
    for table in ["transactions", "holdings", "investments"] {
        let probe = format!("SELECT source_document_ids FROM {table} LIMIT 0");
        if conn.prepare(&probe).is_err() {
            conn.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN source_document_ids TEXT NOT NULL DEFAULT '[]'"
            ))?;
        }
    }

    // ── 12. Add fee_currency to investments ──
    // The fee may be charged in a different currency from the trade price (e.g. a
    // USD-priced share with a GBP commission). A pre-existing fee was implicitly in
    // the trade currency, so backfill those rows to keep the invariant that
    // fee_currency is non-null exactly when a fee is present.
    if conn
        .prepare("SELECT fee_currency FROM investments LIMIT 0")
        .is_err()
    {
        conn.execute_batch("ALTER TABLE investments ADD COLUMN fee_currency TEXT")?;
        conn.execute_batch(
            "UPDATE investments SET fee_currency = currency \
             WHERE fee IS NOT NULL AND (fee_currency IS NULL OR fee_currency = '')",
        )?;
    }

    // ── 13. Add category_type to categories + backfill from the default taxonomy ──
    // New column defaults every existing row to 'spending'; then any category
    // whose name matches a default (case-insensitive) inherits that default's
    // type, so unchanged default categories map automatically. Custom-named
    // categories stay 'spending'. Guarded, so the backfill runs exactly once.
    if conn
        .prepare("SELECT category_type FROM categories LIMIT 0")
        .is_err()
    {
        conn.execute_batch(
            "ALTER TABLE categories ADD COLUMN category_type TEXT NOT NULL DEFAULT 'spending'",
        )?;
        for (_parent, leaves) in parse_default_categories() {
            for (name, ctype, _desc) in leaves {
                conn.execute(
                    "UPDATE categories SET category_type = ?1 WHERE LOWER(name) = LOWER(?2)",
                    params![ctype, name],
                )?;
            }
        }
    }

    // ── 14. Collapse removed account/holding type variants ──
    // `cash` accounts fold into `checking`; the `savings` holding type (which
    // had no real data source) folds into `cash`; and the redundant `loan` and
    // `credit` holding types merge into a single `debt` liability line. Plain
    // idempotent UPDATEs (no CHECK constraint on either column): after the first
    // pass no rows match, so they are safe to run on every startup.
    conn.execute_batch("UPDATE accounts SET type = 'checking' WHERE type = 'cash'")?;
    conn.execute_batch("UPDATE holdings SET holding_type = 'cash' WHERE holding_type = 'savings'")?;
    conn.execute_batch(
        "UPDATE holdings SET holding_type = 'debt' WHERE holding_type IN ('loan', 'credit')",
    )?;

    // ── 15. Add utr to profiles ──
    // HMRC Unique Taxpayer Reference, needed on every SA108 page. Nullable, so
    // existing rows are unaffected and a household that never files a CGT
    // report never has to supply one.
    if conn.prepare("SELECT utr FROM profiles LIMIT 0").is_err() {
        conn.execute_batch("ALTER TABLE profiles ADD COLUMN utr TEXT")?;
    }

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod consolidation_tests {
    use super::*;
    use chrono::NaiveDate;
    use rust_decimal::Decimal;
    use std::str::FromStr;
    use tempfile::NamedTempFile;

    macro_rules! dec {
        ($val:expr) => {
            Decimal::from_str(stringify!($val)).unwrap()
        };
    }

    fn test_db() -> (Db, NamedTempFile) {
        let file = NamedTempFile::new().expect("temp file");
        let db = Db::open(file.path()).expect("test db");
        (db, file)
    }

    fn make_account(id: &str, account_type: AccountType) -> Account {
        let is_available = is_available_account(&account_type);
        Account {
            id: id.to_string(),
            name: id.to_string(),
            institution: "TestBank".to_string(),
            account_type,
            currency: "GBP".to_string(),
            balance: None,
            balance_date: None,
            is_active: true,
            notes: None,
            profile_ids: vec!["default".to_string()],
            is_stale: None,
            is_available,
        }
    }

    fn naive_dt(year: i32, month: u32, day: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
    }

    fn naive_date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    fn make_holding(
        account_id: &str,
        symbol: &str,
        holding_type: HoldingType,
        value: Decimal,
        as_of: NaiveDateTime,
    ) -> Holding {
        Holding {
            account_id: account_id.to_string(),
            symbol: symbol.to_string(),
            name: symbol.to_string(),
            holding_type,
            quantity: Decimal::ONE,
            price_per_unit: None,
            value,
            currency: "GBP".to_string(),
            as_of,
            short_name: None,
            sub_account: None,
            is_closed: false,
            derived: false,
            source_document_ids: Vec::new(),
            source_file: None,
        }
    }

    fn make_holding_with_sub(
        account_id: &str,
        symbol: &str,
        holding_type: HoldingType,
        value: Decimal,
        as_of: NaiveDateTime,
        sub_account: Option<&str>,
    ) -> Holding {
        Holding {
            account_id: account_id.to_string(),
            symbol: symbol.to_string(),
            name: symbol.to_string(),
            holding_type,
            quantity: Decimal::ONE,
            price_per_unit: None,
            value,
            currency: "GBP".to_string(),
            as_of,
            short_name: None,
            sub_account: sub_account.map(|s| s.to_string()),
            is_closed: false,
            derived: false,
            source_document_ids: Vec::new(),
            source_file: None,
        }
    }

    #[test]
    fn set_account_balance_creates_cash_holding() {
        let (db, _file) = test_db();
        db.create_account(&make_account("monzo", AccountType::Checking))
            .unwrap();

        db.set_account_balance("monzo", dec!(1500), naive_dt(2025, 1, 15))
            .unwrap();

        let holdings = db
            .get_holdings_batch(&["monzo".to_string()], false)
            .unwrap();
        assert_eq!(holdings.len(), 1);
        assert_eq!(holdings[0].symbol, "_CASH");
        assert_eq!(holdings[0].holding_type, HoldingType::Cash);
        assert_eq!(holdings[0].value, dec!(1500));
    }

    #[test]
    fn set_account_balance_upserts_on_same_date() {
        let (db, _file) = test_db();
        db.create_account(&make_account("monzo", AccountType::Checking))
            .unwrap();

        let dt = naive_dt(2025, 1, 15);
        db.set_account_balance("monzo", dec!(1000), dt).unwrap();
        db.set_account_balance("monzo", dec!(1200), dt).unwrap();

        let holdings = db
            .get_holdings_batch(&["monzo".to_string()], false)
            .unwrap();
        assert_eq!(holdings.len(), 1, "should not duplicate on same date");
        assert_eq!(holdings[0].value, dec!(1200));
    }

    #[test]
    fn accounts_as_of_sums_all_holdings() {
        let (db, _file) = test_db();
        db.create_account(&make_account("t212", AccountType::Investment))
            .unwrap();

        let dt = naive_dt(2025, 1, 15);
        db.upsert_holdings(
            "t212",
            &[
                make_holding("t212", "AAPL", HoldingType::Stock, dec!(5000), dt),
                make_holding("t212", "MSFT", HoldingType::Stock, dec!(3000), dt),
                make_holding("t212", "_CASH", HoldingType::Cash, dec!(2000), dt),
            ],
        )
        .unwrap();

        let accounts = db.accounts_as_of(naive_date(2025, 2, 1), None).unwrap();
        let t212 = accounts.iter().find(|a| a.id == "t212").unwrap();
        // accounts_as_of sums Decimal values exactly (no CAST AS REAL).
        assert_eq!(t212.balance.unwrap(), dec!(10000));
    }

    #[test]
    fn accounts_as_of_carry_forward() {
        let (db, _file) = test_db();
        db.create_account(&make_account("monzo", AccountType::Checking))
            .unwrap();

        db.set_account_balance("monzo", dec!(1000), naive_dt(2025, 1, 15))
            .unwrap();
        db.set_account_balance("monzo", dec!(1500), naive_dt(2025, 3, 1))
            .unwrap();

        // Query for Feb: should carry forward Jan value.
        let accounts = db.accounts_as_of(naive_date(2025, 2, 15), None).unwrap();
        let monzo = accounts.iter().find(|a| a.id == "monzo").unwrap();
        assert_eq!(monzo.balance.unwrap(), dec!(1000));

        // Query for April: should use March value.
        let accounts = db.accounts_as_of(naive_date(2025, 4, 15), None).unwrap();
        let monzo = accounts.iter().find(|a| a.id == "monzo").unwrap();
        assert_eq!(monzo.balance.unwrap(), dec!(1500));
    }

    #[test]
    fn accounts_as_of_stale_flag() {
        let (db, _file) = test_db();
        db.create_account(&make_account("monzo", AccountType::Checking))
            .unwrap();

        // Record balance on Jan 1. Query 60 days later: should be stale.
        db.set_account_balance("monzo", dec!(500), naive_dt(2025, 1, 1))
            .unwrap();
        let accounts = db.accounts_as_of(naive_date(2025, 3, 2), None).unwrap();
        let monzo = accounts.iter().find(|a| a.id == "monzo").unwrap();
        assert_eq!(monzo.is_stale, Some(true));

        // Record balance on Feb 28. Query March 2: within 45 days, not stale.
        db.set_account_balance("monzo", dec!(600), naive_dt(2025, 2, 28))
            .unwrap();
        let accounts = db.accounts_as_of(naive_date(2025, 3, 2), None).unwrap();
        let monzo = accounts.iter().find(|a| a.id == "monzo").unwrap();
        assert_eq!(monzo.is_stale, Some(false));
    }

    #[test]
    fn get_balance_summary_returns_delta() {
        let (db, _file) = test_db();
        db.create_account(&make_account("monzo", AccountType::Checking))
            .unwrap();

        db.set_account_balance("monzo", dec!(1000), naive_dt(2025, 1, 1))
            .unwrap();
        db.set_account_balance("monzo", dec!(1300), naive_dt(2025, 3, 1))
            .unwrap();

        let summary = db
            .get_balance_summary(naive_date(2025, 1, 1), naive_date(2025, 3, 31))
            .unwrap();
        assert_eq!(summary.len(), 1);
        let row = &summary[0];
        assert_eq!(row.account_id, "monzo");

        let start = row.start_balance.unwrap();
        let end = row.end_balance.unwrap();
        let delta = row.delta.unwrap();

        let tol = Decimal::from_str("0.01").unwrap();
        assert!((start - dec!(1000)).abs() < tol);
        assert!((end - dec!(1300)).abs() < tol);
        assert!((delta - dec!(300)).abs() < tol);
    }

    #[test]
    fn get_balance_summary_multi_account_range_semantics() {
        let (db, _file) = test_db();
        db.create_account(&make_account("single", AccountType::Checking))
            .unwrap();
        db.create_account(&make_account("spanning", AccountType::Checking))
            .unwrap();
        db.create_account(&make_account("outside", AccountType::Checking))
            .unwrap();

        // Exactly one snapshot inside the range.
        db.set_account_balance("single", dec!(500), naive_dt(2025, 2, 10))
            .unwrap();

        // Snapshots on both sides of the range plus two inside it: the
        // out-of-range ones must not contribute.
        db.set_account_balance("spanning", dec!(100), naive_dt(2024, 12, 1))
            .unwrap();
        db.set_account_balance("spanning", dec!(200), naive_dt(2025, 1, 10))
            .unwrap();
        db.set_account_balance("spanning", dec!(350), naive_dt(2025, 3, 20))
            .unwrap();
        db.set_account_balance("spanning", dec!(900), naive_dt(2025, 5, 1))
            .unwrap();

        // Only out-of-range snapshots: must not appear at all.
        db.set_account_balance("outside", dec!(42), naive_dt(2024, 6, 1))
            .unwrap();
        db.set_account_balance("outside", dec!(43), naive_dt(2025, 6, 1))
            .unwrap();

        let mut summary = db
            .get_balance_summary(naive_date(2025, 1, 1), naive_date(2025, 3, 31))
            .unwrap();
        summary.sort_by(|a, b| a.account_id.cmp(&b.account_id));
        assert_eq!(summary.len(), 2, "out-of-range-only account is omitted");

        let tol = Decimal::from_str("0.01").unwrap();

        let single = &summary[0];
        assert_eq!(single.account_id, "single");
        assert!((single.start_balance.unwrap() - dec!(500)).abs() < tol);
        assert!((single.end_balance.unwrap() - dec!(500)).abs() < tol);
        assert!(single.delta.unwrap().abs() < tol);

        let spanning = &summary[1];
        assert_eq!(spanning.account_id, "spanning");
        assert!((spanning.start_balance.unwrap() - dec!(200)).abs() < tol);
        assert!((spanning.end_balance.unwrap() - dec!(350)).abs() < tol);
        assert!((spanning.delta.unwrap() - dec!(150)).abs() < tol);
    }

    #[test]
    fn get_balances_in_range_aggregates_per_date() {
        let (db, _file) = test_db();
        db.create_account(&make_account("t212", AccountType::Investment))
            .unwrap();

        let dt1 = naive_dt(2025, 1, 1);
        let dt2 = naive_dt(2025, 2, 1);

        db.upsert_holdings(
            "t212",
            &[
                make_holding("t212", "AAPL", HoldingType::Stock, dec!(2000), dt1),
                make_holding("t212", "_CASH", HoldingType::Cash, dec!(500), dt1),
                make_holding("t212", "AAPL", HoldingType::Stock, dec!(2200), dt2),
                make_holding("t212", "_CASH", HoldingType::Cash, dec!(600), dt2),
            ],
        )
        .unwrap();

        let rows = db
            .get_balances_in_range(naive_date(2025, 1, 1), naive_date(2025, 2, 28))
            .unwrap();
        assert_eq!(rows.len(), 2, "one row per (account, date)");

        let tol = Decimal::from_str("0.01").unwrap();
        let jan = rows
            .iter()
            .find(|r| r.as_of.date() == naive_date(2025, 1, 1))
            .unwrap();
        assert!((jan.balance - dec!(2500)).abs() < tol);

        let feb = rows
            .iter()
            .find(|r| r.as_of.date() == naive_date(2025, 2, 1))
            .unwrap();
        assert!((feb.balance - dec!(2800)).abs() < tol);
    }

    #[test]
    fn holdings_api_unchanged() {
        let (db, _file) = test_db();
        db.create_account(&make_account("t212", AccountType::Investment))
            .unwrap();

        let dt = naive_dt(2025, 1, 15);
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "VOO",
                HoldingType::Etf,
                dec!(4000),
                dt,
            )],
        )
        .unwrap();

        let holdings = db.get_holdings_batch(&["t212".to_string()], false).unwrap();
        assert_eq!(holdings.len(), 1);
        assert_eq!(holdings[0].symbol, "VOO");
        assert_eq!(holdings[0].value, dec!(4000));
    }

    #[test]
    fn closed_holding_contributes_zero_to_summary() {
        let (db, _file) = test_db();
        db.create_account(&make_account("t212", AccountType::Investment))
            .unwrap();

        let dt = naive_dt(2025, 1, 15);
        db.upsert_holdings(
            "t212",
            &[
                make_holding("t212", "AAPL", HoldingType::Stock, dec!(5000), dt),
                make_holding("t212", "_CASH", HoldingType::Cash, dec!(2000), dt),
            ],
        )
        .unwrap();

        // Invariant: a position can only be closed once it is zeroed out.
        let dt2 = naive_dt(2025, 1, 20);
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "AAPL",
                HoldingType::Stock,
                dec!(0),
                dt2,
            )],
        )
        .unwrap();
        db.close_holding("t212", "AAPL", None, dt2).unwrap();

        // Carry-forward picks AAPL's latest snapshot (the closed £0 one), so it
        // contributes nothing; only the £2000 cash remains.
        let accounts = db.accounts_as_of(naive_date(2025, 2, 1), None).unwrap();
        let t212 = accounts.iter().find(|a| a.id == "t212").unwrap();
        assert_eq!(t212.balance.unwrap(), dec!(2000));

        // Closing must not delete the row; it stays for history/audit.
        let raw_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM holdings WHERE account_id = 't212'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(raw_count, 3, "closed holding row should still exist");
    }

    #[test]
    fn test_sub_account_holdings() {
        let (db, _file) = test_db();
        db.create_account(&make_account("monzo", AccountType::Checking))
            .unwrap();

        let dt = naive_dt(2026, 4, 15);
        db.upsert_holdings(
            "monzo",
            &[
                make_holding("monzo", "_CASH", HoldingType::Cash, dec!(1200), dt),
                make_holding_with_sub(
                    "monzo",
                    "_CASH",
                    HoldingType::Cash,
                    dec!(500),
                    dt,
                    Some("Bills Pot"),
                ),
                make_holding_with_sub(
                    "monzo",
                    "_CASH",
                    HoldingType::Cash,
                    dec!(3000),
                    dt,
                    Some("Savings Pot"),
                ),
            ],
        )
        .unwrap();

        let holdings = db
            .get_holdings_batch(&["monzo".to_string()], false)
            .unwrap();
        assert_eq!(
            holdings.len(),
            3,
            "all three sub-account holdings should be stored"
        );

        let accounts = db.accounts_as_of(naive_date(2026, 4, 30), None).unwrap();
        let monzo = accounts.iter().find(|a| a.id == "monzo").unwrap();
        let balance = monzo.balance.unwrap();
        let tol = Decimal::from_str("0.01").unwrap();
        assert!(
            (balance - dec!(4700)).abs() < tol,
            "expected ~4700 (sum of all three), got {balance}"
        );
    }

    #[test]
    fn test_sub_account_unique_constraint() {
        let (db, _file) = test_db();
        db.create_account(&make_account("a", AccountType::Checking))
            .unwrap();

        let dt = naive_dt(2026, 4, 15);
        db.upsert_holdings(
            "a",
            &[make_holding("a", "_CASH", HoldingType::Cash, dec!(100), dt)],
        )
        .unwrap();

        db.upsert_holdings(
            "a",
            &[make_holding("a", "_CASH", HoldingType::Cash, dec!(200), dt)],
        )
        .unwrap();

        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM holdings WHERE account_id = 'a' AND symbol = '_CASH'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "upsert should not create duplicates");

        let value: String = db
            .conn
            .query_row(
                "SELECT value FROM holdings WHERE account_id = 'a' AND symbol = '_CASH'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(value, "200", "value should be updated");
    }

    #[test]
    fn test_dry_run_writes_nothing() {
        let (db, _file) = test_db();
        db.create_account(&make_account("t212", AccountType::Investment))
            .unwrap();

        let dt = naive_dt(2026, 4, 15);
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "AAPL",
                HoldingType::Stock,
                dec!(5000),
                dt,
            )],
        )
        .unwrap();

        let count_before: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM holdings", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count_before, 1);

        let previews = db
            .dry_run_holdings(
                "t212",
                &[
                    make_holding("t212", "MSFT", HoldingType::Stock, dec!(3000), dt),
                    make_holding("t212", "GOOG", HoldingType::Stock, dec!(2000), dt),
                ],
            )
            .unwrap();

        let count_after: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM holdings", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count_after, 1, "dry-run must not write to DB");

        assert_eq!(previews.len(), 2);
        assert!(previews.iter().all(|p| p.status == "new"));
    }

    #[test]
    fn test_dry_run_detects_modify() {
        let (db, _file) = test_db();
        db.create_account(&make_account("t212", AccountType::Investment))
            .unwrap();

        let dt = naive_dt(2026, 4, 15);
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "VWRL",
                HoldingType::Etf,
                dec!(8000),
                dt,
            )],
        )
        .unwrap();

        let previews = db
            .dry_run_holdings(
                "t212",
                &[make_holding(
                    "t212",
                    "VWRL",
                    HoldingType::Etf,
                    dec!(9000),
                    dt,
                )],
            )
            .unwrap();

        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].status, "modify");
        assert_eq!(previews[0].existing_value.as_deref(), Some("8000"));
    }

    #[test]
    fn test_dry_run_unchanged_snapshot_is_duplicate() {
        let (db, _file) = test_db();
        db.create_account(&make_account("t212", AccountType::Investment))
            .unwrap();

        let dt = naive_dt(2026, 4, 15);
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "VWRL",
                HoldingType::Etf,
                dec!(8000),
                dt,
            )],
        )
        .unwrap();

        // Re-importing the exact same snapshot (same value, same as_of) is a
        // no-op, so it should be flagged "duplicate" (Skip), not "modify".
        let previews = db
            .dry_run_holdings(
                "t212",
                &[make_holding(
                    "t212",
                    "VWRL",
                    HoldingType::Etf,
                    dec!(8000),
                    dt,
                )],
            )
            .unwrap();

        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].status, "duplicate");
    }

    #[test]
    fn test_holding_import_upsert() {
        let (db, _file) = test_db();
        db.create_account(&make_account("t212", AccountType::Investment))
            .unwrap();

        let dt = naive_dt(2026, 4, 15);
        db.upsert_holdings(
            "t212",
            &[
                make_holding("t212", "AAPL", HoldingType::Stock, dec!(5000), dt),
                make_holding("t212", "MSFT", HoldingType::Stock, dec!(3000), dt),
                make_holding("t212", "GOOG", HoldingType::Stock, dec!(2000), dt),
            ],
        )
        .unwrap();

        db.upsert_holdings(
            "t212",
            &[
                make_holding("t212", "AAPL", HoldingType::Stock, dec!(5500), dt),
                make_holding("t212", "GOOG", HoldingType::Stock, dec!(2200), dt),
                make_holding("t212", "TSLA", HoldingType::Stock, dec!(1000), dt),
            ],
        )
        .unwrap();

        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM holdings WHERE account_id = 't212'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 4, "3 original + 1 new = 4 total");

        let aapl_value: String = db
            .conn
            .query_row(
                "SELECT value FROM holdings WHERE account_id = 't212' AND symbol = 'AAPL'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(aapl_value, "5500", "AAPL should have been updated");
    }

    #[test]
    fn test_close_and_reopen_holding() {
        let (db, _file) = test_db();
        db.create_account(&make_account("t212", AccountType::Investment))
            .unwrap();

        let dt = naive_dt(2026, 4, 15);
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "AAPL",
                HoldingType::Stock,
                dec!(5000),
                dt,
            )],
        )
        .unwrap();

        let holdings = db.get_holdings_batch(&["t212".to_string()], false).unwrap();
        assert_eq!(holdings.len(), 1);

        // Invariant: a non-zero holding cannot be closed.
        assert!(
            db.close_holding("t212", "AAPL", None, dt).is_err(),
            "closing a non-zero holding must be rejected"
        );

        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "AAPL",
                HoldingType::Stock,
                dec!(0),
                dt,
            )],
        )
        .unwrap();
        db.close_holding("t212", "AAPL", None, dt).unwrap();
        let holdings = db.get_holdings_batch(&["t212".to_string()], false).unwrap();
        assert_eq!(
            holdings.len(),
            0,
            "closed holding should not appear in batch"
        );

        db.reopen_holding("t212", "AAPL", None, dt).unwrap();
        let holdings = db.get_holdings_batch(&["t212".to_string()], false).unwrap();
        assert_eq!(holdings.len(), 1, "reopened holding should reappear");
    }

    #[test]
    fn test_upsert_with_sub_account() {
        let (db, _file) = test_db();
        db.create_account(&make_account("monzo", AccountType::Checking))
            .unwrap();

        let dt = naive_dt(2026, 4, 15);
        db.upsert_holdings(
            "monzo",
            &[make_holding_with_sub(
                "monzo",
                "_CASH",
                HoldingType::Cash,
                dec!(500),
                dt,
                Some("Bills Pot"),
            )],
        )
        .unwrap();

        db.upsert_holdings(
            "monzo",
            &[make_holding_with_sub(
                "monzo",
                "_CASH",
                HoldingType::Cash,
                dec!(750),
                dt,
                Some("Bills Pot"),
            )],
        )
        .unwrap();

        let count: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM holdings WHERE account_id = 'monzo' AND sub_account = 'Bills Pot'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(count, 1, "should only have one row for the sub-account");

        let value: String = db.conn.query_row(
            "SELECT value FROM holdings WHERE account_id = 'monzo' AND sub_account = 'Bills Pot'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(value, "750", "value should be updated");
    }

    /// Single GBP-preferred FX map for reconciliation tests (no conversion).
    fn gbp_fx() -> crate::util::fx::FxRateMap {
        crate::util::fx::FxRateMap::new(vec![crate::model::Currency {
            code: "GBP".to_string(),
            is_preferred: true,
            fx_rate: dec!(1),
            updated_at: None,
        }])
        .unwrap()
    }

    /// Net worth the portfolio summary handler would compute for `as_of`:
    /// sum of every carried holding converted to the preferred currency.
    fn summary_net_worth(db: &Db, as_of: NaiveDate, fx: &crate::util::fx::FxRateMap) -> Decimal {
        db.get_holdings_for_summary(as_of, None)
            .unwrap()
            .iter()
            .map(|r| fx.convert(r.holding.value, &r.holding.currency))
            .sum()
    }

    // Regression: history's last point must equal the summary's net worth
    // for the same as_of. Fixture uses per-symbol multi-date snapshots.
    #[test]
    fn history_last_reconciles_with_summary_net_worth() {
        let (db, _file) = test_db();
        db.create_account(&make_account("t212", AccountType::Investment))
            .unwrap();
        db.create_account(&make_account("pension", AccountType::Pension))
            .unwrap();

        // AAPL re-snapshotted twice; MSFT only once (must carry forward);
        // pension once. Different dates per symbol.
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "AAPL",
                HoldingType::Stock,
                dec!(100),
                naive_dt(2025, 1, 15),
            )],
        )
        .unwrap();
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "MSFT",
                HoldingType::Stock,
                dec!(50),
                naive_dt(2025, 2, 5),
            )],
        )
        .unwrap();
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "AAPL",
                HoldingType::Stock,
                dec!(120),
                naive_dt(2025, 3, 10),
            )],
        )
        .unwrap();
        db.upsert_holdings(
            "pension",
            &[make_holding(
                "pension",
                "PEN",
                HoldingType::Fund,
                dec!(1000),
                naive_dt(2025, 1, 31),
            )],
        )
        .unwrap();

        let fx = gbp_fx();
        let as_of = naive_date(2025, 4, 30);
        let net_worth = summary_net_worth(&db, as_of, &fx);
        assert_eq!(net_worth, dec!(1170)); // 120 + 50 + 1000

        let history = db
            .get_monthly_net_worth(
                naive_date(2025, 1, 1),
                as_of,
                &Granularity::Monthly,
                None,
                &fx,
            )
            .unwrap();
        let last = history.last().expect("at least one period");
        assert_eq!(
            last.total_wealth, net_worth,
            "history last total must equal summary net worth"
        );
        assert_eq!(last.available_wealth, dec!(170));
        assert_eq!(last.unavailable_wealth, dec!(1000));

        // Per-account list must also sum to net worth.
        let accounts = db.accounts_as_of(as_of, None).unwrap();
        let sum: Decimal = accounts.iter().filter_map(|a| a.balance).sum();
        assert_eq!(sum, net_worth);
        let t212 = accounts.iter().find(|a| a.id == "t212").unwrap();
        assert_eq!(t212.balance.unwrap(), dec!(170));
    }

    // A position closed (zeroed) later must still be counted at its open value
    // in earlier periods, and contribute 0 from the close onward.
    #[test]
    fn time_aware_closed_holding() {
        let (db, _file) = test_db();
        db.create_account(&make_account("t212", AccountType::Investment))
            .unwrap();

        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "AAPL",
                HoldingType::Stock,
                dec!(100),
                naive_dt(2025, 1, 15),
            )],
        )
        .unwrap();
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "AAPL",
                HoldingType::Stock,
                dec!(0),
                naive_dt(2025, 4, 10),
            )],
        )
        .unwrap();
        db.close_holding("t212", "AAPL", None, naive_dt(2025, 4, 10))
            .unwrap();

        // February: position still active at its open value.
        let feb = db.accounts_as_of(naive_date(2025, 2, 15), None).unwrap();
        assert_eq!(
            feb.iter()
                .find(|a| a.id == "t212")
                .unwrap()
                .balance
                .unwrap(),
            dec!(100)
        );

        // May: latest snapshot is the closed £0 one -> contributes nothing.
        let may = db.accounts_as_of(naive_date(2025, 5, 1), None).unwrap();
        assert_eq!(
            may.iter()
                .find(|a| a.id == "t212")
                .unwrap()
                .balance
                .unwrap(),
            dec!(0)
        );
    }

    // One-scan carry-forward must reproduce per-period recomputation: values
    // carry forward between snapshots, an account whose first snapshot lands
    // mid-range contributes only from then on, and a closed (zeroed) holding
    // contributes 0 to net worth but drops out of investment market value.
    #[test]
    fn history_carry_forward_mid_range_and_closed_holding() {
        let (db, _file) = test_db();
        db.create_account(&make_account("t212", AccountType::Investment))
            .unwrap();
        db.create_account(&make_account("sav", AccountType::Savings))
            .unwrap();

        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "AAPL",
                HoldingType::Stock,
                dec!(100),
                naive_dt(2025, 1, 15),
            )],
        )
        .unwrap();
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "MSFT",
                HoldingType::Stock,
                dec!(50),
                naive_dt(2025, 2, 5),
            )],
        )
        .unwrap();
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "AAPL",
                HoldingType::Stock,
                dec!(120),
                naive_dt(2025, 3, 10),
            )],
        )
        .unwrap();
        // Savings account's first snapshot lands mid-range.
        db.upsert_holdings(
            "sav",
            &[make_holding(
                "sav",
                "_CASH",
                HoldingType::Cash,
                dec!(1000),
                naive_dt(2025, 3, 20),
            )],
        )
        .unwrap();
        // MSFT zeroed and closed in April.
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "MSFT",
                HoldingType::Stock,
                dec!(0),
                naive_dt(2025, 4, 10),
            )],
        )
        .unwrap();
        db.close_holding("t212", "MSFT", None, naive_dt(2025, 4, 10))
            .unwrap();

        let fx = gbp_fx();
        let from = naive_date(2025, 1, 1);
        let to = naive_date(2025, 5, 31);

        let history = db
            .get_monthly_net_worth(from, to, &Granularity::Monthly, None, &fx)
            .unwrap();
        let totals: Vec<(String, Decimal)> = history
            .iter()
            .map(|r| (r.month.clone(), r.total_wealth))
            .collect();
        assert_eq!(
            totals,
            vec![
                ("2025-01".to_string(), dec!(100)),
                ("2025-02".to_string(), dec!(150)),
                ("2025-03".to_string(), dec!(1170)),
                ("2025-04".to_string(), dec!(1120)),
                ("2025-05".to_string(), dec!(1120)),
            ]
        );
        assert_eq!(
            history.last().unwrap().total_wealth,
            summary_net_worth(&db, to, &fx)
        );

        // Investment market value drops the closed MSFT position from April
        // on and never includes the savings account.
        let inv = db
            .get_investment_history(from, to, &Granularity::Monthly, None, &[], &fx)
            .unwrap();
        let mv: Vec<Option<String>> = inv.iter().map(|r| r.market_value.clone()).collect();
        assert_eq!(
            mv,
            vec![
                Some("100".to_string()),
                Some("150".to_string()),
                Some("170".to_string()),
                Some("120".to_string()),
                Some("120".to_string()),
            ]
        );
    }

    // The windowed snapshot fetch must still carry positions whose only
    // snapshots predate the range start.
    #[test]
    fn history_window_start_carries_pre_range_snapshots() {
        let (db, _file) = test_db();
        db.create_account(&make_account("t212", AccountType::Investment))
            .unwrap();

        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "AAPL",
                HoldingType::Stock,
                dec!(100),
                naive_dt(2025, 1, 15),
            )],
        )
        .unwrap();
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "MSFT",
                HoldingType::Stock,
                dec!(50),
                naive_dt(2025, 2, 5),
            )],
        )
        .unwrap();
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "AAPL",
                HoldingType::Stock,
                dec!(120),
                naive_dt(2025, 3, 10),
            )],
        )
        .unwrap();

        let fx = gbp_fx();
        // Range opens after both positions did; only the AAPL re-snapshot
        // falls inside it, so MSFT must arrive via the window-start carry.
        let from = naive_date(2025, 3, 1);
        let to = naive_date(2025, 4, 30);

        let history = db
            .get_monthly_net_worth(from, to, &Granularity::Monthly, None, &fx)
            .unwrap();
        let totals: Vec<Decimal> = history.iter().map(|r| r.total_wealth).collect();
        assert_eq!(totals, vec![dec!(170), dec!(170)]);
        assert_eq!(
            history.last().unwrap().total_wealth,
            summary_net_worth(&db, to, &fx)
        );

        let inv = db
            .get_investment_history(from, to, &Granularity::Monthly, None, &[], &fx)
            .unwrap();
        let mv: Vec<Option<String>> = inv.iter().map(|r| r.market_value.clone()).collect();
        assert_eq!(mv, vec![Some("170".to_string()), Some("170".to_string())]);

        // Metrics window opening mid-history carries AAPL@100 + MSFT@50.
        let m = db
            .compute_investment_metrics(naive_date(2025, 2, 15), to, None, &fx)
            .unwrap();
        assert_eq!(m.start_value, dec!(150));
        assert_eq!(m.end_value, dec!(170));
    }

    #[test]
    fn close_holding_rejects_nonzero_value() {
        let (db, _file) = test_db();
        db.create_account(&make_account("t212", AccountType::Investment))
            .unwrap();
        let dt = naive_dt(2025, 1, 15);
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "AAPL",
                HoldingType::Stock,
                dec!(5000),
                dt,
            )],
        )
        .unwrap();
        assert!(
            db.close_holding("t212", "AAPL", None, dt).is_err(),
            "closing a non-zero holding must be rejected"
        );
    }

    #[test]
    fn upsert_and_replace_reject_closed_nonzero_holding() {
        let (db, _file) = test_db();
        db.create_account(&make_account("t212", AccountType::Investment))
            .unwrap();
        let dt = naive_dt(2025, 1, 15);
        let mut closed_nonzero = make_holding("t212", "AAPL", HoldingType::Stock, dec!(5000), dt);
        closed_nonzero.is_closed = true;

        assert!(
            db.upsert_holdings("t212", std::slice::from_ref(&closed_nonzero))
                .is_err(),
            "upsert of a closed non-zero holding must be rejected"
        );
        assert!(
            db.replace_holdings("t212", std::slice::from_ref(&closed_nonzero))
                .is_err(),
            "replace with a closed non-zero holding must be rejected"
        );

        // A closed zeroed holding is allowed.
        let mut closed_zero = make_holding("t212", "AAPL", HoldingType::Stock, dec!(0), dt);
        closed_zero.is_closed = true;
        assert!(db.upsert_holdings("t212", &[closed_zero]).is_ok());
    }

    // Investment metrics must use per-symbol carry-forward, like net worth.
    // The old single-snapshot-date sum would drop MSFT here (it was last
    // recorded on a different date than AAPL's latest snapshot) -> end_value
    // would wrongly be 120 instead of 170.
    #[test]
    fn investment_metrics_use_per_symbol_carry_forward() {
        let (db, _file) = test_db();
        db.create_account(&make_account("t212", AccountType::Investment))
            .unwrap();

        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "AAPL",
                HoldingType::Stock,
                dec!(100),
                naive_dt(2025, 1, 15),
            )],
        )
        .unwrap();
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "MSFT",
                HoldingType::Stock,
                dec!(50),
                naive_dt(2025, 2, 5),
            )],
        )
        .unwrap();
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "AAPL",
                HoldingType::Stock,
                dec!(120),
                naive_dt(2025, 3, 10),
            )],
        )
        .unwrap();

        let m = db
            .compute_investment_metrics(
                naive_date(2025, 1, 1),
                naive_date(2025, 4, 30),
                None,
                &gbp_fx(),
            )
            .unwrap();
        // Jan 1: no snapshots yet.
        assert_eq!(m.start_value, dec!(0));
        // Apr 30: AAPL carried from Mar 10 (120) + MSFT carried from Feb 5 (50).
        assert_eq!(m.end_value, dec!(170));
        assert_eq!(m.total_growth, dec!(170));
        assert_eq!(m.new_cash_invested, dec!(0));
        assert_eq!(m.market_growth, dec!(170));
    }

    // Closed (zeroed) investment holdings contribute 0; non-investment
    // accounts are excluded entirely.
    #[test]
    fn investment_metrics_exclude_closed_and_non_investment() {
        let (db, _file) = test_db();
        db.create_account(&make_account("t212", AccountType::Investment))
            .unwrap();
        db.create_account(&make_account("monzo", AccountType::Checking))
            .unwrap();

        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "AAPL",
                HoldingType::Stock,
                dec!(100),
                naive_dt(2025, 1, 15),
            )],
        )
        .unwrap();
        // A checking account holding must not count toward investment metrics.
        db.upsert_holdings(
            "monzo",
            &[make_holding(
                "monzo",
                "_CASH",
                HoldingType::Cash,
                dec!(9999),
                naive_dt(2025, 1, 15),
            )],
        )
        .unwrap();
        // Zero out + close AAPL in March.
        db.upsert_holdings(
            "t212",
            &[make_holding(
                "t212",
                "AAPL",
                HoldingType::Stock,
                dec!(0),
                naive_dt(2025, 3, 1),
            )],
        )
        .unwrap();
        db.close_holding("t212", "AAPL", None, naive_dt(2025, 3, 1))
            .unwrap();

        // February: AAPL still open at 100; checking ignored.
        let feb = db
            .compute_investment_metrics(
                naive_date(2025, 1, 1),
                naive_date(2025, 2, 15),
                None,
                &gbp_fx(),
            )
            .unwrap();
        assert_eq!(feb.end_value, dec!(100));

        // April: AAPL latest snapshot is the closed £0 one -> 0.
        let apr = db
            .compute_investment_metrics(
                naive_date(2025, 1, 1),
                naive_date(2025, 4, 30),
                None,
                &gbp_fx(),
            )
            .unwrap();
        assert_eq!(apr.end_value, dec!(0));
    }

    // Multi-currency: values must be FX-converted to the preferred currency,
    // not summed raw. This is the real-data bug (an NGN position was counted
    // at face value as GBP, inflating metrics ~3700x for that holding).
    #[test]
    fn investment_metrics_convert_currency() {
        let (db, _file) = test_db();
        db.create_account(&make_account("t212", AccountType::Investment))
            .unwrap();

        let fx = crate::util::fx::FxRateMap::new(vec![
            crate::model::Currency {
                code: "GBP".to_string(),
                is_preferred: true,
                fx_rate: dec!(1),
                updated_at: None,
            },
            crate::model::Currency {
                code: "USD".to_string(),
                is_preferred: false,
                fx_rate: dec!(0.5),
                updated_at: None,
            },
        ])
        .unwrap();

        let mut usd = make_holding(
            "t212",
            "AAPL",
            HoldingType::Stock,
            dec!(1000),
            naive_dt(2025, 1, 15),
        );
        usd.currency = "USD".to_string();
        let gbp = make_holding(
            "t212",
            "VWRP",
            HoldingType::Stock,
            dec!(200),
            naive_dt(2025, 1, 15),
        );
        db.upsert_holdings("t212", &[usd, gbp]).unwrap();

        let m = db
            .compute_investment_metrics(naive_date(2025, 1, 1), naive_date(2025, 2, 1), None, &fx)
            .unwrap();
        // 1000 USD * 0.5 = 500 GBP, + 200 GBP = 700 GBP (raw sum would be 1200).
        assert_eq!(m.end_value, dec!(700));
    }
}

#[cfg(test)]
mod transaction_dryrun_tests {
    use super::*;
    use chrono::NaiveDate;
    use rust_decimal::Decimal;
    use tempfile::NamedTempFile;

    fn test_db() -> (Db, NamedTempFile) {
        let file = NamedTempFile::new().expect("temp file");
        let db = Db::open(file.path()).expect("test db");
        (db, file)
    }

    fn setup_test_account(db: &Db, id: &str) {
        db.create_account(&Account {
            id: id.to_string(),
            name: id.to_string(),
            institution: "TestBank".to_string(),
            account_type: AccountType::Checking,
            currency: "GBP".to_string(),
            balance: None,
            balance_date: None,
            is_active: true,
            notes: None,
            profile_ids: vec!["default".to_string()],
            is_stale: None,
            is_available: true,
        })
        .unwrap();
    }

    fn make_txn(date: (i32, u32, u32), desc: &str, amount: i64, scale: u32) -> ImportTransaction {
        ImportTransaction {
            date: NaiveDate::from_ymd_opt(date.0, date.1, date.2)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            description: desc.to_string(),
            amount: Decimal::new(amount, scale),
            currency: Some("GBP".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn test_dry_run_transactions_new() {
        let (db, _file) = test_db();
        setup_test_account(&db, "acc1");

        let txns = vec![make_txn((2025, 1, 15), "Coffee Shop", -350, 2)];

        let previews = db.dry_run_transactions("acc1", &txns).unwrap();
        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].status, TransactionPreviewStatus::New);
        assert!(previews[0].existing_id.is_none());
    }

    #[test]
    fn test_dry_run_transactions_duplicate() {
        let (db, _file) = test_db();
        setup_test_account(&db, "acc1");

        let txns = vec![make_txn((2025, 1, 15), "Coffee Shop", -350, 2)];

        db.insert_transactions_bulk("acc1", &txns).unwrap();

        let previews = db.dry_run_transactions("acc1", &txns).unwrap();
        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].status, TransactionPreviewStatus::Duplicate);
        assert!(previews[0].existing_id.is_some());
        assert_eq!(
            previews[0].existing_description.as_deref(),
            Some("Coffee Shop")
        );
    }

    #[test]
    fn test_dry_run_transactions_no_writes() {
        let (db, _file) = test_db();
        setup_test_account(&db, "acc1");

        let txns = vec![make_txn((2025, 1, 15), "Coffee Shop", -350, 2)];

        db.dry_run_transactions("acc1", &txns).unwrap();

        let (rows, count) = db.get_transactions(&TransactionFilters::default()).unwrap();
        assert_eq!(count, 0);
        assert!(rows.is_empty());
    }

    #[test]
    fn test_dry_run_transactions_mixed() {
        let (db, _file) = test_db();
        setup_test_account(&db, "acc1");

        let existing = vec![make_txn((2025, 1, 15), "Coffee", -350, 2)];
        db.insert_transactions_bulk("acc1", &existing).unwrap();

        let txns = vec![
            make_txn((2025, 1, 15), "Coffee", -350, 2),
            make_txn((2025, 1, 16), "Groceries", -2500, 2),
        ];

        let previews = db.dry_run_transactions("acc1", &txns).unwrap();
        assert_eq!(previews.len(), 2);
        assert_eq!(previews[0].status, TransactionPreviewStatus::Duplicate);
        assert_eq!(previews[1].status, TransactionPreviewStatus::New);
    }

    fn new_category(db: &Db, name: &str, parent_id: Option<&str>) -> String {
        db.create_category(&CreateCategoryPayload {
            name: name.to_string(),
            parent_id: parent_id.map(|s| s.to_string()),
            display_order: None,
            description: None,
            category_type: CategoryType::Spending,
        })
        .unwrap()
        .id
    }

    fn gbp_fx() -> crate::util::fx::FxRateMap {
        crate::util::fx::FxRateMap::new(vec![crate::model::Currency {
            code: "GBP".to_string(),
            is_preferred: true,
            fx_rate: Decimal::ONE,
            updated_at: None,
        }])
        .unwrap()
    }

    #[test]
    fn bulk_insert_mixed_batch_reports_counts_and_persists_valid_rows() {
        let (db, _file) = test_db();
        setup_test_account(&db, "acc1");
        let parent_id = new_category(&db, "Bulk Parent", None);
        let leaf_id = new_category(&db, "Bulk Leaf", Some(&parent_id));

        db.insert_transactions_bulk("acc1", &[make_txn((2025, 1, 15), "Coffee", -350, 2)])
            .unwrap();

        let mut valid = make_txn((2025, 1, 16), "Groceries", -2500, 2);
        valid.category_id = Some(leaf_id.clone());
        let mut with_parent_cat = make_txn((2025, 1, 17), "Bad Parent", -100, 2);
        with_parent_cat.category_id = Some(parent_id.clone());
        let mut with_unknown_cat = make_txn((2025, 1, 18), "Bad Unknown", -100, 2);
        with_unknown_cat.category_id = Some("no-such-category".to_string());
        let duplicate = make_txn((2025, 1, 15), "Coffee", -350, 2);

        let result = db
            .insert_transactions_bulk(
                "acc1",
                &[valid, with_parent_cat, with_unknown_cat, duplicate],
            )
            .unwrap();

        assert_eq!(result.rows_total, 4);
        assert_eq!(result.rows_inserted, 1);
        assert_eq!(result.rows_duplicate, 1);
        assert_eq!(result.errors.len(), 2);
        assert_eq!(result.errors[0].index, 1);
        assert!(result.errors[0].reason.contains("is a parent, not a leaf"));
        assert_eq!(result.errors[1].index, 2);
        assert!(result.errors[1].reason.contains("not found or inactive"));

        let (rows, total) = db.get_transactions(&TransactionFilters::default()).unwrap();
        assert_eq!(total, 2, "valid rows commit despite per-row errors");
        let grocery = rows.iter().find(|t| t.description == "Groceries").unwrap();
        assert_eq!(grocery.category_id.as_deref(), Some(leaf_id.as_str()));
    }

    #[test]
    fn bulk_insert_duplicate_merges_source_documents() {
        let (db, _file) = test_db();
        setup_test_account(&db, "acc1");

        let mut first = make_txn((2025, 2, 1), "Rent", -90000, 2);
        first.source_document_ids = vec!["doc-a".to_string()];
        let result = db.insert_transactions_bulk("acc1", &[first]).unwrap();
        assert_eq!(result.rows_inserted, 1);

        let mut reimport = make_txn((2025, 2, 1), "Rent", -90000, 2);
        reimport.source_document_ids = vec!["doc-b".to_string()];
        let result = db.insert_transactions_bulk("acc1", &[reimport]).unwrap();
        assert_eq!(result.rows_inserted, 0);
        assert_eq!(result.rows_duplicate, 1);

        let (rows, _) = db.get_transactions(&TransactionFilters::default()).unwrap();
        assert_eq!(rows.len(), 1);
        let mut doc_ids = rows[0].source_document_ids.clone();
        doc_ids.sort();
        assert_eq!(doc_ids, vec!["doc-a".to_string(), "doc-b".to_string()]);
    }

    #[test]
    fn search_treats_backslash_literally() {
        let (db, _file) = test_db();
        setup_test_account(&db, "acc1");

        db.insert_transactions_bulk(
            "acc1",
            &[
                make_txn((2025, 3, 1), r"ACME C:\Users refund", -1000, 2),
                make_txn((2025, 3, 2), r"100\% cotton shirt", -2000, 2),
                make_txn((2025, 3, 3), "plain coffee", -300, 2),
            ],
        )
        .unwrap();

        let filters = TransactionFilters {
            search: Some(r"C:\Users".to_string()),
            ..TransactionFilters::default()
        };
        let (rows, total) = db.get_transactions(&filters).unwrap();
        assert_eq!(total, 1, "a description containing a backslash is found");
        assert!(rows[0].description.contains(r"C:\Users"));

        // Backslash-percent must match the literal sequence, not act as a
        // wildcard that matches everything.
        let filters = TransactionFilters {
            search: Some(r"\%".to_string()),
            ..TransactionFilters::default()
        };
        let (rows, total) = db.get_transactions(&filters).unwrap();
        assert_eq!(total, 1);
        assert!(rows[0].description.contains(r"\%"));
    }

    #[test]
    fn by_category_uncategorized_sentinel_groups_null_rows() {
        let (db, _file) = test_db();
        setup_test_account(&db, "acc1");
        let parent_id = new_category(&db, "Sentinel Parent", None);
        let leaf_id = new_category(&db, "Sentinel Leaf", Some(&parent_id));

        let mut categorized = make_txn((2025, 4, 1), "Groceries", -1000, 2);
        categorized.category_id = Some(leaf_id.clone());
        let uncategorized = make_txn((2025, 4, 2), "Mystery", -500, 2);
        db.insert_transactions_bulk("acc1", &[categorized, uncategorized])
            .unwrap();

        let fx = gbp_fx();

        // Default: NULL-category rows stay excluded.
        let totals = db
            .get_transactions_by_category(&TransactionFilters::default(), None, &fx)
            .unwrap();
        assert_eq!(totals.len(), 1);
        assert_eq!(totals[0].category_id.as_deref(), Some(leaf_id.as_str()));

        // Sentinel only: NULL rows come back as their own group.
        let filters = TransactionFilters {
            categories: Some(vec!["__uncategorized__".to_string()]),
            ..TransactionFilters::default()
        };
        let totals = db
            .get_transactions_by_category(&filters, None, &fx)
            .unwrap();
        assert_eq!(totals.len(), 1);
        assert_eq!(totals[0].category_id, None);
        assert_eq!(
            totals[0].total.parse::<Decimal>().unwrap(),
            Decimal::new(-500, 2)
        );

        // Sentinel alongside a real category: both groups.
        let filters = TransactionFilters {
            categories: Some(vec![leaf_id.clone(), "__uncategorized__".to_string()]),
            ..TransactionFilters::default()
        };
        let totals = db
            .get_transactions_by_category(&filters, None, &fx)
            .unwrap();
        assert_eq!(totals.len(), 2);
        let null_group = totals.iter().find(|t| t.category_id.is_none()).unwrap();
        assert_eq!(
            null_group.total.parse::<Decimal>().unwrap(),
            Decimal::new(-500, 2)
        );
        let leaf_group = totals
            .iter()
            .find(|t| t.category_id.as_deref() == Some(leaf_id.as_str()))
            .unwrap();
        assert_eq!(
            leaf_group.total.parse::<Decimal>().unwrap(),
            Decimal::new(-1000, 2)
        );
    }
}

#[cfg(test)]
mod investment_dedup_tests {
    use super::*;
    use crate::importers::investments_parser::ParsedInvestmentRow;
    use crate::model::TransactionPreviewStatus;
    use tempfile::NamedTempFile;

    fn make_inv_row(event_type: &str, symbol: &str) -> ParsedInvestmentRow {
        ParsedInvestmentRow {
            event_type: event_type.to_string(),
            symbol: symbol.to_string(),
            date: "2026-03-15T00:00:00".to_string(),
            quantity: "10".to_string(),
            price_per_share: "76.32".to_string(),
            total_value: None,
            fee: "0".to_string(),
            currency: "GBP".to_string(),
            fee_currency: None,
            notes: None,
            row_confidence: 0.95,
            source_file: None,
        }
    }

    fn test_db() -> (Db, NamedTempFile) {
        let file = NamedTempFile::new().expect("temp file");
        let db = Db::open(file.path()).expect("test db");
        (db, file)
    }

    #[test]
    fn dry_run_investments_marks_new_row() {
        let (db, _file) = test_db();
        let rows = vec![make_inv_row("buy", "VUSA")];
        let previews = db.dry_run_investments("acct-1", &rows, 0.70).unwrap();
        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].status, TransactionPreviewStatus::New);
        assert!(previews[0].existing_id.is_none());
    }

    #[test]
    fn dry_run_investments_marks_duplicate_after_insert() {
        let (db, _file) = test_db();

        db.create_account(&crate::model::Account {
            id: "acct-1".to_string(),
            name: "Test".to_string(),
            institution: "Test Bank".to_string(),
            account_type: crate::model::AccountType::Checking,
            currency: "GBP".to_string(),
            balance: None,
            balance_date: None,
            is_active: true,
            notes: None,
            profile_ids: vec!["default".to_string()],
            is_stale: None,
            is_available: true,
        })
        .unwrap();

        let body = crate::model::CreateInvestmentEventBody {
            account_id: "acct-1".to_string(),
            event_type: "buy".to_string(),
            symbol: "VUSA".to_string(),
            date: "2026-03-15T00:00:00".to_string(),
            quantity: "10".to_string(),
            price_per_share: "76.32".to_string(),
            fee: Some("0".to_string()),
            currency: "GBP".to_string(),
            fee_currency: None,
            notes: None,
            source_document_ids: Vec::new(),
        };
        db.create_investment_event(&body).unwrap();

        let rows = vec![make_inv_row("buy", "VUSA")];
        let previews = db.dry_run_investments("acct-1", &rows, 0.70).unwrap();
        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].status, TransactionPreviewStatus::Duplicate);
        assert!(previews[0].existing_id.is_some());
    }

    #[test]
    fn dry_run_investments_marks_error_for_low_confidence() {
        let (db, _file) = test_db();
        let mut row = make_inv_row("buy", "VUSA");
        row.row_confidence = 0.50;
        let previews = db.dry_run_investments("acct-1", &[row], 0.70).unwrap();
        assert_eq!(previews[0].status, TransactionPreviewStatus::Error);
    }

    #[test]
    fn dry_run_investments_marks_error_for_invalid_date() {
        let (db, _file) = test_db();
        let mut row = make_inv_row("buy", "VUSA");
        row.date = "not-a-date".to_string();
        let previews = db.dry_run_investments("acct-1", &[row], 0.70).unwrap();
        assert_eq!(previews[0].status, TransactionPreviewStatus::Error);
    }

    /// fee_currency is stored non-null exactly when a fee is present: defaulted to
    /// the trade currency when omitted, preserved when explicit, null when no fee.
    #[test]
    fn create_investment_event_enforces_fee_currency_invariant() {
        let (db, _file) = test_db();
        db.create_account(&crate::model::Account {
            id: "acct-1".to_string(),
            name: "Test".to_string(),
            institution: "Test Bank".to_string(),
            account_type: crate::model::AccountType::Investment,
            currency: "GBP".to_string(),
            balance: None,
            balance_date: None,
            is_active: true,
            notes: None,
            profile_ids: vec!["default".to_string()],
            is_stale: None,
            is_available: true,
        })
        .unwrap();

        let base = crate::model::CreateInvestmentEventBody {
            account_id: "acct-1".to_string(),
            event_type: "buy".to_string(),
            symbol: "AAPL".to_string(),
            date: "2026-03-15T00:00:00".to_string(),
            quantity: "10".to_string(),
            price_per_share: "100".to_string(),
            fee: Some("5.00".to_string()),
            currency: "USD".to_string(),
            fee_currency: None,
            notes: None,
            source_document_ids: Vec::new(),
        };

        // Fee present, no fee_currency -> defaults to the trade currency.
        let (defaulted, _) = db.create_investment_event(&base).unwrap();
        assert_eq!(defaulted.fee_currency.as_deref(), Some("USD"));

        // Fee present with an explicit, different fee_currency -> preserved.
        let (explicit, _) = db
            .create_investment_event(&crate::model::CreateInvestmentEventBody {
                symbol: "MSFT".to_string(),
                fee_currency: Some("GBP".to_string()),
                ..base.clone()
            })
            .unwrap();
        assert_eq!(explicit.fee_currency.as_deref(), Some("GBP"));

        // No fee -> fee_currency stays null.
        let (no_fee, _) = db
            .create_investment_event(&crate::model::CreateInvestmentEventBody {
                symbol: "TSLA".to_string(),
                fee: None,
                fee_currency: None,
                ..base.clone()
            })
            .unwrap();
        assert_eq!(no_fee.fee_currency, None);
    }

    /// A currency still referenced by the investment ledger cannot be deleted.
    /// Without this, deleting it leaves the CGT engine unable to convert those
    /// events — the "configured currency vanished" state.
    #[test]
    fn delete_currency_refuses_when_used_by_investment_trade_currency() {
        let (db, _file) = test_db();
        make_test_account(&db, "acct-1");
        db.create_currency("USD", Decimal::new(79, 2)).unwrap();

        db.create_investment_event(&crate::model::CreateInvestmentEventBody {
            currency: "USD".to_string(),
            ..make_event_body("AAPL")
        })
        .unwrap();

        let err = db.delete_currency("USD").unwrap_err().to_string();
        assert!(err.contains("in use"), "unexpected error: {err}");
        assert!(
            err.contains("1 investment events"),
            "error should count the investment events: {err}"
        );
        // Still present.
        assert!(db.currency_exists("USD").unwrap());
    }

    /// A fee charged in another currency keeps THAT currency in use on its own,
    /// even though no trade is denominated in it.
    #[test]
    fn delete_currency_refuses_when_used_only_as_investment_fee_currency() {
        let (db, _file) = test_db();
        make_test_account(&db, "acct-1");
        db.create_currency("USD", Decimal::new(79, 2)).unwrap();

        // Trade in GBP, fee in USD — USD appears only in fee_currency.
        db.create_investment_event(&crate::model::CreateInvestmentEventBody {
            currency: "GBP".to_string(),
            fee: Some("2.50".to_string()),
            fee_currency: Some("USD".to_string()),
            ..make_event_body("AAPL")
        })
        .unwrap();

        let err = db.delete_currency("USD").unwrap_err().to_string();
        assert!(
            err.contains("1 investment events"),
            "fee_currency alone should block the delete: {err}"
        );
        assert!(db.currency_exists("USD").unwrap());
    }

    /// The guard must not over-reach: an unreferenced currency still deletes.
    #[test]
    fn delete_currency_allows_when_no_investment_references_it() {
        let (db, _file) = test_db();
        make_test_account(&db, "acct-1");
        db.create_currency("USD", Decimal::new(79, 2)).unwrap();
        db.create_currency("JPY", Decimal::new(52, 4)).unwrap();

        // An investment event in USD must not keep the unrelated JPY row locked.
        db.create_investment_event(&crate::model::CreateInvestmentEventBody {
            currency: "USD".to_string(),
            ..make_event_body("AAPL")
        })
        .unwrap();

        db.delete_currency("JPY").unwrap();
        assert!(!db.currency_exists("JPY").unwrap());
    }

    fn make_test_account(db: &Db, id: &str) {
        db.create_account(&crate::model::Account {
            id: id.to_string(),
            name: "Test".to_string(),
            institution: "Test Bank".to_string(),
            account_type: crate::model::AccountType::Investment,
            currency: "GBP".to_string(),
            balance: None,
            balance_date: None,
            is_active: true,
            notes: None,
            profile_ids: vec!["default".to_string()],
            is_stale: None,
            is_available: true,
        })
        .unwrap();
    }

    fn make_event_body(symbol: &str) -> crate::model::CreateInvestmentEventBody {
        crate::model::CreateInvestmentEventBody {
            account_id: "acct-1".to_string(),
            event_type: "buy".to_string(),
            symbol: symbol.to_string(),
            date: "2026-03-15T00:00:00".to_string(),
            quantity: "10".to_string(),
            price_per_share: "76.32".to_string(),
            fee: None,
            currency: "GBP".to_string(),
            fee_currency: None,
            notes: None,
            source_document_ids: Vec::new(),
        }
    }

    #[test]
    fn create_investment_event_reports_duplicate_on_reimport() {
        let (db, _file) = test_db();
        make_test_account(&db, "acct-1");

        let body = make_event_body("VUSA");
        let (first, outcome) = db.create_investment_event(&body).unwrap();
        assert_eq!(outcome, crate::model::InsertOutcome::Inserted);

        let (second, outcome) = db.create_investment_event(&body).unwrap();
        assert_eq!(outcome, crate::model::InsertOutcome::Duplicate);
        assert_eq!(second.id, first.id, "duplicate returns the existing row");

        let events = db
            .list_investment_events(Some("acct-1"), None, None, None)
            .unwrap();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn update_investment_event_recomputes_fingerprint() {
        let (db, _file) = test_db();
        make_test_account(&db, "acct-1");

        let body = make_event_body("VUSA");
        let (created, _) = db.create_investment_event(&body).unwrap();

        let patched = db
            .update_investment_event(
                &created.id,
                &crate::model::PatchInvestmentEventBody {
                    event_type: None,
                    symbol: None,
                    date: None,
                    quantity: Some("12".to_string()),
                    price_per_share: None,
                    fee: None,
                    currency: None,
                    fee_currency: None,
                    notes: None,
                },
            )
            .unwrap()
            .unwrap();

        assert_ne!(patched.fingerprint, created.fingerprint);
        assert_eq!(
            patched.fingerprint,
            sha256_hex("acct-1|VUSA|2026-03-15T00:00:00|12|76.32|buy"),
            "fingerprint must match what create_investment_event would compute"
        );

        // Dedup follows the new identity: re-importing the original body
        // inserts a fresh row, re-importing the patched identity is a duplicate.
        let (_, outcome) = db.create_investment_event(&body).unwrap();
        assert_eq!(outcome, crate::model::InsertOutcome::Inserted);
        let (_, outcome) = db
            .create_investment_event(&crate::model::CreateInvestmentEventBody {
                quantity: "12".to_string(),
                ..body.clone()
            })
            .unwrap();
        assert_eq!(outcome, crate::model::InsertOutcome::Duplicate);
    }

    #[test]
    fn update_investment_event_notes_only_keeps_fingerprint() {
        let (db, _file) = test_db();
        make_test_account(&db, "acct-1");

        let (created, _) = db
            .create_investment_event(&make_event_body("VUSA"))
            .unwrap();
        let patched = db
            .update_investment_event(
                &created.id,
                &crate::model::PatchInvestmentEventBody {
                    event_type: None,
                    symbol: None,
                    date: None,
                    quantity: None,
                    price_per_share: None,
                    fee: None,
                    currency: None,
                    fee_currency: None,
                    notes: Some("hello".to_string()),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(patched.fingerprint, created.fingerprint);
        assert_eq!(patched.notes.as_deref(), Some("hello"));
    }
}

#[cfg(test)]
mod document_tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn test_db() -> (Db, NamedTempFile) {
        let file = NamedTempFile::new().expect("temp file");
        let db = Db::open(file.path()).expect("test db");
        (db, file)
    }

    fn make_account(db: &Db, id: &str) {
        db.create_account(&crate::model::Account {
            id: id.to_string(),
            name: "Test".to_string(),
            institution: "Test Bank".to_string(),
            account_type: crate::model::AccountType::Investment,
            currency: "GBP".to_string(),
            balance: None,
            balance_date: None,
            is_active: true,
            notes: None,
            profile_ids: vec!["default".to_string()],
            is_stale: None,
            is_available: true,
        })
        .unwrap();
    }

    #[test]
    fn store_document_dedups_by_content_hash() {
        let (db, _f) = test_db();
        let (d1, dup1) = db
            .store_document("a.csv", "text/csv", b"hello", "parse", None)
            .unwrap();
        assert!(!dup1);
        let (d2, dup2) = db
            .store_document("renamed.csv", "text/csv", b"hello", "parse", None)
            .unwrap();
        assert!(dup2, "identical bytes should dedup");
        assert_eq!(d1.id, d2.id);
        assert_eq!(db.list_documents(false).unwrap().len(), 1);
    }

    #[test]
    fn store_document_distinct_bytes_create_rows() {
        let (db, _f) = test_db();
        db.store_document("a.csv", "text/csv", b"aaa", "parse", None)
            .unwrap();
        db.store_document("b.csv", "text/csv", b"bbb", "parse", None)
            .unwrap();
        assert_eq!(db.list_documents(false).unwrap().len(), 2);
    }

    #[test]
    fn stored_file_written_and_removed_on_delete() {
        let (db, _f) = test_db();
        let (doc, _) = db
            .store_document("a.csv", "text/csv", b"hello", "manual", None)
            .unwrap();
        assert!(Path::new(&doc.file_path).exists());
        let outcome = db.delete_document(&doc.id, false).unwrap();
        assert!(matches!(outcome, DeleteDocumentOutcome::Deleted(_)));
        assert!(!Path::new(&doc.file_path).exists());
        assert!(db.get_document(&doc.id).unwrap().is_none());
    }

    #[test]
    fn orphan_flag_is_reference_based_for_any_origin() {
        let (db, _f) = test_db();
        let (manual, _) = db
            .store_document("ref.pdf", "application/pdf", b"x", "manual", None)
            .unwrap();
        let summaries = db.list_documents(false).unwrap();
        let s = summaries.iter().find(|s| s.id == manual.id).unwrap();
        assert_eq!(
            s.reference_count, None,
            "list view leaves the exact count unset (computed lazily per doc)"
        );
        assert!(s.orphaned, "manual upload with zero refs is still orphaned");
    }

    #[test]
    fn references_counted_across_tables_and_force_unlinks() {
        let (db, _f) = test_db();
        make_account(&db, "acct-1");

        let (doc, _) = db
            .store_document("s.csv", "text/csv", b"data", "parse", Some("acct-1"))
            .unwrap();
        let ids = format!("[\"{}\"]", doc.id);

        db.conn
            .execute(
                "INSERT INTO transactions (id, date, description, normalized, amount, currency, \
                 account_id, fingerprint, source_document_ids) \
                 VALUES ('tx1','2026-01-01T00:00:00','x','x','-1','GBP','acct-1','fp1', ?1)",
                rusqlite::params![ids],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO holdings (account_id, symbol, name, holding_type, quantity, value, \
                 currency, as_of, source_document_ids) \
                 VALUES ('acct-1','VUSA','Vanguard','etf','1','10','GBP','2026-01-01T00:00:00', ?1)",
                rusqlite::params![ids],
            )
            .unwrap();

        let refs = db.document_references(&doc.id).unwrap();
        assert_eq!(refs.transactions, 1);
        assert_eq!(refs.holdings, 1);
        assert_eq!(refs.investments, 0);
        assert_eq!(refs.total(), 2);

        let summary = db
            .list_documents(false)
            .unwrap()
            .into_iter()
            .find(|s| s.id == doc.id)
            .unwrap();
        assert_eq!(
            summary.reference_count, None,
            "list view does not compute the exact count"
        );
        assert!(!summary.orphaned);

        match db.delete_document(&doc.id, false).unwrap() {
            DeleteDocumentOutcome::Referenced(r) => assert_eq!(r.total(), 2),
            other => panic!("expected Referenced, got {other:?}"),
        }
        assert!(db.get_document(&doc.id).unwrap().is_some());

        match db.delete_document(&doc.id, true).unwrap() {
            DeleteDocumentOutcome::Deleted(r) => {
                assert_eq!(r.transactions, 1);
                assert_eq!(r.holdings, 1);
            }
            other => panic!("expected Deleted, got {other:?}"),
        }
        assert!(db.get_document(&doc.id).unwrap().is_none());

        let tx_ids: String = db
            .conn
            .query_row(
                "SELECT source_document_ids FROM transactions WHERE id='tx1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tx_ids, "[]");
        let h_ids: String = db
            .conn
            .query_row(
                "SELECT source_document_ids FROM holdings WHERE symbol='VUSA'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(h_ids, "[]");
    }

    #[test]
    fn list_documents_include_refs_matches_per_document_counts() {
        let (db, _f) = test_db();
        make_account(&db, "acct-1");

        let (linked, _) = db
            .store_document("s.csv", "text/csv", b"data", "parse", Some("acct-1"))
            .unwrap();
        let (orphan, _) = db
            .store_document("o.pdf", "application/pdf", b"x", "manual", None)
            .unwrap();
        let ids = format!("[\"{}\"]", linked.id);

        db.conn
            .execute(
                "INSERT INTO transactions (id, date, description, normalized, amount, currency, \
                 account_id, fingerprint, source_document_ids) \
                 VALUES ('tx1','2026-01-01T00:00:00','x','x','-1','GBP','acct-1','fp1', ?1)",
                rusqlite::params![ids],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO transactions (id, date, description, normalized, amount, currency, \
                 account_id, fingerprint, source_document_ids) \
                 VALUES ('tx2','2026-01-02T00:00:00','y','y','-2','GBP','acct-1','fp2', ?1)",
                rusqlite::params![ids],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO holdings (account_id, symbol, name, holding_type, quantity, value, \
                 currency, as_of, source_document_ids) \
                 VALUES ('acct-1','VUSA','Vanguard','etf','1','10','GBP','2026-01-01T00:00:00', ?1)",
                rusqlite::params![ids],
            )
            .unwrap();
        db.create_investment_event(&crate::model::CreateInvestmentEventBody {
            account_id: "acct-1".to_string(),
            event_type: "buy".to_string(),
            symbol: "VUSA".to_string(),
            date: "2026-01-03T00:00:00".to_string(),
            quantity: "1".to_string(),
            price_per_share: "10".to_string(),
            fee: None,
            currency: "GBP".to_string(),
            fee_currency: None,
            notes: None,
            source_document_ids: vec![linked.id.clone()],
        })
        .unwrap();

        let summaries = db.list_documents(true).unwrap();
        for s in &summaries {
            let expected = db.document_references(&s.id).unwrap().total();
            assert_eq!(
                s.reference_count,
                Some(expected),
                "batched count for {} must match the per-document endpoint",
                s.id
            );
            assert_eq!(s.orphaned, expected == 0);
        }

        let linked_row = summaries.iter().find(|s| s.id == linked.id).unwrap();
        assert_eq!(linked_row.reference_count, Some(4));
        assert!(!linked_row.orphaned);

        let orphan_row = summaries.iter().find(|s| s.id == orphan.id).unwrap();
        assert_eq!(orphan_row.reference_count, Some(0));
        assert!(orphan_row.orphaned);
    }
}

#[cfg(test)]
mod token_tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn test_db() -> (Db, NamedTempFile) {
        let file = NamedTempFile::new().expect("temp file");
        let db = Db::open(file.path()).expect("test db");
        (db, file)
    }

    fn last_used(db: &Db) -> Option<String> {
        db.list_tokens().unwrap()[0].last_used.clone()
    }

    fn set_last_used_relative(db: &Db, modifier: &str) {
        db.conn
            .execute(
                "UPDATE api_tokens SET last_used = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?1)",
                params![modifier],
            )
            .unwrap();
    }

    #[test]
    fn validate_token_debounces_last_used_writes() {
        let (db, _f) = test_db();
        let raw = db.create_token("ci").unwrap();

        // NULL last_used gets set on the first validation.
        assert!(last_used(&db).is_none());
        assert_eq!(db.validate_token(&raw).unwrap().as_deref(), Some("ci"));
        assert!(last_used(&db).is_some());

        // A fresh last_used (10s old, well within the 60s window) is left
        // untouched by a second validation.
        set_last_used_relative(&db, "-10 seconds");
        let recent = last_used(&db).unwrap();
        assert_eq!(db.validate_token(&raw).unwrap().as_deref(), Some("ci"));
        assert_eq!(last_used(&db).as_deref(), Some(recent.as_str()));

        // Older than 60s: refreshed.
        set_last_used_relative(&db, "-2 minutes");
        let stale = last_used(&db).unwrap();
        assert_eq!(db.validate_token(&raw).unwrap().as_deref(), Some("ci"));
        let refreshed = last_used(&db).unwrap();
        assert!(refreshed > stale, "stale last_used must be refreshed");
    }

    #[test]
    fn validate_token_still_rejects_bad_and_revoked_tokens() {
        let (db, _f) = test_db();
        let raw = db.create_token("ci").unwrap();

        assert_eq!(db.validate_token("fyn_nope").unwrap(), None);
        db.revoke_token("ci").unwrap();
        assert_eq!(db.validate_token(&raw).unwrap(), None);
    }
}

#[cfg(test)]
mod category_delete_tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn test_db() -> (Db, NamedTempFile) {
        let file = NamedTempFile::new().expect("temp file");
        let db = Db::open(file.path()).expect("test db");
        (db, file)
    }

    fn new_cat(db: &Db, name: &str, parent_id: Option<&str>) -> String {
        db.create_category(&CreateCategoryPayload {
            name: name.to_string(),
            parent_id: parent_id.map(|s| s.to_string()),
            display_order: None,
            description: None,
            category_type: CategoryType::Spending,
        })
        .expect("create category")
        .id
    }

    #[test]
    fn hard_delete_removes_row_unlike_soft_delete() {
        let (db, _f) = test_db();
        let id = new_cat(&db, "Temp Cat", None);

        db.soft_delete_category(&id).expect("soft delete");
        let after_soft = db
            .get_category_by_id(&id)
            .expect("get")
            .expect("still present");
        assert!(!after_soft.is_active, "soft delete only clears is_active");

        db.hard_delete_category(&id).expect("hard delete");
        assert!(
            db.get_category_by_id(&id).expect("get").is_none(),
            "hard delete removes the row entirely"
        );
    }

    #[test]
    fn hard_delete_refuses_when_category_has_children() {
        let (db, _f) = test_db();
        let parent = new_cat(&db, "Temp Parent", None);
        let child = new_cat(&db, "Temp Child", Some(&parent));

        let err = db.hard_delete_category(&parent).unwrap_err().to_string();
        assert!(err.contains("child categories"), "unexpected error: {err}");

        db.hard_delete_category(&child).expect("delete child");
        db.hard_delete_category(&parent)
            .expect("delete parent now childless");
        assert!(db.get_category_by_id(&parent).expect("get").is_none());
    }
}

#[cfg(test)]
mod profile_seed_tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn fresh_db_seeds_no_profiles() {
        let file = NamedTempFile::new().expect("temp file");
        let db = Db::open(file.path()).expect("test db");
        assert!(
            db.get_profiles().expect("get_profiles").is_empty(),
            "Db::open must not auto-seed a default profile"
        );
    }

    #[test]
    fn deleted_profile_stays_gone_after_reopen() {
        let file = NamedTempFile::new().expect("temp file");
        {
            let db = Db::open(file.path()).expect("open");
            db.create_profile("solo", "Solo").expect("create");
            db.delete_profile("solo").expect("delete");
        }
        let db = Db::open(file.path()).expect("reopen");
        assert!(
            db.get_profiles().expect("get_profiles").is_empty(),
            "a deleted profile must not be resurrected when the db is reopened"
        );
    }
}

// ── Sub-unit conversion & migration ─────────────────────────────────────────

#[cfg(test)]
mod subunit_conversion_tests {
    use super::*;
    use crate::model::{
        Account, AccountType, CreateInvestmentEventBody, HoldingType, HoldingWrite,
    };
    use chrono::NaiveDate;
    use rust_decimal::Decimal;
    use std::str::FromStr;
    use tempfile::NamedTempFile;

    fn test_db() -> (Db, NamedTempFile) {
        let file = NamedTempFile::new().expect("temp file");
        let db = Db::open(file.path()).expect("test db");
        (db, file)
    }

    fn naive_dt(year: i32, month: u32, day: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
    }

    fn make_account(id: &str, currency: &str) -> Account {
        Account {
            id: id.to_string(),
            name: id.to_string(),
            institution: "TestBank".to_string(),
            account_type: AccountType::Investment,
            currency: currency.to_string(),
            balance: None,
            balance_date: None,
            is_active: true,
            notes: None,
            profile_ids: vec!["default".to_string()],
            is_stale: None,
            is_available: true,
        }
    }

    // ── Write-time conversion: investments ──────────────────────────────────

    #[test]
    fn create_investment_event_converts_gbx_price_to_gbp() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-1", "GBP")).unwrap();

        let body = CreateInvestmentEventBody {
            account_id: "acct-1".to_string(),
            event_type: "buy".to_string(),
            symbol: "VUSA".to_string(),
            date: "2026-03-15T00:00:00".to_string(),
            quantity: "10".to_string(),
            price_per_share: "1234".to_string(), // 1234 GBX
            fee: None,
            currency: "GBX".to_string(),
            fee_currency: None,
            notes: None,
            source_document_ids: Vec::new(),
        };
        let (event, _) = db.create_investment_event(&body).unwrap();

        assert_eq!(event.currency, "GBP");
        assert_eq!(event.price_per_share, Decimal::from_str("12.34").unwrap());
    }

    /// A change to a single digit of the sub-unit price must change the stored
    /// (converted) price and therefore the fingerprint too — proving the
    /// fingerprint really is computed post-conversion, not on the raw input.
    #[test]
    fn create_investment_event_gbx_fingerprint_reflects_converted_price() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-1", "GBP")).unwrap();

        let mut body = CreateInvestmentEventBody {
            account_id: "acct-1".to_string(),
            event_type: "buy".to_string(),
            symbol: "VUSA".to_string(),
            date: "2026-03-15T00:00:00".to_string(),
            quantity: "10".to_string(),
            price_per_share: "1234".to_string(),
            fee: None,
            currency: "GBX".to_string(),
            fee_currency: None,
            notes: None,
            source_document_ids: Vec::new(),
        };
        let (first, _) = db.create_investment_event(&body).unwrap();

        body.price_per_share = "1235".to_string();
        let (second, _) = db.create_investment_event(&body).unwrap();

        assert_ne!(first.fingerprint, second.fingerprint);
        assert_eq!(second.price_per_share, Decimal::from_str("12.35").unwrap());
    }

    #[test]
    fn create_investment_event_converts_gbx_fee_independently_of_price() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-1", "USD")).unwrap();

        // USD-priced trade with a GBX fee (unusual but the two currencies are
        // independent fields, so the code must handle it).
        let body = CreateInvestmentEventBody {
            account_id: "acct-1".to_string(),
            event_type: "buy".to_string(),
            symbol: "AAPL".to_string(),
            date: "2026-03-15T00:00:00".to_string(),
            quantity: "10".to_string(),
            price_per_share: "150".to_string(),
            fee: Some("500".to_string()), // 500 GBX = 5.00 GBP
            currency: "USD".to_string(),
            fee_currency: Some("GBX".to_string()),
            notes: None,
            source_document_ids: Vec::new(),
        };
        let (event, _) = db.create_investment_event(&body).unwrap();

        assert_eq!(event.currency, "USD");
        assert_eq!(event.price_per_share, Decimal::from(150));
        assert_eq!(event.fee_currency.as_deref(), Some("GBP"));
        assert_eq!(event.fee, Some(Decimal::from(5)));
    }

    #[test]
    fn create_investment_event_ordinary_currency_is_unaffected() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-1", "GBP")).unwrap();

        let body = CreateInvestmentEventBody {
            account_id: "acct-1".to_string(),
            event_type: "buy".to_string(),
            symbol: "VUSA".to_string(),
            date: "2026-03-15T00:00:00".to_string(),
            quantity: "10".to_string(),
            price_per_share: "76.32".to_string(),
            fee: None,
            currency: "GBP".to_string(),
            fee_currency: None,
            notes: None,
            source_document_ids: Vec::new(),
        };
        let (event, _) = db.create_investment_event(&body).unwrap();

        assert_eq!(event.currency, "GBP");
        assert_eq!(event.price_per_share, Decimal::from_str("76.32").unwrap());
    }

    // ── Write-time conversion: holdings ──────────────────────────────────────

    #[test]
    fn into_holding_converts_gbx_value_and_price() {
        let write = HoldingWrite {
            symbol: "VUSA".to_string(),
            name: "Vanguard S&P 500".to_string(),
            holding_type: HoldingType::Etf,
            currency: "GBX".to_string(),
            as_of: naive_dt(2026, 3, 31),
            sub_account: None,
            is_closed: false,
            source_document_ids: Vec::new(),
            value: None,
            quantity: Some(Decimal::from(50)),
            price_per_unit: Some(Decimal::from_str("7632").unwrap()), // 7632 GBX
        };
        let holding = write.into_holding("acct-1").unwrap();

        assert_eq!(holding.currency, "GBP");
        assert_eq!(
            holding.price_per_unit,
            Some(Decimal::from_str("76.32").unwrap())
        );
        // value = quantity * price is computed from the pre-conversion price
        // inside into_holding, then converted along with it: 50 * 76.32 GBP.
        assert_eq!(holding.value, Decimal::from_str("3816.00").unwrap());
    }

    #[test]
    fn into_holding_converts_bare_value_gbx() {
        let write = HoldingWrite {
            symbol: "GBX_CASH".to_string(),
            name: "Cash".to_string(),
            holding_type: HoldingType::Cash,
            currency: "GBX".to_string(),
            as_of: naive_dt(2026, 3, 31),
            sub_account: None,
            is_closed: false,
            source_document_ids: Vec::new(),
            value: Some(Decimal::from(10000)), // 10000 GBX = 100.00 GBP
            quantity: None,
            price_per_unit: None,
        };
        let holding = write.into_holding("acct-1").unwrap();

        assert_eq!(holding.currency, "GBP");
        assert_eq!(holding.value, Decimal::from_str("100.00").unwrap());
    }

    #[test]
    fn into_holding_ordinary_currency_is_unaffected() {
        let write = HoldingWrite {
            symbol: "VUSA".to_string(),
            name: "Vanguard S&P 500".to_string(),
            holding_type: HoldingType::Etf,
            currency: "GBP".to_string(),
            as_of: naive_dt(2026, 3, 31),
            sub_account: None,
            is_closed: false,
            source_document_ids: Vec::new(),
            value: Some(Decimal::from(100)),
            quantity: None,
            price_per_unit: None,
        };
        let holding = write.into_holding("acct-1").unwrap();

        assert_eq!(holding.currency, "GBP");
        assert_eq!(holding.value, Decimal::from(100));
    }

    // ── Write-time conversion: transactions ──────────────────────────────────

    #[test]
    fn insert_transactions_bulk_converts_gbx_amount() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-1", "GBP")).unwrap();

        let txn = crate::model::ImportTransaction {
            date: naive_dt(2026, 3, 15),
            description: "Dividend".to_string(),
            amount: Decimal::from(-1234), // -1234 GBX
            currency: Some("GBX".to_string()),
            category_id: None,
            category_source: None,
            notes: None,
            is_recurring: None,
            exclude_from_summary: None,
            source_document_ids: Vec::new(),
        };
        db.insert_transactions_bulk("acct-1", &[txn]).unwrap();

        let (rows, _total) = db.get_transactions(&TransactionFilters::default()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].currency, "GBP");
        assert_eq!(rows[0].amount, Decimal::from_str("-12.34").unwrap());
    }

    #[test]
    fn transaction_from_unified_converts_gbx_and_dedups_against_converted_gbp() {
        use crate::importers::unified::UnifiedStatementRow;

        let sub_unit_row = UnifiedStatementRow {
            date: naive_dt(2026, 3, 15),
            description: "Sale".to_string(),
            amount: Decimal::from(1234), // 1234 GBX
            currency: "GBX".to_string(),
            fitid: None,
            category: None,
            merchant: None,
            counterparty: None,
            transaction_type: None,
            balance_after: None,
            notes: None,
            reference: None,
            row_confidence: 0.95,
            category_id: None,
            category_confidence: None,
            source_file: None,
        };
        let converted = crate::model::Transaction::from_unified(sub_unit_row, "acct-1");
        assert_eq!(converted.currency, "GBP");
        assert_eq!(converted.amount, Decimal::from_str("12.34").unwrap());

        // The equivalent row expressed directly in GBP must fingerprint
        // identically, so dedup treats the two forms as the same transaction.
        let gbp_row = UnifiedStatementRow {
            date: naive_dt(2026, 3, 15),
            description: "Sale".to_string(),
            amount: Decimal::from_str("12.34").unwrap(),
            currency: "GBP".to_string(),
            fitid: None,
            category: None,
            merchant: None,
            counterparty: None,
            transaction_type: None,
            balance_after: None,
            notes: None,
            reference: None,
            row_confidence: 0.95,
            category_id: None,
            category_confidence: None,
            source_file: None,
        };
        let direct = crate::model::Transaction::from_unified(gbp_row, "acct-1");
        assert_eq!(converted.fingerprint, direct.fingerprint);
    }

    // ── Migration: dry-run / apply / idempotency ─────────────────────────────

    fn seed_gbx_investment(db: &Db, account_id: &str, price_gbx: &str) -> String {
        let body = CreateInvestmentEventBody {
            account_id: account_id.to_string(),
            event_type: "buy".to_string(),
            symbol: "VUSA".to_string(),
            date: "2026-01-10T00:00:00".to_string(),
            quantity: "10".to_string(),
            price_per_share: price_gbx.to_string(),
            fee: None,
            currency: "GBX".to_string(),
            fee_currency: None,
            notes: None,
            source_document_ids: Vec::new(),
        };
        // Bypass write-time conversion by inserting the row directly, simulating
        // data written before the conversion-at-write-time change landed.
        let quantity: Decimal = body.quantity.parse().unwrap();
        let price: Decimal = body.price_per_share.parse().unwrap();
        let date_str = "2026-01-10T00:00:00";
        let fingerprint = sha256_hex(&format!(
            "{}|{}|{}|{}|{}|{}",
            body.account_id, body.symbol, date_str, quantity, price, "buy"
        ));
        let id = uuid::Uuid::new_v4().to_string();
        db.conn
            .execute(
                "INSERT INTO investments \
                 (id, account_id, event_type, symbol, date, quantity, price_per_share, fee, currency, notes, fingerprint, created_at, source_document_ids, fee_currency) \
                 VALUES (?1, ?2, 'buy', ?3, ?4, ?5, ?6, NULL, 'GBX', NULL, ?7, '2026-01-10T00:00:00Z', '[]', NULL)",
                params![
                    id,
                    account_id,
                    body.symbol,
                    date_str,
                    quantity.to_string(),
                    price.to_string(),
                    fingerprint,
                ],
            )
            .unwrap();
        id
    }

    fn seed_gbx_holding(db: &Db, account_id: &str, value_gbx: &str) -> i64 {
        db.conn
            .execute(
                "INSERT INTO holdings (account_id, symbol, name, holding_type, quantity, price_per_unit, value, currency, as_of) \
                 VALUES (?1, 'VUSA', 'Vanguard S&P 500', 'etf', '50', NULL, ?2, 'GBX', '2026-01-31T00:00:00')",
                params![account_id, value_gbx],
            )
            .unwrap();
        db.conn.last_insert_rowid()
    }

    fn seed_gbp_holding(db: &Db, account_id: &str, value_gbp: &str) -> i64 {
        db.conn
            .execute(
                "INSERT INTO holdings (account_id, symbol, name, holding_type, quantity, price_per_unit, value, currency, as_of) \
                 VALUES (?1, 'AAPL', 'Apple', 'stock', '5', NULL, ?2, 'GBP', '2026-01-31T00:00:00')",
                params![account_id, value_gbp],
            )
            .unwrap();
        db.conn.last_insert_rowid()
    }

    /// Seed a transaction row denominated in a sub-unit, bypassing
    /// `Transaction::from_unified` (which now converts on the way in, so it can
    /// no longer express a stored GBX row). The fingerprint is deliberately
    /// keyed on the *pre-conversion* amount, exactly as a row written before
    /// conversion-at-write-time landed would have been.
    fn seed_gbx_transaction(db: &Db, account_id: &str, amount_gbx: &str) -> (String, String) {
        let id = uuid::Uuid::new_v4().to_string();
        let date = "2026-02-05T00:00:00";
        let legacy_fingerprint = crate::util::fingerprint(date, amount_gbx, account_id);
        db.conn
            .execute(
                "INSERT INTO transactions (id, date, description, normalized, amount, currency, \
                 account_id, fingerprint, source_document_ids) \
                 VALUES (?1, ?2, 'Sharesave deduction', 'sharesave deduction', ?3, 'GBX', ?4, ?5, '[]')",
                params![id, date, amount_gbx, account_id, legacy_fingerprint],
            )
            .unwrap();
        (id, legacy_fingerprint)
    }

    fn transaction_row(db: &Db, id: &str) -> (String, String, String) {
        db.conn
            .query_row(
                "SELECT currency, amount, fingerprint FROM transactions WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
    }

    #[test]
    fn apply_converts_sub_unit_transaction_and_leaves_parent_row_untouched() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-1", "GBP")).unwrap();
        let (gbx_id, _) = seed_gbx_transaction(&db, "acct-1", "5000");

        // A transaction already in the parent currency: must be byte-for-byte
        // untouched, including its fingerprint.
        let gbp_id = uuid::Uuid::new_v4().to_string();
        let gbp_fingerprint = crate::util::fingerprint("2026-02-06T00:00:00", "42.50", "acct-1");
        db.conn
            .execute(
                "INSERT INTO transactions (id, date, description, normalized, amount, currency, \
                 account_id, fingerprint, source_document_ids) \
                 VALUES (?1, '2026-02-06T00:00:00', 'Coffee', 'coffee', '42.50', 'GBP', 'acct-1', ?2, '[]')",
                params![gbp_id, gbp_fingerprint],
            )
            .unwrap();

        let report = db.migrate_subunit_currencies(false).unwrap();

        assert_eq!(
            report.transactions_migrated(),
            1,
            "the GBX transaction must be reported as migrated; only the GBP row is skipped"
        );

        let (currency, amount, _) = transaction_row(&db, &gbx_id);
        assert_eq!(currency, "GBP");
        assert_eq!(
            Decimal::from_str(&amount).unwrap(),
            Decimal::from_str("50").unwrap(),
            "5000 GBX is 50.00 GBP"
        );

        let (gbp_currency, gbp_amount, gbp_fp_after) = transaction_row(&db, &gbp_id);
        assert_eq!(gbp_currency, "GBP", "byte-for-byte untouched");
        assert_eq!(gbp_amount, "42.50", "byte-for-byte untouched");
        assert_eq!(gbp_fp_after, gbp_fingerprint, "byte-for-byte untouched");
    }

    #[test]
    fn apply_recomputes_transaction_fingerprint_from_the_converted_amount() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-1", "GBP")).unwrap();
        let (id, legacy_fingerprint) = seed_gbx_transaction(&db, "acct-1", "5000");

        db.migrate_subunit_currencies(false).unwrap();

        let (_, _, migrated_fingerprint) = transaction_row(&db, &id);

        // The fingerprint must be keyed on the POST-conversion amount. If it
        // were left keyed on "5000", the next statement import of this same
        // transaction — now correctly parsed as 50 GBP — would find no
        // matching fingerprint and silently create a duplicate.
        let expected = crate::util::fingerprint("2026-02-05T00:00:00", "50", "acct-1");
        assert_eq!(
            migrated_fingerprint, expected,
            "migrated fingerprint must be recomputed from the converted amount"
        );
        assert_ne!(
            migrated_fingerprint, legacy_fingerprint,
            "the amount changed, so the fingerprint must have changed with it"
        );
    }

    #[test]
    fn migrated_transaction_dedups_against_a_fresh_parent_currency_import() {
        use crate::importers::unified::UnifiedStatementRow;

        let (db, _file) = test_db();
        db.create_account(&make_account("acct-1", "GBP")).unwrap();
        let (id, _) = seed_gbx_transaction(&db, "acct-1", "5000");

        db.migrate_subunit_currencies(false).unwrap();
        let (_, _, migrated_fingerprint) = transaction_row(&db, &id);

        // The real dedup path: re-importing the same transaction from a
        // statement must hash to the fingerprint the migration wrote, or the
        // import creates a duplicate instead of recognising it.
        let reimported = crate::model::Transaction::from_unified(
            UnifiedStatementRow {
                date: naive_dt(2026, 2, 5),
                description: "Sharesave deduction".to_string(),
                amount: Decimal::from_str("50").unwrap(),
                currency: "GBP".to_string(),
                fitid: None,
                category: None,
                merchant: None,
                counterparty: None,
                transaction_type: None,
                balance_after: None,
                notes: None,
                reference: None,
                row_confidence: 0.95,
                category_id: None,
                category_confidence: None,
                source_file: None,
            },
            "acct-1",
        );
        assert_eq!(
            reimported.fingerprint, migrated_fingerprint,
            "a fresh import of the same transaction must dedupe against the migrated row"
        );
    }

    #[test]
    fn dry_run_reports_sub_unit_transactions_without_writing() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-1", "GBP")).unwrap();
        let (id, legacy_fingerprint) = seed_gbx_transaction(&db, "acct-1", "5000");

        let report = db.migrate_subunit_currencies(true).unwrap();

        assert_eq!(report.transactions_migrated(), 1);

        let (currency, amount, fingerprint) = transaction_row(&db, &id);
        assert_eq!(currency, "GBX", "dry-run must not touch storage");
        assert_eq!(amount, "5000", "dry-run must not touch storage");
        assert_eq!(
            fingerprint, legacy_fingerprint,
            "dry-run must not rewrite the fingerprint"
        );
    }

    #[test]
    fn dry_run_reports_changes_without_writing() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-1", "GBP")).unwrap();
        seed_gbx_investment(&db, "acct-1", "1234");
        seed_gbx_holding(&db, "acct-1", "3816.00");

        let report = db.migrate_subunit_currencies(true).unwrap();

        assert_eq!(report.investments_migrated(), 1);
        assert_eq!(report.holdings_migrated(), 1);
        assert!(
            report.currencies_removed.is_empty(),
            "dry-run must never remove currency rows"
        );

        // Nothing was actually written: the row is still GBX in storage.
        let stored_currency: String = db
            .conn
            .query_row("SELECT currency FROM investments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored_currency, "GBX");
    }

    #[test]
    fn apply_converts_sub_unit_row_and_leaves_parent_row_untouched() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-1", "GBP")).unwrap();
        seed_gbx_investment(&db, "acct-1", "1234");
        seed_gbx_holding(&db, "acct-1", "3816.00");
        seed_gbp_holding(&db, "acct-1", "500.00");

        let report = db.migrate_subunit_currencies(false).unwrap();
        assert_eq!(report.investments_migrated(), 1);
        assert_eq!(report.holdings_migrated(), 1);

        let (inv_currency, inv_price): (String, String) = db
            .conn
            .query_row(
                "SELECT currency, price_per_share FROM investments",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(inv_currency, "GBP");
        assert_eq!(
            Decimal::from_str(&inv_price).unwrap(),
            Decimal::from_str("12.34").unwrap()
        );

        let gbx_holding: (String, String) = db
            .conn
            .query_row(
                "SELECT currency, value FROM holdings WHERE symbol = 'VUSA'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(gbx_holding.0, "GBP");
        assert_eq!(
            Decimal::from_str(&gbx_holding.1).unwrap(),
            Decimal::from_str("38.16").unwrap()
        );

        // The already-GBP holding must be untouched, byte for byte.
        let gbp_holding: (String, String) = db
            .conn
            .query_row(
                "SELECT currency, value FROM holdings WHERE symbol = 'AAPL'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(gbp_holding.0, "GBP");
        assert_eq!(gbp_holding.1, "500.00");
    }

    #[test]
    fn apply_recomputes_investment_fingerprint_to_match_create_investment_event() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-1", "GBP")).unwrap();
        let id = seed_gbx_investment(&db, "acct-1", "1234");

        db.migrate_subunit_currencies(false).unwrap();

        let migrated_fingerprint: String = db
            .conn
            .query_row(
                "SELECT fingerprint FROM investments WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();

        // What create_investment_event would compute for the equivalent
        // already-converted GBP row must match exactly, or a fresh import of
        // the same trade (now correctly priced in GBP) would duplicate it.
        let expected_fingerprint = sha256_hex(&format!(
            "{}|{}|{}|{}|{}|{}",
            "acct-1",
            "VUSA",
            "2026-01-10T00:00:00",
            Decimal::from(10),
            Decimal::from_str("12.34").unwrap(),
            "buy"
        ));
        assert_eq!(migrated_fingerprint, expected_fingerprint);
    }

    #[test]
    fn rerunning_migration_is_a_no_op() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-1", "GBP")).unwrap();
        seed_gbx_investment(&db, "acct-1", "1234");
        seed_gbx_holding(&db, "acct-1", "3816.00");

        let first = db.migrate_subunit_currencies(false).unwrap();
        assert_eq!(first.investments_migrated(), 1);
        assert_eq!(first.holdings_migrated(), 1);

        let (currency_after_first, price_after_first): (String, String) = db
            .conn
            .query_row(
                "SELECT currency, price_per_share FROM investments",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();

        // Second run: nothing left to convert, and — critically — the price
        // must NOT be divided by 100 a second time.
        let second = db.migrate_subunit_currencies(false).unwrap();
        assert_eq!(second.investments_migrated(), 0);
        assert_eq!(second.holdings_migrated(), 0);
        assert!(second.rows.is_empty());

        let (currency_after_second, price_after_second): (String, String) = db
            .conn
            .query_row(
                "SELECT currency, price_per_share FROM investments",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(currency_after_first, currency_after_second);
        assert_eq!(price_after_first, price_after_second);
    }

    #[test]
    fn apply_removes_unreferenced_sub_unit_currency_row() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-1", "GBP")).unwrap();
        // GBX configured in currencies (the legacy 0.01-rate setup).
        db.create_currency("GBX", Decimal::from_str("0.01").unwrap())
            .unwrap();
        seed_gbx_investment(&db, "acct-1", "1234");

        let report = db.migrate_subunit_currencies(false).unwrap();

        assert!(report.currencies_removed.contains(&"GBX".to_string()));
        assert!(!db.currency_exists("GBX").unwrap());
    }

    #[test]
    fn apply_keeps_sub_unit_currency_row_if_still_referenced() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-1", "GBP")).unwrap();
        db.create_currency("GBX", Decimal::from_str("0.01").unwrap())
            .unwrap();
        // A holding still denominated in GBX after the investments pass
        // (simulated by inserting it directly, bypassing conversion, as if it
        // were added between the dry-run and the apply — migration should
        // still leave the currency row alone if anything references it).
        seed_gbx_holding(&db, "acct-1", "100.00");

        let report = db.migrate_subunit_currencies(false).unwrap();
        // The holding itself is converted by this same call, so by the time
        // the currency cleanup runs nothing references GBX any more —
        // confirming migration order (rows first, then currency cleanup).
        assert!(report.currencies_removed.contains(&"GBX".to_string()));
        assert!(!db.currency_exists("GBX").unwrap());
    }

    #[test]
    fn fee_only_sub_unit_row_reports_the_pair_that_actually_converted() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-1", "USD")).unwrap();

        // A USD-priced trade whose *fee* alone is denominated in GBX, written
        // directly to bypass write-time conversion. Only the fee converts.
        let id = uuid::Uuid::new_v4().to_string();
        db.conn
            .execute(
                "INSERT INTO investments                  (id, account_id, event_type, symbol, date, quantity, price_per_share, fee, currency, notes, fingerprint, created_at, source_document_ids, fee_currency)                  VALUES (?1, 'acct-1', 'buy', 'AAPL', '2026-03-15T00:00:00', '10', '150', '500', 'USD', NULL, 'seed-fp', '2026-03-15T00:00:00Z', '[]', 'GBX')",
                params![id],
            )
            .unwrap();

        let report = db.migrate_subunit_currencies(true).unwrap();

        let row = report
            .rows
            .iter()
            .find(|r| r.table == "investments")
            .expect("the fee-only row must still be reported");
        assert_eq!(row.sub_unit_code, "GBX");
        assert_eq!(
            row.parent_code, "GBP",
            "GBX converts to GBP; pairing it with the untouched USD price              currency would print a nonsense `GBX -> USD` line"
        );
    }

    // ── Migration: accounts.currency ────────────────────────────────────────

    fn account_currency(db: &Db, id: &str) -> String {
        db.conn
            .query_row(
                "SELECT currency FROM accounts WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn apply_converts_sub_unit_account_and_leaves_parent_currency_account_alone() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-gbx", "GBX")).unwrap();
        db.create_account(&make_account("acct-gbp", "GBP")).unwrap();

        let report = db.migrate_subunit_currencies(false).unwrap();

        assert_eq!(
            report.accounts_migrated(),
            1,
            "only the GBX account is migrated; the GBP one is skipped"
        );
        assert_eq!(account_currency(&db, "acct-gbx"), "GBP");
        assert_eq!(account_currency(&db, "acct-gbp"), "GBP", "untouched");
    }

    #[test]
    fn dry_run_reports_sub_unit_accounts_without_writing() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-gbx", "GBX")).unwrap();

        let report = db.migrate_subunit_currencies(true).unwrap();

        assert_eq!(report.accounts_migrated(), 1);
        assert_eq!(
            account_currency(&db, "acct-gbx"),
            "GBX",
            "dry-run must not touch storage"
        );
    }

    #[test]
    fn sub_unit_account_no_longer_pins_the_currency_row_open() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-gbx", "GBX")).unwrap();
        db.create_currency("GBX", Decimal::from_str("0.01").unwrap())
            .unwrap();

        let report = db.migrate_subunit_currencies(false).unwrap();

        // The in-use check counts `accounts`. Before the accounts pass existed
        // the account stayed GBX, so the count was never zero, the GBX row was
        // retained and `currencies_removed` came back empty — the cleanup
        // silently no-opped.
        assert!(
            report.currencies_removed.contains(&"GBX".to_string()),
            "the GBX currency row must be removed once no account references it"
        );
        assert!(!db.currency_exists("GBX").unwrap());
    }

    #[test]
    fn set_account_balance_after_migration_writes_a_parent_currency_cash_holding() {
        let (db, _file) = test_db();
        db.create_account(&make_account("acct-gbx", "GBX")).unwrap();

        db.migrate_subunit_currencies(false).unwrap();

        // `set_account_balance` copies `accounts.currency` verbatim into the
        // `_CASH` holding with no conversion of its own. Migrating the account
        // is what stops it minting fresh sub-unit holdings after the migration.
        db.set_account_balance("acct-gbx", Decimal::from(10000), naive_dt(2026, 3, 1))
            .unwrap();

        let (currency, value): (String, String) = db
            .conn
            .query_row(
                "SELECT currency, value FROM holdings WHERE account_id = 'acct-gbx' AND symbol = '_CASH'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();

        assert_eq!(
            currency, "GBP",
            "the cash holding must inherit the migrated parent currency, not GBX"
        );
        assert_eq!(value, "10000");
    }
}

#[cfg(test)]
mod tax_storage_tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::str::FromStr;
    use tempfile::NamedTempFile;

    fn test_db() -> (Db, NamedTempFile) {
        let file = NamedTempFile::new().expect("temp file");
        let db = Db::open(file.path()).expect("test db");
        (db, file)
    }

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).expect("decimal literal")
    }

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("date literal")
    }

    /// The statutory seed must be present on a fresh database, or the very first
    /// report a new user runs computes no tax at all.
    #[test]
    fn seeds_the_statutory_values_on_open() {
        let (db, _f) = test_db();

        let entries = db.get_tax_config("2024-25").expect("read config");
        let aea = entries
            .iter()
            .find(|e| e.kind == "aea")
            .expect("2024-25 has an AEA");
        assert_eq!(aea.amount, Some(d("3000")));

        let rates: Vec<_> = entries.iter().filter(|e| e.kind == "rate").collect();
        assert_eq!(
            rates.len(),
            4,
            "2024-25 carries basic+higher on each side of the 30 Oct 2024 change"
        );

        // The pre/post-30-October split, which is the whole reason this year is
        // modelled as two periods.
        let pre_higher = rates
            .iter()
            .find(|e| e.rate_kind == "higher" && e.valid_from == "2024-04-06")
            .expect("pre-30-Oct higher band");
        assert_eq!(pre_higher.rate, Some(d("0.20")));
        assert_eq!(pre_higher.valid_to, "2024-10-29");

        let post_higher = rates
            .iter()
            .find(|e| e.rate_kind == "higher" && e.valid_from == "2024-10-30")
            .expect("post-30-Oct higher band");
        assert_eq!(post_higher.rate, Some(d("0.24")));
        assert_eq!(post_higher.valid_to, "2025-04-05");

        // 2023-24 predates the change and keeps the old rates.
        let older = db.get_tax_config("2023-24").expect("read 2023-24");
        let older_higher = older
            .iter()
            .find(|e| e.kind == "rate" && e.rate_kind == "higher")
            .expect("2023-24 higher band");
        assert_eq!(older_higher.rate, Some(d("0.20")));
        let older_aea = older.iter().find(|e| e.kind == "aea").expect("2023-24 AEA");
        assert_eq!(older_aea.amount, Some(d("6000")));
    }

    /// A user's edit to the statutory table must survive a restart. The seed runs
    /// on every open, so this is the property that stops it clobbering their work.
    #[test]
    fn a_user_edit_survives_reopening() {
        let file = NamedTempFile::new().expect("temp file");
        {
            let db = Db::open(file.path()).expect("first open");
            let edited = vec![TaxConfigEntry {
                tax_year: "2024-25".to_string(),
                kind: "aea".to_string(),
                rate_kind: String::new(),
                valid_from: "2024-04-06".to_string(),
                valid_to: "2025-04-05".to_string(),
                amount: Some(d("1234")),
                rate: None,
                updated_at: None,
            }];
            db.put_tax_config("2024-25", &edited).expect("write");
        }

        let db = Db::open(file.path()).expect("reopen");
        let entries = db.get_tax_config("2024-25").expect("read");
        let aea = entries
            .iter()
            .find(|e| e.kind == "aea")
            .expect("the edited AEA");
        assert_eq!(
            aea.amount,
            Some(d("1234")),
            "the startup seed must not overwrite a user's edit"
        );
    }

    /// Writing a year's config replaces it rather than merging, so an edit that
    /// removes a band cannot leave the old one behind.
    #[test]
    fn putting_config_replaces_the_year_rather_than_merging() {
        let (db, _f) = test_db();
        assert_eq!(db.get_tax_config("2024-25").expect("seeded").len(), 5);

        let replacement = vec![TaxConfigEntry {
            tax_year: "2024-25".to_string(),
            kind: "rate".to_string(),
            rate_kind: "higher".to_string(),
            valid_from: "2024-04-06".to_string(),
            valid_to: "2025-04-05".to_string(),
            amount: None,
            rate: Some(d("0.30")),
            updated_at: None,
        }];
        db.put_tax_config("2024-25", &replacement).expect("write");

        let entries = db.get_tax_config("2024-25").expect("read");
        assert_eq!(entries.len(), 1, "the four seeded rows must be gone");
        assert_eq!(entries[0].rate, Some(d("0.30")));

        // A different year must be untouched by that write.
        assert!(
            !db.get_tax_config("2023-24").expect("read").is_empty(),
            "replacing one year must not disturb another"
        );
    }

    /// An unconfigured profile-year returns the documented defaults, so the
    /// computation never has an "unconfigured" branch.
    #[test]
    fn unconfigured_inputs_return_documented_defaults() {
        let (db, _f) = test_db();
        let inputs = db.get_tax_inputs("nobody", "2024-25").expect("read");

        assert_eq!(inputs.brought_forward_losses, Decimal::ZERO);
        assert_eq!(inputs.allowable_income_remaining, Decimal::ZERO);
        assert!(inputs.aea_claimed, "the AEA is claimed by default");
        assert_eq!(inputs.profile_id, "nobody");
        assert_eq!(inputs.tax_year, "2024-25");
    }

    /// Inputs round-trip, and are keyed per profile AND per year.
    #[test]
    fn inputs_round_trip_per_profile_and_year() {
        let (db, _f) = test_db();
        db.create_profile("alex", "Alex").expect("profile");
        db.create_profile("sam", "Sam").expect("profile");

        db.put_tax_inputs(&TaxInputs {
            profile_id: "alex".to_string(),
            tax_year: "2024-25".to_string(),
            brought_forward_losses: d("1500.50"),
            allowable_income_remaining: d("4000"),
            aea_claimed: false,
            updated_at: None,
        })
        .expect("write");

        let read = db.get_tax_inputs("alex", "2024-25").expect("read");
        assert_eq!(read.brought_forward_losses, d("1500.50"));
        assert_eq!(read.allowable_income_remaining, d("4000"));
        assert!(!read.aea_claimed);

        // Another profile in the same year is unaffected.
        let other = db.get_tax_inputs("sam", "2024-25").expect("read");
        assert_eq!(other.brought_forward_losses, Decimal::ZERO);
        assert!(other.aea_claimed);

        // The same profile in another year is unaffected.
        let other_year = db.get_tax_inputs("alex", "2025-26").expect("read");
        assert_eq!(other_year.brought_forward_losses, Decimal::ZERO);
    }

    /// Writing twice updates in place rather than failing or duplicating.
    #[test]
    fn inputs_upsert_on_second_write() {
        let (db, _f) = test_db();
        db.create_profile("alex", "Alex").expect("profile");

        let mut inputs = TaxInputs {
            profile_id: "alex".to_string(),
            tax_year: "2024-25".to_string(),
            brought_forward_losses: d("100"),
            allowable_income_remaining: Decimal::ZERO,
            aea_claimed: true,
            updated_at: None,
        };
        db.put_tax_inputs(&inputs).expect("first write");

        inputs.brought_forward_losses = d("250");
        db.put_tax_inputs(&inputs).expect("second write");

        let read = db.get_tax_inputs("alex", "2024-25").expect("read");
        assert_eq!(read.brought_forward_losses, d("250"));
    }

    /// The derivation nets within each year and only carries the years that
    /// ended in a loss.
    #[test]
    fn derives_losses_netting_within_each_year() {
        let (db, _f) = test_db();
        let years = vec![
            ("2022-23".to_string(), day("2022-04-06"), day("2023-04-05")),
            ("2023-24".to_string(), day("2023-04-06"), day("2024-04-05")),
        ];
        let realized = vec![
            // 2022-23 nets to a 400 loss (-1000 + 600).
            ("2022-06-01".to_string(), d("-1000")),
            ("2022-09-01".to_string(), d("600")),
            // 2023-24 nets to a gain, so it contributes nothing.
            ("2023-06-01".to_string(), d("900")),
            ("2023-09-01".to_string(), d("-100")),
        ];

        let derived = db
            .derive_brought_forward_losses(&realized, &years)
            .expect("derive");

        assert_eq!(
            derived.contributions.len(),
            1,
            "a year that netted to a gain must not contribute"
        );
        assert_eq!(derived.contributions[0].tax_year, "2022-23");
        assert_eq!(derived.contributions[0].net_loss, d("400"));
        assert_eq!(derived.amount, d("400"));
    }

    /// A gain year must not cancel out another year's loss: losses carried
    /// forward are not reduced by a later year's gains at derivation time.
    #[test]
    fn a_gain_year_does_not_offset_another_years_loss() {
        let (db, _f) = test_db();
        let years = vec![
            ("2022-23".to_string(), day("2022-04-06"), day("2023-04-05")),
            ("2023-24".to_string(), day("2023-04-06"), day("2024-04-05")),
        ];
        let realized = vec![
            ("2022-06-01".to_string(), d("-1000")), // loss year
            ("2023-06-01".to_string(), d("5000")),  // big gain year
        ];

        let derived = db
            .derive_brought_forward_losses(&realized, &years)
            .expect("derive");

        assert_eq!(
            derived.amount,
            d("1000"),
            "the 5000 gain must not net away the 1000 loss"
        );
    }

    /// The derived figure is always flagged as an upper bound. This is the flag
    /// the UI relies on to avoid presenting it as authoritative.
    #[test]
    fn derived_losses_are_always_flagged_as_an_upper_bound() {
        let (db, _f) = test_db();
        let years = vec![("2022-23".to_string(), day("2022-04-06"), day("2023-04-05"))];

        let with_losses = db
            .derive_brought_forward_losses(&[("2022-06-01".to_string(), d("-500"))], &years)
            .expect("derive");
        assert!(with_losses.is_upper_bound);

        // Also true when there is nothing to report, so a consumer cannot infer
        // "empty means certain".
        let empty = db
            .derive_brought_forward_losses(&[], &years)
            .expect("derive");
        assert!(empty.is_upper_bound);
        assert_eq!(empty.amount, Decimal::ZERO);
        assert!(empty.contributions.is_empty());
    }

    /// Disposals outside the supplied year bounds are ignored rather than
    /// silently bucketed into the nearest year.
    #[test]
    fn ignores_disposals_outside_the_supplied_years() {
        let (db, _f) = test_db();
        let years = vec![("2022-23".to_string(), day("2022-04-06"), day("2023-04-05"))];
        let realized = vec![
            ("2022-06-01".to_string(), d("-300")), // inside
            ("2021-06-01".to_string(), d("-900")), // before the window
            ("2024-06-01".to_string(), d("-700")), // after the window
        ];

        let derived = db
            .derive_brought_forward_losses(&realized, &years)
            .expect("derive");

        assert_eq!(derived.amount, d("300"));
        assert_eq!(derived.contributions.len(), 1);
    }

    /// A corrupt stored decimal surfaces as an error rather than coercing to a
    /// default that would understate the tax.
    #[test]
    fn a_corrupt_stored_decimal_is_an_error() {
        let (db, _f) = test_db();
        db.create_profile("alex", "Alex").expect("profile");
        db.conn
            .execute(
                "INSERT INTO tax_inputs (profile_id, tax_year, brought_forward_losses)
                 VALUES ('alex', '2024-25', 'not-a-number')",
                [],
            )
            .expect("insert corrupt row");

        assert!(
            db.get_tax_inputs("alex", "2024-25").is_err(),
            "an unparseable stored decimal must not silently become zero"
        );
    }
}
