//! `fynance account …` — register accounts and set their balances.

use anyhow::{Result, anyhow};
use rust_decimal::Decimal;

use crate::model::{Account, AccountType};
use crate::storage::Db;
use crate::util::parse_naive_datetime;

pub fn add(
    db: &Db,
    id: &str,
    name: &str,
    institution: &str,
    account_type: &str,
    currency: Option<&str>,
    profiles: &[String],
) -> Result<()> {
    let account_type = AccountType::parse(account_type)
        .ok_or_else(|| anyhow!("invalid account type: {account_type}"))?;
    if profiles.is_empty() {
        return Err(anyhow!("at least one --profile is required"));
    }
    for pid in profiles {
        if !db.profile_exists(pid)? {
            return Err(anyhow!(
                "profile {pid} does not exist; create it via the web UI/API first"
            ));
        }
    }
    let is_available = crate::storage::db::is_available_account(&account_type);
    let account = Account {
        id: id.to_string(),
        name: name.to_string(),
        institution: institution.to_string(),
        account_type,
        currency: currency.unwrap_or("GBP").to_string(),
        balance: None,
        balance_date: None,
        is_active: true,
        notes: None,
        profile_ids: profiles.to_vec(),
        is_stale: None,
        is_available,
    };
    db.upsert_account(&account)?;
    println!("Added account {id}");
    Ok(())
}

pub fn set_balance(db: &Db, id: &str, amount: &str, date: &str) -> Result<()> {
    let balance: Decimal = amount
        .parse()
        .map_err(|e| anyhow!("invalid amount {amount:?}: {e}"))?;
    let date = parse_naive_datetime(date)?;
    db.set_account_balance(id, balance, date)?;
    println!("Set {id} balance to {balance} as of {date}");
    Ok(())
}

pub fn list(db: &Db) -> Result<()> {
    let accounts = db.get_accounts(None)?;
    if accounts.is_empty() {
        println!("(no accounts registered)");
        return Ok(());
    }
    for a in accounts {
        let bal = a
            .balance
            .map(|b| format!("{b}"))
            .unwrap_or_else(|| "—".to_string());
        let date = a
            .balance_date
            .map(|d| d.to_string())
            .unwrap_or_else(|| "never".to_string());
        println!(
            "{id} | {name} ({institution}, {kind}) | balance {bal} as of {date}",
            id = a.id,
            name = a.name,
            institution = a.institution,
            kind = a.account_type.as_str(),
        );
    }
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
    fn add_requires_a_profile() {
        let (db, _f) = test_db();
        let err = add(&db, "a1", "Current", "TestBank", "checking", None, &[])
            .unwrap_err()
            .to_string();
        assert!(err.contains("--profile"), "unexpected error: {err}");
    }

    #[test]
    fn add_rejects_unknown_profile() {
        let (db, _f) = test_db();
        let err = add(
            &db,
            "a1",
            "Current",
            "TestBank",
            "checking",
            None,
            &["ghost".to_string()],
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("does not exist"), "unexpected error: {err}");
    }

    #[test]
    fn add_succeeds_with_existing_profile() {
        let (db, _f) = test_db();
        db.create_profile("alice", "Alice").expect("create profile");
        add(
            &db,
            "a1",
            "Current",
            "TestBank",
            "checking",
            None,
            &["alice".to_string()],
        )
        .expect("add account");
        let accounts = db.get_accounts(None).expect("list");
        let acc = accounts
            .iter()
            .find(|a| a.id == "a1")
            .expect("account present");
        assert_eq!(acc.profile_ids, vec!["alice".to_string()]);
    }
}
