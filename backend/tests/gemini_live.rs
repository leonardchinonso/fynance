//! Live smoke test for `GeminiProvider`.
//!
//! This test hits the real Google Gemini API and is therefore:
//! - Gated on the `FYNANCE_GEMINI_API_KEY` (or `GEMINI_API_KEY`) environment variable.
//! - Marked `#[ignore]` so it is excluded from `cargo test` by default.
//!
//! Run manually with:
//!   FYNANCE_GEMINI_API_KEY=... cargo test --test gemini_live -- --ignored --nocapture

use std::path::PathBuf;
use std::sync::Arc;

use fynance::importers::Importer;
use fynance::importers::csv_importer::CsvImporter;
use fynance::importers::llm_parser::LlmStatementParser;
use fynance::importers::provider::GeminiProvider;
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
#[ignore = "requires FYNANCE_GEMINI_API_KEY or GEMINI_API_KEY; run with: cargo test --test gemini_live -- --ignored --nocapture"]
fn live_gemini_monzo_import() {
    let _ = dotenvy::dotenv();

    let provider = Arc::new(GeminiProvider::from_env().expect("Gemini API key must be set for live tests"));
    let parser = LlmStatementParser::new(provider);
    let min_detection_confidence = parser.min_detection_confidence;
    let min_row_confidence = parser.min_row_confidence;

    let importer = CsvImporter {
        parser: Arc::new(parser),
        min_detection_confidence,
        min_row_confidence,
    };

    let dir = tempdir().unwrap();
    let db = Db::open(&dir.path().join("live_gemini_test.db")).unwrap();

    let result = importer
        .import(&fixture("monzo.csv"), "gemini-live-test", &db)
        .expect("Gemini live import should succeed");

    println!("Detected bank: {:?}", result.detected_bank);
    println!("Confidence: {}", result.detection_confidence);
    println!("Rows inserted: {}", result.rows_inserted);

    assert_eq!(
        result.detected_bank,
        BankFormat::Monzo,
        "expected Monzo detection for monzo.csv"
    );
    assert!(
        result.detection_confidence >= 0.75,
        "detection_confidence should be >= 0.75, got {}",
        result.detection_confidence
    );
    assert_eq!(result.rows_inserted, 3, "monzo.csv has 3 data rows");
}

#[tokio::test]
#[ignore = "requires FYNANCE_GEMINI_API_KEY or GEMINI_API_KEY; run with: cargo test --test gemini_live -- --ignored --nocapture"]
async fn live_gemini_pdf_statement_parse() {
    let _ = dotenvy::dotenv();

    let provider = Arc::new(GeminiProvider::from_env().expect("Gemini API key must be set for live tests"));
    let parser = fynance::importers::pdf_parser::PdfStatementParser::new(provider);

    let pdf_bytes = std::fs::read(fixture("sample_statement.pdf")).expect("fixture sample_statement.pdf must exist");
    let (statement, call_result) = parser
        .parse(&pdf_bytes, "sample_statement.pdf", None, None)
        .await
        .expect("Gemini PDF parsing should succeed");

    println!("PDF detected bank: {:?}", statement.detected_bank);
    println!("PDF parsed rows: {}", statement.rows.len());
    println!("PDF model used: {}", call_result.model);
    println!("PDF duration ms: {}", call_result.duration_ms);

    assert!(!statement.rows.is_empty(), "expected at least 1 transaction from sample_statement.pdf");
}
