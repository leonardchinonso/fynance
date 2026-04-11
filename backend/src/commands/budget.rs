//! `fynance budget …` — Phase 1 stub. Stores and lists monthly budget
//! targets. Phase 3 will add the spent-vs-budgeted join and the full
//! status view.

use anyhow::{Result, anyhow};
use rust_decimal::Decimal;

use crate::storage::Db;
use crate::util::parse_month;

pub fn set(db: &Db, month: &str, category: &str, amount: &str) -> Result<()> {
    let month = parse_month(month)?;
    let amount: Decimal = amount
        .parse()
        .map_err(|e| anyhow!("invalid amount {amount:?}: {e}"))?;
    db.set_budget(&month, category, amount)?;
    println!("Set budget {category} = {amount} for {month}");
    Ok(())
}

pub fn status(db: &Db, month: &str) -> Result<()> {
    let month = parse_month(month)?;
    let rows = db.get_budgets_for_month(&month)?;
    if rows.is_empty() {
        println!("(no budgets set for {month})");
        return Ok(());
    }
    println!("Budgets for {month}:");
    for b in rows {
        println!("  {:<30} {}", b.category, b.amount);
    }
    Ok(())
}
