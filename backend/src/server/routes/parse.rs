//! Stage 1 parse endpoint: POST /api/parse.
//! Accepts uploaded documents and returns a structured preview.
//! Phase 0: stub that returns an empty IngestionPreview.

use axum::Json;
use axum::extract::{Multipart, State};

use crate::model::{
    HoldingsIngestionResult, IngestionMetadata, IngestionPreview, IngestionStatus,
    InvestmentIngestionResult, TransactionIngestionResult,
};
use crate::server::error::AppError;
use crate::server::state::AppState;

// ── POST /api/parse ────────��────────────────────────────────────────────────

pub async fn parse_documents(
    State(_state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<IngestionPreview>, AppError> {
    let mut files_count: usize = 0;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        AppError::bad_request(format!("multipart error: {e}"), "invalid_multipart")
    })? {
        let name = field.name().unwrap_or("").to_string();
        if name == "files[]" || name == "file" {
            let _bytes = field.bytes().await.map_err(|e| {
                AppError::bad_request(
                    format!("failed to read file: {e}"),
                    "file_read_error",
                )
            })?;
            files_count += 1;
        }
    }

    if files_count == 0 {
        return Err(AppError::bad_request(
            "at least one file is required",
            "no_files",
        ));
    }

    let preview = IngestionPreview {
        status: IngestionStatus::Success,
        metadata: IngestionMetadata {
            files_processed: files_count,
            institution_detected: None,
            detection_confidence: 0.0,
            processing_time_ms: 0,
            notes: vec![],
            relationships_found: vec![],
        },
        transactions: TransactionIngestionResult {
            count: 0,
            new: 0,
            duplicate: 0,
            errors: 0,
            rows: vec![],
            payload: None,
        },
        holdings: HoldingsIngestionResult {
            count: 0,
            new: 0,
            modify: 0,
            rows: vec![],
            payload: None,
        },
        investments: InvestmentIngestionResult {
            count: 0,
            new: 0,
            duplicate: 0,
            rows: vec![],
            payload: None,
        },
        clarifications_needed: vec![],
    };

    Ok(Json(preview))
}
