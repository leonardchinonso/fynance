//! Category CRUD routes: POST, GET, PATCH, DELETE.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::Value;

use crate::model::{Category, CreateCategoryPayload, PatchCategoryPayload};
use crate::server::error::AppError;
use crate::server::state::AppState;

// ── POST /api/categories ─────────────────────────────────────────────────────

pub async fn create_category(
    State(state): State<AppState>,
    Json(body): Json<CreateCategoryPayload>,
) -> Result<Json<Category>, AppError> {
    if body.name.is_empty() {
        return Err(AppError::bad_request("name must not be empty", "invalid_name"));
    }

    let category = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.create_category(&body)?
    };

    Ok(Json(category))
}

// ── GET /api/categories ──────────────────────────────────────────────────────

pub async fn list_categories(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let tree = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_categories_tree()?
    };
    Ok(Json(serde_json::to_value(tree)?))
}

// ── GET /api/categories/resolve?name=<name> ──────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ResolveQuery {
    pub name: String,
}

pub async fn resolve_category(
    State(state): State<AppState>,
    Query(q): Query<ResolveQuery>,
) -> Result<Json<Category>, AppError> {
    let db = state.db.lock().expect("db mutex poisoned");
    let category = db.resolve_category_by_name(&q.name)?
        .ok_or_else(|| AppError::NotFound(format!("category '{}' not found", q.name)))?;
    Ok(Json(category))
}

// ── GET /api/categories/:id ──────────────────────────────────────────────────

pub async fn get_category(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Category>, AppError> {
    let db = state.db.lock().expect("db mutex poisoned");
    let category = db.get_category_by_id(&id)?
        .ok_or_else(|| AppError::NotFound(format!("category {id} not found")))?;
    Ok(Json(category))
}

// ── PATCH /api/categories/:id ────────────────────────────────────────────────

pub async fn update_category(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchCategoryPayload>,
) -> Result<Json<Category>, AppError> {
    if body.name.is_none() && body.parent_id.is_none() && body.display_order.is_none() {
        return Err(AppError::bad_request(
            "at least one field must be provided",
            "empty_body",
        ));
    }
    if let Some(ref name) = body.name {
        if name.is_empty() {
            return Err(AppError::bad_request("name must not be empty", "invalid_name"));
        }
    }

    let category = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.update_category(&id, &body)?
    };
    Ok(Json(category))
}

// ── DELETE /api/categories/:id ───────────────────────────────────────────────

pub async fn delete_category(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let db = state.db.lock().expect("db mutex poisoned");
    db.soft_delete_category(&id)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
