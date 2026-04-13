//! Holdings routes:
//!   GET  /api/holdings
//!   POST /api/holdings/:account_id

use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use serde::Deserialize;

use crate::model::Holding;
use crate::server::auth::AuthContext;
use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::server::validation::split_csv_param;

// ── GET /api/holdings ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct HoldingsQuery {
    /// Single account ID.
    pub account_id: Option<String>,
    /// Comma-separated list of account IDs.
    pub account_ids: Option<String>,
    /// Return holdings for all investment/pension accounts belonging to this profile.
    pub profile_id: Option<String>,
}

pub async fn list_holdings(
    State(state): State<AppState>,
    Query(q): Query<HoldingsQuery>,
) -> Result<Json<Vec<Holding>>, AppError> {
    let db = state.db.lock().expect("db mutex poisoned");

    // Collect the set of account IDs to query.
    let account_ids: Vec<String> = if let Some(ref id) = q.account_id {
        if !id.is_empty() {
            vec![id.clone()]
        } else {
            vec![]
        }
    } else if let Some(ref ids) = q.account_ids {
        split_csv_param(ids).unwrap_or_default()
    } else if let Some(ref pid) = q.profile_id {
        if !pid.is_empty() {
            // Find all investment/pension accounts for this profile.
            db.get_accounts(Some(pid))?
                .into_iter()
                .filter(|a| {
                    matches!(
                        a.account_type,
                        crate::model::AccountType::Investment | crate::model::AccountType::Pension
                    )
                })
                .map(|a| a.id)
                .collect()
        } else {
            vec![]
        }
    } else {
        return Err(AppError::bad_request(
            "must provide one of: account_id, account_ids, profile_id",
            "missing_parameter",
        ));
    };

    if account_ids.is_empty() {
        return Ok(Json(vec![]));
    }

    let holdings = db.get_holdings_batch(&account_ids)?;
    Ok(Json(holdings))
}

// ── POST /api/holdings/:account_id ────────────────────────────────────────────

pub async fn post_holdings(
    State(state): State<AppState>,
    auth: Extension<AuthContext>,
    Path(account_id): Path<String>,
    Json(body): Json<Vec<Holding>>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !state.loopback_only && !matches!(*auth, AuthContext::Token { .. }) {
        return Err(AppError::Unauthorized(
            "Bearer token required for holdings endpoints in non-loopback mode".to_string(),
        ));
    }

    let holdings_updated = {
        let db = state.db.lock().expect("db mutex poisoned");
        if db.get_account_by_id(&account_id)?.is_none() {
            return Err(AppError::NotFound(format!(
                "account {account_id} not found"
            )));
        }
        db.replace_holdings(&account_id, &body)?
    };

    Ok(Json(serde_json::json!({
        "ok": true,
        "holdings_updated": holdings_updated
    })))
}
