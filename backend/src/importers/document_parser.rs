//! Document parser pipeline for the 2-stage import redesign.
//! Orchestrates: preprocess -> classify -> extract -> deduplicate.
//!
//! Phase 2: Multi-file CSV support with cross-document relationship detection.

use std::sync::Arc;

use anyhow::{Result, anyhow};
use chrono::Local;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;
use ts_rs::TS;

use crate::importers::llm_parser::StatementParser;
use crate::importers::pdf_parser::{
    PdfHoldingsParser, PdfInvestmentsParser, PdfPeriodicHoldingsParser, PdfStatementParser,
};
use crate::importers::provider::LlmProvider;
use crate::importers::unified::UnifiedStatementRow;
use crate::model::{
    BankFormat, CategorySource, CreateInvestmentEventBody, Holding, HoldingType,
    HoldingsImportPayload, HoldingsIngestionResult, ImportPayload, ImportTransaction,
    IngestionMetadata, IngestionPreview, IngestionStatus, InvestmentIngestionResult,
    InvestmentsImportPayload, TransactionIngestionResult,
    TransactionPreviewStatus,
};
use crate::storage::Db;

use super::holdings_parser::{HoldingsExtractor, LlmHoldingsParser, ParsedHoldings};
use super::investments_parser::{LlmInvestmentsParser, ParsedInvestmentRow};
use super::periodic_holdings_parser::LlmPeriodicHoldingsParser;

// ── Public types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileFormat {
    Csv,
    Pdf,
    Excel,
}

#[derive(Debug, Clone)]
pub struct DocumentInput {
    pub filename: String,
    pub format: FileFormat,
    pub text_content: String,
    pub raw_bytes: Vec<u8>,
    pub original_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    Transactions,
    Holdings,
    Investments,
    Unknown,
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

    pub fn to_content_types(&self) -> Vec<ContentType> {
        let mut types = Vec::new();
        if self.transactions {
            types.push(ContentType::Transactions);
        }
        if self.holdings.enabled {
            types.push(ContentType::Holdings);
        }
        if self.investments {
            types.push(ContentType::Investments);
        }
        if types.is_empty() {
            types.push(ContentType::Unknown);
        }
        types
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
    pub investments: Vec<ParsedInvestmentRow>,
    pub detected_bank: BankFormat,
    pub detection_confidence: f32,
}

#[derive(Debug)]
pub enum PipelineOutcome {
    Success { extraction: ExtractionResult },
    NeedsClarification(Box<IngestionPreview>),
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Run the multi-file LLM pipeline: derive content type from hints, extract each, merge.
/// Does NOT touch the database. Classification is skipped since return_type tells us
/// what to extract and institution is derived from the account DB lookup.
pub async fn run_multi_file_pipeline(
    documents: &[DocumentInput],
    hints: &ParseHints,
    _account_institution: &str,
    provider: Arc<dyn LlmProvider>,
) -> Result<PipelineOutcome> {
    let content_types = hints.return_type.to_content_types();
    let extraction = extract_all_parallel(documents, &content_types, hints, provider).await?;
    Ok(PipelineOutcome::Success { extraction })
}

/// Run deduplication checks and assemble the final IngestionPreview.
/// Requires DB access. Called synchronously after the LLM pipeline completes.
pub fn build_multi_preview(
    extraction: ExtractionResult,
    account_id: &str,
    account_institution: &str,
    num_files: usize,
    db: &Db,
    processing_time_ms: u64,
) -> Result<IngestionPreview> {
    let min_row_confidence: f32 = std::env::var("FYNANCE_IMPORT_MIN_ROW_CONF")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.70);

    let institution_detected = Some(account_institution.to_string());
    let detection_confidence = extraction.detection_confidence;
    let notes: Vec<String> = vec![];

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

    // ── Investment deduplication ─────────────────────────────────────────────

    let investments_result = if !extraction.investments.is_empty() {
        let preview_rows =
            db.dry_run_investments(account_id, &extraction.investments, min_row_confidence)?;

        let rows_new = preview_rows
            .iter()
            .filter(|r| r.status == TransactionPreviewStatus::New)
            .count();
        let rows_dup = preview_rows
            .iter()
            .filter(|r| r.status == TransactionPreviewStatus::Duplicate)
            .count();

        let payload_events: Vec<CreateInvestmentEventBody> = extraction
            .investments
            .iter()
            .filter(|r| r.row_confidence >= min_row_confidence)
            .map(|r| CreateInvestmentEventBody {
                account_id: account_id.to_string(),
                event_type: r.event_type.clone(),
                symbol: r.symbol.clone(),
                date: r.date.clone(),
                quantity: r.quantity.clone(),
                price_per_share: r.price_per_share.clone(),
                fee: Some(r.fee.clone()),
                currency: r.currency.clone(),
                notes: r.notes.clone(),
            })
            .collect();

        InvestmentIngestionResult {
            count: preview_rows.len(),
            new: rows_new,
            duplicate: rows_dup,
            rows: preview_rows,
            payload: Some(InvestmentsImportPayload {
                account_id: account_id.to_string(),
                events: payload_events,
            }),
        }
    } else {
        InvestmentIngestionResult {
            count: 0,
            new: 0,
            duplicate: 0,
            rows: vec![],
            payload: None,
        }
    };

    // ── Assemble ────────────────────────────────────────────────────────────

    Ok(IngestionPreview {
        status: IngestionStatus::Success,
        metadata: IngestionMetadata {
            files_processed: num_files,
            institution_detected,
            detection_confidence,
            processing_time_ms,
            notes,
            relationships_found: vec![],
        },
        transactions: tx_result,
        holdings: holdings_result,
        investments: investments_result,
        clarifications_needed: vec![],
    })
}

// ── Parallel extraction ─────────────────────────────────────────────────────

async fn extract_all_parallel(
    documents: &[DocumentInput],
    content_types: &[ContentType],
    hints: &ParseHints,
    provider: Arc<dyn LlmProvider>,
) -> Result<ExtractionResult> {
    let mut join_set: JoinSet<Result<ExtractionResult>> = JoinSet::new();

    let user_hint = hints.hint.clone();
    let period = hints.return_type.holdings.period.clone();

    for doc in documents {
        for ct in content_types {
            if *ct == ContentType::Unknown {
                continue;
            }

            let doc_clone = doc.clone();
            let ct_clone = ct.clone();
            let hint_clone = user_hint.clone();
            let period_clone = period.clone();
            let provider_clone = provider.clone();

            join_set.spawn(async move {
                extract_single_file(
                    &doc_clone,
                    &ct_clone,
                    hint_clone.as_deref(),
                    period_clone.as_ref(),
                    provider_clone,
                )
                .await
            });
        }
    }

    let mut merged = ExtractionResult {
        transactions: vec![],
        holdings: vec![],
        investments: vec![],
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
        merged.investments.extend(extraction.investments);

        if extraction.detection_confidence > max_confidence {
            max_confidence = extraction.detection_confidence;
            merged.detected_bank = extraction.detected_bank;
        }
    }

    merged.detection_confidence = max_confidence;
    Ok(merged)
}

async fn extract_single_file(
    doc: &DocumentInput,
    content_type: &ContentType,
    user_hint: Option<&str>,
    period: Option<&SnapshotPeriod>,
    provider: Arc<dyn LlmProvider>,
) -> Result<ExtractionResult> {
    match content_type {
        ContentType::Transactions => match doc.format {
            FileFormat::Pdf => {
                let parser = PdfStatementParser::new(provider);
                let parsed = parser
                    .parse(&doc.raw_bytes, &doc.filename, user_hint)
                    .await?;
                Ok(ExtractionResult {
                    transactions: parsed.rows,
                    holdings: vec![],
                    investments: vec![],
                    detected_bank: parsed.detected_bank,
                    detection_confidence: parsed.detection_confidence,
                })
            }
            FileFormat::Csv | FileFormat::Excel => {
                let parser = crate::importers::llm_parser::LlmStatementParser::new(provider);
                let parsed = parser
                    .parse(&doc.text_content, &doc.filename, user_hint)
                    .await?;
                Ok(ExtractionResult {
                    transactions: parsed.rows,
                    holdings: vec![],
                    investments: vec![],
                    detected_bank: parsed.detected_bank,
                    detection_confidence: parsed.detection_confidence,
                })
            }
        },
        ContentType::Holdings => match doc.format {
            FileFormat::Pdf => match period {
                Some(p) => {
                    let parser = PdfPeriodicHoldingsParser::new(provider);
                    let parsed = parser
                        .extract(&doc.raw_bytes, &doc.filename, p, user_hint)
                        .await?;
                    let holdings = convert_parsed_holdings(&parsed)?;
                    Ok(ExtractionResult {
                        transactions: vec![],
                        holdings,
                        investments: vec![],
                        detected_bank: BankFormat::Unknown,
                        detection_confidence: parsed.detection_confidence,
                    })
                }
                None => {
                    let parser = PdfHoldingsParser::new(provider);
                    let parsed = parser
                        .extract(&doc.raw_bytes, &doc.filename, user_hint)
                        .await?;
                    let holdings = convert_parsed_holdings(&parsed)?;
                    Ok(ExtractionResult {
                        transactions: vec![],
                        holdings,
                        investments: vec![],
                        detected_bank: BankFormat::Unknown,
                        detection_confidence: parsed.detection_confidence,
                    })
                }
            },
            FileFormat::Csv | FileFormat::Excel => match period {
                Some(p) => {
                    let parser = LlmPeriodicHoldingsParser::new(provider);
                    let parsed = parser
                        .extract_periodic_holdings(&doc.text_content, &doc.filename, p, user_hint)
                        .await?;
                    let holdings = convert_parsed_holdings(&parsed)?;
                    Ok(ExtractionResult {
                        transactions: vec![],
                        holdings,
                        investments: vec![],
                        detected_bank: BankFormat::Unknown,
                        detection_confidence: parsed.detection_confidence,
                    })
                }
                None => {
                    let parser = LlmHoldingsParser::new(provider);
                    let parsed = parser
                        .extract_holdings(&doc.text_content, &doc.filename, user_hint)
                        .await?;
                    let holdings = convert_parsed_holdings(&parsed)?;
                    Ok(ExtractionResult {
                        transactions: vec![],
                        holdings,
                        investments: vec![],
                        detected_bank: BankFormat::Unknown,
                        detection_confidence: parsed.detection_confidence,
                    })
                }
            },
        },
        ContentType::Investments => match doc.format {
            FileFormat::Pdf => {
                let parser = PdfInvestmentsParser::new(provider);
                let parsed = parser.extract(&doc.raw_bytes, &doc.filename, user_hint).await?;
                Ok(ExtractionResult {
                    transactions: vec![],
                    holdings: vec![],
                    investments: parsed.rows,
                    detected_bank: BankFormat::Unknown,
                    detection_confidence: parsed.detection_confidence,
                })
            }
            FileFormat::Csv | FileFormat::Excel => {
                let parser = LlmInvestmentsParser::new(provider);
                let parsed = parser
                    .extract_investments(&doc.text_content, &doc.filename, user_hint)
                    .await?;
                Ok(ExtractionResult {
                    transactions: vec![],
                    holdings: vec![],
                    investments: parsed.rows,
                    detected_bank: BankFormat::Unknown,
                    detection_confidence: parsed.detection_confidence,
                })
            }
        },
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
        let price_per_unit: Option<Decimal> = row
            .price_per_unit
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse::<Decimal>()
                    .map_err(|_| anyhow!("invalid price_per_unit '{}' for {}", s, row.symbol))
            })
            .transpose()?;
        let value: Decimal = match (&row.value, &price_per_unit) {
            (Some(v), _) => v
                .parse::<Decimal>()
                .map_err(|_| anyhow!("invalid value '{}' for {}", v, row.symbol))?,
            (None, Some(ppu)) => quantity * ppu,
            (None, None) => {
                tracing::warn!(symbol = %row.symbol, "skipping holding: no value and no price_per_unit");
                continue;
            }
        };

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
    fn test_extract_result_has_investments_field() {
        let result = ExtractionResult {
            transactions: vec![],
            holdings: vec![],
            investments: vec![],
            detected_bank: BankFormat::Unknown,
            detection_confidence: 0.0,
        };
        assert!(result.investments.is_empty());
    }

    #[test]
    fn test_to_content_types_includes_investments() {
        let rt = ReturnType {
            transactions: false,
            holdings: HoldingsReturnConfig::default(),
            investments: true,
        };
        assert_eq!(rt.to_content_types(), vec![ContentType::Investments]);
    }

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
    fn test_return_type_to_content_types() {
        let rt = ReturnType {
            transactions: true,
            holdings: HoldingsReturnConfig::default(),
            investments: false,
        };
        assert_eq!(rt.to_content_types(), vec![ContentType::Transactions]);

        let rt = ReturnType {
            transactions: false,
            holdings: HoldingsReturnConfig {
                enabled: true,
                period: None,
            },
            investments: false,
        };
        assert_eq!(rt.to_content_types(), vec![ContentType::Holdings]);

        let rt = ReturnType {
            transactions: true,
            holdings: HoldingsReturnConfig {
                enabled: true,
                period: None,
            },
            investments: false,
        };
        assert_eq!(
            rt.to_content_types(),
            vec![ContentType::Transactions, ContentType::Holdings]
        );

        let rt = ReturnType {
            transactions: false,
            holdings: HoldingsReturnConfig::default(),
            investments: true,
        };
        assert_eq!(rt.to_content_types(), vec![ContentType::Investments]);

        let rt = ReturnType {
            transactions: true,
            holdings: HoldingsReturnConfig {
                enabled: true,
                period: None,
            },
            investments: true,
        };
        assert_eq!(
            rt.to_content_types(),
            vec![
                ContentType::Transactions,
                ContentType::Holdings,
                ContentType::Investments
            ]
        );

        let rt = ReturnType {
            transactions: false,
            holdings: HoldingsReturnConfig::default(),
            investments: false,
        };
        assert_eq!(rt.to_content_types(), vec![ContentType::Unknown]);
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
                    value: Some("3816.00".to_string()),
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
                    value: Some("1.00".to_string()),
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

    #[test]
    fn test_convert_parsed_holdings_value_computed_from_price() {
        let parsed = ParsedHoldings {
            detection_confidence: 0.95,
            rows: vec![super::super::holdings_parser::ParsedHoldingRow {
                symbol: "AAPL".to_string(),
                name: "Apple Inc".to_string(),
                holding_type: "stock".to_string(),
                quantity: "10".to_string(),
                price_per_unit: Some("150.00".to_string()),
                value: None,
                currency: "USD".to_string(),
                sub_account: None,
                as_of: None,
                row_confidence: 0.90,
            }],
        };

        let result = convert_parsed_holdings(&parsed).unwrap();
        assert_eq!(result.len(), 1);
        let expected: Decimal = "1500.00".parse().unwrap();
        assert_eq!(result[0].value, expected);
    }

    #[test]
    fn test_convert_parsed_holdings_skips_when_no_value_and_no_price() {
        let parsed = ParsedHoldings {
            detection_confidence: 0.95,
            rows: vec![super::super::holdings_parser::ParsedHoldingRow {
                symbol: "MYSTERY".to_string(),
                name: "Unknown Asset".to_string(),
                holding_type: "stock".to_string(),
                quantity: "5".to_string(),
                price_per_unit: None,
                value: None,
                currency: "GBP".to_string(),
                sub_account: None,
                as_of: None,
                row_confidence: 0.85,
            }],
        };

        let result = convert_parsed_holdings(&parsed).unwrap();
        assert_eq!(
            result.len(),
            0,
            "row with no value and no price should be skipped"
        );
    }
}
