//! Stage 1 parse endpoint: POST /api/parse.
//! Accepts uploaded documents (1-5 CSV files), invokes the LLM pipeline to
//! extract data, runs dedup checks, and returns a structured IngestionPreview.

use std::convert::Infallible;

use axum::Json;
use axum::extract::{Multipart, Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use tokio_stream::StreamExt;

use crate::importers::document_parser::{
    DocumentInput, FileFormat, ParseHints, ParseMode, PipelineOutcome, build_multi_preview,
    run_multi_file_pipeline,
};
use crate::importers::provider::{
    self, ParsePhase, ProgressEvent, ProgressTx, ProviderError, create_provider_with_auth,
    emit_progress,
};
use crate::importers::tax_documents::parser::{LlmTaxDocumentParser, ParsedTaxDocuments};
use crate::importers::unified_parser::{
    CategorySummary, HoldingSummary, UnifiedContext, extract_all as unified_extract_all,
};
use crate::model::IngestionPreview;
use crate::server::error::AppError;
use crate::server::state::{AppState, lock_or_recover};

/// Shared by the post-parse `max_tokens` check and the `Truncated` provider
/// error: either way the model ran out of output budget mid-result.
const RESPONSE_TRUNCATED_MESSAGE: &str = "The uploaded document(s) produced more data than the model could return in a \
     single response, so the result was truncated. Please split your upload into \
     smaller batches (we recommend a few months at a time) and try again.";

const MAX_FILE_SIZE: usize = 10 * 1024 * 1024; // 10 MB per file
const MAX_TOTAL_SIZE: usize = 50 * 1024 * 1024; // 50 MB total
const MAX_FILES: usize = 5;
const MAX_TAX_FILES: usize = 15;

type ProgressChannels =
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, ProgressTx>>>;

/// Removes this parse's progress channel from the shared map on drop, so
/// every exit path out of `parse_documents` (including early error returns)
/// cleans up exactly once instead of leaking the entry until the 600s
/// backstop timer. Dropping the map's sender does not cut off events already
/// broadcast: the SSE stream drains its own receiver, so the final `done`
/// event still reaches subscribers.
struct ProgressCleanupGuard {
    parse_id: Option<String>,
    channels: ProgressChannels,
}

impl Drop for ProgressCleanupGuard {
    fn drop(&mut self) {
        if let Some(pid) = &self.parse_id {
            lock_or_recover(&self.channels).remove(pid);
        }
    }
}

/// Open the broadcast channel for this parse's SSE stream. The map entry is
/// removed by [`ProgressCleanupGuard`]; the timer is only a backstop.
fn register_progress(state: &AppState, parse_id: Option<&String>) -> Option<ProgressTx> {
    parse_id.map(|pid| {
        let (tx, _rx) = tokio::sync::broadcast::channel::<ProgressEvent>(64);
        state.progress_channels().insert(pid.clone(), tx.clone());

        let channels = state.progress_channels.clone();
        let pid_clone = pid.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(600)).await;
            lock_or_recover(&channels).remove(&pid_clone);
        });

        tx
    })
}

// ── POST /api/parse ─────────────────────────────────────────────────────────

pub async fn parse_documents(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<IngestionPreview>, AppError> {
    let route_started = std::time::Instant::now();
    tracing::info!("POST /api/parse: request received");
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut account_id: Option<String> = None;
    let mut profile_id: Option<String> = None;
    let mut hints: Option<ParseHints> = None;
    let mut parse_id: Option<String> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        tracing::error!(error = %e, "parse: multipart parse error");
        AppError::bad_request(format!("multipart error: {e}"), "invalid_multipart")
    })? {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "files[]" | "file" => {
                if files.len() >= MAX_TAX_FILES {
                    return Err(AppError::bad_request(
                        format!("maximum {MAX_TAX_FILES} files per request"),
                        "too_many_files",
                    ));
                }
                let filename = field.file_name().unwrap_or("upload.csv").to_string();
                let bytes = field.bytes().await.map_err(|e| {
                    tracing::error!(filename, error = %e, "parse: failed to read file bytes");
                    AppError::bad_request(format!("failed to read file: {e}"), "file_read_error")
                })?;
                tracing::debug!(filename, bytes = bytes.len(), "parse: file received");
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
            "profile_id" => {
                let bytes = field.bytes().await.map_err(|e| {
                    AppError::bad_request(format!("failed to read profile_id: {e}"), "field_error")
                })?;
                let val = String::from_utf8(bytes.to_vec()).map_err(|_| {
                    AppError::bad_request("profile_id is not valid UTF-8", "field_error")
                })?;
                if !val.is_empty() {
                    profile_id = Some(val);
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
                    hints = Some(serde_json::from_str(&val).map_err(|e| {
                        tracing::error!(
                            hints_bytes = val.len(),
                            error = %e,
                            "parse: failed to parse hints JSON"
                        );
                        AppError::bad_request(
                            format!("hints is not valid JSON: {e}"),
                            "invalid_hints",
                        )
                    })?);
                }
            }
            "parse_id" => {
                let bytes = field.bytes().await.map_err(|e| {
                    AppError::bad_request(format!("failed to read parse_id: {e}"), "field_error")
                })?;
                let val = String::from_utf8(bytes.to_vec()).map_err(|_| {
                    AppError::bad_request("parse_id is not valid UTF-8", "field_error")
                })?;
                if !val.is_empty() {
                    parse_id = Some(val);
                }
            }
            _ => {
                let _ = field.bytes().await;
            }
        }
    }

    tracing::info!(
        files = files.len(),
        has_account_id = account_id.is_some(),
        has_hints = hints.is_some(),
        has_parse_id = parse_id.is_some(),
        "parse: multipart loop complete"
    );

    if files.is_empty() {
        tracing::warn!("parse: rejecting — no files in request");
        return Err(AppError::bad_request(
            "at least one file is required",
            "no_files",
        ));
    }

    let wants_tax = hints
        .as_ref()
        .is_some_and(|h| h.return_type.wants_tax_documents());
    if !wants_tax && files.len() > MAX_FILES {
        return Err(AppError::bad_request(
            format!("maximum {MAX_FILES} files per request"),
            "too_many_files",
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

    if let (true, Some(hints)) = (wants_tax, hints.as_ref()) {
        return run_tax_documents_path(
            state,
            files,
            profile_id,
            hints.clone(),
            parse_id,
            route_started,
        )
        .await
        .map(Json);
    }

    let account_id = account_id.ok_or_else(|| {
        AppError::bad_request(
            "account_id is required for the parse endpoint",
            "missing_account_id",
        )
    })?;

    // Require hints
    let hints = hints.ok_or_else(|| {
        AppError::bad_request("hints is required for the parse endpoint", "missing_hints")
    })?;

    // Validate return_type
    if !hints.return_type.is_valid() {
        return Err(AppError::bad_request(
            "return_type must have at least one extraction type enabled (transactions, holdings.enabled, investments, or tax_documents)",
            "invalid_return_type",
        ));
    }
    if hints.return_type.holdings.period.is_some()
        && !hints.return_type.transactions
        && !hints.return_type.investments
    {
        return Err(AppError::bad_request(
            "holdings.period requires transactions or investments to be enabled (periodic snapshots are derived from the underlying activity)",
            "invalid_return_type",
        ));
    }

    let total_upload_bytes: usize = files.iter().map(|(_, b)| b.len()).sum();
    tracing::info!(
        files = files.len(),
        bytes = total_upload_bytes,
        account_id = %account_id,
        mode = ?hints.mode(),
        agent = ?hints.agent(),
        tx = hints.return_type.transactions,
        holdings = hints.return_type.holdings.enabled,
        period = ?hints.return_type.holdings.period,
        investments = hints.return_type.investments,
        "parse: hints validated, starting pipeline"
    );

    let progress_tx = register_progress(&state, parse_id.as_ref());
    let _progress_cleanup = ProgressCleanupGuard {
        parse_id: parse_id.clone(),
        channels: state.progress_channels.clone(),
    };

    emit_progress(
        &progress_tx,
        ProgressEvent::Phase {
            phase: ParsePhase::Preprocessing,
            message: format!("Processing {} uploaded file(s)", files.len()),
            task_id: None,
        },
    );

    // Build DocumentInputs (detect format, preprocess Excel/PDF)
    let mut documents: Vec<DocumentInput> = Vec::new();
    for (filename, raw_bytes) in files {
        let doc = crate::importers::format_detection::preprocess_file(&filename, raw_bytes)
            .map_err(|e| {
                AppError::bad_request(
                    format!("failed to process '{}': {}", filename, e),
                    "preprocessing_error",
                )
            })?;
        tracing::debug!(
            filename = %doc.filename,
            format = ?doc.format,
            text_bytes = doc.text_content.len(),
            raw_bytes = doc.raw_bytes.len(),
            "parse: document preprocessed"
        );
        documents.push(doc);
    }

    // Look up account (need institution for pipeline) and resolve its profile
    // name(s) so unified mode can recognise transfers to the holder's own other
    // accounts as self-transfers rather than money to other people.
    let (account, account_holder_names) = {
        let db = state.db();
        let account = db.get_account_by_id(&account_id)?.ok_or_else(|| {
            AppError::bad_request(
                format!("account {} not found", account_id),
                "account_not_found",
            )
        })?;
        let profiles = db.get_profiles().unwrap_or_default();
        let names: Vec<String> = account
            .profile_ids
            .iter()
            .filter_map(|pid| {
                profiles
                    .iter()
                    .find(|p| &p.id == pid)
                    .map(|p| p.name.clone())
            })
            .collect();
        (account, names)
    };

    // Create the LLM provider (reads FYNANCE_PARSE_PROVIDER from env). The
    // credential is chosen by hints.auth() (default: prefer the subscription
    // OAuth token, falling back to the API key). Surface the config message:
    // this is almost always a missing credential, and hiding it as a generic
    // 500 makes the issue impossible to debug.
    let provider = create_provider_with_auth(hints.auth())
        .map_err(|e| AppError::bad_request(e.to_string(), "provider_config"))?;

    let start = std::time::Instant::now();

    emit_progress(
        &progress_tx,
        ProgressEvent::Phase {
            phase: ParsePhase::SendingToLlm,
            message: format!("Sending {} file(s) to AI model", documents.len()),
            task_id: None,
        },
    );

    // ── Unified mode dispatch ───────────────────────────────────────────────
    if hints.mode() == ParseMode::Unified {
        let result = run_unified_path(
            state,
            hints,
            documents,
            account_id,
            account.institution,
            account_holder_names,
            provider,
            start,
            progress_tx.clone(),
        )
        .await;
        return result.map(Json);
    }

    // Run the multi-file LLM pipeline (split mode)
    let pipeline_result = run_multi_file_pipeline(
        &documents,
        &hints,
        &account.institution,
        provider,
        progress_tx.clone(),
    )
    .await
    .map_err(|e| {
        emit_progress(
            &progress_tx,
            ProgressEvent::Error {
                code: "parse_error".to_string(),
                message: e.to_string(),
            },
        );
        provider_err_to_app_error(e)
    })?;
    let elapsed = start.elapsed().as_millis() as u64;

    // If clarification needed, return early
    if let PipelineOutcome::NeedsClarification(mut preview) = pipeline_result {
        preview.metadata.processing_time_ms = elapsed;
        return Ok(Json(*preview));
    }

    // Extract successful result
    let merged_extraction = match pipeline_result {
        PipelineOutcome::Success { extraction } => extraction,
        PipelineOutcome::NeedsClarification(_) => unreachable!(),
    };

    emit_progress(
        &progress_tx,
        ProgressEvent::Phase {
            phase: ParsePhase::PostProcessing,
            message: "Checking for duplicates".to_string(),
            task_id: None,
        },
    );

    // Store the source documents and run deduplication (needs DB, synchronous).
    let preview = {
        let db = state.db();
        let (doc_map, all_doc_ids, doc_summaries) =
            store_parse_documents(&db, &documents, Some(&account_id))
                .map_err(|e| AppError::bad_request(e.to_string(), "document_store_error"))?;
        let mut preview = build_multi_preview(
            merged_extraction,
            &account_id,
            &account.institution,
            documents.len(),
            &db,
            elapsed,
            &doc_map,
            &all_doc_ids,
        )
        .map_err(|e| AppError::bad_request(e.to_string(), "parse_error"))?;
        preview.documents = doc_summaries;
        preview
    };

    emit_progress(
        &progress_tx,
        ProgressEvent::Done {
            total_ms: route_started.elapsed().as_millis() as u64,
        },
    );

    tracing::info!(
        elapsed_ms = route_started.elapsed().as_millis() as u64,
        tx_rows = preview.transactions.count,
        holdings_rows = preview.holdings.count,
        investment_rows = preview.investments.count,
        calls = preview.metadata.estimated_price.calls.len(),
        cost_usd = %preview.metadata.estimated_price.total,
        "parse: split-mode response shipped"
    );
    Ok(Json(preview))
}

// ── Unified-mode dispatch ────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn run_unified_path(
    state: AppState,
    hints: ParseHints,
    documents: Vec<DocumentInput>,
    account_id: String,
    account_institution: String,
    account_holder_names: Vec<String>,
    provider: std::sync::Arc<dyn crate::importers::provider::LlmProvider>,
    start: std::time::Instant,
    progress_tx: Option<ProgressTx>,
) -> Result<IngestionPreview, AppError> {
    use crate::importers::document_parser::ExtractionResult;
    use crate::importers::pricing::parser_call_cost;
    use crate::model::BankFormat;

    emit_progress(
        &progress_tx,
        ProgressEvent::Phase {
            phase: ParsePhase::BuildingContext,
            message: "Loading categories and holdings".to_string(),
            task_id: None,
        },
    );

    // Build UnifiedContext: active leaf categories + last open holdings.
    let ctx = {
        let db = state.db();
        let category_tree = db.get_categories_tree().unwrap_or_default();
        let categories: Vec<CategorySummary> = flatten_categories(&category_tree);
        let today = chrono::Local::now().date_naive();
        let last_open_holdings: Vec<HoldingSummary> = db
            .get_holdings_for_summary(today, None)
            .map(|rows| build_holdings_context(rows, &account_id))
            .unwrap_or_default();
        UnifiedContext {
            categories,
            last_open_holdings,
            account_holder_names,
        }
    };

    tracing::info!(
        files = documents.len(),
        categories_in_context = ctx.categories.len(),
        last_open_holdings = ctx.last_open_holdings.len(),
        agent = ?hints.agent(),
        "parse: unified context built, calling LLM"
    );

    // Build file tuples for type-agnostic upload (filename, mime, bytes).
    // PDFs are sent as native document blocks (the model reads the raw bytes);
    // CSV/Excel content lives in `text_content` after preprocessing (raw_bytes
    // is empty for those), so send the extracted text as a text/csv block.
    let files: Vec<(String, String, Vec<u8>)> = documents
        .iter()
        .map(|d| match d.format {
            FileFormat::Pdf => (
                d.filename.clone(),
                "application/pdf".to_string(),
                d.raw_bytes.clone(),
            ),
            FileFormat::Csv | FileFormat::Excel => (
                d.filename.clone(),
                "text/csv".to_string(),
                d.text_content.clone().into_bytes(),
            ),
            FileFormat::Image => (
                d.filename.clone(),
                infer_mime(&d.filename),
                d.raw_bytes.clone(),
            ),
        })
        .collect();

    // Wrap provider with progress if channel is available.
    let provider_for_call: std::sync::Arc<dyn provider::LlmProvider> = match &progress_tx {
        Some(tx) => provider
            .with_progress(tx.clone(), Some("unified".to_string()))
            .unwrap_or_else(|| provider.clone()),
        None => provider,
    };

    let llm_started = std::time::Instant::now();
    let unified = unified_extract_all(
        &files,
        &hints,
        &account_id,
        provider_for_call.as_ref(),
        &ctx,
    )
    .await
    .map_err(|e| {
        tracing::error!(
            error = %e,
            elapsed_ms = llm_started.elapsed().as_millis() as u64,
            "parse: unified LLM call failed"
        );
        emit_progress(
            &progress_tx,
            ProgressEvent::Error {
                code: "parse_error".to_string(),
                message: e.to_string(),
            },
        );
        provider_err_to_app_error(e)
    })?;

    tracing::info!(
        model = %unified.call.model,
        input_tokens = unified.call.usage.input_tokens,
        output_tokens = unified.call.usage.output_tokens,
        duration_ms = unified.call.duration_ms,
        stop_reason = ?unified.call.stop_reason,
        tx_extracted = unified.transactions.len(),
        holdings_extracted = unified.holdings.len(),
        investments_extracted = unified.investments.len(),
        post_validate_notes = unified.notes.len(),
        "parse: unified LLM call complete"
    );

    emit_progress(
        &progress_tx,
        ProgressEvent::Phase {
            phase: ParsePhase::PostProcessing,
            message: "Checking for duplicates".to_string(),
            task_id: None,
        },
    );

    let elapsed = start.elapsed().as_millis() as u64;

    // A truncated response means the model ran out of output budget mid-result,
    // so the extracted data is incomplete and cannot be trusted. Rather than
    // return partial rows, fail with actionable guidance to split the upload.
    if unified.call.stop_reason.as_deref() == Some("max_tokens") {
        return Err(AppError::bad_request(
            RESPONSE_TRUNCATED_MESSAGE,
            "response_truncated",
        ));
    }
    let cost = parser_call_cost("unified", &unified.call);
    let unified_notes = unified.notes;
    let extraction = ExtractionResult {
        transactions: unified.transactions,
        holdings: unified.holdings,
        investments: unified.investments,
        detected_bank: BankFormat::Unknown,
        detection_confidence: 1.0,
        calls: vec![cost],
    };

    let mut preview = {
        let db = state.db();
        let (doc_map, all_doc_ids, doc_summaries) =
            store_parse_documents(&db, &documents, Some(&account_id))
                .map_err(|e| AppError::bad_request(e.to_string(), "document_store_error"))?;
        let mut preview = build_multi_preview(
            extraction,
            &account_id,
            &account_institution,
            documents.len(),
            &db,
            elapsed,
            &doc_map,
            &all_doc_ids,
        )
        .map_err(|e| AppError::bad_request(e.to_string(), "parse_error"))?;
        preview.documents = doc_summaries;
        preview
    };

    // Attach any unified-parser notes (e.g. "dropped N unsolicited entries").
    preview.metadata.notes.extend(unified_notes);

    emit_progress(
        &progress_tx,
        ProgressEvent::Done {
            total_ms: start.elapsed().as_millis() as u64,
        },
    );

    tracing::info!(
        elapsed_ms = elapsed,
        tx_rows = preview.transactions.count,
        holdings_rows = preview.holdings.count,
        investment_rows = preview.investments.count,
        cost_usd = %preview.metadata.estimated_price.total,
        notes = preview.metadata.notes.len(),
        "parse: unified-mode response shipped"
    );
    Ok(preview)
}

// ── Tax-documents dispatch ───────────────────────────────────────────────────

const TAX_DOCUMENT_EXTENSIONS: [&str; 6] = ["pdf", "png", "jpg", "jpeg", "webp", "gif"];

/// Payslips and P60s: no account, one LLM call per file, a profile instead.
async fn run_tax_documents_path(
    state: AppState,
    files: Vec<(String, Vec<u8>)>,
    profile_id: Option<String>,
    hints: ParseHints,
    parse_id: Option<String>,
    route_started: std::time::Instant,
) -> Result<IngestionPreview, AppError> {
    use crate::importers::pricing::parser_call_cost;

    if hints.return_type.wants_statement_data() {
        return Err(AppError::bad_request(
            "tax_documents cannot be combined with transactions, holdings or investments",
            "invalid_return_type",
        ));
    }
    let profile_id = profile_id.ok_or_else(|| {
        AppError::bad_request(
            "profile_id is required for tax documents",
            "missing_profile_id",
        )
    })?;
    if !state.db().profile_exists(&profile_id)? {
        return Err(AppError::bad_request(
            format!("profile {profile_id} not found"),
            "profile_not_found",
        ));
    }
    for (filename, _) in &files {
        let ext = filename
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if !TAX_DOCUMENT_EXTENSIONS.contains(&ext.as_str()) {
            return Err(AppError::bad_request(
                format!(
                    "'{filename}' is not a PDF or image; tax documents must be .pdf, .png, .jpg, .jpeg, .webp or .gif"
                ),
                "unsupported_tax_document_format",
            ));
        }
    }
    tracing::info!(
        files = files.len(),
        agent = ?hints.agent(),
        "parse: tax documents, starting pipeline"
    );

    let progress_tx = register_progress(&state, parse_id.as_ref());
    let _progress_cleanup = ProgressCleanupGuard {
        parse_id: parse_id.clone(),
        channels: state.progress_channels.clone(),
    };
    emit_progress(
        &progress_tx,
        ProgressEvent::Phase {
            phase: ParsePhase::Preprocessing,
            message: format!("Processing {} uploaded file(s)", files.len()),
            task_id: None,
        },
    );
    let mut documents: Vec<DocumentInput> = Vec::new();
    for (filename, raw_bytes) in files {
        let doc = crate::importers::format_detection::preprocess_file(&filename, raw_bytes)
            .map_err(|e| {
                AppError::bad_request(
                    format!("failed to process '{filename}': {e}"),
                    "preprocessing_error",
                )
            })?;
        documents.push(doc);
    }

    let provider = create_provider_with_auth(hints.auth())
        .map_err(|e| AppError::bad_request(e.to_string(), "provider_config"))?;
    let start = std::time::Instant::now();
    emit_progress(
        &progress_tx,
        ProgressEvent::Phase {
            phase: ParsePhase::SendingToLlm,
            message: format!("Sending {} file(s) to AI model", documents.len()),
            task_id: None,
        },
    );

    let per_file = extract_tax_files(
        &documents,
        provider,
        hints.hint.as_deref(),
        hints.agent(),
        &progress_tx,
    )
    .await
    .map_err(|e| {
        emit_progress(
            &progress_tx,
            ProgressEvent::Error {
                code: "parse_error".to_string(),
                message: e.to_string(),
            },
        );
        provider_err_to_app_error(e)
    })?;

    for (doc, (_, call)) in documents.iter().zip(&per_file) {
        if call.stop_reason.as_deref() == Some("max_tokens") {
            return Err(AppError::bad_request(
                format!(
                    "'{}' produced more data than the model could return in a single response, \
                     so the result was truncated. Please split it into smaller files and try again.",
                    doc.filename
                ),
                "response_truncated",
            ));
        }
    }
    emit_progress(
        &progress_tx,
        ProgressEvent::Phase {
            phase: ParsePhase::PostProcessing,
            message: "Checking for duplicates".to_string(),
            task_id: None,
        },
    );

    let calls = per_file
        .iter()
        .map(|(_, call)| parser_call_cost("tax_documents", call))
        .collect();
    let parsed = per_file.into_iter().map(|(p, _)| p).collect();
    let preview = {
        let db = state.db();
        finish_tax_preview(
            &db,
            &profile_id,
            &documents,
            parsed,
            calls,
            start.elapsed().as_millis() as u64,
        )?
    };

    emit_progress(
        &progress_tx,
        ProgressEvent::Done {
            total_ms: route_started.elapsed().as_millis() as u64,
        },
    );
    tracing::info!(
        elapsed_ms = route_started.elapsed().as_millis() as u64,
        rows = preview.tax_documents.as_ref().map_or(0, |t| t.count),
        cost_usd = %preview.metadata.estimated_price.total,
        "parse: tax-documents response shipped"
    );
    Ok(preview)
}

/// One LLM call per file, in parallel; results come back in upload order.
async fn extract_tax_files(
    documents: &[DocumentInput],
    provider: std::sync::Arc<dyn provider::LlmProvider>,
    user_hint: Option<&str>,
    agent: Option<crate::model::Agent>,
    progress_tx: &Option<ProgressTx>,
) -> anyhow::Result<Vec<(ParsedTaxDocuments, provider::ProviderCallResult)>> {
    let mut join_set = tokio::task::JoinSet::new();
    for (index, doc) in documents.iter().enumerate() {
        let task_provider = match progress_tx {
            Some(tx) => provider
                .with_progress(tx.clone(), Some(format!("{}:tax_documents", doc.filename)))
                .unwrap_or_else(|| provider.clone()),
            None => provider.clone(),
        };
        let filename = doc.filename.clone();
        let mime = infer_mime(&doc.filename);
        let bytes = doc.raw_bytes.clone();
        let hint = user_hint.map(str::to_string);
        join_set.spawn(async move {
            let parser = LlmTaxDocumentParser::new(task_provider);
            let result = parser
                .extract(&filename, &mime, bytes, hint.as_deref(), agent)
                .await;
            (index, result)
        });
    }
    let mut results: Vec<Option<(ParsedTaxDocuments, provider::ProviderCallResult)>> =
        (0..documents.len()).map(|_| None).collect();
    while let Some(joined) = join_set.join_next().await {
        let (index, result) =
            joined.map_err(|e| anyhow::anyhow!("tax document task panicked: {e}"))?;
        results[index] = Some(result?);
    }
    Ok(results.into_iter().flatten().collect())
}

/// Store the uploads and turn the parsed documents into the preview. Needs
/// the database, so it runs after the LLM calls with the lock held.
fn finish_tax_preview(
    db: &crate::storage::Db,
    profile_id: &str,
    documents: &[DocumentInput],
    parsed: Vec<ParsedTaxDocuments>,
    calls: Vec<crate::model::ParserCallCost>,
    processing_time_ms: u64,
) -> Result<IngestionPreview, AppError> {
    use crate::importers::tax_documents::{assemble, validate::SourceDocuments};
    use crate::model::{
        HoldingsIngestionResult, IngestionMetadata, IngestionStatus, InvestmentIngestionResult,
        TransactionIngestionResult,
    };

    let (doc_map, all_doc_ids, doc_summaries) = store_parse_documents(db, documents, None)
        .map_err(|e| AppError::bad_request(e.to_string(), "document_store_error"))?;
    let min_row_confidence: f32 = std::env::var("FYNANCE_IMPORT_MIN_ROW_CONF")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.70);

    let mut notes = Vec::new();
    let mut detection_confidence: f32 = 0.0;
    let mut elements = Vec::new();
    for (doc, file) in documents.iter().zip(parsed) {
        detection_confidence = detection_confidence.max(file.detection_confidence);
        if file.documents.is_empty() {
            notes.push(format!(
                "No payslips or P60s were found in {}.",
                doc.filename
            ));
        }
        elements.extend(file.documents);
    }

    let tax_documents = assemble(
        db,
        profile_id,
        elements,
        min_row_confidence,
        &SourceDocuments {
            ids_by_filename: &doc_map,
            all_ids: &all_doc_ids,
        },
    )
    .map_err(|e| AppError::bad_request(e.to_string(), "parse_error"))?;

    Ok(IngestionPreview {
        status: IngestionStatus::Success,
        metadata: IngestionMetadata {
            files_processed: documents.len(),
            institution_detected: None,
            detection_confidence,
            processing_time_ms,
            notes,
            relationships_found: vec![],
            estimated_price: crate::importers::pricing::estimated_price(calls),
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
            known_holdings: vec![],
        },
        investments: InvestmentIngestionResult {
            count: 0,
            new: 0,
            duplicate: 0,
            rows: vec![],
            payload: None,
        },
        clarifications_needed: vec![],
        documents: doc_summaries,
        tax_documents: Some(tax_documents),
    })
}

// ── SSE progress endpoint ─────────────────────────────────────────────────────

pub async fn parse_progress(
    State(state): State<AppState>,
    Path(parse_id): Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // The channel is registered near the start of POST /api/parse. A client that
    // opens this stream first (the normal case) can briefly race ahead of that
    // insert, so poll for up to ~2s before treating the parse as unknown.
    let mut rx = None;
    for _ in 0..20 {
        if let Some(tx) = state.progress_channels().get(&parse_id) {
            rx = Some(tx.subscribe());
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let stream = match rx {
        Some(rx) => {
            let broadcast_stream = tokio_stream::wrappers::BroadcastStream::new(rx);
            let mapped = broadcast_stream.filter_map(|item| {
                let Ok(event) = item else { return None };
                let data = serde_json::to_string(&event).unwrap_or_default();
                let event_name = match &event {
                    ProgressEvent::Phase { .. } => "phase",
                    ProgressEvent::LlmStart { .. } => "llm_start",
                    ProgressEvent::LlmProgress { .. } => "llm_progress",
                    ProgressEvent::Done { .. } => "done",
                    ProgressEvent::Error { .. } => "error",
                };
                Some(Ok::<_, Infallible>(
                    Event::default().event(event_name).data(data),
                ))
            });
            futures_util::future::Either::Left(mapped)
        }
        None => {
            let once = futures_util::stream::once(async {
                Ok::<_, Infallible>(
                    Event::default()
                        .event("error")
                        .data(r#"{"code":"not_found","message":"No active parse with this ID"}"#),
                )
            });
            futures_util::future::Either::Right(once)
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ── ProviderError → AppError mapping ─────────────────────────────────────────

fn provider_err_to_app_error(e: anyhow::Error) -> AppError {
    if let Some(pe) = e.downcast_ref::<ProviderError>() {
        return match pe {
            ProviderError::Timeout(msg) => {
                AppError::gateway_timeout(msg.clone(), "upstream_timeout")
            }
            ProviderError::Unreachable(msg) => {
                AppError::bad_gateway(msg.clone(), "upstream_unreachable")
            }
            ProviderError::RateLimit { detail, .. } => {
                AppError::too_many_requests(detail.clone(), "upstream_rate_limit")
            }
            ProviderError::AuthRejected(msg) => AppError::bad_gateway(msg.clone(), "upstream_auth"),
            ProviderError::UpstreamServerError { body, .. } => {
                AppError::bad_gateway(body.clone(), "upstream_error")
            }
            ProviderError::UpstreamClientError { body, .. } => {
                AppError::bad_request(body.clone(), "upstream_rejected")
            }
            ProviderError::ResponseUnreadable(msg) => {
                AppError::bad_gateway(msg.clone(), "upstream_garbled")
            }
            ProviderError::NoToolUse { tool_name } => AppError::bad_gateway(
                format!("AI service did not return structured data (expected {tool_name} tool)"),
                "upstream_no_tool_use",
            ),
            ProviderError::StreamInterrupted(msg) => {
                AppError::bad_gateway(msg.clone(), "upstream_stream_interrupted")
            }
            ProviderError::NotSupported(msg) => {
                AppError::bad_request(msg.clone(), "provider_not_supported")
            }
            ProviderError::Truncated { .. } => {
                AppError::bad_request(RESPONSE_TRUNCATED_MESSAGE, "response_truncated")
            }
            ProviderError::DocumentConversion(msg) => {
                AppError::bad_request(msg.clone(), "document_conversion_failed")
            }
        };
    }
    AppError::bad_request(e.to_string(), "parse_error")
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn flatten_categories(nodes: &[crate::model::CategoryNode]) -> Vec<CategorySummary> {
    let mut out = Vec::new();
    walk_categories(nodes, None, &mut out);
    out
}

/// Emit one `CategorySummary` per leaf, with `"Parent: Child"` display
/// names (matching the format the frontend uses).
fn walk_categories(
    nodes: &[crate::model::CategoryNode],
    parent_name: Option<&str>,
    out: &mut Vec<CategorySummary>,
) {
    for node in nodes {
        if node.children.is_empty() {
            let display_name = match parent_name {
                Some(p) => format!("{p}: {}", node.name),
                None => node.name.clone(),
            };
            out.push(CategorySummary {
                id: node.id.clone(),
                name: display_name,
                description: node.description.clone(),
            });
        } else {
            walk_categories(&node.children, Some(&node.name), out);
        }
    }
}

/// `(filename -> document id, distinct document ids, document summaries)`
/// returned by [`store_parse_documents`].
type StoredParseDocuments = (
    std::collections::HashMap<String, String>,
    Vec<String>,
    Vec<crate::model::DocumentSummary>,
);

/// Persist each uploaded file as a document (deduped by content hash) and
/// return a `filename -> document_id` map plus the de-duplicated list of all
/// document ids in this parse call. Used to attribute extracted rows back to
/// their source file.
fn store_parse_documents(
    db: &crate::storage::Db,
    documents: &[DocumentInput],
    account_id: Option<&str>,
) -> anyhow::Result<StoredParseDocuments> {
    let mut by_filename = std::collections::HashMap::new();
    let mut all_ids: Vec<String> = Vec::new();
    let mut summaries: Vec<crate::model::DocumentSummary> = Vec::new();
    for doc in documents {
        let mime = infer_mime(&doc.filename);
        // CSV/Excel keep their content in `text_content` (raw_bytes is empty);
        // PDFs/images keep the original bytes in `raw_bytes`. Store whichever is
        // populated so the document isn't written as a 0-byte file. For valid
        // CSV, `text_content.as_bytes()` is the original upload byte-for-byte.
        let bytes: &[u8] = if doc.raw_bytes.is_empty() {
            doc.text_content.as_bytes()
        } else {
            &doc.raw_bytes
        };
        let (stored, _deduped) =
            db.store_document(&doc.filename, &mime, bytes, "parse", account_id)?;
        by_filename.insert(doc.filename.clone(), stored.id.clone());
        if !all_ids.contains(&stored.id) {
            all_ids.push(stored.id.clone());
            let refs = db.document_references(&stored.id)?;
            summaries.push(crate::model::DocumentSummary {
                id: stored.id,
                filename: stored.filename,
                mime_type: stored.mime_type,
                size_bytes: stored.size_bytes as usize,
                origin: stored.origin,
                account_id: stored.account_id,
                uploaded_at: stored.uploaded_at,
                reference_count: Some(refs.total()),
                orphaned: refs.total() == 0,
            });
        }
    }
    Ok((by_filename, all_ids, summaries))
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

/// Build the parse "continuity context": the caller's latest open holdings, **scoped to the
/// account being parsed**.
///
/// The scoping is load-bearing, not a nicety. `holdings_section.txt` tells the model these
/// snapshots are "for each open holding on this account" and instructs it to anchor amounts to
/// them "where the documents are silent" — i.e. exactly the month-end extrapolation path. If
/// another account's balances reach the model here, it emits them as *this* account's holdings.
/// That produced real cross-account (and cross-person) corruption before this was scoped.
fn build_holdings_context(
    rows: Vec<crate::model::HoldingSummaryRow>,
    account_id: &str,
) -> Vec<HoldingSummary> {
    rows.into_iter()
        .filter(|r| !r.holding.is_closed)
        .filter(|r| r.holding.account_id == account_id)
        .map(|r| HoldingSummary {
            account_id: r.holding.account_id,
            symbol: r.holding.symbol,
            name: r.holding.name,
            holding_type: format!("{:?}", r.holding.holding_type).to_ascii_lowercase(),
            quantity: r.holding.quantity.to_string(),
            currency: r.holding.currency,
            value: Some(r.holding.value.to_string()),
            as_of: Some(r.holding.as_of.format("%Y-%m-%d").to_string()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary_row(
        account_id: &str,
        symbol: &str,
        value: &str,
        is_closed: bool,
    ) -> crate::model::HoldingSummaryRow {
        use crate::model::{AccountType, Holding, HoldingSummaryRow, HoldingType};
        use rust_decimal::Decimal;
        use std::str::FromStr;

        HoldingSummaryRow {
            holding: Holding {
                account_id: account_id.to_string(),
                symbol: symbol.to_string(),
                name: symbol.to_string(),
                holding_type: HoldingType::Cash,
                quantity: Decimal::ZERO,
                price_per_unit: None,
                value: Decimal::from_str(value).unwrap(),
                currency: "GBP".to_string(),
                as_of: chrono::NaiveDate::from_ymd_opt(2026, 7, 31)
                    .unwrap()
                    .and_hms_opt(23, 59, 59)
                    .unwrap(),
                short_name: None,
                sub_account: None,
                is_closed,
                derived: false,
                source_document_ids: Vec::new(),
                source_file: None,
            },
            account_type: AccountType::Checking,
            institution: "Test Bank".to_string(),
        }
    }

    #[test]
    fn holdings_context_excludes_other_accounts() {
        // Regression: the continuity context used to be unscoped, so every account's
        // balances were handed to the model as if they belonged to the parsed account.
        let rows = vec![
            summary_row("ope-monzo", "_CASH_MAIN", "218.79", false),
            summary_row("ope-natwest-bills", "_CASH", "3360.21", false),
            summary_row("ope-chase-main", "_CASH_MAIN", "0.66", false),
        ];

        let ctx = build_holdings_context(rows, "ope-monzo");

        assert_eq!(
            ctx.len(),
            1,
            "only the parsed account's holdings belong here"
        );
        assert_eq!(ctx[0].account_id, "ope-monzo");
        assert_eq!(ctx[0].value.as_deref(), Some("218.79"));
        assert!(
            !ctx.iter().any(|h| h.value.as_deref() == Some("3360.21")),
            "another account's balance must never reach the parse context"
        );
    }

    #[test]
    fn holdings_context_excludes_closed_positions() {
        let rows = vec![
            summary_row("ope-monzo", "_CASH_MAIN", "218.79", false),
            summary_row("ope-monzo", "_CASH_HOUSE_FUND", "0.00", true),
        ];

        let ctx = build_holdings_context(rows, "ope-monzo");

        assert_eq!(ctx.len(), 1);
        assert_eq!(ctx[0].symbol, "_CASH_MAIN");
    }

    #[test]
    fn progress_cleanup_guard_removes_channel_on_drop() {
        let channels: ProgressChannels = Default::default();
        let (tx, _rx) = tokio::sync::broadcast::channel::<ProgressEvent>(4);
        lock_or_recover(&channels).insert("p1".to_string(), tx);

        {
            let _guard = ProgressCleanupGuard {
                parse_id: Some("p1".to_string()),
                channels: channels.clone(),
            };
        }
        assert!(
            !lock_or_recover(&channels).contains_key("p1"),
            "dropping the guard must remove the channel entry"
        );
    }

    #[test]
    fn progress_cleanup_guard_without_parse_id_is_noop() {
        let channels: ProgressChannels = Default::default();
        let (tx, _rx) = tokio::sync::broadcast::channel::<ProgressEvent>(4);
        lock_or_recover(&channels).insert("p2".to_string(), tx);

        drop(ProgressCleanupGuard {
            parse_id: None,
            channels: channels.clone(),
        });
        assert!(lock_or_recover(&channels).contains_key("p2"));
    }
}
