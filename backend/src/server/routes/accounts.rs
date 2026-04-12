//! Account routes: GET /api/accounts, POST /api/accounts.

use axum::Json;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::Value;

use crate::model::{Account, AccountType};
use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::server::validation::parse_date;

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
    pub balance: Option<String>,
    pub balance_date: Option<String>,
    #[serde(default)]
    pub profile_ids: Vec<String>,
    pub notes: Option<String>,
}

pub async fn create_account(
    State(state): State<AppState>,
    Json(body): Json<CreateAccountBody>,
) -> Result<Json<Account>, AppError> {
    if body.id.is_empty() {
        return Err(AppError::bad_request("id must not be empty", "invalid_account_id"));
    }

    let account_type = AccountType::parse(&body.account_type).ok_or_else(|| {
        AppError::bad_request(
            format!("invalid account type: {}", body.account_type),
            "invalid_account_type",
        )
    })?;

    // Validate balance / balance_date pair
    if body.balance.is_some() != body.balance_date.is_some() {
        return Err(AppError::bad_request(
            "balance and balance_date must both be provided or both omitted",
            "missing_balance_date",
        ));
    }

    let balance = body
        .balance
        .as_deref()
        .map(|s| {
            s.parse::<rust_decimal::Decimal>().map_err(|_| {
                AppError::bad_request(format!("invalid balance: {s}"), "invalid_decimal")
            })
        })
        .transpose()?;

    let balance_date = body
        .balance_date
        .as_deref()
        .map(parse_date)
        .transpose()?;

    // Normalize profile_ids: empty -> ["default"]
    let profile_ids = if body.profile_ids.is_empty() {
        vec!["default".to_string()]
    } else {
        body.profile_ids.clone()
    };

    let account = Account {
        id: body.id.clone(),
        name: body.name,
        institution: body.institution,
        account_type,
        currency: body.currency.unwrap_or_else(|| "GBP".to_string()),
        balance,
        balance_date,
        is_active: true,
        notes: body.notes,
        profile_ids,
        is_stale: None,
    };

    {
        let db = state.db.lock().expect("db mutex poisoned");
        if db.account_exists(&body.id)? {
            return Err(AppError::conflict(
                format!("account {} already exists", body.id),
                "account_exists",
            ));
        }
        db.create_account(&account)?;
    }

    Ok(Json(account))
}

// ── PATCH /api/accounts/:id/balance ──────────────────────────────────────────

use axum::extract::Path;

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

    let date = crate::server::validation::parse_date(&body.date)?;

    {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_account_by_id(&id)?
            .ok_or_else(|| AppError::NotFound(format!("account {id} not found")))?;
        db.set_account_balance(&id, balance, date)?;
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}
