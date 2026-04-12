//! `fynance stats` — quick sanity check on what's in the DB.

use anyhow::Result;

use crate::storage::Db;

pub fn run(db: &Db) -> Result<()> {
    let stats = db.stats()?;
    let range = match (stats.min_date.as_deref(), stats.max_date.as_deref()) {
        (Some(a), Some(b)) => format!(" ({a} to {b})"),
        _ => String::new(),
    };
    println!("Total: {} transactions{range}", stats.total);
    if stats.per_account.is_empty() {
        println!("(no accounts registered yet)");
        return Ok(());
    }
    for acct in stats.per_account {
        let range = match (acct.min_date.as_deref(), acct.max_date.as_deref()) {
            (Some(a), Some(b)) => format!(" {a}..{b}"),
            _ => String::new(),
        };
        println!(
            "  {}: {}{} | uncategorized: {}",
            acct.account_id, acct.count, range, acct.uncategorized
        );
    }
    Ok(())
}
