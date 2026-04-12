//! Ingestion checklist routes: GET /api/ingestion/checklist/:month,
//! POST /api/ingestion/checklist/:month/:account_id.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::Value;

use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::server::validation::parse_month;

// ── GET /api/ingestion/checklist/:month ───────────────────────────────────────

pub async fn get_checklist(
    State(state): State<AppState>,
    Path(month): Path<String>,
) -> Result<Json<Value>, AppError> {
    parse_month(&month)?;
    let items = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_checklist(&month)?
    };
    Ok(Json(serde_json::to_value(items)?))
}

// ── POST /api/ingestion/checklist/:month/:account_id ─────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct MarkCompleteBody {
    pub notes: Option<String>,
}

pub async fn mark_complete(
    State(state): State<AppState>,
    Path((month, account_id)): Path<(String, String)>,
    body: Option<Json<MarkCompleteBody>>,
) -> Result<Json<Value>, AppError> {
    parse_month(&month)?;

    let notes = body.and_then(|b| b.notes.clone());

    {
        let db = state.db.lock().expect("db mutex poisoned");
        if !db.account_exists(&account_id)? {
            return Err(AppError::NotFound(format!("account {account_id} not found")));
        }
        db.mark_checklist_complete(&month, &account_id, notes.as_deref())?;
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}
