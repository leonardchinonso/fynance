//! The LLM call: one file in, the raw payslips and P60s it contains out.

use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};

use crate::importers::provider::{LlmProvider, ProviderCallResult};
use crate::model::Agent;

const TAX_DOCUMENT_PROMPT: &str = include_str!("../../../config/prompts/tax_document_parser.txt");
pub const TOOL_NAME: &str = "parse_tax_documents";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedTaxDocuments {
    pub detection_confidence: f32,
    pub documents: Vec<ParsedTaxDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedTaxDocument {
    pub document_type: String,
    #[serde(default)]
    pub employer: Option<String>,
    #[serde(default)]
    pub paye_reference: Option<String>,
    #[serde(default)]
    pub pay_date: Option<String>,
    #[serde(default)]
    pub period_start: Option<String>,
    #[serde(default)]
    pub period_end: Option<String>,
    #[serde(default)]
    pub printed_tax_year: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub line_items: Vec<ParsedTaxLine>,
    pub row_confidence: f32,
    #[serde(default)]
    pub notes: Option<String>,
    /// Filename of the upload this element came from; set by the caller.
    #[serde(skip)]
    pub source_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedTaxLine {
    pub code: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(deserialize_with = "string_or_number")]
    pub amount: String,
    #[serde(default = "default_basis")]
    pub basis: String,
}

fn default_basis() -> String {
    "period".to_string()
}

/// Models sometimes emit a JSON number where the schema asks for a string.
fn string_or_number<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        Value::Number(n) => Ok(n.to_string()),
        other => Err(serde::de::Error::custom(format!(
            "amount must be a string or number, got {other}"
        ))),
    }
}

pub const LINE_CODES: [&str; 11] = [
    "gross_pay",
    "taxable_pay",
    "tax_deducted",
    "employee_ni",
    "student_loan",
    "postgraduate_loan",
    "employee_pension",
    "notional_pay",
    "net_pay",
    "other_payment",
    "other_deduction",
];

pub fn build_tool_schema() -> Value {
    json!({
        "type": "object",
        "required": ["detection_confidence", "documents"],
        "properties": {
            "detection_confidence": {
                "type": "number", "minimum": 0.0, "maximum": 1.0,
                "description": "Confidence that this file contains payslips or P60s."
            },
            "documents": {
                "type": "array",
                "description": "One element per payslip or P60 in the file.",
                "items": {
                    "type": "object",
                    "required": ["document_type", "employer", "line_items", "row_confidence"],
                    "properties": {
                        "document_type": { "type": "string", "enum": ["payslip", "p60", "other"] },
                        "employer": { "type": ["string", "null"], "description": "Employer name as printed." },
                        "paye_reference": { "type": ["string", "null"], "description": "Employer PAYE reference when printed." },
                        "pay_date": { "type": ["string", "null"], "description": "YYYY-MM-DD. Null for a P60." },
                        "period_start": { "type": ["string", "null"], "description": "YYYY-MM-DD, when printed." },
                        "period_end": { "type": ["string", "null"], "description": "YYYY-MM-DD, when printed." },
                        "printed_tax_year": { "type": ["string", "null"], "description": "Tax year text copied verbatim from the document." },
                        "currency": { "type": ["string", "null"], "description": "ISO 4217 code. GBP unless stated." },
                        "line_items": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "required": ["code", "amount", "basis"],
                                "properties": {
                                    "code": { "type": "string", "enum": LINE_CODES },
                                    "label": { "type": ["string", "null"], "description": "Wording printed beside the figure." },
                                    "amount": { "type": "string", "description": "Plain non-negative decimal string, no symbol or thousands separator." },
                                    "basis": { "type": "string", "enum": ["period", "year_to_date"] }
                                }
                            }
                        },
                        "row_confidence": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                        "notes": { "type": ["string", "null"] }
                    }
                }
            }
        }
    })
}

pub struct LlmTaxDocumentParser {
    provider: Arc<dyn LlmProvider>,
}

impl LlmTaxDocumentParser {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }

    /// One call for one file. Elements are attributed to `filename`.
    pub async fn extract(
        &self,
        filename: &str,
        mime: &str,
        bytes: Vec<u8>,
        user_hint: Option<&str>,
        agent_override: Option<Agent>,
    ) -> Result<(ParsedTaxDocuments, ProviderCallResult)> {
        let mut supplement = format!("filename: {filename}");
        if let Some(hint) = user_hint {
            supplement = format!("User instructions: {hint}\n\n{supplement}");
        }
        let call = self
            .provider
            .chat_with_files_and_tools(
                TAX_DOCUMENT_PROMPT,
                &[(filename.to_string(), mime.to_string(), bytes)],
                &supplement,
                TOOL_NAME,
                build_tool_schema(),
                agent_override,
            )
            .await?;
        let mut parsed: ParsedTaxDocuments = crate::importers::deserialize_tool_use(
            call.value.clone(),
            "tax document parser",
            filename,
            TOOL_NAME,
        )?;
        for doc in &mut parsed.documents {
            doc.source_file = Some(filename.to_string());
        }
        Ok((parsed, call))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importers::provider::testing::MockProvider;

    #[tokio::test]
    async fn extracts_documents_and_attributes_them_to_the_file() {
        let provider = MockProvider::new(json!({
            "detection_confidence": 0.9,
            "documents": [{
                "document_type": "payslip",
                "employer": "Example Widgets Ltd",
                "pay_date": "2025-06-27",
                "line_items": [{"code": "gross_pay", "amount": 3000, "basis": "period"}],
                "row_confidence": 0.95
            }]
        }));
        let parser = LlmTaxDocumentParser::new(provider);
        let (parsed, _) = parser
            .extract("slips.pdf", "application/pdf", vec![1], None, None)
            .await
            .unwrap();
        assert_eq!(parsed.documents.len(), 1);
        assert_eq!(
            parsed.documents[0].source_file.as_deref(),
            Some("slips.pdf")
        );
        assert_eq!(parsed.documents[0].line_items[0].amount, "3000");
    }

    #[tokio::test]
    async fn schema_mismatch_names_the_tool() {
        let provider = MockProvider::new(json!({"documents": "nope"}));
        let parser = LlmTaxDocumentParser::new(provider);
        let err = parser
            .extract("slips.pdf", "application/pdf", vec![1], None, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains(TOOL_NAME));
    }
}
