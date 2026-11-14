//! `POST /api/income-records/import`: commit reviewed payslips and P60s
//! (plan 30 section 9). Valid rows are written in one SQLite transaction;
//! rows that fail validation are reported per row and never fail the batch.

use std::collections::HashSet;

use axum::Json;
use axum::extract::State;

use crate::importers::tax_documents::{normalise_source_name, store::find_employment_source};
use crate::income::line_types::IncomeLineType;
use crate::income::validate::{validate_currency, validate_record};
use crate::model::{
    IncomeBasis, IncomeCheckFigure, IncomeCheckType, IncomeLineItemInput, IncomeOrigin,
    IncomeRecordInsert, IncomeRecordKind, IncomeSource, IncomeSourceIdentifiers,
    IncomeSourcePayload, IncomeSourceType, PatchIncomeSourceBody, TaxDocumentDraft,
    TaxDocumentImportError, TaxDocumentType, TaxDocumentsImportPayload, TaxDocumentsImportResult,
};
use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::storage::db::Db;
use crate::util::tax_year::TaxYear;

enum RowOutcome {
    Inserted,
    Duplicate,
    Rejected(String),
}

pub async fn import_income_records(
    State(state): State<AppState>,
    Json(payload): Json<TaxDocumentsImportPayload>,
) -> Result<Json<TaxDocumentsImportResult>, AppError> {
    let db = state.db();
    if !db.profile_exists(&payload.profile_id)? {
        return Err(AppError::bad_request(
            format!("profile {} not found", payload.profile_id),
            "profile_not_found",
        ));
    }
    let result = db.in_transaction(|db| commit_documents(db, &payload))?;
    Ok(Json(result))
}

fn commit_documents(
    db: &Db,
    payload: &TaxDocumentsImportPayload,
) -> Result<TaxDocumentsImportResult, AppError> {
    let mut linked = db.linked_income_transaction_ids()?;
    let mut result = TaxDocumentsImportResult {
        inserted: 0,
        duplicates: 0,
        errors: Vec::new(),
    };
    for (index, draft) in payload.documents.iter().enumerate() {
        match import_row(db, &payload.profile_id, draft, &mut linked)? {
            RowOutcome::Inserted => result.inserted += 1,
            RowOutcome::Duplicate => result.duplicates += 1,
            RowOutcome::Rejected(reason) => {
                result.errors.push(TaxDocumentImportError { index, reason })
            }
        }
    }
    Ok(result)
}

/// A client-side validation message becomes a row rejection; anything else
/// aborts the batch.
fn rejection(e: AppError) -> Result<RowOutcome, AppError> {
    match e {
        AppError::BadRequest { message, .. } => Ok(RowOutcome::Rejected(message)),
        other => Err(other),
    }
}

fn reject(reason: impl Into<String>) -> Result<RowOutcome, AppError> {
    Ok(RowOutcome::Rejected(reason.into()))
}

fn import_row(
    db: &Db,
    profile_id: &str,
    draft: &TaxDocumentDraft,
    linked: &mut HashSet<String>,
) -> Result<RowOutcome, AppError> {
    let name = draft.source_name.trim();
    if name.is_empty() {
        return reject("employer name is missing");
    }
    if draft.line_items.is_empty() {
        return reject("no line items");
    }
    for id in &draft.document_ids {
        if db.get_document(id)?.is_none() {
            return reject(format!("document {id} not found"));
        }
    }
    let existing = find_employment_source(db, profile_id, &normalise_source_name(name))?;
    match draft.document_type {
        TaxDocumentType::Payslip => import_payslip(db, profile_id, draft, existing, linked),
        TaxDocumentType::AnnualCertificate => import_p60(db, profile_id, draft, existing),
    }
}

fn import_payslip(
    db: &Db,
    profile_id: &str,
    draft: &TaxDocumentDraft,
    existing: Option<IncomeSource>,
    linked: &mut HashSet<String>,
) -> Result<RowOutcome, AppError> {
    let Some(period_end) = draft.period_end.clone().or_else(|| draft.pay_date.clone()) else {
        return reject("a payslip needs a pay date or a period end");
    };
    let transaction_ids: Vec<String> = draft.transaction_id.iter().cloned().collect();
    if let Some(id) = transaction_ids.iter().find(|id| linked.contains(*id)) {
        return reject(format!("transaction {id} is already linked to a record"));
    }
    let mut insert = IncomeRecordInsert {
        source_id: existing.as_ref().map(|s| s.id.clone()).unwrap_or_default(),
        kind: IncomeRecordKind::Period,
        period_start: draft
            .period_start
            .clone()
            .unwrap_or_else(|| period_end.clone()),
        period_end,
        pay_date: draft.pay_date.clone(),
        currency: draft.currency.clone(),
        transaction_ids,
        document_ids: draft.document_ids.clone(),
        origin: IncomeOrigin::Parse,
        external_ref: None,
        notes: draft.notes.clone(),
        line_items: draft
            .line_items
            .iter()
            .map(|i| IncomeLineItemInput {
                line_type: i.line_type,
                amount: i.amount,
                currency: None,
                basis: i.basis,
                label: i.label.clone(),
                investment_event_id: None,
            })
            .collect(),
    };
    let validation_source = existing
        .clone()
        .unwrap_or_else(|| unsaved_source(profile_id, draft));
    let tax_year = match validate_record(db, &validation_source, &insert, true) {
        Ok(year) => year,
        Err(e) => return rejection(e),
    };
    if let Some(source) = &existing {
        if db
            .find_period_record_id(&source.id, &tax_year, &insert.period_end)?
            .is_some()
        {
            return Ok(RowOutcome::Duplicate);
        }
    }
    insert.source_id = ensure_source(db, profile_id, draft, existing)?.id;
    db.insert_income_record_inner(&insert, &tax_year)?;
    linked.extend(insert.transaction_ids);
    Ok(RowOutcome::Inserted)
}

fn import_p60(
    db: &Db,
    profile_id: &str,
    draft: &TaxDocumentDraft,
    existing: Option<IncomeSource>,
) -> Result<RowOutcome, AppError> {
    let Some(year) = TaxYear::parse(&draft.tax_year) else {
        return reject(format!(
            "tax_year must look like '2024-25', got {:?}",
            draft.tax_year
        ));
    };
    let tax_year = year.label();
    if let Err(e) = validate_currency(db, &draft.currency) {
        return rejection(e);
    }
    let figures: Vec<IncomeCheckFigure> = draft
        .line_items
        .iter()
        .filter(|i| i.basis == IncomeBasis::Period)
        .map(|i| IncomeCheckFigure {
            line_type: i.line_type,
            amount: i.amount,
            currency: draft.currency.clone(),
        })
        .collect();
    if figures
        .iter()
        .any(|f| !f.line_type.valid_for(IncomeSourceType::Employment))
    {
        return reject("a P60 holds employment figures only");
    }
    for needed in [IncomeLineType::TaxablePay, IncomeLineType::PayeTax] {
        if !figures.iter().any(|f| f.line_type == needed) {
            return reject(format!("a P60 needs a {} figure", needed.as_str()));
        }
    }
    if let Some(source) = &existing {
        if db
            .find_income_check(&source.id, &tax_year, IncomeCheckType::P60)?
            .is_some()
        {
            return reject(format!(
                "a P60 for {} in {tax_year} is already saved",
                source.name
            ));
        }
    }
    let source = ensure_source(db, profile_id, draft, existing)?;
    db.create_income_check(
        &source.id,
        &tax_year,
        IncomeCheckType::P60,
        &figures,
        &draft.document_ids,
        draft.notes.as_deref(),
    )?;
    Ok(RowOutcome::Inserted)
}

fn paye_reference(draft: &TaxDocumentDraft) -> Option<String> {
    draft
        .paye_reference
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
}

fn employment_payload(profile_id: &str, draft: &TaxDocumentDraft) -> IncomeSourcePayload {
    IncomeSourcePayload {
        profile_id: profile_id.to_string(),
        source_type: IncomeSourceType::Employment,
        name: draft.source_name.trim().to_string(),
        identifiers: IncomeSourceIdentifiers {
            paye_reference: paye_reference(draft),
            ..Default::default()
        },
        account_ids: Vec::new(),
        expense_category_map: Default::default(),
        sync_kind: None,
        active: true,
        notes: None,
    }
}

/// Stand-in for a source that does not exist yet, so a row can be validated
/// before anything is written.
fn unsaved_source(profile_id: &str, draft: &TaxDocumentDraft) -> IncomeSource {
    let p = employment_payload(profile_id, draft);
    IncomeSource {
        id: String::new(),
        profile_id: p.profile_id,
        source_type: p.source_type,
        name: p.name,
        identifiers: p.identifiers,
        account_ids: p.account_ids,
        expense_category_map: p.expense_category_map,
        sync_kind: None,
        active: true,
        notes: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

/// The saved employment source for a row, created when new; a missing PAYE
/// reference is filled in from the draft.
fn ensure_source(
    db: &Db,
    profile_id: &str,
    draft: &TaxDocumentDraft,
    existing: Option<IncomeSource>,
) -> Result<IncomeSource, AppError> {
    let Some(source) = existing else {
        return Ok(db.create_income_source(&employment_payload(profile_id, draft))?);
    };
    match paye_reference(draft) {
        Some(reference) if source.identifiers.paye_reference.is_none() => {
            let mut identifiers = source.identifiers.clone();
            identifiers.paye_reference = Some(reference);
            let patch = PatchIncomeSourceBody {
                name: None,
                identifiers: Some(identifiers),
                account_ids: None,
                expense_category_map: None,
                sync_kind: None,
                active: None,
                notes: None,
            };
            Ok(db
                .update_income_source(&source.id, &patch)?
                .unwrap_or(source))
        }
        _ => Ok(source),
    }
}
