# Budgeting System

## Budget Generation via Claude (`src/budget/advisor.rs`)

```rust
use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
pub struct BudgetItem {
    pub category: String,
    pub amount: f64,
    #[serde(rename = "type")]
    pub kind: String,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GeneratedBudget {
    pub budget: Vec<BudgetItem>,
    pub recommendations: Vec<String>,
    pub total_income: f64,
    pub total_budgeted: f64,
}

pub async fn generate_initial_budget(
    client: &Client,
    api_key: &str,
    income: f64,
    month: &str,
    history: &[(String, f64)], // (category, 6-month avg)
) -> Result<GeneratedBudget> {
    let history_str = history.iter()
        .map(|(cat, avg)| format!("- {}: ${:.2}/month (6-month avg)", cat, avg))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(r#"Create a zero-based monthly budget for {month}.

Monthly after-tax income: ${income:.2}

My 6-month spending averages:
{history_str}

Requirements:
1. Every dollar assigned (total = ${income:.2} exactly)
2. Keep fixed expenses at current levels
3. Savings >= 15% of income (${:.2})
4. Suggest 10-15% reduction in high discretionary categories
5. Add small buffer in "Other" for unexpected expenses

Return ONLY valid JSON:
{{
  "budget": [{{"category":"...","amount":0.00,"type":"fixed|savings|needs|discretionary","note":"optional"}}],
  "recommendations": ["...", "..."],
  "total_income": {income},
  "total_budgeted": {income}
}}"#, income * 0.15);

    let body = json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 2048,
        "messages": [{ "role": "user", "content": prompt }]
    });

    let resp: serde_json::Value = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send().await?
        .error_for_status()?
        .json().await?;

    let text = resp["content"][0]["text"].as_str().unwrap_or("{}");
    let budget: GeneratedBudget = serde_json::from_str(text)?;
    Ok(budget)
}

pub async fn monthly_insights(
    client: &Client,
    api_key: &str,
    month: &str,
    variance_report: &str,
) -> Result<String> {
    let prompt = format!(r#"Analyze my {month} spending vs budget.

Budget vs Actual:
{variance_report}

Provide:
1. **Overall assessment** (1-2 sentences, direct and honest)
2. **Top wins** (categories under budget)
3. **Areas to watch** (over budget, with specific suggestions)
4. **One key action** for next month

Be direct. Format as markdown. Under 300 words."#);

    let body = json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 800,
        "messages": [{ "role": "user", "content": prompt }]
    });

    let resp: serde_json::Value = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send().await?
        .error_for_status()?
        .json().await?;

    Ok(resp["content"][0]["text"].as_str().unwrap_or("").to_string())
}
```

## Projection (`src/budget/analyzer.rs`)

```rust
use anyhow::Result;
use chrono::Datelike;
use rusqlite::Connection;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

/// 3-month rolling average per category, +10% buffer for discretionary.
pub fn project_next_month(
    conn: &Connection,
    fixed_categories: &[&str],
) -> Result<HashMap<String, Decimal>> {
    let mut stmt = conn.prepare(r#"
        SELECT category, AVG(CAST(amount AS REAL)) * -1 as avg
        FROM (
            SELECT category, strftime('%Y-%m', date) as m,
                   SUM(CAST(amount AS REAL)) as amount
            FROM transactions
            WHERE CAST(amount AS REAL) < 0
              AND date >= date('now', '-3 months')
              AND category NOT LIKE 'Finance: Internal%'
            GROUP BY category, m
        )
        GROUP BY category
    "#)?;

    let mut projections = HashMap::new();
    let rows = stmt.query_map([], |row| {
        let cat: String = row.get(0)?;
        let avg: f64   = row.get(1)?;
        Ok((cat, avg))
    })?;

    for row in rows {
        let (cat, avg) = row?;
        let multiplier = if fixed_categories.contains(&cat.as_str()) { 1.0 } else { 1.10 };
        let projected = Decimal::from_str(&format!("{:.2}", avg * multiplier))?;
        projections.insert(cat, projected);
    }

    Ok(projections)
}

/// Check spending against the 50/30/20 rule for a quick health summary.
pub fn check_50_30_20(
    conn: &Connection,
    year_month: &str,
    income: Decimal,
) -> Result<BudgetHealth> {
    let mut stmt = conn.prepare(r#"
        SELECT category, SUM(CAST(amount AS REAL)) * -1 as total
        FROM transactions
        WHERE strftime('%Y-%m', date) = ?
          AND CAST(amount AS REAL) < 0
        GROUP BY category
    "#)?;

    let mut needs  = Decimal::ZERO;
    let mut wants  = Decimal::ZERO;

    const NEEDS: &[&str] = &["Housing", "Food: Groceries", "Transport: Gas", "Health", "Finance: Loan"];
    const WANTS: &[&str] = &["Food: Dining", "Food: Coffee", "Life:", "Shopping:", "Digital:"];

    let rows = stmt.query_map([year_month], |row| {
        let cat: String = row.get(0)?;
        let total: f64  = row.get(1)?;
        Ok((cat, total))
    })?;

    for row in rows {
        let (cat, total) = row?;
        let d = Decimal::from_str(&format!("{:.2}", total))?;
        if NEEDS.iter().any(|p| cat.starts_with(p)) {
            needs += d;
        } else if WANTS.iter().any(|p| cat.starts_with(p)) {
            wants += d;
        }
    }

    let savings = income - needs - wants;
    let inc_f: f64 = income.to_string().parse().unwrap_or(1.0);

    Ok(BudgetHealth {
        needs_pct:   (needs.to_string().parse::<f64>().unwrap_or(0.0) / inc_f * 100.0),
        wants_pct:   (wants.to_string().parse::<f64>().unwrap_or(0.0) / inc_f * 100.0),
        savings_pct: (savings.to_string().parse::<f64>().unwrap_or(0.0) / inc_f * 100.0),
    })
}

pub struct BudgetHealth {
    pub needs_pct: f64,
    pub wants_pct: f64,
    pub savings_pct: f64,
}

impl BudgetHealth {
    pub fn status(&self) -> &'static str {
        if self.savings_pct >= 20.0 { "Great" }
        else if self.savings_pct >= 10.0 { "OK" }
        else { "Low savings" }
    }
}
```

## Budget Commands (`src/commands/budget.rs`)

```rust
use crate::budget::{advisor, analyzer};
use crate::storage::db::Db;
use anyhow::Result;

/// Generate an initial zero-based budget for the given month.
pub async fn init(
    income: f64,
    month: &str,
    db: &Db,
    http: &reqwest::Client,
    api_key: &str,
) -> Result<()> {
    let history = db.category_averages_6mo()?;
    let generated = advisor::generate_initial_budget(http, api_key, income, month, &history).await?;

    for item in &generated.budget {
        db.set_budget(month, &item.category, item.amount)?;
    }

    println!("Budget saved for {}:", month);
    for item in &generated.budget {
        println!("  {:<35} ${:.2}", item.category, item.amount);
    }
    println!("\nRecommendations:");
    for rec in &generated.recommendations {
        println!("  - {}", rec);
    }
    Ok(())
}

/// Print current month's budget vs actual variance table.
pub fn status(db: &Db) -> Result<()> {
    let month = chrono::Local::now().format("%Y-%m").to_string();
    let rows = db.budget_vs_actual(&month)?;

    println!("{:<35} {:>10} {:>10} {:>10} {}", "Category", "Budget", "Spent", "Delta", "Status");
    println!("{}", "-".repeat(75));

    for row in &rows {
        let status = if row.spent > row.budget { "OVER" }
            else if row.spent > row.budget * 0.9 { "WATCH" }
            else { "OK" };
        println!("{:<35} {:>10.2} {:>10.2} {:>+10.2} {}",
            row.category, row.budget, row.spent, row.budget - row.spent, status);
    }
    Ok(())
}
```

## Variance SQL Query

The `budget_vs_actual` database method runs:

```sql
SELECT
    b.category,
    CAST(b.amount AS REAL) as budget,
    COALESCE(SUM(CAST(t.amount AS REAL)) * -1, 0) as spent
FROM budgets b
LEFT JOIN transactions t
    ON t.category = b.category
    AND strftime('%Y-%m', t.date) = b.year_month
    AND CAST(t.amount AS REAL) < 0
WHERE b.year_month = ?
GROUP BY b.category
ORDER BY (CAST(b.amount AS REAL) - COALESCE(SUM(CAST(t.amount AS REAL)) * -1, 0)) ASC;
```
