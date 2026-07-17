//! LLM-based holdings parser.
//!
//! Sends CSV text to Anthropic and uses tool_use to extract holdings rows.
//! Analogous to `llm_parser.rs` but produces `ParsedHoldings` instead of
//! `ParsedStatement`.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::provider::{LlmProvider, ModelTier, ProviderCallResult};
use crate::model::Agent;

const HOLDINGS_PROMPT: &str = include_str!("../../config/prompts/holdings_parser.txt");
const MAX_CSV_BYTES: usize = 200_000;

// ── Output types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedHoldings {
    pub detection_confidence: f32,
    pub rows: Vec<ParsedHoldingRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedHoldingRow {
    pub symbol: String,
    pub name: String,
    pub holding_type: String,
    pub quantity: String,
    pub price_per_unit: Option<String>,
    pub value: Option<String>,
    pub currency: String,
    pub sub_account: Option<String>,
    pub as_of: Option<String>,
    pub row_confidence: f32,
    /// True when this snapshot was computed from other data (e.g. interpolated
    /// from transactions or neighbouring period balances) rather than read
    /// directly from the source document. Surfaces to the import preview so
    /// the user can tell explicit vs derived rows apart.
    #[serde(default)]
    pub derived: bool,
    /// Filename this row was attributed to during a parse. Set per-file in split
    /// mode or by the model in unified mode; resolved to `source_document_ids`.
    #[serde(default)]
    pub source_file: Option<String>,
}

// ── Trait ────────────────────────────────────────────────────────────────────

#[async_trait]
pub trait HoldingsExtractor: Send + Sync {
    async fn extract_holdings(
        &self,
        raw: &str,
        filename: &str,
        user_hint: Option<&str>,
        agent_override: Option<Agent>,
    ) -> Result<(ParsedHoldings, ProviderCallResult)>;
}

// ── LLM implementation ──────────────────────────────────────────────────────

pub struct LlmHoldingsParser {
    provider: Arc<dyn LlmProvider>,
    pub min_detection_confidence: f32,
    pub min_row_confidence: f32,
}

impl LlmHoldingsParser {
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
impl HoldingsExtractor for LlmHoldingsParser {
    async fn extract_holdings(
        &self,
        raw: &str,
        filename: &str,
        user_hint: Option<&str>,
        agent_override: Option<Agent>,
    ) -> Result<(ParsedHoldings, ProviderCallResult)> {
        let content = if raw.len() > MAX_CSV_BYTES {
            tracing::warn!(
                filename,
                bytes = raw.len(),
                max_bytes = MAX_CSV_BYTES,
                "Holdings CSV is large; truncating before sending to LLM"
            );
            &raw[..crate::util::char_boundary_floor(raw, MAX_CSV_BYTES)]
        } else {
            raw
        };

        let tool_schema = build_holdings_tool_schema();

        let mut user_msg = format!("filename: {filename}\n\n{content}");
        if let Some(hint) = user_hint {
            user_msg = format!("User instructions: {hint}\n\n{user_msg}");
        }

        let call = self
            .provider
            .chat_with_tools(
                HOLDINGS_PROMPT,
                &user_msg,
                "parse_holdings",
                tool_schema,
                ModelTier::Standard,
                agent_override,
            )
            .await?;

        let parsed: ParsedHoldings = super::deserialize_tool_use(
            call.value.clone(),
            "holdings parser",
            filename,
            "parse_holdings",
        )?;

        tracing::debug!(
            filename,
            detection_confidence = parsed.detection_confidence,
            row_count = parsed.rows.len(),
            "LLM parsed holdings"
        );

        Ok((parsed, call))
    }
}

// ── Tool schema ─────────────────────────────────────────────────────────────

pub(crate) fn build_holdings_tool_schema() -> Value {
    json!({
        "type": "object",
        "required": ["detection_confidence", "rows"],
        "properties": {
            "detection_confidence": {
                "type": "number",
                "minimum": 0.0,
                "maximum": 1.0,
                "description": "Confidence that this file contains holdings data."
            },
            "rows": {
                "type": "array",
                "description": "One element per holding row.",
                "items": {
                    "type": "object",
                    "required": ["symbol", "name", "holding_type", "quantity", "currency", "row_confidence"],
                    "properties": {
                        "symbol": {
                            "type": "string",
                            "description": "Ticker symbol, fund code, or currency code for cash."
                        },
                        "name": {
                            "type": "string",
                            "description": "Full display name of the holding."
                        },
                        "holding_type": {
                            "type": "string",
                            "enum": ["stock", "etf", "fund", "bond", "crypto", "cash", "property", "loan", "credit"],
                            "description": "Type of holding."
                        },
                        "quantity": {
                            "type": "string",
                            "description": "Number of units/shares as decimal string."
                        },
                        "price_per_unit": {
                            "type": ["string", "null"],
                            "description": "Price per unit as decimal string, or null if unavailable."
                        },
                        "value": {
                            "type": ["string", "null"],
                            "description": "Total market value as decimal string. Provide ONLY if explicitly present in the source data. Do NOT compute it. Null if absent."
                        },
                        "currency": {
                            "type": "string",
                            "description": "ISO 4217 currency code for the value."
                        },
                        "sub_account": {
                            "type": ["string", "null"],
                            "description": "Sub-account or pot name if multiple of same currency exist."
                        },
                        "as_of": {
                            "type": ["string", "null"],
                            "description": "ISO 8601 date (YYYY-MM-DD) this snapshot is valid for: the statement closing/period-end date or position date read from the document. Do NOT use today's date unless the document carries no usable date, in which case fall back to today AND set row_confidence below 0.3."
                        },
                        "row_confidence": {
                            "type": "number",
                            "minimum": 0.0,
                            "maximum": 1.0,
                            "description": "Confidence that this row was correctly parsed."
                        },
                        "derived": {
                            "type": "boolean",
                            "description": "True if this snapshot was computed/inferred from other data (e.g. interpolated from transactions or neighbouring period balances). False if read directly from the source document."
                        }
                    }
                }
            }
        }
    })
}

// ── Mock implementation ─────────────────────────────────────────────────────

#[cfg(test)]
pub struct MockHoldingsParser {
    pub result: ParsedHoldings,
}

#[cfg(test)]
#[async_trait]
impl HoldingsExtractor for MockHoldingsParser {
    async fn extract_holdings(
        &self,
        _raw: &str,
        _filename: &str,
        _user_hint: Option<&str>,
        _agent_override: Option<Agent>,
    ) -> Result<(ParsedHoldings, ProviderCallResult)> {
        Ok((
            self.result.clone(),
            ProviderCallResult {
                value: Value::Null,
                usage: super::provider::TokenUsage::default(),
                model: "mock".to_string(),
                duration_ms: 0,
                stop_reason: None,
            },
        ))
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_parser_returns_canned_result() {
        let parsed = ParsedHoldings {
            detection_confidence: 0.95,
            rows: vec![ParsedHoldingRow {
                symbol: "VUSA".to_string(),
                name: "Vanguard S&P 500 UCITS ETF".to_string(),
                holding_type: "etf".to_string(),
                quantity: "50.0000".to_string(),
                price_per_unit: Some("76.32".to_string()),
                value: Some("3816.00".to_string()),
                currency: "GBP".to_string(),
                sub_account: None,
                as_of: None,
                row_confidence: 0.97,
                derived: false,
                source_file: None,
            }],
        };

        let mock = MockHoldingsParser {
            result: parsed.clone(),
        };
        let (result, _call) = mock
            .extract_holdings("anything", "test.csv", None, None)
            .await
            .unwrap();
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].symbol, "VUSA");
        assert_eq!(result.detection_confidence, 0.95);
    }
}
