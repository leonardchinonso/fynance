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
use crate::importers::unified_parser::{
    CategorySummary, HoldingSummary, UnifiedContext, extract_all as unified_extract_all,
};
use crate::model::IngestionPreview;
use crate::server::error::AppError;
use crate::server::state::{AppState, lock_or_recover};

const MAX_FILE_SIZE: usize = 10 * 1024 * 1024; // 10 MB per file
const MAX_TOTAL_SIZE: usize = 50 * 1024 * 1024; // 50 MB total
const MAX_FILES: usize = 5;

// ── POST /api/parse ─────────────────────────────────────────────────────────

pub async fn parse_documents(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<IngestionPreview>, AppError> {
    let route_started = std::time::Instant::now();
    tracing::info!("POST /api/parse: request received");
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut account_id: Option<String> = None;
    let mut hints: Option<ParseHints> = None;
    let mut parse_id: Option<String> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        tracing::error!(error = %e, "parse: multipart parse error");
        AppError::bad_request(format!("multipart error: {e}"), "invalid_multipart")
    })? {
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

    // Require hints
    let hints = hints.ok_or_else(|| {
        AppError::bad_request("hints is required for the parse endpoint", "missing_hints")
    })?;

    // Validate return_type
    if !hints.return_type.is_valid() {
        return Err(AppError::bad_request(
            "return_type must have at least one extraction type enabled (transactions, holdings.enabled, or investments)",
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

    // ── Progress channel setup ─────────────────────────────────────────────
    let progress_tx: Option<ProgressTx> = parse_id.as_ref().map(|pid| {
        let (tx, _rx) = tokio::sync::broadcast::channel::<ProgressEvent>(64);
        state.progress_channels().insert(pid.clone(), tx.clone());

        let channels = state.progress_channels.clone();
        let pid_clone = pid.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(600)).await;
            lock_or_recover(&channels).remove(&pid_clone);
        });

        tx
    });

    let cleanup_progress = {
        let channels = state.progress_channels.clone();
        let pid = parse_id.clone();
        move || {
            if let Some(pid) = &pid {
                lock_or_recover(&channels).remove(pid);
            }
        }
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
        match result {
            Ok(preview) => {
                cleanup_progress();
                return Ok(Json(preview));
            }
            Err(e) => {
                cleanup_progress();
                return Err(e);
            }
        }
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
        cleanup_progress();
        provider_err_to_app_error(e)
    })?;
    let elapsed = start.elapsed().as_millis() as u64;

    // If clarification needed, return early
    if let PipelineOutcome::NeedsClarification(mut preview) = pipeline_result {
        preview.metadata.processing_time_ms = elapsed;
        cleanup_progress();
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
            store_parse_documents(&db, &documents, &account_id)
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
    cleanup_progress();

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
            .map(|rows| {
                rows.into_iter()
                    .filter(|r| !r.holding.is_closed)
                    .map(|r| HoldingSummary {
                        symbol: r.holding.symbol,
                        name: r.holding.name,
                        holding_type: format!("{:?}", r.holding.holding_type).to_ascii_lowercase(),
                        quantity: r.holding.quantity.to_string(),
                        currency: r.holding.currency,
                        value: Some(r.holding.value.to_string()),
                        as_of: Some(r.holding.as_of.format("%Y-%m-%d").to_string()),
                    })
                    .collect()
            })
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
            "The uploaded document(s) produced more data than the model could return in a \
             single response, so the result was truncated. Please split your upload into \
             smaller batches (we recommend a few months at a time) and try again.",
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
            store_parse_documents(&db, &documents, &account_id)
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
    account_id: &str,
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
            db.store_document(&doc.filename, &mime, bytes, "parse", Some(account_id))?;
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
