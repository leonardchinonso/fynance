//! Document parser pipeline for the 2-stage import redesign.
//! Orchestrates: preprocess -> classify -> extract -> deduplicate.
//!
//! Phase 2: Multi-file CSV support with cross-document relationship detection.

use anyhow::{Result, anyhow};
use chrono::Local;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;
use ts_rs::TS;

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
use super::periodic_holdings_parser::LlmPeriodicHoldingsParser;

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

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct ParseHints {
    pub return_type: ReturnType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct ReturnType {
    #[serde(default)]
    pub transactions: bool,
    #[serde(default)]
    pub holdings: HoldingsReturnConfig,
    #[serde(default)]
    pub investments: bool,
}

impl ReturnType {
    pub fn is_valid(&self) -> bool {
        self.transactions || self.holdings.enabled || self.investments
    }

    pub fn to_content_type(&self) -> ContentType {
        let has_tx = self.transactions || self.investments;
        let has_h = self.holdings.enabled;
        match (has_tx, has_h) {
            (true, true) => ContentType::Both,
            (true, false) => ContentType::Transactions,
            (false, true) => ContentType::Holdings,
            (false, false) => ContentType::Unknown,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct HoldingsReturnConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<SnapshotPeriod>,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
#[serde(rename_all = "snake_case")]
pub enum SnapshotPeriod {
    Monthly,
    Quarterly,
    Yearly,
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

// ── Public API ──────────────────────────────────────────────────────────────

/// Run the multi-file LLM pipeline: derive content type from hints, extract each, merge.
/// Does NOT touch the database. Classification is skipped since return_type tells us
/// what to extract and institution is derived from the account DB lookup.
pub async fn run_multi_file_pipeline(
    documents: &[DocumentInput],
    hints: &ParseHints,
    account_institution: &str,
) -> Result<PipelineOutcome> {
    let content_type = hints.return_type.to_content_type();
    let multi_classification = MultiClassificationResult {
        files: documents
            .iter()
            .map(|doc| FileClassification {
                filename: doc.filename.clone(),
                institution: Some(account_institution.to_string()),
                institution_confidence: 1.0,
                content_type: content_type.clone(),
            })
            .collect(),
        relationships: vec![],
    };

    let extraction = extract_all_parallel(documents, &multi_classification, hints).await?;

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
    account_institution: &str,
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

    // ── Institution mismatch warning ────────────────────────────────────────
    let mut notes = vec![];
    if let Some(ref detected) = institution_detected {
        let detected_lower = detected.to_lowercase();
        let account_lower = account_institution.to_lowercase();
        if detected_lower != account_lower && detected_lower != "unknown" {
            notes.push(format!(
                "Detected institution ({}) does not match account institution ({}). \
                 Verify you selected the correct account.",
                detected, account_institution
            ));
        }
    }

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
            notes,
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

// ── Parallel extraction ─────────────────────────────────────────────────────

async fn extract_all_parallel(
    documents: &[DocumentInput],
    classification: &MultiClassificationResult,
    hints: &ParseHints,
) -> Result<ExtractionResult> {
    let mut join_set: JoinSet<Result<ExtractionResult>> = JoinSet::new();

    let user_hint = hints.hint.clone();
    let period = hints.return_type.holdings.period.clone();

    for (doc, file_class) in documents.iter().zip(classification.files.iter()) {
        if file_class.content_type == ContentType::Unknown {
            continue;
        }

        let text = doc.text_content.clone();
        let filename = doc.filename.clone();
        let content_type = file_class.content_type.clone();
        let hint_clone = user_hint.clone();
        let period_clone = period.clone();

        join_set.spawn(async move {
            extract_single_file(
                &text,
                &filename,
                &content_type,
                hint_clone.as_deref(),
                period_clone.as_ref(),
            )
            .await
        });
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
    user_hint: Option<&str>,
    period: Option<&SnapshotPeriod>,
) -> Result<ExtractionResult> {
    match content_type {
        ContentType::Transactions => {
            let parser = crate::importers::llm_parser::LlmStatementParser::from_env()?;
            let parsed = parser.parse(text, filename, user_hint).await?;
            Ok(ExtractionResult {
                transactions: parsed.rows,
                holdings: vec![],
                detected_bank: parsed.detected_bank,
                detection_confidence: parsed.detection_confidence,
            })
        }
        ContentType::Holdings => match period {
            Some(p) => {
                let parser = LlmPeriodicHoldingsParser::from_env()?;
                let parsed = parser
                    .extract_periodic_holdings(text, filename, p, user_hint)
                    .await?;
                let holdings = convert_parsed_holdings(&parsed)?;
                Ok(ExtractionResult {
                    transactions: vec![],
                    holdings,
                    detected_bank: BankFormat::Unknown,
                    detection_confidence: parsed.detection_confidence,
                })
            }
            None => {
                let parser = LlmHoldingsParser::from_env()?;
                let parsed = parser.extract_holdings(text, filename, user_hint).await?;
                let holdings = convert_parsed_holdings(&parsed)?;
                Ok(ExtractionResult {
                    transactions: vec![],
                    holdings,
                    detected_bank: BankFormat::Unknown,
                    detection_confidence: parsed.detection_confidence,
                })
            }
        },
        ContentType::Both => {
            let tx_parser = crate::importers::llm_parser::LlmStatementParser::from_env()?;

            match period {
                Some(p) => {
                    let h_parser = LlmPeriodicHoldingsParser::from_env()?;
                    let (tx_result, h_result) = tokio::join!(
                        tx_parser.parse(text, filename, user_hint),
                        h_parser.extract_periodic_holdings(text, filename, p, user_hint)
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
                None => {
                    let h_parser = LlmHoldingsParser::from_env()?;
                    let (tx_result, h_result) = tokio::join!(
                        tx_parser.parse(text, filename, user_hint),
                        h_parser.extract_holdings(text, filename, user_hint)
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
            }
        }
        ContentType::Unknown => Err(anyhow!("cannot extract from Unknown content type")),
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

        let as_of = row
            .as_of
            .as_deref()
            .and_then(|s| {
                chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                    .ok()
                    .and_then(|d| d.and_hms_opt(23, 59, 59))
            })
            .unwrap_or(now);

        holdings.push(Holding {
            account_id: String::new(),
            symbol: row.symbol.clone(),
            name: row.name.clone(),
            holding_type,
            quantity,
            price_per_unit,
            value,
            currency: row.currency.clone(),
            as_of,
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
    fn test_return_type_is_valid() {
        let rt = ReturnType {
            transactions: false,
            holdings: HoldingsReturnConfig::default(),
            investments: false,
        };
        assert!(!rt.is_valid());

        let rt = ReturnType {
            transactions: true,
            holdings: HoldingsReturnConfig::default(),
            investments: false,
        };
        assert!(rt.is_valid());

        let rt = ReturnType {
            transactions: false,
            holdings: HoldingsReturnConfig {
                enabled: true,
                period: None,
            },
            investments: false,
        };
        assert!(rt.is_valid());

        let rt = ReturnType {
            transactions: false,
            holdings: HoldingsReturnConfig::default(),
            investments: true,
        };
        assert!(rt.is_valid());
    }

    #[test]
    fn test_return_type_to_content_type() {
        let rt = ReturnType {
            transactions: true,
            holdings: HoldingsReturnConfig::default(),
            investments: false,
        };
        assert_eq!(rt.to_content_type(), ContentType::Transactions);

        let rt = ReturnType {
            transactions: false,
            holdings: HoldingsReturnConfig {
                enabled: true,
                period: None,
            },
            investments: false,
        };
        assert_eq!(rt.to_content_type(), ContentType::Holdings);

        let rt = ReturnType {
            transactions: true,
            holdings: HoldingsReturnConfig {
                enabled: true,
                period: None,
            },
            investments: false,
        };
        assert_eq!(rt.to_content_type(), ContentType::Both);

        let rt = ReturnType {
            transactions: false,
            holdings: HoldingsReturnConfig::default(),
            investments: true,
        };
        assert_eq!(rt.to_content_type(), ContentType::Transactions);
    }

    #[test]
    fn test_parse_hints_deserialization() {
        let json = r#"{"return_type": {"transactions": true, "holdings": {"enabled": false}, "investments": false}, "hint": "ignore pending"}"#;
        let hints: ParseHints = serde_json::from_str(json).unwrap();
        assert!(hints.return_type.transactions);
        assert!(!hints.return_type.holdings.enabled);
        assert_eq!(hints.hint, Some("ignore pending".to_string()));
    }

    #[test]
    fn test_parse_hints_rejects_old_format() {
        let json = r#"{"institution": "monzo", "expected_data": ["transactions"]}"#;
        let result: Result<ParseHints, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_period_requires_transactions() {
        // period=Some with transactions=false is invalid per the route handler validation
        let hints = ParseHints {
            return_type: ReturnType {
                transactions: false,
                holdings: HoldingsReturnConfig {
                    enabled: true,
                    period: Some(SnapshotPeriod::Monthly),
                },
                investments: false,
            },
            hint: None,
        };
        // is_valid() passes (holdings.enabled=true) but the route rejects period without transactions
        assert!(hints.return_type.is_valid());
        assert!(hints.return_type.holdings.period.is_some());
        assert!(!hints.return_type.transactions);
    }

    #[test]
    fn test_institution_mismatch_warning() {
        let classification = MultiClassificationResult {
            files: vec![FileClassification {
                filename: "test.csv".to_string(),
                institution: Some("Monzo".to_string()),
                institution_confidence: 1.0,
                content_type: ContentType::Transactions,
            }],
            relationships: vec![],
        };
        let extraction = ExtractionResult {
            transactions: vec![],
            holdings: vec![],
            detected_bank: crate::model::BankFormat::Monzo,
            detection_confidence: 0.95,
        };
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = crate::storage::Db::open(tmp.path()).unwrap();
        let preview =
            build_multi_preview(&classification, extraction, "acc1", "Revolut", &db, 10).unwrap();
        assert!(
            preview
                .metadata
                .notes
                .iter()
                .any(|n| n.contains("does not match")),
            "expected mismatch note, got: {:?}",
            preview.metadata.notes
        );
    }

    #[test]
    fn test_no_mismatch_when_unknown() {
        let classification = MultiClassificationResult {
            files: vec![FileClassification {
                filename: "test.csv".to_string(),
                institution: Some("unknown".to_string()),
                institution_confidence: 0.5,
                content_type: ContentType::Transactions,
            }],
            relationships: vec![],
        };
        let extraction = ExtractionResult {
            transactions: vec![],
            holdings: vec![],
            detected_bank: crate::model::BankFormat::Unknown,
            detection_confidence: 0.5,
        };
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = crate::storage::Db::open(tmp.path()).unwrap();
        let preview =
            build_multi_preview(&classification, extraction, "acc1", "Monzo", &db, 10).unwrap();
        assert!(
            preview.metadata.notes.is_empty(),
            "expected no notes for unknown institution, got: {:?}",
            preview.metadata.notes
        );
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
                    as_of: None,
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
                    as_of: None,
                    row_confidence: 0.50,
                },
            ],
        };

        let result = convert_parsed_holdings(&parsed).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].symbol, "VUSA");
    }
}
