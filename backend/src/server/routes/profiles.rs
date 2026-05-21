//! Profile routes: GET /api/profiles, POST /api/profiles,
//! PATCH /api/profiles/:id, DELETE /api/profiles/:id.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::Value;

use crate::model::Profile;
use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::server::validation::validate_profile_id;

// ── GET /api/profiles ─────────────────────────────────────────────────────────

pub async fn list_profiles(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let profiles = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_profiles()?
    };
    Ok(Json(serde_json::to_value(profiles)?))
}

// ── POST /api/profiles ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateProfileBody {
    pub id: String,
    pub name: String,
}

pub async fn create_profile(
    State(state): State<AppState>,
    Json(body): Json<CreateProfileBody>,
) -> Result<Json<Profile>, AppError> {
    validate_profile_id(&body.id)?;

    {
        let db = state.db.lock().expect("db mutex poisoned");
        if db.profile_exists(&body.id)? {
            return Err(AppError::conflict(
                format!("profile {} already exists", body.id),
                "profile_exists",
            ));
        }
        db.create_profile(&body.id, &body.name)?;
    }

    Ok(Json(Profile {
        id: body.id,
        name: body.name,
    }))
}

// ── PATCH /api/profiles/:id ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PatchProfileBody {
    pub name: Option<String>,
}

pub async fn update_profile(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchProfileBody>,
) -> Result<Json<Profile>, AppError> {
    let name = body.name.ok_or_else(|| {
        AppError::bad_request("name is required", "empty_body")
    })?;
    if name.trim().is_empty() {
        return Err(AppError::bad_request(
            "name must not be empty",
            "invalid_name",
        ));
    }
    let db = state.db.lock().expect("db mutex poisoned");
    if !db.profile_exists(&id)? {
        return Err(AppError::NotFound(format!("profile {id} not found")));
    }
    db.update_profile_name(&id, &name)?;
    Ok(Json(Profile { id, name }))
}

// ── DELETE /api/profiles/:id ──────────────────────────────────────────────────

pub async fn delete_profile(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let db = state.db.lock().expect("db mutex poisoned");
    if !db.profile_exists(&id)? {
        return Err(AppError::NotFound(format!("profile {id} not found")));
    }
    let referencing = db.count_accounts_referencing_profile(&id)?;
    if referencing > 0 {
        return Err(AppError::conflict(
            format!("{referencing} account(s) still reference profile {id}; remove them from those accounts first"),
            "profile_in_use",
        ));
    }
    db.delete_profile(&id)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
