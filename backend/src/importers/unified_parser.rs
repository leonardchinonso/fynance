//! Unified-mode parser.
//!
//! Single LLM call that returns transactions + holdings + investments together
//! from any number of arbitrary documents (CSV, PDF, image, etc.). Each
//! document is shipped to the model as a file attachment with no server-side
//! parsing. The prompt is composed dynamically from optional sections so only
//! the requested entry types are described to the model.

use anyhow::Result;
use chrono::{NaiveDate, NaiveDateTime};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};

use super::document_parser::SnapshotPeriod;
use super::holdings_parser::{ParsedHoldingRow, ParsedHoldings};
use super::investments_parser::ParsedInvestmentRow;
use super::provider::{LlmProvider, ProviderCallResult};
use super::unified::UnifiedStatementRow;
use crate::importers::document_parser::ParseHints;
use crate::model::{Agent, Holding};

const INTRO: &str = include_str!("../../config/prompts/unified/intro.txt");
const OUTPUT_SHAPE: &str = include_str!("../../config/prompts/unified/output_shape.txt");
const TRANSACTIONS_SECTION: &str =
    include_str!("../../config/prompts/unified/transactions_section.txt");
const HOLDINGS_SECTION: &str = include_str!("../../config/prompts/unified/holdings_section.txt");
const HOLDINGS_PERIOD_SNAPSHOT: &str =
    include_str!("../../config/prompts/unified/holdings_period_snapshot.txt");
const HOLDINGS_PERIOD_MONTHLY: &str =
    include_str!("../../config/prompts/unified/holdings_period_monthly.txt");
const HOLDINGS_PERIOD_QUARTERLY: &str =
    include_str!("../../config/prompts/unified/holdings_period_quarterly.txt");
const INVESTMENTS_SECTION: &str =
    include_str!("../../config/prompts/unified/investments_section.txt");
const CLOSING: &str = include_str!("../../config/prompts/unified/closing.txt");

// ── Input context built by the route handler ────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct UnifiedContext {
    pub categories: Vec<CategorySummary>,
    pub last_open_holdings: Vec<HoldingSummary>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CategorySummary {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HoldingSummary {
    pub symbol: String,
    pub name: String,
    pub holding_type: String,
    pub quantity: String,
    pub currency: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of: Option<String>,
}

// ── Output ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct UnifiedExtraction {
    pub transactions: Vec<UnifiedStatementRow>,
    pub holdings: Vec<Holding>,
    pub investments: Vec<ParsedInvestmentRow>,
    /// Forwarded to `IngestionMetadata.notes`.
    pub notes: Vec<String>,
    pub call: ProviderCallResult,
}

impl Default for UnifiedExtraction {
    fn default() -> Self {
        Self {
            transactions: Vec::new(),
            holdings: Vec::new(),
            investments: Vec::new(),
            notes: Vec::new(),
            call: ProviderCallResult {
                value: serde_json::Value::Null,
                usage: super::provider::TokenUsage::default(),
                model: String::new(),
                duration_ms: 0,
                stop_reason: None,
            },
        }
    }
}

// ── Raw shape returned by the LLM ───────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct UnifiedRaw {
    #[serde(default)]
    transactions: Vec<UnifiedTransactionRaw>,
    #[serde(default)]
    holdings: Vec<ParsedHoldingRow>,
    #[serde(default)]
    investments: Vec<ParsedInvestmentRow>,
}

#[derive(Debug, Deserialize)]
struct UnifiedTransactionRaw {
    /// `YYYY-MM-DD` or full ISO 8601 datetime.
    date: String,
    description: String,
    amount: String,
    currency: String,
    #[serde(default)]
    category_id: Option<String>,
    #[serde(default)]
    category_confidence: Option<f32>,
    row_confidence: f32,
    #[serde(default)]
    merchant: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    source_file: Option<String>,
}

// ── Public API ──────────────────────────────────────────────────────────────

pub async fn extract_all(
    files: &[(String, String, Vec<u8>)],
    hints: &ParseHints,
    account_id: &str,
    provider: &dyn LlmProvider,
    ctx: &UnifiedContext,
) -> Result<UnifiedExtraction> {
    let system_prompt = build_unified_prompt(hints, ctx);
    let tool_schema = build_unified_tool_schema();
    let agent_override = Some(hints.agent().unwrap_or(Agent::Sonnet));

    let mut text_supplement = format!(
        "Account being imported: {account_id}. Extract entries strictly according to the rules in the system prompt."
    );
    // Surface user-provided context to the model, but only when it's actually present.
    if let Some(hint) = hints.hint.as_deref().map(str::trim).filter(|h| !h.is_empty()) {
        text_supplement.push_str(
            "\n\nAdditional context provided by the user that they believe will help you \
             parse this document correctly:\n",
        );
        text_supplement.push_str(hint);
    }

    // Sizes only, never content (prompt + schema can be many KB).
    tracing::debug!(
        files = files.len(),
        system_prompt_bytes = system_prompt.len(),
        tool_schema_bytes = serde_json::to_string(&tool_schema).map(|s| s.len()).unwrap_or(0),
        text_supplement_bytes = text_supplement.len(),
        "unified parser: sending request"
    );

    let call = provider
        .chat_with_files_and_tools(
            &system_prompt,
            files,
            &text_supplement,
            "extract_unified",
            tool_schema,
            agent_override,
        )
        .await?;

    let raw: UnifiedRaw =
        serde_json::from_value(call.value.clone()).map_err(|e| {
            anyhow::anyhow!("unified parser: invalid tool input shape: {e}")
        })?;

    let mut extraction = post_validate(raw, hints, account_id)?;
    extraction.call = call;
    Ok(extraction)
}

// ── Prompt assembly ─────────────────────────────────────────────────────────

fn build_unified_prompt(hints: &ParseHints, ctx: &UnifiedContext) -> String {
    let mut out = String::with_capacity(8192);
    out.push_str(INTRO);
    out.push_str("\n\n");
    out.push_str(OUTPUT_SHAPE);
    out.push_str("\n\n");

    if hints.return_type.transactions {
        let categories_json = serde_json::to_string_pretty(&ctx.categories)
            .unwrap_or_else(|_| "[]".to_string());
        let section = TRANSACTIONS_SECTION.replace("{{CATEGORIES_JSON}}", &categories_json);
        out.push_str(&section);
        out.push_str("\n\n");
    }

    if hints.return_type.holdings.enabled {
        let holdings_json = serde_json::to_string_pretty(&ctx.last_open_holdings)
            .unwrap_or_else(|_| "[]".to_string());
        let section = HOLDINGS_SECTION.replace("{{LAST_HOLDINGS_JSON}}", &holdings_json);
        out.push_str(&section);
        out.push_str("\n\n");

        match hints.return_type.holdings.period {
            Some(SnapshotPeriod::Monthly) => out.push_str(HOLDINGS_PERIOD_MONTHLY),
            Some(SnapshotPeriod::Quarterly) => out.push_str(HOLDINGS_PERIOD_QUARTERLY),
            Some(SnapshotPeriod::Yearly) | None => out.push_str(HOLDINGS_PERIOD_SNAPSHOT),
        }
        out.push_str("\n\n");
    }

    if hints.return_type.investments {
        out.push_str(INVESTMENTS_SECTION);
        out.push_str("\n\n");
    }

    out.push_str(CLOSING);
    out
}

// ── Tool schema ─────────────────────────────────────────────────────────────

fn build_unified_tool_schema() -> Value {
    let source_file_prop = json!({
        "type": ["string", "null"],
        "description": "The exact filename (as provided in the upload) that THIS row was extracted from. Required when more than one file is uploaded so each row can be traced to its source document."
    });

    // Holdings / investments reuse the split-mode item schemas, with a
    // `source_file` property injected so unified-mode rows carry attribution.
    let mut holdings_item =
        super::holdings_parser::build_holdings_tool_schema()["properties"]["rows"]["items"].clone();
    if let Some(props) = holdings_item
        .get_mut("properties")
        .and_then(|p| p.as_object_mut())
    {
        props.insert("source_file".to_string(), source_file_prop.clone());
    }
    let mut investments_item = super::investments_parser::build_investments_tool_schema()
        ["properties"]["rows"]["items"]
        .clone();
    if let Some(props) = investments_item
        .get_mut("properties")
        .and_then(|p| p.as_object_mut())
    {
        props.insert("source_file".to_string(), source_file_prop.clone());
    }

    json!({
        "type": "object",
        "required": ["transactions", "holdings", "investments"],
        "properties": {
            "transactions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["date", "description", "amount", "currency", "row_confidence"],
                    "properties": {
                        "date": { "type": "string" },
                        "description": { "type": "string" },
                        "amount": { "type": "string" },
                        "currency": { "type": "string" },
                        "category_id": { "type": ["string", "null"] },
                        "category_confidence": { "type": ["number", "null"] },
                        "row_confidence": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                        "merchant": { "type": ["string", "null"] },
                        "notes": { "type": ["string", "null"] },
                        "source_file": source_file_prop
                    }
                }
            },
            "holdings": {
                "type": "array",
                "items": holdings_item
            },
            "investments": {
                "type": "array",
                "items": investments_item
            }
        }
    })
}

// ── Post-validation + conversion ────────────────────────────────────────────

fn post_validate(
    raw: UnifiedRaw,
    hints: &ParseHints,
    account_id: &str,
) -> Result<UnifiedExtraction> {
    let mut notes: Vec<String> = Vec::new();

    let raw_transactions = if hints.return_type.transactions {
        raw.transactions
    } else if !raw.transactions.is_empty() {
        notes.push(format!(
            "dropped {} unsolicited transaction entries from unified parser output",
            raw.transactions.len()
        ));
        Vec::new()
    } else {
        Vec::new()
    };

    let raw_holdings = if hints.return_type.holdings.enabled {
        raw.holdings
    } else if !raw.holdings.is_empty() {
        notes.push(format!(
            "dropped {} unsolicited holding entries from unified parser output",
            raw.holdings.len()
        ));
        Vec::new()
    } else {
        Vec::new()
    };

    let raw_investments = if hints.return_type.investments {
        raw.investments
    } else if !raw.investments.is_empty() {
        notes.push(format!(
            "dropped {} unsolicited investment entries from unified parser output",
            raw.investments.len()
        ));
        Vec::new()
    } else {
        Vec::new()
    };

    let transactions: Vec<UnifiedStatementRow> = raw_transactions
        .into_iter()
        .filter_map(|t| convert_transaction(t).ok())
        .collect();

    let parsed_holdings = ParsedHoldings {
        detection_confidence: 1.0,
        rows: raw_holdings,
    };
    let holdings = super::document_parser::convert_parsed_holdings(&parsed_holdings)?
        .into_iter()
        .map(|mut h| {
            h.account_id = account_id.to_string();
            h
        })
        .collect();

    Ok(UnifiedExtraction {
        transactions,
        holdings,
        investments: raw_investments,
        notes,
        ..Default::default()
    })
}

fn convert_transaction(t: UnifiedTransactionRaw) -> Result<UnifiedStatementRow> {
    let date = parse_iso_date(&t.date)?;
    let amount: Decimal = t
        .amount
        .parse()
        .map_err(|e| anyhow::anyhow!("unified parser: bad amount '{}': {e}", t.amount))?;

    Ok(UnifiedStatementRow {
        date,
        description: t.description,
        amount,
        currency: t.currency,
        fitid: None,
        category: None,
        merchant: t.merchant,
        counterparty: None,
        transaction_type: None,
        balance_after: None,
        notes: t.notes,
        reference: None,
        row_confidence: t.row_confidence,
        category_id: t.category_id,
        category_confidence: t.category_confidence,
        source_file: t.source_file,
    })
}

fn parse_iso_date(s: &str) -> Result<NaiveDateTime> {
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(d.and_hms_opt(0, 0, 0).unwrap());
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Ok(dt);
    }
    Err(anyhow::anyhow!(
        "unified parser: unrecognized date format '{s}'"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importers::document_parser::{
        ExperimentalParseOptions, HoldingsReturnConfig, ParseMode, ReturnType,
    };

    fn make_hints(tx: bool, holdings: bool, inv: bool) -> ParseHints {
        ParseHints {
            return_type: ReturnType {
                transactions: tx,
                holdings: HoldingsReturnConfig {
                    enabled: holdings,
                    period: None,
                },
                investments: inv,
            },
            experimental: Some(ExperimentalParseOptions {
                mode: ParseMode::Unified,
                agent: None,
            }),
            hint: None,
        }
    }

    #[test]
    fn test_prompt_includes_only_requested_sections() {
        let ctx = UnifiedContext::default();
        let hints = make_hints(true, false, false);
        let prompt = build_unified_prompt(&hints, &ctx);
        assert!(prompt.contains("TRANSACTIONS (requested)"));
        assert!(!prompt.contains("HOLDINGS (requested)"));
        assert!(!prompt.contains("INVESTMENTS (requested)"));
    }

    #[test]
    fn test_prompt_picks_monthly_period_section() {
        let ctx = UnifiedContext::default();
        let mut hints = make_hints(false, true, false);
        hints.return_type.holdings.period = Some(SnapshotPeriod::Monthly);
        let prompt = build_unified_prompt(&hints, &ctx);
        assert!(prompt.contains("HOLDINGS PERIOD: monthly"));
        assert!(!prompt.contains("HOLDINGS PERIOD: single snapshot"));
        assert!(!prompt.contains("HOLDINGS PERIOD: quarterly"));
    }

    #[test]
    fn test_post_validate_drops_unsolicited() {
        let raw = UnifiedRaw {
            transactions: vec![],
            holdings: vec![ParsedHoldingRow {
                symbol: "X".into(),
                name: "x".into(),
                holding_type: "cash".into(),
                quantity: "1".into(),
                price_per_unit: None,
                value: None,
                currency: "USD".into(),
                sub_account: None,
                as_of: None,
                row_confidence: 0.9,
                derived: false,
                source_file: None,
            }],
            investments: vec![],
        };
        let hints = make_hints(true, false, false);
        let extraction = post_validate(raw, &hints, "acct_test").unwrap();
        assert!(extraction.holdings.is_empty());
        assert_eq!(extraction.notes.len(), 1);
        assert!(extraction.notes[0].contains("dropped 1 unsolicited holding"));
    }

    #[test]
    fn test_convert_transaction_keeps_id_drops_name() {
        let raw = UnifiedTransactionRaw {
            date: "2026-03-15".into(),
            description: "Lidl".into(),
            amount: "-5.50".into(),
            currency: "GBP".into(),
            category_id: Some("cat-groceries".into()),
            category_confidence: Some(0.93),
            row_confidence: 0.97,
            merchant: None,
            notes: None,
            source_file: None,
        };
        let row = convert_transaction(raw).unwrap();
        assert_eq!(row.description, "Lidl");
        assert_eq!(row.category_id.as_deref(), Some("cat-groceries"));
        assert!(row.category.is_none());
    }
}
