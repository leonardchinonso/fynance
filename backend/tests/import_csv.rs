//! Integration tests for the CSV importer.
//!
//! These tests exercise the full stack (importer → storage → sqlite) using a
//! temp DB path. They use `MockStatementParser` seeded from JSON fixtures so
//! they run without a live Anthropic API key. The CSV fixture files still need
//! to exist on disk because `CsvImporter::import` reads the file before
//! calling the parser.

use std::path::PathBuf;
use std::sync::Arc;

use fynance::importers::csv_importer::CsvImporter;
use fynance::importers::llm_parser::MockStatementParser;
use fynance::importers::Importer;
use fynance::model::BankFormat;
use fynance::storage::Db;
use rust_decimal::Decimal;
use tempfile::tempdir;

fn fixture(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("fixtures");
    p.push(name);
    p
}

fn fixture_json(name: &str) -> String {
    std::fs::read_to_string(fixture(name))
        .unwrap_or_else(|e| panic!("could not read fixture {name}: {e}"))
}

fn open_temp_db() -> (Db, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Db::open(&db_path).unwrap();
    (db, dir)
}

fn make_importer(fixture_json_name: &str) -> CsvImporter {
    let mock = MockStatementParser::from_json(&fixture_json(fixture_json_name)).unwrap();
    CsvImporter {
        parser: Arc::new(mock),
        min_detection_confidence: 0.80,
        min_row_confidence: 0.70,
    }
}

fn total_amount(db: &Db) -> Decimal {
    db.get_transactions(&Default::default())
        .unwrap()
        .into_iter()
        .map(|t| t.amount)
        .sum()
}

#[test]
fn imports_monzo_csv() {
    let (db, _dir) = open_temp_db();
    let result = make_importer("monzo.expected.json")
        .import(&fixture("monzo.csv"), "monzo-current", &db)
        .unwrap();
    assert_eq!(result.rows_total, 3);
    assert_eq!(result.rows_inserted, 3);
    assert_eq!(result.rows_duplicate, 0);
    assert_eq!(result.detected_bank, BankFormat::Monzo);
    // Two debits of -5.50 and -2.80 plus one credit of 2500.00 = 2491.70
    assert_eq!(total_amount(&db), Decimal::new(249170, 2));
}

#[test]
fn imports_revolut_csv() {
    let (db, _dir) = open_temp_db();
    let result = make_importer("revolut.expected.json")
        .import(&fixture("revolut.csv"), "revolut-main", &db)
        .unwrap();
    assert_eq!(result.rows_inserted, 3);
    assert_eq!(result.detected_bank, BankFormat::Revolut);
    assert_eq!(total_amount(&db), Decimal::new(249170, 2));
}

#[test]
fn imports_lloyds_csv_and_flips_debit_sign() {
    let (db, _dir) = open_temp_db();
    let result = make_importer("lloyds.expected.json")
        .import(&fixture("lloyds.csv"), "lloyds-current", &db)
        .unwrap();
    assert_eq!(result.rows_inserted, 3);
    assert_eq!(result.detected_bank, BankFormat::Lloyds);
    let txs = db.get_transactions(&Default::default()).unwrap();
    // Exactly two negative amounts and one positive; the fixture already
    // encodes the correct signs from the LLM (negative = debit convention).
    assert_eq!(txs.iter().filter(|t| t.amount.is_sign_negative()).count(), 2);
    assert_eq!(txs.iter().filter(|t| t.amount.is_sign_positive()).count(), 1);
    assert_eq!(total_amount(&db), Decimal::new(249170, 2));
}

#[test]
fn reimport_is_idempotent() {
    let (db, _dir) = open_temp_db();
    let first = make_importer("monzo.expected.json")
        .import(&fixture("monzo.csv"), "monzo-current", &db)
        .unwrap();
    let second = make_importer("monzo.expected.json")
        .import(&fixture("monzo.csv"), "monzo-current", &db)
        .unwrap();
    assert_eq!(first.rows_inserted, 3);
    assert_eq!(second.rows_inserted, 0);
    assert_eq!(second.rows_duplicate, 3);
    assert_eq!(db.get_transactions(&Default::default()).unwrap().len(), 3);
}

#[test]
fn imports_unknown_bank_csv() {
    let (db, _dir) = open_temp_db();
    let result = make_importer("unknown_bank.expected.json")
        .import(&fixture("unknown_bank.csv"), "other-account", &db)
        .unwrap();
    assert_eq!(result.rows_inserted, 3);
    assert_eq!(result.detected_bank, BankFormat::Unknown);
    // detection_confidence 0.82 is above the 0.80 threshold, so import succeeds.
    assert!(result.detection_confidence >= 0.80);
    assert_eq!(total_amount(&db), Decimal::new(249170, 2));
}

#[test]
fn garbage_file_fails_hard() {
    // The mock returns detection_confidence = 0.4 for a shopping list.
    let mock_json = r#"{
        "detected_bank": "unknown",
        "detection_confidence": 0.4,
        "rows": []
    }"#;
    let mock = MockStatementParser::from_json(mock_json).unwrap();
    let importer = CsvImporter {
        parser: Arc::new(mock),
        min_detection_confidence: 0.80,
        min_row_confidence: 0.70,
    };
    let (db, _dir) = open_temp_db();
    let err = importer
        .import(&fixture("garbage.csv"), "any-account", &db)
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("0.40") || msg.contains("confidence"),
        "error message should mention confidence: {msg}"
    );
}

#[test]
fn low_confidence_rows_are_skipped() {
    // Two good rows and one row below the threshold.
    let mock_json = r#"{
        "detected_bank": "unknown",
        "detection_confidence": 0.85,
        "rows": [
            {
                "date": "2026-03-10",
                "description": "Good row A",
                "amount": "-10.00",
                "currency": "GBP",
                "fitid": null, "category": null, "merchant": null,
                "counterparty": null, "transaction_type": null,
                "balance_after": null, "notes": null, "reference": null,
                "row_confidence": 0.95
            },
            {
                "date": "2026-03-11",
                "description": "Low confidence row",
                "amount": "-99.00",
                "currency": "GBP",
                "fitid": null, "category": null, "merchant": null,
                "counterparty": null, "transaction_type": null,
                "balance_after": null, "notes": null, "reference": null,
                "row_confidence": 0.50
            },
            {
                "date": "2026-03-12",
                "description": "Good row B",
                "amount": "500.00",
                "currency": "GBP",
                "fitid": null, "category": null, "merchant": null,
                "counterparty": null, "transaction_type": null,
                "balance_after": null, "notes": null, "reference": null,
                "row_confidence": 0.90
            }
        ]
    }"#;
    let mock = MockStatementParser::from_json(mock_json).unwrap();
    let importer = CsvImporter {
        parser: Arc::new(mock),
        min_detection_confidence: 0.80,
        min_row_confidence: 0.70,
    };
    let (db, _dir) = open_temp_db();
    let result = importer
        .import(&fixture("garbage.csv"), "test-account", &db)
        .unwrap();
    // 3 total but 1 skipped.
    assert_eq!(result.rows_total, 3);
    assert_eq!(result.rows_inserted, 2);
    assert_eq!(result.rows_duplicate, 0);
}
