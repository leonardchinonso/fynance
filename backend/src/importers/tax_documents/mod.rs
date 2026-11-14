//! Tax-document import: payslips and P60s read from PDFs or photos
//! (plan 29 Part A, storage per plan 30).
//!
//! `parser` makes the LLM call, `validate` turns its output into preview
//! rows, `link` suggests the bank credit for each payslip's net pay,
//! `reconcile` compares payslips with the P60, and `store` is the seam to
//! saved income records.

pub mod link;
pub mod parser;
pub mod reconcile;
pub mod store;
pub mod validate;

use chrono::{Duration, NaiveDate};

use crate::model::{
    TaxDocumentIngestionResult, TaxDocumentPreviewRow, TaxDocumentType, TransactionPreviewStatus,
};
use crate::storage::db::Db;
use parser::ParsedTaxDocument;
use store::{TaxRecordKey, TaxRecordStore};

/// Lowercase, drop punctuation, collapse whitespace and drop a trailing
/// company suffix, so "Example Widgets Ltd." and "example widgets" match.
pub fn normalise_source_name(name: &str) -> String {
    let cleaned: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let mut words: Vec<&str> = cleaned.split_whitespace().collect();
    if words.len() > 1
        && words
            .last()
            .is_some_and(|w| matches!(*w, "ltd" | "limited" | "plc" | "llp" | "inc"))
    {
        words.pop();
    }
    words.join(" ")
}

/// The duplicate key of a row (plan 29 decision 10). `None` for a row too
/// incomplete to key.
pub fn record_key(profile_id: &str, row: &TaxDocumentPreviewRow) -> Option<TaxRecordKey> {
    let d = &row.draft;
    let date = match d.document_type {
        TaxDocumentType::AnnualCertificate => None,
        TaxDocumentType::Payslip => Some(d.period_end.clone().or_else(|| d.pay_date.clone())?),
    };
    Some(TaxRecordKey {
        profile_id: profile_id.to_string(),
        source: normalise_source_name(&d.source_name),
        tax_year: d.tax_year.clone(),
        document_type: d.document_type,
        date,
    })
}

/// Mark rows that repeat an earlier row in the same preview, or that match a
/// saved record, as duplicates. Error rows are left alone.
pub fn flag_duplicates(
    profile_id: &str,
    rows: &mut [TaxDocumentPreviewRow],
    store: &dyn TaxRecordStore,
) -> anyhow::Result<()> {
    let mut seen: Vec<(TaxRecordKey, usize)> = Vec::new();
    for (i, row) in rows.iter_mut().enumerate() {
        if row.status == TransactionPreviewStatus::Error {
            continue;
        }
        let Some(key) = record_key(profile_id, row) else {
            continue;
        };
        if let Some((_, first)) = seen.iter().find(|(k, _)| *k == key) {
            row.status = TransactionPreviewStatus::Duplicate;
            row.same_key_as_row = Some(*first);
            continue;
        }
        if let Some(existing) = store.existing_record_id(&key)? {
            row.status = TransactionPreviewStatus::Duplicate;
            row.existing_id = Some(existing);
        }
        seen.push((key, i));
    }
    Ok(())
}

/// Turn the parsed documents of one request into the preview: validate,
/// flag duplicates, suggest bank credits for net pay and reconcile with any
/// P60. Needs the database for the saved-record and credit lookups.
pub fn assemble(
    db: &Db,
    profile_id: &str,
    parsed: Vec<ParsedTaxDocument>,
    min_row_confidence: f32,
    docs: &validate::SourceDocuments,
) -> anyhow::Result<TaxDocumentIngestionResult> {
    let mut rows = validate::validate_documents(parsed, min_row_confidence, docs);
    flag_duplicates(profile_id, &mut rows, db)?;
    link_net_pay(db, profile_id, &mut rows)?;
    let reconciliation = reconcile::reconcile(&rows);
    let known_sources = db.known_sources(profile_id)?;
    Ok(summarise(profile_id, rows, reconciliation, known_sources))
}

fn link_net_pay(
    db: &Db,
    profile_id: &str,
    rows: &mut [TaxDocumentPreviewRow],
) -> anyhow::Result<()> {
    let dates: Vec<NaiveDate> = rows
        .iter()
        .filter(|r| {
            r.status == TransactionPreviewStatus::New
                && r.draft.document_type == TaxDocumentType::Payslip
        })
        .filter_map(link::expected_credit_date)
        .collect();
    let (Some(first), Some(last)) = (dates.iter().min(), dates.iter().max()) else {
        return Ok(());
    };
    let from = *first - Duration::days(link::WINDOW_DAYS);
    let to = *last + Duration::days(link::WINDOW_DAYS);

    let mut linked = db.linked_transaction_ids()?;
    let profile_accounts: Vec<String> = db
        .get_accounts(Some(profile_id))?
        .into_iter()
        .map(|a| a.id)
        .collect();
    if !profile_accounts.is_empty() {
        let credits = db.find_credit_transactions(from, to, Some(&profile_accounts))?;
        link::assign_net_pay_candidates(rows, &credits, &mut linked);
    }
    if rows.iter().any(|r| {
        r.status == TransactionPreviewStatus::New
            && r.draft.document_type == TaxDocumentType::Payslip
            && r.net_pay_candidates.is_empty()
    }) {
        let credits = db.find_credit_transactions(from, to, None)?;
        link::assign_net_pay_candidates(rows, &credits, &mut linked);
    }
    Ok(())
}

pub fn summarise(
    profile_id: &str,
    rows: Vec<TaxDocumentPreviewRow>,
    reconciliation: Vec<crate::model::TaxReconciliation>,
    known_sources: Vec<String>,
) -> TaxDocumentIngestionResult {
    let count_of = |s: TransactionPreviewStatus| rows.iter().filter(|r| r.status == s).count();
    TaxDocumentIngestionResult {
        count: rows.len(),
        new: count_of(TransactionPreviewStatus::New),
        duplicate: count_of(TransactionPreviewStatus::Duplicate),
        errors: count_of(TransactionPreviewStatus::Error),
        profile_id: profile_id.to_string(),
        rows,
        reconciliation,
        known_sources,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_company_names() {
        assert_eq!(
            normalise_source_name("Example Widgets Ltd."),
            "example widgets"
        );
        assert_eq!(
            normalise_source_name("  EXAMPLE   widgets LIMITED"),
            "example widgets"
        );
        assert_eq!(
            normalise_source_name("Example, Widgets & Co"),
            "example widgets co"
        );
        assert_eq!(normalise_source_name("Inc"), "inc");
    }
}
