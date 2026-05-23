//! Stage 1 parse endpoint: POST /api/parse.
//! Accepts uploaded documents (1-5 CSV files), invokes the LLM pipeline to
//! extract data, runs dedup checks, and returns a structured IngestionPreview.

use axum::Json;
use axum::extract::{Multipart, State};

use crate::importers::document_parser::{
    DocumentInput, ParseHints, ParseMode, PipelineOutcome, build_multi_preview,
    run_multi_file_pipeline,
};
use crate::importers::provider::create_provider;
use crate::importers::unified_parser::{
    CategorySummary, HoldingSummary, UnifiedContext, extract_all as unified_extract_all,
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
    let route_started = std::time::Instant::now();
    tracing::info!("POST /api/parse: request received");
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut account_id: Option<String> = None;
    let mut hints: Option<ParseHints> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "parse: multipart parse error");
            AppError::bad_request(format!("multipart error: {e}"), "invalid_multipart")
        })?
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
            _ => {
                let _ = field.bytes().await;
            }
        }
    }

    tracing::info!(
        files = files.len(),
        has_account_id = account_id.is_some(),
        has_hints = hints.is_some(),
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
    if hints.return_type.holdings.period.is_some() && !hints.return_type.transactions {
        return Err(AppError::bad_request(
            "holdings.period requires transactions to be enabled (periodic snapshots are derived from transaction data)",
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

    // Look up account (need institution for pipeline)
    let account = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.get_account_by_id(&account_id)?.ok_or_else(|| {
            AppError::bad_request(
                format!("account {} not found", account_id),
                "account_not_found",
            )
        })?
    };

    // Create the LLM provider (reads FYNANCE_PARSE_PROVIDER from env).
    // Surface the config message: this is almost always a missing API key,
    // and hiding it as a generic 500 makes the issue impossible to debug.
    let provider = create_provider()
        .map_err(|e| AppError::bad_request(e.to_string(), "provider_config"))?;

    let start = std::time::Instant::now();

    // ── Unified mode dispatch ───────────────────────────────────────────────
    if hints.mode() == ParseMode::Unified {
        return run_unified_path(state, hints, documents, account_id, account.institution, provider, start)
            .await
            .map(Json);
    }

    // Run the multi-file LLM pipeline (split mode)
    let pipeline_result =
        run_multi_file_pipeline(&documents, &hints, &account.institution, provider)
            .await
            .map_err(|e| AppError::bad_request(e.to_string(), "parse_error"))?;
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

    // Run deduplication (needs DB, synchronous)
    let preview = {
        let db = state.db.lock().expect("db mutex poisoned");
        build_multi_preview(
            merged_extraction,
            &account_id,
            &account.institution,
            documents.len(),
            &db,
            elapsed,
        )
    }
    .map_err(|e| AppError::bad_request(e.to_string(), "parse_error"))?;

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

async fn run_unified_path(
    state: AppState,
    hints: ParseHints,
    documents: Vec<DocumentInput>,
    account_id: String,
    account_institution: String,
    provider: std::sync::Arc<dyn crate::importers::provider::LlmProvider>,
    start: std::time::Instant,
) -> Result<IngestionPreview, AppError> {
    use crate::importers::document_parser::ExtractionResult;
    use crate::importers::pricing::parser_call_cost;
    use crate::model::BankFormat;

    // Build UnifiedContext: active leaf categories + last open holdings.
    let ctx = {
        let db = state.db.lock().expect("db mutex poisoned");
        let category_tree = db.get_categories_tree().unwrap_or_default();
        let categories: Vec<CategorySummary> = category_tree
            .values()
            .flat_map(|roots| flatten_categories(roots))
            .collect();
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
    let files: Vec<(String, String, Vec<u8>)> = documents
        .iter()
        .map(|d| (d.filename.clone(), infer_mime(&d.filename), d.raw_bytes.clone()))
        .collect();

    let llm_started = std::time::Instant::now();
    let unified = unified_extract_all(&files, &hints, &account_id, provider.as_ref(), &ctx)
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                elapsed_ms = llm_started.elapsed().as_millis() as u64,
                "parse: unified LLM call failed"
            );
            AppError::bad_request(e.to_string(), "parse_error")
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

    let elapsed = start.elapsed().as_millis() as u64;

    let cost = parser_call_cost("unified", &unified.call);
    let mut unified_notes = unified.notes;
    if unified.call.stop_reason.as_deref() == Some("max_tokens") {
        unified_notes.push(format!(
            "LLM response was truncated (hit max_tokens cap; output_tokens={}). \
             Results below are incomplete. Try fewer or smaller files, or a different agent.",
            unified.call.usage.output_tokens
        ));
    }
    let extraction = ExtractionResult {
        transactions: unified.transactions,
        holdings: unified.holdings,
        investments: unified.investments,
        detected_bank: BankFormat::Unknown,
        detection_confidence: 1.0,
        calls: vec![cost],
    };

    let mut preview = {
        let db = state.db.lock().expect("db mutex poisoned");
        build_multi_preview(
            extraction,
            &account_id,
            &account_institution,
            documents.len(),
            &db,
            elapsed,
        )
    }
    .map_err(|e| AppError::bad_request(e.to_string(), "parse_error"))?;

    // Attach any unified-parser notes (e.g. "dropped N unsolicited entries").
    preview.metadata.notes.extend(unified_notes);

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

