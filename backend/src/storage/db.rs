//! SQLite-backed persistence layer.
//!
//! The `Db` type owns a single `rusqlite::Connection` and exposes typed
//! methods for every query the rest of the crate needs. Phase 1 is
//! synchronous and single-threaded; the Axum server wraps this behind a
//! shared `Arc<Mutex<Db>>` without changing the surface area here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result, anyhow};
use chrono::NaiveDate;
use rusqlite::{Connection, params};
use rust_decimal::Decimal;

use crate::model::{
    Account, AccountType, BudgetRow, CategorySource, CategoryTotal, ChecklistItem,
    ChecklistStatus, CashFlowMonth, Granularity, Holding, HoldingType, ImportLog, ImportResult,
    ImportRowError, ImportTransaction, InsertOutcome, InvestmentMetrics, PortfolioHistoryRow,
    PortfolioSnapshot, Profile, SectionMapping, SnapshotDelta, SpendingGridRow, StandingBudget,
    Transaction,
};

/// The full schema DDL. Embedded at compile time so a release binary can
/// create a fresh DB on a new machine with no files on disk beside itself.
const SCHEMA_SQL: &str = include_str!("../../../db/sql/schema.sql");

/// Embedded categories for default section-mapping seed.
const CATEGORIES_YAML: &str = include_str!("../../config/categories.yaml");

/// Default section mappings seeded on first startup.
/// Format: (section, category). Built from `categories.yaml` taxonomy.
const DEFAULT_SECTION_MAPPINGS: &[(&str, &str)] = &[
    ("Income", "Income: Salary"),
    ("Income", "Income: Freelance"),
    ("Income", "Income: Investments"),
    ("Income", "Income: Other Income"),
    ("Bills", "Housing: Rent / Mortgage"),
    ("Bills", "Housing: Utilities"),
    ("Bills", "Housing: Internet & Phone"),
    ("Bills", "Housing: Home Maintenance"),
    ("Bills", "Finance: Insurance"),
    ("Bills", "Entertainment: Streaming Services"),
    ("Transfers", "Finance: Savings Transfer"),
    ("Transfers", "Finance: Investment Transfer"),
    ("Irregular", "Travel: Flights"),
    ("Irregular", "Travel: Accommodation"),
    ("Irregular", "Travel: Holiday Spending"),
    ("Spending", "Food: Groceries"),
    ("Spending", "Food: Dining & Bars"),
    ("Spending", "Food: Coffee & Cafes"),
    ("Spending", "Transport: Public Transit"),
    ("Spending", "Transport: Taxi & Rideshare"),
    ("Spending", "Transport: Fuel"),
    ("Spending", "Transport: Car Maintenance"),
    ("Spending", "Health: Gym & Fitness"),
    ("Spending", "Health: Medical & Dental"),
    ("Spending", "Health: Pharmacy"),
    ("Spending", "Shopping: Clothing"),
    ("Spending", "Shopping: Electronics"),
    ("Spending", "Shopping: General"),
    ("Spending", "Entertainment: Events & Concerts"),
    ("Spending", "Entertainment: Hobbies"),
    ("Spending", "Finance: Fees & Charges"),
    ("Spending", "Personal Care: Haircut & Beauty"),
    ("Spending", "Gifts & Donations: Gifts"),
    ("Spending", "Education: Courses & Books"),
    ("Spending", "Other: Uncategorized"),
];

/// Migration 001: adds detected_bank / detection_confidence to import_log.
const MIGRATION_001_STMTS: &[&str] = &[
    "ALTER TABLE import_log ADD COLUMN detected_bank TEXT",
    "ALTER TABLE import_log ADD COLUMN detection_confidence REAL",
];

/// Migration 002: adds profile_ids to accounts, short_name to holdings.
/// Each statement is guarded in `ensure_migration_002` by a pragma_table_info
/// check, so this can run safely on every startup.
const MIGRATION_002_STMTS: &[(&str, &str)] = &[
    (
        "accounts",
        "ALTER TABLE accounts ADD COLUMN profile_ids TEXT NOT NULL DEFAULT '[]'",
    ),
    (
        "holdings",
        "ALTER TABLE holdings ADD COLUMN short_name TEXT",
    ),
];

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
    /// Multi-select category names. Empty vec = no filter.
    pub categories: Option<Vec<String>>,
    /// Free-text search across normalized, description, category, notes.
    pub search: Option<String>,
    pub profile_id: Option<String>,
    pub page: u32,
    pub limit: u32,
}

impl Default for TransactionFilters {
    fn default() -> Self {
        Self {
            start: None,
            end: None,
            accounts: None,
            categories: None,
            search: None,
            profile_id: None,
            page: 1,
            limit: 25,
        }
    }
}

pub struct Db {
    conn: Connection,
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

        conn.execute_batch(SCHEMA_SQL)
            .context("running schema.sql")?;

        ensure_migration_001(&conn)?;
        ensure_migration_002(&conn)?;

        if path.exists() {
            set_file_mode_600(path)?;
        }

        Ok(Self { conn })
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

    // ── Accounts ─────────────────────────────────────────────────────────────

    pub fn upsert_account(&self, account: &Account) -> Result<()> {
        let profile_ids = serde_json::to_string(&account.profile_ids)
            .unwrap_or_else(|_| r#"["default"]"#.to_string());
        self.conn.execute(
            r"INSERT INTO accounts (
                id, name, institution, type, currency, balance, balance_date,
                is_active, notes, profile_ids
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                name        = excluded.name,
                institution = excluded.institution,
                type        = excluded.type,
                currency    = excluded.currency,
                balance     = COALESCE(excluded.balance, accounts.balance),
                balance_date = COALESCE(excluded.balance_date, accounts.balance_date),
                is_active   = excluded.is_active,
                notes       = excluded.notes,
                profile_ids = excluded.profile_ids",
            params![
                account.id,
                account.name,
                account.institution,
                account.account_type.as_str(),
                account.currency,
                account.balance.map(|b| b.to_string()),
                account.balance_date.map(|d| d.format("%Y-%m-%d").to_string()),
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
                id, name, institution, type, currency, balance, balance_date,
                is_active, notes, profile_ids
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                account.id,
                account.name,
                account.institution,
                account.account_type.as_str(),
                account.currency,
                account.balance.map(|b| b.to_string()),
                account.balance_date.map(|d| d.format("%Y-%m-%d").to_string()),
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
    pub fn get_accounts(&self, profile_id: Option<&str>) -> Result<Vec<Account>> {
        let (sql, args): (String, Vec<Box<dyn rusqlite::ToSql>>) = if let Some(pid) = profile_id {
            let pattern = format!("%\"{pid}\"%");
            (
                r"SELECT id, name, institution, type, currency, balance, balance_date,
                         is_active, notes, profile_ids
                  FROM accounts
                  WHERE profile_ids LIKE ?1
                  ORDER BY institution, name"
                    .to_string(),
                vec![Box::new(pattern)],
            )
        } else {
            (
                r"SELECT id, name, institution, type, currency, balance, balance_date,
                         is_active, notes, profile_ids
                  FROM accounts
                  ORDER BY institution, name"
                    .to_string(),
                vec![],
            )
        };

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())),
                row_to_account,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_account_by_id(&self, id: &str) -> Result<Option<Account>> {
        let result = self.conn.query_row(
            r"SELECT id, name, institution, type, currency, balance, balance_date,
                     is_active, notes, profile_ids
              FROM accounts WHERE id = ?1",
            params![id],
            row_to_account,
        );
        match result {
            Ok(a) => Ok(Some(a)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn account_exists(&self, id: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM accounts WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn set_account_balance(
        &self,
        account_id: &str,
        balance: Decimal,
        date: NaiveDate,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let updated = tx.execute(
            "UPDATE accounts SET balance = ?1, balance_date = ?2 WHERE id = ?3",
            params![
                balance.to_string(),
                date.format("%Y-%m-%d").to_string(),
                account_id
            ],
        )?;
        if updated == 0 {
            return Err(anyhow!("unknown account: {account_id}"));
        }
        tx.execute(
            r"INSERT INTO portfolio_snapshots (snapshot_date, account_id, balance, currency)
              VALUES (?1, ?2, ?3, COALESCE((SELECT currency FROM accounts WHERE id = ?2), 'GBP'))
              ON CONFLICT(snapshot_date, account_id) DO UPDATE SET balance = excluded.balance",
            params![
                date.format("%Y-%m-%d").to_string(),
                account_id,
                balance.to_string()
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    // ── Transactions ──────────────────────────────────────────────────────────

    /// Insert one transaction. `INSERT OR IGNORE` on the unique fingerprint
    /// makes the import idempotent.
    pub fn insert_transaction(&self, tx: &Transaction) -> Result<InsertOutcome> {
        let rows = self.conn.execute(
            r"INSERT OR IGNORE INTO transactions (
                id, date, description, normalized, amount, currency,
                account_id, category, category_source, confidence, notes,
                is_recurring, fingerprint, fitid
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                tx.id,
                tx.date.format("%Y-%m-%d").to_string(),
                tx.description,
                tx.normalized,
                tx.amount.to_string(),
                tx.currency,
                tx.account_id,
                tx.category,
                tx.category_source.as_ref().map(|s| s.as_str()),
                tx.confidence,
                tx.notes,
                tx.is_recurring as i64,
                tx.fingerprint,
                tx.fitid,
            ],
        )?;
        Ok(if rows == 1 {
            InsertOutcome::Inserted
        } else {
            InsertOutcome::Duplicate
        })
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
            let date_iso = t.date.format("%Y-%m-%d").to_string();
            let amount_str = t.amount.to_string();
            let currency = t.currency.clone().unwrap_or_else(|| "GBP".to_string());
            let normalized = normalize_description(&t.description);
            let fp = fingerprint(&date_iso, &amount_str, &t.description, account_id);

            let tx = Transaction {
                id: Uuid::new_v4().to_string(),
                date: t.date,
                description: t.description.clone(),
                normalized,
                amount: t.amount,
                currency,
                account_id: account_id.to_string(),
                category: t.category.clone(),
                category_source: t.category_source.clone(),
                confidence: None,
                notes: t.notes.clone(),
                is_recurring: t.is_recurring.unwrap_or(false),
                fingerprint: fp,
                fitid: None,
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

        if let Some(start) = filters.start {
            args.push(Box::new(start.format("%Y-%m-%d").to_string()));
            conditions.push(format!("t.date >= ?{}", args.len()));
        }
        if let Some(end) = filters.end {
            args.push(Box::new(end.format("%Y-%m-%d").to_string()));
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
                conditions.push(format!("t.category IN ({})", placeholders.join(",")));
            }
        }
        if let Some(search) = &filters.search {
            let pattern = format!("%{}%", search.replace('%', "\\%").replace('_', "\\_"));
            args.push(Box::new(pattern.clone()));
            let idx = args.len();
            conditions.push(format!(
                "(t.normalized LIKE ?{idx} ESCAPE '\\' OR t.description LIKE ?{idx} ESCAPE '\\' OR t.category LIKE ?{idx} ESCAPE '\\' OR t.notes LIKE ?{idx} ESCAPE '\\')"
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
        let where_clause = conditions.join(" AND ");

        let count_sql = format!(
            "SELECT COUNT(*) FROM transactions t {join} WHERE {where_clause}"
        );
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

        let data_sql = format!(
            r"SELECT t.id, t.date, t.description, t.normalized, t.amount, t.currency,
                     t.account_id, t.category, t.category_source, t.confidence, t.notes,
                     t.is_recurring, t.fingerprint, t.fitid
              FROM transactions t {join}
              WHERE {where_clause}
              ORDER BY t.date DESC, t.id DESC
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
    /// Returns signed sums (negative = net spend, positive = net income).
    pub fn get_transactions_by_category(
        &self,
        filters: &TransactionFilters,
    ) -> Result<Vec<CategoryTotal>> {
        let mut conditions: Vec<String> = vec!["t.category IS NOT NULL".to_string()];
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let mut need_account_join = false;

        if let Some(start) = filters.start {
            args.push(Box::new(start.format("%Y-%m-%d").to_string()));
            conditions.push(format!("t.date >= ?{}", args.len()));
        }
        if let Some(end) = filters.end {
            args.push(Box::new(end.format("%Y-%m-%d").to_string()));
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
        let where_clause = conditions.join(" AND ");

        let sql = format!(
            r"SELECT t.category, SUM(CAST(t.amount AS REAL)) AS total
              FROM transactions t {join}
              WHERE {where_clause}
              GROUP BY t.category
              ORDER BY total ASC"
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())),
                |row| {
                    let category: String = row.get(0)?;
                    let total: f64 = row.get(1)?;
                    Ok((category, total))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(rows
            .into_iter()
            .map(|(category, total)| CategoryTotal {
                category,
                total: Decimal::try_from(total)
                    .unwrap_or_default()
                    .to_string(),
            })
            .collect())
    }

    /// Returns the union of static taxonomy categories and distinct categories
    /// already present in the transactions table. Sorted, deduplicated.
    pub fn get_all_categories(&self) -> Result<Vec<String>> {
        let db_cats: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare("SELECT DISTINCT category FROM transactions WHERE category IS NOT NULL ORDER BY category")?;
            stmt.query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };

        let mut all = taxonomy_categories().clone();
        for c in db_cats {
            if !all.contains(&c) {
                all.push(c);
            }
        }
        all.sort();
        all.dedup();
        Ok(all)
    }

    pub fn update_transaction_category(
        &self,
        id: &str,
        category: &str,
        source: CategorySource,
    ) -> Result<()> {
        let updated = self.conn.execute(
            "UPDATE transactions SET category = ?1, category_source = ?2 WHERE id = ?3",
            params![category, source.as_str(), id],
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
        let result = self.conn.query_row(
            r"SELECT id, date, description, normalized, amount, currency,
                     account_id, category, category_source, confidence, notes,
                     is_recurring, fingerprint, fitid
              FROM transactions WHERE id = ?1",
            params![id],
            row_to_transaction,
        );
        match result {
            Ok(t) => Ok(Some(t)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // ── Section mappings ──────────────────────────────────────────────────────

    pub fn get_section_mappings(&self) -> Result<Vec<SectionMapping>> {
        let mut stmt = self.conn.prepare(
            "SELECT section, category FROM section_mappings ORDER BY section, category",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SectionMapping {
                    section: row.get(0)?,
                    category: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Full replacement: delete all rows and insert the new set.
    pub fn update_section_mappings(&self, mappings: &[SectionMapping]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute_batch("DELETE FROM section_mappings")?;
        for m in mappings {
            tx.execute(
                "INSERT INTO section_mappings (section, category) VALUES (?1, ?2)",
                params![m.section, m.category],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    // ── Budgets (standing + overrides) ───────────────────────────────────────

    pub fn get_standing_budgets(&self) -> Result<Vec<StandingBudget>> {
        let mut stmt = self
            .conn
            .prepare("SELECT category, amount FROM standing_budgets ORDER BY category")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(StandingBudget {
                    category: row.get(0)?,
                    amount: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_standing_budget(&self, category: &str, amount: Decimal) -> Result<()> {
        self.conn.execute(
            r"INSERT INTO standing_budgets (category, amount) VALUES (?1, ?2)
              ON CONFLICT(category) DO UPDATE SET amount = excluded.amount",
            params![category, amount.to_string()],
        )?;
        Ok(())
    }

    pub fn set_budget_override(
        &self,
        month: &str,
        category: &str,
        amount: Decimal,
    ) -> Result<()> {
        self.conn.execute(
            r"INSERT INTO budget_overrides (month, category, amount) VALUES (?1, ?2, ?3)
              ON CONFLICT(month, category) DO UPDATE SET amount = excluded.amount",
            params![month, category, amount.to_string()],
        )?;
        Ok(())
    }

    /// Returns effective budget rows for `month`, merging standing targets with
    /// per-month overrides. Includes actual spend from transactions.
    pub fn get_effective_budget(&self, month: &str) -> Result<Vec<BudgetRow>> {
        // Collect all categories that either have a standing budget or had
        // transactions this month.
        let sql = r"
            SELECT
                COALESCE(sb.category, t_agg.category) AS category,
                COALESCE(bo.amount, sb.amount)         AS budgeted,
                COALESCE(t_agg.actual, '0')            AS actual
            FROM standing_budgets sb
            LEFT JOIN budget_overrides bo
                ON bo.category = sb.category AND bo.month = ?1
            LEFT JOIN (
                SELECT category, SUM(ABS(CAST(amount AS REAL))) AS actual_raw,
                       CAST(SUM(ABS(CAST(amount AS REAL))) AS TEXT) AS actual
                FROM transactions
                WHERE substr(date, 1, 7) = ?1 AND CAST(amount AS REAL) < 0
                  AND category IS NOT NULL
                GROUP BY category
            ) t_agg ON t_agg.category = sb.category
            UNION
            SELECT
                t_agg2.category,
                bo2.amount,
                t_agg2.actual
            FROM (
                SELECT category, CAST(SUM(ABS(CAST(amount AS REAL))) AS TEXT) AS actual
                FROM transactions
                WHERE substr(date, 1, 7) = ?1 AND CAST(amount AS REAL) < 0
                  AND category IS NOT NULL
                GROUP BY category
            ) t_agg2
            LEFT JOIN budget_overrides bo2
                ON bo2.category = t_agg2.category AND bo2.month = ?1
            WHERE t_agg2.category NOT IN (SELECT category FROM standing_budgets)
            ORDER BY category
        ";

        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt
            .query_map(params![month, month, month], |row| {
                let category: String = row.get(0)?;
                let budgeted: Option<String> = row.get(1)?;
                let actual: String = row.get(2)?;
                Ok((category, budgeted, actual))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(rows
            .into_iter()
            .map(|(category, budgeted, actual)| {
                let actual_dec: Decimal = actual.parse().unwrap_or_default();
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
                    category,
                    budgeted,
                    actual: actual_dec.to_string(),
                    percent,
                }
            })
            .collect())
    }

    /// Spending grid: aggregated spending per category per time period.
    pub fn get_spending_grid(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        granularity: &Granularity,
        profile_id: Option<&str>,
    ) -> Result<Vec<SpendingGridRow>> {
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

        let mut conditions = vec![
            "t.date >= ?1".to_string(),
            "t.date <= ?2".to_string(),
            "t.category IS NOT NULL".to_string(),
        ];
        let mut extra_args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        let join = if let Some(pid) = profile_id {
            let pattern = format!("%\"{pid}\"%");
            extra_args.push(Box::new(pattern));
            conditions.push(format!("a.profile_ids LIKE ?{}", 2 + extra_args.len()));
            "JOIN accounts a ON a.id = t.account_id"
        } else {
            ""
        };

        let where_clause = conditions.join(" AND ");

        let sql = format!(
            r"SELECT
                t.category,
                COALESCE(sm.section, 'Spending') AS section,
                {period_expr} AS period,
                SUM(CAST(t.amount AS REAL)) AS period_total
              FROM transactions t
              {join}
              LEFT JOIN section_mappings sm ON sm.category = t.category
              WHERE {where_clause}
              GROUP BY t.category, period
              ORDER BY t.category, period"
        );

        let start_str = start.format("%Y-%m-%d").to_string();
        let end_str = end.format("%Y-%m-%d").to_string();

        let mut base_args: Vec<Box<dyn rusqlite::ToSql>> = vec![
            Box::new(start_str),
            Box::new(end_str),
        ];
        base_args.extend(extra_args);

        let mut stmt = self.conn.prepare(&sql)?;
        let raw: Vec<(String, String, String, f64)> = stmt
            .query_map(
                rusqlite::params_from_iter(base_args.iter().map(|b| b.as_ref())),
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        // Fetch standing budgets for budget column
        let budgets: HashMap<String, String> = {
            let mut stmt = self
                .conn
                .prepare("SELECT category, amount FROM standing_budgets")?;
            stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .collect()
        };

        // Build the grid rows
        let mut grid: HashMap<String, SpendingGridRow> = HashMap::new();
        for (category, section, period, total_f64) in raw {
            let total_dec = Decimal::try_from(total_f64).unwrap_or_default();
            let entry = grid.entry(category.clone()).or_insert_with(|| SpendingGridRow {
                category: category.clone(),
                section: section.clone(),
                periods: HashMap::new(),
                average: None,
                budget: None,
                total: None,
            });
            entry.periods.insert(period, Some(total_dec.to_string()));
        }

        // Compute totals, averages, and attach budgets
        let mut result: Vec<SpendingGridRow> = grid
            .into_values()
            .map(|mut row| {
                let vals: Vec<Decimal> = row
                    .periods
                    .values()
                    .filter_map(|v| v.as_ref())
                    .filter_map(|s| s.parse::<Decimal>().ok())
                    .collect();
                if !vals.is_empty() {
                    let sum: Decimal = vals.iter().sum();
                    row.total = Some(sum.to_string());
                    let count = Decimal::from(vals.len() as u64);
                    row.average = Some((sum / count).to_string());
                }
                row.budget = budgets.get(&row.category).cloned();
                row
            })
            .collect();

        result.sort_by(|a, b| a.category.cmp(&b.category));
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

    pub fn upsert_portfolio_snapshot(&self, snapshot: &PortfolioSnapshot) -> Result<()> {
        self.conn.execute(
            r"INSERT INTO portfolio_snapshots (snapshot_date, account_id, balance, currency)
              VALUES (?1, ?2, ?3, ?4)
              ON CONFLICT(snapshot_date, account_id) DO UPDATE SET
                balance  = excluded.balance,
                currency = excluded.currency",
            params![
                snapshot.snapshot_date.format("%Y-%m-%d").to_string(),
                snapshot.account_id,
                snapshot.balance.to_string(),
                snapshot.currency,
            ],
        )?;
        Ok(())
    }

    pub fn upsert_holdings(&self, account_id: &str, holdings: &[Holding]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for h in holdings {
            tx.execute(
                r"INSERT INTO holdings (
                    account_id, symbol, name, holding_type, quantity, price_per_unit,
                    value, currency, as_of, short_name
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT(account_id, symbol, as_of) DO UPDATE SET
                    name           = excluded.name,
                    holding_type   = excluded.holding_type,
                    quantity       = excluded.quantity,
                    price_per_unit = excluded.price_per_unit,
                    value          = excluded.value,
                    currency       = excluded.currency,
                    short_name     = excluded.short_name",
                params![
                    account_id,
                    h.symbol,
                    h.name,
                    h.holding_type.as_str(),
                    h.quantity.to_string(),
                    h.price_per_unit.map(|p| p.to_string()),
                    h.value.to_string(),
                    h.currency,
                    h.as_of.format("%Y-%m-%d").to_string(),
                    h.short_name,
                ],
            )?;
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

    /// Returns all accounts with their carry-forward balance as of `as_of`.
    /// Each returned `Account` has:
    /// - `balance`: the most recent portfolio_snapshots balance <= `as_of`, or `None`
    /// - `balance_date`: the snapshot_date that was carried forward
    /// - `is_stale`: `Some(true)` if snapshot_date is > 45 days before `as_of`
    pub fn get_portfolio_as_of(
        &self,
        as_of: NaiveDate,
        profile_id: Option<&str>,
    ) -> Result<Vec<Account>> {
        let as_of_str = as_of.format("%Y-%m-%d").to_string();
        let stale_days = 45i64;

        let (profile_filter, profile_arg): (String, Option<String>) = if let Some(pid) = profile_id
        {
            let pattern = format!("%\"{pid}\"%");
            (
                "AND a.profile_ids LIKE ?2".to_string(),
                Some(pattern),
            )
        } else {
            (String::new(), None)
        };

        let sql = format!(
            r"SELECT
                a.id, a.name, a.institution, a.type, a.currency,
                a.is_active, a.notes, a.profile_ids,
                ps.balance AS snap_balance,
                ps.snapshot_date
              FROM accounts a
              LEFT JOIN (
                  SELECT ps1.account_id, ps1.balance, ps1.snapshot_date
                  FROM portfolio_snapshots ps1
                  WHERE ps1.snapshot_date <= ?1
                    AND ps1.snapshot_date = (
                        SELECT MAX(ps2.snapshot_date)
                        FROM portfolio_snapshots ps2
                        WHERE ps2.account_id = ps1.account_id
                          AND ps2.snapshot_date <= ?1
                    )
              ) ps ON ps.account_id = a.id
              WHERE a.is_active = 1
              {profile_filter}
              ORDER BY a.institution, a.name"
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows: Vec<Account> = if let Some(ref pat) = profile_arg {
            stmt.query_map(rusqlite::params![as_of_str, pat], |row| {
                row_to_portfolio_account(row, as_of, stale_days)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map(rusqlite::params![as_of_str], |row| {
                row_to_portfolio_account(row, as_of, stale_days)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };

        Ok(rows)
    }

    /// Returns one `PortfolioHistoryRow` per period between `from` and `to`.
    /// Uses carry-forward semantics: point-in-time balance at period end.
    pub fn get_monthly_net_worth(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        granularity: &Granularity,
        profile_id: Option<&str>,
    ) -> Result<Vec<PortfolioHistoryRow>> {
        let periods = generate_period_end_dates(from, to, granularity);
        let mut rows = Vec::new();

        for (label, period_end) in periods {
            let accounts = self.get_portfolio_as_of(period_end, profile_id)?;
            let available: Decimal = accounts
                .iter()
                .filter(|a| is_available_account(&a.account_type))
                .filter_map(|a| a.balance)
                .sum();
            let unavailable: Decimal = accounts
                .iter()
                .filter(|a| !is_available_account(&a.account_type))
                .filter_map(|a| a.balance)
                .sum();
            rows.push(PortfolioHistoryRow {
                month: label,
                available_wealth: available,
                unavailable_wealth: unavailable,
                total_wealth: available + unavailable,
            });
        }

        Ok(rows)
    }

    /// Returns the first and last snapshot per account within `[start, end]`,
    /// and the delta between them.
    pub fn get_snapshot_summary(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<SnapshotDelta>> {
        let start_str = start.format("%Y-%m-%d").to_string();
        let end_str = end.format("%Y-%m-%d").to_string();

        // Get all account IDs that have at least one snapshot in range.
        let account_ids: Vec<String> = {
            let mut stmt = self.conn.prepare(
                "SELECT DISTINCT account_id FROM portfolio_snapshots WHERE snapshot_date >= ?1 AND snapshot_date <= ?2",
            )?;
            stmt.query_map(rusqlite::params![start_str, end_str], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };

        let mut result = Vec::new();
        for account_id in account_ids {
            // First snapshot >= start
            let start_balance: Option<String> = self
                .conn
                .query_row(
                    r"SELECT balance FROM portfolio_snapshots
                      WHERE account_id = ?1 AND snapshot_date >= ?2
                      ORDER BY snapshot_date ASC LIMIT 1",
                    rusqlite::params![account_id, start_str],
                    |row| row.get(0),
                )
                .ok();

            // Last snapshot <= end
            let end_balance: Option<String> = self
                .conn
                .query_row(
                    r"SELECT balance FROM portfolio_snapshots
                      WHERE account_id = ?1 AND snapshot_date <= ?2
                      ORDER BY snapshot_date DESC LIMIT 1",
                    rusqlite::params![account_id, end_str],
                    |row| row.get(0),
                )
                .ok();

            let start_dec = start_balance.as_deref().and_then(|s| s.parse::<Decimal>().ok());
            let end_dec = end_balance.as_deref().and_then(|s| s.parse::<Decimal>().ok());
            let delta = start_dec.zip(end_dec).map(|(s, e)| e - s);

            result.push(SnapshotDelta {
                account_id,
                start_balance: start_dec,
                end_balance: end_dec,
                delta,
            });
        }

        Ok(result)
    }

    /// Returns all portfolio snapshots in `[start, end]`, ordered by date and account.
    pub fn get_snapshots_in_range(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<PortfolioSnapshot>> {
        let mut stmt = self.conn.prepare(
            r"SELECT snapshot_date, account_id, balance, currency
              FROM portfolio_snapshots
              WHERE snapshot_date >= ?1 AND snapshot_date <= ?2
              ORDER BY snapshot_date, account_id",
        )?;
        let rows = stmt
            .query_map(
                rusqlite::params![
                    start.format("%Y-%m-%d").to_string(),
                    end.format("%Y-%m-%d").to_string()
                ],
                |row| {
                    let date_str: String = row.get(0)?;
                    let balance_str: String = row.get(2)?;
                    Ok((date_str, row.get::<_, String>(1)?, balance_str, row.get::<_, String>(3)?))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(rows
            .into_iter()
            .filter_map(|(date_str, account_id, balance_str, currency)| {
                let date = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d").ok()?;
                let balance = balance_str.parse::<Decimal>().ok()?;
                Some(PortfolioSnapshot {
                    snapshot_date: date,
                    account_id,
                    balance,
                    currency,
                })
            })
            .collect())
    }

    /// Returns income and spending aggregated by period.
    pub fn get_cash_flow(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        profile_id: Option<&str>,
        granularity: &Granularity,
    ) -> Result<Vec<CashFlowMonth>> {
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

        let mut conditions = vec![
            "t.date >= ?1".to_string(),
            "t.date <= ?2".to_string(),
        ];
        let mut extra_args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        let join = if let Some(pid) = profile_id {
            let pattern = format!("%\"{pid}\"%");
            extra_args.push(Box::new(pattern));
            conditions.push(format!("a.profile_ids LIKE ?{}", 2 + extra_args.len()));
            "JOIN accounts a ON a.id = t.account_id"
        } else {
            ""
        };

        let where_clause = conditions.join(" AND ");

        let sql = format!(
            r"SELECT
                {period_expr} AS period,
                SUM(CASE WHEN CAST(t.amount AS REAL) > 0 THEN CAST(t.amount AS REAL) ELSE 0 END) AS income,
                SUM(CASE WHEN CAST(t.amount AS REAL) < 0 THEN ABS(CAST(t.amount AS REAL)) ELSE 0 END) AS spending
              FROM transactions t
              {join}
              WHERE {where_clause}
              GROUP BY period
              ORDER BY period"
        );

        let start_str = start.format("%Y-%m-%d").to_string();
        let end_str = end.format("%Y-%m-%d").to_string();

        let mut base_args: Vec<Box<dyn rusqlite::ToSql>> =
            vec![Box::new(start_str), Box::new(end_str)];
        base_args.extend(extra_args);

        let mut stmt = self.conn.prepare(&sql)?;
        let raw: Vec<(String, f64, f64)> = stmt
            .query_map(
                rusqlite::params_from_iter(base_args.iter().map(|b| b.as_ref())),
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(raw
            .into_iter()
            .map(|(period, income_f, spending_f)| CashFlowMonth {
                month: period,
                income: Decimal::try_from(income_f).unwrap_or_default(),
                spending: Decimal::try_from(spending_f).unwrap_or_default(),
            })
            .collect())
    }

    /// Returns the latest holdings (carry-forward) for all specified accounts.
    pub fn get_holdings_batch(&self, account_ids: &[String]) -> Result<Vec<Holding>> {
        if account_ids.is_empty() {
            return Ok(vec![]);
        }
        let placeholders: String = account_ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");

        let sql = format!(
            r"SELECT h.account_id, h.symbol, h.name, h.holding_type,
                     h.quantity, h.price_per_unit, h.value, h.currency,
                     h.as_of, h.short_name
              FROM holdings h
              WHERE h.account_id IN ({placeholders})
                AND h.as_of = (
                    SELECT MAX(h2.as_of) FROM holdings h2
                    WHERE h2.account_id = h.account_id
                )
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

        let tx = self.conn.unchecked_transaction()?;

        // Collect distinct as_of dates to replace.
        let mut dates: Vec<String> = holdings
            .iter()
            .map(|h| h.as_of.format("%Y-%m-%d").to_string())
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
                    value, currency, as_of, short_name
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    account_id,
                    h.symbol,
                    h.name,
                    h.holding_type.as_str(),
                    h.quantity.to_string(),
                    h.price_per_unit.map(|p| p.to_string()),
                    h.value.to_string(),
                    h.currency,
                    h.as_of.format("%Y-%m-%d").to_string(),
                    h.short_name,
                ],
            )?;
            inserted += 1;
        }

        tx.commit()?;
        Ok(inserted)
    }

    /// Compute investment performance metrics for `[start, end]`.
    ///
    /// Uses carry-forward for start and end values on investment accounts.
    /// `new_cash_invested` = signed sum of `Finance: Investment Transfer` transactions.
    pub fn compute_investment_metrics(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        profile_id: Option<&str>,
    ) -> Result<InvestmentMetrics> {
        // Fetch investment accounts only.
        let all_accounts = self.get_accounts(profile_id)?;
        let investment_ids: Vec<String> = all_accounts
            .into_iter()
            .filter(|a| matches!(a.account_type, AccountType::Investment))
            .map(|a| a.id)
            .collect();

        let sum_carry_forward = |date: NaiveDate| -> Result<Decimal> {
            let date_str = date.format("%Y-%m-%d").to_string();
            let mut total = Decimal::ZERO;
            for id in &investment_ids {
                let balance: Option<String> = self
                    .conn
                    .query_row(
                        r"SELECT balance FROM portfolio_snapshots
                          WHERE account_id = ?1 AND snapshot_date <= ?2
                          ORDER BY snapshot_date DESC LIMIT 1",
                        rusqlite::params![id, date_str],
                        |row| row.get(0),
                    )
                    .ok();
                if let Some(b) = balance.and_then(|s| s.parse::<Decimal>().ok()) {
                    total += b;
                }
            }
            Ok(total)
        };

        let start_value = sum_carry_forward(start)?;
        let end_value = sum_carry_forward(end)?;

        // Net cash moved into investment accounts.
        let new_cash_invested: Decimal = {
            let start_str = start.format("%Y-%m-%d").to_string();
            let end_str = end.format("%Y-%m-%d").to_string();
            let raw: Option<f64> = self
                .conn
                .query_row(
                    r"SELECT SUM(CAST(amount AS REAL)) FROM transactions
                      WHERE category = 'Finance: Investment Transfer'
                        AND date >= ?1 AND date <= ?2",
                    rusqlite::params![start_str, end_str],
                    |row| row.get(0),
                )
                .ok()
                .flatten();
            Decimal::try_from(raw.unwrap_or(0.0)).unwrap_or_default()
        };

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

// ── Row mappers ───────────────────────────────────────────────────────────────

/// Map a row from the carry-forward portfolio query into an Account.
/// Columns: id(0) name(1) institution(2) type(3) currency(4) is_active(5)
///          notes(6) profile_ids(7) snap_balance(8) snapshot_date(9)
fn row_to_portfolio_account(
    row: &rusqlite::Row<'_>,
    as_of: NaiveDate,
    stale_days: i64,
) -> rusqlite::Result<Account> {
    let type_str: String = row.get(3)?;
    let profile_ids_str: String = row.get(7).unwrap_or_else(|_| "[]".to_string());
    let profile_ids: Vec<String> =
        serde_json::from_str(&profile_ids_str).unwrap_or_else(|_| vec!["default".to_string()]);

    let snap_balance: Option<String> = row.get(8)?;
    let snap_date_str: Option<String> = row.get(9)?;

    let balance = snap_balance.as_deref().and_then(|s| s.parse::<Decimal>().ok());
    let balance_date =
        snap_date_str.as_deref().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());

    let is_stale = balance_date
        .map(|d| (as_of - d).num_days() > stale_days)
        .unwrap_or(false);

    Ok(Account {
        id: row.get(0)?,
        name: row.get(1)?,
        institution: row.get(2)?,
        account_type: AccountType::parse(&type_str).unwrap_or(AccountType::Checking),
        currency: row.get(4)?,
        balance,
        balance_date,
        is_active: row.get::<_, i64>(5)? != 0,
        notes: row.get(6)?,
        profile_ids,
        is_stale: Some(is_stale),
    })
}

fn row_to_holding(row: &rusqlite::Row<'_>) -> rusqlite::Result<Holding> {
    let holding_type_str: String = row.get(3)?;
    let quantity_str: String = row.get(4)?;
    let price_str: Option<String> = row.get(5)?;
    let value_str: String = row.get(6)?;
    let as_of_str: String = row.get(8)?;
    Ok(Holding {
        account_id: row.get(0)?,
        symbol: row.get(1)?,
        name: row.get(2)?,
        holding_type: HoldingType::parse(&holding_type_str).unwrap_or(HoldingType::Stock),
        quantity: quantity_str.parse::<Decimal>().unwrap_or_default(),
        price_per_unit: price_str.and_then(|s| s.parse::<Decimal>().ok()),
        value: value_str.parse::<Decimal>().unwrap_or_default(),
        currency: row.get(7)?,
        as_of: NaiveDate::parse_from_str(&as_of_str, "%Y-%m-%d").unwrap_or_else(|_| {
            chrono::Local::now().date_naive()
        }),
        short_name: row.get(9)?,
    })
}

fn row_to_transaction(row: &rusqlite::Row<'_>) -> rusqlite::Result<Transaction> {
    let date: String = row.get(1)?;
    let amount: String = row.get(4)?;
    let cat_source: Option<String> = row.get(8)?;
    Ok(Transaction {
        id: row.get(0)?,
        date: NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
        })?,
        description: row.get(2)?,
        normalized: row.get(3)?,
        amount: amount.parse::<Decimal>().unwrap_or_default(),
        currency: row.get(5)?,
        account_id: row.get(6)?,
        category: row.get(7)?,
        category_source: cat_source.as_deref().and_then(CategorySource::parse),
        confidence: row.get(9)?,
        notes: row.get(10)?,
        is_recurring: row.get::<_, i64>(11)? != 0,
        fingerprint: row.get(12)?,
        fitid: row.get(13)?,
    })
}

fn row_to_account(row: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
    let type_str: String = row.get(3)?;
    let balance: Option<String> = row.get(5)?;
    let balance_date: Option<String> = row.get(6)?;
    let profile_ids_str: String = row.get(9).unwrap_or_else(|_| "[]".to_string());
    let profile_ids: Vec<String> =
        serde_json::from_str(&profile_ids_str).unwrap_or_else(|_| vec!["default".to_string()]);
    Ok(Account {
        id: row.get(0)?,
        name: row.get(1)?,
        institution: row.get(2)?,
        account_type: AccountType::parse(&type_str).unwrap_or(AccountType::Checking),
        currency: row.get(4)?,
        balance: balance.and_then(|s| s.parse::<Decimal>().ok()),
        balance_date: balance_date.and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
        is_active: row.get::<_, i64>(7)? != 0,
        notes: row.get(8)?,
        profile_ids,
        is_stale: None,
    })
}

// ── Portfolio helpers ─────────────────────────────────────────────────────────

/// Returns `true` for account types counted in "available wealth".
pub fn is_available_account(t: &AccountType) -> bool {
    matches!(
        t,
        AccountType::Checking
            | AccountType::Savings
            | AccountType::Investment
            | AccountType::Cash
            | AccountType::Credit
    )
}

/// Map an account type to a broad asset class label for `by_asset_class`.
pub fn account_type_to_asset_class(t: &AccountType) -> &'static str {
    match t {
        AccountType::Investment => "Stocks",
        AccountType::Pension => "Pension",
        AccountType::Checking | AccountType::Savings | AccountType::Cash => "Cash",
        AccountType::Credit => "Credit",
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
                let period_end =
                    NaiveDate::from_ymd_opt(year, 12, 31).unwrap().min(to);
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

// ── Taxonomy helpers ──────────────────────────────────────────────────────────

fn taxonomy_categories() -> &'static Vec<String> {
    static CATS: OnceLock<Vec<String>> = OnceLock::new();
    CATS.get_or_init(|| {
        let value: serde_yaml::Value =
            serde_yaml::from_str(CATEGORIES_YAML).unwrap_or(serde_yaml::Value::Null);
        let mut result = Vec::new();
        if let Some(cats) = value.get("categories").and_then(|v| v.as_sequence()) {
            for cat in cats {
                let parent = cat
                    .get("parent")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if let Some(children) = cat.get("children").and_then(|v| v.as_sequence()) {
                    for child in children {
                        if let Some(child_str) = child.as_str() {
                            result.push(format!("{parent}: {child_str}"));
                        }
                    }
                }
            }
        }
        result.sort();
        result
    })
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
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    hex::encode(hasher.finalize())
}

// ── Migration helpers ─────────────────────────────────────────────────────────

fn ensure_migration_001(conn: &Connection) -> Result<()> {
    for stmt in MIGRATION_001_STMTS {
        let col_name = stmt
            .split_whitespace()
            .nth(5)
            .ok_or_else(|| anyhow!("malformed migration 001: {stmt}"))?;
        let already_exists: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('import_log') WHERE name = ?1",
            params![col_name],
            |row| row.get(0),
        )?;
        if !already_exists {
            conn.execute_batch(stmt)
                .with_context(|| format!("applying migration 001: {stmt}"))?;
        }
    }
    Ok(())
}

fn ensure_migration_002(conn: &Connection) -> Result<()> {
    // ADD COLUMN steps (idempotent via pragma check)
    for (table, stmt) in MIGRATION_002_STMTS {
        let col_name = stmt
            .split_whitespace()
            .nth(5)
            .ok_or_else(|| anyhow!("malformed migration 002: {stmt}"))?;
        let already_exists: bool = conn.query_row(
            &format!("SELECT COUNT(*) > 0 FROM pragma_table_info('{table}') WHERE name = ?1"),
            params![col_name],
            |row| row.get(0),
        )?;
        if !already_exists {
            conn.execute_batch(stmt)
                .with_context(|| format!("applying migration 002 to {table}: {stmt}"))?;
        }
    }

    // Backfill existing accounts: set profile_ids = '["default"]' where still '[]'
    conn.execute_batch(
        r#"UPDATE accounts SET profile_ids = '["default"]' WHERE profile_ids = '[]'"#,
    )?;

    // Migrate budgets -> standing_budgets (take most recent amount per category)
    conn.execute_batch(
        r"INSERT OR IGNORE INTO standing_budgets (category, amount)
          SELECT b.category, b.amount
          FROM budgets b
          INNER JOIN (
              SELECT category, MAX(month) AS max_month FROM budgets GROUP BY category
          ) latest ON b.category = latest.category AND b.month = latest.max_month",
    )?;

    // Seed default profile (idempotent)
    conn.execute_batch(
        "INSERT OR IGNORE INTO profiles (id, name) VALUES ('default', 'Default')",
    )?;

    // Seed section_mappings defaults (INSERT OR IGNORE is idempotent due to UNIQUE on category)
    let mut insert_section = conn.prepare(
        "INSERT OR IGNORE INTO section_mappings (section, category) VALUES (?1, ?2)",
    )?;
    for (section, category) in DEFAULT_SECTION_MAPPINGS {
        insert_section.execute(params![section, category])?;
    }

    Ok(())
}

// Keep HoldingType referenced to avoid dead_code warning.
#[allow(dead_code)]
const _: Option<HoldingType> = None;
