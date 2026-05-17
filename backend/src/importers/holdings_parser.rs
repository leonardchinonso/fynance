//! LLM-based holdings parser.
//!
//! Sends CSV text to Anthropic and uses tool_use to extract holdings rows.
//! Analogous to `llm_parser.rs` but produces `ParsedHoldings` instead of
//! `ParsedStatement`.

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

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
    pub value: String,
    pub currency: String,
    pub sub_account: Option<String>,
    pub row_confidence: f32,
}

// ── Trait ────────────────────────────────────────────────────────────────────

#[async_trait]
pub trait HoldingsExtractor: Send + Sync {
    async fn extract_holdings(&self, raw: &str, filename: &str) -> Result<ParsedHoldings>;
}

// ── LLM implementation ──────────────────────────────────────────────────────

pub struct LlmHoldingsParser {
    client: Client,
    api_key: String,
    model: String,
    pub min_detection_confidence: f32,
    pub min_row_confidence: f32,
}

impl LlmHoldingsParser {
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("FYNANCE_ANTHROPIC_API_KEY").map_err(|_| {
            anyhow!("FYNANCE_ANTHROPIC_API_KEY is not set. Required for holdings parsing.")
        })?;
        let model = std::env::var("FYNANCE_PARSE_EXTRACT_MODEL")
            .or_else(|_| std::env::var("FYNANCE_IMPORT_LLM_MODEL"))
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
impl HoldingsExtractor for LlmHoldingsParser {
    async fn extract_holdings(&self, raw: &str, filename: &str) -> Result<ParsedHoldings> {
        let content = if raw.len() > MAX_CSV_BYTES {
            tracing::warn!(
                filename,
                bytes = raw.len(),
                max_bytes = MAX_CSV_BYTES,
                "Holdings CSV is large; truncating before sending to LLM"
            );
            &raw[..MAX_CSV_BYTES]
        } else {
            raw
        };

        let tool_schema = build_holdings_tool_schema();

        let request_body = json!({
            "model": self.model,
            "max_tokens": 8192,
            "system": HOLDINGS_PROMPT,
            "tools": [{
                "name": "parse_holdings",
                "description": "Parse holdings/positions data into structured records.",
                "input_schema": tool_schema
            }],
            "tool_choice": { "type": "tool", "name": "parse_holdings" },
            "messages": [{
                "role": "user",
                "content": format!("filename: {filename}\n\n{content}")
            }]
        });

        tracing::debug!(
            filename,
            bytes = content.len(),
            model = self.model,
            "sending holdings CSV to Anthropic"
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
            .context("sending holdings parse request to Anthropic API")?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("reading Anthropic holdings parse response")?;

        if !status.is_success() {
            return Err(anyhow!(
                "Anthropic API returned {status} for holdings parse: {body}"
            ));
        }

        tracing::debug!(
            filename,
            response_preview = &body[..body.len().min(300)],
            "received Anthropic holdings response"
        );

        let api_resp: AnthropicResponse = serde_json::from_str(&body).with_context(|| {
            format!(
                "parsing Anthropic holdings response (preview: {}...)",
                &body[..body.len().min(200)]
            )
        })?;

        let tool_input = api_resp
            .content
            .into_iter()
            .find_map(|block| match block {
                ContentBlock::ToolUse {
                    block_type,
                    name,
                    input,
                } if block_type == "tool_use" && name == "parse_holdings" => Some(input),
                _ => None,
            })
            .ok_or_else(|| anyhow!("no parse_holdings tool_use block in response"))?;

        let parsed: ParsedHoldings = serde_json::from_value(tool_input)
            .context("deserializing ParsedHoldings from tool_use input")?;

        tracing::debug!(
            filename,
            detection_confidence = parsed.detection_confidence,
            row_count = parsed.rows.len(),
            "LLM parsed holdings"
        );

        Ok(parsed)
    }
}

// ── Tool schema ─────────────────────────────────────────────────────────────

fn build_holdings_tool_schema() -> Value {
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
                    "required": ["symbol", "name", "holding_type", "quantity", "value", "currency", "row_confidence"],
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
                            "type": "string",
                            "description": "Total market value as decimal string."
                        },
                        "currency": {
                            "type": "string",
                            "description": "ISO 4217 currency code for the value."
                        },
                        "sub_account": {
                            "type": ["string", "null"],
                            "description": "Sub-account or pot name if multiple of same currency exist."
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

// ── Anthropic response types ────────────────────────────────────────────────

#[derive(Deserialize)]
struct AnthropicResponse {
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

// ── Mock implementation ─────────────────────────────────────────────────────

#[cfg(test)]
pub struct MockHoldingsParser {
    pub result: ParsedHoldings,
}

#[cfg(test)]
#[async_trait]
impl HoldingsExtractor for MockHoldingsParser {
    async fn extract_holdings(&self, _raw: &str, _filename: &str) -> Result<ParsedHoldings> {
        Ok(self.result.clone())
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
                value: "3816.00".to_string(),
                currency: "GBP".to_string(),
                sub_account: None,
                row_confidence: 0.97,
            }],
        };

        let mock = MockHoldingsParser {
            result: parsed.clone(),
        };
        let result = mock.extract_holdings("anything", "test.csv").await.unwrap();
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].symbol, "VUSA");
        assert_eq!(result.detection_confidence, 0.95);
    }
}
