//! LLM-based periodic holdings parser.
//!
//! Instructs the LLM to compute periodic balance snapshots from transaction data.
//! Reuses `ParsedHoldings` and `ParsedHoldingRow` from `holdings_parser`.

use std::sync::Arc;

use anyhow::Result;
use serde_json::{Value, json};

use super::document_parser::SnapshotPeriod;
use super::holdings_parser::ParsedHoldings;
use super::provider::{LlmProvider, ModelTier, ProviderCallResult};
use crate::model::Agent;

const PERIODIC_HOLDINGS_PROMPT: &str =
    include_str!("../../config/prompts/periodic_holdings_parser.txt");
const MAX_CSV_BYTES: usize = 200_000;

// ── LLM implementation ──────────────────────────────────────────────────────

pub struct LlmPeriodicHoldingsParser {
    provider: Arc<dyn LlmProvider>,
}

impl LlmPeriodicHoldingsParser {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }

    pub async fn extract_periodic_holdings(
        &self,
        raw: &str,
        filename: &str,
        period: &SnapshotPeriod,
        user_hint: Option<&str>,
        agent_override: Option<Agent>,
    ) -> Result<(ParsedHoldings, ProviderCallResult)> {
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

        let call = self
            .provider
            .chat_with_tools(
                PERIODIC_HOLDINGS_PROMPT,
                &user_msg,
                "extract_periodic_holdings",
                tool_schema,
                ModelTier::Standard,
                agent_override,
            )
            .await?;

        let parsed: ParsedHoldings = super::deserialize_tool_use(
            call.value.clone(),
            "periodic holdings parser",
            filename,
            "extract_periodic_holdings",
        )?;

        tracing::debug!(
            filename,
            period = period_str,
            detection_confidence = parsed.detection_confidence,
            row_count = parsed.rows.len(),
            "LLM extracted periodic holdings"
        );

        Ok((parsed, call))
    }
}

// ── Tool schema ─────────────────────────────────────────────────────────────

pub(crate) fn build_periodic_holdings_tool_schema() -> Value {
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
                            "description": "The balance amount as a decimal string (same as price_per_unit for cash)."
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
                        },
                        "derived": {
                            "type": "boolean",
                            "description": "True if this snapshot was computed from transactions (or neighbouring balances). False if the balance was read directly from the document for this exact period boundary."
                        }
                    }
                }
            }
        }
    })
}
