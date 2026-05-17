//! LLM-based periodic holdings parser.
//!
//! Instructs the LLM to compute periodic balance snapshots from transaction data.
//! Reuses `ParsedHoldings` and `ParsedHoldingRow` from `holdings_parser`.

use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};

use super::document_parser::SnapshotPeriod;
use super::holdings_parser::ParsedHoldings;

const PERIODIC_HOLDINGS_PROMPT: &str =
    include_str!("../../config/prompts/periodic_holdings_parser.txt");
const MAX_CSV_BYTES: usize = 200_000;

// ── LLM implementation ──────────────────────────────────────────────────────

pub struct LlmPeriodicHoldingsParser {
    client: Client,
    api_key: String,
    model: String,
}

impl LlmPeriodicHoldingsParser {
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("FYNANCE_ANTHROPIC_API_KEY").map_err(|_| {
            anyhow!("FYNANCE_ANTHROPIC_API_KEY is not set. Required for periodic holdings parsing.")
        })?;
        let model = std::env::var("FYNANCE_PARSE_EXTRACT_MODEL")
            .or_else(|_| std::env::var("FYNANCE_IMPORT_LLM_MODEL"))
            .unwrap_or_else(|_| "claude-haiku-4-5-20251001".to_string());

        Ok(Self {
            client: Client::new(),
            api_key,
            model,
        })
    }

    pub async fn extract_periodic_holdings(
        &self,
        raw: &str,
        filename: &str,
        period: &SnapshotPeriod,
        user_hint: Option<&str>,
    ) -> Result<ParsedHoldings> {
        let content = if raw.len() > MAX_CSV_BYTES {
            tracing::warn!(
                filename,
                bytes = raw.len(),
                max_bytes = MAX_CSV_BYTES,
                "CSV is large; truncating before sending to LLM for periodic holdings"
            );
            &raw[..MAX_CSV_BYTES]
        } else {
            raw
        };

        let period_str = match period {
            SnapshotPeriod::Monthly => "monthly",
            SnapshotPeriod::Quarterly => "quarterly",
            SnapshotPeriod::Yearly => "yearly",
        };

        let tool_schema = build_periodic_holdings_tool_schema();

        let mut user_msg =
            format!("Requested snapshot period: {period_str}\n\nfilename: {filename}\n\n{content}");
        if let Some(hint) = user_hint {
            user_msg = format!("User instructions: {hint}\n\n{user_msg}");
        }

        let request_body = json!({
            "model": self.model,
            "max_tokens": 8192,
            "system": PERIODIC_HOLDINGS_PROMPT,
            "tools": [{
                "name": "extract_periodic_holdings",
                "description": "Extract periodic balance snapshots from transaction data.",
                "input_schema": tool_schema
            }],
            "tool_choice": { "type": "tool", "name": "extract_periodic_holdings" },
            "messages": [{
                "role": "user",
                "content": user_msg
            }]
        });

        tracing::debug!(
            filename,
            period = period_str,
            bytes = content.len(),
            model = self.model,
            "sending CSV to Anthropic for periodic holdings extraction"
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
            .context("sending periodic holdings request to Anthropic API")?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("reading Anthropic periodic holdings response")?;

        if !status.is_success() {
            return Err(anyhow!(
                "Anthropic API returned {status} for periodic holdings: {body}"
            ));
        }

        tracing::debug!(
            filename,
            response_preview = &body[..body.len().min(300)],
            "received Anthropic periodic holdings response"
        );

        let api_resp: AnthropicResponse = serde_json::from_str(&body).with_context(|| {
            format!(
                "parsing Anthropic periodic holdings response (preview: {}...)",
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
                } if block_type == "tool_use" && name == "extract_periodic_holdings" => Some(input),
                _ => None,
            })
            .ok_or_else(|| anyhow!("no extract_periodic_holdings tool_use block in response"))?;

        let parsed: ParsedHoldings = serde_json::from_value(tool_input)
            .context("deserializing ParsedHoldings from periodic holdings tool_use input")?;

        tracing::debug!(
            filename,
            period = period_str,
            detection_confidence = parsed.detection_confidence,
            row_count = parsed.rows.len(),
            "LLM extracted periodic holdings"
        );

        Ok(parsed)
    }
}

// ── Tool schema ─────────────────────────────────────────────────────────────

fn build_periodic_holdings_tool_schema() -> Value {
    json!({
        "type": "object",
        "required": ["detection_confidence", "rows"],
        "properties": {
            "detection_confidence": {
                "type": "number",
                "minimum": 0.0,
                "maximum": 1.0,
                "description": "Confidence that periodic balance snapshots were correctly computed."
            },
            "rows": {
                "type": "array",
                "description": "One element per balance snapshot at a period boundary.",
                "items": {
                    "type": "object",
                    "required": ["symbol", "name", "holding_type", "quantity", "value", "currency", "as_of", "row_confidence"],
                    "properties": {
                        "symbol": {
                            "type": "string",
                            "description": "Currency code (e.g. GBP, USD, EUR)."
                        },
                        "name": {
                            "type": "string",
                            "description": "Display name, e.g. 'GBP Balance'."
                        },
                        "holding_type": {
                            "type": "string",
                            "enum": ["cash"],
                            "description": "Always 'cash' for balance snapshots."
                        },
                        "quantity": {
                            "type": "string",
                            "enum": ["1"],
                            "description": "Always '1' for cash balance snapshots."
                        },
                        "price_per_unit": {
                            "type": ["string", "null"],
                            "description": "The balance amount as a decimal string."
                        },
                        "value": {
                            "type": "string",
                            "description": "Same as price_per_unit (for cash, value = balance)."
                        },
                        "currency": {
                            "type": "string",
                            "description": "ISO 4217 currency code."
                        },
                        "as_of": {
                            "type": "string",
                            "description": "Period boundary date in YYYY-MM-DD format."
                        },
                        "sub_account": {
                            "type": ["string", "null"],
                            "description": "Sub-account or pot name if applicable."
                        },
                        "row_confidence": {
                            "type": "number",
                            "minimum": 0.0,
                            "maximum": 1.0,
                            "description": "Confidence that this snapshot is correct."
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
