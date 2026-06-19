//! Transaction routes: list, by-category aggregation, categories list, PATCH.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::Value;

use crate::model::{CategorySource, Paginated, Transaction, TransactionDirection};
use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::server::validation::{
    parse_date, split_csv_param, validate_date_range, validate_pagination,
};
use crate::storage::{SortDir, TransactionFilters, TransactionSort};
use crate::util::fx::FxRateMap;

// ── GET /api/transactions ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListTransactionsQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub accounts: Option<String>,
    pub categories: Option<String>,
    pub search: Option<String>,
    pub profile_id: Option<String>,
    /// Filter by category source: "rule" | "agent" | "manual"
    pub category_source: Option<String>,
    /// Sort column: "date" | "amount" | "category". Default is newest-first by date.
    pub sort: Option<String>,
    /// Sort direction: "asc" | "desc". Default is "desc".
    pub sort_dir: Option<String>,
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_page() -> u32 {
    1
}
fn default_limit() -> u32 {
    25
}

pub async fn list_transactions(
    State(state): State<AppState>,
    Query(q): Query<ListTransactionsQuery>,
) -> Result<Json<Paginated<Transaction>>, AppError> {
    let start = q.start.as_deref().map(parse_date).transpose()?;
    let end = q.end.as_deref().map(parse_date).transpose()?;
    if let (Some(s), Some(e)) = (start, end) {
        validate_date_range(s, e)?;
    }
    validate_pagination(q.page, q.limit)?;

    let category_source = match q.category_source.as_deref() {
        None | Some("") => None,
        Some(raw) => Some(CategorySource::parse(raw).ok_or_else(|| {
            AppError::bad_request(
                "category_source must be one of: rule, agent, manual",
                "invalid_category_source",
            )
        })?),
    };

    let sort = match q.sort.as_deref() {
        None | Some("") => None,
        Some(raw) => Some(TransactionSort::parse(raw).ok_or_else(|| {
            AppError::bad_request(
                "sort must be one of: date, amount, category",
                "invalid_sort",
            )
        })?),
    };
    let sort_dir = match q.sort_dir.as_deref() {
        None | Some("") => SortDir::Desc,
        Some(raw) => SortDir::parse(raw).ok_or_else(|| {
            AppError::bad_request("sort_dir must be 'asc' or 'desc'", "invalid_sort_dir")
        })?,
    };

    let filters = TransactionFilters {
        start,
        end,
        accounts: q.accounts.as_deref().and_then(split_csv_param),
        categories: q.categories.as_deref().and_then(split_csv_param),
        search: q.search.filter(|s| !s.is_empty()),
        profile_id: q.profile_id.filter(|s| !s.is_empty()),
        category_source,
        sort,
        sort_dir,
        page: q.page,
        limit: q.limit,
    };

    let (data, total) = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_transactions(&filters)?
    };

    Ok(Json(Paginated {
        data,
        total: total as u32,
        page: q.page,
        limit: q.limit,
    }))
}

// ── GET /api/transactions/by-category ────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ByCategoryQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub accounts: Option<String>,
    pub categories: Option<String>,
    pub profile_id: Option<String>,
    /// Optional sign filter: `outflow` or `income`. When omitted the
    /// aggregation returns signed net sums per category.
    pub direction: Option<String>,
}

pub async fn transactions_by_category(
    State(state): State<AppState>,
    Query(q): Query<ByCategoryQuery>,
) -> Result<Json<Value>, AppError> {
    let start = q.start.as_deref().map(parse_date).transpose()?;
    let end = q.end.as_deref().map(parse_date).transpose()?;
    if let (Some(s), Some(e)) = (start, end) {
        validate_date_range(s, e)?;
    }

    let direction = match q.direction.as_deref() {
        None | Some("") => None,
        Some(raw) => Some(TransactionDirection::parse(raw).ok_or_else(|| {
            AppError::bad_request(
                "direction must be 'outflow' or 'income'",
                "invalid_direction",
            )
        })?),
    };

    let filters = TransactionFilters {
        start,
        end,
        accounts: q.accounts.as_deref().and_then(split_csv_param),
        categories: q.categories.as_deref().and_then(split_csv_param),
        profile_id: q.profile_id.filter(|s| !s.is_empty()),
        ..TransactionFilters::default()
    };

    let (totals, preferred_currency) = {
        let db = state.db.lock().expect("db mutex poisoned");
        let currencies = db.get_currencies()?;
        let fx = FxRateMap::new(currencies)?;
        let totals = db.get_transactions_by_category(&filters, direction, &fx)?;
        let preferred = fx.preferred().to_string();
        (totals, preferred)
    };

    Ok(Json(serde_json::json!({
        "preferred_currency": preferred_currency,
        "rows": totals
    })))
}

// ── GET /api/transactions/categories ─────────────────────────────────────────

pub async fn list_categories(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let tree = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_all_categories()?
    };
    Ok(Json(serde_json::to_value(tree)?))
}

// ── PATCH /api/transactions/:id ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PatchTransactionBody {
    /// FK to categories.id (must be a leaf)
    pub category_id: Option<String>,
    pub notes: Option<String>,
    pub exclude_from_summary: Option<bool>,
}

pub async fn patch_transaction(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchTransactionBody>,
) -> Result<Json<Transaction>, AppError> {
    if body.category_id.is_none() && body.notes.is_none() && body.exclude_from_summary.is_none() {
        return Err(AppError::bad_request(
            "request body must include at least one of: category_id, notes, exclude_from_summary",
            "empty_body",
        ));
    }

    let db = state.db.lock().expect("db mutex poisoned");

    let tx = db
        .get_transaction_by_id(&id)?
        .ok_or_else(|| AppError::NotFound(format!("transaction {id} not found")))?;

    if let Some(ref cat_id) = body.category_id {
        db.update_transaction_category(&id, cat_id, CategorySource::Manual)?;
    }

    if let Some(ref notes) = body.notes {
        db.update_transaction_notes(&id, Some(notes.as_str()))?;
    }

    if let Some(exclude) = body.exclude_from_summary {
        db.update_transaction_exclude_summary(&id, exclude)?;
    }

    let updated = db.get_transaction_by_id(&id)?.unwrap_or(tx);
    Ok(Json(updated))
}

// ── DELETE /api/transactions/:id ──────────────────────────────────────────────

pub async fn delete_transaction(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let db = state.db.lock().expect("db mutex poisoned");
    let deleted = db.delete_transaction(&id)?;
    if deleted == 0 {
        return Err(AppError::NotFound(format!("transaction {id} not found")));
    }
    Ok(Json(serde_json::json!({ "ok": true, "deleted": deleted })))
}

// ── DELETE /api/transactions ──────────────────────────────────────────────────
//
// Bulk hard-delete. Provide exactly one of `ids` (delete those transactions) or
// `account_id` (delete every transaction for that account, e.g. to clear an
// account before deleting it).

#[derive(Debug, Deserialize)]
pub struct BulkDeleteBody {
    pub ids: Option<Vec<String>>,
    pub account_id: Option<String>,
}

pub async fn bulk_delete_transactions(
    State(state): State<AppState>,
    Json(body): Json<BulkDeleteBody>,
) -> Result<Json<Value>, AppError> {
    let account_id = body.account_id.filter(|s| !s.is_empty());
    let ids = body.ids.filter(|v| !v.is_empty());

    let db = state.db.lock().expect("db mutex poisoned");
    let deleted = match (ids, account_id) {
        (Some(ids), None) => db.delete_transactions(&ids)?,
        (None, Some(account_id)) => db.delete_transactions_for_account(&account_id)?,
        (Some(_), Some(_)) => {
            return Err(AppError::bad_request(
                "provide either ids or account_id, not both",
                "ambiguous_request",
            ));
        }
        (None, None) => {
            return Err(AppError::bad_request(
                "request body must include a non-empty ids array or an account_id",
                "empty_body",
            ));
        }
    };

    Ok(Json(serde_json::json!({ "ok": true, "deleted": deleted })))
}

// ── GET /api/transactions/accounts (legacy alias) ────────────────────────────

pub async fn list_transaction_accounts(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let accounts = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_accounts(None)?
    };
    Ok(Json(serde_json::to_value(accounts)?))
}
