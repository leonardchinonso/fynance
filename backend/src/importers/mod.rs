//! Importer abstraction.
//!
//! Phase 1 has exactly one implementation, `csv_importer::CsvImporter`,
//! but the trait is here so Phase 2 can plug a JSON / multipart importer in
//! without touching the `import` command.

pub mod csv_importer;

use std::path::Path;

use anyhow::Result;

use crate::model::ImportResult;
use crate::storage::Db;

pub trait Importer {
    fn import(&self, path: &Path, account_id: &str, db: &Db) -> Result<ImportResult>;
}

/// Pick an importer based on the file's extension. Today everything goes
/// through `CsvImporter`; format auto-detection of the bank dialect itself
/// is done inside that importer by looking at the header row.
pub fn get_importer(path: &Path) -> Result<Box<dyn Importer>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("csv") => Ok(Box::new(csv_importer::CsvImporter)),
        other => Err(anyhow::anyhow!(
            "unsupported file extension: {other:?} (only .csv is supported in phase 1)"
        )),
    }
}
