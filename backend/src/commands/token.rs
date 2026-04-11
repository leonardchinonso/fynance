//! `fynance token …` — manage API bearer tokens for programmatic
//! access (scripts, external AI agents).

use anyhow::Result;

use crate::storage::Db;

pub fn create(db: &Db, name: &str) -> Result<()> {
    let raw = db.create_token(name)?;
    // Printed once, to stdout only. Callers are expected to capture
    // the output into a secret manager or env var; there is no way to
    // retrieve the raw token again.
    println!("Token: {raw}");
    println!("(Shown once. Store it now — fynance cannot recover it later.)");
    Ok(())
}

pub fn list(db: &Db) -> Result<()> {
    let tokens = db.list_tokens()?;
    if tokens.is_empty() {
        println!("(no api tokens registered)");
        return Ok(());
    }
    for t in tokens {
        let status = if t.is_active { "active" } else { "revoked" };
        let last = t.last_used.unwrap_or_else(|| "never".to_string());
        println!(
            "{name:<24} created {created}  last_used {last}  [{status}]",
            name = t.name,
            created = t.created_at,
        );
    }
    Ok(())
}

pub fn revoke(db: &Db, name: &str) -> Result<()> {
    db.revoke_token(name)?;
    println!("Revoked token {name}");
    Ok(())
}
