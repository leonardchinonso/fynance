//! LLM-based statement parser.
//!
//! `StatementParser` is the async trait the CSV importer calls.
//! `LlmStatementParser` implements it by sending the raw CSV text to the
//! Anthropic messages API and using tool_use to force a structured JSON
//! response. `MockStatementParser` is a test-only implementation that
//! returns a pre-canned `ParsedStatement` without any network traffic.

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::model::BankFormat;
use super::unified::UnifiedStatementRow;

// The system prompt is pinned in the repo so it can be reviewed in git and
// diffed like any other source file.
const SYSTEM_PROMPT: &str =
    include_str!("../../config/prompts/statement_parser.txt");

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
    async fn parse(&self, raw: &str, filename: &str) -> Result<ParsedStatement>;
}

// ── LLM implementation ────────────────────────────────────────────────────────

/// Parses a CSV bank statement by sending it to Anthropic and using tool_use
/// to receive a `ParsedStatement`-shaped JSON object back.
pub struct LlmStatementParser {
    client: Client,
    api_key: String,
    model: String,
    /// File-level confidence threshold. Import fails if detection_confidence
    /// falls below this.
    pub min_detection_confidence: f32,
    /// Row-level confidence threshold. Rows below this are skipped with a
    /// warning rather than failing the whole file.
    pub min_row_confidence: f32,
}

impl LlmStatementParser {
    /// Build from environment variables.
    ///
    /// Required: `FYNANCE_ANTHROPIC_API_KEY`
    /// Optional: `FYNANCE_IMPORT_LLM_MODEL`, `FYNANCE_IMPORT_MIN_DETECT_CONF`,
    ///           `FYNANCE_IMPORT_MIN_ROW_CONF`
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("FYNANCE_ANTHROPIC_API_KEY").map_err(|_| {
            anyhow!(
                "FYNANCE_ANTHROPIC_API_KEY is not set. \
                 LLM-based CSV import requires an Anthropic API key. \
                 Set it in your .env file or environment and try again."
            )
        })?;
        let model = std::env::var("FYNANCE_IMPORT_LLM_MODEL")
            .unwrap_or_else(|_| "claude-haiku-4-5-20251001".to_string());
        let min_detection_confidence = std::env::var("FYNANCE_IMPORT_MIN_DETECT_CONF")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.80_f32);
        let min_row_confidence = std::env::var("FYNANCE_IMPORT_MIN_ROW_CONF")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.70_f32);

        Ok(Self {
            client: Client::new(),
            api_key,
            model,
            min_detection_confidence,
            min_row_confidence,
        })
    }
}

#[async_trait]
impl StatementParser for LlmStatementParser {
    async fn parse(&self, raw: &str, filename: &str) -> Result<ParsedStatement> {
        // Truncate very large files. The open-question chunking path in
        // docs/plans/10_llm_csv_import.md §11 will handle >200 KB properly.
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

        let request_body = json!({
            "model": self.model,
            "max_tokens": 8192,
            "system": SYSTEM_PROMPT,
            "tools": [{
                "name": "parse_bank_statement",
                "description": "Parse a bank statement CSV into structured transaction records.",
                "input_schema": tool_schema
            }],
            "tool_choice": { "type": "tool", "name": "parse_bank_statement" },
            "messages": [{
                "role": "user",
                "content": format!("filename: {filename}\n\n{content}")
            }]
        });

        tracing::debug!(
            filename,
            bytes = content.len(),
            model = self.model,
            "sending CSV to Anthropic"
        );

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await
            .context("sending request to Anthropic API")?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("reading Anthropic API response body")?;

        if !status.is_success() {
            return Err(anyhow!("Anthropic API returned {status}: {body}"));
        }

        // Log at DEBUG only; never log full body at INFO to avoid leaking
        // transaction descriptions (see CLAUDE.md security conventions).
        tracing::debug!(
            filename,
            response_preview = &body[..body.len().min(300)],
            "received Anthropic response"
        );

        let api_resp: AnthropicResponse =
            serde_json::from_str(&body).with_context(|| {
                format!(
                    "parsing Anthropic response JSON (preview: {}...)",
                    &body[..body.len().min(200)]
                )
            })?;

        let tool_input = api_resp
            .content
            .into_iter()
            .find_map(|block| match block {
                ContentBlock::ToolUse { block_type, name, input }
                    if block_type == "tool_use" && name == "parse_bank_statement" =>
                {
                    Some(input)
                }
                _ => None,
            })
            .ok_or_else(|| {
                anyhow!("Anthropic response contained no parse_bank_statement tool_use block")
            })?;

        let parsed: ParsedStatement = serde_json::from_value(tool_input)
            .context("deserializing ParsedStatement from tool_use input")?;

        tracing::debug!(
            filename,
            detected_bank = ?parsed.detected_bank,
            detection_confidence = parsed.detection_confidence,
            row_count = parsed.rows.len(),
            "LLM parsed statement"
        );

        Ok(parsed)
    }
}

// ── Anthropic response types (internal) ───────────────────────────────────────

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
}

/// We only need the `tool_use` block; all other block types (text, thinking,
/// etc.) are captured as a raw `Value` and discarded. Using `#[serde(untagged)]`
/// lets serde try `ToolUse` first and fall through to `Other` for anything
/// that does not match.
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

// ── Tool schema (hand-written JSON Schema) ────────────────────────────────────

fn build_tool_schema() -> Value {
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
    async fn parse(&self, _raw: &str, _filename: &str) -> Result<ParsedStatement> {
        Ok(self.result.clone())
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;
    use chrono::NaiveDate;

    use super::*;

    fn make_row(date: &str, description: &str, amount: &str, confidence: f32) -> UnifiedStatementRow {
        UnifiedStatementRow {
            date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
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
        let parsed = mock.parse("anything", "test.csv").await.unwrap();
        assert_eq!(parsed.detected_bank, BankFormat::Unknown);
        assert_eq!(parsed.rows.len(), 1);
    }
}
