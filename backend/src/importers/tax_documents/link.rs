//! Net-pay link suggestion (plan 29 decision 11): which bank credit is each
//! payslip's take-home pay.

use std::collections::HashSet;

use chrono::NaiveDate;

use super::validate::period_amount;
use crate::income::line_types::IncomeLineType;
use crate::model::{
    NetPayCandidate, TaxDocumentPreviewRow, TaxDocumentType, TransactionPreviewStatus,
};

pub const WINDOW_DAYS: i64 = 4;
pub const MAX_CANDIDATES: usize = 3;

/// The date a payslip's credit is expected: pay date, else period end.
pub fn expected_credit_date(row: &TaxDocumentPreviewRow) -> Option<NaiveDate> {
    row.draft
        .pay_date
        .as_deref()
        .or(row.draft.period_end.as_deref())
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
}

/// Suggest credits for new payslip rows that have none yet. A candidate has
/// the exact net amount in the payslip's currency, falls within
/// [`WINDOW_DAYS`] of the expected date and is not in `linked`. Each
/// credit goes to at most one row, nearest pairs first; the ids handed out
/// are added to `linked`. A row left with exactly one candidate has it
/// pre-filled as its `transaction_id`.
pub fn assign_net_pay_candidates(
    rows: &mut [TaxDocumentPreviewRow],
    candidates: &[NetPayCandidate],
    linked: &mut HashSet<String>,
) {
    let mut pairs: Vec<(i64, usize, usize)> = Vec::new();
    for (ri, row) in rows.iter().enumerate() {
        if row.status != TransactionPreviewStatus::New
            || row.draft.document_type != TaxDocumentType::Payslip
            || !row.net_pay_candidates.is_empty()
        {
            continue;
        }
        let (Some(net), Some(expected)) = (
            period_amount(&row.draft.line_items, IncomeLineType::NetPay),
            expected_credit_date(row),
        ) else {
            continue;
        };
        for (ci, c) in candidates.iter().enumerate() {
            let Ok(date) = NaiveDate::parse_from_str(&c.date, "%Y-%m-%d") else {
                continue;
            };
            let days = (date - expected).num_days();
            if c.amount == net
                && c.currency == row.draft.currency
                && days.abs() <= WINDOW_DAYS
                && !linked.contains(&c.transaction_id)
            {
                pairs.push((days.abs(), ri, ci));
            }
        }
    }
    pairs.sort_by(|a, b| {
        (a.0, a.1, &candidates[a.2].transaction_id).cmp(&(
            b.0,
            b.1,
            &candidates[b.2].transaction_id,
        ))
    });

    let mut taken: HashSet<usize> = HashSet::new();
    for (_, ri, ci) in pairs {
        if taken.contains(&ci) || rows[ri].net_pay_candidates.len() >= MAX_CANDIDATES {
            continue;
        }
        let Some(expected) = expected_credit_date(&rows[ri]) else {
            continue;
        };
        let mut candidate = candidates[ci].clone();
        if let Ok(date) = NaiveDate::parse_from_str(&candidate.date, "%Y-%m-%d") {
            candidate.days_from_pay_date = (date - expected).num_days();
        }
        taken.insert(ci);
        rows[ri].net_pay_candidates.push(candidate);
    }

    for row in rows.iter_mut() {
        if row.net_pay_candidates.len() == 1 && row.draft.transaction_id.is_none() {
            row.draft.transaction_id = Some(row.net_pay_candidates[0].transaction_id.clone());
        }
    }
    linked.extend(
        taken
            .into_iter()
            .map(|ci| candidates[ci].transaction_id.clone()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{IncomeBasis, TaxDocumentDraft, TaxLineItem};
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn row(
        index: usize,
        kind: TaxDocumentType,
        pay_date: &str,
        net: &str,
    ) -> TaxDocumentPreviewRow {
        TaxDocumentPreviewRow {
            index,
            status: TransactionPreviewStatus::New,
            error_reason: None,
            existing_id: None,
            same_key_as_row: None,
            row_confidence: 0.9,
            source_document_ids: vec![],
            warnings: vec![],
            net_pay_candidates: vec![],
            draft: TaxDocumentDraft {
                document_type: kind,
                source_name: "Example Widgets Ltd".to_string(),
                paye_reference: None,
                tax_year: "2025-26".to_string(),
                pay_date: Some(pay_date.to_string()),
                period_start: None,
                period_end: None,
                currency: "GBP".to_string(),
                line_items: vec![TaxLineItem {
                    line_type: IncomeLineType::NetPay,
                    label: None,
                    amount: dec(net),
                    basis: IncomeBasis::Period,
                }],
                transaction_id: None,
                document_ids: vec![],
                notes: None,
            },
        }
    }

    fn credit(id: &str, date: &str, amount: &str) -> NetPayCandidate {
        NetPayCandidate {
            transaction_id: id.to_string(),
            account_id: "acc".to_string(),
            account_name: "Main".to_string(),
            date: date.to_string(),
            amount: dec(amount),
            currency: "GBP".to_string(),
            description: "EXAMPLE WIDGETS SALARY".to_string(),
            days_from_pay_date: 0,
        }
    }

    fn assign(rows: &mut [TaxDocumentPreviewRow], credits: &[NetPayCandidate]) {
        assign_net_pay_candidates(rows, credits, &mut HashSet::new());
    }

    #[test]
    fn exact_amount_inside_the_window_is_matched_and_prefilled() {
        let mut rows = vec![row(0, TaxDocumentType::Payslip, "2025-06-27", "2450.00")];
        assign(&mut rows, &[credit("t1", "2025-06-26", "2450.00")]);
        assert_eq!(rows[0].net_pay_candidates.len(), 1);
        assert_eq!(rows[0].net_pay_candidates[0].days_from_pay_date, -1);
        assert_eq!(rows[0].draft.transaction_id.as_deref(), Some("t1"));
    }

    #[test]
    fn off_by_a_penny_or_outside_the_window_is_not_matched() {
        let mut rows = vec![row(0, TaxDocumentType::Payslip, "2025-06-27", "2450.00")];
        assign(
            &mut rows,
            &[
                credit("t1", "2025-06-27", "2450.01"),
                credit("t2", "2025-07-02", "2450.00"),
                credit("t3", "2025-06-27", "-2450.00"),
            ],
        );
        assert!(rows[0].net_pay_candidates.is_empty());
        assert_eq!(rows[0].draft.transaction_id, None);
    }

    #[test]
    fn window_edge_is_inclusive() {
        let mut rows = vec![row(0, TaxDocumentType::Payslip, "2025-06-27", "2450.00")];
        assign(&mut rows, &[credit("t1", "2025-07-01", "2450.00")]);
        assert_eq!(rows[0].net_pay_candidates[0].days_from_pay_date, 4);
    }

    #[test]
    fn equal_net_pay_payslips_get_one_credit_each_by_distance() {
        let mut rows = vec![
            row(0, TaxDocumentType::Payslip, "2025-06-27", "2450.00"),
            row(1, TaxDocumentType::Payslip, "2025-06-29", "2450.00"),
        ];
        assign(
            &mut rows,
            &[
                credit("early", "2025-06-27", "2450.00"),
                credit("late", "2025-06-29", "2450.00"),
            ],
        );
        assert_eq!(rows[0].draft.transaction_id.as_deref(), Some("early"));
        assert_eq!(rows[1].draft.transaction_id.as_deref(), Some("late"));
    }

    #[test]
    fn already_linked_credits_are_excluded() {
        let mut rows = vec![row(0, TaxDocumentType::Payslip, "2025-06-27", "2450.00")];
        let mut linked: HashSet<String> = ["t1".to_string()].into();
        assign_net_pay_candidates(
            &mut rows,
            &[credit("t1", "2025-06-27", "2450.00")],
            &mut linked,
        );
        assert!(rows[0].net_pay_candidates.is_empty());
    }

    #[test]
    fn several_candidates_are_listed_but_not_prefilled() {
        let mut rows = vec![row(0, TaxDocumentType::Payslip, "2025-06-27", "2450.00")];
        assign(
            &mut rows,
            &[
                credit("t1", "2025-06-26", "2450.00"),
                credit("t2", "2025-06-28", "2450.00"),
            ],
        );
        assert_eq!(rows[0].net_pay_candidates.len(), 2);
        assert_eq!(rows[0].draft.transaction_id, None);
    }

    #[test]
    fn p60_and_error_rows_get_no_candidates() {
        let mut rows = vec![
            row(
                0,
                TaxDocumentType::AnnualCertificate,
                "2025-06-27",
                "2450.00",
            ),
            row(1, TaxDocumentType::Payslip, "2025-06-27", "2450.00"),
        ];
        rows[1].status = TransactionPreviewStatus::Error;
        assign(&mut rows, &[credit("t1", "2025-06-27", "2450.00")]);
        assert!(rows.iter().all(|r| r.net_pay_candidates.is_empty()));
    }

    #[test]
    fn currency_must_match() {
        let mut rows = vec![row(0, TaxDocumentType::Payslip, "2025-06-27", "2450.00")];
        let mut c = credit("t1", "2025-06-27", "2450.00");
        c.currency = "USD".to_string();
        assign(&mut rows, &[c]);
        assert!(rows[0].net_pay_candidates.is_empty());
    }

    #[test]
    fn assigned_ids_join_the_linked_set() {
        let mut rows = vec![row(0, TaxDocumentType::Payslip, "2025-06-27", "2450.00")];
        let mut linked = HashSet::new();
        assign_net_pay_candidates(
            &mut rows,
            &[credit("t1", "2025-06-27", "2450.00")],
            &mut linked,
        );
        assert!(linked.contains("t1"));
    }
}
