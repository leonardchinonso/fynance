use std::sync::Arc;

use anyhow::{Result, anyhow};

use super::document_parser::SnapshotPeriod;
use super::holdings_parser::ParsedHoldings;
use super::investments_parser::{ParsedInvestments, build_investments_tool_schema};
use super::llm_parser::ParsedStatement;
use super::provider::{LlmProvider, ProviderCallResult};
use crate::model::Agent;

const STATEMENT_PROMPT: &str = include_str!("../../config/prompts/statement_parser.txt");
const HOLDINGS_PROMPT: &str = include_str!("../../config/prompts/holdings_parser.txt");
const PERIODIC_HOLDINGS_PROMPT: &str =
    include_str!("../../config/prompts/periodic_holdings_parser.txt");
const INVESTMENTS_PROMPT: &str = include_str!("../../config/prompts/investments_parser.txt");

/// MIME type for a binary document inferred from its filename extension. Drives
/// the Anthropic content block in the provider: `application/pdf` -> document
/// block, `image/*` -> image block. Defaults to PDF for unknown extensions.
fn binary_mime(filename: &str) -> String {
    match filename.rsplit('.').next().map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => "application/pdf",
    }
    .to_string()
}

// ── PDF Transaction Parser ─────────────────────────────────────────────────

pub struct PdfStatementParser {
    provider: Arc<dyn LlmProvider>,
}

impl PdfStatementParser {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }

    pub async fn parse(
        &self,
        pdf_bytes: &[u8],
        filename: &str,
        user_hint: Option<&str>,
        agent_override: Option<Agent>,
    ) -> Result<(ParsedStatement, ProviderCallResult)> {
        let tool_schema = super::llm_parser::build_tool_schema();

        let mut text_supplement = format!("filename: {filename}");
        if let Some(hint) = user_hint {
            text_supplement = format!("User instructions: {hint}\n\n{text_supplement}");
        }

        tracing::debug!(
            provider = self.provider.name(),
            filename,
            pdf_size = pdf_bytes.len(),
            "sending PDF for transaction extraction"
        );

        let call = self
            .provider
            .chat_with_files_and_tools(
                STATEMENT_PROMPT,
                &[(filename.to_string(), binary_mime(filename), pdf_bytes.to_vec())],
                &text_supplement,
                "parse_bank_statement",
                tool_schema,
                agent_override,
            )
            .await?;


        let parsed: ParsedStatement = super::deserialize_tool_use(
            call.value.clone(),
            "pdf bank statement parser",
            filename,
            "parse_bank_statement",
        )?;

        if parsed.rows.is_empty() {
            return Err(anyhow!(
                "No transactions could be extracted from PDF '{}'. \
                 This may be a scanned/image-only PDF that requires OCR (not supported). \
                 Try exporting as CSV from your bank instead.",
                filename
            ));
        }

        Ok((parsed, call))
    }
}

// ── PDF Holdings Parser ────────────────────────────────────────────────────

pub struct PdfHoldingsParser {
    provider: Arc<dyn LlmProvider>,
}

impl PdfHoldingsParser {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }

    pub async fn extract(
        &self,
        pdf_bytes: &[u8],
        filename: &str,
        user_hint: Option<&str>,
        agent_override: Option<Agent>,
    ) -> Result<(ParsedHoldings, ProviderCallResult)> {
        let tool_schema = super::holdings_parser::build_holdings_tool_schema();

        let mut text_supplement = format!("filename: {filename}");
        if let Some(hint) = user_hint {
            text_supplement = format!("User instructions: {hint}\n\n{text_supplement}");
        }

        let call = self
            .provider
            .chat_with_files_and_tools(
                HOLDINGS_PROMPT,
                &[(filename.to_string(), binary_mime(filename), pdf_bytes.to_vec())],
                &text_supplement,
                "parse_holdings",
                tool_schema,
                agent_override,
            )
            .await?;


        let parsed: ParsedHoldings = super::deserialize_tool_use(
            call.value.clone(),
            "pdf holdings parser",
            filename,
            "parse_holdings",
        )?;

        if parsed.rows.is_empty() {
            return Err(anyhow!(
                "No holdings could be extracted from PDF '{}'. \
                 This may be a scanned document or the format is not recognized.",
                filename
            ));
        }

        Ok((parsed, call))
    }
}

// ── PDF Periodic Holdings Parser ───────────────────────────────────────────

pub struct PdfPeriodicHoldingsParser {
    provider: Arc<dyn LlmProvider>,
}

impl PdfPeriodicHoldingsParser {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }

    pub async fn extract(
        &self,
        pdf_bytes: &[u8],
        filename: &str,
        period: &SnapshotPeriod,
        user_hint: Option<&str>,
        agent_override: Option<Agent>,
    ) -> Result<(ParsedHoldings, ProviderCallResult)> {
        let tool_schema = super::periodic_holdings_parser::build_periodic_holdings_tool_schema();

        let period_str = match period {
            SnapshotPeriod::Monthly => "monthly",
            SnapshotPeriod::Quarterly => "quarterly",
            SnapshotPeriod::Yearly => "yearly",
        };

        let mut text_supplement =
            format!("Requested snapshot period: {period_str}\n\nfilename: {filename}");
        if let Some(hint) = user_hint {
            text_supplement = format!("User instructions: {hint}\n\n{text_supplement}");
        }

        let call = self
            .provider
            .chat_with_files_and_tools(
                PERIODIC_HOLDINGS_PROMPT,
                &[(filename.to_string(), binary_mime(filename), pdf_bytes.to_vec())],
                &text_supplement,
                "extract_periodic_holdings",
                tool_schema,
                agent_override,
            )
            .await?;


        let parsed: ParsedHoldings = super::deserialize_tool_use(
            call.value.clone(),
            "pdf periodic holdings parser",
            filename,
            "extract_periodic_holdings",
        )?;

        Ok((parsed, call))
    }
}

// ── PDF Investments Parser ────────────────────────────────────────────────────

pub struct PdfInvestmentsParser {
    provider: Arc<dyn LlmProvider>,
}

impl PdfInvestmentsParser {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }

    pub async fn extract(
        &self,
        pdf_bytes: &[u8],
        filename: &str,
        user_hint: Option<&str>,
        agent_override: Option<Agent>,
    ) -> Result<(ParsedInvestments, ProviderCallResult)> {
        let tool_schema = build_investments_tool_schema();

        let mut text_supplement = format!("filename: {filename}");
        if let Some(hint) = user_hint {
            text_supplement = format!("User instructions: {hint}\n\n{text_supplement}");
        }

        tracing::debug!(
            provider = self.provider.name(),
            filename,
            pdf_size = pdf_bytes.len(),
            "sending PDF for investment event extraction"
        );

        let call = self
            .provider
            .chat_with_files_and_tools(
                INVESTMENTS_PROMPT,
                &[(filename.to_string(), binary_mime(filename), pdf_bytes.to_vec())],
                &text_supplement,
                "parse_investments",
                tool_schema,
                agent_override,
            )
            .await?;


        let parsed: ParsedInvestments = super::deserialize_tool_use(
            call.value.clone(),
            "pdf investments parser",
            filename,
            "parse_investments",
        )?;

        if parsed.rows.is_empty() {
            return Err(anyhow!(
                "No investment events could be extracted from PDF '{}'. \
                 This may be a scanned document or the format is not recognized.",
                filename
            ));
        }

        Ok((parsed, call))
    }
}
