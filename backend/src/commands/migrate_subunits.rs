//! `fynance migrate-subunits [--apply]` — one-time data migration converting
//! rows stored in a broker sub-unit currency (GBX, USX, ZAC, ILA) to their
//! parent currency. See `Db::migrate_subunit_currencies` for the full
//! contract: idempotent, and re-runnable after a partial failure (each table
//! is migrated in its own transaction, so it is not atomic across tables).
//!
//! Defaults to a dry run: it prints exactly what would change and writes
//! nothing. Pass `--apply` to actually write the changes.

use anyhow::Result;

use crate::storage::Db;

pub fn run(db: &Db, apply: bool) -> Result<()> {
    let report = db.migrate_subunit_currencies(!apply)?;

    if report.rows.is_empty() {
        println!("No sub-unit rows found. Nothing to migrate.");
        if !report.currencies_removed.is_empty() {
            println!(
                "Removed unreferenced sub-unit currency rows: {}",
                report.currencies_removed.join(", ")
            );
        }
        return Ok(());
    }

    let mode = if apply { "APPLYING" } else { "DRY RUN" };
    println!("{mode} — sub-unit currency migration");
    println!(
        "  investments: {}  holdings: {}  transactions: {}  accounts: {}",
        report.investments_migrated(),
        report.holdings_migrated(),
        report.transactions_migrated(),
        report.accounts_migrated(),
    );
    println!();

    for row in &report.rows {
        println!(
            "[{}] {} ({} -> {})",
            row.table, row.id, row.sub_unit_code, row.parent_code
        );
        println!("    before: {}", row.before);
        println!("    after:  {}", row.after);
    }

    if !report.currencies_removed.is_empty() {
        println!();
        println!(
            "Removed unreferenced sub-unit currency rows: {}",
            report.currencies_removed.join(", ")
        );
    }

    println!();
    if apply {
        println!("Applied. {} row(s) converted.", report.rows.len());
    } else {
        println!(
            "Dry run only — nothing was written. Re-run with --apply to commit these {} change(s).",
            report.rows.len()
        );
    }

    Ok(())
}
