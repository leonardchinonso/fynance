//! `fynance budget ...`: set per-month budget overrides and inspect the
//! effective budget (standing targets merged with overrides, plus actual
//! spend). Writes through the same storage methods as the REST API.

use anyhow::{Result, anyhow};
use rust_decimal::Decimal;

use crate::storage::Db;
use crate::util::fx::FxRateMap;
use crate::util::parse_month;

pub fn set(db: &Db, month: &str, category: &str, amount: &str) -> Result<()> {
    let month = parse_month(month)?;
    let amount: Decimal = amount
        .parse()
        .map_err(|e| anyhow!("invalid amount {amount:?}: {e}"))?;
    let cat = db.resolve_category_by_name(category)?.ok_or_else(|| {
        anyhow!(
            "unknown category {category:?}; pass an existing category name, either \"Parent: Child\" or the bare leaf name (see `GET /api/categories`)"
        )
    })?;
    db.set_budget_override(&month, &cat.id, amount)?;
    println!("Set budget {} = {amount} for {month}", cat.name);
    Ok(())
}

pub fn status(db: &Db, month: &str) -> Result<()> {
    let month = parse_month(month)?;
    let fx = FxRateMap::new(db.get_currencies()?)?;
    let rows = db.get_effective_budget(&month, &fx)?;
    if rows.is_empty() {
        println!("(no budgets set for {month})");
        return Ok(());
    }
    println!("Budgets for {month} (amounts in {}):", fx.preferred());
    for row in rows {
        let name = match row.category_id.as_deref() {
            Some(id) => category_display_name(db, id)?,
            None => "(uncategorized)".to_string(),
        };
        let budgeted = row.budgeted.as_deref().unwrap_or("-").to_string();
        let percent = row
            .percent
            .map(|p| format!("  ({p:.0}%)"))
            .unwrap_or_default();
        println!(
            "  {name:<32} budget {budgeted:>10}  spent {:>10}{percent}",
            row.actual
        );
    }
    Ok(())
}

/// "Parent: Child" for leaves with a parent, the bare name otherwise.
fn category_display_name(db: &Db, id: &str) -> Result<String> {
    let Some(cat) = db.get_category_by_id(id)? else {
        return Ok(id.to_string());
    };
    if let Some(parent_id) = &cat.parent_id {
        if let Some(parent) = db.get_category_by_id(parent_id)? {
            return Ok(format!("{}: {}", parent.name, cat.name));
        }
    }
    Ok(cat.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CategoryType, CreateCategoryPayload};
    use tempfile::NamedTempFile;

    fn test_db() -> (Db, NamedTempFile) {
        let file = NamedTempFile::new().expect("temp file");
        let db = Db::open(file.path()).expect("test db");
        (db, file)
    }

    fn create_category(db: &Db, name: &str, parent_id: Option<&str>) -> String {
        db.create_category(&CreateCategoryPayload {
            name: name.to_string(),
            parent_id: parent_id.map(str::to_string),
            display_order: None,
            description: None,
            category_type: CategoryType::Spending,
        })
        .expect("create category")
        .id
    }

    fn effective_budget_for(db: &Db, month: &str, category_id: &str) -> Option<String> {
        let fx = FxRateMap::new(db.get_currencies().unwrap()).unwrap();
        db.get_effective_budget(month, &fx)
            .unwrap()
            .into_iter()
            .find(|r| r.category_id.as_deref() == Some(category_id))
            .and_then(|r| r.budgeted)
    }

    #[test]
    fn set_resolves_leaf_name_and_writes_override() {
        let (db, _f) = test_db();
        let parent_id = create_category(&db, "FoodTest", None);
        let leaf_id = create_category(&db, "GroceriesTest", Some(&parent_id));
        db.set_standing_budget(&leaf_id, Decimal::from(100))
            .expect("standing budget");

        set(&db, "2026-03", "GroceriesTest", "400").expect("budget set");
        assert_eq!(
            effective_budget_for(&db, "2026-03", &leaf_id).as_deref(),
            Some("400"),
            "override must win for its month"
        );
        assert_eq!(
            effective_budget_for(&db, "2026-04", &leaf_id).as_deref(),
            Some("100"),
            "other months must fall back to the standing budget"
        );
    }

    #[test]
    fn set_resolves_parent_child_name() {
        let (db, _f) = test_db();
        let parent_id = create_category(&db, "FoodTest", None);
        let leaf_id = create_category(&db, "GroceriesTest", Some(&parent_id));
        db.set_standing_budget(&leaf_id, Decimal::from(100))
            .expect("standing budget");

        set(&db, "2026-03", "FoodTest: GroceriesTest", "250").expect("budget set");
        assert_eq!(
            effective_budget_for(&db, "2026-03", &leaf_id).as_deref(),
            Some("250")
        );
    }

    #[test]
    fn set_rejects_unknown_category() {
        let (db, _f) = test_db();
        let err = set(&db, "2026-03", "NoSuchCategory", "400")
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown category"), "unexpected error: {err}");
    }

    #[test]
    fn set_rejects_invalid_amount() {
        let (db, _f) = test_db();
        let err = set(&db, "2026-03", "GroceriesTest", "abc")
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid amount"), "unexpected error: {err}");
    }
}
