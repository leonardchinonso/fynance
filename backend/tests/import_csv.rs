//! Integration tests for the CSV importer. These exercise the full stack:
//! importer → storage → sqlite, using a temp DB path.

use std::path::PathBuf;

use fynance::importers::get_importer;
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

fn open_temp_db() -> (Db, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Db::open(&db_path).unwrap();
    (db, dir)
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
    let result = get_importer(&fixture("monzo.csv"))
        .unwrap()
        .import(&fixture("monzo.csv"), "monzo-current", &db)
        .unwrap();
    assert_eq!(result.rows_total, 3);
    assert_eq!(result.rows_inserted, 3);
    assert_eq!(result.rows_duplicate, 0);
    // Two debits of -5.50 and -2.80 plus one credit of 2500.00 = 2491.70
    assert_eq!(total_amount(&db), Decimal::new(249170, 2));
}

#[test]
fn imports_revolut_csv() {
    let (db, _dir) = open_temp_db();
    let result = get_importer(&fixture("revolut.csv"))
        .unwrap()
        .import(&fixture("revolut.csv"), "revolut-main", &db)
        .unwrap();
    assert_eq!(result.rows_inserted, 3);
    assert_eq!(total_amount(&db), Decimal::new(249170, 2));
}

#[test]
fn imports_lloyds_csv_and_flips_debit_sign() {
    let (db, _dir) = open_temp_db();
    let result = get_importer(&fixture("lloyds.csv"))
        .unwrap()
        .import(&fixture("lloyds.csv"), "lloyds-current", &db)
        .unwrap();
    assert_eq!(result.rows_inserted, 3);
    let txs = db.get_transactions(&Default::default()).unwrap();
    // Exactly two negative amounts and one positive, confirming Lloyds
    // debits were flipped to negative.
    assert_eq!(
        txs.iter().filter(|t| t.amount.is_sign_negative()).count(),
        2
    );
    assert_eq!(
        txs.iter().filter(|t| t.amount.is_sign_positive()).count(),
        1
    );
    assert_eq!(total_amount(&db), Decimal::new(249170, 2));
}

#[test]
fn reimport_is_idempotent() {
    let (db, _dir) = open_temp_db();
    let first = get_importer(&fixture("monzo.csv"))
        .unwrap()
        .import(&fixture("monzo.csv"), "monzo-current", &db)
        .unwrap();
    let second = get_importer(&fixture("monzo.csv"))
        .unwrap()
        .import(&fixture("monzo.csv"), "monzo-current", &db)
        .unwrap();
    assert_eq!(first.rows_inserted, 3);
    assert_eq!(second.rows_inserted, 0);
    assert_eq!(second.rows_duplicate, 3);
    // Transaction count didn't double.
    assert_eq!(db.get_transactions(&Default::default()).unwrap().len(), 3);
}
