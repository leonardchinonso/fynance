//! Transaction routes: list, by-category aggregation, categories list, PATCH.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{CategorySource, Transaction, TransactionDirection};
use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::server::validation::{
    parse_date, split_csv_param, validate_date_range, validate_pagination,
};
use crate::storage::TransactionFilters;

// ── GET /api/transactions ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListTransactionsQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub accounts: Option<String>,
    pub categories: Option<String>,
    pub search: Option<String>,
    pub profile_id: Option<String>,
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

#[derive(Debug, Serialize)]
pub struct TransactionListResponse {
    pub data: Vec<Transaction>,
    pub total: u64,
    pub page: u32,
    pub limit: u32,
}

pub async fn list_transactions(
    State(state): State<AppState>,
    Query(q): Query<ListTransactionsQuery>,
) -> Result<Json<TransactionListResponse>, AppError> {
    let start = q.start.as_deref().map(parse_date).transpose()?;
    let end = q.end.as_deref().map(parse_date).transpose()?;
    if let (Some(s), Some(e)) = (start, end) {
        validate_date_range(s, e)?;
    }
    validate_pagination(q.page, q.limit)?;

    let filters = TransactionFilters {
        start,
        end,
        accounts: q.accounts.as_deref().and_then(split_csv_param),
        categories: q.categories.as_deref().and_then(split_csv_param),
        search: q.search.filter(|s| !s.is_empty()),
        profile_id: q.profile_id.filter(|s| !s.is_empty()),
        page: q.page,
        limit: q.limit,
    };

    let (data, total) = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_transactions(&filters)?
    };

    Ok(Json(TransactionListResponse {
        data,
        total,
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

    let totals = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_transactions_by_category(&filters, direction)?
    };

    Ok(Json(serde_json::to_value(totals)?))
}

// ── GET /api/transactions/categories ─────────────────────────────────────────

pub async fn list_categories(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let categories = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_all_categories()?
    };
    Ok(Json(serde_json::to_value(categories)?))
}

// ── PATCH /api/transactions/:id ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PatchTransactionBody {
    pub category: Option<String>,
    pub notes: Option<String>,
}

pub async fn patch_transaction(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchTransactionBody>,
) -> Result<Json<Transaction>, AppError> {
    if body.category.is_none() && body.notes.is_none() {
        return Err(AppError::bad_request(
            "request body must include at least one of: category, notes",
            "empty_body",
        ));
    }
    if let Some(cat) = &body.category {
        if cat.is_empty() {
            return Err(AppError::bad_request(
                "category must not be an empty string",
                "invalid_category",
            ));
        }
    }

    let db = state.db.lock().expect("db mutex poisoned");

    // Confirm the transaction exists first.
    let tx = db
        .get_transaction_by_id(&id)?
        .ok_or_else(|| AppError::NotFound(format!("transaction {id} not found")))?;

    if let Some(cat) = &body.category {
        db.update_transaction_category(&id, cat, CategorySource::Manual)?;
    }
    if let Some(notes) = &body.notes {
        db.update_transaction_notes(&id, Some(notes.as_str()))?;
    }

    // Re-fetch the updated row.
    let updated = db.get_transaction_by_id(&id)?.unwrap_or(tx); // fall back to pre-update copy if re-fetch fails
    Ok(Json(updated))
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
