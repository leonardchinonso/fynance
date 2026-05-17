//! Document parser pipeline for the 2-stage import redesign.
//! Orchestrates: preprocess -> classify -> extract -> deduplicate.
//!
//! Phase 2: Multi-file CSV support with cross-document relationship detection.

use anyhow::{Result, anyhow};
use chrono::Local;
use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::task::JoinSet;

use crate::importers::llm_parser::StatementParser;
use crate::importers::unified::UnifiedStatementRow;
use crate::model::{
    BankFormat, CategorySource, ClarificationRequest, Holding, HoldingType, HoldingsImportPayload,
    HoldingsIngestionResult, ImportPayload, ImportTransaction, IngestionMetadata, IngestionPreview,
    IngestionStatus, InvestmentIngestionResult, TransactionIngestionResult,
    TransactionPreviewStatus,
};
use crate::storage::Db;

use super::holdings_parser::{HoldingsExtractor, LlmHoldingsParser, ParsedHoldings};

// ── Public types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileFormat {
    Csv,
}

#[derive(Debug, Clone)]
pub struct DocumentInput {
    pub filename: String,
    pub format: FileFormat,
    pub text_content: String,
    pub original_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    Transactions,
    Holdings,
    Both,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ClassificationResult {
    pub institution: Option<String>,
    pub institution_confidence: f32,
    pub content_type: ContentType,
}

#[derive(Debug, Clone)]
pub struct MultiClassificationResult {
    pub files: Vec<FileClassification>,
    pub relationships: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FileClassification {
    pub filename: String,
    pub institution: Option<String>,
    pub institution_confidence: f32,
    pub content_type: ContentType,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ParseHints {
    pub institution: Option<String>,
    pub expected_data: Option<Vec<String>>,
    #[allow(dead_code)]
    pub date_format: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExtractionResult {
    pub transactions: Vec<UnifiedStatementRow>,
    pub holdings: Vec<Holding>,
    pub detected_bank: BankFormat,
    pub detection_confidence: f32,
}

#[derive(Debug)]
pub enum PipelineOutcome {
    Success {
        classification: MultiClassificationResult,
        extraction: ExtractionResult,
    },
    NeedsClarification(Box<IngestionPreview>),
}

// ── Constants ───────────────────────────────────────────────────────────────

const CLASSIFIER_PROMPT: &str = include_str!("../../config/prompts/document_classifier.txt");
const MULTI_CLASSIFIER_PROMPT: &str =
    include_str!("../../config/prompts/multi_document_classifier.txt");
const CLASSIFICATION_SAMPLE_BYTES: usize = 10_000;
const CONFIDENCE_THRESHOLD: f32 = 0.7;

// ── Public API ──────────────────────────────────────────────────────────────

/// Run the multi-file LLM pipeline: classify all -> extract each -> merge.
/// Does NOT touch the database.
pub async fn run_multi_file_pipeline(
    documents: &[DocumentInput],
    hints: &ParseHints,
) -> Result<PipelineOutcome> {
    let multi_classification = classify_documents(documents, hints).await?;

    // Check for low-confidence files with unknown content type
    let unclear_files: Vec<&FileClassification> = multi_classification
        .files
        .iter()
        .filter(|f| {
            f.institution_confidence < CONFIDENCE_THRESHOLD
                && f.content_type == ContentType::Unknown
        })
        .collect();

    if !unclear_files.is_empty() {
        let clarifications: Vec<ClarificationRequest> = unclear_files
            .iter()
            .map(|f| ClarificationRequest {
                file: f.filename.clone(),
                question: format!(
                    "Cannot determine the content type of '{}' (confidence: {:.0}%). \
                     What institution is this from and what data does it contain?",
                    f.filename,
                    f.institution_confidence * 100.0,
                ),
                suggestions: vec![
                    "monzo".to_string(),
                    "revolut".to_string(),
                    "trading_212".to_string(),
                    "lloyds".to_string(),
                ],
            })
            .collect();

        let preview = IngestionPreview {
            status: IngestionStatus::NeedsClarification,
            metadata: IngestionMetadata {
                files_processed: documents.len(),
                institution_detected: None,
                detection_confidence: 0.0,
                processing_time_ms: 0,
                notes: vec![],
                relationships_found: multi_classification.relationships.clone(),
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
            clarifications_needed: clarifications,
        };
        return Ok(PipelineOutcome::NeedsClarification(Box::new(preview)));
    }

    // Parallel extraction
    let extraction = extract_all_parallel(documents, &multi_classification).await?;

    Ok(PipelineOutcome::Success {
        classification: multi_classification,
        extraction,
    })
}

/// Run deduplication checks and assemble the final IngestionPreview.
/// Requires DB access. Called synchronously after the LLM pipeline completes.
pub fn build_multi_preview(
    classification: &MultiClassificationResult,
    extraction: ExtractionResult,
    account_id: &str,
    db: &Db,
    processing_time_ms: u64,
) -> Result<IngestionPreview> {
    let min_row_confidence: f32 = std::env::var("FYNANCE_IMPORT_MIN_ROW_CONF")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.70);

    // Determine primary institution (highest confidence across files)
    let primary = classification
        .files
        .iter()
        .max_by(|a, b| {
            a.institution_confidence
                .partial_cmp(&b.institution_confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|f| (f.institution.clone(), f.institution_confidence));

    let (institution_detected, detection_confidence) = primary.unwrap_or((None, 0.0));

    // ── Transaction deduplication ────────────────────────────────────────────

    let tx_result = if !extraction.transactions.is_empty() {
        let preview_rows = db.dry_run_transactions_from_parsed(
            account_id,
            &extraction.transactions,
            min_row_confidence,
        )?;

        let import_transactions: Vec<ImportTransaction> = extraction
            .transactions
            .iter()
            .filter(|r| r.row_confidence >= min_row_confidence)
            .map(|r| ImportTransaction {
                date: r.date,
                description: r
                    .merchant
                    .as_deref()
                    .filter(|m| !m.is_empty())
                    .unwrap_or(&r.description)
                    .to_string(),
                amount: r.amount,
                currency: Some(r.currency.clone()),
                category: r.category.clone(),
                category_id: None,
                category_source: r.category.as_ref().map(|_| CategorySource::Rule),
                notes: r.notes.clone(),
                is_recurring: None,
                exclude_from_summary: None,
            })
            .collect();

        let rows_new = preview_rows
            .iter()
            .filter(|r| r.status == TransactionPreviewStatus::New)
            .count();
        let rows_dup = preview_rows
            .iter()
            .filter(|r| r.status == TransactionPreviewStatus::Duplicate)
            .count();
        let rows_err = preview_rows
            .iter()
            .filter(|r| r.status == TransactionPreviewStatus::Error)
            .count();

        TransactionIngestionResult {
            count: preview_rows.len(),
            new: rows_new,
            duplicate: rows_dup,
            errors: rows_err,
            rows: preview_rows,
            payload: Some(ImportPayload {
                account_id: account_id.to_string(),
                transactions: import_transactions,
            }),
        }
    } else {
        TransactionIngestionResult {
            count: 0,
            new: 0,
            duplicate: 0,
            errors: 0,
            rows: vec![],
            payload: None,
        }
    };

    // ── Holdings deduplication ───────────────────────────────────────────────

    let holdings_result = if !extraction.holdings.is_empty() {
        let holdings_with_account: Vec<Holding> = extraction
            .holdings
            .into_iter()
            .map(|mut h| {
                h.account_id = account_id.to_string();
                h
            })
            .collect();

        let previews = db.dry_run_holdings(account_id, &holdings_with_account)?;

        let rows_new = previews.iter().filter(|p| p.status == "new").count();
        let rows_modify = previews.iter().filter(|p| p.status == "modify").count();

        HoldingsIngestionResult {
            count: previews.len(),
            new: rows_new,
            modify: rows_modify,
            rows: previews,
            payload: Some(HoldingsImportPayload {
                account_id: account_id.to_string(),
                holdings: holdings_with_account,
            }),
        }
    } else {
        HoldingsIngestionResult {
            count: 0,
            new: 0,
            modify: 0,
            rows: vec![],
            payload: None,
        }
    };

    // ── Assemble ────────────────────────────────────────────────────────────

    Ok(IngestionPreview {
        status: IngestionStatus::Success,
        metadata: IngestionMetadata {
            files_processed: classification.files.len(),
            institution_detected,
            detection_confidence,
            processing_time_ms,
            notes: vec![],
            relationships_found: classification.relationships.clone(),
        },
        transactions: tx_result,
        holdings: holdings_result,
        investments: InvestmentIngestionResult {
            count: 0,
            new: 0,
            duplicate: 0,
            rows: vec![],
            payload: None,
        },
        clarifications_needed: vec![],
    })
}

// ── Classification ──────────────────────────────────────────────────────────

/// Classify all documents. Single-file with complete hints skips the LLM.
/// Multi-file always calls the LLM for relationship detection.
async fn classify_documents(
    documents: &[DocumentInput],
    hints: &ParseHints,
) -> Result<MultiClassificationResult> {
    // Single-file with complete hints: skip LLM entirely
    if documents.len() == 1 {
        if let (Some(institution), Some(expected_data)) = (&hints.institution, &hints.expected_data)
        {
            if !expected_data.is_empty() {
                let content_type = resolve_content_type(expected_data);
                return Ok(MultiClassificationResult {
                    files: vec![FileClassification {
                        filename: documents[0].filename.clone(),
                        institution: Some(institution.clone()),
                        institution_confidence: 1.0,
                        content_type,
                    }],
                    relationships: vec![],
                });
            }
        }
    }

    let api_key = std::env::var("FYNANCE_ANTHROPIC_API_KEY").map_err(|_| {
        anyhow!("FYNANCE_ANTHROPIC_API_KEY is not set. Required for document parsing.")
    })?;
    let model = std::env::var("FYNANCE_PARSE_CLASSIFY_MODEL")
        .unwrap_or_else(|_| "claude-haiku-4-5-20251001".to_string());

    // Use single-file prompt for 1 file, multi-file prompt for 2+
    let (system_prompt, tool_name, tool_schema, user_message) = if documents.len() == 1 {
        let sample = &documents[0].text_content[..documents[0]
            .text_content
            .len()
            .min(CLASSIFICATION_SAMPLE_BYTES)];
        let msg = format!("filename: {}\n\n{}", documents[0].filename, sample);
        (
            CLASSIFIER_PROMPT,
            "classify_document",
            build_single_classification_tool_schema(),
            msg,
        )
    } else {
        let msg = build_multi_file_classification_message(documents);
        (
            MULTI_CLASSIFIER_PROMPT,
            "classify_documents",
            build_multi_classification_tool_schema(),
            msg,
        )
    };

    let request_body = json!({
        "model": model,
        "max_tokens": 2048,
        "system": system_prompt,
        "tools": [{
            "name": tool_name,
            "description": "Classify financial documents by institution and content type.",
            "input_schema": tool_schema
        }],
        "tool_choice": { "type": "tool", "name": tool_name },
        "messages": [{
            "role": "user",
            "content": user_message
        }]
    });

    let client = Client::new();
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| anyhow!("classification request failed: {e}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| anyhow!("reading classification response: {e}"))?;

    if !status.is_success() {
        return Err(anyhow!(
            "Anthropic API returned {status} during classification: {body}"
        ));
    }

    if documents.len() == 1 {
        parse_single_classification_response(&body, documents)
    } else {
        parse_multi_classification_response(&body, documents)
    }
}

// ── Parallel extraction ─────────────────────────────────────────────────────

async fn extract_all_parallel(
    documents: &[DocumentInput],
    classification: &MultiClassificationResult,
) -> Result<ExtractionResult> {
    let mut join_set: JoinSet<Result<ExtractionResult>> = JoinSet::new();

    for (doc, file_class) in documents.iter().zip(classification.files.iter()) {
        if file_class.content_type == ContentType::Unknown {
            continue;
        }

        let text = doc.text_content.clone();
        let filename = doc.filename.clone();
        let content_type = file_class.content_type.clone();

        join_set.spawn(async move { extract_single_file(&text, &filename, &content_type).await });
    }

    let mut merged = ExtractionResult {
        transactions: vec![],
        holdings: vec![],
        detected_bank: BankFormat::Unknown,
        detection_confidence: 0.0,
    };

    let mut max_confidence: f32 = 0.0;

    while let Some(result) = join_set.join_next().await {
        let extraction = result
            .map_err(|e| anyhow!("extraction task panicked: {e}"))?
            .map_err(|e| anyhow!("extraction failed: {e}"))?;

        merged.transactions.extend(extraction.transactions);
        merged.holdings.extend(extraction.holdings);

        if extraction.detection_confidence > max_confidence {
            max_confidence = extraction.detection_confidence;
            merged.detected_bank = extraction.detected_bank;
        }
    }

    merged.detection_confidence = max_confidence;
    Ok(merged)
}

async fn extract_single_file(
    text: &str,
    filename: &str,
    content_type: &ContentType,
) -> Result<ExtractionResult> {
    match content_type {
        ContentType::Transactions => {
            let parser = crate::importers::llm_parser::LlmStatementParser::from_env()?;
            let parsed = parser.parse(text, filename).await?;
            Ok(ExtractionResult {
                transactions: parsed.rows,
                holdings: vec![],
                detected_bank: parsed.detected_bank,
                detection_confidence: parsed.detection_confidence,
            })
        }
        ContentType::Holdings => {
            let parser = LlmHoldingsParser::from_env()?;
            let parsed = parser.extract_holdings(text, filename).await?;
            let holdings = convert_parsed_holdings(&parsed)?;
            Ok(ExtractionResult {
                transactions: vec![],
                holdings,
                detected_bank: BankFormat::Unknown,
                detection_confidence: parsed.detection_confidence,
            })
        }
        ContentType::Both => {
            let tx_parser = crate::importers::llm_parser::LlmStatementParser::from_env()?;
            let h_parser = LlmHoldingsParser::from_env()?;

            let (tx_result, h_result) = tokio::join!(
                tx_parser.parse(text, filename),
                h_parser.extract_holdings(text, filename)
            );

            let tx_parsed = tx_result?;
            let h_parsed = h_result?;
            let holdings = convert_parsed_holdings(&h_parsed)?;

            Ok(ExtractionResult {
                transactions: tx_parsed.rows,
                holdings,
                detected_bank: tx_parsed.detected_bank,
                detection_confidence: tx_parsed.detection_confidence,
            })
        }
        ContentType::Unknown => Err(anyhow!("cannot extract from Unknown content type")),
    }
}

// ── Classification helpers ──────────────────────────────────────────────────

fn build_multi_file_classification_message(documents: &[DocumentInput]) -> String {
    let mut message = String::new();
    message.push_str(&format!(
        "I have {} file(s) to classify:\n\n",
        documents.len()
    ));

    for (i, doc) in documents.iter().enumerate() {
        let sample = &doc.text_content[..doc.text_content.len().min(CLASSIFICATION_SAMPLE_BYTES)];
        message.push_str(&format!(
            "--- FILE {} ---\nfilename: {}\nsize: {} bytes\n\n{}\n\n",
            i + 1,
            doc.filename,
            doc.original_size,
            sample
        ));
    }

    message
}

fn build_single_classification_tool_schema() -> Value {
    json!({
        "type": "object",
        "required": ["institution", "institution_confidence", "content_type"],
        "properties": {
            "institution": {
                "type": "string",
                "description": "Institution identifier in lowercase_snake_case, or 'unknown'."
            },
            "institution_confidence": {
                "type": "number",
                "minimum": 0.0,
                "maximum": 1.0,
                "description": "Confidence that the institution was correctly identified."
            },
            "content_type": {
                "type": "string",
                "enum": ["transactions", "holdings", "both"],
                "description": "What type of financial data this document contains."
            }
        }
    })
}

fn build_multi_classification_tool_schema() -> Value {
    json!({
        "type": "object",
        "required": ["files", "relationships"],
        "properties": {
            "files": {
                "type": "array",
                "description": "Classification for each input file, in the same order as provided.",
                "items": {
                    "type": "object",
                    "required": ["filename", "institution", "institution_confidence", "content_type"],
                    "properties": {
                        "filename": {
                            "type": "string",
                            "description": "The filename as provided in the input."
                        },
                        "institution": {
                            "type": "string",
                            "description": "Institution identifier in lowercase_snake_case, or 'unknown'."
                        },
                        "institution_confidence": {
                            "type": "number",
                            "minimum": 0.0,
                            "maximum": 1.0
                        },
                        "content_type": {
                            "type": "string",
                            "enum": ["transactions", "holdings", "both"]
                        }
                    }
                }
            },
            "relationships": {
                "type": "array",
                "description": "Detected relationships between files. Empty if only one file or files are unrelated.",
                "items": {
                    "type": "string"
                }
            }
        }
    })
}

fn parse_single_classification_response(
    body: &str,
    documents: &[DocumentInput],
) -> Result<MultiClassificationResult> {
    let api_resp: ApiResponse =
        serde_json::from_str(body).map_err(|e| anyhow!("parsing classification response: {e}"))?;

    let tool_input = api_resp
        .content
        .into_iter()
        .find_map(|block| match block {
            ContentBlock::ToolUse {
                block_type,
                name,
                input,
            } if block_type == "tool_use" && name == "classify_document" => Some(input),
            _ => None,
        })
        .ok_or_else(|| anyhow!("no classify_document tool_use block in response"))?;

    let institution = tool_input
        .get("institution")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| s != "unknown");

    let institution_confidence = tool_input
        .get("institution_confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as f32;

    let content_type_str = tool_input
        .get("content_type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let content_type = match content_type_str {
        "transactions" => ContentType::Transactions,
        "holdings" => ContentType::Holdings,
        "both" => ContentType::Both,
        _ => ContentType::Unknown,
    };

    Ok(MultiClassificationResult {
        files: vec![FileClassification {
            filename: documents[0].filename.clone(),
            institution,
            institution_confidence,
            content_type,
        }],
        relationships: vec![],
    })
}

fn parse_multi_classification_response(
    body: &str,
    documents: &[DocumentInput],
) -> Result<MultiClassificationResult> {
    let api_resp: ApiResponse =
        serde_json::from_str(body).map_err(|e| anyhow!("parsing classification response: {e}"))?;

    let tool_input = api_resp
        .content
        .into_iter()
        .find_map(|block| match block {
            ContentBlock::ToolUse {
                block_type,
                name,
                input,
            } if block_type == "tool_use" && name == "classify_documents" => Some(input),
            _ => None,
        })
        .ok_or_else(|| anyhow!("no classify_documents tool_use block in response"))?;

    let files_arr = tool_input
        .get("files")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("classification response missing 'files' array"))?;

    if files_arr.len() != documents.len() {
        tracing::warn!(
            expected = documents.len(),
            got = files_arr.len(),
            "classification returned different number of files than uploaded"
        );
    }

    let mut files = Vec::new();
    for (i, file_val) in files_arr.iter().enumerate() {
        let filename = file_val
            .get("filename")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                documents
                    .get(i)
                    .map(|d| d.filename.as_str())
                    .unwrap_or("unknown")
            })
            .to_string();

        let institution = file_val
            .get("institution")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| s != "unknown");

        let institution_confidence = file_val
            .get("institution_confidence")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32;

        let content_type_str = file_val
            .get("content_type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let content_type = match content_type_str {
            "transactions" => ContentType::Transactions,
            "holdings" => ContentType::Holdings,
            "both" => ContentType::Both,
            _ => ContentType::Unknown,
        };

        files.push(FileClassification {
            filename,
            institution,
            institution_confidence,
            content_type,
        });
    }

    let relationships = tool_input
        .get("relationships")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Ok(MultiClassificationResult {
        files,
        relationships,
    })
}

fn resolve_content_type(expected_data: &[String]) -> ContentType {
    let has_holdings = expected_data.contains(&"holdings".to_string());
    let has_transactions = expected_data.contains(&"transactions".to_string());
    match (has_transactions, has_holdings) {
        (true, true) => ContentType::Both,
        (false, true) => ContentType::Holdings,
        (true, false) => ContentType::Transactions,
        (false, false) => ContentType::Unknown,
    }
}

// ── Holdings conversion ─────────────────────────────────────────────────────

fn convert_parsed_holdings(parsed: &ParsedHoldings) -> Result<Vec<Holding>> {
    let now = Local::now().naive_local();
    let mut holdings = Vec::new();

    for row in &parsed.rows {
        if row.row_confidence < 0.70 {
            continue;
        }

        let holding_type = parse_holding_type_fuzzy(&row.holding_type);
        let quantity: Decimal = row
            .quantity
            .parse()
            .map_err(|_| anyhow!("invalid quantity '{}' for {}", row.quantity, row.symbol))?;
        let value: Decimal = row
            .value
            .parse()
            .map_err(|_| anyhow!("invalid value '{}' for {}", row.value, row.symbol))?;
        let price_per_unit: Option<Decimal> = row
            .price_per_unit
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse()
                    .map_err(|_| anyhow!("invalid price_per_unit '{}' for {}", s, row.symbol))
            })
            .transpose()?;

        holdings.push(Holding {
            account_id: String::new(),
            symbol: row.symbol.clone(),
            name: row.name.clone(),
            holding_type,
            quantity,
            price_per_unit,
            value,
            currency: row.currency.clone(),
            as_of: now,
            short_name: Some(row.symbol.clone()),
            sub_account: row.sub_account.clone(),
            is_closed: false,
        });
    }

    Ok(holdings)
}

fn parse_holding_type_fuzzy(s: &str) -> HoldingType {
    match s.to_ascii_lowercase().as_str() {
        "stock" | "equity" | "share" | "shares" => HoldingType::Stock,
        "etf" | "exchange_traded_fund" => HoldingType::Etf,
        "fund" | "mutual_fund" | "unit_trust" | "oeic" => HoldingType::Fund,
        "bond" | "gilt" | "fixed_income" => HoldingType::Bond,
        "crypto" | "cryptocurrency" | "digital_asset" => HoldingType::Crypto,
        "cash" | "money" | "deposit" => HoldingType::Cash,
        "property" | "real_estate" | "reit" => HoldingType::Property,
        "loan" | "debt" => HoldingType::Loan,
        "credit" | "credit_card" => HoldingType::Credit,
        _ => HoldingType::Stock,
    }
}

// ── Shared Anthropic response types ─────────────────────────────────────────

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ContentBlock {
    ToolUse {
        #[serde(rename = "type")]
        block_type: String,
        name: String,
        input: Value,
    },
    #[allow(dead_code)]
    Other(Value),
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_holding_type_fuzzy() {
        assert_eq!(parse_holding_type_fuzzy("stock"), HoldingType::Stock);
        assert_eq!(parse_holding_type_fuzzy("ETF"), HoldingType::Etf);
        assert_eq!(parse_holding_type_fuzzy("equity"), HoldingType::Stock);
        assert_eq!(parse_holding_type_fuzzy("mutual_fund"), HoldingType::Fund);
        assert_eq!(parse_holding_type_fuzzy("CRYPTO"), HoldingType::Crypto);
        assert_eq!(
            parse_holding_type_fuzzy("unknown_thing"),
            HoldingType::Stock
        );
    }

    #[test]
    fn test_resolve_content_type() {
        assert_eq!(
            resolve_content_type(&["transactions".to_string()]),
            ContentType::Transactions
        );
        assert_eq!(
            resolve_content_type(&["holdings".to_string()]),
            ContentType::Holdings
        );
        assert_eq!(
            resolve_content_type(&["transactions".to_string(), "holdings".to_string()]),
            ContentType::Both
        );
        assert_eq!(
            resolve_content_type(&["other".to_string()]),
            ContentType::Unknown
        );
    }

    #[test]
    fn test_single_file_hints_skip_classification() {
        let hints = ParseHints {
            institution: Some("monzo".to_string()),
            expected_data: Some(vec!["transactions".to_string()]),
            date_format: None,
        };

        let doc = DocumentInput {
            filename: "test.csv".to_string(),
            format: FileFormat::Csv,
            text_content: "header\nrow1".to_string(),
            original_size: 15,
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(classify_documents(&[doc], &hints)).unwrap();

        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].institution, Some("monzo".to_string()));
        assert_eq!(result.files[0].institution_confidence, 1.0);
        assert_eq!(result.files[0].content_type, ContentType::Transactions);
        assert!(result.relationships.is_empty());
    }

    #[test]
    fn test_single_file_hints_holdings() {
        let hints = ParseHints {
            institution: Some("trading_212".to_string()),
            expected_data: Some(vec!["holdings".to_string()]),
            date_format: None,
        };

        let doc = DocumentInput {
            filename: "positions.csv".to_string(),
            format: FileFormat::Csv,
            text_content: "header\nrow1".to_string(),
            original_size: 15,
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(classify_documents(&[doc], &hints)).unwrap();

        assert_eq!(result.files[0].institution, Some("trading_212".to_string()));
        assert_eq!(result.files[0].content_type, ContentType::Holdings);
    }

    #[test]
    fn test_multi_file_hints_does_not_skip_classification() {
        // With 2+ files, the LLM is always called (for relationship detection).
        // Without a valid API key this will error, proving it tried the LLM path.
        let docs = vec![
            DocumentInput {
                filename: "a.csv".to_string(),
                format: FileFormat::Csv,
                text_content: "h\nr".to_string(),
                original_size: 3,
            },
            DocumentInput {
                filename: "b.csv".to_string(),
                format: FileFormat::Csv,
                text_content: "h\nr".to_string(),
                original_size: 3,
            },
        ];
        let hints = ParseHints {
            institution: Some("monzo".to_string()),
            expected_data: Some(vec!["transactions".to_string()]),
            date_format: None,
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(classify_documents(&docs, &hints));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_multi_classification_response_basic() {
        let body = r#"{
            "content": [{
                "type": "tool_use",
                "id": "test",
                "name": "classify_documents",
                "input": {
                    "files": [
                        {
                            "filename": "monzo.csv",
                            "institution": "monzo",
                            "institution_confidence": 0.95,
                            "content_type": "transactions"
                        },
                        {
                            "filename": "t212.csv",
                            "institution": "trading_212",
                            "institution_confidence": 0.92,
                            "content_type": "holdings"
                        }
                    ],
                    "relationships": [
                        "Files are from different institutions (Monzo for transactions, Trading 212 for holdings)"
                    ]
                }
            }]
        }"#;

        let docs = vec![
            DocumentInput {
                filename: "monzo.csv".to_string(),
                format: FileFormat::Csv,
                text_content: String::new(),
                original_size: 0,
            },
            DocumentInput {
                filename: "t212.csv".to_string(),
                format: FileFormat::Csv,
                text_content: String::new(),
                original_size: 0,
            },
        ];

        let result = parse_multi_classification_response(body, &docs).unwrap();
        assert_eq!(result.files.len(), 2);
        assert_eq!(result.files[0].institution, Some("monzo".to_string()));
        assert_eq!(result.files[0].content_type, ContentType::Transactions);
        assert_eq!(result.files[1].institution, Some("trading_212".to_string()));
        assert_eq!(result.files[1].content_type, ContentType::Holdings);
        assert_eq!(result.relationships.len(), 1);
    }

    #[test]
    fn test_parse_single_classification_response() {
        let body = r#"{
            "content": [{
                "type": "tool_use",
                "id": "test",
                "name": "classify_document",
                "input": {
                    "institution": "monzo",
                    "institution_confidence": 0.97,
                    "content_type": "transactions"
                }
            }]
        }"#;

        let docs = vec![DocumentInput {
            filename: "monzo_may.csv".to_string(),
            format: FileFormat::Csv,
            text_content: String::new(),
            original_size: 0,
        }];

        let result = parse_single_classification_response(body, &docs).unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].filename, "monzo_may.csv");
        assert_eq!(result.files[0].institution, Some("monzo".to_string()));
        assert_eq!(result.files[0].content_type, ContentType::Transactions);
        assert!(result.relationships.is_empty());
    }

    #[test]
    fn test_build_multi_file_classification_message() {
        let docs = vec![
            DocumentInput {
                filename: "monzo.csv".to_string(),
                format: FileFormat::Csv,
                text_content: "Date,Description,Amount\n2026-01-01,Lidl,-23.45".to_string(),
                original_size: 50,
            },
            DocumentInput {
                filename: "t212.csv".to_string(),
                format: FileFormat::Csv,
                text_content: "Symbol,Qty,Value\nVUSA,50,3816".to_string(),
                original_size: 35,
            },
        ];

        let msg = build_multi_file_classification_message(&docs);
        assert!(msg.contains("2 file(s)"));
        assert!(msg.contains("--- FILE 1 ---"));
        assert!(msg.contains("filename: monzo.csv"));
        assert!(msg.contains("--- FILE 2 ---"));
        assert!(msg.contains("filename: t212.csv"));
    }

    #[test]
    fn test_convert_parsed_holdings_skips_low_confidence() {
        let parsed = ParsedHoldings {
            detection_confidence: 0.95,
            rows: vec![
                super::super::holdings_parser::ParsedHoldingRow {
                    symbol: "VUSA".to_string(),
                    name: "Vanguard S&P 500".to_string(),
                    holding_type: "etf".to_string(),
                    quantity: "50".to_string(),
                    price_per_unit: Some("76.32".to_string()),
                    value: "3816.00".to_string(),
                    currency: "GBP".to_string(),
                    sub_account: None,
                    row_confidence: 0.95,
                },
                super::super::holdings_parser::ParsedHoldingRow {
                    symbol: "BAD".to_string(),
                    name: "Low confidence".to_string(),
                    holding_type: "stock".to_string(),
                    quantity: "1".to_string(),
                    price_per_unit: None,
                    value: "1.00".to_string(),
                    currency: "GBP".to_string(),
                    sub_account: None,
                    row_confidence: 0.50,
                },
            ],
        };

        let result = convert_parsed_holdings(&parsed).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].symbol, "VUSA");
    }
}
