//! Document parser pipeline for the 2-stage import redesign.
//! Orchestrates: preprocess -> classify -> extract -> deduplicate.
//!
//! Phase 1: Single CSV file, Anthropic only, no investments.

use anyhow::{Result, anyhow};
use chrono::Local;
use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::importers::llm_parser::StatementParser;
use crate::importers::unified::UnifiedStatementRow;
use crate::model::{
    BankFormat, CategorySource, Holding, HoldingType, HoldingsImportPayload,
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

// ── Constants ───────────────────────────────────────────────────────────────

const CLASSIFIER_PROMPT: &str = include_str!("../../config/prompts/document_classifier.txt");
const CLASSIFICATION_SAMPLE_BYTES: usize = 10_000;

// ── Public API ──────────────────────────────────────────────────────────────

/// Run the LLM portion of the pipeline (classify + extract).
/// Does NOT touch the database.
pub async fn run_llm_pipeline(
    document: &DocumentInput,
    hints: &ParseHints,
) -> Result<(ClassificationResult, ExtractionResult)> {
    let classification = classify_document(document, hints).await?;

    if classification.content_type == ContentType::Unknown {
        return Err(anyhow!(
            "Could not determine document content type (confidence: {:.2}). \
             Provide hints with expected_data to help.",
            classification.institution_confidence
        ));
    }

    let extraction = match classification.content_type {
        ContentType::Transactions => {
            let parser = crate::importers::llm_parser::LlmStatementParser::from_env()?;
            let parsed = parser.parse(&document.text_content, &document.filename).await?;
            ExtractionResult {
                transactions: parsed.rows,
                holdings: vec![],
                detected_bank: parsed.detected_bank,
                detection_confidence: parsed.detection_confidence,
            }
        }
        ContentType::Holdings => {
            let parser = LlmHoldingsParser::from_env()?;
            let parsed = parser
                .extract_holdings(&document.text_content, &document.filename)
                .await?;
            let holdings = convert_parsed_holdings(&parsed)?;
            ExtractionResult {
                transactions: vec![],
                holdings,
                detected_bank: BankFormat::Unknown,
                detection_confidence: parsed.detection_confidence,
            }
        }
        ContentType::Both => {
            let tx_parser = crate::importers::llm_parser::LlmStatementParser::from_env()?;
            let h_parser = LlmHoldingsParser::from_env()?;

            let tx_fut = tx_parser.parse(&document.text_content, &document.filename);
            let h_fut = h_parser.extract_holdings(&document.text_content, &document.filename);
            let (tx_result, h_result): (Result<_>, Result<_>) =
                tokio::join!(tx_fut, h_fut);

            let tx_parsed = tx_result?;
            let h_parsed = h_result?;
            let holdings = convert_parsed_holdings(&h_parsed)?;

            ExtractionResult {
                transactions: tx_parsed.rows,
                holdings,
                detected_bank: tx_parsed.detected_bank,
                detection_confidence: tx_parsed.detection_confidence,
            }
        }
        ContentType::Unknown => unreachable!(),
    };

    Ok((classification, extraction))
}

/// Run deduplication checks and assemble the final IngestionPreview.
/// Requires DB access. Called synchronously after the LLM pipeline completes.
pub fn build_preview(
    classification: &ClassificationResult,
    extraction: ExtractionResult,
    account_id: &str,
    db: &Db,
    processing_time_ms: u64,
) -> Result<IngestionPreview> {
    let min_row_confidence: f32 = std::env::var("FYNANCE_IMPORT_MIN_ROW_CONF")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.70);

    // ── Transaction deduplication ────────────────────────────────────────────

    let tx_result = if !extraction.transactions.is_empty() {
        let preview_rows =
            db.dry_run_transactions_from_parsed(account_id, &extraction.transactions, min_row_confidence)?;

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
            files_processed: 1,
            institution_detected: classification.institution.clone(),
            detection_confidence: classification.institution_confidence,
            processing_time_ms,
            notes: vec![],
            relationships_found: vec![],
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

async fn classify_document(
    document: &DocumentInput,
    hints: &ParseHints,
) -> Result<ClassificationResult> {
    // If hints fully specify routing, skip the LLM call
    if let (Some(institution), Some(expected_data)) = (&hints.institution, &hints.expected_data) {
        if !expected_data.is_empty() {
            let content_type = if expected_data.contains(&"holdings".to_string())
                && expected_data.contains(&"transactions".to_string())
            {
                ContentType::Both
            } else if expected_data.contains(&"holdings".to_string()) {
                ContentType::Holdings
            } else if expected_data.contains(&"transactions".to_string()) {
                ContentType::Transactions
            } else {
                ContentType::Unknown
            };
            return Ok(ClassificationResult {
                institution: Some(institution.clone()),
                institution_confidence: 1.0,
                content_type,
            });
        }
    }

    let sample =
        &document.text_content[..document.text_content.len().min(CLASSIFICATION_SAMPLE_BYTES)];

    let api_key = std::env::var("FYNANCE_ANTHROPIC_API_KEY").map_err(|_| {
        anyhow!("FYNANCE_ANTHROPIC_API_KEY is not set. Required for document parsing.")
    })?;
    let model = std::env::var("FYNANCE_PARSE_CLASSIFY_MODEL")
        .unwrap_or_else(|_| "claude-haiku-4-5-20251001".to_string());

    let tool_schema = build_classification_tool_schema();
    let user_message = format!("filename: {}\n\n{}", document.filename, sample);

    let request_body = json!({
        "model": model,
        "max_tokens": 1024,
        "system": CLASSIFIER_PROMPT,
        "tools": [{
            "name": "classify_document",
            "description": "Classify a financial document by institution and content type.",
            "input_schema": tool_schema
        }],
        "tool_choice": { "type": "tool", "name": "classify_document" },
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

    parse_classification_response(&body)
}

fn build_classification_tool_schema() -> Value {
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

fn parse_classification_response(body: &str) -> Result<ClassificationResult> {
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
        .map(|s| s.to_string());
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

    let institution = institution.filter(|s| s != "unknown");

    Ok(ClassificationResult {
        institution,
        institution_confidence,
        content_type,
    })
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
        assert_eq!(parse_holding_type_fuzzy("unknown_thing"), HoldingType::Stock);
    }

    #[test]
    fn test_hints_skip_classification() {
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
        let result = rt.block_on(classify_document(&doc, &hints)).unwrap();

        assert_eq!(result.institution, Some("monzo".to_string()));
        assert_eq!(result.institution_confidence, 1.0);
        assert_eq!(result.content_type, ContentType::Transactions);
    }

    #[test]
    fn test_hints_holdings() {
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
        let result = rt.block_on(classify_document(&doc, &hints)).unwrap();

        assert_eq!(result.institution, Some("trading_212".to_string()));
        assert_eq!(result.content_type, ContentType::Holdings);
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
