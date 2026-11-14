//! The line-type catalogue: one table that says which self-assessment box each
//! income line feeds, whether it is summed, and which source types may carry it.
//! Served at `GET /api/income/line-types` so the frontend builds its columns
//! from the same table the summary uses.
//!
//! Box numbers are from HMRC's 2024-25 forms:
//! https://assets.publishing.service.gov.uk/media/67ee79969eae202448299c79/SA102_2025.pdf
//! https://assets.publishing.service.gov.uk/media/67dd4776c6194abe97358be4/SA103S-2025.pdf
//! The student loan section of the SA100 was not checked against the form.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::IncomeSourceType;

/// SA103S: below this turnover the trading allowance can replace actual expenses.
pub const TRADING_ALLOWANCE: Decimal = Decimal::from_parts(1000, 0, 0, false, 0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
#[serde(rename_all = "snake_case")]
pub enum IncomeLineType {
    TaxablePay,
    GrossPay,
    PayeTax,
    EmployeeNi,
    StudentLoan,
    PostgraduateLoan,
    EmployeePension,
    RsuVestValue,
    RsuTax,
    NetPay,
    OtherPayment,
    OtherDeduction,
    Sales,
    Refunds,
    PlatformFees,
    PlatformPostage,
    CostOfGoods,
    Postage,
    Packaging,
    OtherExpenses,
    CostingMaterials,
    CostingPackaging,
    CostingPostage,
    CostingOther,
    RentReceived,
    PropertyExpenses,
}

/// One catalogue row. `sign` is the effect on the headline figure the line is
/// summed into: `-1` for refunds (against turnover), `+1` otherwise.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct LineTypeInfo {
    pub line_type: IncomeLineType,
    pub source_type: IncomeSourceType,
    pub label: &'static str,
    pub summed: bool,
    pub sign: i8,
    pub form: &'static str,
    pub box_ref: &'static str,
    pub note: &'static str,
}

macro_rules! info {
    ($lt:ident, $st:ident, $label:expr, $summed:expr, $sign:expr, $form:expr, $box_ref:expr, $note:expr) => {
        LineTypeInfo {
            line_type: IncomeLineType::$lt,
            source_type: IncomeSourceType::$st,
            label: $label,
            summed: $summed,
            sign: $sign,
            form: $form,
            box_ref: $box_ref,
            note: $note,
        }
    };
}

static CATALOGUE: [LineTypeInfo; 26] = [
    info!(
        TaxablePay,
        Employment, "Taxable pay", true, 1, "SA102", "1", "Preferred over gross pay for a record."
    ),
    info!(
        GrossPay,
        Employment,
        "Gross pay",
        true,
        1,
        "SA102",
        "1",
        "Counts toward box 1 only when the record has no taxable pay."
    ),
    info!(
        PayeTax,
        Employment,
        "PAYE tax",
        true,
        1,
        "SA102",
        "2",
        "Total PAYE for the period, includes tax on RSU."
    ),
    info!(
        EmployeeNi,
        Employment, "Employee NI", false, 1, "", "", "Shown only."
    ),
    info!(
        StudentLoan,
        Employment,
        "Student loan",
        true,
        1,
        "SA100",
        "Student loan repayments deducted by employer",
        "Summed into the SA100 student loan section."
    ),
    info!(
        PostgraduateLoan,
        Employment,
        "Postgraduate loan",
        true,
        1,
        "SA100",
        "Student loan repayments deducted by employer",
        "Same SA100 section as the student loan."
    ),
    info!(
        EmployeePension,
        Employment, "Employee pension", false, 1, "", "", "Shown only."
    ),
    info!(
        RsuVestValue,
        Employment,
        "RSU vest value",
        false,
        1,
        "",
        "",
        "Already inside taxable pay; shown and checked against the vest event."
    ),
    info!(
        RsuTax,
        Employment,
        "RSU tax",
        false,
        1,
        "",
        "",
        "An of-which breakdown of PAYE tax; never added again."
    ),
    info!(
        NetPay,
        Employment, "Net pay", false, 1, "", "", "Shown and used to link the bank credit."
    ),
    info!(
        OtherPayment,
        Employment, "Other payment", false, 1, "", "", "Shown only."
    ),
    info!(
        OtherDeduction,
        Employment, "Other deduction", false, 1, "", "", "Shown only."
    ),
    info!(
        Sales,
        SelfEmployment, "Sales", true, 1, "SA103S", "9", "Turnover."
    ),
    info!(
        Refunds,
        SelfEmployment, "Refunds", true, -1, "SA103S", "9", "Deducted from turnover."
    ),
    info!(
        PlatformFees,
        SelfEmployment,
        "Platform fees",
        true,
        1,
        "SA103S",
        "19",
        "Card and marketplace fees deducted at source."
    ),
    info!(
        PlatformPostage,
        SelfEmployment,
        "Platform postage",
        true,
        1,
        "SA103S",
        "18",
        "Marketplace postage labels deducted at source."
    ),
    info!(
        CostOfGoods,
        SelfEmployment,
        "Cost of goods",
        true,
        1,
        "SA103S",
        "11",
        "From categorised business-account outflows."
    ),
    info!(
        Postage,
        SelfEmployment,
        "Postage",
        true,
        1,
        "SA103S",
        "18",
        "From categorised business-account outflows."
    ),
    info!(
        Packaging,
        SelfEmployment,
        "Packaging",
        true,
        1,
        "SA103S",
        "11",
        "From categorised business-account outflows."
    ),
    info!(
        OtherExpenses,
        SelfEmployment,
        "Other expenses",
        true,
        1,
        "SA103S",
        "19",
        "From categorised business-account outflows."
    ),
    info!(
        CostingMaterials,
        SelfEmployment,
        "Costing: materials",
        false,
        1,
        "",
        "",
        "Shop-app per-order costing; information only."
    ),
    info!(
        CostingPackaging,
        SelfEmployment,
        "Costing: packaging",
        false,
        1,
        "",
        "",
        "Shop-app per-order costing; information only."
    ),
    info!(
        CostingPostage,
        SelfEmployment,
        "Costing: postage",
        false,
        1,
        "",
        "",
        "Shop-app per-order costing; information only."
    ),
    info!(
        CostingOther,
        SelfEmployment,
        "Costing: other",
        false,
        1,
        "",
        "",
        "Shop-app per-order costing; information only."
    ),
    info!(
        RentReceived,
        Property, "Rent received", false, 1, "", "", "Reserved; no summary yet."
    ),
    info!(
        PropertyExpenses,
        Property, "Property expenses", false, 1, "", "", "Reserved; no summary yet."
    ),
];

pub fn catalogue() -> &'static [LineTypeInfo] {
    &CATALOGUE
}

impl IncomeLineType {
    pub fn info(self) -> &'static LineTypeInfo {
        CATALOGUE
            .iter()
            .find(|i| i.line_type == self)
            .expect("every line type has a catalogue row")
    }

    /// Whether a source of `source_type` may carry this line. Property and
    /// other sources share the reserved property lines.
    pub fn valid_for(self, source_type: IncomeSourceType) -> bool {
        match source_type {
            IncomeSourceType::Employment | IncomeSourceType::SelfEmployment => {
                self.info().source_type == source_type
            }
            IncomeSourceType::Property | IncomeSourceType::Other => {
                self.info().source_type == IncomeSourceType::Property
            }
        }
    }

    pub fn is_turnover(self) -> bool {
        matches!(self, Self::Sales | Self::Refunds)
    }

    /// Summed self-employment expense lines (everything summed that is not turnover).
    pub fn is_expense(self) -> bool {
        let i = self.info();
        i.source_type == IncomeSourceType::SelfEmployment && i.summed && !self.is_turnover()
    }

    pub fn is_costing(self) -> bool {
        matches!(
            self,
            Self::CostingMaterials
                | Self::CostingPackaging
                | Self::CostingPostage
                | Self::CostingOther
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::TaxablePay => "taxable_pay",
            Self::GrossPay => "gross_pay",
            Self::PayeTax => "paye_tax",
            Self::EmployeeNi => "employee_ni",
            Self::StudentLoan => "student_loan",
            Self::PostgraduateLoan => "postgraduate_loan",
            Self::EmployeePension => "employee_pension",
            Self::RsuVestValue => "rsu_vest_value",
            Self::RsuTax => "rsu_tax",
            Self::NetPay => "net_pay",
            Self::OtherPayment => "other_payment",
            Self::OtherDeduction => "other_deduction",
            Self::Sales => "sales",
            Self::Refunds => "refunds",
            Self::PlatformFees => "platform_fees",
            Self::PlatformPostage => "platform_postage",
            Self::CostOfGoods => "cost_of_goods",
            Self::Postage => "postage",
            Self::Packaging => "packaging",
            Self::OtherExpenses => "other_expenses",
            Self::CostingMaterials => "costing_materials",
            Self::CostingPackaging => "costing_packaging",
            Self::CostingPostage => "costing_postage",
            Self::CostingOther => "costing_other",
            Self::RentReceived => "rent_received",
            Self::PropertyExpenses => "property_expenses",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        CATALOGUE
            .iter()
            .map(|i| i.line_type)
            .find(|l| l.as_str() == s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_round_trips_through_its_string() {
        for info in catalogue() {
            assert_eq!(
                IncomeLineType::parse(info.line_type.as_str()),
                Some(info.line_type)
            );
            let json = serde_json::to_string(&info.line_type).unwrap();
            assert_eq!(json, format!("\"{}\"", info.line_type.as_str()));
        }
    }

    #[test]
    fn catalogue_has_one_row_per_variant() {
        let mut seen: Vec<_> = catalogue().iter().map(|i| i.line_type).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), catalogue().len());
    }

    #[test]
    fn validity_follows_source_type() {
        assert!(IncomeLineType::PayeTax.valid_for(IncomeSourceType::Employment));
        assert!(!IncomeLineType::Sales.valid_for(IncomeSourceType::Employment));
        assert!(IncomeLineType::Sales.valid_for(IncomeSourceType::SelfEmployment));
        assert!(IncomeLineType::RentReceived.valid_for(IncomeSourceType::Other));
        assert!(!IncomeLineType::PayeTax.valid_for(IncomeSourceType::Property));
    }

    #[test]
    fn costings_are_never_summed() {
        for l in [
            IncomeLineType::CostingMaterials,
            IncomeLineType::CostingPackaging,
            IncomeLineType::CostingPostage,
            IncomeLineType::CostingOther,
        ] {
            assert!(l.is_costing() && !l.info().summed && !l.is_expense());
        }
    }
}
