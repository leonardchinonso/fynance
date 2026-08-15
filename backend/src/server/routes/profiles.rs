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
        let db = state.db();
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
        let db = state.db();
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
        utr: None,
    }))
}

// ── PATCH /api/profiles/:id ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PatchProfileBody {
    pub name: Option<String>,
    /// HMRC Unique Taxpayer Reference. An explicit JSON `null` clears it; an
    /// absent key leaves it alone. `Option<Option<_>>` is what distinguishes
    /// the two — without it, "don't touch the UTR" and "remove the UTR" would
    /// arrive as the same request.
    #[serde(default, deserialize_with = "deserialize_optional_utr")]
    pub utr: Option<Option<String>>,
}

fn deserialize_optional_utr<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize as _;
    Ok(Some(Option::<String>::deserialize(deserializer)?))
}

/// A UTR is exactly 10 digits. Validated so a malformed one is caught at entry rather than
/// appearing on a generated SA108 page, where it would be wrong on a document sent to HMRC.
/// Spaces are tolerated on input (HMRC prints it as `12345 67890`) and stripped before storage.
fn normalize_utr(raw: &str) -> Result<Option<String>, AppError> {
    let cleaned: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.is_empty() {
        return Ok(None);
    }
    if cleaned.len() != 10 || !cleaned.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::bad_request(
            "UTR must be 10 digits",
            "invalid_utr",
        ));
    }
    Ok(Some(cleaned))
}

pub async fn update_profile(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchProfileBody>,
) -> Result<Json<Profile>, AppError> {
    if body.name.is_none() && body.utr.is_none() {
        return Err(AppError::bad_request(
            "at least one of 'name' or 'utr' is required",
            "empty_body",
        ));
    }

    if let Some(ref name) = body.name {
        if name.trim().is_empty() {
            return Err(AppError::bad_request(
                "name must not be empty",
                "invalid_name",
            ));
        }
    }

    // Validate before touching the DB, so a bad UTR cannot leave the name half-applied.
    let utr_update = match body.utr {
        Some(Some(ref raw)) => Some(normalize_utr(raw)?),
        Some(None) => Some(None),
        None => None,
    };

    let db = state.db();
    if !db.profile_exists(&id)? {
        return Err(AppError::NotFound(format!("profile {id} not found")));
    }

    if let Some(ref name) = body.name {
        db.update_profile_name(&id, name)?;
    }
    if let Some(ref utr) = utr_update {
        db.update_profile_utr(&id, utr.as_deref())?;
    }

    // Read back rather than reconstructing, so the response reflects whichever fields were
    // left untouched by this request.
    let profile = db
        .get_profiles()?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::NotFound(format!("profile {id} not found")))?;
    Ok(Json(profile))
}

// ── DELETE /api/profiles/:id ──────────────────────────────────────────────────

pub async fn delete_profile(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let db = state.db();
    if !db.profile_exists(&id)? {
        return Err(AppError::NotFound(format!("profile {id} not found")));
    }
    let referencing = db.count_accounts_referencing_profile(&id)?;
    if referencing > 0 {
        return Err(AppError::conflict(
            format!(
                "{referencing} account(s) still reference profile {id}; remove them from those accounts first"
            ),
            "profile_in_use",
        ));
    }
    db.delete_profile(&id)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
