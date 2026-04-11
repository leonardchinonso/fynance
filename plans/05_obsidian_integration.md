# Obsidian Integration

## Vault Setup

All financial data lives inside the existing SecondBrain vault.

```
~/SecondBrain/
└── financial/
    ├── transactions.db          <- SQLite (source of truth, synced with vault)
    ├── raw-exports/             <- Original bank files, never modified
    ├── dashboard.md             <- Main overview with live queries
    ├── monthly/
    │   └── YYYY-MM.md           <- Per-month reports
    ├── yearly/
    │   └── YYYY.md              <- Annual summaries
    └── _templates/
        ├── monthly.md           <- Templater template
        └── yearly.md
```

## Plugin Installation

Install via Settings > Community plugins > Browse:

1. **Dataview** by blacksmithgu
2. **Templater** by SilentVoid13
3. **SQLite DB** by bdaenen (search "SQLite")
4. **Charts** by phibr0

After installing SQLite DB:
Settings > SQLite DB > Database path: `financial/transactions.db`

> Note: amount is stored as TEXT in SQLite. Use `CAST(amount AS REAL)` in all SQL queries that do arithmetic.

## Dashboard Note

Save as `~/SecondBrain/financial/dashboard.md`:

````markdown
---
cssclasses: [finance-dashboard]
---

# Finance Dashboard

## This Month: Spending by Category

```sqlitedb chart:pie
SELECT category as label,
       ROUND(SUM(CAST(amount AS REAL)) * -1, 2) as value
FROM transactions
WHERE date >= date('now', 'start of month')
  AND CAST(amount AS REAL) < 0
  AND category NOT LIKE 'Finance: Internal%'
GROUP BY category
ORDER BY value DESC
LIMIT 8
```

## Budget vs Actual

```sqlitedb
SELECT
    b.category,
    printf('$%.2f', CAST(b.amount AS REAL)) as budget,
    printf('$%.2f', COALESCE(SUM(CAST(t.amount AS REAL)) * -1, 0)) as spent,
    printf('$%.2f', CAST(b.amount AS REAL) - COALESCE(SUM(CAST(t.amount AS REAL)) * -1, 0)) as remaining,
    CASE
        WHEN COALESCE(SUM(CAST(t.amount AS REAL)) * -1, 0) > CAST(b.amount AS REAL) THEN 'OVER'
        WHEN COALESCE(SUM(CAST(t.amount AS REAL)) * -1, 0) > CAST(b.amount AS REAL) * 0.9 THEN 'WATCH'
        ELSE 'OK'
    END as status
FROM budgets b
LEFT JOIN transactions t
    ON t.category = b.category
    AND strftime('%Y-%m', t.date) = strftime('%Y-%m', 'now')
    AND CAST(t.amount AS REAL) < 0
WHERE b.year_month = strftime('%Y-%m', 'now')
GROUP BY b.category
ORDER BY remaining ASC
```

## 6-Month Spending Trend

```sqlitedb chart:bar
SELECT strftime('%Y-%m', date) as label,
       ROUND(SUM(CAST(amount AS REAL)) * -1, 2) as value
FROM transactions
WHERE CAST(amount AS REAL) < 0
  AND date >= date('now', '-6 months')
  AND category NOT LIKE 'Finance: Internal%'
GROUP BY label
ORDER BY label ASC
```

## Net Savings by Month

```sqlitedb
SELECT
    strftime('%Y-%m', date) as month,
    printf('$%.2f', SUM(CASE WHEN CAST(amount AS REAL) > 0 THEN CAST(amount AS REAL) ELSE 0 END)) as income,
    printf('$%.2f', SUM(CASE WHEN CAST(amount AS REAL) < 0 THEN CAST(amount AS REAL) ELSE 0 END) * -1) as expenses,
    printf('$%.2f', SUM(CAST(amount AS REAL))) as net
FROM transactions
WHERE category NOT LIKE 'Finance: Internal%'
GROUP BY month
ORDER BY month DESC
LIMIT 12
```

## Needs Review

```sqlitedb
SELECT t.date, t.description, printf('$%.2f', CAST(t.amount AS REAL)) as amount,
       r.suggested, printf('%.0f%%', r.confidence * 100) as confidence
FROM review_queue r
JOIN transactions t ON t.id = r.transaction_id
WHERE r.reviewed = 0
ORDER BY r.confidence ASC
LIMIT 10
```
````

## Monthly Report Template

Save as `~/SecondBrain/financial/_templates/monthly.md`:

````markdown
<%*
const month = tp.date.now("YYYY-MM")
await tp.file.rename(month)
_%>
---
month: <% tp.date.now("YYYY-MM") %>
tags: [finance, monthly-report]
created: <% tp.date.now("YYYY-MM-DD") %>
---

# <% tp.date.now("MMMM YYYY") %> Finance Report

## Summary

```sqlitedb
SELECT
    printf('$%.2f', SUM(CASE WHEN CAST(amount AS REAL) > 0 THEN CAST(amount AS REAL) ELSE 0 END)) as income,
    printf('$%.2f', SUM(CASE WHEN CAST(amount AS REAL) < 0 THEN CAST(amount AS REAL) ELSE 0 END) * -1) as expenses,
    printf('$%.2f', SUM(CAST(amount AS REAL))) as net_savings
FROM transactions
WHERE strftime('%Y-%m', date) = '<% tp.date.now("YYYY-MM") %>'
  AND category NOT LIKE 'Finance: Internal%'
```

## Spending by Category

```sqlitedb chart:pie
SELECT category as label, ROUND(SUM(CAST(amount AS REAL)) * -1, 2) as value
FROM transactions
WHERE strftime('%Y-%m', date) = '<% tp.date.now("YYYY-MM") %>'
  AND CAST(amount AS REAL) < 0
  AND category NOT LIKE 'Finance: Internal%'
GROUP BY category
ORDER BY value DESC
```

## Top Transactions

```sqlitedb
SELECT date, description, printf('$%.2f', CAST(amount AS REAL) * -1) as amount, category
FROM transactions
WHERE strftime('%Y-%m', date) = '<% tp.date.now("YYYY-MM") %>'
  AND CAST(amount AS REAL) < -20
ORDER BY CAST(amount AS REAL) ASC
LIMIT 20
```

## Budget vs Actual

```sqlitedb
SELECT
    b.category,
    printf('$%.2f', CAST(b.amount AS REAL)) as budget,
    printf('$%.2f', COALESCE(SUM(CAST(t.amount AS REAL)) * -1, 0)) as actual,
    printf('$%.2f', CAST(b.amount AS REAL) - COALESCE(SUM(CAST(t.amount AS REAL)) * -1, 0)) as delta
FROM budgets b
LEFT JOIN transactions t
    ON t.category = b.category
    AND strftime('%Y-%m', t.date) = '<% tp.date.now("YYYY-MM") %>'
    AND CAST(t.amount AS REAL) < 0
WHERE b.year_month = '<% tp.date.now("YYYY-MM") %>'
GROUP BY b.category
ORDER BY delta ASC
```

## Claude Analysis

<!-- Generated by: fynance report --month <% tp.date.now("YYYY-MM") %> --with-analysis -->
````

## Rust: Writing Monthly Notes

The CLI's `report` command generates or updates the monthly note and appends Claude's analysis:

```rust
pub async fn run(
    month: &str,
    vault_path: &std::path::Path,
    db: &crate::storage::db::Db,
    http: &reqwest::Client,
    api_key: &str,
    with_analysis: bool,
) -> anyhow::Result<()> {
    let note_path = vault_path
        .join("financial/monthly")
        .join(format!("{}.md", month));

    if !note_path.exists() {
        println!("Note not found. Create it via Templater in Obsidian first, then re-run.");
        return Ok(());
    }

    if with_analysis {
        let variance = db.budget_vs_actual(month)?;
        let report_text = variance.iter().map(|v| {
            format!("- {}: spent ${:.2} vs ${:.2} budget (${:+.2})",
                v.category, v.spent, v.budget, v.budget - v.spent)
        }).collect::<Vec<_>>().join("\n");

        let analysis = crate::budget::advisor::monthly_insights(
            http, api_key, month, &report_text
        ).await?;

        let mut content = std::fs::read_to_string(&note_path)?;
        content = content.replace(
            "<!-- Generated by: fynance report",
            &format!("{}\n\n<!-- Generated by: fynance report", analysis),
        );
        std::fs::write(&note_path, content)?;
        println!("Analysis written to {}", note_path.display());
    }

    Ok(())
}
```

## Templater Hotkey Setup

1. Settings > Templater > Folder Templates > Add
2. Folder: `financial/monthly`
3. Template: `financial/_templates/monthly.md`
4. Optional hotkey: `Cmd+Shift+M` for instant monthly note creation
