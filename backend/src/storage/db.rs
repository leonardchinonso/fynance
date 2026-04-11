//! SQLite-backed persistence layer.
//!
//! The `Db` type owns a single `rusqlite::Connection` and exposes typed
//! methods for every query the rest of the crate needs. Phase 1 is
//! synchronous and single-threaded; the Axum server in Phase 2 will wrap
//! this behind a shared `Arc<Mutex<Db>>` (or a connection pool) without
//! changing the surface area here.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::NaiveDate;
use rusqlite::{Connection, params};
use rust_decimal::Decimal;

use crate::model::{
    Account, AccountType, Budget, CategorySource, Holding, HoldingType, ImportLog, InsertOutcome,
    PortfolioSnapshot, Transaction,
};

/// The full schema DDL. Embedded at compile time so a release binary can
/// create a fresh DB on a new machine with no files on disk beside itself.
const SCHEMA_SQL: &str = include_str!("../../../db/sql/schema.sql");

/// Resolve the default DB path. On Linux this is
/// `~/.local/share/fynance/fynance.db`; on macOS it's
/// `~/Library/Application Support/fynance/fynance.db`.
pub fn default_db_path() -> Result<PathBuf> {
    let base = dirs::data_local_dir()
        .ok_or_else(|| anyhow!("could not resolve OS data directory; set FYNANCE_DB_PATH"))?;
    Ok(base.join("fynance").join("fynance.db"))
}

/// Filter params for `Db::get_transactions`. `None` means "no filter on
/// this column".
#[derive(Debug, Default, Clone)]
pub struct TransactionFilters {
    pub month: Option<String>,
    pub category: Option<String>,
    pub account_id: Option<String>,
    pub page: Option<u32>,
    pub limit: Option<u32>,
}

pub struct Db {
    conn: Connection,
}

impl Db {
    /// Open (or create) the database at `path`. The parent directory is
    /// created with mode 0700 and the DB file with 0600 on Unix so that a
    /// shared-machine snoop cannot read another user's transactions.
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

        // WAL mode gives us concurrent readers + one writer without the
        // rollback-journal stalls we'd get from the default.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;

        conn.execute_batch(SCHEMA_SQL)
            .context("running schema.sql")?;

        // Set file mode after the DB file has been created so the pragma
        // calls above don't race against the chmod.
        if path.exists() {
            set_file_mode_600(path)?;
        }

        Ok(Self { conn })
    }

    /// Insert one transaction. Relies on the `fingerprint UNIQUE`
    /// constraint plus `INSERT OR IGNORE` so that a repeat statement does
    /// not blow up the whole import run.
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

    pub fn log_import(&self, log: &ImportLog) -> Result<()> {
        self.conn.execute(
            r"INSERT INTO import_log (
                filename, account_id, rows_total, rows_inserted, rows_duplicate, source
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                log.filename,
                log.account_id,
                log.rows_total as i64,
                log.rows_inserted as i64,
                log.rows_duplicate as i64,
                log.source,
            ],
        )?;
        Ok(())
    }

    pub fn upsert_account(&self, account: &Account) -> Result<()> {
        self.conn.execute(
            r"INSERT INTO accounts (
                id, name, institution, type, currency, balance, balance_date, is_active, notes
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                institution = excluded.institution,
                type = excluded.type,
                currency = excluded.currency,
                balance = COALESCE(excluded.balance, accounts.balance),
                balance_date = COALESCE(excluded.balance_date, accounts.balance_date),
                is_active = excluded.is_active,
                notes = excluded.notes",
            params![
                account.id,
                account.name,
                account.institution,
                account.account_type.as_str(),
                account.currency,
                account.balance.map(|b| b.to_string()),
                account
                    .balance_date
                    .map(|d| d.format("%Y-%m-%d").to_string()),
                account.is_active as i64,
                account.notes,
            ],
        )?;
        Ok(())
    }

    pub fn get_accounts(&self) -> Result<Vec<Account>> {
        let mut stmt = self.conn.prepare(
            r"SELECT id, name, institution, type, currency, balance, balance_date, is_active, notes
              FROM accounts
              ORDER BY institution, name",
        )?;
        let rows = stmt
            .query_map([], row_to_account)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_account_balance(
        &self,
        account_id: &str,
        balance: Decimal,
        date: NaiveDate,
    ) -> Result<()> {
        // Keep the accounts row and the snapshots table in lockstep so the
        // "latest balance" and "historical balance" readers never disagree.
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

    pub fn get_transactions(&self, filters: &TransactionFilters) -> Result<Vec<Transaction>> {
        let mut sql = String::from(
            r"SELECT id, date, description, normalized, amount, currency, account_id,
                     category, category_source, confidence, notes, is_recurring,
                     fingerprint, fitid
              FROM transactions WHERE 1=1",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(month) = &filters.month {
            sql.push_str(" AND substr(date, 1, 7) = ?");
            args.push(Box::new(month.clone()));
        }
        if let Some(category) = &filters.category {
            sql.push_str(" AND category = ?");
            args.push(Box::new(category.clone()));
        }
        if let Some(account) = &filters.account_id {
            sql.push_str(" AND account_id = ?");
            args.push(Box::new(account.clone()));
        }
        sql.push_str(" ORDER BY date DESC, id DESC");

        if let Some(limit) = filters.limit {
            sql.push_str(" LIMIT ?");
            args.push(Box::new(limit as i64));
            let page = filters.page.unwrap_or(1).max(1);
            let offset = (page - 1) as i64 * limit as i64;
            sql.push_str(" OFFSET ?");
            args.push(Box::new(offset));
        }

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())),
                row_to_transaction,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
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

    pub fn set_budget(&self, month: &str, category: &str, amount: Decimal) -> Result<()> {
        self.conn.execute(
            r"INSERT INTO budgets (month, category, amount)
              VALUES (?1, ?2, ?3)
              ON CONFLICT(month, category) DO UPDATE SET amount = excluded.amount",
            params![month, category, amount.to_string()],
        )?;
        Ok(())
    }

    pub fn get_budgets_for_month(&self, month: &str) -> Result<Vec<Budget>> {
        let mut stmt = self.conn.prepare(
            "SELECT month, category, amount FROM budgets WHERE month = ?1 ORDER BY category",
        )?;
        let rows = stmt
            .query_map(params![month], |row| {
                let amount: String = row.get(2)?;
                Ok(Budget {
                    month: row.get(0)?,
                    category: row.get(1)?,
                    amount: amount.parse::<Decimal>().unwrap_or_default(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn upsert_portfolio_snapshot(&self, snapshot: &PortfolioSnapshot) -> Result<()> {
        self.conn.execute(
            r"INSERT INTO portfolio_snapshots (snapshot_date, account_id, balance, currency)
              VALUES (?1, ?2, ?3, ?4)
              ON CONFLICT(snapshot_date, account_id) DO UPDATE SET
                balance = excluded.balance,
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
                    value, currency, as_of
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                ON CONFLICT(account_id, symbol, as_of) DO UPDATE SET
                    name = excluded.name,
                    holding_type = excluded.holding_type,
                    quantity = excluded.quantity,
                    price_per_unit = excluded.price_per_unit,
                    value = excluded.value,
                    currency = excluded.currency",
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
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    // ── API tokens ──────────────────────────────────────────────────
    //
    // Tokens are generated as `fyn_` + 32 random hex bytes (64 chars of
    // hex after the prefix). We only store the SHA-256 of the raw token
    // so an attacker with read-only DB access cannot replay tokens. The
    // raw token is shown to the user exactly once at creation time.

    /// Create a new token and return the raw string to display. The
    /// caller is responsible for telling the user this is their only
    /// chance to copy it.
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

    /// Return the token name if `raw_token` matches an active row, and
    /// update `last_used` on success. Returns `Ok(None)` for unknown or
    /// revoked tokens so the middleware can translate to `401` without
    /// distinguishing "bad token" from "no token" (avoids user
    /// enumeration).
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

    /// Headline stats for the CLI `stats` command.
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
    })
}

// Silence "unused import" on non-Unix platforms.
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

/// Generate a fresh `fyn_`-prefixed token. 32 random bytes rendered as
/// hex gives 256 bits of entropy, well above anything we need for a
/// local bearer token.
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

// Silence `HoldingType` unused warning if the holdings upsert is the only caller.
#[allow(dead_code)]
const _: Option<HoldingType> = None;
