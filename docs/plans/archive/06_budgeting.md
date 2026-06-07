# Budgeting System

> **Updated after Prompt 1.1.** No longer deferred. The Budget tab is a core MVP view (Phase 3 of `08_mvp_phases_v2.md`).

## Scope

1. Set monthly income per month
2. Set budget amounts per category per month
3. Show actual spending against budget per category
4. Visualize monthly spending trends per category
5. Flag over-budget categories
6. Suggest next month's budget based on historical averages

Advanced features (rolling budgets, envelope budgeting, 50/30/20 splits) are deferred until the manual workflow is validated.

## Data Model

See `02_data_model.md` for the full schema. Relevant tables:

```sql
CREATE TABLE budgets (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    month    TEXT NOT NULL,          -- YYYY-MM
    category TEXT NOT NULL,
    amount   TEXT NOT NULL,          -- Decimal as string
    UNIQUE(month, category) ON CONFLICT REPLACE
);

CREATE TABLE monthly_income (
    month  TEXT PRIMARY KEY,         -- YYYY-MM
    amount TEXT NOT NULL,
    notes  TEXT
);
```

## Core Queries

### Budget vs Actual for a Month

```sql
SELECT
    COALESCE(b.category, t.category) AS category,
    CAST(COALESCE(b.amount, '0') AS REAL) AS budgeted,
    COALESCE(SUM(ABS(CAST(t.amount AS REAL))), 0) AS actual,
    ROUND(
      CAST(COALESCE(b.amount, '0') AS REAL) - COALESCE(SUM(ABS(CAST(t.amount AS REAL))), 0),
      2
    ) AS remaining
FROM budgets b
FULL OUTER JOIN transactions t
    ON t.category = b.category
    AND substr(t.date, 1, 7) = b.month
    AND CAST(t.amount AS REAL) < 0
    AND t.category NOT IN ('Finance: Savings Transfer', 'Finance: Investment Transfer')
WHERE b.month = ?1 OR substr(t.date, 1, 7) = ?1
GROUP BY COALESCE(b.category, t.category)
ORDER BY remaining ASC;
```

SQLite does not support `FULL OUTER JOIN` natively before 3.39. For older builds, the query is rewritten as a `UNION` of two `LEFT JOIN`s, or the Rust handler merges two separate queries in memory.

### Six-Month Average per Category

```sql
SELECT
    category,
    ROUND(AVG(monthly_spend), 2) AS avg_spend
FROM (
    SELECT
        substr(date, 1, 7) AS month,
        category,
        SUM(ABS(CAST(amount AS REAL))) AS monthly_spend
    FROM transactions
    WHERE date >= date('now', '-6 months')
      AND CAST(amount AS REAL) < 0
      AND category IS NOT NULL
    GROUP BY month, category
)
GROUP BY category
ORDER BY avg_spend DESC;
```

Used for suggesting budget amounts when the user initializes a new month's budget.

## Budget Engine (`src/budget/analyzer.rs`)

```rust
use anyhow::Result;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::storage::db::Db;

#[derive(Debug, Clone, Serialize)]
pub struct CategoryStatus {
    pub category: String,
    pub budgeted: Decimal,
    pub actual: Decimal,
    pub remaining: Decimal,
    pub percent_used: f64,
    pub state: BudgetState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetState {
    UnderHalf,      // 0-50%
    OnTrack,        // 50-80%
    Warning,        // 80-100%
    Over,           // >100%
}

pub fn status(db: &Db, month: &str) -> Result<Vec<CategoryStatus>> {
    let rows = db.budget_vs_actual(month)?;
    Ok(rows.into_iter().map(|(category, budgeted, actual)| {
        let percent = if budgeted.is_zero() {
            0.0
        } else {
            (actual / budgeted * Decimal::from(100)).to_string().parse().unwrap_or(0.0)
        };
        let state = match percent {
            p if p >= 100.0 => BudgetState::Over,
            p if p >=  80.0 => BudgetState::Warning,
            p if p >=  50.0 => BudgetState::OnTrack,
            _               => BudgetState::UnderHalf,
        };
        CategoryStatus {
            category,
            budgeted,
            actual,
            remaining: budgeted - actual,
            percent_used: percent,
            state,
        }
    }).collect())
}

pub fn suggest_next_month(db: &Db, target_month: &str) -> Result<Vec<(String, Decimal)>> {
    let averages = db.category_averages_6mo()?;
    let rounded = averages.into_iter()
        .map(|(cat, avg)| (cat, round_to_nearest(avg, Decimal::from(5))))
        .collect();
    Ok(rounded)
}

fn round_to_nearest(value: Decimal, unit: Decimal) -> Decimal {
    ((value / unit).round()) * unit
}
```

## Budget Advisor (`src/budget/advisor.rs`)

Optional Claude-powered monthly summary that highlights anomalies and suggests adjustments.

```rust
pub async fn monthly_insights(
    db: &Db,
    claude: &crate::categorizer::claude::ClaudeCategorizer,
    month: &str,
) -> anyhow::Result<String> {
    let status = crate::budget::analyzer::status(db, month)?;
    let payload = serde_json::to_string(&status)?;

    // Send only aggregates (category, budgeted, actual) to Claude.
    // No individual transactions, no merchant names.
    let prompt = format!(
        "Given these monthly budget results, write a 3-paragraph summary: \
         (1) what went well, (2) what went over, (3) suggestions for next month. \
         Use GBP. Data: {}",
        payload
    );

    claude.complete_text(&prompt).await
}
```

Only category-level aggregates (category name, budgeted, actual) are sent to Claude, never individual transactions. This is consistent with the data minimization policy in `../design/05_security_isolation.md`.

## CLI Commands

```bash
fynance budget set --month 2026-05 --category "Food: Groceries" --amount 300
fynance budget status                              # current month by default
fynance budget status --month 2026-04
fynance budget suggest --month 2026-05             # print suggested amounts
fynance budget init --income 4500 --month 2026-05  # set income + apply suggestions
```

## API Endpoints

```
GET    /api/budget/:month               → { income, categories: [...], totals }
POST   /api/budget                      → set one (month, category, amount)
DELETE /api/budget/:month/:category     → remove a budget row

GET    /api/monthly-income/:month       → { month, amount, notes }
POST   /api/monthly-income              → set income for a month

GET    /api/budget/:month/suggest       → suggested amounts from 6mo averages
POST   /api/budget/:month/init          → seed all categories with suggestions
```

## UI (Budget Tab)

See `../design/02_architecture.md` and `08_mvp_phases_v2.md` Phase 3 for the React page layout.

Key elements:

1. **Month picker** at the top, defaults to current month
2. **Income card** showing budgeted vs actual income
3. **Category rows** with:
   - Progress bar (color-coded by state)
   - Amounts: `£278 / £300`
   - Click to edit budget amount inline
4. **Totals row** at the bottom
5. **Spending trend chart** — stacked area or bar chart showing spend per category over last 6 months (below the table)
6. **"Suggest amounts" button** — calls `/api/budget/:month/suggest` and previews before applying

## Categories Excluded from Budgets

These categories never count toward monthly spending totals or budgets:

- `Finance: Savings Transfer`
- `Finance: Investment Transfer`

They represent money moving between the user's own accounts, not spending. The Transactions view still shows them, but they are filtered out of budget aggregates and spending charts.
