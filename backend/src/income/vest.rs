//! RSU vest linking (plan 30): which `vest` investment events could a
//! payslip's vest line refer to.

use anyhow::Result;
use chrono::Duration;
use rust_decimal::Decimal;

use crate::income::line_types::IncomeLineType;
use crate::income::summary::VEST_TOLERANCE;
use crate::model::{
    IncomeBasis, IncomeRecord, IncomeSource, InvestmentEventType, VestCandidate, VestMatchStatus,
};
use crate::storage::db::Db;
use crate::util::fx::FxRateMap;

/// How far from the pay date (else the period end) a vest may fall.
const WINDOW_DAYS: i64 = 7;

/// `vest` events of the source's profile within a week of the record's pay
/// date, each marked `matched` when its GBP value is within a pound of the
/// record's vest lines. Empty when the record has no `rsu_vest_value` line.
pub fn vest_candidates(
    db: &Db,
    source: &IncomeSource,
    record: &IncomeRecord,
    fx: &FxRateMap,
) -> Result<Vec<VestCandidate>> {
    let anchor = record
        .pay_date
        .as_deref()
        .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .or_else(|| chrono::NaiveDate::parse_from_str(&record.period_end, "%Y-%m-%d").ok());
    let Some(anchor) = anchor else {
        return Ok(Vec::new());
    };
    let vest_lines: Vec<_> = record
        .line_items
        .iter()
        .filter(|l| l.line_type == IncomeLineType::RsuVestValue && l.basis == IncomeBasis::Period)
        .collect();
    if vest_lines.is_empty() {
        return Ok(Vec::new());
    }
    let line_value: Decimal = vest_lines
        .iter()
        .map(|l| fx.convert_as_of_stored(l.amount, &l.currency, anchor))
        .sum();

    let accounts = db.get_accounts(Some(&source.profile_id))?;
    let ids: Vec<String> = accounts.iter().map(|a| a.id.clone()).collect();
    let events = db.list_investment_events(None, None, Some("vest"), Some(&ids))?;
    let window = Duration::days(WINDOW_DAYS);
    let mut out = Vec::new();
    for ev in events {
        if ev.event_type != InvestmentEventType::Vest {
            continue;
        }
        let date = ev.date.date();
        if date < anchor - window || date > anchor + window {
            continue;
        }
        let value_gbp =
            fx.convert_as_of_stored(ev.quantity * ev.price_per_share, &ev.currency, date);
        let account_name = accounts
            .iter()
            .find(|a| a.id == ev.account_id)
            .map(|a| a.name.clone())
            .unwrap_or_default();
        out.push(VestCandidate {
            investment_event_id: ev.id,
            account_name,
            symbol: ev.symbol,
            date: date.format("%Y-%m-%d").to_string(),
            quantity: ev.quantity,
            price_per_share: ev.price_per_share,
            currency: ev.currency,
            value_gbp,
            status: if (value_gbp - line_value).abs() <= VEST_TOLERANCE {
                VestMatchStatus::Matched
            } else {
                VestMatchStatus::Mismatch
            },
        });
    }
    Ok(out)
}
