//! Import routes: POST /api/import (JSON), POST /api/import/csv,
//! POST /api/import/bulk (multipart).
//!
//! Auth notes:
//! - Loopback mode: requests pass without a token (browser UI can import).
//! - Non-loopback (Docker / remote): a valid bearer token is required.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Multipart, Query, State};
use serde::Deserialize;

use crate::importers::llm_parser::StatementParser;
use crate::model::{BankFormat, ImportLog, ImportPayload, ImportResult};
use crate::server::auth::AuthContext;
use crate::server::error::AppError;
use crate::server::state::AppState;

// ── Auth helper ───────────────────────────────────────────────────────────────

fn require_token_if_remote(state: &AppState, auth: &AuthContext) -> Result<(), AppError> {
    if !state.loopback_only && !matches!(auth, AuthContext::Token { .. }) {
        return Err(AppError::Unauthorized(
            "Bearer token required for import endpoints in non-loopback mode".to_string(),
        ));
    }
    Ok(())
}

// ── POST /api/import ──────────────────────────────────────────────────────────

pub async fn import_json(
    State(state): State<AppState>,
    auth: axum::extract::Extension<AuthContext>,
    Json(payload): Json<ImportPayload>,
) -> Result<Json<ImportResult>, AppError> {
    require_token_if_remote(&state, &auth)?;

    if payload.transactions.is_empty() {
        return Err(AppError::bad_request(
            "transactions array must not be empty",
            "empty_transactions",
        ));
    }

    let db = state.db.lock().expect("db mutex poisoned");

    if !db.account_exists(&payload.account_id)? {
        return Err(AppError::bad_request(
            format!("account {} not found", payload.account_id),
            "account_not_found",
        ));
    }

    let result = db.insert_transactions_bulk(&payload.account_id, &payload.transactions)?;

    db.log_import(&ImportLog {
        filename: "<api>".to_string(),
        account_id: payload.account_id.clone(),
        rows_total: result.rows_total,
        rows_inserted: result.rows_inserted,
        rows_duplicate: result.rows_duplicate,
        source: "api".to_string(),
        detected_bank: BankFormat::Unknown,
        detection_confidence: 0.0,
    })?;

    Ok(Json(result))
}

// ── POST /api/import/csv ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CsvImportQuery {
    pub account: Option<String>,
}

pub async fn import_csv(
    State(state): State<AppState>,
    auth: axum::extract::Extension<AuthContext>,
    Query(q): Query<CsvImportQuery>,
    mut multipart: Multipart,
) -> Result<Json<ImportResult>, AppError> {
    require_token_if_remote(&state, &auth)?;

    let account_id = q.account.ok_or_else(|| {
        AppError::bad_request("missing required parameter: account", "account_not_found")
    })?;

    {
        let db = state.db.lock().expect("db mutex poisoned");
        if !db.account_exists(&account_id)? {
            return Err(AppError::bad_request(
                format!("account {account_id} not found"),
                "account_not_found",
            ));
        }
    }

    let (filename, raw_csv) = extract_csv_from_multipart(&mut multipart).await?;

    let parser = crate::importers::llm_parser::LlmStatementParser::from_env()
        .map_err(anyhow::Error::from)?;
    let min_detection_confidence = parser.min_detection_confidence;
    let min_row_confidence = parser.min_row_confidence;

    let parsed = parser
        .parse(&raw_csv, &filename)
        .await
        .map_err(anyhow::Error::from)?;

    if parsed.detection_confidence < min_detection_confidence {
        return Err(AppError::bad_request(
            format!(
                "LLM detection confidence {:.2} is below threshold {min_detection_confidence:.2}",
                parsed.detection_confidence
            ),
            "invalid_csv",
        ));
    }

    let result = {
        let db = state.db.lock().expect("db mutex poisoned");
        process_parsed_statement(&db, &account_id, &filename, parsed, min_row_confidence)?
    };

    {
        let db = state.db.lock().expect("db mutex poisoned");
        db.log_import(&ImportLog {
            filename: filename.clone(),
            account_id: account_id.to_string(),
            rows_total: result.rows_total,
            rows_inserted: result.rows_inserted,
            rows_duplicate: result.rows_duplicate,
            source: "csv_api".to_string(),
            detected_bank: result.detected_bank,
            detection_confidence: result.detection_confidence,
        })?;
    }

    Ok(Json(result))
}

// ── POST /api/import/bulk ─────────────────────────────────────────────────────

pub async fn import_bulk(
    State(state): State<AppState>,
    auth: axum::extract::Extension<AuthContext>,
    mut multipart: Multipart,
) -> Result<Json<Vec<ImportResult>>, AppError> {
    require_token_if_remote(&state, &auth)?;

    // Collect all files and accounts from the multipart form.
    // Expected fields: `files[]` (file upload) and `accounts[]` (account ID text),
    // paired in order.
    let mut files: Vec<(String, String, String)> = Vec::new(); // (account_id, filename, raw_csv)

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(e.to_string(), "invalid_csv"))?
    {
        let field_name = field.name().unwrap_or("").to_string();
        if field_name == "files[]" || field_name == "file" {
            let filename = field.file_name().unwrap_or("upload.csv").to_string();
            let bytes = field
                .bytes()
                .await
                .map_err(|e| AppError::bad_request(e.to_string(), "invalid_csv"))?;
            let raw_csv = String::from_utf8(bytes.to_vec())
                .map_err(|_| AppError::bad_request("CSV file is not valid UTF-8", "invalid_csv"))?;
            files.push(("__pending__".to_string(), filename, raw_csv));
        } else if field_name == "accounts[]" || field_name == "account" {
            let bytes = field
                .bytes()
                .await
                .map_err(|e| AppError::bad_request(e.to_string(), "invalid_csv"))?;
            let account_id = String::from_utf8(bytes.to_vec()).map_err(|_| {
                AppError::bad_request("invalid account_id encoding", "account_not_found")
            })?;
            if let Some(last) = files.last_mut() {
                if last.0 == "__pending__" {
                    last.0 = account_id;
                }
            }
        }
    }

    if files.is_empty() {
        return Err(AppError::bad_request("no files provided", "missing_file"));
    }

    let parser = crate::importers::llm_parser::LlmStatementParser::from_env()
        .map_err(anyhow::Error::from)?;
    let min_detection_confidence = parser.min_detection_confidence;
    let min_row_confidence = parser.min_row_confidence;
    let parser: Arc<dyn StatementParser> = Arc::new(parser);

    let mut results = Vec::new();

    for (account_id, filename, raw_csv) in files {
        if account_id == "__pending__" {
            results.push(ImportResult {
                filename: filename.clone(),
                errors: vec![crate::model::ImportRowError {
                    index: 0,
                    reason: "no account_id provided for this file".to_string(),
                }],
                ..ImportResult::default()
            });
            continue;
        }

        let account_exists = {
            let db = state.db.lock().expect("db mutex poisoned");
            db.account_exists(&account_id)?
        };
        if !account_exists {
            results.push(ImportResult {
                filename: filename.clone(),
                account_id: account_id.clone(),
                errors: vec![crate::model::ImportRowError {
                    index: 0,
                    reason: format!("account {account_id} not found"),
                }],
                ..ImportResult::default()
            });
            continue;
        }

        let parse_result: Result<_, anyhow::Error> =
            parser.parse(&raw_csv, &filename).await.map_err(Into::into);

        match parse_result {
            Err(e) => {
                results.push(ImportResult {
                    filename: filename.clone(),
                    account_id: account_id.clone(),
                    errors: vec![crate::model::ImportRowError {
                        index: 0,
                        reason: e.to_string(),
                    }],
                    ..ImportResult::default()
                });
            }
            Ok(parsed) => {
                if parsed.detection_confidence < min_detection_confidence {
                    results.push(ImportResult {
                        filename: filename.clone(),
                        account_id: account_id.clone(),
                        errors: vec![crate::model::ImportRowError {
                            index: 0,
                            reason: format!(
                                "detection confidence too low: {:.2}",
                                parsed.detection_confidence
                            ),
                        }],
                        ..ImportResult::default()
                    });
                    continue;
                }

                let result = {
                    let db = state.db.lock().expect("db mutex poisoned");
                    match process_parsed_statement(
                        &db,
                        &account_id,
                        &filename,
                        parsed,
                        min_row_confidence,
                    ) {
                        Ok(r) => r,
                        Err(e) => {
                            ImportResult {
                                filename: filename.clone(),
                                account_id: account_id.clone(),
                                errors: vec![crate::model::ImportRowError {
                                    index: 0,
                                    reason: e.to_string(),
                                }],
                                ..ImportResult::default()
                            }
                        }
                    }
                };

                {
                    let db = state.db.lock().expect("db mutex poisoned");
                    let _ = db.log_import(&ImportLog {
                        filename: filename.clone(),
                        account_id: account_id.clone(),
                        rows_total: result.rows_total,
                        rows_inserted: result.rows_inserted,
                        rows_duplicate: result.rows_duplicate,
                        source: "csv_api_bulk".to_string(),
                        detected_bank: result.detected_bank,
                        detection_confidence: result.detection_confidence,
                    });
                }

                results.push(result);
            }
        }
    }

    Ok(Json(results))
}

// ── Shared helpers ────────────────────────────────────────────────────────────

async fn extract_csv_from_multipart(
    multipart: &mut Multipart,
) -> Result<(String, String), AppError> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(e.to_string(), "invalid_csv"))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" || name == "files[]" {
            let filename = field.file_name().unwrap_or("upload.csv").to_string();
            let bytes = field
                .bytes()
                .await
                .map_err(|e| AppError::bad_request(e.to_string(), "invalid_csv"))?;
            if bytes.is_empty() {
                return Err(AppError::bad_request("uploaded file is empty", "missing_file"));
            }
            let raw = String::from_utf8(bytes.to_vec())
                .map_err(|_| AppError::bad_request("CSV file is not valid UTF-8", "invalid_csv"))?;
            return Ok((filename, raw));
        }
    }
    Err(AppError::bad_request(
        "no file field found in multipart body",
        "missing_file",
    ))
}

fn process_parsed_statement(
    db: &crate::storage::Db,
    account_id: &str,
    filename: &str,
    parsed: crate::importers::llm_parser::ParsedStatement,
    min_row_confidence: f32,
) -> anyhow::Result<ImportResult> {
    use crate::model::{InsertOutcome, Transaction};

    let mut result = ImportResult {
        filename: filename.to_string(),
        account_id: account_id.to_string(),
        detected_bank: parsed.detected_bank,
        detection_confidence: parsed.detection_confidence,
        ..ImportResult::default()
    };

    for row in parsed.rows {
        result.rows_total += 1;
        if row.row_confidence < min_row_confidence {
            tracing::warn!(
                filename,
                row_confidence = row.row_confidence,
                threshold = min_row_confidence,
                "skipping low-confidence row"
            );
            continue;
        }
        let tx = Transaction::from_unified(row, account_id);
        match db.insert_transaction(&tx)? {
            InsertOutcome::Inserted => result.rows_inserted += 1,
            InsertOutcome::Duplicate => result.rows_duplicate += 1,
        }
    }

    Ok(result)
}
