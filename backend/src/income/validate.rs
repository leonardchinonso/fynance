//! Validation shared by the income routes and by the import and sync writers.
//! Every failure is a 400 with a specific code.

use chrono::NaiveDate;
use rust_decimal::Decimal;

use crate::income::line_types::IncomeLineType;
use crate::model::{
    IncomeLineItemInput, IncomeRecordInsert, IncomeRecordKind, IncomeSource, IncomeSourceType,
    InvestmentEventType,
};
use crate::server::error::AppError;
use crate::storage::db::Db;
use crate::util::tax_year::TaxYear;

pub fn parse_iso_date(s: &str, field: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
        AppError::bad_request(
            format!("{field} must be a YYYY-MM-DD date, got {s:?}"),
            "invalid_date",
        )
    })
}

pub fn validate_currency(db: &Db, code: &str) -> Result<(), AppError> {
    if db.get_currencies()?.iter().any(|c| c.code == code) {
        Ok(())
    } else {
        Err(AppError::bad_request(
            format!("currency {code:?} is not a known currency"),
            "invalid_currency",
        ))
    }
}

/// The tax year a record belongs to, derived server-side: employment by pay
/// date (else period end), everything else by period end. Non-employment
/// records may not cross a tax year.
pub fn derive_tax_year(
    source_type: IncomeSourceType,
    kind: IncomeRecordKind,
    period_start: NaiveDate,
    period_end: NaiveDate,
    pay_date: Option<NaiveDate>,
) -> Result<String, AppError> {
    if period_end < period_start {
        return Err(AppError::bad_request(
            "period_end must not be before period_start",
            "invalid_period",
        ));
    }
    let anchor = match (source_type, kind) {
        (IncomeSourceType::Employment, IncomeRecordKind::Period) => pay_date.unwrap_or(period_end),
        _ => period_end,
    };
    let year = TaxYear::for_date(anchor);
    let crosses =
        source_type != IncomeSourceType::Employment || kind == IncomeRecordKind::YearEndAdjustment;
    if crosses && !year.contains(period_start) {
        return Err(AppError::bad_request(
            "a record's period must sit inside one tax year",
            "period_crosses_tax_year",
        ));
    }
    Ok(year.label())
}

fn validate_line_items(
    db: &Db,
    source: &IncomeSource,
    kind: IncomeRecordKind,
    items: &[IncomeLineItemInput],
) -> Result<(), AppError> {
    for item in items {
        if !item.line_type.valid_for(source.source_type) {
            return Err(AppError::bad_request(
                format!(
                    "line type {} is not valid for a {} source",
                    item.line_type.as_str(),
                    source.source_type.as_str()
                ),
                "invalid_line_type",
            ));
        }
        if item.amount < Decimal::ZERO && kind != IncomeRecordKind::YearEndAdjustment {
            return Err(AppError::bad_request(
                format!("amount must not be negative, got {}", item.amount),
                "negative_amount",
            ));
        }
        if let Some(c) = item.currency.as_deref() {
            validate_currency(db, c)?;
        }
        if let Some(event_id) = item.investment_event_id.as_deref() {
            if item.line_type != IncomeLineType::RsuVestValue {
                return Err(AppError::bad_request(
                    "investment_event_id is only valid on rsu_vest_value lines",
                    "invalid_investment_event",
                ));
            }
            match db.get_investment_event(event_id)? {
                Some(ev) if ev.event_type == InvestmentEventType::Vest => {}
                _ => {
                    return Err(AppError::bad_request(
                        format!("investment event {event_id} is not a vest event"),
                        "invalid_investment_event",
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Every rule a record must satisfy before it is stored. Returns the derived
/// tax year. `check_lines` is false when a PATCH leaves the line items alone.
pub fn validate_record(
    db: &Db,
    source: &IncomeSource,
    r: &IncomeRecordInsert,
    check_lines: bool,
) -> Result<String, AppError> {
    let start = parse_iso_date(&r.period_start, "period_start")?;
    let end = parse_iso_date(&r.period_end, "period_end")?;
    let pay = r
        .pay_date
        .as_deref()
        .map(|d| parse_iso_date(d, "pay_date"))
        .transpose()?;
    validate_currency(db, &r.currency)?;
    let tax_year = derive_tax_year(source.source_type, r.kind, start, end, pay)?;
    for tx in &r.transaction_ids {
        if !db.transaction_exists(tx)? {
            return Err(AppError::bad_request(
                format!("transaction {tx} not found"),
                "transaction_not_found",
            ));
        }
    }
    if check_lines {
        validate_line_items(db, source, r.kind, &r.line_items)?;
    }
    Ok(tax_year)
}
