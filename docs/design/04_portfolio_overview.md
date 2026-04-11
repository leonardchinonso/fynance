# Portfolio Overview Design

## Goal

Give the user a single view of their financial position: how much they have, where it is, and how it is diversified. This is not investment analytics — it is account-level wealth tracking.

---

## Portfolio Tab: UI Sections

### 1. Net Worth Card

A prominent figure at the top showing total assets minus total liabilities.

```
┌─────────────────────────────────────────────────┐
│  Net Worth                                       │
│                                                  │
│  £28,450.00              ↑ £320 this month      │
│                                                  │
│  Assets: £30,200    Liabilities: £1,750          │
└─────────────────────────────────────────────────┘
```

Calculation:
- Assets: sum of balances for `type IN ('checking', 'savings', 'investment', 'cash', 'pension')`
- Liabilities: sum of balances for `type = 'credit'` where balance > 0 (outstanding credit card debt)

### 2. Accounts List

A card per account showing balance, institution, and type badge.

```
┌──────────────────────────────┐  ┌──────────────────────────────┐
│  Monzo Current               │  │  Revolut Savings             │
│  Checking                    │  │  Savings                     │
│                              │  │                              │
│  £1,240.00         GBP       │  │  £8,000.00         GBP       │
│  Updated: 10 Apr 2026        │  │  Updated: 10 Apr 2026        │
└──────────────────────────────┘  └──────────────────────────────┘
```

### 3. Diversity Breakdown

A pie or donut chart breaking down net worth by account type. A secondary breakdown by institution.

```
By Account Type             By Institution

[Pie chart]                 [Pie chart]
  Savings    42%              Revolut    48%
  Investments 50%             Monzo      15%
  Checking    7%              Lloyds     37%
  Credit     -6%
```

**Account types and their colors**:
| Type | Color |
|---|---|
| Checking | Blue |
| Savings | Green |
| Investment | Purple |
| Pension | Indigo |
| Credit (liability) | Red |
| Cash | Yellow |

### 4. Net Worth Over Time

A line chart showing monthly net worth snapshots.

- X axis: months (last 12-24 months)
- Y axis: GBP value
- Data source: `portfolio_snapshots` table, one point per month per account, summed per month

```
£30,000 ┤                                              ╭─
£28,000 ┤                              ╭───────────────╯
£25,000 ┤              ╭───────────────╯
£22,000 ┤  ────────────╯
        └────────────────────────────────────────────────
        Jan 25   Apr 25   Jul 25   Oct 25   Jan 26   Apr 26
```

### 5. Monthly Cash Flow Bar Chart

Side-by-side bars for income vs spending per month.

```
£5,000 ┤
£4,000 ┤  ██ ░░   ██ ░░   ██ ░░   ██ ░░
£3,000 ┤  ██ ░░   ██ ░░   ██ ░░   ██ ░░
£2,000 ┤  ██ ░░   ██ ░░   ██ ░░   ██ ░░
£1,000 ┤  ██ ░░   ██ ░░   ██ ░░   ██ ░░
        └──────────────────────────────
        Jan   Feb   Mar   Apr
        ██ Income   ░░ Spending
```

---

## Data Sources

### Account Balances

Balances are not computed from transactions (which would require complete transaction history). Instead, the user provides a balance when registering an account or after each import.

**CLI to update balance**:
```bash
fynance account set-balance monzo-current 1240.00 --date 2026-04-10
```

**UI**: Each account card has an "Update balance" button that opens a small form.

**Auto-snapshot**: Every time a balance is updated, a row is written to `portfolio_snapshots`. This builds the history for the net worth trend chart automatically.

### Transaction-Derived Cash Flow

The monthly cash flow chart is computed from transactions:
```sql
SELECT
  substr(date, 1, 7)  AS month,
  SUM(CASE WHEN CAST(amount AS REAL) > 0 THEN CAST(amount AS REAL) ELSE 0 END) AS income,
  SUM(CASE WHEN CAST(amount AS REAL) < 0 THEN ABS(CAST(amount AS REAL)) ELSE 0 END) AS spending
FROM transactions
WHERE date >= date('now', '-12 months')
GROUP BY month
ORDER BY month;
```

---

## Approach Options: Balance Tracking

### Option A: Manual Balance Updates (Recommended for MVP)

User updates the balance for each account after each CSV import. Simple, no reconciliation needed.

**Pros**: Simple, always accurate, no complex reconciliation
**Cons**: Requires manual update step; balance can drift if user forgets

### Option B: Compute Balance from Transactions

Run `SUM(amount)` over all transactions per account as the "balance."

**Pros**: Always in sync with imported transactions; no manual step
**Cons**: Only accurate if all transactions are imported (no gaps in history); opening balance must be set; credit card balances computed incorrectly if statements show payments as income

### Option C: Hybrid (MVP+)

Start with Option A. For Option B, the user sets an "opening balance as of date X" and the system adds the transaction sum from that date onward. Display a warning if the computed balance diverges from the last manually set balance by more than a threshold.

**Recommendation**: Ship Option A for MVP. Add Option C as a later enhancement once users have full history imported.

---

## API Endpoint

### GET /api/portfolio

```json
{
  "net_worth": "28450.00",
  "currency": "GBP",
  "as_of": "2026-04-10",
  "total_assets": "30200.00",
  "total_liabilities": "1750.00",
  "accounts": [
    {
      "id": "monzo-current",
      "name": "Monzo Current",
      "institution": "Monzo",
      "type": "checking",
      "balance": "1240.00",
      "balance_date": "2026-04-10",
      "currency": "GBP",
      "is_active": true
    }
  ],
  "by_type": [
    { "type": "savings",     "total": "12000.00", "percent": 42.2 },
    { "type": "investment",  "total": "14310.00", "percent": 50.3 },
    { "type": "checking",    "total": "2140.00",  "percent": 7.5  },
    { "type": "credit",      "total": "-1750.00", "percent": -6.1 }
  ],
  "by_institution": [
    { "institution": "Revolut", "total": "13650.00", "percent": 48.0 },
    { "institution": "Lloyds",  "total": "10520.00", "percent": 37.0 },
    { "institution": "Monzo",   "total": "4280.00",  "percent": 15.0 }
  ],
  "monthly_snapshots": [
    { "month": "2025-05", "net_worth": "22100.00" },
    { "month": "2025-06", "net_worth": "23400.00" }
  ]
}
```

### POST /api/accounts

Register a new account.

```json
{
  "id": "lloyds-savings",
  "name": "Lloyds ISA",
  "institution": "Lloyds",
  "type": "savings",
  "currency": "GBP",
  "balance": "10520.00",
  "balance_date": "2026-04-01"
}
```

### PATCH /api/accounts/:id/balance

Update a single account's balance.

```json
{ "balance": "10820.00", "balance_date": "2026-04-10" }
```
