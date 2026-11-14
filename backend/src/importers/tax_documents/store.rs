//! The seam between the import pipeline and saved income records.

use std::collections::HashSet;

use anyhow::Result;

use super::normalise_source_name;
use crate::model::{IncomeCheckType, IncomeSource, IncomeSourceType, TaxDocumentType};
use crate::storage::db::Db;

/// Duplicate key of a payslip (by period end) or P60 (no date), over the
/// source's normalised name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxRecordKey {
    pub profile_id: String,
    pub source: String,
    pub tax_year: String,
    pub document_type: TaxDocumentType,
    pub date: Option<String>,
}

pub trait TaxRecordStore {
    /// The saved record (payslip) or check (P60) this key matches.
    fn existing_record_id(&self, key: &TaxRecordKey) -> Result<Option<String>>;
    fn known_sources(&self, profile_id: &str) -> Result<Vec<String>>;
    fn linked_transaction_ids(&self) -> Result<HashSet<String>>;
}

/// The profile's employment source whose normalised name is `normalised`.
pub fn find_employment_source(
    db: &Db,
    profile_id: &str,
    normalised: &str,
) -> Result<Option<IncomeSource>> {
    Ok(db
        .list_income_sources(Some(profile_id))?
        .into_iter()
        .find(|s| {
            s.source_type == IncomeSourceType::Employment
                && normalise_source_name(&s.name) == normalised
        }))
}

impl TaxRecordStore for Db {
    fn existing_record_id(&self, key: &TaxRecordKey) -> Result<Option<String>> {
        let Some(source) = find_employment_source(self, &key.profile_id, &key.source)? else {
            return Ok(None);
        };
        match (key.document_type, key.date.as_deref()) {
            (TaxDocumentType::AnnualCertificate, _) => Ok(self
                .find_income_check(&source.id, &key.tax_year, IncomeCheckType::P60)?
                .map(|c| c.id)),
            (TaxDocumentType::Payslip, Some(date)) => {
                self.find_period_record_id(&source.id, &key.tax_year, date)
            }
            (TaxDocumentType::Payslip, None) => Ok(None),
        }
    }

    fn known_sources(&self, profile_id: &str) -> Result<Vec<String>> {
        Ok(self
            .list_income_sources(Some(profile_id))?
            .into_iter()
            .filter(|s| s.source_type == IncomeSourceType::Employment)
            .map(|s| s.name)
            .collect())
    }

    fn linked_transaction_ids(&self) -> Result<HashSet<String>> {
        self.linked_income_transaction_ids()
    }
}

#[cfg(test)]
pub struct FakeStore {
    pub existing: Vec<(TaxRecordKey, String)>,
    pub linked: HashSet<String>,
}

#[cfg(test)]
impl TaxRecordStore for FakeStore {
    fn existing_record_id(&self, key: &TaxRecordKey) -> Result<Option<String>> {
        Ok(self
            .existing
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, id)| id.clone()))
    }

    fn known_sources(&self, _profile_id: &str) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn linked_transaction_ids(&self) -> Result<HashSet<String>> {
        Ok(self.linked.clone())
    }
}
