//! Storage for the income model (plan 30): sources, records with their line
//! items, and checks. Validation lives in `crate::income`; these methods trust
//! their inputs and only do the SQL.

use anyhow::{Result, anyhow};
use chrono::NaiveDate;
use rusqlite::{OptionalExtension, params};
use rust_decimal::Decimal;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::{Db, column_error, parse_decimal_column, parse_id_array};
use crate::income::line_types::IncomeLineType;
use crate::model::{
    IncomeCheck, IncomeCheckFigure, IncomeCheckType, IncomeLineItem, IncomeLineItemInput,
    IncomeRecord, IncomeRecordInsert, IncomeRecordKind, IncomeSource, IncomeSourcePayload,
    InvestmentEvent, PatchIncomeSourceBody,
};

/// Money legs of the rows `Db::income_period_rows` returns, kept as Decimal.
#[derive(Debug, Clone)]
pub struct BankOutflow {
    pub category_id: Option<String>,
    pub category_name: Option<String>,
    pub currency: String,
    pub amount: Decimal,
}

/// One interest or donation leg on an account.
#[derive(Debug, Clone)]
pub struct AccountLeg {
    pub account_id: String,
    pub account_name: String,
    pub currency: String,
    pub category_type: String,
    pub amount: Decimal,
    /// Number of profiles that hold the account.
    pub profile_count: u32,
}

fn now_stamp() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// The snake_case wire name of a serde enum, as stored in TEXT columns.
fn enum_to_str<T: Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}

fn enum_from_str<T: DeserializeOwned>(idx: usize, field: &str, s: &str) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(s.to_string()))
        .map_err(|_| column_error(idx, format!("invalid {field}: {s:?}")))
}

fn json_col<T: DeserializeOwned>(idx: usize, field: &str, s: &str) -> rusqlite::Result<T> {
    serde_json::from_str(s).map_err(|e| column_error(idx, format!("invalid {field}: {e}")))
}

const SOURCE_COLS: &str = "id, profile_id, source_type, name, identifiers, account_ids, \
     expense_category_map, sync_kind, active, notes, created_at, updated_at";

fn row_to_income_source(row: &rusqlite::Row<'_>) -> rusqlite::Result<IncomeSource> {
    let source_type: String = row.get(2)?;
    let identifiers: String = row.get(4)?;
    let account_ids: String = row.get(5)?;
    let category_map: String = row.get(6)?;
    Ok(IncomeSource {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        source_type: enum_from_str(2, "income_sources.source_type", &source_type)?,
        name: row.get(3)?,
        identifiers: json_col(4, "income_sources.identifiers", &identifiers)?,
        account_ids: parse_id_array(&account_ids),
        expense_category_map: json_col(6, "income_sources.expense_category_map", &category_map)?,
        sync_kind: row.get(7)?,
        active: row.get::<_, i64>(8)? != 0,
        notes: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

const RECORD_COLS: &str = "r.id, r.source_id, r.kind, r.period_start, r.period_end, r.pay_date, \
     r.tax_year, r.currency, r.transaction_ids, r.document_ids, r.origin, r.external_ref, \
     r.notes, r.created_at, r.updated_at";

fn row_to_income_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<IncomeRecord> {
    let kind: String = row.get(2)?;
    let origin: String = row.get(10)?;
    let tx_ids: String = row.get(8)?;
    let doc_ids: String = row.get(9)?;
    Ok(IncomeRecord {
        id: row.get(0)?,
        source_id: row.get(1)?,
        kind: enum_from_str(2, "income_records.kind", &kind)?,
        period_start: row.get(3)?,
        period_end: row.get(4)?,
        pay_date: row.get(5)?,
        tax_year: row.get(6)?,
        currency: row.get(7)?,
        transaction_ids: parse_id_array(&tx_ids),
        document_ids: parse_id_array(&doc_ids),
        origin: enum_from_str(10, "income_records.origin", &origin)?,
        external_ref: row.get(11)?,
        notes: row.get(12)?,
        line_items: Vec::new(),
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

fn row_to_line_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<IncomeLineItem> {
    let line_type: String = row.get(1)?;
    let amount: String = row.get(2)?;
    let basis: String = row.get(4)?;
    Ok(IncomeLineItem {
        id: row.get(0)?,
        line_type: enum_from_str::<IncomeLineType>(1, "income_line_items.line_type", &line_type)?,
        amount: parse_decimal_column(2, "income_line_items.amount", &amount)?,
        currency: row.get(3)?,
        basis: enum_from_str(4, "income_line_items.basis", &basis)?,
        label: row.get(5)?,
        investment_event_id: row.get(6)?,
    })
}

const CHECK_COLS: &str = "id, source_id, tax_year, check_type, figures, document_ids, notes, \
     created_at, updated_at";

fn row_to_income_check(row: &rusqlite::Row<'_>) -> rusqlite::Result<IncomeCheck> {
    let check_type: String = row.get(3)?;
    let figures: String = row.get(4)?;
    let doc_ids: String = row.get(5)?;
    let check_type = IncomeCheckType::parse(&check_type).ok_or_else(|| {
        column_error(
            3,
            format!("invalid income_checks.check_type: {check_type:?}"),
        )
    })?;
    Ok(IncomeCheck {
        id: row.get(0)?,
        source_id: row.get(1)?,
        tax_year: row.get(2)?,
        check_type,
        figures: json_col::<Vec<IncomeCheckFigure>>(4, "income_checks.figures", &figures)?,
        document_ids: parse_id_array(&doc_ids),
        notes: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

impl Db {
    // ── Sources ──────────────────────────────────────────────────────────────

    pub fn create_income_source(&self, p: &IncomeSourcePayload) -> Result<IncomeSource> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_stamp();
        self.conn.execute(
            "INSERT INTO income_sources (id, profile_id, source_type, name, identifiers,
                account_ids, expense_category_map, sync_kind, active, notes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
            params![
                id,
                p.profile_id,
                p.source_type.as_str(),
                p.name,
                serde_json::to_string(&p.identifiers)?,
                serde_json::to_string(&p.account_ids)?,
                serde_json::to_string(&p.expense_category_map)?,
                p.sync_kind,
                p.active as i64,
                p.notes,
                now,
            ],
        )?;
        self.get_income_source(&id)?
            .ok_or_else(|| anyhow!("income_sources row {id} vanished after insert"))
    }

    pub fn get_income_source(&self, id: &str) -> Result<Option<IncomeSource>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {SOURCE_COLS} FROM income_sources WHERE id = ?1"),
                params![id],
                row_to_income_source,
            )
            .optional()?)
    }

    pub fn list_income_sources(&self, profile_id: Option<&str>) -> Result<Vec<IncomeSource>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SOURCE_COLS} FROM income_sources
             WHERE (?1 IS NULL OR profile_id = ?1) ORDER BY name, id"
        ))?;
        let rows = stmt
            .query_map(params![profile_id], row_to_income_source)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn update_income_source(
        &self,
        id: &str,
        body: &PatchIncomeSourceBody,
    ) -> Result<Option<IncomeSource>> {
        let Some(mut s) = self.get_income_source(id)? else {
            return Ok(None);
        };
        if let Some(v) = &body.name {
            s.name = v.clone();
        }
        if let Some(v) = &body.identifiers {
            s.identifiers = v.clone();
        }
        if let Some(v) = &body.account_ids {
            s.account_ids = v.clone();
        }
        if let Some(v) = &body.expense_category_map {
            s.expense_category_map = v.clone();
        }
        if let Some(v) = &body.sync_kind {
            s.sync_kind = v.clone();
        }
        if let Some(v) = body.active {
            s.active = v;
        }
        if let Some(v) = &body.notes {
            s.notes = v.clone();
        }
        self.conn.execute(
            "UPDATE income_sources SET name = ?2, identifiers = ?3, account_ids = ?4,
                expense_category_map = ?5, sync_kind = ?6, active = ?7, notes = ?8,
                updated_at = ?9
             WHERE id = ?1",
            params![
                id,
                s.name,
                serde_json::to_string(&s.identifiers)?,
                serde_json::to_string(&s.account_ids)?,
                serde_json::to_string(&s.expense_category_map)?,
                s.sync_kind,
                s.active as i64,
                s.notes,
                now_stamp(),
            ],
        )?;
        self.get_income_source(id)
    }

    pub fn count_income_records_for_source(&self, source_id: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM income_records WHERE source_id = ?1",
            params![source_id],
            |r| r.get(0),
        )?)
    }

    pub fn count_income_sources_for_profile(&self, profile_id: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM income_sources WHERE profile_id = ?1",
            params![profile_id],
            |r| r.get(0),
        )?)
    }

    /// Delete a source and its checks. The caller has already refused when the
    /// source still has records.
    pub fn delete_income_source(&self, id: &str) -> Result<bool> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM income_checks WHERE source_id = ?1",
            params![id],
        )?;
        let n = tx.execute("DELETE FROM income_sources WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(n > 0)
    }

    // ── Records ──────────────────────────────────────────────────────────────

    fn insert_line_items(
        &self,
        record_id: &str,
        record_currency: &str,
        items: &[IncomeLineItemInput],
    ) -> Result<()> {
        for item in items {
            self.conn.execute(
                "INSERT INTO income_line_items
                    (id, record_id, line_type, amount, currency, basis, label, investment_event_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    record_id,
                    item.line_type.as_str(),
                    item.amount.to_string(),
                    item.currency.as_deref().unwrap_or(record_currency),
                    enum_to_str(&item.basis),
                    item.label,
                    item.investment_event_id,
                ],
            )?;
        }
        Ok(())
    }

    fn load_line_items(&self, record_id: &str) -> Result<Vec<IncomeLineItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, line_type, amount, currency, basis, label, investment_event_id
             FROM income_line_items WHERE record_id = ?1 ORDER BY rowid",
        )?;
        let rows = stmt
            .query_map(params![record_id], row_to_line_item)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Insert one record and its line items in a single transaction.
    /// `tax_year` is derived by the caller, never taken from the client.
    pub fn insert_income_record(
        &self,
        r: &IncomeRecordInsert,
        tax_year: &str,
    ) -> Result<IncomeRecord> {
        let tx = self.conn.unchecked_transaction()?;
        let id = self.insert_income_record_inner(r, tax_year)?;
        tx.commit()?;
        self.get_income_record(&id)?
            .ok_or_else(|| anyhow!("income_records row {id} vanished after insert"))
    }

    /// Same as `insert_income_record` but joins the caller's transaction, for
    /// the import and sync writers that insert many records atomically.
    pub fn insert_income_record_inner(
        &self,
        r: &IncomeRecordInsert,
        tax_year: &str,
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_stamp();
        self.conn.execute(
            "INSERT INTO income_records (id, source_id, kind, period_start, period_end, pay_date,
                tax_year, currency, transaction_ids, document_ids, origin, external_ref, notes,
                created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)",
            params![
                id,
                r.source_id,
                enum_to_str(&r.kind),
                r.period_start,
                r.period_end,
                r.pay_date,
                tax_year,
                r.currency,
                serde_json::to_string(&r.transaction_ids)?,
                serde_json::to_string(&r.document_ids)?,
                enum_to_str(&r.origin),
                r.external_ref,
                r.notes,
                now,
            ],
        )?;
        self.insert_line_items(&id, &r.currency, &r.line_items)?;
        Ok(id)
    }

    pub fn get_income_record(&self, id: &str) -> Result<Option<IncomeRecord>> {
        let rec = self
            .conn
            .query_row(
                &format!("SELECT {RECORD_COLS} FROM income_records r WHERE r.id = ?1"),
                params![id],
                row_to_income_record,
            )
            .optional()?;
        match rec {
            Some(mut rec) => {
                rec.line_items = self.load_line_items(&rec.id)?;
                Ok(Some(rec))
            }
            None => Ok(None),
        }
    }

    /// Records with line items embedded, ordered by pay date then period end.
    /// Every filter is optional.
    pub fn list_income_records(
        &self,
        profile_id: Option<&str>,
        source_id: Option<&str>,
        tax_year: Option<&str>,
    ) -> Result<Vec<IncomeRecord>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {RECORD_COLS} FROM income_records r
             JOIN income_sources s ON s.id = r.source_id
             WHERE (?1 IS NULL OR s.profile_id = ?1)
               AND (?2 IS NULL OR r.source_id = ?2)
               AND (?3 IS NULL OR r.tax_year = ?3)
             ORDER BY COALESCE(r.pay_date, r.period_end), r.period_end, r.id"
        ))?;
        let mut rows = stmt
            .query_map(
                params![profile_id, source_id, tax_year],
                row_to_income_record,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for rec in &mut rows {
            rec.line_items = self.load_line_items(&rec.id)?;
        }
        Ok(rows)
    }

    /// Overwrite a record's columns; `line_items` replaces every line item when
    /// `Some`, and leaves them alone when `None`.
    pub fn update_income_record(
        &self,
        id: &str,
        r: &IncomeRecordInsert,
        tax_year: &str,
        line_items: Option<&[IncomeLineItemInput]>,
    ) -> Result<Option<IncomeRecord>> {
        let tx = self.conn.unchecked_transaction()?;
        let n = self.conn.execute(
            "UPDATE income_records SET period_start = ?2, period_end = ?3, pay_date = ?4,
                tax_year = ?5, currency = ?6, transaction_ids = ?7, document_ids = ?8,
                notes = ?9, updated_at = ?10
             WHERE id = ?1",
            params![
                id,
                r.period_start,
                r.period_end,
                r.pay_date,
                tax_year,
                r.currency,
                serde_json::to_string(&r.transaction_ids)?,
                serde_json::to_string(&r.document_ids)?,
                r.notes,
                now_stamp(),
            ],
        )?;
        if n == 0 {
            return Ok(None);
        }
        if let Some(items) = line_items {
            self.conn.execute(
                "DELETE FROM income_line_items WHERE record_id = ?1",
                params![id],
            )?;
            self.insert_line_items(id, &r.currency, items)?;
        }
        tx.commit()?;
        self.get_income_record(id)
    }

    /// Replace the line items of a record, keeping its other columns. Used by
    /// the sync upsert, which rewrites synced records in place.
    pub fn replace_income_line_items(
        &self,
        record_id: &str,
        currency: &str,
        items: &[IncomeLineItemInput],
    ) -> Result<()> {
        self.conn.execute(
            "DELETE FROM income_line_items WHERE record_id = ?1",
            params![record_id],
        )?;
        self.insert_line_items(record_id, currency, items)?;
        self.conn.execute(
            "UPDATE income_records SET updated_at = ?2 WHERE id = ?1",
            params![record_id, now_stamp()],
        )?;
        Ok(())
    }

    /// The record of a source with this `(origin, external_ref)`, if any.
    pub fn find_income_record_by_ref(
        &self,
        source_id: &str,
        origin: &str,
        external_ref: &str,
    ) -> Result<Option<IncomeRecord>> {
        let id: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM income_records
                 WHERE source_id = ?1 AND origin = ?2 AND external_ref = ?3",
                params![source_id, origin, external_ref],
                |r| r.get(0),
            )
            .optional()?;
        match id {
            Some(id) => self.get_income_record(&id),
            None => Ok(None),
        }
    }

    /// Line items cascade through the foreign key.
    pub fn delete_income_record(&self, id: &str) -> Result<bool> {
        let n = self
            .conn
            .execute("DELETE FROM income_records WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }

    /// Whether a source already has a `year_end_adjustment` record for a tax year.
    pub fn year_end_adjustment_exists(&self, source_id: &str, tax_year: &str) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM income_records
             WHERE source_id = ?1 AND tax_year = ?2 AND kind = ?3",
            params![
                source_id,
                tax_year,
                enum_to_str(&IncomeRecordKind::YearEndAdjustment)
            ],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    // ── Checks ───────────────────────────────────────────────────────────────

    pub fn create_income_check(
        &self,
        source_id: &str,
        tax_year: &str,
        check_type: IncomeCheckType,
        figures: &[IncomeCheckFigure],
        document_ids: &[String],
        notes: Option<&str>,
    ) -> Result<IncomeCheck> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_stamp();
        self.conn.execute(
            "INSERT INTO income_checks (id, source_id, tax_year, check_type, figures,
                document_ids, notes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![
                id,
                source_id,
                tax_year,
                check_type.as_str(),
                serde_json::to_string(figures)?,
                serde_json::to_string(document_ids)?,
                notes,
                now,
            ],
        )?;
        self.get_income_check(&id)?
            .ok_or_else(|| anyhow!("income_checks row {id} vanished after insert"))
    }

    pub fn get_income_check(&self, id: &str) -> Result<Option<IncomeCheck>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {CHECK_COLS} FROM income_checks WHERE id = ?1"),
                params![id],
                row_to_income_check,
            )
            .optional()?)
    }

    pub fn find_income_check(
        &self,
        source_id: &str,
        tax_year: &str,
        check_type: IncomeCheckType,
    ) -> Result<Option<IncomeCheck>> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {CHECK_COLS} FROM income_checks
                     WHERE source_id = ?1 AND tax_year = ?2 AND check_type = ?3"
                ),
                params![source_id, tax_year, check_type.as_str()],
                row_to_income_check,
            )
            .optional()?)
    }

    pub fn list_income_checks(
        &self,
        profile_id: Option<&str>,
        tax_year: Option<&str>,
    ) -> Result<Vec<IncomeCheck>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.id, c.source_id, c.tax_year, c.check_type, c.figures, c.document_ids,
                    c.notes, c.created_at, c.updated_at
             FROM income_checks c JOIN income_sources s ON s.id = c.source_id
             WHERE (?1 IS NULL OR s.profile_id = ?1) AND (?2 IS NULL OR c.tax_year = ?2)
             ORDER BY c.tax_year, c.check_type, c.id",
        )?;
        let rows = stmt
            .query_map(params![profile_id, tax_year], row_to_income_check)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn update_income_check(
        &self,
        id: &str,
        figures: &[IncomeCheckFigure],
        document_ids: &[String],
        notes: Option<&str>,
    ) -> Result<Option<IncomeCheck>> {
        let n = self.conn.execute(
            "UPDATE income_checks SET figures = ?2, document_ids = ?3, notes = ?4, updated_at = ?5
             WHERE id = ?1",
            params![
                id,
                serde_json::to_string(figures)?,
                serde_json::to_string(document_ids)?,
                notes,
                now_stamp(),
            ],
        )?;
        if n == 0 {
            return Ok(None);
        }
        self.get_income_check(id)
    }

    pub fn delete_income_check(&self, id: &str) -> Result<bool> {
        let n = self
            .conn
            .execute("DELETE FROM income_checks WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }

    // ── Reads the summary and validation need ────────────────────────────────

    pub fn get_investment_event(&self, id: &str) -> Result<Option<InvestmentEvent>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, account_id, event_type, symbol, date, quantity, price_per_share, fee,
                        currency, notes, fingerprint, created_at, source_document_ids, fee_currency
                 FROM investments WHERE id = ?1",
                params![id],
                super::row_to_investment_event,
            )
            .optional()?)
    }

    /// Outflows (`amount < 0`, absolute value) on `account_ids` in the window,
    /// excluding summary-excluded rows and internal transfers. One row per
    /// transaction; the caller sums as Decimal.
    pub fn bank_outflows(
        &self,
        account_ids: &[String],
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<BankOutflow>> {
        if account_ids.is_empty() {
            return Ok(Vec::new());
        }
        let start_str = start.format("%Y-%m-%dT00:00:00").to_string();
        let end_str = end.format("%Y-%m-%dT23:59:59").to_string();
        let mut stmt = self.conn.prepare(
            "SELECT t.category_id, c.name, t.currency, t.amount
             FROM transactions t
             LEFT JOIN categories c ON c.id = t.category_id
             WHERE t.date >= ?1 AND t.date <= ?2
               AND t.exclude_from_summary = 0
               AND (c.category_type IS NULL OR c.category_type <> 'internal_transfer')
               AND t.account_id IN (SELECT value FROM json_each(?3))",
        )?;
        let ids = serde_json::to_string(account_ids)?;
        let raw = stmt
            .query_map(params![start_str, end_str, ids], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::new();
        for (category_id, category_name, currency, amount) in raw {
            let amount = parse_decimal_column(3, "transactions.amount", &amount)?;
            if amount < Decimal::ZERO {
                out.push(BankOutflow {
                    category_id,
                    category_name,
                    currency,
                    amount: amount.abs(),
                });
            }
        }
        Ok(out)
    }

    /// Interest legs (signed amounts) and donation legs (outflow magnitudes) on
    /// the accounts of one profile, per transaction, so the caller sums as
    /// Decimal and splits joint accounts.
    pub fn account_legs_for_profile(
        &self,
        profile_id: &str,
        start: NaiveDate,
        end: NaiveDate,
        category_types: &[&str],
    ) -> Result<Vec<AccountLeg>> {
        let start_str = start.format("%Y-%m-%dT00:00:00").to_string();
        let end_str = end.format("%Y-%m-%dT23:59:59").to_string();
        let types = serde_json::to_string(category_types)?;
        let mut stmt = self.conn.prepare(
            "SELECT t.account_id, a.name, t.currency, c.category_type, t.amount,
                    (SELECT COUNT(*) FROM json_each(a.profile_ids))
             FROM transactions t
             JOIN categories c ON c.id = t.category_id
             JOIN accounts a ON a.id = t.account_id
             WHERE t.date >= ?1 AND t.date <= ?2
               AND t.exclude_from_summary = 0
               AND c.category_type IN (SELECT value FROM json_each(?3))
               AND EXISTS (SELECT 1 FROM json_each(a.profile_ids) j WHERE j.value = ?4)
             ORDER BY a.name, t.account_id, t.currency",
        )?;
        let raw = stmt
            .query_map(params![start_str, end_str, types, profile_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::new();
        for (account_id, account_name, currency, category_type, amount, count) in raw {
            out.push(AccountLeg {
                account_id,
                account_name,
                currency,
                category_type,
                amount: parse_decimal_column(4, "transactions.amount", &amount)?,
                profile_count: count.max(1) as u32,
            });
        }
        Ok(out)
    }
}
