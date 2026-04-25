//! `fynance import` — read one CSV file or a directory of CSVs into the DB.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::importers::get_importer;
use crate::model::{BankFormat, ImportLog, ImportResult};
use crate::storage::Db;

pub fn run(db: &Db, path: &Path, account_id: &str) -> Result<()> {
    let files = collect_csv_files(path)?;
    if files.is_empty() {
        println!("No CSV files found at {path:?}");
        return Ok(());
    }

    let mut totals = ImportResult {
        filename: "<total>".to_string(),
        account_id: account_id.to_string(),
        ..ImportResult::default()
    };

    for file in files {
        let importer = get_importer(&file)?;
        let result = importer
            .import(&file, account_id, db)
            .with_context(|| format!("importing {file:?}"))?;
        db.log_import(&ImportLog {
            filename: result.filename.clone(),
            account_id: account_id.to_string(),
            rows_total: result.rows_total,
            rows_inserted: result.rows_inserted,
            rows_duplicate: result.rows_duplicate,
            source: "csv".to_string(),
            detected_bank: result.detected_bank,
            detection_confidence: result.detection_confidence,
        })?;

        let bank_tag = match result.detected_bank {
            BankFormat::Unknown => format!(
                "unknown bank ({:.0}% confidence)",
                result.detection_confidence * 100.0
            ),
            b => format!(
                "{} ({:.0}%)",
                b.as_str(),
                result.detection_confidence * 100.0
            ),
        };
        println!(
            "{}: {} new, {} duplicates [{}]",
            result.filename, result.rows_inserted, result.rows_duplicate, bank_tag
        );

        totals.rows_total += result.rows_total;
        totals.rows_inserted += result.rows_inserted;
        totals.rows_duplicate += result.rows_duplicate;
    }

    println!(
        "Totals: {} new, {} duplicates across {} rows",
        totals.rows_inserted, totals.rows_duplicate, totals.rows_total
    );
    Ok(())
}

/// Expand a path argument into a flat list of CSV files. A file path
/// returns itself; a directory returns every `*.csv` it contains,
/// non-recursively, sorted for deterministic output.
fn collect_csv_files(path: &Path) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    if !path.is_dir() {
        anyhow::bail!("path does not exist: {path:?}");
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(path)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("csv"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    Ok(files)
}
