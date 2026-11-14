//! `POST /api/income-sources/:id/sync`: pull the business's sales periods from
//! the shop app and upsert them as income records (plan 30 section 8).

use axum::Json;
use axum::extract::{Path, State};

use crate::income::sync::{SyncConfig, apply, fetch};
use crate::income::validate::parse_iso_date;
use crate::model::{IncomeSourceType, IncomeSyncRequest, IncomeSyncResult};
use crate::server::error::AppError;
use crate::server::routes::income::BodyJson;
use crate::server::state::AppState;

const SYNC_KIND_SHOP_APP: &str = "shop_app";

pub async fn sync_income_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
    BodyJson(body): BodyJson<IncomeSyncRequest>,
) -> Result<Json<IncomeSyncResult>, AppError> {
    let from = parse_iso_date(&body.from, "from")?;
    let to = parse_iso_date(&body.to, "to")?;
    if to < from {
        return Err(AppError::bad_request(
            "to must not be before from",
            "invalid_range",
        ));
    }
    // The shop app refuses a range over three years, so refuse it here first.
    if to > from + chrono::Months::new(36) {
        return Err(AppError::bad_request(
            "the range must not be longer than three years",
            "invalid_range",
        ));
    }
    // The database lock is released before the HTTP call.
    let source = {
        let db = state.db();
        db.get_income_source(&id)?.ok_or_else(|| {
            AppError::bad_request(format!("income source {id} not found"), "source_not_found")
        })?
    };
    let is_shop_source = source.source_type == IncomeSourceType::SelfEmployment
        && source.sync_kind.as_deref() == Some(SYNC_KIND_SHOP_APP);
    if !is_shop_source {
        return Err(AppError::bad_request(
            "only a self_employment source with sync_kind shop_app can be synced",
            "sync_not_supported",
        ));
    }
    let config = SyncConfig::from_env()?;
    let response = fetch(&config, &body.from, &body.to).await?;
    let result = {
        let db = state.db();
        if body.dry_run {
            apply(&db, &source, &response, true)?
        } else {
            db.in_transaction(|db| apply(db, &source, &response, false))?
        }
    };
    Ok(Json(result))
}
