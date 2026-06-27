//! Document routes: store, list, download, and delete uploaded source files.
//!
//! Documents are the provenance anchor for imported data: every transaction,
//! holding, and investment carries a `source_document_ids` array pointing back
//! here. Files auto-stored during a parse have `origin = "parse"`; files
//! uploaded directly through `POST /api/documents` have `origin = "manual"`.
//!
//! Auth mirrors the import endpoints: loopback browser requests pass without a
//! token; non-loopback (Docker / remote) requires a bearer token.

use axum::Json;
use axum::body::Body;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;

use crate::model::{DocumentDeleteResult, DocumentSummary};
use crate::server::auth::AuthContext;
use crate::server::error::AppError;
use crate::server::state::AppState;
use crate::storage::DeleteDocumentOutcome;

const MAX_FILE_SIZE: usize = 10 * 1024 * 1024; // 10 MB per file

fn require_token_if_remote(state: &AppState, auth: &AuthContext) -> Result<(), AppError> {
    if !state.loopback_only && !matches!(auth, AuthContext::Token { .. }) {
        return Err(AppError::Unauthorized(
            "Bearer token required for document endpoints in non-loopback mode".to_string(),
        ));
    }
    Ok(())
}

// ── GET /api/documents ────────────────────────────────────────────────────────

pub async fn list_documents(
    State(state): State<AppState>,
    auth: axum::extract::Extension<AuthContext>,
) -> Result<Json<Vec<DocumentSummary>>, AppError> {
    require_token_if_remote(&state, &auth)?;
    let db = state.db.lock().expect("db mutex poisoned");
    Ok(Json(db.list_documents()?))
}

// ── GET /api/documents/:id ────────────────────────────────────────────────────

pub async fn get_document(
    State(state): State<AppState>,
    auth: axum::extract::Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<DocumentSummary>, AppError> {
    require_token_if_remote(&state, &auth)?;
    let db = state.db.lock().expect("db mutex poisoned");
    let doc = db
        .get_document(&id)?
        .ok_or_else(|| AppError::NotFound(format!("document {id} not found")))?;
    let refs = db.document_references(&id)?;
    Ok(Json(DocumentSummary {
        id: doc.id,
        filename: doc.filename,
        mime_type: doc.mime_type,
        size_bytes: doc.size_bytes as usize,
        origin: doc.origin,
        account_id: doc.account_id,
        uploaded_at: doc.uploaded_at,
        reference_count: refs.total(),
        orphaned: refs.total() == 0,
    }))
}

// ── GET /api/documents/:id/download ───────────────────────────────────────────

pub async fn download_document(
    State(state): State<AppState>,
    auth: axum::extract::Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    require_token_if_remote(&state, &auth)?;
    let doc = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_document(&id)?
            .ok_or_else(|| AppError::NotFound(format!("document {id} not found")))?
    };

    let bytes = std::fs::read(&doc.file_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::NotFound(format!("document {id} file is missing on disk"))
        } else {
            AppError::Internal(e.into())
        }
    })?;

    let mut resp = Response::new(Body::from(bytes));
    let headers = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&doc.mime_type) {
        headers.insert(header::CONTENT_TYPE, v);
    }
    // Quote the filename and strip quotes/control chars to keep the header valid.
    let safe_name: String = doc
        .filename
        .chars()
        .filter(|c| *c != '"' && !c.is_control())
        .collect();
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{safe_name}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(resp)
}

// ── POST /api/documents (standalone upload) ───────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UploadQuery {
    pub account_id: Option<String>,
}

pub async fn upload_document(
    State(state): State<AppState>,
    auth: axum::extract::Extension<AuthContext>,
    Query(q): Query<UploadQuery>,
    mut multipart: Multipart,
) -> Result<Json<Vec<DocumentSummary>>, AppError> {
    require_token_if_remote(&state, &auth)?;

    let account_id = q.account_id.filter(|s| !s.is_empty());
    let mut stored: Vec<(String, String, Vec<u8>)> = Vec::new(); // (filename, mime, bytes)
    let mut form_account_id: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(format!("multipart error: {e}"), "invalid_multipart"))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" | "files[]" => {
                let filename = field.file_name().unwrap_or("upload").to_string();
                let mime = field
                    .content_type()
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| infer_mime(&filename));
                let bytes = field.bytes().await.map_err(|e| {
                    AppError::bad_request(format!("failed to read file: {e}"), "file_read_error")
                })?;
                if bytes.is_empty() {
                    return Err(AppError::bad_request(
                        format!("file '{filename}' is empty"),
                        "empty_file",
                    ));
                }
                if bytes.len() > MAX_FILE_SIZE {
                    return Err(AppError::bad_request(
                        format!("file '{filename}' exceeds 10 MB limit"),
                        "file_too_large",
                    ));
                }
                stored.push((filename, mime, bytes.to_vec()));
            }
            "account_id" => {
                let bytes = field.bytes().await.map_err(|e| {
                    AppError::bad_request(format!("failed to read account_id: {e}"), "field_error")
                })?;
                let val = String::from_utf8(bytes.to_vec()).map_err(|_| {
                    AppError::bad_request("account_id is not valid UTF-8", "field_error")
                })?;
                if !val.is_empty() {
                    form_account_id = Some(val);
                }
            }
            _ => {
                let _ = field.bytes().await;
            }
        }
    }

    if stored.is_empty() {
        return Err(AppError::bad_request(
            "at least one file is required",
            "no_files",
        ));
    }

    let account_id = account_id.or(form_account_id);
    let db = state.db.lock().expect("db mutex poisoned");
    let mut out = Vec::with_capacity(stored.len());
    for (filename, mime, bytes) in stored {
        let (doc, _deduped) =
            db.store_document(&filename, &mime, &bytes, "manual", account_id.as_deref())?;
        let refs = db.document_references(&doc.id)?;
        out.push(DocumentSummary {
            id: doc.id,
            filename: doc.filename,
            mime_type: doc.mime_type,
            size_bytes: doc.size_bytes as usize,
            origin: doc.origin,
            account_id: doc.account_id,
            uploaded_at: doc.uploaded_at,
            reference_count: refs.total(),
            orphaned: refs.total() == 0,
        });
    }
    Ok(Json(out))
}

// ── DELETE /api/documents/:id ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DeleteQuery {
    #[serde(default)]
    pub force: Option<bool>,
}

pub async fn delete_document(
    State(state): State<AppState>,
    auth: axum::extract::Extension<AuthContext>,
    Path(id): Path<String>,
    Query(q): Query<DeleteQuery>,
) -> Result<Response, AppError> {
    require_token_if_remote(&state, &auth)?;
    let force = q.force.unwrap_or(false);

    let outcome = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.delete_document(&id, force)?
    };

    match outcome {
        DeleteDocumentOutcome::NotFound => {
            Err(AppError::NotFound(format!("document {id} not found")))
        }
        DeleteDocumentOutcome::Referenced(refs) => {
            // Custom body: AppError only carries error+code, but the UI needs
            // the per-entity breakdown to write a precise confirm dialog.
            let mut parts = Vec::new();
            if refs.transactions > 0 {
                parts.push(format!("{} transactions", refs.transactions));
            }
            if refs.holdings > 0 {
                parts.push(format!("{} holdings", refs.holdings));
            }
            if refs.investments > 0 {
                parts.push(format!("{} investments", refs.investments));
            }
            let body = json!({
                "error": format!("document is referenced by {}", parts.join(", ")),
                "code": "document_referenced",
                "references": refs,
            });
            Ok((StatusCode::CONFLICT, Json(body)).into_response())
        }
        DeleteDocumentOutcome::Deleted(unlinked) => Ok(Json(DocumentDeleteResult {
            deleted: true,
            unlinked,
        })
        .into_response()),
    }
}

fn infer_mime(filename: &str) -> String {
    let ext = filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "csv" => "text/csv",
        "txt" => "text/plain",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "application/octet-stream",
    }
    .to_string()
}
