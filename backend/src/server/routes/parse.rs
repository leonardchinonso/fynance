//! Stage 1 parse endpoint: POST /api/parse.
//! Accepts uploaded documents, invokes the LLM pipeline to classify and
//! extract data, runs dedup checks, and returns a structured IngestionPreview.

use axum::Json;
use axum::extract::{Multipart, State};

use crate::importers::document_parser::{
    DocumentInput, FileFormat, ParseHints, build_preview, run_llm_pipeline,
};
use crate::model::IngestionPreview;
use crate::server::error::AppError;
use crate::server::state::AppState;

const MAX_FILE_SIZE: usize = 10 * 1024 * 1024; // 10 MB

// ── POST /api/parse ─────────────────────────────────────────────────────────

pub async fn parse_documents(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<IngestionPreview>, AppError> {
    let mut file_data: Option<(String, Vec<u8>)> = None;
    let mut account_id: Option<String> = None;
    let mut hints = ParseHints::default();

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        AppError::bad_request(format!("multipart error: {e}"), "invalid_multipart")
    })? {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "files[]" | "file" => {
                if file_data.is_some() {
                    return Err(AppError::bad_request(
                        "Phase 1 supports single-file only. Upload one file at a time.",
                        "too_many_files",
                    ));
                }
                let filename = field.file_name().unwrap_or("upload.csv").to_string();
                let bytes = field.bytes().await.map_err(|e| {
                    AppError::bad_request(format!("failed to read file: {e}"), "file_read_error")
                })?;
                if bytes.is_empty() {
                    return Err(AppError::bad_request(
                        "uploaded file is empty",
                        "empty_file",
                    ));
                }
                if bytes.len() > MAX_FILE_SIZE {
                    return Err(AppError::bad_request(
                        format!("file exceeds 10 MB limit ({} bytes)", bytes.len()),
                        "file_too_large",
                    ));
                }
                file_data = Some((filename, bytes.to_vec()));
            }
            "account_id" => {
                let bytes = field.bytes().await.map_err(|e| {
                    AppError::bad_request(format!("failed to read account_id: {e}"), "field_error")
                })?;
                let val = String::from_utf8(bytes.to_vec()).map_err(|_| {
                    AppError::bad_request("account_id is not valid UTF-8", "field_error")
                })?;
                if !val.is_empty() {
                    account_id = Some(val);
                }
            }
            "hints" => {
                let bytes = field.bytes().await.map_err(|e| {
                    AppError::bad_request(format!("failed to read hints: {e}"), "field_error")
                })?;
                let val = String::from_utf8(bytes.to_vec()).map_err(|_| {
                    AppError::bad_request("hints is not valid UTF-8", "field_error")
                })?;
                if !val.is_empty() {
                    hints = serde_json::from_str(&val).map_err(|e| {
                        AppError::bad_request(
                            format!("hints is not valid JSON: {e}"),
                            "invalid_hints",
                        )
                    })?;
                }
            }
            _ => {
                let _ = field.bytes().await;
            }
        }
    }

    let (filename, raw_bytes) = file_data.ok_or_else(|| {
        AppError::bad_request("at least one file is required", "no_files")
    })?;

    let account_id = account_id.ok_or_else(|| {
        AppError::bad_request(
            "account_id is required for the parse endpoint",
            "missing_account_id",
        )
    })?;

    // Phase 1: CSV only
    let text_content = String::from_utf8(raw_bytes.clone()).map_err(|_| {
        AppError::bad_request(
            "file is not valid UTF-8 (only CSV files are supported in this version)",
            "invalid_format",
        )
    })?;

    let document = DocumentInput {
        original_size: raw_bytes.len(),
        filename,
        format: FileFormat::Csv,
        text_content,
    };

    // Validate account exists
    {
        let db = state.db.lock().expect("db mutex poisoned");
        if !db.account_exists(&account_id)? {
            return Err(AppError::bad_request(
                format!("account {} not found", account_id),
                "account_not_found",
            ));
        }
    }

    // Run the LLM pipeline (no DB needed, safe to await)
    let start = std::time::Instant::now();
    let (classification, extraction) = run_llm_pipeline(&document, &hints)
        .await
        .map_err(|e| AppError::bad_request(e.to_string(), "parse_error"))?;
    let elapsed = start.elapsed().as_millis() as u64;

    // Run deduplication (needs DB, synchronous)
    let preview = {
        let db = state.db.lock().expect("db mutex poisoned");
        build_preview(&classification, extraction, &account_id, &db, elapsed)
    }
    .map_err(|e| AppError::bad_request(e.to_string(), "parse_error"))?;

    Ok(Json(preview))
}
