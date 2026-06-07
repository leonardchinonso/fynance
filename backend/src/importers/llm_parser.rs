//! LLM-based statement parser.
//!
//! `StatementParser` is the async trait the CSV importer calls.
//! `LlmStatementParser` implements it by sending the raw CSV text to the
//! Anthropic messages API and using tool_use to force a structured JSON
//! response. `MockStatementParser` is a test-only implementation that
//! returns a pre-canned `ParsedStatement` without any network traffic.

use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::provider::{LlmProvider, ModelTier, ProviderCallResult};
use super::unified::UnifiedStatementRow;
use crate::model::{Agent, BankFormat};

// The system prompt is pinned in the repo so it can be reviewed in git and
// diffed like any other source file.
const SYSTEM_PROMPT: &str = include_str!("../../config/prompts/statement_parser.txt");

// Truncate CSV input at this byte limit before sending to the LLM.
// A yearly Monzo export is ~150 KB; this leaves headroom while keeping
// costs bounded. Chunking for very large files is tracked as an open
// question in docs/plans/10_llm_csv_import.md §11.
const MAX_CSV_BYTES: usize = 200_000;

// ── Result of a single parse call ────────────────────────────────────────────

/// The output produced by any `StatementParser` implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedStatement {
    pub detected_bank: BankFormat,
    /// LLM's confidence that it correctly identified the bank [0.0, 1.0].
    pub detection_confidence: f32,
    pub rows: Vec<UnifiedStatementRow>,
}

// ── Trait ─────────────────────────────────────────────────────────────────────

#[async_trait]
pub trait StatementParser: Send + Sync {
    async fn parse(
        &self,
        raw: &str,
        filename: &str,
        user_hint: Option<&str>,
        agent_override: Option<Agent>,
    ) -> Result<(ParsedStatement, ProviderCallResult)>;
}

// ── LLM implementation ────────────────────────────────────────────────────────

/// Parses a CSV bank statement by sending it to an LLM provider and using
/// tool_use to receive a `ParsedStatement`-shaped JSON object back.
pub struct LlmStatementParser {
    provider: Arc<dyn LlmProvider>,
    /// File-level confidence threshold. Import fails if detection_confidence
    /// falls below this.
    pub min_detection_confidence: f32,
    /// Row-level confidence threshold. Rows below this are skipped with a
    /// warning rather than failing the whole file.
    pub min_row_confidence: f32,
}

impl LlmStatementParser {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        let min_detection_confidence = std::env::var("FYNANCE_IMPORT_MIN_DETECT_CONF")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.80_f32);
        let min_row_confidence = std::env::var("FYNANCE_IMPORT_MIN_ROW_CONF")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.70_f32);

        Self {
            provider,
            min_detection_confidence,
            min_row_confidence,
        }
    }
}

#[async_trait]
impl StatementParser for LlmStatementParser {
    async fn parse(
        &self,
        raw: &str,
        filename: &str,
        user_hint: Option<&str>,
        agent_override: Option<Agent>,
    ) -> Result<(ParsedStatement, ProviderCallResult)> {
        let content = if raw.len() > MAX_CSV_BYTES {
            tracing::warn!(
                filename,
                bytes = raw.len(),
                max_bytes = MAX_CSV_BYTES,
                "CSV is large; truncating before sending to LLM"
            );
            &raw[..MAX_CSV_BYTES]
        } else {
            raw
        };

        let tool_schema = build_tool_schema();

        let mut user_msg = format!("filename: {filename}\n\n{content}");
        if let Some(hint) = user_hint {
            user_msg = format!("User instructions: {hint}\n\n{user_msg}");
        }

        tracing::debug!(
            provider = self.provider.name(),
            filename,
            bytes = content.len(),
            "parsing statement"
        );

        let call = self
            .provider
            .chat_with_tools(
                SYSTEM_PROMPT,
                &user_msg,
                "parse_bank_statement",
                tool_schema,
                ModelTier::Standard,
                agent_override,
            )
            .await?;

        let parsed: ParsedStatement = super::deserialize_tool_use(
            call.value.clone(),
            "bank statement parser",
            filename,
            "parse_bank_statement",
        )?;

        tracing::debug!(
            filename,
            detected_bank = ?parsed.detected_bank,
            detection_confidence = parsed.detection_confidence,
            row_count = parsed.rows.len(),
            "LLM parsed statement"
        );

        Ok((parsed, call))
    }
}

// ── Tool schema (hand-written JSON Schema) ────────────────────────────────────

pub(crate) fn build_tool_schema() -> Value {
    json!({
        "type": "object",
        "required": ["detected_bank", "detection_confidence", "rows"],
        "properties": {
            "detected_bank": {
                "type": "string",
                "enum": ["monzo", "revolut", "lloyds", "unknown"],
                "description": "The bank that issued this statement, or 'unknown' if not recognised."
            },
            "detection_confidence": {
                "type": "number",
                "minimum": 0.0,
                "maximum": 1.0,
                "description": "Confidence [0.0–1.0] that this file is a valid bank statement and that the bank was correctly identified."
            },
            "rows": {
                "type": "array",
                "description": "One element per transaction row. Skip header, metadata, and summary/total lines.",
                "items": {
                    "type": "object",
                    "required": ["date", "description", "amount", "currency", "row_confidence"],
                    "properties": {
                        "date": {
                            "type": "string",
                            "description": "Transaction date in ISO 8601 format: YYYY-MM-DD."
                        },
                        "description": {
                            "type": "string",
                            "description": "Primary transaction description or merchant/payee name."
                        },
                        "amount": {
                            "type": "string",
                            "description": "Signed decimal string. Negative = money out, positive = money in. No currency symbols or commas. Example: \"-5.50\" or \"2500.00\"."
                        },
                        "currency": {
                            "type": "string",
                            "description": "ISO 4217 currency code (e.g. \"GBP\"). Default to \"GBP\" if not present in the file."
                        },
                        "fitid": {
                            "type": ["string", "null"],
                            "description": "Unique transaction ID from the bank, if present."
                        },
                        "category": {
                            "type": ["string", "null"],
                            "description": "Spending category if the bank provides one."
                        },
                        "merchant": {
                            "type": ["string", "null"],
                            "description": "Merchant name when available as a separate column from description."
                        },
                        "counterparty": {
                            "type": ["string", "null"],
                            "description": "Counterparty name for peer-to-peer transfers."
                        },
                        "transaction_type": {
                            "type": ["string", "null"],
                            "description": "Transaction type as labelled by the bank (e.g. CARD_PAYMENT)."
                        },
                        "balance_after": {
                            "type": ["string", "null"],
                            "description": "Running balance after this transaction as a decimal string, if the bank includes it."
                        },
                        "notes": {
                            "type": ["string", "null"],
                            "description": "Notes or tags on the transaction."
                        },
                        "reference": {
                            "type": ["string", "null"],
                            "description": "Payment reference, if available."
                        },
                        "row_confidence": {
                            "type": "number",
                            "minimum": 0.0,
                            "maximum": 1.0,
                            "description": "Confidence [0.0–1.0] that this row was correctly parsed as a transaction."
                        }
                    }
                }
            }
        }
    })
}

// ── Mock implementation for tests ─────────────────────────────────────────────

/// A `StatementParser` that returns a pre-canned `ParsedStatement` without
/// any network calls. Used in unit and integration tests so that the full
/// CSV import pipeline can be exercised without an API key.
pub struct MockStatementParser {
    pub result: ParsedStatement,
}

impl MockStatementParser {
    /// Load a fixture from JSON. The fixture format matches `ParsedStatement`'s
    /// serde shape exactly.
    pub fn from_json(json: &str) -> Result<Self> {
        let result: ParsedStatement =
            serde_json::from_str(json).context("parsing MockStatementParser fixture JSON")?;
        Ok(Self { result })
    }
}

#[async_trait]
impl StatementParser for MockStatementParser {
    async fn parse(
        &self,
        _raw: &str,
        _filename: &str,
        _user_hint: Option<&str>,
        _agent_override: Option<Agent>,
    ) -> Result<(ParsedStatement, ProviderCallResult)> {
        Ok((
            self.result.clone(),
            ProviderCallResult {
                value: serde_json::Value::Null,
                usage: super::provider::TokenUsage::default(),
                model: "mock".to_string(),
                duration_ms: 0,
                stop_reason: None,
            },
        ))
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod provider_tests {
    use std::sync::Arc;

    use serde_json::json;

    use crate::importers::provider::testing::MockProvider;
    use crate::model::BankFormat;

    use super::{LlmStatementParser, StatementParser};

    #[tokio::test]
    async fn test_parser_uses_provider_result() {
        let mock_input = json!({
            "detected_bank": "monzo",
            "detection_confidence": 0.97,
            "rows": [{
                "date": "2026-05-01",
                "description": "Lidl",
                "amount": "-5.50",
                "currency": "GBP",
                "row_confidence": 0.99
            }]
        });

        let provider = MockProvider::new(mock_input);
        let parser = LlmStatementParser::new(provider as Arc<_>);
        let (result, _call) = parser
            .parse("some csv content", "test.csv", None, None)
            .await
            .unwrap();

        assert_eq!(result.detected_bank, BankFormat::Monzo);
        assert_eq!(result.detection_confidence, 0.97);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].description, "Lidl");
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use rust_decimal::Decimal;

    use super::*;

    fn make_row(
        date: &str,
        description: &str,
        amount: &str,
        confidence: f32,
    ) -> UnifiedStatementRow {
        UnifiedStatementRow {
            date: NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            description: description.to_string(),
            amount: amount.parse::<Decimal>().unwrap(),
            currency: "GBP".to_string(),
            fitid: None,
            category: None,
            merchant: None,
            counterparty: None,
            transaction_type: None,
            balance_after: None,
            notes: None,
            reference: None,
            row_confidence: confidence,
            category_id: None,
            category_confidence: None,
        }
    }

    #[test]
    fn mock_parser_round_trips_parsed_statement() {
        let json = r#"{
            "detected_bank": "monzo",
            "detection_confidence": 0.97,
            "rows": [
                {
                    "date": "2026-03-10",
                    "description": "Lidl",
                    "amount": "-5.50",
                    "currency": "GBP",
                    "fitid": null,
                    "category": "Groceries",
                    "merchant": null,
                    "counterparty": null,
                    "transaction_type": null,
                    "balance_after": null,
                    "notes": null,
                    "reference": null,
                    "row_confidence": 0.99
                }
            ]
        }"#;
        let mock = MockStatementParser::from_json(json).unwrap();
        assert_eq!(mock.result.detected_bank, BankFormat::Monzo);
        assert_eq!(mock.result.rows.len(), 1);
        assert_eq!(mock.result.rows[0].description, "Lidl");
    }

    #[tokio::test]
    async fn mock_parser_ignores_input() {
        let stmt = ParsedStatement {
            detected_bank: BankFormat::Unknown,
            detection_confidence: 0.85,
            rows: vec![make_row("2026-03-10", "Test", "-1.00", 0.9)],
        };
        let mock = MockStatementParser { result: stmt };
        let (parsed, _call) = mock.parse("anything", "test.csv", None, None).await.unwrap();
        assert_eq!(parsed.detected_bank, BankFormat::Unknown);
        assert_eq!(parsed.rows.len(), 1);
    }
}
