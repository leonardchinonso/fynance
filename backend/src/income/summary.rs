//! The tax-year summary for one profile (plan 30, section 7).
//!
//! Only `period`-basis line items of `period` and `year_end_adjustment`
//! records are summed, and only line types the catalogue marks `summed`. A P60
//! or annual accounts (an `IncomeCheck`) is compared with the sum and never
//! added to it. Money legs are exact Decimals; nothing here goes through a REAL
//! cast.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, anyhow};
use chrono::NaiveDate;
use rust_decimal::Decimal;

use crate::income::line_types::{IncomeLineType, TRADING_ALLOWANCE};
use crate::model::{
    BankExpensesSummary, CgtLink, DonationSummary, EmploymentBoxes, EmploymentCheckSummary,
    EmploymentShown, EmploymentSourceSummary, IncomeBasis, IncomeCheck, IncomeCheckStatus,
    IncomeCheckType, IncomeLineItemInput, IncomeRecord, IncomeRecordKind, IncomeSource,
    IncomeSourceType, InterestAccountTotal, InterestSummary, RsuLinkStatus, RsuVestSummary,
    SelfEmploymentCheckSummary, SelfEmploymentSummary, TaxYearSummary, UnmappedExpenseCategory,
};
use crate::storage::db::Db;
use crate::util::fx::FxRateMap;
use crate::util::tax_year::TaxYear;

/// An RSU vest line matches its event when the two GBP values are within this.
pub const VEST_TOLERANCE: Decimal = Decimal::from_parts(1, 0, 0, false, 0);

/// Converts amounts to the preferred currency at a date, collecting one
/// warning per missing rate instead of failing the whole summary (D5).
struct Converter<'a> {
    fx: &'a FxRateMap,
    warnings: BTreeSet<String>,
}

impl<'a> Converter<'a> {
    fn new(fx: &'a FxRateMap) -> Self {
        Self {
            fx,
            warnings: BTreeSet::new(),
        }
    }

    fn in_preferred(
        &mut self,
        amount: Decimal,
        currency: &str,
        date: Option<NaiveDate>,
    ) -> Decimal {
        let Some(date) = date else {
            return self.fx.convert(amount, currency);
        };
        match self.fx.convert_as_of(amount, currency, date) {
            Ok(v) => v,
            Err(m) => {
                self.warnings.insert(format!(
                    "no {} exchange rate stored for {}; amounts in that currency on that date are left out",
                    m.currency, m.date
                ));
                Decimal::ZERO
            }
        }
    }
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
}

/// The date a record's amounts convert at: PAYE is by payment date.
fn record_date(rec: &IncomeRecord) -> Option<NaiveDate> {
    rec.pay_date
        .as_deref()
        .and_then(parse_date)
        .or_else(|| parse_date(&rec.period_end))
}

/// Per-line-type sums of the period-basis lines of a set of records, in the
/// preferred currency, plus the SA102 pay figure (taxable pay per record when
/// present, else gross pay).
#[derive(Default)]
struct Sums {
    by_type: BTreeMap<IncomeLineType, Decimal>,
    pay_box: Decimal,
}

impl Sums {
    fn get(&self, t: IncomeLineType) -> Decimal {
        self.by_type.get(&t).copied().unwrap_or(Decimal::ZERO)
    }
}

fn sum_records(records: &[&IncomeRecord], conv: &mut Converter) -> Sums {
    let mut sums = Sums::default();
    for rec in records {
        let date = record_date(rec);
        let mut taxable_present = false;
        let mut taxable = Decimal::ZERO;
        let mut gross = Decimal::ZERO;
        for li in rec
            .line_items
            .iter()
            .filter(|l| l.basis == IncomeBasis::Period)
        {
            let v = conv.in_preferred(li.amount, &li.currency, date);
            *sums.by_type.entry(li.line_type).or_default() += v;
            match li.line_type {
                IncomeLineType::TaxablePay => {
                    taxable_present = true;
                    taxable += v;
                }
                IncomeLineType::GrossPay => gross += v,
                _ => {}
            }
        }
        sums.pay_box += if taxable_present { taxable } else { gross };
    }
    sums
}

fn months(records: &[&IncomeRecord]) -> Vec<String> {
    let set: BTreeSet<String> = records
        .iter()
        .filter(|r| r.kind == IncomeRecordKind::Period)
        .filter_map(|r| {
            // Employment months follow the pay date, like the tax year does.
            let date = record_date(r)?;
            Some(date.format("%Y-%m").to_string())
        })
        .collect();
    set.into_iter().collect()
}

fn check_figure(
    check: &IncomeCheck,
    types: &[IncomeLineType],
    conv: &mut Converter,
    date: NaiveDate,
) -> Option<Decimal> {
    types.iter().find_map(|t| {
        let figs: Vec<_> = check.figures.iter().filter(|f| f.line_type == *t).collect();
        if figs.is_empty() {
            return None;
        }
        Some(
            figs.iter()
                .map(|f| conv.in_preferred(f.amount, &f.currency, Some(date)))
                .sum(),
        )
    })
}

/// How a check stands against the records.
fn status_of(
    period_diffs: &[Decimal],
    total_diffs: &[Decimal],
    has_adjustment: bool,
) -> IncomeCheckStatus {
    if period_diffs.iter().all(|d| d.is_zero()) {
        IncomeCheckStatus::Matched
    } else if has_adjustment && total_diffs.iter().all(|d| d.is_zero()) {
        IncomeCheckStatus::Adjusted
    } else {
        IncomeCheckStatus::Gap
    }
}

/// Employment figures for one source: `(boxes sums over all records,
/// SA102 box sums over period records only)`.
struct EmploymentFigures {
    all: Sums,
    period_only: Sums,
}

fn employment_figures(records: &[IncomeRecord], conv: &mut Converter) -> EmploymentFigures {
    let all: Vec<&IncomeRecord> = records.iter().collect();
    let period: Vec<&IncomeRecord> = records
        .iter()
        .filter(|r| r.kind == IncomeRecordKind::Period)
        .collect();
    EmploymentFigures {
        all: sum_records(&all, conv),
        period_only: sum_records(&period, conv),
    }
}

fn rsu_rows(records: &[IncomeRecord], db: &Db, fx: &FxRateMap) -> Result<Vec<RsuVestSummary>> {
    let mut out = Vec::new();
    for rec in records
        .iter()
        .filter(|r| r.kind == IncomeRecordKind::Period)
    {
        for li in rec.line_items.iter().filter(|l| {
            l.line_type == IncomeLineType::RsuVestValue && l.basis == IncomeBasis::Period
        }) {
            let date = record_date(rec);
            let vest_value = match date {
                Some(d) => fx.convert_as_of_stored(li.amount, &li.currency, d),
                None => fx.convert(li.amount, &li.currency),
            };
            let mut row = RsuVestSummary {
                record_id: rec.id.clone(),
                line_item_id: li.id.clone(),
                vest_value,
                investment_event_id: None,
                event_value: None,
                status: RsuLinkStatus::Unlinked,
            };
            let event = match &li.investment_event_id {
                Some(event_id) => db.get_investment_event(event_id)?,
                None => None,
            };
            if let Some(ev) = event {
                let event_id = ev.id.clone();
                let value = fx.convert_as_of_stored(
                    ev.quantity * ev.price_per_share,
                    &ev.currency,
                    ev.date.date(),
                );
                row.investment_event_id = Some(event_id);
                row.event_value = Some(value);
                row.status = if (value - vest_value).abs() <= VEST_TOLERANCE {
                    RsuLinkStatus::Matched
                } else {
                    RsuLinkStatus::Mismatch
                };
            }
            out.push(row);
        }
    }
    Ok(out)
}

fn employment_summary(
    db: &Db,
    source: &IncomeSource,
    year: &TaxYear,
    records: &[IncomeRecord],
    check: Option<&IncomeCheck>,
    fx: &FxRateMap,
    conv: &mut Converter,
) -> Result<EmploymentSourceSummary> {
    let figs = employment_figures(records, conv);
    let s = &figs.all;
    let all_refs: Vec<&IncomeRecord> = records.iter().collect();
    let check_summary = check.map(|c| {
        let pay = check_figure(
            c,
            &[IncomeLineType::TaxablePay, IncomeLineType::GrossPay],
            conv,
            year.end,
        );
        let tax = check_figure(c, &[IncomeLineType::PayeTax], conv, year.end);
        let pay_diff = pay.map(|p| p - s.pay_box).unwrap_or(Decimal::ZERO);
        let tax_diff = tax
            .map(|t| t - s.get(IncomeLineType::PayeTax))
            .unwrap_or(Decimal::ZERO);
        let period_pay = pay
            .map(|p| p - figs.period_only.pay_box)
            .unwrap_or(Decimal::ZERO);
        let period_tax = tax
            .map(|t| t - figs.period_only.get(IncomeLineType::PayeTax))
            .unwrap_or(Decimal::ZERO);
        let has_adjustment = records
            .iter()
            .any(|r| r.kind == IncomeRecordKind::YearEndAdjustment);
        EmploymentCheckSummary {
            check_id: c.id.clone(),
            pay: pay.unwrap_or(Decimal::ZERO),
            tax: tax.unwrap_or(Decimal::ZERO),
            pay_difference: pay_diff,
            tax_difference: tax_diff,
            status: status_of(
                &[period_pay, period_tax],
                &[pay_diff, tax_diff],
                has_adjustment,
            ),
        }
    });
    Ok(EmploymentSourceSummary {
        source_id: source.id.clone(),
        name: source.name.clone(),
        paye_reference: source.identifiers.paye_reference.clone(),
        record_count: records
            .iter()
            .filter(|r| r.kind == IncomeRecordKind::Period)
            .count() as u32,
        months_covered: months(&all_refs),
        boxes: EmploymentBoxes {
            pay: s.pay_box,
            tax: s.get(IncomeLineType::PayeTax),
            student_loan: s.get(IncomeLineType::StudentLoan),
            postgraduate_loan: s.get(IncomeLineType::PostgraduateLoan),
        },
        shown: EmploymentShown {
            gross_pay: s.get(IncomeLineType::GrossPay),
            employee_ni: s.get(IncomeLineType::EmployeeNi),
            employee_pension: s.get(IncomeLineType::EmployeePension),
            rsu_vest_value: s.get(IncomeLineType::RsuVestValue),
            rsu_tax: s.get(IncomeLineType::RsuTax),
            net_pay: s.get(IncomeLineType::NetPay),
        },
        check: check_summary,
        rsu: rsu_rows(records, db, fx)?,
    })
}

/// Self-employment turnover and expenses from the record lines alone.
struct SeFigures {
    turnover: Decimal,
    by_type: BTreeMap<IncomeLineType, Decimal>,
    info: BTreeMap<IncomeLineType, Decimal>,
}

fn se_figures(records: &[&IncomeRecord], conv: &mut Converter) -> SeFigures {
    let mut f = SeFigures {
        turnover: Decimal::ZERO,
        by_type: BTreeMap::new(),
        info: BTreeMap::new(),
    };
    for rec in records {
        let date = record_date(rec);
        for li in rec
            .line_items
            .iter()
            .filter(|l| l.basis == IncomeBasis::Period)
        {
            let v = conv.in_preferred(li.amount, &li.currency, date);
            let lt = li.line_type;
            if lt.is_turnover() {
                f.turnover += v * Decimal::from(lt.info().sign);
            } else if lt.is_expense() {
                *f.by_type.entry(lt).or_default() += v;
            } else if lt.is_costing() {
                *f.info.entry(lt).or_default() += v;
            }
        }
    }
    f
}

struct BankFigures {
    by_type: BTreeMap<IncomeLineType, Decimal>,
    summary: BankExpensesSummary,
}

fn bank_expenses(
    db: &Db,
    source: &IncomeSource,
    year: &TaxYear,
    conv: &mut Converter,
) -> Result<BankFigures> {
    let rows = db.bank_outflows(&source.account_ids, year.start, year.end)?;
    let mut mapped_by_type: BTreeMap<IncomeLineType, Decimal> = BTreeMap::new();
    let mut unmapped: BTreeMap<String, (String, Decimal)> = BTreeMap::new();
    for row in rows {
        let amount = conv.in_preferred(row.amount, &row.currency, Some(row.date));
        let category = row
            .category_id
            .clone()
            .unwrap_or_else(|| "uncategorized".to_string());
        match source.expense_category_map.get(&category) {
            Some(lt) => *mapped_by_type.entry(*lt).or_default() += amount,
            None => {
                let name = row
                    .category_name
                    .clone()
                    .unwrap_or_else(|| "Uncategorized".to_string());
                unmapped.entry(category).or_insert((name, Decimal::ZERO)).1 += amount;
            }
        }
    }
    let mut unmapped: Vec<UnmappedExpenseCategory> = unmapped
        .into_iter()
        .map(|(category_id, (name, total))| UnmappedExpenseCategory {
            category_id,
            name,
            total,
        })
        .collect();
    unmapped.sort_by(|a, b| b.total.cmp(&a.total).then(a.name.cmp(&b.name)));
    Ok(BankFigures {
        summary: BankExpensesSummary {
            mapped_total: mapped_by_type.values().copied().sum(),
            unmapped,
        },
        by_type: mapped_by_type,
    })
}

fn self_employment_summary(
    db: &Db,
    source: &IncomeSource,
    year: &TaxYear,
    records: &[IncomeRecord],
    check: Option<&IncomeCheck>,
    conv: &mut Converter,
) -> Result<SelfEmploymentSummary> {
    let all: Vec<&IncomeRecord> = records.iter().collect();
    let period: Vec<&IncomeRecord> = records
        .iter()
        .filter(|r| r.kind == IncomeRecordKind::Period)
        .collect();
    let rec_all = se_figures(&all, conv);
    let rec_period = se_figures(&period, conv);
    let bank = bank_expenses(db, source, year, conv)?;

    let mut by_type = rec_all.by_type.clone();
    for (lt, v) in &bank.by_type {
        *by_type.entry(*lt).or_default() += *v;
    }
    let total_expenses: Decimal = by_type.values().copied().sum();
    let period_expenses: Decimal =
        rec_period.by_type.values().copied().sum::<Decimal>() + bank.summary.mapped_total;

    let check_summary = check.map(|c| {
        let sales: Decimal = check_figure(c, &[IncomeLineType::Sales], conv, year.end)
            .unwrap_or(Decimal::ZERO)
            - check_figure(c, &[IncomeLineType::Refunds], conv, year.end).unwrap_or(Decimal::ZERO);
        let expenses = check_expenses(c, conv, year.end);
        let turnover_diff = sales - rec_all.turnover;
        let expenses_diff = expenses - total_expenses;
        let has_adjustment = records
            .iter()
            .any(|r| r.kind == IncomeRecordKind::YearEndAdjustment);
        SelfEmploymentCheckSummary {
            check_id: c.id.clone(),
            turnover: sales,
            expenses,
            turnover_difference: turnover_diff,
            expenses_difference: expenses_diff,
            status: status_of(
                &[sales - rec_period.turnover, expenses - period_expenses],
                &[turnover_diff, expenses_diff],
                has_adjustment,
            ),
        }
    });

    Ok(SelfEmploymentSummary {
        source_id: source.id.clone(),
        name: source.name.clone(),
        months_covered: months(&all),
        turnover: rec_all.turnover,
        expenses_by_line_type: by_type
            .iter()
            .filter(|(_, v)| !v.is_zero())
            .map(|(k, v)| (k.as_str().to_string(), v.to_string()))
            .collect(),
        total_expenses,
        net_profit: rec_all.turnover - total_expenses,
        trading_allowance_better: rec_all.turnover > Decimal::ZERO
            && total_expenses < TRADING_ALLOWANCE,
        bank_expenses: bank.summary,
        info: rec_all
            .info
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), v.to_string()))
            .collect(),
        check: check_summary,
    })
}

/// An annual-accounts check's total expenses: every expense-type figure.
fn check_expenses(check: &IncomeCheck, conv: &mut Converter, date: NaiveDate) -> Decimal {
    check
        .figures
        .iter()
        .filter(|f| f.line_type.is_expense())
        .map(|f| conv.in_preferred(f.amount, &f.currency, Some(date)))
        .sum()
}

/// Differences between a check and the `period` records of its source, as the
/// signed line items a year-end adjustment would carry (GBP, zero lines
/// omitted). Employment compares taxable pay and PAYE tax; self-employment
/// compares turnover and total expenses (record lines plus mapped bank outflows).
pub fn adjustment_lines(
    db: &Db,
    source: &IncomeSource,
    check: &IncomeCheck,
    year: &TaxYear,
    fx: &FxRateMap,
) -> Result<Vec<IncomeLineItemInput>> {
    let records = db.list_income_records(None, Some(&source.id), Some(&year.label()))?;
    let period: Vec<&IncomeRecord> = records
        .iter()
        .filter(|r| r.kind == IncomeRecordKind::Period)
        .collect();
    let mut conv = Converter::new(fx);
    let mut lines: Vec<(IncomeLineType, Decimal)> = Vec::new();
    match (source.source_type, check.check_type) {
        (IncomeSourceType::Employment, IncomeCheckType::P60) => {
            let sums = sum_records(&period, &mut conv);
            if let Some(pay) = check_figure(
                check,
                &[IncomeLineType::TaxablePay, IncomeLineType::GrossPay],
                &mut conv,
                year.end,
            ) {
                lines.push((IncomeLineType::TaxablePay, pay - sums.pay_box));
            }
            if let Some(tax) = check_figure(check, &[IncomeLineType::PayeTax], &mut conv, year.end)
            {
                lines.push((
                    IncomeLineType::PayeTax,
                    tax - sums.get(IncomeLineType::PayeTax),
                ));
            }
        }
        (IncomeSourceType::SelfEmployment, IncomeCheckType::AnnualAccounts) => {
            let rec = se_figures(&period, &mut conv);
            let bank = bank_expenses(db, source, year, &mut conv)?;
            let sales = check_figure(check, &[IncomeLineType::Sales], &mut conv, year.end)
                .unwrap_or(Decimal::ZERO)
                - check_figure(check, &[IncomeLineType::Refunds], &mut conv, year.end)
                    .unwrap_or(Decimal::ZERO);
            let expenses = check_expenses(check, &mut conv, year.end);
            let have: Decimal =
                rec.by_type.values().copied().sum::<Decimal>() + bank.summary.mapped_total;
            lines.push((IncomeLineType::Sales, sales - rec.turnover));
            lines.push((IncomeLineType::OtherExpenses, expenses - have));
        }
        _ => return Err(anyhow!("check type does not match the source type")),
    }
    Ok(lines
        .into_iter()
        .filter(|(_, d)| !d.is_zero())
        .map(|(line_type, amount)| IncomeLineItemInput {
            line_type,
            amount,
            currency: Some(fx.preferred().to_string()),
            basis: IncomeBasis::Period,
            label: Some("Year-end adjustment".to_string()),
            investment_event_id: None,
        })
        .collect())
}

fn split_share(amount: Decimal, share_of: u32) -> Decimal {
    if share_of > 1 {
        (amount / Decimal::from(share_of)).round_dp(2)
    } else {
        amount
    }
}

fn interest_summary(
    db: &Db,
    profile_id: &str,
    year: &TaxYear,
    fx: &FxRateMap,
) -> Result<InterestSummary> {
    let legs = db.account_legs_for_profile(
        profile_id,
        year.start,
        year.end,
        &["interest_taxable", "interest_non_taxable"],
    )?;
    let mut by_account: Vec<InterestAccountTotal> = Vec::new();
    let mut taxable_total = Decimal::ZERO;
    let mut non_taxable_total = Decimal::ZERO;
    for leg in legs {
        let share = split_share(fx.convert(leg.amount, &leg.currency), leg.profile_count);
        let taxable = leg.category_type == "interest_taxable";
        if taxable {
            taxable_total += share;
        } else {
            non_taxable_total += share;
        }
        let idx = match by_account
            .iter()
            .position(|e| e.account_id == leg.account_id)
        {
            Some(i) => i,
            None => {
                by_account.push(InterestAccountTotal {
                    account_id: leg.account_id.clone(),
                    account_name: leg.account_name.clone(),
                    taxable: Decimal::ZERO,
                    non_taxable: Decimal::ZERO,
                    joint_share_of: leg.profile_count,
                });
                by_account.len() - 1
            }
        };
        if taxable {
            by_account[idx].taxable += share;
        } else {
            by_account[idx].non_taxable += share;
        }
    }
    Ok(InterestSummary {
        taxable_total,
        non_taxable_total,
        by_account,
    })
}

fn donation_summary(
    db: &Db,
    profile_id: &str,
    year: &TaxYear,
    fx: &FxRateMap,
) -> Result<DonationSummary> {
    let legs = db.account_legs_for_profile(
        profile_id,
        year.start,
        year.end,
        &["donation_taxable", "donation_non_taxable"],
    )?;
    let mut gift_aid_total = Decimal::ZERO;
    let mut non_gift_aid_total = Decimal::ZERO;
    for leg in legs.into_iter().filter(|l| l.amount < Decimal::ZERO) {
        let share = split_share(
            fx.convert(leg.amount.abs(), &leg.currency),
            leg.profile_count,
        );
        if leg.category_type == "donation_taxable" {
            gift_aid_total += share;
        } else {
            non_gift_aid_total += share;
        }
    }
    Ok(DonationSummary {
        gift_aid_total,
        non_gift_aid_total,
    })
}

/// The whole `GET /api/tax/summary/:tax_year` payload for one profile.
pub fn compute_tax_year_summary(
    db: &Db,
    profile_id: &str,
    tax_year: &str,
    fx: &FxRateMap,
) -> Result<TaxYearSummary> {
    let year = TaxYear::parse(tax_year)
        .ok_or_else(|| anyhow!("tax_year must look like '2024-25', got {tax_year:?}"))?;
    let label = year.label();
    let sources = db.list_income_sources(Some(profile_id))?;
    let records = db.list_income_records(Some(profile_id), None, Some(&label))?;
    let checks = db.list_income_checks(Some(profile_id), Some(&label))?;
    let mut conv = Converter::new(fx);

    let mut employment = Vec::new();
    let mut self_employment = Vec::new();
    for source in &sources {
        let own: Vec<IncomeRecord> = records
            .iter()
            .filter(|r| r.source_id == source.id)
            .cloned()
            .collect();
        let check = checks.iter().find(|c| {
            c.source_id == source.id
                && match source.source_type {
                    IncomeSourceType::Employment => c.check_type == IncomeCheckType::P60,
                    _ => c.check_type == IncomeCheckType::AnnualAccounts,
                }
        });
        if !source.active && own.is_empty() && check.is_none() {
            continue;
        }
        match source.source_type {
            IncomeSourceType::Employment => employment.push(employment_summary(
                db, source, &year, &own, check, fx, &mut conv,
            )?),
            IncomeSourceType::SelfEmployment => self_employment.push(self_employment_summary(
                db, source, &year, &own, check, &mut conv,
            )?),
            IncomeSourceType::Property | IncomeSourceType::Other => {}
        }
    }

    Ok(TaxYearSummary {
        tax_year: label,
        start_date: year.start.format("%Y-%m-%d").to_string(),
        end_date: year.end.format("%Y-%m-%d").to_string(),
        profile_id: profile_id.to_string(),
        employment,
        self_employment,
        interest: interest_summary(db, profile_id, &year, fx)?,
        donations: donation_summary(db, profile_id, &year, fx)?,
        cgt_link: CgtLink {
            start_date: year.start.format("%Y-%m-%d").to_string(),
            end_date: year.end.format("%Y-%m-%d").to_string(),
        },
        warnings: conv.warnings.into_iter().collect(),
    })
}
