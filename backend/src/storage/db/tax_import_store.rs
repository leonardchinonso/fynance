//! Reads and the commit transaction behind tax-document import (plan 30
//! section 9). Validation lives in `crate::income::validate`.

use std::collections::HashSet;

use anyhow::Result;
use chrono::NaiveDate;
use rusqlite::{OptionalExtension, params};
use rust_decimal::Decimal;

use super::{Db, parse_decimal_column};
use crate::model::NetPayCandidate;

impl Db {
    /// Positive-amount transactions dated within `from..=to`, optionally on
    /// the given accounts only. `days_from_pay_date` is left at 0 for the
    /// caller to fill.
    pub fn find_credit_transactions(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        account_ids: Option<&[String]>,
    ) -> Result<Vec<NetPayCandidate>> {
        let ids = account_ids.map(serde_json::to_string).transpose()?;
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.account_id, a.name, t.date, t.amount, t.currency, t.description
             FROM transactions t JOIN accounts a ON a.id = t.account_id
             WHERE t.date >= ?1 AND t.date <= ?2
               AND (?3 IS NULL OR t.account_id IN (SELECT value FROM json_each(?3)))
             ORDER BY t.date, t.id",
        )?;
        let raw = stmt
            .query_map(
                params![
                    from.format("%Y-%m-%dT00:00:00").to_string(),
                    to.format("%Y-%m-%dT23:59:59").to_string(),
                    ids
                ],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, String>(6)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::new();
        for (transaction_id, account_id, account_name, date, amount, currency, description) in raw {
            let amount = parse_decimal_column(4, "transactions.amount", &amount)?;
            if amount > Decimal::ZERO {
                out.push(NetPayCandidate {
                    transaction_id,
                    account_id,
                    account_name,
                    date: date.chars().take(10).collect(),
                    amount,
                    currency,
                    description,
                    days_from_pay_date: 0,
                });
            }
        }
        Ok(out)
    }

    /// Every transaction id linked from any income record.
    pub fn linked_income_transaction_ids(&self) -> Result<HashSet<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT j.value FROM income_records r, json_each(r.transaction_ids) j")?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<HashSet<_>>>()?;
        Ok(ids)
    }

    /// A `period` record of the source for this tax year ending on `period_end`.
    pub fn find_period_record_id(
        &self,
        source_id: &str,
        tax_year: &str,
        period_end: &str,
    ) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM income_records
                 WHERE source_id = ?1 AND tax_year = ?2 AND period_end = ?3 AND kind = 'period'
                 ORDER BY created_at, id LIMIT 1",
                params![source_id, tax_year, period_end],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Run `f` in one SQLite transaction: commit on `Ok`, roll back on `Err`.
    pub fn in_transaction<T, E: From<rusqlite::Error>>(
        &self,
        f: impl FnOnce(&Db) -> Result<T, E>,
    ) -> Result<T, E> {
        let tx = self.conn.unchecked_transaction()?;
        let out = f(self)?;
        tx.commit()?;
        Ok(out)
    }
}
