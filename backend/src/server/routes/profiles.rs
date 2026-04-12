//! Profile routes: GET /api/profiles, POST /api/profiles.

use axum::Json;
use axum::extract::State;
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
