//! `fynance account …` — register accounts and set their balances.

use anyhow::{Result, anyhow};
use rust_decimal::Decimal;

use crate::model::{Account, AccountType};
use crate::storage::Db;
use crate::util::parse_date;

pub fn add(
    db: &Db,
    id: &str,
    name: &str,
    institution: &str,
    account_type: &str,
    currency: Option<&str>,
    balance: Option<&str>,
) -> Result<()> {
    let account_type = AccountType::parse(account_type)
        .ok_or_else(|| anyhow!("invalid account type: {account_type}"))?;
    let balance: Option<Decimal> = match balance {
        Some(raw) => Some(
            raw.parse()
                .map_err(|e| anyhow!("invalid balance {raw:?}: {e}"))?,
        ),
        None => None,
    };
    let account = Account {
        id: id.to_string(),
        name: name.to_string(),
        institution: institution.to_string(),
        account_type,
        currency: currency.unwrap_or("GBP").to_string(),
        balance,
        balance_date: None,
        is_active: true,
        notes: None,
        profile_ids: vec!["default".to_string()],
    };
    db.upsert_account(&account)?;
    println!("Added account {id}");
    Ok(())
}

pub fn set_balance(db: &Db, id: &str, amount: &str, date: &str) -> Result<()> {
    let balance: Decimal = amount
        .parse()
        .map_err(|e| anyhow!("invalid amount {amount:?}: {e}"))?;
    let date = parse_date(date)?;
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
