//! CSV importer — thin adapter over `LlmStatementParser`.
//!
//! The previous version of this file contained `detect_format`, `ColumnIndex`,
//! `map_row`, and a hard-coded dispatch per bank dialect. All of that logic has
//! been replaced by `LlmStatementParser`, which handles any CSV that a bank
//! could export. See `docs/plans/10_llm_csv_import.md` for the design.
//!
//! `CsvImporter` now just:
//! 1. Reads the file to a string.
//! 2. Calls `parser.parse()` (blocking on the async future with a fresh
//!    single-threaded Tokio runtime for the CLI path).
//! 3. Applies the two confidence gates.
//! 4. Fingerprints and inserts each accepted row.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use indicatif::{ProgressBar, ProgressStyle};

use crate::importers::Importer;
use crate::importers::llm_parser::StatementParser;
use crate::model::{ImportResult, InsertOutcome, Transaction};
use crate::storage::Db;

pub struct CsvImporter {
    pub parser: Arc<dyn StatementParser>,
    /// File-level confidence threshold: import fails hard if the LLM's
    /// detection_confidence falls below this.
    pub min_detection_confidence: f32,
    /// Row-level confidence threshold: rows below this are skipped with a
    /// warning; the rest of the file still ingests.
    pub min_row_confidence: f32,
}

impl Importer for CsvImporter {
    fn import(&self, path: &Path, account_id: &str, db: &Db) -> Result<ImportResult> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("reading {path:?}"))?;
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("<unknown>")
            .to_string();

        // The `Importer` trait is synchronous (Phase 1 CLI path). We spin up
        // a lightweight single-threaded runtime just for this call. The server
        // path (Phase 2) will call `parser.parse().await` directly from an
        // async handler.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("building tokio runtime for CSV import")?;
        let parsed = rt.block_on(self.parser.parse(&raw, &filename))?;

        if parsed.detection_confidence < self.min_detection_confidence {
            return Err(anyhow!(
                "{filename}: LLM detection confidence {:.2} is below threshold {:.2}. \
                 The file may not be a bank statement. \
                 Raise FYNANCE_IMPORT_MIN_DETECT_CONF to lower the bar or inspect the file.",
                parsed.detection_confidence,
                self.min_detection_confidence,
            ));
        }

        let progress = ProgressBar::new(parsed.rows.len() as u64);
        progress.set_style(
            ProgressStyle::with_template("{spinner} {pos}/{len} rows — {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        progress.set_message(filename.clone());

        let mut result = ImportResult {
            filename: filename.clone(),
            account_id: account_id.to_string(),
            detected_bank: parsed.detected_bank,
            detection_confidence: parsed.detection_confidence,
            ..ImportResult::default()
        };

        for row in parsed.rows {
            result.rows_total += 1;
            progress.inc(1);

            if row.row_confidence < self.min_row_confidence {
                tracing::warn!(
                    filename,
                    row_confidence = row.row_confidence,
                    threshold = self.min_row_confidence,
                    "skipping low-confidence row"
                );
                continue;
            }

            let tx = Transaction::from_unified(row, account_id);
            match db.insert_transaction(&tx)? {
                InsertOutcome::Inserted => result.rows_inserted += 1,
                InsertOutcome::Duplicate => result.rows_duplicate += 1,
            }
        }

        progress.finish_and_clear();
        Ok(result)
    }
}
