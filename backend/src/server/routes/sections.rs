//! Section-mapping routes: GET /api/sections, PUT /api/sections.
//!
//! Section mappings classify budget categories into display sections
//! (Income, Bills, Spending, Irregular, Transfers) for the spending grid.

use axum::Json;
use axum::extract::State;
use serde_json::Value;

use crate::model::SectionMapping;
use crate::server::error::AppError;
use crate::server::state::AppState;

const VALID_SECTIONS: &[&str] = &["Income", "Bills", "Spending", "Irregular", "Transfers"];

// ── GET /api/sections ─────────────────────────────────────────────────────────

pub async fn list_sections(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let mappings = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_section_mappings()?
    };
    Ok(Json(serde_json::to_value(mappings)?))
}

// ── PUT /api/sections ─────────────────────────────────────────────────────────

pub async fn replace_sections(
    State(state): State<AppState>,
    Json(body): Json<Vec<SectionMapping>>,
) -> Result<Json<Value>, AppError> {
    // Validate every mapping
    for m in &body {
        if m.category_id.is_none() && m.category.as_ref().is_none_or(|c| c.is_empty()) {
            return Err(AppError::bad_request(
                "each mapping must have a category_id or category",
                "invalid_category",
            ));
        }
        if !VALID_SECTIONS.contains(&m.section.as_str()) {
            return Err(AppError::bad_request(
                format!(
                    "invalid section {}: must be one of Income|Bills|Spending|Irregular|Transfers",
                    m.section
                ),
                "invalid_section",
            ));
        }
    }

    if body.is_empty() {
        tracing::warn!(
            "PUT /api/sections called with empty array: all section mappings will be cleared"
        );
    }

    {
        let db = state.db.lock().expect("db mutex poisoned");
        db.update_section_mappings(&body)?;
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}
