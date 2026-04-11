//! Importer abstraction.
//!
//! Phase 1 has exactly one implementation, `csv_importer::CsvImporter`,
//! but the trait is here so Phase 2 can plug a JSON / multipart importer in
//! without touching the `import` command.

pub mod csv_importer;
pub mod llm_parser;
pub mod unified;

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use crate::model::ImportResult;
use crate::storage::Db;

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
            let parser = llm_parser::LlmStatementParser::from_env()?;
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
