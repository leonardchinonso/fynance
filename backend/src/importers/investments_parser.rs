use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::provider::{LlmProvider, ModelTier, ProviderCallResult};
use crate::model::Agent;

const INVESTMENTS_PROMPT: &str = include_str!("../../config/prompts/investments_parser.txt");
const MAX_CSV_BYTES: usize = 200_000;

// ── Output types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedInvestments {
    pub detection_confidence: f32,
    pub rows: Vec<ParsedInvestmentRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedInvestmentRow {
    pub event_type: String,
    pub symbol: String,
    pub date: String,
    pub quantity: String,
    pub price_per_share: String,
    pub total_value: Option<String>,
    pub fee: String,
    pub currency: String,
    /// ISO 4217 currency of the fee when it differs from the trade currency.
    /// `None` (the common case) means the fee is in the trade currency.
    #[serde(default)]
    pub fee_currency: Option<String>,
    pub notes: Option<String>,
    pub row_confidence: f32,
    /// Filename this row was attributed to during a parse. Set per-file in split
    /// mode or by the model in unified mode; resolved to `source_document_ids`.
    #[serde(default)]
    pub source_file: Option<String>,
}

// ── Tool schema ──────────────────────────────────────────────────────────────

pub fn build_investments_tool_schema() -> Value {
    json!({
        "type": "object",
        "required": ["detection_confidence", "rows"],
        "properties": {
            "detection_confidence": {
                "type": "number",
                "minimum": 0.0,
                "maximum": 1.0,
                "description": "Confidence that this file contains investment event data."
            },
            "rows": {
                "type": "array",
                "description": "One element per investment event row.",
                "items": {
                    "type": "object",
                    "required": [
                        "event_type", "symbol", "date", "quantity",
                        "price_per_share", "fee", "currency", "row_confidence"
                    ],
                    "properties": {
                        "event_type": {
                            "type": "string",
                            "enum": ["vest", "buy", "sell", "transfer", "withhold", "split"],
                            "description": "Type of investment event."
                        },
                        "symbol": {
                            "type": "string",
                            "description": "Ticker or fund code. Use currency code for cash."
                        },
                        "date": {
                            "type": "string",
                            "description": "ISO 8601 datetime: YYYY-MM-DDTHH:MM:SS."
                        },
                        "quantity": {
                            "type": "string",
                            "description": "Number of units as a decimal string. Positive for buy/sell/vest/withhold (event_type encodes direction). For `transfer`, the sign carries direction: negative when shares leave this account (journal/transfer OUT), positive when they arrive. Never drop a transfer-out's negative sign."
                        },
                        "price_per_share": {
                            "type": "string",
                            "description": "Price per unit as decimal string. Use '0' when not applicable."
                        },
                        "total_value": {
                            "type": ["string", "null"],
                            "description": "Total transaction value as decimal string. Only if explicitly present in source data. Do NOT compute it. Null if absent."
                        },
                        "fee": {
                            "type": "string",
                            "description": "Transaction fee as decimal string. NEVER drop fees (CGT-allowable). Sum all fee columns on the row (commission + FX fee + exchange/stamp/SEC fee). '0' only if the statement genuinely shows no fee."
                        },
                        "currency": {
                            "type": "string",
                            "description": "ISO 4217 currency code of the price."
                        },
                        "fee_currency": {
                            "type": ["string", "null"],
                            "description": "ISO 4217 currency of the fee, ONLY if it differs from currency (e.g. USD-priced share with a GBP commission). Null when the fee is in the same currency as the price."
                        },
                        "notes": {
                            "type": ["string", "null"],
                            "description": "Optional note or reference from the source data."
                        },
                        "row_confidence": {
                            "type": "number",
                            "minimum": 0.0,
                            "maximum": 1.0,
                            "description": "Confidence that this row was correctly parsed."
                        }
                    }
                }
            }
        }
    })
}

// ── LLM implementation ────────────────────────────────────────────────────────

pub struct LlmInvestmentsParser {
    provider: Arc<dyn LlmProvider>,
}

impl LlmInvestmentsParser {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }

    pub async fn extract_investments(
        &self,
        raw: &str,
        filename: &str,
        user_hint: Option<&str>,
        agent_override: Option<Agent>,
    ) -> Result<(ParsedInvestments, ProviderCallResult)> {
        let content = if raw.len() > MAX_CSV_BYTES {
            tracing::warn!(
                filename,
                bytes = raw.len(),
                max_bytes = MAX_CSV_BYTES,
                "Investments CSV is large; truncating before sending to LLM"
            );
            &raw[..MAX_CSV_BYTES]
        } else {
            raw
        };

        let tool_schema = build_investments_tool_schema();

        let mut user_msg = format!("filename: {filename}\n\n{content}");
        if let Some(hint) = user_hint {
            user_msg = format!("User instructions: {hint}\n\n{user_msg}");
        }

        let call = self
            .provider
            .chat_with_tools(
                INVESTMENTS_PROMPT,
                &user_msg,
                "parse_investments",
                tool_schema,
                ModelTier::Standard,
                agent_override,
            )
            .await?;

        let parsed: ParsedInvestments = super::deserialize_tool_use(
            call.value.clone(),
            "investments parser",
            filename,
            "parse_investments",
        )?;

        tracing::debug!(
            filename,
            detection_confidence = parsed.detection_confidence,
            row_count = parsed.rows.len(),
            "LLM parsed investment events"
        );

        Ok((parsed, call))
    }
}

// ── Mock implementation ───────────────────────────────────────────────────────

#[cfg(test)]
pub struct MockInvestmentsParser {
    pub result: ParsedInvestments,
}

#[cfg(test)]
impl MockInvestmentsParser {
    pub async fn extract_investments(
        &self,
        _raw: &str,
        _filename: &str,
        _user_hint: Option<&str>,
        _agent_override: Option<Agent>,
    ) -> Result<(ParsedInvestments, ProviderCallResult)> {
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_parser_returns_canned_result() {
        let parsed = ParsedInvestments {
            detection_confidence: 0.95,
            rows: vec![ParsedInvestmentRow {
                event_type: "buy".to_string(),
                symbol: "VUSA".to_string(),
                date: "2026-03-15T00:00:00".to_string(),
                quantity: "10".to_string(),
                price_per_share: "76.32".to_string(),
                total_value: Some("763.20".to_string()),
                fee: "0".to_string(),
                currency: "GBP".to_string(),
                fee_currency: None,
                notes: None,
                row_confidence: 0.97,
                source_file: None,
            }],
        };

        let mock = MockInvestmentsParser {
            result: parsed.clone(),
        };
        let (result, _call) = mock
            .extract_investments("anything", "test.csv", None, None)
            .await
            .unwrap();
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].symbol, "VUSA");
        assert_eq!(result.rows[0].event_type, "buy");
        assert_eq!(result.detection_confidence, 0.95);
    }
}
