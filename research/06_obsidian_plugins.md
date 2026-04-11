# Obsidian Plugin Ecosystem

## Recommended Plugin Stack

### Core (Required)

| Plugin | Author | Purpose |
|---|---|---|
| **Dataview** | blacksmithgu | Query markdown notes as a database |
| **Templater** | SilentVoid13 | Dynamic templates with JavaScript |
| **Charts** | phibr0 | Pie, bar, line charts from data |
| **SQLite DB** | bdaenen | Run SQL queries and render charts from .db files |

### Optional Enhancements

| Plugin | Purpose |
|---|---|
| **Ledger** (tgrosinger) | Plain-text accounting if skipping SQLite |
| **Dataview Publisher** | Keep Dataview results current in Obsidian Publish |
| **Charts View** | More chart types (treemap, dual-axis) |

## Dataview

The most powerful Obsidian plugin for querying structured data in notes.

**Install**: Settings > Community plugins > Search "Dataview"

### Query Modes

```javascript
// TABLE mode: tabular output
TABLE file.name, category, amount
FROM "financial/monthly"
WHERE amount < 0
SORT date DESC

// LIST mode: bullet list
LIST
FROM "financial/monthly"
WHERE contains(tags, "finance")

// TASK mode: track todo items
TASK
FROM "financial"
WHERE !completed

// CALENDAR mode: heatmap by date
CALENDAR date
FROM "financial/monthly"
```

### Inline DQL

Use inside notes to show live summaries:

```markdown
Total spent this month: `= sum(filter(this.transactions, (t) => t.amount < 0))`
```

### DataviewJS (Full JavaScript)

For complex computations:

```javascript
```dataviewjs
const pages = dv.pages('"financial/monthly"')
  .where(p => p.date >= dv.date("2026-01-01"))

const byCategory = {}
for (const page of pages) {
  for (const txn of (page.transactions || [])) {
    if (txn.amount < 0) {
      byCategory[txn.category] = (byCategory[txn.category] || 0) + txn.amount
    }
  }
}

dv.table(
  ["Category", "Total Spent"],
  Object.entries(byCategory)
    .sort((a, b) => a[1] - b[1])
    .map(([cat, amt]) => [cat, `$${(amt * -1).toFixed(2)}`])
)
```
```

## SQLite DB Plugin

Runs SQL queries against a local SQLite file and renders results as tables or charts.

**Install**: Settings > Community plugins > Search "SQLite DB"

### Configuration

After installing, configure the database path in plugin settings:
- Path: `financial/transactions.db` (relative to vault root)

### Usage in Notes

Wrap SQL in a code block with `sqlitedb` language tag:

````markdown
```sqlitedb
SELECT category, ROUND(SUM(amount) * -1, 2) as total
FROM transactions
WHERE date >= '2026-04-01' AND amount < 0
GROUP BY category
ORDER BY total DESC
```
````

### Chart Rendering

Add `chart:pie` or `chart:bar` after the query type:

````markdown
```sqlitedb chart:pie
SELECT category as label, ROUND(SUM(amount) * -1, 2) as value
FROM transactions
WHERE strftime('%Y-%m', date) = '2026-04'
  AND amount < 0
GROUP BY category
ORDER BY value DESC
LIMIT 8
```
````

## Templater

Automates note creation with dynamic content.

**Install**: Settings > Community plugins > Search "Templater"

### Key Functions

```javascript
// Current date/time
tp.date.now("YYYY-MM-DD")               // "2026-04-11"
tp.date.now("MMMM YYYY")               // "April 2026"
tp.date.now("YYYY-MM", 0, "YYYY-MM", -1)  // Previous month

// User input
await tp.system.prompt("Budget for Groceries?")
await tp.system.suggester(["Option A", "Option B"], ["a", "b"])

// File operations
tp.file.title          // Current note title
tp.file.folder()       // Current folder path
```

### Monthly Note Auto-Creation

Set Templater to auto-create monthly reports in `financial/monthly/`:

```javascript
// Template: financial/_templates/monthly.md
<%*
const month = tp.date.now("YYYY-MM")
const title = tp.date.now("MMMM YYYY")
await tp.file.rename(month)
_%>
---
month: <% tp.date.now("YYYY-MM") %>
title: <% title %>
tags: [finance, monthly]
---

# <% title %> Finance Report
...
```

Set a Templater hotkey (e.g., `Cmd+Shift+M`) to trigger monthly note creation.

## Charts Plugin

Renders Chart.js visualizations from inline data.

**Install**: Settings > Community plugins > Search "Obsidian Charts"

### Pie Chart Example

````markdown
```chart
type: pie
labels: [Groceries, Dining, Subscriptions, Transport, Other]
datasets:
  - data: [400, 250, 90, 130, 180]
    label: April 2026
tension: 0.2
width: 60%
```
````

### Line Chart for Trends

````markdown
```chart
type: line
labels: [Nov, Dec, Jan, Feb, Mar, Apr]
datasets:
  - data: [2100, 2800, 1950, 2050, 2200, 2100]
    label: Monthly Spending
    fill: true
tension: 0.3
width: 80%
```
````

## Finance-Specific Plugin Alternatives

If the SQLite + Dataview approach feels heavy, these dedicated plugins are simpler:

### Ledger Plugin

- Plain-text double-entry accounting in Obsidian
- Uses standard Ledger file format (`*.ledger`)
- Quick expense widget for fast entry
- Works on mobile via Obsidian URI protocol

```
2026/04/11 Whole Foods
    Expenses:Groceries   $87.23
    Assets:Chase

2026/04/10 Starbucks
    Expenses:Coffee   $6.75
    Assets:Chase
```

**Pros**: Pure markdown, portable, version-control friendly
**Cons**: Learning curve, no native visualization, requires separate reporting tool

### Budget Planner Plugin

Simple inline budget tracking in code blocks:

````markdown
```budget
income: 5200
rent: 1800
groceries: 400
dining: 300
savings: 700
```
````

Shows a simple table with allocation and remaining balance.

## Plugin Interaction Diagram

```
Bank Statement (CSV/PDF)
        ↓
Python importer script
        ↓
transactions.db (SQLite)
        ↓
SQLite DB Plugin ←→ Obsidian Notes
        ↓
Charts Plugin (visualizations)
        ↓
Dataview (cross-note summaries)
        ↓
Templater (auto-generate monthly reports)
```
