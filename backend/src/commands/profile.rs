//! `fynance profile …` — manage the profiles that accounts belong to.

use anyhow::{Result, anyhow};

use crate::storage::Db;

/// Profile ids mirror the API contract: non-empty, lowercase alphanumeric
/// plus hyphens, no whitespace. Kept in sync with
/// `server::validation::validate_profile_id`.
fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.contains(char::is_whitespace)
        || !id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(anyhow!(
            "invalid profile id {id:?}: must be non-empty lowercase alphanumeric + hyphens"
        ));
    }
    Ok(())
}

pub fn add(db: &Db, id: &str, name: &str) -> Result<()> {
    validate_id(id)?;
    if db.profile_exists(id)? {
        return Err(anyhow!("profile {id} already exists"));
    }
    db.create_profile(id, name)?;
    println!("Added profile {id}");
    Ok(())
}

pub fn list(db: &Db) -> Result<()> {
    let profiles = db.get_profiles()?;
    if profiles.is_empty() {
        println!("(no profiles registered)");
        return Ok(());
    }
    for p in profiles {
        println!("{id} | {name}", id = p.id, name = p.name);
    }
    Ok(())
}

/// Delete a profile. Refuses if any account still references it.
pub fn delete(db: &Db, id: &str) -> Result<()> {
    if !db.profile_exists(id)? {
        return Err(anyhow!("profile {id} not found"));
    }
    let referencing = db.count_accounts_referencing_profile(id)?;
    if referencing > 0 {
        return Err(anyhow!(
            "{referencing} account(s) still reference profile {id}; remove them from those accounts first"
        ));
    }
    db.delete_profile(id)?;
    println!("Deleted profile {id}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn test_db() -> (Db, NamedTempFile) {
        let file = NamedTempFile::new().expect("temp file");
        let db = Db::open(file.path()).expect("test db");
        (db, file)
    }

    #[test]
    fn add_rejects_invalid_id() {
        let (db, _f) = test_db();
        let err = add(&db, "Bad ID", "x").unwrap_err().to_string();
        assert!(err.contains("invalid profile id"), "unexpected: {err}");
    }

    #[test]
    fn add_then_duplicate_then_delete() {
        let (db, _f) = test_db();
        add(&db, "alice", "Alice").expect("add");
        assert!(db.profile_exists("alice").expect("exists"));
        let err = add(&db, "alice", "Alice").unwrap_err().to_string();
        assert!(err.contains("already exists"), "unexpected: {err}");
        delete(&db, "alice").expect("delete");
        assert!(!db.profile_exists("alice").expect("exists"));
    }

    #[test]
    fn delete_refuses_when_referenced() {
        let (db, _f) = test_db();
        add(&db, "alice", "Alice").expect("add profile");
        crate::commands::account::add(
            &db,
            "a1",
            "Current",
            "Bank",
            "checking",
            None,
            &["alice".to_string()],
        )
        .expect("add account");
        let err = delete(&db, "alice").unwrap_err().to_string();
        assert!(err.contains("still reference"), "unexpected: {err}");
    }

    #[test]
    fn delete_unknown_fails() {
        let (db, _f) = test_db();
        let err = delete(&db, "ghost").unwrap_err().to_string();
        assert!(err.contains("not found"), "unexpected: {err}");
    }
}
