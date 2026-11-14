//! Unified-mode parser.
//!
//! Single LLM call that returns transactions + holdings + investments together
//! from any number of arbitrary documents (CSV, PDF, image, etc.). Each
//! document is shipped to the model as a file attachment with no server-side
//! parsing. The prompt is composed dynamically from optional sections so only
//! the requested entry types are described to the model.

use anyhow::Result;
use chrono::{Local, NaiveDate, NaiveDateTime};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::Path;

use super::document_parser::SnapshotPeriod;
use super::holdings_parser::ParsedHoldings;
use super::investments_parser::ParsedInvestmentRow;
use super::provider::{LlmProvider, ProviderCallResult};
use super::unified::UnifiedStatementRow;
use crate::importers::document_parser::ParseHints;
use crate::model::Holding;

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
    /// Display name(s) of the profile(s) that own the account being imported.
    /// Lets the model recognise transfers to/from the holder's own other
    /// accounts as self-transfers rather than money to other people.
    pub account_holder_names: Vec<String>,
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
    pub account_id: String,
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
                warnings: Vec::new(),
            },
        }
    }
}

// ── Raw shape returned by the LLM ───────────────────────────────────────────

// Sections hold raw JSON values: rows are deserialised one at a time so a
// malformed row drops only itself, and JSON-number money fields are rewritten
// to strings first (`Decimal` fields reject JSON numbers outright).
#[derive(Debug, Deserialize, Default)]
struct UnifiedRaw {
    #[serde(default)]
    transactions: Vec<Value>,
    #[serde(default)]
    holdings: Vec<Value>,
    #[serde(default)]
    investments: Vec<Value>,
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
    let agent_override = hints.agent();

    let mut text_supplement = format!(
        "Account being imported: {account_id}. Extract entries strictly according to the rules in the system prompt."
    );
    // Surface user-provided context to the model, but only when it's actually present.
    if let Some(hint) = hints
        .hint
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty())
    {
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
        tool_schema_bytes = serde_json::to_string(&tool_schema)
            .map(|s| s.len())
            .unwrap_or(0),
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

    dump_raw_output(&call, account_id);

    let raw: UnifiedRaw = serde_json::from_value(call.value.clone())
        .map_err(|e| anyhow::anyhow!("unified parser: invalid tool input shape: {e}"))?;

    let mut extraction = post_validate(raw, hints, account_id, ctx)?;
    // Degraded-input notes from the call itself (a PDF page the extract tool
    // could not read) join the row-drop notes so they reach metadata.notes.
    extraction.notes.extend(call.warnings.iter().cloned());
    extraction.call = call;
    Ok(extraction)
}

/// When `FYNANCE_PARSE_DUMP_DIR` is set, write the raw tool JSON for this call
/// for diagnosis and scoring. The dump holds personal financial data, so it is
/// opt-in and its path is logged. Failures to write never fail the parse.
fn dump_raw_output(call: &ProviderCallResult, account_id: &str) {
    let dir = match std::env::var_os("FYNANCE_PARSE_DUMP_DIR") {
        Some(d) if !d.is_empty() => d,
        _ => return,
    };
    // Millisecond stamp plus a short nonce: two calls for the same account
    // in the same millisecond must not overwrite each other's dump. The
    // account id goes into a path segment, so only safe characters survive.
    let stamp = Local::now().format("%Y%m%dT%H%M%S%.3f");
    let safe_account: String = account_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let path = Path::new(&dir).join(format!(
        "{stamp}-{safe_account}-{}-unified.json",
        &nonce[..8]
    ));
    let payload = json!({
        "model": call.model,
        "stop_reason": call.stop_reason,
        "usage": {
            "input_tokens": call.usage.input_tokens,
            "output_tokens": call.usage.output_tokens,
        },
        "duration_ms": call.duration_ms,
        "value": call.value,
    });
    let result = serde_json::to_vec_pretty(&payload)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        .and_then(|bytes| std::fs::write(&path, bytes));
    match result {
        Ok(()) => {
            tracing::info!(path = %path.display(), "unified parser: wrote raw model output dump")
        }
        Err(e) => tracing::warn!(
            path = %path.display(),
            "unified parser: failed to write raw output dump: {e}"
        ),
    }
}

// ── Prompt assembly ─────────────────────────────────────────────────────────

fn build_unified_prompt(hints: &ParseHints, ctx: &UnifiedContext) -> String {
    let mut out = String::with_capacity(8192);
    out.push_str(INTRO);
    out.push_str("\n\n");
    out.push_str(OUTPUT_SHAPE);
    out.push_str("\n\n");

    if hints.return_type.transactions {
        let categories_json =
            serde_json::to_string_pretty(&ctx.categories).unwrap_or_else(|_| "[]".to_string());
        let holders = if ctx.account_holder_names.is_empty() {
            "(not provided)".to_string()
        } else {
            ctx.account_holder_names.join(", ")
        };
        let section = TRANSACTIONS_SECTION
            .replace("{{CATEGORIES_JSON}}", &categories_json)
            .replace("{{ACCOUNT_HOLDER_NAMES}}", &holders);
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
        "description": "The filename of the source document this row was extracted from, copied verbatim from the \"Source document filename: …\" label shown before each uploaded document. When more than one file is uploaded you MUST set this on every row so each row is traced to the correct document. With a single uploaded file it may be null."
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
    let mut investments_item =
        super::investments_parser::build_investments_tool_schema()["properties"]["rows"]["items"]
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
    ctx: &UnifiedContext,
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

    let mut transactions = Vec::new();
    let mut tx_drops: Vec<String> = Vec::new();
    for value in raw_transactions {
        match convert_transaction_row(value) {
            Ok(row) => transactions.push(row),
            // One entry per dropped row, whatever the number of reasons, so
            // the note's count is a row count.
            Err(reasons) => tx_drops.push(reasons.join(", ")),
        }
    }
    if !tx_drops.is_empty() {
        notes.push(drop_note("transaction", &tx_drops));
    }

    let known_categories: HashSet<&str> = ctx.categories.iter().map(|c| c.id.as_str()).collect();
    let cleared = scrub_unknown_categories(&mut transactions, &known_categories);
    if cleared > 0 {
        notes.push(format!(
            "cleared category ids on {cleared} transaction rows (not in the known category list)"
        ));
    }

    let holding_rows = deserialize_rows(
        raw_holdings,
        &["quantity", "price_per_unit", "value"],
        "holding",
        &mut notes,
    );
    let parsed_holdings = ParsedHoldings {
        detection_confidence: 1.0,
        rows: holding_rows,
    };
    let (holdings, holding_notes) =
        super::document_parser::convert_parsed_holdings_lenient(&parsed_holdings);
    notes.extend(holding_notes);
    let holdings = holdings
        .into_iter()
        .map(|mut h| {
            h.account_id = account_id.to_string();
            h
        })
        .collect();

    let investments = deserialize_rows(
        raw_investments,
        &["quantity", "price_per_share", "total_value", "fee"],
        "investment",
        &mut notes,
    );

    Ok(UnifiedExtraction {
        transactions,
        holdings,
        investments,
        notes,
        ..Default::default()
    })
}

/// Deserialise each row on its own, rewriting JSON-number money fields to
/// strings first. Rows that still fail are dropped; one note per section
/// reports the count and up to 3 examples.
fn deserialize_rows<T: serde::de::DeserializeOwned>(
    values: Vec<Value>,
    money_fields: &[&str],
    section: &str,
    notes: &mut Vec<String>,
) -> Vec<T> {
    let mut rows = Vec::with_capacity(values.len());
    let mut drops: Vec<String> = Vec::new();
    for mut value in values {
        stringify_numeric_fields(&mut value, money_fields);
        match serde_json::from_value(value) {
            Ok(row) => rows.push(row),
            Err(e) => drops.push(short_serde_error(&e.to_string())),
        }
    }
    if !drops.is_empty() {
        notes.push(drop_note(section, &drops));
    }
    rows
}

fn convert_transaction_row(value: Value) -> std::result::Result<UnifiedStatementRow, Vec<String>> {
    let mut value = value;
    stringify_numeric_fields(&mut value, &["amount"]);
    let raw = match serde_json::from_value::<UnifiedTransactionRaw>(value) {
        Ok(raw) => raw,
        Err(e) => return Err(vec![short_serde_error(&e.to_string())]),
    };
    let mut reasons = Vec::new();
    let date = match parse_date_lenient(&raw.date) {
        Ok(d) => Some(d),
        Err(_) => {
            reasons.push(format!("bad date '{}'", raw.date));
            None
        }
    };
    let amount = match parse_amount_lenient(&raw.amount) {
        Ok((amount, printed_currency)) => {
            // A currency code printed on the figure must agree with the row's
            // currency field: "120.00 GBX" on a GBP row is a ×100 scale flip,
            // not a GBP amount.
            match printed_currency {
                Some(code) if !code.eq_ignore_ascii_case(raw.currency.trim()) => {
                    reasons.push(format!(
                        "amount currency '{code}' contradicts row currency '{}'",
                        raw.currency
                    ));
                    None
                }
                _ => Some(amount),
            }
        }
        Err(_) => {
            reasons.push(format!("bad amount '{}'", raw.amount));
            None
        }
    };
    let (Some(date), Some(amount)) = (date, amount) else {
        return Err(reasons);
    };

    Ok(UnifiedStatementRow {
        date,
        description: raw.description,
        amount,
        currency: raw.currency,
        fitid: None,
        category: None,
        merchant: raw.merchant,
        counterparty: None,
        transaction_type: None,
        balance_after: None,
        notes: raw.notes,
        reference: None,
        row_confidence: raw.row_confidence,
        category_id: raw.category_id,
        category_confidence: raw.category_confidence,
        source_file: raw.source_file,
    })
}

fn drop_note(section: &str, reasons: &[String]) -> String {
    let examples = reasons
        .iter()
        .map(|r| format!("'{r}'"))
        .take(3)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "dropped {} {} rows (examples: {})",
        reasons.len(),
        section,
        examples
    )
}

fn short_serde_error(msg: &str) -> String {
    match msg.find(" at line") {
        Some(i) => msg[..i].trim_end().to_string(),
        None => msg.to_string(),
    }
}

fn stringify_numeric_fields(row: &mut Value, fields: &[&str]) {
    let Some(obj) = row.as_object_mut() else {
        return;
    };
    for field in fields {
        let as_string = obj
            .get(*field)
            .and_then(Value::as_number)
            .map(|n| n.to_string());
        if let Some(s) = as_string {
            obj.insert((*field).to_string(), Value::String(s));
        }
    }
}

/// Clear category ids the model invented. An empty known set means the
/// category list was not provided, so there is nothing to check against.
fn scrub_unknown_categories(rows: &mut [UnifiedStatementRow], known: &HashSet<&str>) -> usize {
    if known.is_empty() {
        return 0;
    }
    let mut cleared = 0;
    for row in rows {
        let unknown = row
            .category_id
            .as_deref()
            .is_some_and(|id| !known.contains(id));
        if unknown {
            row.category_id = None;
            row.category_confidence = None;
            cleared += 1;
        }
    }
    cleared
}

/// Amounts the way bank documents actually print them: currency symbols and
/// ISO codes around the number, unicode minus, parentheses for negatives,
/// thousands separators. Ambiguous forms (European decimal commas, CR/DR
/// markers, an explicit sign inside parentheses) are rejected rather than
/// guessed. Returns the value plus any ISO currency code printed alongside
/// the number, so the caller can check it against the row's currency field.
fn parse_amount_lenient(raw: &str) -> Result<(Decimal, Option<String>)> {
    let mut s = raw.trim().to_string();
    if s.is_empty() {
        return Err(anyhow::anyhow!("empty amount"));
    }
    let parenthesised = s.len() >= 2 && s.starts_with('(') && s.ends_with(')');
    if parenthesised {
        s = s[1..s.len() - 1].trim().to_string();
    }
    s = s.replace('\u{2212}', "-");
    s = s.replace(['\u{00A3}', '$', '\u{20AC}', '\u{20A6}'], "");

    let mut printed_currency: Option<String> = None;
    let lead_end = s
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(s.len());
    if lead_end == 3 {
        printed_currency = Some(s[..lead_end].to_ascii_uppercase());
        s = s[lead_end..].trim_start().to_string();
    }
    let trimmed_alpha = s
        .trim_end_matches(|c: char| c.is_ascii_alphabetic())
        .to_string();
    let tail_len = s.len() - trimmed_alpha.len();
    if tail_len == 3 {
        printed_currency = Some(s[s.len() - 3..].to_ascii_uppercase());
        s = trimmed_alpha.trim_end().to_string();
    } else if tail_len == 2 && matches!(s[s.len() - 2..].to_ascii_uppercase().as_str(), "CR" | "DR")
    {
        // Accounting CR/DR markers would need a sign guess, so reject.
        return Err(anyhow::anyhow!("accounting CR/DR suffix"));
    }

    // Parentheses already say "negative"; an explicit sign inside them means
    // the source text is self-contradictory, and negating again would flip a
    // debit into a credit.
    if parenthesised && s.trim_start().starts_with(['-', '+']) {
        return Err(anyhow::anyhow!("already-signed parenthesised amount"));
    }

    let normalised =
        normalise_number_text(&s).ok_or_else(|| anyhow::anyhow!("unreadable amount '{raw}'"))?;
    let amount: Decimal = normalised
        .parse()
        .map_err(|_| anyhow::anyhow!("unreadable amount '{raw}'"))?;
    Ok((
        if parenthesised { -amount } else { amount },
        printed_currency,
    ))
}

/// Validate a decimal figure allowing only strict `1,234,567` thousands
/// grouping, and strip the commas.
fn normalise_number_text(s: &str) -> Option<String> {
    let (sign, rest) = match s.strip_prefix('-') {
        Some(r) => ("-", r),
        None => ("", s.strip_prefix('+').unwrap_or(s)),
    };
    let (int_part, frac_part) = match rest.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (rest, None),
    };
    if int_part.is_empty() || !int_part.bytes().all(|b| b.is_ascii_digit() || b == b',') {
        return None;
    }
    let groups: Vec<&str> = int_part.split(',').collect();
    if groups.iter().any(|g| g.is_empty()) {
        return None;
    }
    if groups.len() > 1 && (groups[0].len() > 3 || groups[1..].iter().any(|g| g.len() != 3)) {
        return None;
    }
    if let Some(f) = frac_part {
        if f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    let int_norm = groups.join("");
    match frac_part {
        Some(f) => Some(format!("{sign}{int_norm}.{f}")),
        None => Some(format!("{sign}{int_norm}")),
    }
}

/// Dates bank documents print: ISO date, `T` or space datetimes, optional
/// fractional seconds, optional `Z` or RFC 3339 offset (wall-clock time is
/// kept). Slashed forms are ambiguous, so they are rejected.
fn parse_date_lenient(s: &str) -> Result<NaiveDateTime> {
    let s = s.trim();
    if s.is_empty() {
        return Err(anyhow::anyhow!("empty date"));
    }
    let body = strip_tz_suffix(s);
    if let Ok(dt) = NaiveDateTime::parse_from_str(body, "%Y-%m-%dT%H:%M:%S%.f") {
        return Ok(dt);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(body, "%Y-%m-%d %H:%M:%S%.f") {
        return Ok(dt);
    }
    if let Ok(d) = NaiveDate::parse_from_str(body, "%Y-%m-%d") {
        return Ok(d.and_hms_opt(0, 0, 0).unwrap());
    }
    Err(anyhow::anyhow!("unrecognized date format '{s}'"))
}

fn strip_tz_suffix(s: &str) -> &str {
    if s.ends_with('Z') || s.ends_with('z') {
        return s[..s.len() - 1].trim_end();
    }
    if let Some(tail) = s.get(s.len().saturating_sub(6)..) {
        if tail.len() == 6
            && (tail.starts_with('+') || tail.starts_with('-'))
            && tail[1..3].bytes().all(|b| b.is_ascii_digit())
            && tail.as_bytes()[3] == b':'
            && tail[4..].bytes().all(|b| b.is_ascii_digit())
        {
            return s[..s.len() - 6].trim_end();
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importers::TEST_ENV_LOCK;
    use crate::importers::document_parser::{
        ExperimentalParseOptions, HoldingsReturnConfig, ParseMode, ReturnType,
    };
    use crate::importers::provider::testing::MockProvider;

    // `extract_all` reads `FYNANCE_PARSE_DUMP_DIR`, process-global env: every
    // test that calls it holds the crate-wide test env lock (see
    // `importers::mod`), shared with the provider and pdf_extract tests.

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
                auth: None,
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
            holdings: vec![holding_json("X", "1")],
            ..Default::default()
        };
        let hints = make_hints(true, false, false);
        let extraction =
            post_validate(raw, &hints, "acct_test", &UnifiedContext::default()).unwrap();
        assert!(extraction.holdings.is_empty());
        assert_eq!(extraction.notes.len(), 1);
        assert!(extraction.notes[0].contains("dropped 1 unsolicited holding"));
    }

    fn good_tx_json() -> Value {
        json!({
            "date": "2026-03-01",
            "description": "Corner Shop",
            "amount": "-12.50",
            "currency": "GBP",
            "category_id": null,
            "category_confidence": null,
            "row_confidence": 0.95,
            "merchant": null,
            "notes": null,
            "source_file": null
        })
    }

    fn holding_json(symbol: &str, quantity: &str) -> Value {
        json!({
            "symbol": symbol,
            "name": "Example Fund",
            "holding_type": "etf",
            "quantity": quantity,
            "price_per_unit": "10.00",
            "value": null,
            "currency": "USD",
            "sub_account": null,
            "as_of": "2026-03-31",
            "row_confidence": 0.9,
            "derived": false,
            "source_file": null
        })
    }

    /// Drives `extract_all` from a sync test: the env lock is a std mutex,
    /// and holding its guard across an `.await` is what clippy rejects, so
    /// the await happens inside a `block_on` on the test thread.
    fn run_extract(
        files: &[(String, String, Vec<u8>)],
        hints: &ParseHints,
        account_id: &str,
        provider: &dyn LlmProvider,
        ctx: &UnifiedContext,
    ) -> Result<UnifiedExtraction> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(extract_all(files, hints, account_id, provider, ctx))
    }

    // The matching "before" test ran on the pre-fix HEAD (92bf04c) in a
    // throwaway worktree and showed the whole parse failing on this input.
    #[test]
    fn test_bad_holding_quantity_drops_only_that_row() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tool_input = json!({
            "transactions": [good_tx_json()],
            "holdings": [holding_json("VWRD", "10"), holding_json("VWRL", "N/A")],
            "investments": []
        });
        let provider = MockProvider::new(tool_input);
        let hints = make_hints(true, true, false);
        let extraction = run_extract(
            &[],
            &hints,
            "acct_test",
            provider.as_ref(),
            &UnifiedContext::default(),
        )
        .unwrap();
        assert_eq!(extraction.transactions.len(), 1);
        assert_eq!(extraction.holdings.len(), 1);
        assert_eq!(extraction.holdings[0].symbol, "VWRD");
        assert_eq!(extraction.holdings[0].account_id, "acct_test");
        assert!(
            extraction
                .notes
                .iter()
                .any(|n| n.contains("holding") && n.contains("quantity 'N/A' for VWRL")),
            "notes: {:?}",
            extraction.notes
        );
    }

    #[test]
    fn test_convert_transaction_row_keeps_id_drops_name() {
        let row = convert_transaction_row(good_tx_json()).unwrap();
        assert_eq!(row.description, "Corner Shop");
        assert_eq!(row.amount, Decimal::new(-1250, 2));
        assert!(row.category_id.is_none());
        assert!(row.category.is_none());

        let mut with_cat = good_tx_json();
        with_cat["category_id"] = json!("cat-groceries");
        with_cat["category_confidence"] = json!(0.93);
        let row = convert_transaction_row(with_cat).unwrap();
        assert_eq!(row.category_id.as_deref(), Some("cat-groceries"));
        assert_eq!(row.category_confidence, Some(0.93));
    }

    #[test]
    fn test_parse_amount_lenient_accepts_common_forms() {
        let cases = [
            ("-12.5", "-12.5", None),
            ("£1,234.50", "1234.50", None),
            ("(12.00)", "-12.00", None),
            ("\u{2212}5.00", "-5.00", None),
            ("5.00 GBP", "5.00", Some("GBP")),
            ("GBP 1,234.50", "1234.50", Some("GBP")),
            ("$1234", "1234", None),
            ("-£5.00", "-5.00", None),
            ("£-5.00", "-5.00", None),
            ("₦1,500.00", "1500.00", None),
            ("120.00 gbp", "120.00", Some("GBP")),
        ];
        for (input, expected, expected_code) in cases {
            let (parsed, code) =
                parse_amount_lenient(input).unwrap_or_else(|e| panic!("rejecting '{input}': {e}"));
            assert_eq!(parsed.to_string(), expected, "for input '{input}'");
            assert_eq!(code.as_deref(), expected_code, "for input '{input}'");
        }
    }

    #[test]
    fn test_parse_amount_lenient_rejects_ambiguous_forms() {
        for input in [
            "1.234,56",
            "1,23.45",
            "100.00 CR",
            "250.00 DR",
            "250.00cr",
            "",
            "   ",
            "N/A",
            "1,234,56.7",
            // Parentheses already mean negative: an explicit sign inside them
            // would double-negate, flipping a debit into a credit.
            "(-12.00)",
            "(\u{2212}5.00)",
            "(£-5.00)",
            "(+12.00)",
            "(GBP -5.00)",
        ] {
            assert!(
                parse_amount_lenient(input).is_err(),
                "accepted ambiguous amount '{input}'"
            );
        }
    }

    #[test]
    fn test_currency_code_on_figure_must_match_row_currency() {
        let mut contra = good_tx_json();
        contra["amount"] = json!("120.00 GBX");
        let mut agrees = good_tx_json();
        agrees["amount"] = json!("12.00 gbp");
        let raw = UnifiedRaw {
            transactions: vec![contra, agrees],
            ..Default::default()
        };
        let hints = make_hints(true, false, false);
        let extraction =
            post_validate(raw, &hints, "acct_test", &UnifiedContext::default()).unwrap();
        assert_eq!(extraction.transactions.len(), 1);
        assert_eq!(extraction.transactions[0].amount, Decimal::new(1200, 2));
        let note = extraction
            .notes
            .iter()
            .find(|n| n.contains("transaction rows"))
            .expect("drop note");
        assert!(
            note.contains("currency 'GBX' contradicts row currency 'GBP'"),
            "note: {note}"
        );
    }

    #[test]
    fn test_one_row_with_two_problems_counted_once() {
        let mut double = good_tx_json();
        double["date"] = json!("not a date");
        double["amount"] = json!("1.234,56");
        let raw = UnifiedRaw {
            transactions: vec![double],
            ..Default::default()
        };
        let hints = make_hints(true, false, false);
        let extraction =
            post_validate(raw, &hints, "acct_test", &UnifiedContext::default()).unwrap();
        assert!(extraction.transactions.is_empty());
        let note = extraction
            .notes
            .iter()
            .find(|n| n.contains("transaction rows"))
            .expect("drop note");
        assert!(note.contains("dropped 1 transaction rows"), "note: {note}");
        assert!(note.contains("bad date 'not a date'"), "note: {note}");
        assert!(note.contains("bad amount '1.234,56'"), "note: {note}");
    }

    #[test]
    fn test_parse_date_lenient() {
        assert_eq!(
            parse_date_lenient("2026-08-12").unwrap(),
            NaiveDate::from_ymd_opt(2026, 8, 12)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
        );
        assert_eq!(
            parse_date_lenient("2026-08-12 10:11:12").unwrap(),
            NaiveDate::from_ymd_opt(2026, 8, 12)
                .unwrap()
                .and_hms_opt(10, 11, 12)
                .unwrap()
        );
        assert_eq!(
            parse_date_lenient("2026-08-12T10:11:12.5Z").unwrap(),
            NaiveDate::from_ymd_opt(2026, 8, 12)
                .unwrap()
                .and_hms_micro_opt(10, 11, 12, 500_000)
                .unwrap()
        );
        assert_eq!(
            parse_date_lenient("2026-08-12T10:11:12+01:00").unwrap(),
            NaiveDate::from_ymd_opt(2026, 8, 12)
                .unwrap()
                .and_hms_opt(10, 11, 12)
                .unwrap()
        );
        for input in ["12/08/2026", "08/12/2026", ""] {
            assert!(
                parse_date_lenient(input).is_err(),
                "accepted ambiguous date '{input}'"
            );
        }
    }

    #[test]
    fn test_missing_field_drops_only_that_transaction_row() {
        let mut missing = good_tx_json();
        missing.as_object_mut().unwrap().remove("description");
        let raw = UnifiedRaw {
            transactions: vec![good_tx_json(), missing, good_tx_json()],
            ..Default::default()
        };
        let hints = make_hints(true, false, false);
        let extraction =
            post_validate(raw, &hints, "acct_test", &UnifiedContext::default()).unwrap();
        assert_eq!(extraction.transactions.len(), 2);
        assert_eq!(extraction.notes.len(), 1);
        assert!(extraction.notes[0].contains("dropped 1 transaction rows"));
        assert!(extraction.notes[0].contains("missing field"));
        assert!(extraction.notes[0].contains("description"));
    }

    #[test]
    fn test_unparseable_date_and_amount_drop_with_notes() {
        let mut slash_date = good_tx_json();
        slash_date["date"] = json!("12/08/2026");
        let mut euro_amount = good_tx_json();
        euro_amount["amount"] = json!("1.234,56");
        let raw = UnifiedRaw {
            transactions: vec![slash_date, euro_amount],
            ..Default::default()
        };
        let hints = make_hints(true, false, false);
        let extraction =
            post_validate(raw, &hints, "acct_test", &UnifiedContext::default()).unwrap();
        assert!(extraction.transactions.is_empty());
        let note = extraction
            .notes
            .iter()
            .find(|n| n.contains("transaction rows"))
            .expect("drop note");
        assert!(note.contains("bad date '12/08/2026'"), "note: {note}");
        assert!(note.contains("bad amount '1.234,56'"), "note: {note}");
    }

    #[test]
    fn test_json_number_money_fields_are_stringified() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tool_input = json!({
            "transactions": [{
                "date": "2026-03-01",
                "description": "Payroll",
                "amount": -12.5,
                "currency": "GBP",
                "row_confidence": 0.9
            }],
            "holdings": [holding_json("VWRD", "10")],
            "investments": [
                {
                    "event_type": "buy",
                    "symbol": "VWRD",
                    "date": "2026-02-01",
                    "quantity": 10,
                    "price_per_share": 72.5,
                    "total_value": 725.0,
                    "fee": 1.5,
                    "currency": "USD",
                    "row_confidence": 0.95
                },
                {
                    "event_type": "sell",
                    "date": "2026-02-02",
                    "quantity": 1,
                    "price_per_share": 73.0,
                    "fee": 0.0,
                    "currency": "USD",
                    "row_confidence": 0.9
                }
            ]
        });
        let provider = MockProvider::new(tool_input);
        let hints = make_hints(true, true, true);
        let extraction = run_extract(
            &[],
            &hints,
            "acct_test",
            provider.as_ref(),
            &UnifiedContext::default(),
        )
        .unwrap();
        assert_eq!(extraction.transactions[0].amount, Decimal::new(-125, 1));
        assert_eq!(extraction.holdings.len(), 1);
        assert_eq!(extraction.investments.len(), 1);
        assert!(
            extraction
                .notes
                .iter()
                .any(|n| n.contains("investment rows") && n.contains("symbol")),
            "notes: {:?}",
            extraction.notes
        );
    }

    fn tx_row(category_id: Option<&str>) -> UnifiedStatementRow {
        let mut value = good_tx_json();
        value["category_id"] = json!(category_id);
        value["category_confidence"] = json!(category_id.map(|_| 0.85));
        convert_transaction_row(value).unwrap()
    }

    #[test]
    fn test_scrub_unknown_categories_clears_only_unknown() {
        let mut rows = vec![
            tx_row(Some("cat-groceries")),
            tx_row(Some("cat-made-up")),
            tx_row(None),
        ];
        let known: HashSet<&str> = ["cat-groceries"].into_iter().collect();
        assert_eq!(scrub_unknown_categories(&mut rows, &known), 1);
        assert_eq!(rows[0].category_id.as_deref(), Some("cat-groceries"));
        assert_eq!(rows[0].category_confidence, Some(0.85));
        assert!(rows[1].category_id.is_none());
        assert!(rows[1].category_confidence.is_none());

        // Empty context list means there is nothing to check against.
        let mut rows = vec![tx_row(Some("cat-made-up"))];
        assert_eq!(scrub_unknown_categories(&mut rows, &HashSet::new()), 0);
        assert_eq!(rows[0].category_id.as_deref(), Some("cat-made-up"));
    }

    #[test]
    fn test_extract_all_scrubs_category_and_notes() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut tx = good_tx_json();
        tx["category_id"] = json!("cat-made-up");
        let provider = MockProvider::new(json!({
            "transactions": [tx],
            "holdings": [],
            "investments": []
        }));
        let hints = make_hints(true, false, false);
        let ctx = UnifiedContext {
            categories: vec![CategorySummary {
                id: "cat-groceries".into(),
                name: "Groceries".into(),
                description: None,
            }],
            ..Default::default()
        };
        let extraction = run_extract(&[], &hints, "acct_test", provider.as_ref(), &ctx).unwrap();
        assert!(extraction.transactions[0].category_id.is_none());
        assert!(extraction.transactions[0].category_confidence.is_none());
        assert!(
            extraction
                .notes
                .iter()
                .any(|n| n.contains("cleared category ids on 1")),
            "notes: {:?}",
            extraction.notes
        );
    }

    #[test]
    fn test_provider_warnings_land_in_extraction_notes() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let provider = MockProvider::with_warnings(
            json!({ "transactions": [], "holdings": [], "investments": [] }),
            vec!["page 2 of statement.pdf could not be extracted: render failed".to_string()],
        );
        let hints = make_hints(true, false, false);
        let extraction = run_extract(
            &[],
            &hints,
            "acct_test",
            provider.as_ref(),
            &UnifiedContext::default(),
        )
        .unwrap();
        assert!(
            extraction
                .notes
                .iter()
                .any(|n| n == "page 2 of statement.pdf could not be extracted: render failed"),
            "notes: {:?}",
            extraction.notes
        );
    }

    #[test]
    fn test_non_object_top_level_fails_whole_parse() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let provider = MockProvider::new(json!(["not", "an", "object"]));
        let hints = make_hints(true, false, false);
        let result = run_extract(
            &[],
            &hints,
            "acct_test",
            provider.as_ref(),
            &UnifiedContext::default(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_dump_writes_raw_value_when_dir_set() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("FYNANCE_PARSE_DUMP_DIR", dir.path()) };
        let tool_input = json!({
            "transactions": [good_tx_json()],
            "holdings": [],
            "investments": []
        });
        let provider = MockProvider::new(tool_input.clone());
        let hints = make_hints(true, false, false);
        let result = run_extract(
            &[],
            &hints,
            "acct_test",
            provider.as_ref(),
            &UnifiedContext::default(),
        );
        unsafe { std::env::remove_var("FYNANCE_PARSE_DUMP_DIR") };
        let extraction = result.unwrap();

        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries.len(), 1, "entries: {entries:?}");
        let name = &entries[0];
        assert!(
            name.contains("-acct_test-") && name.ends_with("-unified.json") && name[8..9] == *"T",
            "file name: {name}"
        );
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join(name)).unwrap()).unwrap();
        assert_eq!(written["model"], "mock");
        assert_eq!(written["value"], tool_input);
        assert_eq!(extraction.transactions.len(), 1);
    }

    #[test]
    fn test_dump_names_collide_never_and_sanitise_account_id() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("FYNANCE_PARSE_DUMP_DIR", dir.path()) };
        let provider = MockProvider::new(json!({
            "transactions": [good_tx_json()],
            "holdings": [],
            "investments": []
        }));
        let hints = make_hints(true, false, false);
        // Two calls back to back for the same account: same second, likely
        // same millisecond, and a hostile account id.
        for account in ["acct_test", "we/ird:na\\me"] {
            run_extract(
                &[],
                &hints,
                account,
                provider.as_ref(),
                &UnifiedContext::default(),
            )
            .unwrap();
        }
        unsafe { std::env::remove_var("FYNANCE_PARSE_DUMP_DIR") };

        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries.len(), 2, "dumps must not overwrite: {entries:?}");
        for name in entries {
            assert!(
                name.ends_with("-unified.json") && name[8..9] == *"T",
                "{name}"
            );
            assert!(
                name.contains("acct_test") || name.contains("we_ird_na_me"),
                "hostile id not sanitised: {name}"
            );
            assert!(
                !name.contains('/') && !name.contains('\\') && !name.contains(':'),
                "unsafe path character in dump name: {name}"
            );
        }
    }

    #[test]
    fn test_unwritable_dump_dir_does_not_fail_parse() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        std::fs::remove_dir_all(dir.path()).unwrap();
        unsafe { std::env::set_var("FYNANCE_PARSE_DUMP_DIR", dir.path()) };
        let provider = MockProvider::new(json!({
            "transactions": [good_tx_json()],
            "holdings": [],
            "investments": []
        }));
        let hints = make_hints(true, false, false);
        let result = run_extract(
            &[],
            &hints,
            "acct_test",
            provider.as_ref(),
            &UnifiedContext::default(),
        );
        unsafe { std::env::remove_var("FYNANCE_PARSE_DUMP_DIR") };
        assert!(result.is_ok());
    }
}
