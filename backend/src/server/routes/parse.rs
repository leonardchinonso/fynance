//! Stage 1 parse endpoint: POST /api/parse.
//! Accepts uploaded documents (1-5 CSV files), invokes the LLM pipeline to
//! classify and extract data, runs dedup checks, and returns a structured
//! IngestionPreview.

use axum::Json;
use axum::extract::{Multipart, State};

use crate::importers::document_parser::{
    DocumentInput, FileFormat, ParseHints, PipelineOutcome, build_multi_preview,
    run_multi_file_pipeline,
};
use crate::model::IngestionPreview;
use crate::server::error::AppError;
use crate::server::state::AppState;

const MAX_FILE_SIZE: usize = 10 * 1024 * 1024; // 10 MB per file
const MAX_TOTAL_SIZE: usize = 50 * 1024 * 1024; // 50 MB total
const MAX_FILES: usize = 5;

// ── POST /api/parse ─────────────────────────────────────────────────────────

pub async fn parse_documents(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<IngestionPreview>, AppError> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut account_id: Option<String> = None;
    let mut hints = ParseHints::default();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(format!("multipart error: {e}"), "invalid_multipart"))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "files[]" | "file" => {
                if files.len() >= MAX_FILES {
                    return Err(AppError::bad_request(
                        format!("maximum {MAX_FILES} files per request"),
                        "too_many_files",
                    ));
                }
                let filename = field.file_name().unwrap_or("upload.csv").to_string();
                let bytes = field.bytes().await.map_err(|e| {
                    AppError::bad_request(format!("failed to read file: {e}"), "file_read_error")
                })?;
                if bytes.is_empty() {
                    return Err(AppError::bad_request(
                        format!("file '{}' is empty", filename),
                        "empty_file",
                    ));
                }
                if bytes.len() > MAX_FILE_SIZE {
                    return Err(AppError::bad_request(
                        format!(
                            "file '{}' exceeds 10 MB limit ({} bytes)",
                            filename,
                            bytes.len()
                        ),
                        "file_too_large",
                    ));
                }
                files.push((filename, bytes.to_vec()));
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

    if files.is_empty() {
        return Err(AppError::bad_request(
            "at least one file is required",
            "no_files",
        ));
    }

    // Validate total size
    let total_size: usize = files.iter().map(|(_, b)| b.len()).sum();
    if total_size > MAX_TOTAL_SIZE {
        return Err(AppError::bad_request(
            format!("total upload size exceeds 50 MB limit ({total_size} bytes)"),
            "total_too_large",
        ));
    }

    let account_id = account_id.ok_or_else(|| {
        AppError::bad_request(
            "account_id is required for the parse endpoint",
            "missing_account_id",
        )
    })?;

    // Build DocumentInputs (Phase 2: still CSV only)
    let mut documents: Vec<DocumentInput> = Vec::new();
    for (filename, raw_bytes) in &files {
        let text_content = String::from_utf8(raw_bytes.clone()).map_err(|_| {
            AppError::bad_request(
                format!(
                    "file '{}' is not valid UTF-8 (only CSV supported)",
                    filename
                ),
                "invalid_format",
            )
        })?;
        documents.push(DocumentInput {
            original_size: raw_bytes.len(),
            filename: filename.clone(),
            format: FileFormat::Csv,
            text_content,
        });
    }

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

    // Run the multi-file LLM pipeline
    let start = std::time::Instant::now();
    let pipeline_result = run_multi_file_pipeline(&documents, &hints)
        .await
        .map_err(|e| AppError::bad_request(e.to_string(), "parse_error"))?;
    let elapsed = start.elapsed().as_millis() as u64;

    // If clarification needed, return early
    if let PipelineOutcome::NeedsClarification(mut preview) = pipeline_result {
        preview.metadata.processing_time_ms = elapsed;
        return Ok(Json(*preview));
    }

    // Extract successful result
    let (multi_classification, merged_extraction) = match pipeline_result {
        PipelineOutcome::Success {
            classification,
            extraction,
        } => (classification, extraction),
        PipelineOutcome::NeedsClarification(_) => unreachable!(),
    };

    // Run deduplication (needs DB, synchronous)
    let preview = {
        let db = state.db.lock().expect("db mutex poisoned");
        build_multi_preview(
            &multi_classification,
            merged_extraction,
            &account_id,
            &db,
            elapsed,
        )
    }
    .map_err(|e| AppError::bad_request(e.to_string(), "parse_error"))?;

    Ok(Json(preview))
}
