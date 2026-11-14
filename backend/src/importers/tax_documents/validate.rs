//! Raw tool output to preview rows: cleaning, tax-year derivation and the
//! warnings and error rows of plan 29 (decisions 6, 8 and 9).

use std::collections::HashMap;
use std::str::FromStr;

use chrono::NaiveDate;
use rust_decimal::Decimal;

use super::parser::ParsedTaxDocument;
use crate::income::line_types::IncomeLineType;
use crate::model::{
    IncomeBasis, IncomeSourceType, TaxDocumentDraft, TaxDocumentPreviewRow, TaxDocumentType,
    TaxLineItem, TransactionPreviewStatus,
};
use crate::util::tax_year::TaxYear;

const ARITHMETIC_TOLERANCE: Decimal = Decimal::from_parts(1, 0, 0, false, 2);

/// Where a row's source documents come from: the upload each element was read
/// from, falling back to every upload when the filename is unknown.
pub struct SourceDocuments<'a> {
    pub ids_by_filename: &'a HashMap<String, String>,
    pub all_ids: &'a [String],
}

impl SourceDocuments<'_> {
    fn ids_for(&self, source_file: Option<&str>) -> Vec<String> {
        match source_file.and_then(|f| self.ids_by_filename.get(f)) {
            Some(id) => vec![id.clone()],
            None => self.all_ids.to_vec(),
        }
    }
}

/// Validate every element, in order. Indices run over the whole slice.
pub fn validate_documents(
    parsed: Vec<ParsedTaxDocument>,
    min_row_confidence: f32,
    docs: &SourceDocuments,
) -> Vec<TaxDocumentPreviewRow> {
    parsed
        .into_iter()
        .enumerate()
        .map(|(index, doc)| validate_document(index, doc, min_row_confidence, docs))
        .collect()
}

fn validate_document(
    index: usize,
    doc: ParsedTaxDocument,
    min_row_confidence: f32,
    docs: &SourceDocuments,
) -> TaxDocumentPreviewRow {
    let source_document_ids = docs.ids_for(doc.source_file.as_deref());
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    let document_type = match doc.document_type.as_str() {
        "payslip" => TaxDocumentType::Payslip,
        "p60" => TaxDocumentType::AnnualCertificate,
        "other" => {
            errors.push("not a payslip or P60".to_string());
            TaxDocumentType::Payslip
        }
        other => {
            errors.push(format!("unknown document type {other:?}"));
            TaxDocumentType::Payslip
        }
    };
    let is_p60 = document_type == TaxDocumentType::AnnualCertificate;

    let source_name = doc
        .employer
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    if source_name.is_empty() {
        errors.push("employer name is missing".to_string());
    }

    let currency = doc
        .currency
        .as_deref()
        .map(|c| c.trim().to_uppercase())
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| "GBP".to_string());
    if currency.len() != 3 || !currency.chars().all(|c| c.is_ascii_alphabetic()) {
        errors.push(format!("invalid currency {currency:?}"));
    }

    let pay_date = parse_date("pay_date", doc.pay_date.as_deref(), &mut errors);
    let mut period_start = parse_date("period_start", doc.period_start.as_deref(), &mut errors);
    let mut period_end = parse_date("period_end", doc.period_end.as_deref(), &mut errors);
    if let (Some(s), Some(e)) = (period_start, period_end) {
        if e < s {
            errors.push("period end is before period start".to_string());
        }
    }

    let printed_year = doc.printed_tax_year.as_deref().and_then(parse_printed_year);
    let anchor = pay_date.or(period_end).or(period_start);
    let tax_year = match (is_p60, printed_year, anchor) {
        (true, Some(printed), anchor) => {
            if let Some(a) = anchor {
                let derived = TaxYear::for_date(a);
                if derived != printed {
                    warnings.push(format!(
                        "printed tax year differs from the document's dates ({}); using {}",
                        derived.label(),
                        printed.label()
                    ));
                }
            }
            Some(printed)
        }
        (_, printed, Some(a)) => {
            let derived = TaxYear::for_date(a);
            if let Some(p) = printed {
                if p != derived {
                    warnings.push(format!(
                        "printed tax year {} differs from the pay date's tax year; using {}",
                        p.label(),
                        derived.label()
                    ));
                }
            }
            Some(derived)
        }
        (_, _, None) => {
            errors.push("no pay date, period or tax year could be read".to_string());
            None
        }
    };

    if is_p60 {
        if let Some(year) = tax_year {
            period_start = period_start.or(Some(year.start));
            period_end = period_end.or(Some(year.end));
        }
    }

    let force_period = is_p60;
    let mut line_items: Vec<TaxLineItem> = Vec::new();
    for line in &doc.line_items {
        let Some(line_type) = map_code(&line.code) else {
            errors.push(format!("unknown line code {:?}", line.code));
            continue;
        };
        let amount = match clean_amount(&line.amount) {
            Ok(a) => a,
            Err(reason) => {
                errors.push(format!("{}: {reason}", line.code));
                continue;
            }
        };
        let basis = if force_period {
            IncomeBasis::Period
        } else {
            match line.basis.as_str() {
                "period" => IncomeBasis::Period,
                "year_to_date" => IncomeBasis::YearToDate,
                other => {
                    errors.push(format!("{}: unknown basis {other:?}", line.code));
                    continue;
                }
            }
        };
        line_items.push(TaxLineItem {
            line_type,
            label: line.label.clone().filter(|l| !l.trim().is_empty()),
            amount,
            basis,
        });
    }

    if errors.is_empty() {
        let has_pay = period_amount(&line_items, IncomeLineType::TaxablePay).is_some()
            || period_amount(&line_items, IncomeLineType::GrossPay).is_some();
        if !has_pay {
            errors.push("no taxable pay or gross pay for the period".to_string());
        }
        if is_p60 && period_amount(&line_items, IncomeLineType::TaxablePay).is_none() {
            errors.push("a P60 needs pay in this employment (taxable pay)".to_string());
        }
        if period_amount(&line_items, IncomeLineType::PayeTax).is_none() {
            errors.push("no tax deducted".to_string());
        }
    }
    if doc.row_confidence < min_row_confidence {
        errors.push(format!(
            "row confidence {:.2} is below the minimum {:.2}",
            doc.row_confidence, min_row_confidence
        ));
    }

    if errors.is_empty() && !is_p60 {
        warnings.extend(payslip_warnings(&line_items));
    }

    let status = if errors.is_empty() {
        TransactionPreviewStatus::New
    } else {
        TransactionPreviewStatus::Error
    };
    TaxDocumentPreviewRow {
        index,
        status,
        error_reason: (!errors.is_empty()).then(|| errors.join("; ")),
        existing_id: None,
        same_key_as_row: None,
        row_confidence: doc.row_confidence,
        source_document_ids: source_document_ids.clone(),
        warnings,
        net_pay_candidates: Vec::new(),
        draft: TaxDocumentDraft {
            document_type,
            source_name,
            paye_reference: doc
                .paye_reference
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty()),
            tax_year: tax_year.map(|y| y.label()).unwrap_or_default(),
            pay_date: if is_p60 { None } else { iso(pay_date) },
            period_start: iso(period_start),
            period_end: iso(period_end),
            currency,
            line_items,
            transaction_id: None,
            document_ids: source_document_ids,
            notes: doc.notes.filter(|n| !n.trim().is_empty()),
        },
    }
}

fn iso(d: Option<NaiveDate>) -> Option<String> {
    d.map(|d| d.format("%Y-%m-%d").to_string())
}

fn parse_date(field: &str, raw: Option<&str>, errors: &mut Vec<String>) -> Option<NaiveDate> {
    let raw = raw.map(str::trim).filter(|s| !s.is_empty())?;
    match NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        Ok(d) => Some(d),
        Err(_) => {
            errors.push(format!("{field} {raw:?} is not a YYYY-MM-DD date"));
            None
        }
    }
}

/// Maps the model's code to the stored line type. The plan-30 names are
/// accepted too, as long as they are employment lines.
fn map_code(code: &str) -> Option<IncomeLineType> {
    match code {
        "tax_deducted" => Some(IncomeLineType::PayeTax),
        "notional_pay" => Some(IncomeLineType::RsuVestValue),
        other => IncomeLineType::parse(other).filter(|t| t.valid_for(IncomeSourceType::Employment)),
    }
}

/// Strips `£`, thousands commas and spaces; anything else non-numeric, or a
/// negative figure, is rejected.
fn clean_amount(raw: &str) -> Result<Decimal, String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !matches!(c, '£' | ',') && !c.is_whitespace())
        .collect();
    let amount =
        Decimal::from_str(&cleaned).map_err(|_| format!("amount {raw:?} is not a number"))?;
    if amount < Decimal::ZERO {
        return Err(format!("amount {raw:?} is negative"));
    }
    Ok(amount)
}

/// The tax year named by printed text such as "Year to 5 April 2026" (the
/// last four-digit number ends the year) or "2025/26" (the first starts it).
fn parse_printed_year(text: &str) -> Option<TaxYear> {
    let runs: Vec<(usize, &str)> = digit_runs(text);
    for (i, (pos, run)) in runs.iter().enumerate() {
        if run.len() != 4 {
            continue;
        }
        let start: i32 = run.parse().ok()?;
        let after = text[pos + 4..].chars().next();
        if let (Some('/' | '-'), Some((next_pos, next))) = (after, runs.get(i + 1)) {
            if *next_pos == pos + 5 {
                let end: i32 = next.parse().ok()?;
                if end == (start + 1) % 100 || end == start + 1 {
                    return TaxYear::from_start_year(start);
                }
            }
        }
    }
    let last_four = runs.iter().rev().find(|(_, r)| r.len() == 4)?;
    let end_year: i32 = last_four.1.parse().ok()?;
    TaxYear::from_start_year(end_year - 1)
}

fn digit_runs(text: &str) -> Vec<(usize, &str)> {
    let mut runs = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        match (c.is_ascii_digit(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                runs.push((s, &text[s..i]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        runs.push((s, &text[s..]));
    }
    runs
}

/// The first `period`-basis amount of a line type.
pub fn period_amount(items: &[TaxLineItem], line_type: IncomeLineType) -> Option<Decimal> {
    amount_with_basis(items, line_type, IncomeBasis::Period)
}

pub fn amount_with_basis(
    items: &[TaxLineItem],
    line_type: IncomeLineType,
    basis: IncomeBasis,
) -> Option<Decimal> {
    items
        .iter()
        .find(|i| i.line_type == line_type && i.basis == basis)
        .map(|i| i.amount)
}

fn period_sum(items: &[TaxLineItem], line_type: IncomeLineType) -> Decimal {
    items
        .iter()
        .filter(|i| i.line_type == line_type && i.basis == IncomeBasis::Period)
        .map(|i| i.amount)
        .sum()
}

/// Warnings only: the notional-pay note and the net-pay arithmetic check.
/// Pension schemes and benefits make the arithmetic unreliable, so it never
/// rejects a row.
fn payslip_warnings(items: &[TaxLineItem]) -> Vec<String> {
    let mut warnings = Vec::new();
    let notional = period_sum(items, IncomeLineType::RsuVestValue);
    if notional > Decimal::ZERO {
        warnings.push(format!(
            "includes notional pay {notional} (share value taxed as pay but not paid in cash)"
        ));
    }
    let (Some(gross), Some(net)) = (
        period_amount(items, IncomeLineType::GrossPay),
        period_amount(items, IncomeLineType::NetPay),
    ) else {
        return warnings;
    };
    let Some(tax) = period_amount(items, IncomeLineType::PayeTax) else {
        return warnings;
    };
    let deductions: Decimal = [
        IncomeLineType::EmployeeNi,
        IncomeLineType::StudentLoan,
        IncomeLineType::PostgraduateLoan,
        IncomeLineType::EmployeePension,
        IncomeLineType::OtherDeduction,
    ]
    .iter()
    .map(|t| period_sum(items, *t))
    .sum();
    let expected =
        gross - tax - deductions + period_sum(items, IncomeLineType::OtherPayment) - notional;
    if (expected - net).abs() > ARITHMETIC_TOLERANCE {
        warnings.push(format!(
            "net pay {net} does not match the printed figures (they give {expected})"
        ));
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importers::tax_documents::parser::{ParsedTaxDocuments, ParsedTaxLine};
    use serde_json::json;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn line(code: &str, amount: &str, basis: &str) -> ParsedTaxLine {
        ParsedTaxLine {
            code: code.to_string(),
            label: None,
            amount: amount.to_string(),
            basis: basis.to_string(),
        }
    }

    fn payslip(pay_date: &str) -> ParsedTaxDocument {
        ParsedTaxDocument {
            document_type: "payslip".to_string(),
            employer: Some("Example Widgets Ltd".to_string()),
            paye_reference: None,
            pay_date: Some(pay_date.to_string()),
            period_start: None,
            period_end: None,
            printed_tax_year: None,
            currency: None,
            line_items: vec![
                line("gross_pay", "3000.00", "period"),
                line("tax_deducted", "400.00", "period"),
                line("employee_ni", "150.00", "period"),
                line("net_pay", "2450.00", "period"),
            ],
            row_confidence: 0.95,
            notes: None,
            source_file: Some("a.pdf".to_string()),
        }
    }

    fn p60(printed: &str) -> ParsedTaxDocument {
        ParsedTaxDocument {
            document_type: "p60".to_string(),
            employer: Some("Example Widgets Ltd".to_string()),
            printed_tax_year: Some(printed.to_string()),
            pay_date: None,
            line_items: vec![
                line("taxable_pay", "36000.00", "period"),
                line("tax_deducted", "4800.00", "period"),
            ],
            ..payslip("2026-01-01")
        }
    }

    fn run(docs: Vec<ParsedTaxDocument>) -> Vec<TaxDocumentPreviewRow> {
        let mut ids = HashMap::new();
        ids.insert("a.pdf".to_string(), "doc-a".to_string());
        ids.insert("b.pdf".to_string(), "doc-b".to_string());
        let all = vec!["doc-a".to_string(), "doc-b".to_string()];
        validate_documents(
            docs,
            0.70,
            &SourceDocuments {
                ids_by_filename: &ids,
                all_ids: &all,
            },
        )
    }

    fn one(doc: ParsedTaxDocument) -> TaxDocumentPreviewRow {
        run(vec![doc]).remove(0)
    }

    #[test]
    fn two_payslips_in_one_file_share_source_documents() {
        let rows = run(vec![payslip("2025-06-27"), payslip("2025-07-25")]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].index, 0);
        assert_eq!(rows[1].index, 1);
        assert_eq!(rows[0].source_document_ids, vec!["doc-a"]);
        assert_eq!(rows[0].source_document_ids, rows[1].source_document_ids);
        assert_eq!(rows[0].status, TransactionPreviewStatus::New);
        assert_eq!(rows[0].draft.document_ids, vec!["doc-a"]);
    }

    #[test]
    fn unknown_source_file_falls_back_to_all_documents() {
        let mut doc = payslip("2025-06-27");
        doc.source_file = Some("zzz.pdf".to_string());
        assert_eq!(one(doc).source_document_ids, vec!["doc-a", "doc-b"]);
    }

    #[test]
    fn payslip_tax_year_follows_the_pay_date_across_five_six_april() {
        assert_eq!(one(payslip("2026-04-05")).draft.tax_year, "2025-26");
        assert_eq!(one(payslip("2026-04-06")).draft.tax_year, "2026-27");
    }

    #[test]
    fn pay_date_wins_over_a_period_in_the_previous_year() {
        let mut doc = payslip("2026-04-08");
        doc.period_start = Some("2026-03-06".to_string());
        doc.period_end = Some("2026-04-05".to_string());
        assert_eq!(one(doc).draft.tax_year, "2026-27");
    }

    #[test]
    fn p60_year_comes_from_the_printed_year_end() {
        let row = one(p60("Year to 5 April 2026"));
        assert_eq!(row.status, TransactionPreviewStatus::New);
        assert_eq!(row.draft.tax_year, "2025-26");
        assert_eq!(row.draft.pay_date, None);
        assert_eq!(row.draft.period_start.as_deref(), Some("2025-04-06"));
        assert_eq!(row.draft.period_end.as_deref(), Some("2026-04-05"));
        assert_eq!(one(p60("2024/25")).draft.tax_year, "2024-25");
        assert_eq!(
            one(p60("6 April 2023 to 5 April 2024")).draft.tax_year,
            "2023-24"
        );
    }

    #[test]
    fn p60_figures_are_always_period_basis() {
        let mut doc = p60("2025/26");
        doc.line_items[0].basis = "year_to_date".to_string();
        let row = one(doc);
        assert!(
            row.draft
                .line_items
                .iter()
                .all(|i| i.basis == IncomeBasis::Period)
        );
    }

    #[test]
    fn printed_year_disagreeing_with_the_pay_date_warns_and_derived_wins() {
        let mut doc = payslip("2025-06-27");
        doc.printed_tax_year = Some("2024/25".to_string());
        let row = one(doc);
        assert_eq!(row.draft.tax_year, "2025-26");
        assert!(row.warnings.iter().any(|w| w.contains("printed tax year")));
        assert_eq!(row.status, TransactionPreviewStatus::New);
    }

    #[test]
    fn amounts_are_cleaned_or_rejected() {
        let mut doc = payslip("2025-06-27");
        doc.line_items[0] = line("gross_pay", "£3,000.00", "period");
        doc.line_items[3] = line("net_pay", " 2 450.00 ", "period");
        let row = one(doc);
        assert_eq!(row.status, TransactionPreviewStatus::New);
        assert_eq!(row.draft.line_items[0].amount, dec("3000.00"));
        assert_eq!(row.draft.line_items[3].amount, dec("2450.00"));

        let mut bad = payslip("2025-06-27");
        bad.line_items[0] = line("gross_pay", "3k", "period");
        assert_eq!(one(bad).status, TransactionPreviewStatus::Error);

        let mut negative = payslip("2025-06-27");
        negative.line_items[1] = line("tax_deducted", "-400.00", "period");
        let row = one(negative);
        assert_eq!(row.status, TransactionPreviewStatus::Error);
        assert!(row.error_reason.unwrap().contains("negative"));
    }

    #[test]
    fn missing_fields_are_error_rows() {
        let mut no_employer = payslip("2025-06-27");
        no_employer.employer = Some("  ".to_string());
        assert_eq!(one(no_employer).status, TransactionPreviewStatus::Error);

        let mut no_pay = payslip("2025-06-27");
        no_pay.line_items.retain(|l| l.code != "gross_pay");
        assert_eq!(one(no_pay).status, TransactionPreviewStatus::Error);

        let mut no_tax = payslip("2025-06-27");
        no_tax.line_items.retain(|l| l.code != "tax_deducted");
        assert_eq!(one(no_tax).status, TransactionPreviewStatus::Error);

        let mut only_ytd_tax = payslip("2025-06-27");
        only_ytd_tax.line_items[1] = line("tax_deducted", "400.00", "year_to_date");
        assert_eq!(one(only_ytd_tax).status, TransactionPreviewStatus::Error);

        let mut no_date = payslip("2025-06-27");
        no_date.pay_date = None;
        assert_eq!(one(no_date).status, TransactionPreviewStatus::Error);
    }

    #[test]
    fn p60_without_taxable_pay_is_an_error_row() {
        let mut doc = p60("2025/26");
        doc.line_items[0] = line("gross_pay", "36000.00", "period");
        assert_eq!(one(doc).status, TransactionPreviewStatus::Error);
    }

    #[test]
    fn taxable_pay_alone_satisfies_the_pay_requirement() {
        let mut doc = payslip("2025-06-27");
        doc.line_items[0] = line("taxable_pay", "2900.00", "period");
        assert_eq!(one(doc).status, TransactionPreviewStatus::New);
    }

    #[test]
    fn low_confidence_and_other_documents_are_error_rows() {
        let mut low = payslip("2025-06-27");
        low.row_confidence = 0.5;
        assert_eq!(one(low).status, TransactionPreviewStatus::Error);

        let mut other = payslip("2025-06-27");
        other.document_type = "other".to_string();
        let row = one(other);
        assert_eq!(row.status, TransactionPreviewStatus::Error);
        assert!(row.error_reason.unwrap().contains("not a payslip or P60"));
    }

    #[test]
    fn notional_pay_maps_to_rsu_vest_value_and_warns() {
        let mut doc = payslip("2025-06-27");
        doc.line_items[0] = line("gross_pay", "5000.00", "period");
        doc.line_items[3] = line("net_pay", "2450.00", "period");
        doc.line_items
            .push(line("notional_pay", "2000.00", "period"));
        let row = one(doc);
        assert_eq!(row.status, TransactionPreviewStatus::New);
        assert!(
            row.draft
                .line_items
                .iter()
                .any(|i| i.line_type == IncomeLineType::RsuVestValue)
        );
        assert!(row.warnings.iter().any(|w| w.contains("notional pay 2000")));
        assert!(
            !row.warnings.iter().any(|w| w.contains("does not match")),
            "an equity payslip that adds up must not trip the arithmetic warning"
        );
    }

    #[test]
    fn arithmetic_mismatch_warns_and_stays_new() {
        let mut doc = payslip("2025-06-27");
        doc.line_items[3] = line("net_pay", "2000.00", "period");
        let row = one(doc);
        assert_eq!(row.status, TransactionPreviewStatus::New);
        assert!(row.warnings.iter().any(|w| w.contains("does not match")));
    }

    #[test]
    fn unknown_code_and_basis_are_error_rows() {
        let mut doc = payslip("2025-06-27");
        doc.line_items.push(line("bonus_pay", "10", "period"));
        assert_eq!(one(doc).status, TransactionPreviewStatus::Error);

        let mut doc = payslip("2025-06-27");
        doc.line_items.push(line("other_payment", "10", "weekly"));
        assert_eq!(one(doc).status, TransactionPreviewStatus::Error);
    }

    #[test]
    fn paye_reference_is_carried_and_trimmed() {
        let mut doc = payslip("2025-06-27");
        doc.paye_reference = Some(" 123/AB456 ".to_string());
        assert_eq!(one(doc).draft.paye_reference.as_deref(), Some("123/AB456"));
    }

    #[test]
    fn raw_output_with_numeric_amounts_deserialises() {
        let parsed: ParsedTaxDocuments = serde_json::from_value(json!({
            "detection_confidence": 0.9,
            "documents": [{
                "document_type": "payslip", "employer": "Example Widgets Ltd",
                "line_items": [{"code": "gross_pay", "amount": 12.5, "basis": "period"}],
                "row_confidence": 0.9
            }]
        }))
        .unwrap();
        assert_eq!(parsed.documents[0].line_items[0].amount, "12.5");
    }
}
