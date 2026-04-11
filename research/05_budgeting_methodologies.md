# Budgeting Methodologies

## Comparison

| Method | Complexity | Control | Best For |
|---|---|---|---|
| 50/30/20 | Low | Medium | Getting started, big-picture view |
| Zero-Based | High | Maximum | Full control, maximizing savings |
| Envelope | Medium | High | Impulse control, clear limits |
| Pay Yourself First | Low | Medium | Automatic savings prioritization |

## Recommendation: Zero-Based with Claude Assistance

Given that this app has Claude for analysis, zero-based budgeting is the best fit. It requires more setup but delivers the most insight, and Claude can generate the initial budget template from historical spending data.

## Zero-Based Budgeting

**Principle**: Income - All Expenses = $0. Every dollar is assigned a job before you spend it.

### Setup Process

1. Calculate monthly after-tax income (use 3-month average if variable)
2. List all fixed expenses (rent, insurance, subscriptions, loan payments)
3. Allocate to savings goals (emergency fund, retirement, travel)
4. Distribute remainder to variable categories (groceries, dining, entertainment)
5. The sum of all categories equals income exactly

### Example Budget

```
Monthly After-Tax Income: $5,200

Fixed:
  Rent:                 $1,800
  Car Insurance:          $120
  Health Insurance:       $200
  Internet:                $70
  Subscriptions:          $60
  Subtotal:            $2,250

Savings & Investments:
  Emergency Fund:         $300
  IRA:                    $500
  Vacation Fund:          $150
  Subtotal:              $950

Variable Necessities:
  Groceries:              $400
  Gas:                    $100
  Parking & Transit:       $80
  Pharmacy/Health:         $50
  Subtotal:              $630

Discretionary:
  Dining & Bars:          $300
  Coffee:                  $60
  Entertainment:          $120
  Shopping/Clothing:      $150
  Personal Care:           $80
  Amazon/Online:          $100
  Travel (monthly):       $100
  Misc/Buffer:             $60
  Subtotal:              $970

TOTAL:                  $5,200  (= income, zero-based)
```

### Claude Prompt to Generate Budget from History

```rust
pub const BUDGET_PROMPT_TEMPLATE: &str = r#"You are a personal finance advisor. Based on the following 6-month spending history, generate a zero-based monthly budget for next month.

Income: ${income}/month after tax

Historical monthly averages by category:
{category_averages}

Requirements:
1. Fixed expenses must stay as-is
2. Suggest 10-20% reduction in any discretionary categories that seem high
3. Ensure savings >= 15% of income
4. Every dollar must be assigned (total = income)
5. Add a small buffer (~1-2%) for unexpected expenses

Return JSON:
{
  "budget": [
    {"category": "...", "amount": 0.00, "type": "fixed|savings|variable|discretionary"}
  ],
  "notes": ["...", "..."],
  "total_income": {income},
  "total_budgeted": {income}
}"#;
```

## 50/30/20 Rule

Simple, no tracking required. Good as a sanity check.

```rust
use rust_decimal::Decimal;
use std::collections::HashMap;

pub struct BudgetHealth {
    pub needs_pct: f64,
    pub wants_pct: f64,
    pub savings_pct: f64,
    pub status: &'static str,
}

pub fn check_50_30_20(income: Decimal, spending: &HashMap<String, Decimal>) -> BudgetHealth {
    const NEEDS_PREFIXES: &[&str] = &["Housing", "Food: Groceries", "Transport: Gas", "Health"];
    const WANTS_PREFIXES: &[&str] = &["Food: Dining", "Life", "Shopping", "Digital"];

    let mut needs = Decimal::ZERO;
    let mut wants = Decimal::ZERO;
    for (cat, amt) in spending {
        if NEEDS_PREFIXES.iter().any(|p| cat.starts_with(p)) {
            needs += *amt;
        } else if WANTS_PREFIXES.iter().any(|p| cat.starts_with(p)) {
            wants += *amt;
        }
    }
    let savings = income - needs - wants;

    let to_f = |d: Decimal| d.to_string().parse::<f64>().unwrap_or(0.0);
    let inc = to_f(income);

    BudgetHealth {
        needs_pct: to_f(needs) / inc * 100.0,
        wants_pct: to_f(wants) / inc * 100.0,
        savings_pct: to_f(savings) / inc * 100.0,
        status: if to_f(savings) >= inc * 0.15 { "OK" } else { "LOW SAVINGS" },
    }
}
```

## Projecting Future Spending

### Method 1: Rolling Average

Use a 3-month rolling average for variable categories, latest known value for fixed:

```rust
use chrono::NaiveDate;
use rusqlite::Connection;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

pub fn project_next_month(
    conn: &Connection,
    fixed_categories: &[&str],
) -> anyhow::Result<HashMap<String, Decimal>> {
    let mut projections = HashMap::new();

    let mut stmt = conn.prepare(r#"
        SELECT category, AVG(CAST(amount AS REAL)) * -1 as avg_amount
        FROM (
            SELECT category, strftime('%Y-%m', date) as m, SUM(CAST(amount AS REAL)) as amount
            FROM transactions
            WHERE CAST(amount AS REAL) < 0
              AND date >= date('now', '-3 months')
            GROUP BY category, m
        )
        GROUP BY category
    "#)?;

    let rows = stmt.query_map([], |row| {
        let cat: String = row.get(0)?;
        let avg: f64 = row.get(1)?;
        Ok((cat, avg))
    })?;

    for row in rows {
        let (category, avg) = row?;
        let multiplier = if fixed_categories.contains(&category.as_str()) { 1.0 } else { 1.10 };
        let projected = Decimal::from_str(&format!("{:.2}", avg * multiplier))?;
        projections.insert(category, projected);
    }

    Ok(projections)
}
```

### Method 2: Seasonal Adjustment

Some spending is predictably seasonal (holiday shopping, summer travel). Track year-over-year in SQL:

```sql
SELECT category, AVG(CAST(amount AS REAL)) * -1 as avg
FROM transactions
WHERE strftime('%m', date) = ?     -- target month number
  AND CAST(amount AS REAL) < 0
GROUP BY category;
```

### Method 3: Claude Budget Analysis

Feed 6 months of categorized data to Claude (Sonnet) and ask for projections plus recommendations. See `research/07_claude_api.md` for the full HTTP client pattern.

## Tracking Budget vs Actual

Store budget targets in the `budgets` table. Generate variance reports monthly:

```sql
SELECT
    b.category,
    b.amount as target,
    COALESCE(SUM(CAST(t.amount AS REAL)) * -1, 0) as actual,
    ROUND(b.amount - COALESCE(SUM(CAST(t.amount AS REAL)) * -1, 0), 2) as variance,
    CASE
        WHEN COALESCE(SUM(CAST(t.amount AS REAL)) * -1, 0) > b.amount THEN 'OVER'
        WHEN COALESCE(SUM(CAST(t.amount AS REAL)) * -1, 0) > b.amount * 0.9 THEN 'AT RISK'
        ELSE 'ON TRACK'
    END as status
FROM budgets b
LEFT JOIN transactions t
    ON t.category = b.category
    AND strftime('%Y-%m', t.date) = b.year_month
    AND CAST(t.amount AS REAL) < 0
WHERE b.year_month = '2026-04'
GROUP BY b.category, b.amount
ORDER BY variance ASC;
```
