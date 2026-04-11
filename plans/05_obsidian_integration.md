# Obsidian Integration

## Vault Setup

All financial data lives inside the existing SecondBrain vault.

```
~/SecondBrain/
└── financial/
    ├── transactions.db          <- SQLite (source of truth, synced with vault)
    ├── raw-exports/             <- Original bank CSV files, never modified
    ├── dashboard.md             <- Main overview with live queries
    ├── monthly/
    │   └── YYYY-MM.md           <- Per-month reports
    └── _templates/
        └── monthly.md           <- Templater template
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

## This Month: Spending by Account

```sqlitedb
SELECT account,
       COUNT(*) as transactions,
       printf('$%.2f', SUM(CAST(amount AS REAL)) * -1) as spent
FROM transactions
WHERE date >= date('now', 'start of month')
  AND CAST(amount AS REAL) < 0
GROUP BY account
ORDER BY spent DESC
```

## 6-Month Spending Trend

```sqlitedb chart:bar
SELECT strftime('%Y-%m', date) as label,
       ROUND(SUM(CAST(amount AS REAL)) * -1, 2) as value
FROM transactions
WHERE CAST(amount AS REAL) < 0
  AND date >= date('now', '-6 months')
GROUP BY label
ORDER BY label ASC
```

## Net by Month

```sqlitedb
SELECT
    strftime('%Y-%m', date) as month,
    printf('$%.2f', SUM(CASE WHEN CAST(amount AS REAL) > 0 THEN CAST(amount AS REAL) ELSE 0 END)) as income,
    printf('$%.2f', SUM(CASE WHEN CAST(amount AS REAL) < 0 THEN CAST(amount AS REAL) ELSE 0 END) * -1) as expenses,
    printf('$%.2f', SUM(CAST(amount AS REAL))) as net
FROM transactions
GROUP BY month
ORDER BY month DESC
LIMIT 12
```

## Recent Transactions

```sqlitedb
SELECT date, description, printf('$%.2f', CAST(amount AS REAL)) as amount, account, category
FROM transactions
ORDER BY date DESC, imported_at DESC
LIMIT 25
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
```

## Spending by Category

```sqlitedb chart:pie
SELECT category as label, ROUND(SUM(CAST(amount AS REAL)) * -1, 2) as value
FROM transactions
WHERE strftime('%Y-%m', date) = '<% tp.date.now("YYYY-MM") %>'
  AND CAST(amount AS REAL) < 0
  AND category IS NOT NULL
GROUP BY category
ORDER BY value DESC
```

## Top Transactions

```sqlitedb
SELECT date, description, printf('$%.2f', CAST(amount AS REAL) * -1) as amount, account, category
FROM transactions
WHERE strftime('%Y-%m', date) = '<% tp.date.now("YYYY-MM") %>'
  AND CAST(amount AS REAL) < -20
ORDER BY CAST(amount AS REAL) ASC
LIMIT 20
```
````

## Templater Hotkey Setup

1. Settings > Templater > Folder Templates > Add
2. Folder: `financial/monthly`
3. Template: `financial/_templates/monthly.md`
4. Optional hotkey: `Cmd+Shift+M` for instant monthly note creation
