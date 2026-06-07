# Portfolio and Holdings System: Deep Dive

This document provides a comprehensive explanation of how the Portfolio and Holdings systems work in fynance, answering all the key architectural questions.

## 1. How is the Portfolio Balance Calculated?

### High-Level Overview

The portfolio balance is **not** derived from individual transaction sums. Instead, it uses a **carry-forward snapshot model** where account balances are stored as point-in-time snapshots in the `portfolio_snapshots` table.

### The Calculation Flow

**Entry Point:** `backend/src/server/routes/portfolio.rs:32-147` (`get_portfolio` handler)

1. **Query the database for all accounts as of a specific date:**
   ```rust
   // Line 49: Get accounts with carry-forward balance as of the query date
   let accounts = db.get_portfolio_as_of(as_of, profile_id)?;
   ```

2. **Sum up the balances:**
   ```rust
   // Lines 55-78
   let mut total_assets = Decimal::ZERO;
   let mut total_liabilities = Decimal::ZERO;
   let mut available_wealth = Decimal::ZERO;
   let mut unavailable_wealth = Decimal::ZERO;
   
   for account in &accounts {
       let balance = account.balance.unwrap_or(Decimal::ZERO);
       
       if balance >= Decimal::ZERO {
           total_assets += balance;
       } else {
           total_liabilities += balance;  // Credit cards, loans
       }
       
       if is_available_account(&account.account_type) {
           available_wealth += balance;
       } else {
           unavailable_wealth += balance;  // Pensions, property (future)
       }
   }
   ```

3. **Calculate net worth:**
   ```rust
   // Line 93
   let net_worth = total_assets + total_liabilities;
   ```

### The Carry-Forward Model

The key method is `db.get_portfolio_as_of()` at `backend/src/storage/db.rs:1183-1239`. This is where the magic happens:

**The Query Logic:**
```sql
-- Lines 1202-1223 of db.rs
SELECT
    a.id, a.name, a.institution, a.type, a.currency,
    a.is_active, a.notes, a.profile_ids,
    ps.balance AS snap_balance,
    ps.snapshot_date
FROM accounts a
LEFT JOIN (
    SELECT ps1.account_id, ps1.balance, ps1.snapshot_date
    FROM portfolio_snapshots ps1
    WHERE ps1.snapshot_date <= ?1  -- as_of parameter
      AND ps1.snapshot_date = (
          SELECT MAX(ps2.snapshot_date)
          FROM portfolio_snapshots ps2
          WHERE ps2.account_id = ps1.account_id
            AND ps2.snapshot_date <= ?1
      )
) ps ON ps.account_id = a.id
WHERE a.is_active = 1
```

**What this does:**
- For each active account, finds the **most recent snapshot on or before the query date** (`as_of`)
- If an account has no snapshot data, `balance` is `None`
- This is the "carry-forward" or "point-in-time" semantics mentioned in the requirements

### Why Not Use Transactions?

Transactions represent **changes** (debits/credits), but:
1. They don't capture the full account balance (need opening balance)
2. A transaction-based calculation would be fragile to missing historical data
3. Investment accounts can't be derived from transactions alone (dividends, price appreciation, stock splits)

Instead, snapshots represent **actual known balances** at specific points in time, which is what banks report.

---

## 2. How Do the Imports for Portfolios Work?

### Two Paths to Portfolio Data

**Path 1: Manual balance setting via CLI**

```bash
fynance account set-balance <account_id> <amount> --date YYYY-MM-DD
```

Implementation: `backend/src/commands/account.rs` → calls `db.set_account_balance()`

```rust
// backend/src/storage/db.rs:324-354
pub fn set_account_balance(
    &self,
    account_id: &str,
    balance: Decimal,
    date: NaiveDate,
) -> Result<()> {
    let tx = self.conn.unchecked_transaction()?;
    
    // 1. Update the account's current balance/date
    let updated = tx.execute(
        "UPDATE accounts SET balance = ?1, balance_date = ?2 WHERE id = ?3",
        params![balance.to_string(), date.format("%Y-%m-%d").to_string(), account_id],
    )?;
    
    // 2. Insert or update a snapshot record
    tx.execute(
        r"INSERT INTO portfolio_snapshots (snapshot_date, account_id, balance, currency)
          VALUES (?1, ?2, ?3, COALESCE((SELECT currency FROM accounts WHERE id = ?2), 'GBP'))
          ON CONFLICT(snapshot_date, account_id) DO UPDATE SET balance = excluded.balance",
        params![date.format("%Y-%m-%d").to_string(), account_id, balance.to_string()],
    )?;
    
    tx.commit()?;
    Ok(())
}
```

This creates a row in `portfolio_snapshots` with the given balance and date.

**Path 2: API-driven portfolio import (future)**

The API endpoint `POST /api/portfolio/snapshots` (not yet implemented but planned) would accept structured snapshot data from external agents and write directly to `portfolio_snapshots`.

### The Portfolio Snapshots Table Schema

```sql
-- db/sql/schema.sql:64-72
CREATE TABLE IF NOT EXISTS portfolio_snapshots (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    snapshot_date   TEXT NOT NULL,
    account_id      TEXT NOT NULL,
    balance         TEXT NOT NULL,    -- Stored as TEXT to avoid float errors
    currency        TEXT NOT NULL DEFAULT 'GBP',
    UNIQUE(snapshot_date, account_id)  -- Can't have 2 snapshots for same account on same day
);
```

**Key design choices:**
- **snapshot_date + account_id = unique**: There can only be one balance per account per day
- **balance as TEXT**: Uses Decimal precision, never floats
- **No other details**: Only stores the **net balance**, not the composition (that's what holdings are for)

---

## 3. How Are Portfolios Related to Holdings?

### The Relationship

**Portfolios** = **Account-level aggregation**
- One balance per account (the total)
- Used for net worth calculations, trend analysis, breakdowns by type/institution
- Queried from `portfolio_snapshots` + `accounts`

**Holdings** = **Symbol-level detail within investment accounts**
- Multiple rows per investment account (one per symbol/holding)
- Shows what you own: AAPL, GOOGL, VTSAX, etc.
- Used to answer "what stocks do I own?" and "how much in each?"
- Queried from `holdings` table

### Data Flow Example

User has a Trading 212 account with £10,000 invested in:
- 20 shares of AAPL @ £150 = £3,000
- 30 shares of GOOGL @ £140 = £4,200
- 100 units of VTSAX @ £27.50 = £2,750
- Cash remainder = £50

**Portfolio side:**
```
portfolio_snapshots {
  snapshot_date: 2026-04-12
  account_id: "trading-212-main"
  balance: "10000.00"
}
```

**Holdings side:**
```
holdings (all with as_of: 2026-04-12, account_id: "trading-212-main") {
  { symbol: "AAPL", quantity: "20", price_per_unit: "150.00", value: "3000.00" },
  { symbol: "GOOGL", quantity: "30", price_per_unit: "140.00", value: "4200.00" },
  { symbol: "VTSAX", quantity: "100", price_per_unit: "27.50", value: "2750.00" },
  { symbol: "CASH", quantity: "1", price_per_unit: "50.00", value: "50.00" }
}
```

### Why Separate?

1. **Performance**: Aggregated `portfolio_snapshots` is much faster for net-worth queries than summing detailed holdings
2. **Flexibility**: Non-investment accounts don't have holdings, so we don't force a parallel structure
3. **Clarity**: Portfolio is about "how much total?" Holdings are about "what do I own?"
4. **Carry-forward**: Both use the same point-in-time model independently

### API Routes Reflect This

```
GET  /api/portfolio              -- Fetch net worth & aggregates (uses portfolio_snapshots)
GET  /api/portfolio/history      -- Trend over time (uses portfolio_snapshots)
GET  /api/holdings               -- Fetch holdings for specific accounts
POST /api/holdings/:account_id   -- Import/replace holdings
```

---

## 4. How Are the Holding Balances Calculated?

### No Calculation — They Are Provided

Holdings are not derived or calculated. They are **directly imported** from investment platform exports or API calls.

### The Holdings Import Flow

**Entry Point:** `backend/src/server/routes/holdings.rs:75-101` (`post_holdings` handler)

```rust
pub async fn post_holdings(
    State(state): State<AppState>,
    auth: Extension<AuthContext>,
    Path(account_id): Path<String>,
    Json(body): Json<Vec<Holding>>,  // <-- List of holdings to import
) -> Result<Json<serde_json::Value>, AppError> {
    let holdings_updated = {
        let db = state.db.lock().expect("db mutex poisoned");
        if db.get_account_by_id(&account_id)?.is_none() {
            return Err(AppError::NotFound(format!("account {account_id} not found")));
        }
        db.replace_holdings(&account_id, &body)?  // <-- Replace all holdings for this account
    };
    
    Ok(Json(serde_json::json!({
        "ok": true,
        "holdings_updated": holdings_updated
    })))
}
```

### The Holding Model

From `backend/src/model.rs:349-371`:

```rust
pub struct Holding {
    pub account_id: String,
    pub symbol: String,
    pub name: String,
    pub holding_type: HoldingType,  // Stock, ETF, Fund, Bond, Crypto, Cash
    pub quantity: Decimal,
    pub price_per_unit: Option<Decimal>,  // May be None
    pub value: Decimal,               // quantity * price (or explicit from API)
    pub currency: String,
    pub as_of: NaiveDate,             // Point-in-time date
    pub short_name: Option<String>,   // Ticker alias
}
```

### Carry-Forward for Holdings

Like portfolio snapshots, holdings use carry-forward. `get_holdings_batch()` at `backend/src/storage/db.rs:1456-1488`:

```rust
pub fn get_holdings_batch(&self, account_ids: &[String]) -> Result<Vec<Holding>> {
    let sql = format!(
        r"SELECT h.account_id, h.symbol, h.name, h.holding_type,
                 h.quantity, h.price_per_unit, h.value, h.currency,
                 h.as_of, h.short_name
          FROM holdings h
          WHERE h.account_id IN ({placeholders})
            AND h.as_of = (
                SELECT MAX(h2.as_of) FROM holdings h2
                WHERE h2.account_id = h.account_id
            )
          ORDER BY h.account_id, h.symbol"
    );
    // Returns the latest (most recent as_of) holding per symbol per account
}
```

**Key: `as_of = MAX(h2.as_of)`** — Only returns the **most recent snapshot** for each symbol.

### Example Scenario

If you import holdings on 2026-04-10 with 20 AAPL shares @ £150, then import again on 2026-04-12 with 20 AAPL shares @ £155:

```
holdings table:
  account_id | symbol | quantity | price_per_unit | value | as_of
  -----------+--------+----------+----------------+-------+----------
  trading212 | AAPL   | 20       | 150.00         | 3000  | 2026-04-10
  trading212 | AAPL   | 20       | 155.00         | 3100  | 2026-04-12
```

`get_holdings_batch()` returns **only the 2026-04-12 row** (the latest).

---

## 5. How Do the Spending/Cash Flow Logic Work in Portfolio and Holdings Territory?

### Cash Flow Is Separate from Portfolio

Cash flow is **transaction-based**, not balance-based. Portfolio tracks "how much do I have," cash flow tracks "how much flowed in/out."

### Cash Flow Calculation

Entry point: `backend/src/server/routes/portfolio.rs:245-275` (`get_cash_flow` handler)

Implementation: `backend/src/storage/db.rs:1378-1453` (`get_cash_flow` method)

```rust
pub fn get_cash_flow(
    &self,
    start: NaiveDate,
    end: NaiveDate,
    profile_id: Option<&str>,
    granularity: &Granularity,
) -> Result<Vec<CashFlowMonth>> {
    // ... build a SQL query that groups transactions by period ...
    
    let sql = format!(
        r"SELECT
            {period_expr} AS period,
            SUM(CASE WHEN CAST(t.amount AS REAL) > 0 
                THEN CAST(t.amount AS REAL) ELSE 0 END) AS income,
            SUM(CASE WHEN CAST(t.amount AS REAL) < 0 
                THEN ABS(CAST(t.amount AS REAL)) ELSE 0 END) AS spending
          FROM transactions t
          {join}
          WHERE {where_clause}
          GROUP BY period
          ORDER BY period"
    );
    // ...
}
```

**Key logic:**
- **Income**: Sum of all positive transactions
- **Spending**: Sum of absolute value of all negative transactions
- Grouped by period (monthly, quarterly, yearly)

### Connection to Portfolio

**There is no direct connection.** They answer different questions:
- **Portfolio**: "What is my net worth on April 12?" → Answer: £X (balance snapshot)
- **Cash Flow**: "How much did I spend in April?" → Answer: £Y (sum of negative transactions)

**But they should be consistent:**
- If you had £100K on March 31 (portfolio snapshot)
- And earned £5K in April (cash flow income)
- And spent £3K in April (cash flow spending)
- Then you should have ~£102K on April 30 (next portfolio snapshot)

The difference = profit from investments + carry-forward from prior months.

### Investment Metrics: Portfolio + Cash Flow Together

`compute_investment_metrics()` at `backend/src/storage/db.rs:1546-1612` combines both:

```rust
pub fn compute_investment_metrics(
    &self,
    start: NaiveDate,
    end: NaiveDate,
    profile_id: Option<&str>,
) -> Result<InvestmentMetrics> {
    // 1. Get start and end portfolio values (carry-forward from snapshots)
    let start_value = sum_carry_forward(start)?;  // Portfolio as of start date
    let end_value = sum_carry_forward(end)?;      // Portfolio as of end date
    
    // 2. Get new cash invested (from transactions)
    let new_cash_invested: Decimal = {
        // Sum of "Finance: Investment Transfer" transactions in the period
        // (money moved INTO investment accounts)
    };
    
    // 3. Calculate metrics
    let total_growth = end_value - start_value;           // Overall change
    let market_growth = total_growth - new_cash_invested; // Growth beyond new deposits
    
    Ok(InvestmentMetrics {
        start_value,
        end_value,
        total_growth,
        new_cash_invested,
        market_growth,  // Pure market appreciation (excludes new cash)
    })
}
```

**Example:**
- Start of year: £50K in investments
- During year: Deposit £10K more
- End of year: £63K in investments
- `total_growth` = £13K
- `new_cash_invested` = £10K
- `market_growth` = £3K (the "gain" from market appreciation, excluding deposits)

---

## 6. Explain the Logic Behind the "LAST VALUE (Point-in-Time)" for Portfolio and Balance

### The Core Concept

**Point-in-time** means: "Give me the most recent known value on or before this date, even if I haven't updated it since then."

This is essential because:
1. You don't update your portfolio daily (import happens monthly or quarterly)
2. You still want to ask "what was my net worth on Feb 15?" even if you only imported data on Feb 1 and Mar 1

### The SQL Pattern

Both `portfolio_snapshots` and `holdings` use the same pattern:

```sql
-- For portfolio_snapshots (generic template)
SELECT MAX(snapshot_date)
FROM portfolio_snapshots ps2
WHERE ps2.account_id = ps1.account_id
  AND ps2.snapshot_date <= ?1  -- as_of parameter
```

This finds the **most recent snapshot on or before the query date**.

### Code Implementation

`row_to_portfolio_account()` at `backend/src/storage/db.rs:1690-1727`:

```rust
fn row_to_portfolio_account(
    row: &rusqlite::Row<'_>,
    as_of: NaiveDate,
    stale_days: i64,
) -> rusqlite::Result<Account> {
    // ... extract snap_balance, snap_date_str from the query result ...
    
    let balance = snap_balance
        .as_deref()
        .and_then(|s| s.parse::<Decimal>().ok());
    
    let balance_date = snap_date_str
        .as_deref()
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
    
    // Mark as stale if older than 45 days
    let is_stale = balance_date
        .map(|d| (as_of - d).num_days() > stale_days)
        .unwrap_or(false);
    
    Ok(Account {
        // ...
        balance,
        balance_date,
        is_stale: Some(is_stale),  // Flag for UI to show "as of Jan 1"
    })
}
```

### Staleness Indicator

The `is_stale` field (line 1725) flags old data:
- **`is_stale: true`** = Most recent snapshot is > 45 days old
- **`is_stale: false`** = Recently updated (within 45 days)

The UI uses this to show "£10,000 (as of Jan 15, stale)" vs "£10,000 (fresh today)".

### Example Timeline

```
import on Jan 1:   balance = £100K
import on Feb 1:   balance = £105K
query on Feb 15:   return £105K (from Feb 1), is_stale = false
query on Apr 1:    return £105K (from Feb 1), is_stale = true (>45 days)
query on Jun 1:    return £105K (from Feb 1), is_stale = true
```

---

## 7. How Do Imports for Holdings Work and How Is This Different from Imports for Portfolio and Regular Account Transactions?

### Three Distinct Import Paths

#### A. Regular Transactions (CSV Import)

**What:** Bank statements (Monzo, Revolut, Lloyds CSV)
**Where:** `backend/src/importers/csv_importer.rs` + `backend/src/server/routes/import_api.rs`
**API:** `POST /api/import/csv` or `POST /api/import` (JSON)
**Deduplication:** Fingerprint on `(date, amount, account_id)` — prevents duplicate rows

```rust
// backend/src/storage/db.rs:414
let fp = fingerprint(&date_iso, &amount_str, &t.description, account_id);
```

Each transaction is a **discrete event** and never changes.

#### B. Portfolio Snapshots (Balance Import)

**What:** Account balances at specific dates (Manual via CLI or API)
**Entry point:** `fynance account set-balance` or future `POST /api/portfolio/snapshots`
**How:** Calls `db.set_account_balance()` → writes to both `accounts` and `portfolio_snapshots`

```rust
// backend/src/storage/db.rs:324-354
// 1. Updates accounts.balance, accounts.balance_date
// 2. Inserts or updates portfolio_snapshots row
```

**Unique constraint:** `UNIQUE(snapshot_date, account_id)` — Can only have one balance per account per day

#### C. Holdings (Investment Detail Import)

**What:** Symbol-level holdings within investment accounts
**Entry point:** `POST /api/holdings/:account_id` (JSON)
**Route:** `backend/src/server/routes/holdings.rs:75-101` (`post_holdings` handler)
**How:** Calls `db.replace_holdings()` at `backend/src/storage/db.rs:1493-1540`

```rust
pub fn replace_holdings(&self, account_id: &str, holdings: &[Holding]) -> Result<u32> {
    // 1. Collect all distinct as_of dates from the payload
    let mut dates: Vec<String> = holdings
        .iter()
        .map(|h| h.as_of.format("%Y-%m-%d").to_string())
        .collect();
    dates.sort();
    dates.dedup();
    
    // 2. Delete all existing holdings for this account on those dates
    for date in &dates {
        tx.execute(
            "DELETE FROM holdings WHERE account_id = ?1 AND as_of = ?2",
            rusqlite::params![account_id, date],
        )?;
    }
    
    // 3. Insert new holdings
    for h in holdings {
        tx.execute(
            r"INSERT INTO holdings (...) VALUES (...)",
            // ... all fields ...
        )?;
        inserted += 1;
    }
    
    tx.commit()?;
    Ok(inserted)
}
```

### Key Differences

| Aspect | Transactions | Portfolio Snapshots | Holdings |
|--------|--------------|--------------------|---------| 
| **What** | Debits/credits | Account balances | Individual securities |
| **API** | `POST /api/import/csv` or `POST /api/import` | CLI: `account set-balance` | `POST /api/holdings/:account_id` |
| **Table** | `transactions` | `portfolio_snapshots` | `holdings` |
| **Dedup** | Fingerprint (date, amount, account_id) | Unique(snapshot_date, account_id) | Unique(account_id, symbol, as_of) |
| **Update semantics** | `INSERT OR IGNORE` (idempotent) | Upsert (overwrite if exists) | Replace all on date (full replace) |
| **Immutable?** | Yes (once imported, never changes) | No (can re-import for same date) | No (can replace the whole portfolio on a date) |
| **Carry-forward?** | No (summed for ranges) | Yes (use most recent <= date) | Yes (use most recent <= date) |

### Why Separate Import Paths?

1. **Data comes from different sources**: Banks export transactions, brokerages export holdings
2. **Different update patterns**: Transactions are immutable events; holdings are snapshots that get replaced wholesale
3. **Different semantics**: Transactions need deduplication; holdings need full replacement (you can't have 2 "latest" AAPL entries)
4. **Different processing**: Transactions run through categorization pipeline; holdings are already structured

### Example: Monthly Ingestion Workflow

```
1. User logs into Monzo, exports CSV for March
   → POST /api/import/csv
   → Inserts 47 new transactions, 2 duplicates
   
2. User logs into Trading 212, exports holdings data as JSON
   → POST /api/holdings/trading-212-main
   → Replaces all holdings for trading-212-main as of 2026-04-01
     (deletes old AAPL/GOOGL entries, inserts new ones)
   
3. User sets account balance on savings account
   → fynance account set-balance savings-1 50000 --date 2026-04-01
   → Updates accounts.balance, inserts portfolio_snapshots row
   → Carries forward when portfolio is queried
```

### Idempotency

- **Transactions**: `INSERT OR IGNORE` means re-importing the same CSV twice is safe
- **Portfolio snapshots**: Upsert means re-setting the same balance is safe
- **Holdings**: Replace means re-importing the same holdings is safe (old data is deleted first)

All three paths are **idempotent** — safe to run multiple times.

---

## Summary Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                         API Requests                         │
└─────┬───────────────────┬─────────────────────────┬──────────┘
      │                   │                         │
   POST /api/import       fynance account           POST /api/
   (CSV or JSON)        set-balance               holdings/:id
      │                   │                         │
      ▼                   ▼                         ▼
   ┌─────────┐         ┌──────────┐            ┌─────────┐
   │transactions│       │accounts  │            │holdings │
   └─────────┘         │portfolio │            └─────────┘
   · Daily events      │_snapshots│            · By symbol
   · Immutable         └──────────┘            · Carry-forward
   · Deduplicated      · Point-in-time         · Replaced
                       · Carry-forward
                       · Can be updated
      
      ▼                   ▼                       ▼
   Cash flow          Net worth               Investment
   (Sum amounts)      (Sum balances)          detail
```

---

## Code Pointers

| Question | Code Location |
|----------|---------------|
| How is portfolio balance calculated? | `backend/src/server/routes/portfolio.rs:32-93` |
| Carry-forward query for portfolio? | `backend/src/storage/db.rs:1183-1239` |
| Set account balance (manual)? | `backend/src/storage/db.rs:324-354` |
| Import holdings (API)? | `backend/src/server/routes/holdings.rs:75-101` |
| Replace holdings in DB? | `backend/src/storage/db.rs:1493-1540` |
| Get holdings (carry-forward)? | `backend/src/storage/db.rs:1456-1488` |
| Cash flow calculation? | `backend/src/storage/db.rs:1378-1453` |
| Staleness logic? | `backend/src/storage/db.rs:1710-1712` |
| Transaction fingerprint? | `backend/src/storage/db.rs:414` |
| Investment metrics? | `backend/src/storage/db.rs:1546-1612` |

