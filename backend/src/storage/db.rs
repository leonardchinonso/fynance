//! SQLite-backed persistence layer.
//!
//! The `Db` type owns a single `rusqlite::Connection` and exposes typed
//! methods for every query the rest of the crate needs. Phase 1 is
//! synchronous and single-threaded; the Axum server wraps this behind a
//! shared `Arc<Mutex<Db>>` without changing the surface area here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::{NaiveDate, NaiveDateTime};
use rusqlite::{Connection, OptionalExtension, params};
use rust_decimal::Decimal;

use crate::model::{
    Account, AccountHoldingHistoryRow, AccountHoldingSeries, AccountHoldingValue, AccountSnapshot,
    AccountType, AssetClass, BalanceDelta, BudgetRow, Category, CategoryNode, CategorySource,
    CategoryTotal, CategoryType, ChecklistItem, ChecklistStatus, CreateCategoryPayload,
    CreateInvestmentEventBody, Currency, Document, DocumentReferences, DocumentSummary,
    Granularity, Holding, HoldingPreview, HoldingSummaryRow, HoldingType, HoldingsCashFlowMonth,
    HoldingsHistoryRow, ImportLog, ImportResult, ImportRowError, ImportTransaction, InsertOutcome,
    InvestmentEvent, InvestmentEventType, InvestmentHistoryRow, InvestmentMetrics,
    PatchCategoryPayload, PatchInvestmentEventBody, Profile, SpendingGridRow, SpendingGroupBy,
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

        let total = holding_count + account_count + transaction_count;
        if total > 0 {
            anyhow::bail!(
                "cannot delete currency '{code}': in use by {holding_count} holdings, {account_count} accounts, {transaction_count} transactions"
            );
        }

        self.conn
            .execute("DELETE FROM currencies WHERE code = ?1", params![code])?;
        Ok(())
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
            .prepare("SELECT id, name FROM profiles ORDER BY name")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Profile {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
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
    fn balances_from_holdings_as_of(
        &self,
        as_of: NaiveDate,
    ) -> Result<std::collections::HashMap<String, (Decimal, NaiveDateTime)>> {
        use std::collections::HashMap;
        let as_of_str = as_of.format("%Y-%m-%dT23:59:59").to_string();
        let mut stmt = self.conn.prepare(
            r"SELECT h.account_id, h.value, h.as_of
              FROM holdings h
              WHERE h.as_of = (
                  SELECT MAX(h2.as_of) FROM holdings h2
                  WHERE h2.account_id = h.account_id
                    AND h2.symbol = h.symbol
                    AND COALESCE(h2.sub_account, '') = COALESCE(h.sub_account, '')
                    AND h2.as_of <= ?1
              )",
        )?;
        let rows = stmt.query_map(params![as_of_str], |row| {
            let account_id: String = row.get(0)?;
            let value_str: String = row.get(1)?;
            let as_of_str: String = row.get(2)?;
            Ok((account_id, value_str, as_of_str))
        })?;

        let mut agg: HashMap<String, (Decimal, NaiveDateTime)> = HashMap::new();
        for r in rows {
            let (account_id, value_str, as_of_str) = r?;
            let value: Decimal = value_str.parse().unwrap_or(Decimal::ZERO);
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

    pub fn count_transactions_for_account(&self, id: &str) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM transactions WHERE account_id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn count_holdings_for_account(&self, id: &str) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM holdings WHERE account_id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        Ok(count)
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
    /// transactions or holdings first (the DELETE route guard does this) so no
    /// rows are orphaned.
    pub fn hard_delete_account(&self, id: &str) -> Result<()> {
        let deleted = self
            .conn
            .execute("DELETE FROM accounts WHERE id = ?1", params![id])?;
        if deleted == 0 {
            return Err(anyhow!("account {id} not found"));
        }
        Ok(())
    }

    // ── Investments ───────────────────────────────────────────────────────────

    pub fn create_investment_event(
        &self,
        body: &CreateInvestmentEventBody,
    ) -> Result<InvestmentEvent> {
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

        self.conn.execute(
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
                body.currency,
                body.notes,
                fingerprint,
                now,
                source_ids_json,
                fee_currency,
            ],
        )?;

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
        Ok(event)
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

    pub fn update_investment_event(
        &self,
        id: &str,
        body: &PatchInvestmentEventBody,
    ) -> Result<Option<InvestmentEvent>> {
        let exists: bool = self.conn.query_row(
            "SELECT COUNT(*) > 0 FROM investments WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        if !exists {
            return Ok(None);
        }

        if let Some(ref et) = body.event_type {
            InvestmentEventType::parse(et)
                .ok_or_else(|| anyhow::anyhow!("invalid event_type: {}", et))?;
            self.conn.execute(
                "UPDATE investments SET event_type = ?1 WHERE id = ?2",
                params![et, id],
            )?;
        }
        if let Some(ref s) = body.symbol {
            self.conn.execute(
                "UPDATE investments SET symbol = ?1 WHERE id = ?2",
                params![s, id],
            )?;
        }
        if let Some(ref d) = body.date {
            let dt = parse_transaction_datetime(d)
                .ok_or_else(|| anyhow::anyhow!("invalid date format"))?;
            self.conn.execute(
                "UPDATE investments SET date = ?1 WHERE id = ?2",
                params![dt.format("%Y-%m-%dT%H:%M:%S").to_string(), id],
            )?;
        }
        if let Some(ref q) = body.quantity {
            q.parse::<Decimal>()
                .map_err(|_| anyhow::anyhow!("invalid quantity"))?;
            self.conn.execute(
                "UPDATE investments SET quantity = ?1 WHERE id = ?2",
                params![q, id],
            )?;
        }
        if let Some(ref p) = body.price_per_share {
            p.parse::<Decimal>()
                .map_err(|_| anyhow::anyhow!("invalid price_per_share"))?;
            self.conn.execute(
                "UPDATE investments SET price_per_share = ?1 WHERE id = ?2",
                params![p, id],
            )?;
        }
        if body.fee.is_some() {
            let fee_str = body.fee.as_deref();
            if let Some(f) = fee_str {
                f.parse::<Decimal>()
                    .map_err(|_| anyhow::anyhow!("invalid fee"))?;
            }
            self.conn.execute(
                "UPDATE investments SET fee = ?1 WHERE id = ?2",
                params![fee_str, id],
            )?;
        }
        if let Some(ref c) = body.currency {
            self.conn.execute(
                "UPDATE investments SET currency = ?1 WHERE id = ?2",
                params![c, id],
            )?;
        }
        if let Some(ref fc) = body.fee_currency {
            self.conn.execute(
                "UPDATE investments SET fee_currency = ?1 WHERE id = ?2",
                params![fc, id],
            )?;
        }
        if body.notes.is_some() {
            self.conn.execute(
                "UPDATE investments SET notes = ?1 WHERE id = ?2",
                params![body.notes.as_deref(), id],
            )?;
        }

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
    /// `ImportResult` with per-row error details for any skipped rows.
    pub fn insert_transactions_bulk(
        &self,
        account_id: &str,
        txns: &[ImportTransaction],
    ) -> Result<ImportResult> {
        use crate::util::{fingerprint, normalize_description};
        use uuid::Uuid;

        let mut result = ImportResult {
            filename: String::new(),
            account_id: account_id.to_string(),
            ..ImportResult::default()
        };

        for (i, t) in txns.iter().enumerate() {
            result.rows_total += 1;
            let date_iso = t.date.format("%Y-%m-%dT%H:%M:%S").to_string();
            let amount_str = t.amount.to_string();
            let currency = t.currency.clone().unwrap_or_else(|| "GBP".to_string());
            let normalized = normalize_description(&t.description);
            let fp = fingerprint(&date_iso, &amount_str, account_id);

            // Validate the category_id (must be an active leaf) when provided.
            let category_id = if let Some(ref cid) = t.category_id {
                match self.get_category_by_id(cid)? {
                    Some(cat) if cat.parent_id.is_some() && cat.is_active => Some(cid.clone()),
                    Some(cat) if cat.parent_id.is_none() => {
                        result.errors.push(ImportRowError {
                            index: i,
                            reason: format!("category {cid} is a parent, not a leaf"),
                        });
                        continue;
                    }
                    _ => {
                        result.errors.push(ImportRowError {
                            index: i,
                            reason: format!("category {cid} not found or inactive"),
                        });
                        continue;
                    }
                }
            } else {
                None
            };

            let tx = Transaction {
                id: Uuid::new_v4().to_string(),
                date: t.date,
                description: t.description.clone(),
                normalized,
                amount: t.amount,
                currency,
                account_id: account_id.to_string(),
                category_id,
                category_source: t.category_source.clone(),
                confidence: None,
                notes: t.notes.clone(),
                is_recurring: t.is_recurring.unwrap_or(false),
                exclude_from_summary: t.exclude_from_summary.unwrap_or(false),
                fingerprint: fp,
                fitid: None,
                source_document_ids: t.source_document_ids.clone(),
            };

            match self.insert_transaction(&tx) {
                Ok(InsertOutcome::Inserted) => result.rows_inserted += 1,
                Ok(InsertOutcome::Duplicate) => result.rows_duplicate += 1,
                Err(e) => {
                    result.errors.push(ImportRowError {
                        index: i,
                        reason: e.to_string(),
                    });
                }
            }
        }
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
            let amount_str = t.amount.to_string();
            let currency = t.currency.clone().unwrap_or_else(|| "GBP".to_string());
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
                amount: t.amount,
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
            let amount_str = row.amount.to_string();
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
                amount: row.amount,
                currency: row.currency.clone(),
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
            let pattern = format!("%{}%", search.replace('%', "\\%").replace('_', "\\_"));
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

        let mut conditions: Vec<String> = vec![
            "t.category_id IS NOT NULL".to_string(),
            "t.exclude_from_summary = 0".to_string(),
        ];
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
        if let Some(cats) = &filters.categories {
            if !cats.is_empty() {
                let placeholders: Vec<String> = cats
                    .iter()
                    .map(|v| {
                        args.push(Box::new(v.clone()));
                        format!("?{}", args.len())
                    })
                    .collect();
                let ph = placeholders.join(",");
                conditions.push(format!("t.category_id IN ({ph})"));
            }
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
            let total = Decimal::try_from(total_f64).unwrap_or_default();
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
        let child_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM categories WHERE parent_id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        if child_count > 0 {
            return Err(anyhow!(
                "category {id} has {child_count} child categories; reparent or delete them first"
            ));
        }

        self.conn.execute(
            "UPDATE transactions SET category_id = NULL WHERE category_id = ?1",
            params![id],
        )?;
        self.conn
            .execute("DELETE FROM budgets WHERE category_id = ?1", params![id])?;
        self.conn.execute(
            "DELETE FROM standing_budgets WHERE category_id = ?1",
            params![id],
        )?;
        self.conn.execute(
            "DELETE FROM budget_overrides WHERE category_id = ?1",
            params![id],
        )?;

        let deleted = self
            .conn
            .execute("DELETE FROM categories WHERE id = ?1", params![id])?;
        if deleted == 0 {
            return Err(anyhow!("category {id} not found"));
        }
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
            .query_map(params![month, month, month], |row| {
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
            let total_dec = Decimal::try_from(total_f64).unwrap_or_default();
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

    // ── Legacy budget (CLI compat) ─────────────────────────────────────────────

    pub fn set_budget(&self, month: &str, category: &str, amount: Decimal) -> Result<()> {
        self.conn.execute(
            r"INSERT INTO budgets (month, category, amount)
              VALUES (?1, ?2, ?3)
              ON CONFLICT(month, category) DO UPDATE SET amount = excluded.amount",
            params![month, category, amount.to_string()],
        )?;
        Ok(())
    }

    pub fn get_budgets_for_month(&self, month: &str) -> Result<Vec<crate::model::Budget>> {
        let mut stmt = self.conn.prepare(
            "SELECT month, category, amount FROM budgets WHERE month = ?1 ORDER BY category",
        )?;
        let rows = stmt
            .query_map(params![month], |row| {
                let amount: String = row.get(2)?;
                Ok(crate::model::Budget {
                    month: row.get(0)?,
                    category: row.get(1)?,
                    amount: amount.parse::<Decimal>().unwrap_or_default(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
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

    /// List every stored document with its orphan flag. The exact reference
    /// count is intentionally NOT computed here: the three correlated `COUNT`s
    /// over `json_each` are slow over the whole dataset and block the list.
    /// `reference_count` is left `None`; clients fetch it lazily per row via
    /// `get_document`. Orphaned is still computed, but cheaply: `NOT EXISTS`
    /// short-circuits on the first referencing row instead of counting them all.
    pub fn list_documents(&self) -> Result<Vec<DocumentSummary>> {
        // Collect every referenced document id in a single pass per table. The
        // previous per-document correlated `NOT EXISTS` was O(docs x rows) and
        // re-scanned/re-parsed every source_document_ids JSON array once per
        // document; this scans each table's json arrays exactly once.
        let referenced: std::collections::HashSet<String> = {
            let mut stmt = self.conn.prepare(
                r"SELECT j.value FROM transactions t, json_each(t.source_document_ids) j
                  UNION
                  SELECT j.value FROM holdings h, json_each(h.source_document_ids) j
                  UNION
                  SELECT j.value FROM investments i, json_each(i.source_document_ids) j",
            )?;
            let ids = stmt.query_map([], |row| row.get::<_, String>(0))?;
            ids.collect::<rusqlite::Result<std::collections::HashSet<String>>>()?
        };

        let mut stmt = self.conn.prepare(
            r"SELECT d.id, d.filename, d.mime_type, d.size_bytes, d.origin, d.account_id, d.uploaded_at
              FROM documents d
              ORDER BY d.uploaded_at DESC, d.filename",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let orphaned = !referenced.contains(&id);
                Ok(DocumentSummary {
                    id,
                    filename: row.get(1)?,
                    mime_type: row.get(2)?,
                    size_bytes: row.get::<_, i64>(3)? as usize,
                    origin: row.get(4)?,
                    account_id: row.get(5)?,
                    uploaded_at: row.get(6)?,
                    reference_count: None,
                    orphaned,
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
                self.conn.execute(
                    "UPDATE api_tokens SET last_used = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE name = ?1",
                    params![name],
                )?;
                Ok(Some(name))
            }
            _ => Ok(None),
        }
    }

    // ── Portfolio queries ─────────────────────────────────────────────────────

    /// One `HoldingsHistoryRow` per period in `[from, to]`. The last point
    /// reconciles with the portfolio summary's net worth for the same
    /// `as_of` (both reduce `get_holdings_for_summary`).
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
        let mut rows = Vec::new();

        for (label, period_end) in periods {
            let holdings = self.get_holdings_for_summary(period_end, profile_id)?;
            let mut available_agg: CurrencyAggregator = Default::default();
            let mut unavailable_agg: CurrencyAggregator = Default::default();

            for row in holdings {
                let h = &row.holding;
                if is_available_account(&row.account_type) {
                    available_agg.add(h.value, &h.currency, fx);
                } else {
                    unavailable_agg.add(h.value, &h.currency, fx);
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
            .filter_map(
                |(date, event_type, symbol, q, p, fee, currency, fee_currency)| {
                    let q: Decimal = q.parse().ok()?;
                    let p: Decimal = p.parse().ok()?;
                    let principal = fx.convert(q * p, &currency);
                    let fee = fee
                        .and_then(|f| f.parse::<Decimal>().ok())
                        .map(|f| fx.convert(f, fee_currency.as_deref().unwrap_or(&currency)))
                        .unwrap_or(Decimal::ZERO);
                    Some((date, event_type, symbol, q, principal, fee))
                },
            )
            .collect();

        let periods = generate_period_end_dates(from, to, granularity);
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

            // Market value of active (unclosed) investment + ISA holdings.
            let mut mv = CurrencyAggregator::default();
            let mut has_active = false;
            for r in self.get_holdings_for_summary(period_end, profile_id)? {
                if matches!(
                    r.account_type,
                    AccountType::Investment | AccountType::InvestmentIsa
                ) && !r.holding.is_closed
                    && (account_ids.is_empty() || account_ids.contains(&r.holding.account_id))
                {
                    mv.add(r.holding.value, &r.holding.currency, fx);
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
                let value = value_str.parse::<Decimal>().unwrap_or_default();
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

    /// Returns the first and last balance (SUM of holdings) per account within `[start, end]`,
    /// and the delta between them.
    pub fn get_balance_summary(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<BalanceDelta>> {
        let start_str = start.format("%Y-%m-%dT00:00:00").to_string();
        let end_str = end.format("%Y-%m-%dT23:59:59").to_string();

        let account_ids: Vec<String> = {
            let mut stmt = self.conn.prepare(
                "SELECT DISTINCT account_id FROM holdings WHERE as_of >= ?1 AND as_of <= ?2",
            )?;
            stmt.query_map(rusqlite::params![start_str, end_str], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };

        let mut result = Vec::new();
        for account_id in account_ids {
            let first_date: Option<String> = self
                .conn
                .query_row(
                    r"SELECT MIN(as_of) FROM holdings
                      WHERE account_id = ?1 AND as_of >= ?2",
                    rusqlite::params![account_id, start_str],
                    |row| row.get(0),
                )
                .ok()
                .flatten();

            let last_date: Option<String> = self
                .conn
                .query_row(
                    r"SELECT MAX(as_of) FROM holdings
                      WHERE account_id = ?1 AND as_of <= ?2",
                    rusqlite::params![account_id, end_str],
                    |row| row.get(0),
                )
                .ok()
                .flatten();

            let start_balance: Option<Decimal> = first_date.as_ref().and_then(|d| {
                self.conn
                    .query_row(
                        r"SELECT SUM(CAST(value AS REAL)) FROM holdings
                          WHERE account_id = ?1 AND as_of = ?2",
                        rusqlite::params![account_id, d],
                        |row| row.get::<_, Option<f64>>(0),
                    )
                    .ok()
                    .flatten()
                    .and_then(|f| Decimal::try_from(f).ok())
            });

            let end_balance: Option<Decimal> = last_date.as_ref().and_then(|d| {
                self.conn
                    .query_row(
                        r"SELECT SUM(CAST(value AS REAL)) FROM holdings
                          WHERE account_id = ?1 AND as_of = ?2",
                        rusqlite::params![account_id, d],
                        |row| row.get::<_, Option<f64>>(0),
                    )
                    .ok()
                    .flatten()
                    .and_then(|f| Decimal::try_from(f).ok())
            });

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
            let income = Decimal::try_from(income_f).unwrap_or_default();
            let spending = Decimal::try_from(spending_f).unwrap_or_default();
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
    /// `get_monthly_net_worth` all reduce this, so they reconcile.
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
            r"SELECT h.account_id, h.symbol, h.name, h.holding_type,
                     h.quantity, h.price_per_unit, h.value, h.currency,
                     h.as_of, h.short_name, h.sub_account, h.is_closed,
                     a.type AS account_type, a.institution, a.profile_ids,
                     h.source_document_ids
              FROM holdings h
              JOIN accounts a ON a.id = h.account_id
              WHERE a.is_active = 1
                {profile_filter}
                AND h.as_of = (
                    SELECT MAX(h2.as_of) FROM holdings h2
                    WHERE h2.account_id = h.account_id
                      AND h2.symbol = h.symbol
                      AND COALESCE(h2.sub_account, '') = COALESCE(h.sub_account, '')
                      AND h2.as_of <= ?1
                )
              ORDER BY h.account_id, h.symbol"
        );

        let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<HoldingSummaryRow> {
            let account_type_str: String = row.get(12)?;
            let institution: String = row.get(13)?;
            let holding = row_to_holding(row)?;
            Ok(HoldingSummaryRow {
                holding,
                account_type: AccountType::parse(&account_type_str)
                    .unwrap_or(AccountType::Checking),
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

            let account_type = AccountType::parse(&type_str).unwrap_or(AccountType::Checking);
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

    /// Investment performance for `[start, end]`. Start/end values reduce
    /// `get_holdings_for_summary` (Investment accounts only), FX-converted via
    /// `fx` per holding/transaction currency, so they stay consistent with
    /// net worth. `new_cash_invested` = net buys minus sells in range
    /// (see [`Self::compute_new_cash_invested`]). `market_growth` strips that
    /// out of the total value change to isolate price movement.
    pub fn compute_investment_metrics(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        profile_id: Option<&str>,
        fx: &crate::util::fx::FxRateMap,
    ) -> Result<InvestmentMetrics> {
        let sum_carry_forward = |date: NaiveDate| -> Result<Decimal> {
            Ok(self
                .get_holdings_for_summary(date, profile_id)?
                .iter()
                .filter(|r| matches!(r.account_type, AccountType::Investment))
                .map(|r| fx.convert(r.holding.value, &r.holding.currency))
                .sum())
        };

        let start_value = sum_carry_forward(start)?;
        let end_value = sum_carry_forward(end)?;

        let new_cash_invested = self.compute_new_cash_invested(start, end, profile_id, fx)?;

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
            let q = qty.parse::<Decimal>().unwrap_or_default();
            let p = price.parse::<Decimal>().unwrap_or_default();
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
                let f = fee.parse::<Decimal>().unwrap_or_default();
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
            let amount = Decimal::try_from(total_f64).unwrap_or_default();
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

            let price_per_share = match row.price_per_share.parse::<rust_decimal::Decimal>() {
                Ok(d) => d.to_string(),
                Err(_) => {
                    tracing::warn!(symbol = %row.symbol, "invalid price_per_share in investment row; marking as error");
                    previews.push(err_row(format!(
                        "Invalid price per share \"{}\"",
                        row.price_per_share
                    )));
                    continue;
                }
            };

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
                price_per_share: row.price_per_share.clone(),
                currency: row.currency.clone(),
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
fn parse_transaction_datetime(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .or_else(|| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
        })
}

// ── Row mappers ───────────────────────────────────────────────────────────────

fn row_to_category(row: &rusqlite::Row<'_>) -> rusqlite::Result<Category> {
    Ok(Category {
        id: row.get(0)?,
        name: row.get(1)?,
        parent_id: row.get(2)?,
        display_order: row.get(3)?,
        is_active: row.get::<_, i64>(4)? != 0,
        description: row.get(5)?,
        category_type: CategoryType::parse(&row.get::<_, String>(6)?)
            .unwrap_or(CategoryType::Spending),
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
        holding_type: HoldingType::parse(&holding_type_str).unwrap_or(HoldingType::Stock),
        quantity: quantity_str.parse::<Decimal>().unwrap_or_default(),
        price_per_unit: price_str.and_then(|s| s.parse::<Decimal>().ok()),
        value: value_str.parse::<Decimal>().unwrap_or_default(),
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
        amount: amount.parse::<Decimal>().unwrap_or_default(),
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
    let account_type = AccountType::parse(&type_str).unwrap_or(AccountType::Checking);
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
    Ok(InvestmentEvent {
        id: row.get(0)?,
        account_id: row.get(1)?,
        event_type: InvestmentEventType::parse(&event_type_str).unwrap_or(InvestmentEventType::Buy),
        symbol: row.get(3)?,
        date: parse_transaction_datetime(&date_str)
            .unwrap_or_else(|| chrono::Utc::now().naive_utc()),
        quantity: row.get::<_, String>(5)?.parse().unwrap_or_default(),
        price_per_share: row.get::<_, String>(6)?.parse().unwrap_or_default(),
        fee: fee.and_then(|s| s.parse::<Decimal>().ok()),
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
        let defaulted = db.create_investment_event(&base).unwrap();
        assert_eq!(defaulted.fee_currency.as_deref(), Some("USD"));

        // Fee present with an explicit, different fee_currency -> preserved.
        let explicit = db
            .create_investment_event(&crate::model::CreateInvestmentEventBody {
                symbol: "MSFT".to_string(),
                fee_currency: Some("GBP".to_string()),
                ..base.clone()
            })
            .unwrap();
        assert_eq!(explicit.fee_currency.as_deref(), Some("GBP"));

        // No fee -> fee_currency stays null.
        let no_fee = db
            .create_investment_event(&crate::model::CreateInvestmentEventBody {
                symbol: "TSLA".to_string(),
                fee: None,
                fee_currency: None,
                ..base.clone()
            })
            .unwrap();
        assert_eq!(no_fee.fee_currency, None);
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
        assert_eq!(db.list_documents().unwrap().len(), 1);
    }

    #[test]
    fn store_document_distinct_bytes_create_rows() {
        let (db, _f) = test_db();
        db.store_document("a.csv", "text/csv", b"aaa", "parse", None)
            .unwrap();
        db.store_document("b.csv", "text/csv", b"bbb", "parse", None)
            .unwrap();
        assert_eq!(db.list_documents().unwrap().len(), 2);
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
        let summaries = db.list_documents().unwrap();
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
            .list_documents()
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
