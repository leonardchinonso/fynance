//! Income routes (plan 30): sources, records, checks, the line-type catalogue,
//! RSU vest candidates and the tax-year summary.
//!
//! Validation lives in `crate::income::validate`, shared with the import and
//! sync writers; every payload problem is a 400 with a specific code.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::income::line_types::{IncomeLineType, catalogue};
use crate::income::summary::{adjustment_lines, compute_tax_year_summary};
use crate::income::validate::{validate_currency, validate_record};
use crate::income::vest::vest_candidates as find_vest_candidates;
use crate::model::{
    IncomeCheck, IncomeCheckFigure, IncomeCheckPayload, IncomeCheckType, IncomeOrigin,
    IncomeRecord, IncomeRecordInsert, IncomeRecordKind, IncomeRecordPayload, IncomeSource,
    IncomeSourcePayload, IncomeSourceType, PatchIncomeCheckBody, PatchIncomeRecordBody,
    PatchIncomeSourceBody, TaxYearSummary, YearEndAdjustmentBody,
};
use crate::server::error::AppError;
use crate::server::routes::tax::validate_tax_year;
use crate::server::state::AppState;
use crate::storage::db::Db;
use crate::util::fx::FxRateMap;
use crate::util::tax_year::TaxYear;

const SYNC_KIND_SHOP_APP: &str = "shop_app";

fn fx_with_history(db: &Db) -> Result<FxRateMap, AppError> {
    let fx = FxRateMap::new(db.get_currencies()?)?;
    let historical = db.get_exchange_rates_for_quote(fx.preferred())?;
    Ok(fx.with_historical(historical))
}

fn require_source(db: &Db, id: &str) -> Result<IncomeSource, AppError> {
    db.get_income_source(id)?.ok_or_else(|| {
        AppError::bad_request(format!("income source {id} not found"), "source_not_found")
    })
}

fn require_profile(db: &Db, id: &str) -> Result<(), AppError> {
    if db.profile_exists(id)? {
        Ok(())
    } else {
        Err(AppError::bad_request(
            format!("profile {id} not found"),
            "profile_not_found",
        ))
    }
}

/// The check type a source of this type keeps, or a 400 for types that have none.
fn check_type_for(source_type: IncomeSourceType) -> Result<IncomeCheckType, AppError> {
    match source_type {
        IncomeSourceType::Employment => Ok(IncomeCheckType::P60),
        IncomeSourceType::SelfEmployment => Ok(IncomeCheckType::AnnualAccounts),
        _ => Err(AppError::bad_request(
            "checks and year-end adjustments exist for employment and self_employment sources only",
            "unsupported_source_type",
        )),
    }
}

// ── GET /api/income/line-types ───────────────────────────────────────────────

pub async fn list_line_types() -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "items": catalogue() })))
}

// ── GET /api/income/vest-candidates?record_id= ───────────────────────────────

#[derive(Debug, Deserialize)]
pub struct VestCandidatesQuery {
    pub record_id: Option<String>,
}

pub async fn vest_candidates(
    State(state): State<AppState>,
    Query(q): Query<VestCandidatesQuery>,
) -> Result<Json<Value>, AppError> {
    let record_id = q
        .record_id
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::bad_request("record_id is required", "missing_record_id"))?;
    let db = state.db();
    let record = db
        .get_income_record(&record_id)?
        .ok_or_else(|| AppError::NotFound(format!("income record {record_id} not found")))?;
    let source = require_source(&db, &record.source_id)?;
    let fx = fx_with_history(&db)?;
    let items = find_vest_candidates(&db, &source, &record, &fx)?;
    Ok(Json(json!({ "items": items })))
}

// ── Sources ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListSourcesQuery {
    pub profile_id: Option<String>,
}

pub async fn list_sources(
    State(state): State<AppState>,
    Query(q): Query<ListSourcesQuery>,
) -> Result<Json<Value>, AppError> {
    let items = state
        .db()
        .list_income_sources(q.profile_id.as_deref().filter(|s| !s.is_empty()))?;
    Ok(Json(json!({ "items": items })))
}

/// Rules for the self-employment-only fields of a source, whether it is being
/// created or patched.
fn validate_source_fields(
    db: &Db,
    source_type: IncomeSourceType,
    account_ids: &[String],
    map: &std::collections::BTreeMap<String, IncomeLineType>,
    sync_kind: Option<&str>,
) -> Result<(), AppError> {
    let business = source_type == IncomeSourceType::SelfEmployment;
    if !business && (!account_ids.is_empty() || !map.is_empty() || sync_kind.is_some()) {
        return Err(AppError::bad_request(
            "account_ids, expense_category_map and sync_kind apply to self_employment sources only",
            "invalid_source_field",
        ));
    }
    for id in account_ids {
        if !db.account_exists(id)? {
            return Err(AppError::bad_request(
                format!("account {id} not found"),
                "account_not_found",
            ));
        }
    }
    for (category_id, line_type) in map {
        // "uncategorized" is the sentinel for transactions with no category.
        if category_id != "uncategorized" && db.get_category_by_id(category_id)?.is_none() {
            return Err(AppError::bad_request(
                format!("category {category_id} not found"),
                "category_not_found",
            ));
        }
        if !line_type.is_expense() {
            return Err(AppError::bad_request(
                format!("{} is not an expense line type", line_type.as_str()),
                "invalid_line_type",
            ));
        }
    }
    if let Some(kind) = sync_kind.filter(|k| *k != SYNC_KIND_SHOP_APP) {
        return Err(AppError::bad_request(
            format!("sync_kind must be {SYNC_KIND_SHOP_APP:?}, got {kind:?}"),
            "invalid_sync_kind",
        ));
    }
    Ok(())
}

pub async fn create_source(
    State(state): State<AppState>,
    Json(body): Json<IncomeSourcePayload>,
) -> Result<(StatusCode, Json<IncomeSource>), AppError> {
    if body.name.trim().is_empty() {
        return Err(AppError::bad_request(
            "name must not be empty",
            "invalid_name",
        ));
    }
    let db = state.db();
    require_profile(&db, &body.profile_id)?;
    validate_source_fields(
        &db,
        body.source_type,
        &body.account_ids,
        &body.expense_category_map,
        body.sync_kind.as_deref(),
    )?;
    let source = db.create_income_source(&body)?;
    Ok((StatusCode::CREATED, Json(source)))
}

pub async fn update_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchIncomeSourceBody>,
) -> Result<Json<IncomeSource>, AppError> {
    if body.name.is_none()
        && body.identifiers.is_none()
        && body.account_ids.is_none()
        && body.expense_category_map.is_none()
        && body.sync_kind.is_none()
        && body.active.is_none()
        && body.notes.is_none()
    {
        return Err(AppError::bad_request(
            "at least one field must be provided",
            "empty_body",
        ));
    }
    if body.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
        return Err(AppError::bad_request(
            "name must not be empty",
            "invalid_name",
        ));
    }
    let db = state.db();
    let current = db
        .get_income_source(&id)?
        .ok_or_else(|| AppError::NotFound(format!("income source {id} not found")))?;
    let empty_map = Default::default();
    validate_source_fields(
        &db,
        current.source_type,
        body.account_ids.as_deref().unwrap_or(&[]),
        body.expense_category_map.as_ref().unwrap_or(&empty_map),
        body.sync_kind.as_ref().and_then(|o| o.as_deref()),
    )?;
    let source = db
        .update_income_source(&id, &body)?
        .ok_or_else(|| AppError::NotFound(format!("income source {id} not found")))?;
    Ok(Json(source))
}

pub async fn delete_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let db = state.db();
    if db.get_income_source(&id)?.is_none() {
        return Err(AppError::NotFound(format!("income source {id} not found")));
    }
    let records = db.count_income_records_for_source(&id)?;
    if records > 0 {
        return Err(AppError::conflict(
            format!("source still has {records} income record(s); delete them first"),
            "source_has_records",
        ));
    }
    db.delete_income_source(&id)?;
    Ok(StatusCode::NO_CONTENT)
}

// ── POST /api/income-sources/:id/year-end-adjustment ─────────────────────────

pub async fn create_year_end_adjustment(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<YearEndAdjustmentBody>,
) -> Result<(StatusCode, Json<IncomeRecord>), AppError> {
    validate_tax_year(&body.tax_year)?;
    let year = TaxYear::parse(&body.tax_year).expect("validated above");
    let db = state.db();
    let source = db
        .get_income_source(&id)?
        .ok_or_else(|| AppError::NotFound(format!("income source {id} not found")))?;
    let check_type = check_type_for(source.source_type)?;
    let Some(check) = db.find_income_check(&id, &year.label(), check_type)? else {
        return Err(AppError::conflict(
            "no check is stored for that source and tax year",
            "no_check",
        ));
    };
    if db.year_end_adjustment_exists(&id, &year.label())? {
        return Err(AppError::conflict(
            "a year-end adjustment already exists for that tax year",
            "adjustment_exists",
        ));
    }
    let fx = fx_with_history(&db)?;
    let lines = adjustment_lines(&db, &source, &check, &year, &fx)?;
    if lines.is_empty() {
        return Err(AppError::conflict(
            "the records already match the check",
            "no_gap",
        ));
    }
    let insert = IncomeRecordInsert {
        source_id: id,
        kind: IncomeRecordKind::YearEndAdjustment,
        period_start: year.start.format("%Y-%m-%d").to_string(),
        period_end: year.end.format("%Y-%m-%d").to_string(),
        pay_date: None,
        currency: fx.preferred().to_string(),
        transaction_ids: Vec::new(),
        document_ids: Vec::new(),
        origin: IncomeOrigin::Adjustment,
        external_ref: None,
        notes: Some("Year-end adjustment to match the check".to_string()),
        line_items: lines,
    };
    let tax_year = validate_record(&db, &source, &insert, true)?;
    let record = db.insert_income_record(&insert, &tax_year)?;
    Ok((StatusCode::CREATED, Json(record)))
}

// ── Records ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListRecordsQuery {
    pub profile_id: Option<String>,
    pub source_id: Option<String>,
    pub tax_year: Option<String>,
}

pub async fn list_records(
    State(state): State<AppState>,
    Query(q): Query<ListRecordsQuery>,
) -> Result<Json<Value>, AppError> {
    let tax_year = q.tax_year.as_deref().filter(|s| !s.is_empty());
    if let Some(y) = tax_year {
        validate_tax_year(y)?;
    }
    let items = state.db().list_income_records(
        q.profile_id.as_deref().filter(|s| !s.is_empty()),
        q.source_id.as_deref().filter(|s| !s.is_empty()),
        tax_year,
    )?;
    Ok(Json(json!({ "items": items })))
}

pub async fn create_record(
    State(state): State<AppState>,
    Json(body): Json<IncomeRecordPayload>,
) -> Result<(StatusCode, Json<IncomeRecord>), AppError> {
    let db = state.db();
    let source = require_source(&db, &body.source_id)?;
    let insert = IncomeRecordInsert {
        source_id: body.source_id,
        kind: IncomeRecordKind::Period,
        period_start: body.period_start,
        period_end: body.period_end,
        pay_date: body.pay_date,
        currency: body.currency,
        transaction_ids: body.transaction_ids,
        document_ids: body.document_ids,
        origin: IncomeOrigin::Manual,
        external_ref: None,
        notes: body.notes,
        line_items: body.line_items,
    };
    let tax_year = validate_record(&db, &source, &insert, true)?;
    let record = db.insert_income_record(&insert, &tax_year)?;
    Ok((StatusCode::CREATED, Json(record)))
}

pub async fn get_record(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<IncomeRecord>, AppError> {
    state
        .db()
        .get_income_record(&id)?
        .map(Json)
        .ok_or_else(|| AppError::NotFound(format!("income record {id} not found")))
}

pub async fn update_record(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchIncomeRecordBody>,
) -> Result<Json<IncomeRecord>, AppError> {
    if body.period_start.is_none()
        && body.period_end.is_none()
        && body.pay_date.is_none()
        && body.currency.is_none()
        && body.transaction_ids.is_none()
        && body.document_ids.is_none()
        && body.notes.is_none()
        && body.line_items.is_none()
    {
        return Err(AppError::bad_request(
            "at least one field must be provided",
            "empty_body",
        ));
    }
    let db = state.db();
    let current = db
        .get_income_record(&id)?
        .ok_or_else(|| AppError::NotFound(format!("income record {id} not found")))?;
    let source = require_source(&db, &current.source_id)?;
    let replace_lines = body.line_items.is_some();
    let insert = IncomeRecordInsert {
        source_id: current.source_id.clone(),
        kind: current.kind,
        period_start: body.period_start.unwrap_or(current.period_start),
        period_end: body.period_end.unwrap_or(current.period_end),
        pay_date: body.pay_date.unwrap_or(current.pay_date),
        currency: body.currency.unwrap_or(current.currency),
        transaction_ids: body.transaction_ids.unwrap_or(current.transaction_ids),
        document_ids: body.document_ids.unwrap_or(current.document_ids),
        origin: current.origin,
        external_ref: current.external_ref,
        notes: body.notes.unwrap_or(current.notes),
        line_items: body.line_items.clone().unwrap_or_default(),
    };
    let tax_year = validate_record(&db, &source, &insert, replace_lines)?;
    let record = db
        .update_income_record(&id, &insert, &tax_year, body.line_items.as_deref())?
        .ok_or_else(|| AppError::NotFound(format!("income record {id} not found")))?;
    Ok(Json(record))
}

pub async fn delete_record(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    if state.db().delete_income_record(&id)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound(format!("income record {id} not found")))
    }
}

// ── Checks ───────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListChecksQuery {
    pub profile_id: Option<String>,
    pub tax_year: Option<String>,
}

pub async fn list_checks(
    State(state): State<AppState>,
    Query(q): Query<ListChecksQuery>,
) -> Result<Json<Value>, AppError> {
    let tax_year = q.tax_year.as_deref().filter(|s| !s.is_empty());
    if let Some(y) = tax_year {
        validate_tax_year(y)?;
    }
    let items = state
        .db()
        .list_income_checks(q.profile_id.as_deref().filter(|s| !s.is_empty()), tax_year)?;
    Ok(Json(json!({ "items": items })))
}

fn validate_check_figures(
    db: &Db,
    source: &IncomeSource,
    figures: &[IncomeCheckFigure],
) -> Result<(), AppError> {
    for f in figures {
        if !f.line_type.valid_for(source.source_type) {
            return Err(AppError::bad_request(
                format!(
                    "line type {} is not valid for a {} source",
                    f.line_type.as_str(),
                    source.source_type.as_str()
                ),
                "invalid_line_type",
            ));
        }
        if f.amount < Decimal::ZERO {
            return Err(AppError::bad_request(
                format!("amount must not be negative, got {}", f.amount),
                "negative_amount",
            ));
        }
        validate_currency(db, &f.currency)?;
    }
    Ok(())
}

pub async fn create_check(
    State(state): State<AppState>,
    Json(body): Json<IncomeCheckPayload>,
) -> Result<(StatusCode, Json<IncomeCheck>), AppError> {
    validate_tax_year(&body.tax_year)?;
    let db = state.db();
    let source = require_source(&db, &body.source_id)?;
    let expected = check_type_for(source.source_type)?;
    if body.check_type != expected {
        return Err(AppError::bad_request(
            format!(
                "a {} source takes a {} check",
                source.source_type.as_str(),
                expected.as_str()
            ),
            "invalid_check_type",
        ));
    }
    validate_check_figures(&db, &source, &body.figures)?;
    if db
        .find_income_check(&body.source_id, &body.tax_year, body.check_type)?
        .is_some()
    {
        return Err(AppError::conflict(
            "that source already has this check for the tax year",
            "check_exists",
        ));
    }
    let check = db.create_income_check(
        &body.source_id,
        &body.tax_year,
        body.check_type,
        &body.figures,
        &body.document_ids,
        body.notes.as_deref(),
    )?;
    Ok((StatusCode::CREATED, Json(check)))
}

pub async fn update_check(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchIncomeCheckBody>,
) -> Result<Json<IncomeCheck>, AppError> {
    if body.figures.is_none() && body.document_ids.is_none() && body.notes.is_none() {
        return Err(AppError::bad_request(
            "at least one field must be provided",
            "empty_body",
        ));
    }
    let db = state.db();
    let current = db
        .get_income_check(&id)?
        .ok_or_else(|| AppError::NotFound(format!("income check {id} not found")))?;
    let source = require_source(&db, &current.source_id)?;
    if let Some(figures) = &body.figures {
        validate_check_figures(&db, &source, figures)?;
    }
    let notes = match &body.notes {
        Some(n) => n.as_deref(),
        None => current.notes.as_deref(),
    };
    let check = db
        .update_income_check(
            &id,
            body.figures.as_deref().unwrap_or(&current.figures),
            body.document_ids
                .as_deref()
                .unwrap_or(&current.document_ids),
            notes,
        )?
        .ok_or_else(|| AppError::NotFound(format!("income check {id} not found")))?;
    Ok(Json(check))
}

pub async fn delete_check(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    if state.db().delete_income_check(&id)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound(format!("income check {id} not found")))
    }
}

// ── GET /api/tax/summary/:tax_year?profile_id= ───────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SummaryQuery {
    pub profile_id: Option<String>,
}

pub async fn get_tax_year_summary(
    State(state): State<AppState>,
    Path(tax_year): Path<String>,
    Query(q): Query<SummaryQuery>,
) -> Result<Json<TaxYearSummary>, AppError> {
    validate_tax_year(&tax_year)?;
    let profile_id = q
        .profile_id
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::bad_request("profile_id is required", "missing_profile_id"))?;
    let db = state.db();
    require_profile(&db, &profile_id)?;
    let fx = fx_with_history(&db)?;
    Ok(Json(compute_tax_year_summary(
        &db,
        &profile_id,
        &tax_year,
        &fx,
    )?))
}
