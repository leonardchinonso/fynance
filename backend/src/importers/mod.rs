//! Importer abstraction.
//!
//! Phase 1 has exactly one implementation, `csv_importer::CsvImporter`,
//! but the trait is here so Phase 2 can plug a JSON / multipart importer in
//! without touching the `import` command.

pub mod csv_importer;
pub mod document_parser;
pub mod format_detection;
pub mod holdings_parser;
pub mod investments_parser;
pub mod llm_parser;
pub mod pdf_parser;
pub mod periodic_holdings_parser;
pub mod pricing;
pub mod provider;
pub mod unified;
pub mod unified_parser;

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use crate::model::ImportResult;
use crate::storage::Db;

/// Deserialize a `tool_use` JSON payload from the LLM into the parser's
/// expected struct, attaching rich context to any serde failure so the error
/// tells you which parser failed, which file/tool it was for, and which JSON
/// field tripped the schema. The raw payload is included (truncated) so
/// failures are reproducible from the error message alone — useful in
/// production where DEBUG logs may not be enabled.
pub(crate) fn deserialize_tool_use<T: serde::de::DeserializeOwned>(
    raw: serde_json::Value,
    parser_label: &str,
    filename: &str,
    tool_name: &str,
) -> Result<T> {
    serde_json::from_value::<T>(raw.clone()).map_err(|e| {
        let mut preview = serde_json::to_string(&raw).unwrap_or_default();
        const MAX: usize = 1024;
        if preview.len() > MAX {
            preview.truncate(MAX);
            preview.push_str("\u{2026}(truncated)");
        }
        anyhow::anyhow!(
            "{parser_label} (file `{filename}`, tool `{tool_name}`) returned JSON that did not match the expected schema: {e}. Raw tool response: {preview}"
        )
    })
}

pub trait Importer {
    fn import(&self, path: &Path, account_id: &str, db: &Db) -> Result<ImportResult>;
}

/// Pick an importer based on the file's extension. Today everything goes
/// through `CsvImporter` backed by `LlmStatementParser`. The API key and
/// model are read from environment variables; see `.env.example` for the
/// full list.
pub fn get_importer(path: &Path) -> Result<Box<dyn Importer>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("csv") => {
            let llm_provider = provider::create_provider()?;
            let parser = llm_parser::LlmStatementParser::new(llm_provider);
            let min_detection_confidence = parser.min_detection_confidence;
            let min_row_confidence = parser.min_row_confidence;
            Ok(Box::new(csv_importer::CsvImporter {
                parser: Arc::new(parser),
                min_detection_confidence,
                min_row_confidence,
            }))
        }
        other => Err(anyhow::anyhow!(
            "unsupported file extension: {other:?} (only .csv is supported in phase 1)"
        )),
    }
}
