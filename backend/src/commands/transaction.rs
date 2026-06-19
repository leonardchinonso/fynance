//! `fynance transaction …` — manage stored transactions.

use anyhow::{Result, anyhow};

use crate::storage::Db;

/// Hard-delete transactions by id, or every transaction for an account.
/// Exactly one of `ids` / `account` must be supplied.
pub fn delete(db: &Db, ids: &[String], account: Option<&str>) -> Result<()> {
    let deleted = match (ids.is_empty(), account) {
        (false, None) => db.delete_transactions(ids)?,
        (true, Some(account_id)) => db.delete_transactions_for_account(account_id)?,
        (false, Some(_)) => {
            return Err(anyhow!(
                "provide either transaction id(s) or --account <id>, not both"
            ));
        }
        (true, None) => {
            return Err(anyhow!("provide transaction id(s) or --account <id>"));
        }
    };
    println!("Deleted {deleted} transaction(s)");
    Ok(())
}
