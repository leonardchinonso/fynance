//! Account routes:
//!   GET    /api/accounts
//!   POST   /api/accounts
//!   PATCH  /api/accounts/:id
//!   DELETE /api/accounts/:id
//!   PATCH  /api/accounts/:id/balance

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::Value;

use crate::model::{Account, AccountType};
use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::server::validation::validate_currency;

// ── GET /api/accounts ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListAccountsQuery {
    pub profile_id: Option<String>,
}

pub async fn list_accounts(
    State(state): State<AppState>,
    Query(q): Query<ListAccountsQuery>,
) -> Result<Json<Value>, AppError> {
    let profile_id = q.profile_id.as_deref().filter(|s| !s.is_empty());
    let accounts = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_accounts(profile_id)?
    };
    Ok(Json(serde_json::to_value(accounts)?))
}

// ── POST /api/accounts ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateAccountBody {
    pub id: String,
    pub name: String,
    pub institution: String,
    #[serde(rename = "type")]
    pub account_type: String,
    pub currency: Option<String>,
    #[serde(default)]
    pub profile_ids: Vec<String>,
    pub notes: Option<String>,
}

pub async fn create_account(
    State(state): State<AppState>,
    Json(body): Json<CreateAccountBody>,
) -> Result<Json<Account>, AppError> {
    if body.id.is_empty() {
        return Err(AppError::bad_request(
            "id must not be empty",
            "invalid_account_id",
        ));
    }

    let account_type = AccountType::parse(&body.account_type).ok_or_else(|| {
        AppError::bad_request(
            format!("invalid account type: {}", body.account_type),
            "invalid_account_type",
        )
    })?;

    // Normalize profile_ids: empty -> ["default"]
    let profile_ids = if body.profile_ids.is_empty() {
        vec!["default".to_string()]
    } else {
        body.profile_ids.clone()
    };

    let currency = body.currency.as_deref().unwrap_or("GBP");

    let is_available = crate::storage::db::is_available_account(&account_type);
    let account = Account {
        id: body.id.clone(),
        name: body.name,
        institution: body.institution,
        account_type,
        currency: currency.to_string(),
        balance: None,
        balance_date: None,
        is_active: true,
        notes: body.notes,
        profile_ids,
        is_stale: None,
        is_available,
    };

    {
        let db = state.db.lock().expect("db mutex poisoned");
        if db.account_exists(&body.id)? {
            return Err(AppError::conflict(
                format!("account {} already exists", body.id),
                "account_exists",
            ));
        }
        validate_currency(&db, currency)?;
        db.create_account(&account)?;
    }

    Ok(Json(account))
}

// ── PATCH /api/accounts/:id ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PatchAccountBody {
    pub name: Option<String>,
    pub institution: Option<String>,
    #[serde(rename = "type")]
    pub account_type: Option<String>,
    pub currency: Option<String>,
    pub is_active: Option<bool>,
    pub profile_ids: Option<Vec<String>>,
    pub notes: Option<String>,
}

pub async fn update_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchAccountBody>,
) -> Result<Json<Account>, AppError> {
    if body.name.is_none()
        && body.institution.is_none()
        && body.account_type.is_none()
        && body.currency.is_none()
        && body.is_active.is_none()
        && body.profile_ids.is_none()
        && body.notes.is_none()
    {
        return Err(AppError::bad_request(
            "at least one field must be provided",
            "empty_body",
        ));
    }

    if let Some(ref n) = body.name {
        if n.trim().is_empty() {
            return Err(AppError::bad_request(
                "name must not be empty",
                "invalid_name",
            ));
        }
    }
    if let Some(ref inst) = body.institution {
        if inst.trim().is_empty() {
            return Err(AppError::bad_request(
                "institution must not be empty",
                "invalid_institution",
            ));
        }
    }

    let account_type = body
        .account_type
        .as_deref()
        .map(|s| {
            AccountType::parse(s).ok_or_else(|| {
                AppError::bad_request(
                    format!("invalid account type: {s}"),
                    "invalid_account_type",
                )
            })
        })
        .transpose()?;

    let db = state.db.lock().expect("db mutex poisoned");
    if !db.account_exists(&id)? {
        return Err(AppError::NotFound(format!("account {id} not found")));
    }
    if let Some(ref c) = body.currency {
        validate_currency(&db, c)?;
    }

    let notes_arg = body.notes.as_deref().map(Some);
    let updated = db.update_account(
        &id,
        body.name.as_deref(),
        body.institution.as_deref(),
        account_type.as_ref(),
        body.currency.as_deref(),
        body.is_active,
        notes_arg,
        body.profile_ids.as_deref(),
    )?;

    Ok(Json(updated))
}

// ── DELETE /api/accounts/:id ─────────────────────────────────────────────────

pub async fn delete_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let db = state.db.lock().expect("db mutex poisoned");
    if !db.account_exists(&id)? {
        return Err(AppError::NotFound(format!("account {id} not found")));
    }
    let tx_count = db.count_transactions_for_account(&id)?;
    let holding_count = db.count_holdings_for_account(&id)?;
    if tx_count > 0 || holding_count > 0 {
        return Err(AppError::conflict(
            format!(
                "account {id} is still referenced by {tx_count} transaction(s) and {holding_count} holding(s); remove them first"
            ),
            "account_in_use",
        ));
    }
    db.delete_account(&id)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── PATCH /api/accounts/:id/balance ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SetBalanceBody {
    pub balance: String,
    pub date: String,
}

pub async fn set_account_balance(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SetBalanceBody>,
) -> Result<Json<Value>, AppError> {
    let balance = body.balance.parse::<rust_decimal::Decimal>().map_err(|_| {
        AppError::bad_request(
            format!("invalid balance: {}", body.balance),
            "invalid_decimal",
        )
    })?;

    let date = crate::server::validation::parse_naive_datetime(&body.date)?;

    {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_account_by_id(&id)?
            .ok_or_else(|| AppError::NotFound(format!("account {id} not found")))?;
        db.set_account_balance(&id, balance, date)?;
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}
