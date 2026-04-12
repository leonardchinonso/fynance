//! Live smoke test for `LlmStatementParser`.
//!
//! This test hits the real Anthropic API and is therefore:
//! - Gated on the `FYNANCE_ANTHROPIC_API_KEY` environment variable.
//! - Marked `#[ignore]` so it is excluded from `cargo test` by default.
//!
//! Run manually with:
//!   FYNANCE_ANTHROPIC_API_KEY=... cargo test -- --ignored
//!
//! The test runs the Monzo fixture through the live model and checks that
//! the detected bank and row counts match expectations.

use std::path::PathBuf;
use std::sync::Arc;

use fynance::importers::csv_importer::CsvImporter;
use fynance::importers::llm_parser::LlmStatementParser;
use fynance::importers::Importer;
use fynance::model::BankFormat;
use fynance::storage::Db;
use tempfile::tempdir;

fn fixture(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("fixtures");
    p.push(name);
    p
}

#[test]
#[ignore = "requires FYNANCE_ANTHROPIC_API_KEY; run with: cargo test -- --ignored"]
fn live_monzo_import() {
    // Load .env so the test picks up FYNANCE_ANTHROPIC_API_KEY when running
    // from the project root.
    let _ = dotenvy::dotenv();

    let parser = LlmStatementParser::from_env()
        .expect("FYNANCE_ANTHROPIC_API_KEY must be set for live tests");
    let min_detection_confidence = parser.min_detection_confidence;
    let min_row_confidence = parser.min_row_confidence;

    let importer = CsvImporter {
        parser: Arc::new(parser),
        min_detection_confidence,
        min_row_confidence,
    };

    let dir = tempdir().unwrap();
    let db = Db::open(&dir.path().join("live_test.db")).unwrap();

    let result = importer
        .import(&fixture("monzo.csv"), "monzo-live-test", &db)
        .expect("live import should succeed");

    assert_eq!(
        result.detected_bank,
        BankFormat::Monzo,
        "expected Monzo detection for monzo.csv"
    );
    assert!(
        result.detection_confidence >= 0.80,
        "detection_confidence should be >= 0.80, got {}",
        result.detection_confidence
    );
    assert_eq!(result.rows_inserted, 3, "monzo.csv has 3 data rows");
    assert_eq!(result.rows_duplicate, 0);
}
