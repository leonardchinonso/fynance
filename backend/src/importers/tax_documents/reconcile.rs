//! Payslips against the P60, per employer and tax year (plan 29 decision 7).
//! A P60's figures never enter the payslip totals.

use rust_decimal::Decimal;

use super::normalise_source_name;
use super::validate::{amount_with_basis, period_amount};
use crate::income::line_types::IncomeLineType;
use crate::model::{
    IncomeBasis, TaxDocumentPreviewRow, TaxDocumentType, TaxReconciliation,
    TransactionPreviewStatus,
};

/// The pay figure that reconciles: taxable pay when printed, else gross pay.
fn pay_of(row: &TaxDocumentPreviewRow, basis: IncomeBasis) -> Option<Decimal> {
    let items = &row.draft.line_items;
    amount_with_basis(items, IncomeLineType::TaxablePay, basis)
        .or_else(|| amount_with_basis(items, IncomeLineType::GrossPay, basis))
}

fn date_of(row: &TaxDocumentPreviewRow) -> Option<&str> {
    row.draft
        .pay_date
        .as_deref()
        .or(row.draft.period_end.as_deref())
}

/// Normalised employer name, tax year, currency.
type GroupKey = (String, String, String);

pub fn reconcile(rows: &[TaxDocumentPreviewRow]) -> Vec<TaxReconciliation> {
    let mut groups: Vec<(GroupKey, Vec<&TaxDocumentPreviewRow>)> = Vec::new();
    for row in rows
        .iter()
        .filter(|r| r.status != TransactionPreviewStatus::Error && r.same_key_as_row.is_none())
    {
        let key = (
            normalise_source_name(&row.draft.source_name),
            row.draft.tax_year.clone(),
            row.draft.currency.clone(),
        );
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, members)) => members.push(row),
            None => groups.push((key, vec![row])),
        }
    }
    groups
        .into_iter()
        .map(|(_, members)| reconcile_group(&members))
        .collect()
}

fn reconcile_group(members: &[&TaxDocumentPreviewRow]) -> TaxReconciliation {
    let first = members[0];
    let certificate = members
        .iter()
        .find(|r| r.draft.document_type == TaxDocumentType::AnnualCertificate);
    let mut payslips: Vec<&&TaxDocumentPreviewRow> = members
        .iter()
        .filter(|r| r.draft.document_type == TaxDocumentType::Payslip)
        .collect();
    payslips.sort_by_key(|r| (date_of(r).map(str::to_string), r.index));

    let payslips_pay_total: Decimal = payslips
        .iter()
        .filter_map(|r| pay_of(r, IncomeBasis::Period))
        .sum();
    let payslips_tax_total: Decimal = payslips
        .iter()
        .filter_map(|r| period_amount(&r.draft.line_items, IncomeLineType::PayeTax))
        .sum();
    let latest = payslips.last();
    let latest_ytd_pay = latest.and_then(|r| pay_of(r, IncomeBasis::YearToDate));
    let latest_ytd_tax = latest.and_then(|r| {
        amount_with_basis(
            &r.draft.line_items,
            IncomeLineType::PayeTax,
            IncomeBasis::YearToDate,
        )
    });
    let certificate_pay = certificate.and_then(|r| pay_of(r, IncomeBasis::Period));
    let certificate_tax =
        certificate.and_then(|r| period_amount(&r.draft.line_items, IncomeLineType::PayeTax));

    let has_payslips = !payslips.is_empty();
    let pay_difference = certificate_pay
        .filter(|_| has_payslips)
        .map(|p| p - payslips_pay_total);
    let tax_difference = certificate_tax
        .filter(|_| has_payslips)
        .map(|t| t - payslips_tax_total);
    let mut months_covered: Vec<String> = payslips
        .iter()
        .filter_map(|r| date_of(r).map(|d| d.chars().take(7).collect()))
        .collect();
    months_covered.sort();
    months_covered.dedup();

    let mut rec = TaxReconciliation {
        source_name: first.draft.source_name.clone(),
        tax_year: first.draft.tax_year.clone(),
        currency: first.draft.currency.clone(),
        certificate_row: certificate.map(|r| r.index),
        payslip_rows: payslips.iter().map(|r| r.index).collect(),
        payslips_pay_total,
        payslips_tax_total,
        certificate_pay,
        certificate_tax,
        latest_ytd_pay,
        latest_ytd_tax,
        pay_difference,
        tax_difference,
        months_covered,
        message: String::new(),
    };
    rec.message = message(&rec);
    rec
}

fn message(rec: &TaxReconciliation) -> String {
    let who = format!("{} {}", rec.source_name, rec.tax_year);
    let months = rec.months_covered.len();
    match (rec.certificate_row.is_some(), rec.payslip_rows.is_empty()) {
        (true, false) => {
            let pay = rec.pay_difference.unwrap_or_default();
            let tax = rec.tax_difference.unwrap_or_default();
            if pay.is_zero() && tax.is_zero() {
                format!(
                    "{who}: {months} month(s) of payslips add up to the P60 (pay {}, tax {}).",
                    rec.payslips_pay_total, rec.payslips_tax_total
                )
            } else {
                format!(
                    "{who}: {months} month(s) of payslips differ from the P60 by {pay} pay and {tax} tax. \
                     A positive figure means the P60 is higher; months may be missing.",
                )
            }
        }
        (true, true) => format!(
            "{who}: P60 only (pay {}, tax {}); no payslips to compare.",
            rec.certificate_pay.unwrap_or_default(),
            rec.certificate_tax.unwrap_or_default()
        ),
        (false, _) => match (rec.latest_ytd_pay, rec.latest_ytd_tax) {
            (Some(pay), Some(tax)) => format!(
                "{who}: {months} month(s) of payslips (pay {}, tax {}); the latest payslip shows year to date pay {pay} and tax {tax}. No P60 to compare.",
                rec.payslips_pay_total, rec.payslips_tax_total
            ),
            _ => format!(
                "{who}: {months} month(s) of payslips (pay {}, tax {}). No P60 to compare.",
                rec.payslips_pay_total, rec.payslips_tax_total
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{TaxDocumentDraft, TaxLineItem};
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn item(line_type: IncomeLineType, amount: &str, basis: IncomeBasis) -> TaxLineItem {
        TaxLineItem {
            line_type,
            label: None,
            amount: dec(amount),
            basis,
        }
    }

    fn row(
        index: usize,
        kind: TaxDocumentType,
        date: Option<&str>,
        items: Vec<TaxLineItem>,
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
                pay_date: date.map(str::to_string),
                period_start: None,
                period_end: None,
                currency: "GBP".to_string(),
                line_items: items,
                transaction_id: None,
                document_ids: vec![],
                notes: None,
            },
        }
    }

    fn payslip(index: usize, date: &str, ytd_factor: i64) -> TaxDocumentPreviewRow {
        row(
            index,
            TaxDocumentType::Payslip,
            Some(date),
            vec![
                item(IncomeLineType::TaxablePay, "3000", IncomeBasis::Period),
                item(IncomeLineType::PayeTax, "400", IncomeBasis::Period),
                item(
                    IncomeLineType::TaxablePay,
                    &(3000 * ytd_factor).to_string(),
                    IncomeBasis::YearToDate,
                ),
                item(
                    IncomeLineType::PayeTax,
                    &(400 * ytd_factor).to_string(),
                    IncomeBasis::YearToDate,
                ),
            ],
        )
    }

    fn certificate(index: usize, pay: &str, tax: &str) -> TaxDocumentPreviewRow {
        row(
            index,
            TaxDocumentType::AnnualCertificate,
            None,
            vec![
                item(IncomeLineType::TaxablePay, pay, IncomeBasis::Period),
                item(IncomeLineType::PayeTax, tax, IncomeBasis::Period),
            ],
        )
    }

    fn months() -> [&'static str; 12] {
        [
            "2025-04-25",
            "2025-05-30",
            "2025-06-27",
            "2025-07-25",
            "2025-08-29",
            "2025-09-26",
            "2025-10-31",
            "2025-11-28",
            "2025-12-19",
            "2026-01-30",
            "2026-02-27",
            "2026-03-27",
        ]
    }

    #[test]
    fn p60_and_twelve_matching_payslips_give_no_difference() {
        let mut rows: Vec<_> = months()
            .iter()
            .enumerate()
            .map(|(i, d)| payslip(i, d, i as i64 + 1))
            .collect();
        rows.push(certificate(12, "36000", "4800"));
        let rec = &reconcile(&rows)[0];
        assert_eq!(rec.pay_difference, Some(Decimal::ZERO));
        assert_eq!(rec.tax_difference, Some(Decimal::ZERO));
        assert_eq!(rec.payslips_pay_total, dec("36000"));
        assert_eq!(rec.certificate_row, Some(12));
        assert_eq!(rec.months_covered.len(), 12);
        assert!(rec.message.contains("add up to the P60"));
    }

    #[test]
    fn a_missing_month_shows_a_difference_and_the_gap() {
        let mut rows: Vec<_> = months()
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 5)
            .map(|(i, d)| payslip(i, d, i as i64 + 1))
            .collect();
        rows.push(certificate(12, "36000", "4800"));
        let rec = &reconcile(&rows)[0];
        assert_eq!(rec.pay_difference, Some(dec("3000")));
        assert_eq!(rec.tax_difference, Some(dec("400")));
        assert_eq!(rec.months_covered.len(), 11);
        assert!(!rec.months_covered.contains(&"2025-09".to_string()));
        assert!(rec.message.contains("differ from the P60"));
    }

    #[test]
    fn payslips_only_report_the_latest_year_to_date() {
        let rows = vec![
            payslip(0, "2025-04-25", 1),
            payslip(1, "2025-05-30", 2),
            payslip(2, "2025-06-27", 3),
        ];
        let rec = &reconcile(&rows)[0];
        assert_eq!(rec.certificate_row, None);
        assert_eq!(rec.certificate_pay, None);
        assert_eq!(rec.pay_difference, None);
        assert_eq!(rec.payslips_pay_total, dec("9000"));
        assert_eq!(rec.latest_ytd_pay, Some(dec("9000")));
        assert_eq!(rec.latest_ytd_tax, Some(dec("1200")));
    }

    #[test]
    fn p60_only_has_no_payslip_totals() {
        let rows = vec![certificate(0, "36000", "4800")];
        let rec = &reconcile(&rows)[0];
        assert_eq!(rec.payslips_pay_total, Decimal::ZERO);
        assert_eq!(rec.payslips_tax_total, Decimal::ZERO);
        assert_eq!(rec.certificate_pay, Some(dec("36000")));
        assert_eq!(rec.pay_difference, None);
        assert!(rec.payslip_rows.is_empty());
    }

    #[test]
    fn p60_figures_never_enter_the_payslip_totals() {
        let rows = vec![payslip(0, "2025-04-25", 1), certificate(1, "36000", "4800")];
        let rec = &reconcile(&rows)[0];
        assert_eq!(rec.payslips_pay_total, dec("3000"));
        assert_eq!(rec.payslips_tax_total, dec("400"));
    }

    #[test]
    fn error_rows_and_repeats_are_left_out_and_groups_split_by_year() {
        let mut bad = payslip(1, "2025-05-30", 2);
        bad.status = TransactionPreviewStatus::Error;
        let mut repeat = payslip(2, "2025-04-25", 1);
        repeat.same_key_as_row = Some(0);
        let mut other_year = payslip(3, "2026-04-30", 1);
        other_year.draft.tax_year = "2026-27".to_string();
        let rows = vec![payslip(0, "2025-04-25", 1), bad, repeat, other_year];
        let recs = reconcile(&rows);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].payslip_rows, vec![0]);
        assert_eq!(recs[1].tax_year, "2026-27");
    }
}
