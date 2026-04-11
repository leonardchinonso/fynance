# Budgeting System

> **Deferred.** Budgeting is not part of the current implementation. It depends on having categorized transactions, which itself depends on the categorizer being built first.

## Planned Approach (future)

Once transactions are imported and categorized, the budgeting system will track a monthly budget per category and compare it against actual spending.

### Planned commands

```bash
fynance budget init --income 5200 --month 2026-05
fynance budget set --month 2026-05 --category "Food: Dining & Bars" --amount 250
fynance budget status
```

### Planned schema additions

```sql
CREATE TABLE IF NOT EXISTS budgets (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    year_month  TEXT NOT NULL,
    category    TEXT NOT NULL,
    amount      TEXT NOT NULL,
    UNIQUE(year_month, category) ON CONFLICT REPLACE
);
```

### Variance query (for future use)

```sql
SELECT
    b.category,
    CAST(b.amount AS REAL) as budget,
    COALESCE(SUM(CAST(t.amount AS REAL)) * -1, 0) as spent,
    ROUND(CAST(b.amount AS REAL) - COALESCE(SUM(CAST(t.amount AS REAL)) * -1, 0), 2) as remaining
FROM budgets b
LEFT JOIN transactions t
    ON t.category = b.category
    AND strftime('%Y-%m', t.date) = b.year_month
    AND CAST(t.amount AS REAL) < 0
WHERE b.year_month = ?
GROUP BY b.category
ORDER BY remaining ASC;
```

Budget generation via Claude (Sonnet) is one option for later but is not planned until the manual workflow is established first.
