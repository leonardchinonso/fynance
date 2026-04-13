# Plan 12: Fingerprint and Snapshot Improvements

> Status: **Draft** — awaiting review before implementation

This plan addresses two related issues found during code review:

1. **Transaction fingerprints** use `NaiveDate` (day-only) and include `description`, causing false deduplication and fragile hashing.
2. **Snapshot unique constraints** use day-level granularity, preventing multiple balance recordings per day.

Both changes share the same underlying theme: moving from day-level to datetime-level granularity across the data model.

---

## Table of Contents

1. [Change 1: Transaction Fingerprint — Date to DateTime](#change-1-transaction-fingerprint--date-to-datetime)
2. [Change 2: Transaction Fingerprint — Remove Description](#change-2-transaction-fingerprint--remove-description)
3. [Change 3: Snapshot Unique Constraint — Date to DateTime](#change-3-snapshot-unique-constraint--date-to-datetime)
4. [Migration Strategy](#migration-strategy)
5. [Execution Order](#execution-order)

---

## Change 1: Transaction Fingerprint — Date to DateTime

### Problem

The fingerprint is computed as:

```
sha256(date | amount | description | account_id)
```

where `date` is formatted as `YYYY-MM-DD` from a `NaiveDate`. Two distinct transactions on the same day with the same amount, description, and account will collide. For example:

- 08:30 — Tesco Express, -£4.50
- 18:15 — Tesco Express, -£4.50

Both resolve to the same fingerprint. The second is silently dropped via `INSERT OR IGNORE`.

### Proposed Change

Replace `NaiveDate` with `NaiveDateTime` for the transaction date field. Format as `YYYY-MM-DDTHH:MM:SS` in the fingerprint hash.

### Justification

Most bank exports include time information:
- **Monzo**: full datetime in ISO 8601
- **Revolut**: datetime with timezone
- **Lloyds**: date only (no time), but Lloyds transactions with duplicate (date, amount, description) are genuinely rare given their description format includes unique references

For banks that only provide a date (no time component), we default to `T00:00:00`. This preserves backward compatibility: existing fingerprints for time-less imports remain valid as long as we format the same way during migration.

### Code Changes

#### 1. Model: `Transaction.date` — `NaiveDate` to `NaiveDateTime`

**File:** `backend/src/model.rs`

```rust
// BEFORE (line 24)
#[ts(type = "string")]
pub date: NaiveDate,

// AFTER
#[ts(type = "string")]
pub date: NaiveDateTime,
```

#### 2. Model: `ImportTransaction.date` — same change

**File:** `backend/src/model.rs`

```rust
// BEFORE (line 255)
#[ts(type = "string")]
pub date: NaiveDate,

// AFTER
#[ts(type = "string")]
pub date: NaiveDateTime,
```

#### 3. Unified Statement Row: `date` field

**File:** `backend/src/importers/unified.rs`

```rust
// BEFORE (line 31)
#[ts(type = "string")]
pub date: NaiveDate,

// AFTER
#[ts(type = "string")]
pub date: NaiveDateTime,
```

#### 4. Fingerprint generation call sites

Everywhere that formats the date for fingerprinting must use `%Y-%m-%dT%H:%M:%S` instead of `%Y-%m-%d`.

**File:** `backend/src/importers/unified.rs` (line 70)

```rust
// BEFORE
let date_iso = row.date.format("%Y-%m-%d").to_string();

// AFTER
let date_iso = row.date.format("%Y-%m-%dT%H:%M:%S").to_string();
```

**File:** `backend/src/storage/db.rs` (line 410, in `insert_transactions_bulk`)

```rust
// BEFORE
let date_iso = t.date.format("%Y-%m-%d").to_string();

// AFTER
let date_iso = t.date.format("%Y-%m-%dT%H:%M:%S").to_string();
```

#### 5. Database storage format

**File:** `backend/src/storage/db.rs` (line 369, in `insert_transaction`)

```rust
// BEFORE
tx.date.format("%Y-%m-%d").to_string(),

// AFTER
tx.date.format("%Y-%m-%dT%H:%M:%S").to_string(),
```

#### 6. Database read parsing

All `NaiveDate::parse_from_str(&str, "%Y-%m-%d")` calls for transaction dates must change to `NaiveDateTime::parse_from_str(&str, "%Y-%m-%dT%H:%M:%S")`, with a fallback to parse `%Y-%m-%d` as `%Y-%m-%dT00:00:00` for pre-migration data.

```rust
// Helper to parse either format during transition
fn parse_transaction_date(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .or_else(|| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .ok()
                .map(|d| d.and_hms_opt(0, 0, 0).unwrap())
        })
}
```

#### 7. Imports: `use chrono::NaiveDate` to `use chrono::NaiveDateTime`

Update all `use` statements in files that reference the transaction date type. Keep `NaiveDate` imports where it is still used (e.g., snapshot dates before Change 3, budget months).

#### 8. Frontend TypeScript bindings

The `ts-rs` export for `date: NaiveDateTime` will generate `date: string`. The frontend already treats dates as strings, so no change is needed in the frontend beyond being aware that the string now contains a `T` and time component. Verify that any date display logic in the frontend correctly parses and formats the new format (most `Date.parse()` and `new Date()` calls handle ISO 8601 with time natively).

### Impact Assessment

- **Existing data**: The migration (see [Migration Strategy](#migration-strategy)) will rewrite date strings from `YYYY-MM-DD` to `YYYY-MM-DDT00:00:00` and recompute fingerprints.
- **API consumers**: The `/api/transactions` response will now return datetime strings instead of date strings. This is a breaking change for any external agents or scripts. Since this is pre-launch MVP, this is acceptable.
- **Indexes**: The `idx_tx_date` index on `transactions(date)` remains valid. The `idx_tx_month` index using `substr(date, 1, 7)` also remains valid since the first 7 chars of `YYYY-MM-DDTHH:MM:SS` are still `YYYY-MM`.

---

## Change 2: Transaction Fingerprint — Remove Description

### Problem

The fingerprint includes `description`:

```
sha256(date | amount | description | account_id)
```

If the same transaction is imported twice via different channels (e.g., CSV then screenshot-based ingestion in the future), the description may differ slightly:
- CSV: `TESCO STORES 2345 LONDON GB`
- Screenshot OCR: `Tesco Stores`

These produce different fingerprints, so the same real-world transaction gets inserted twice.

### Proposed Change

Remove `description` from the fingerprint computation. The new fingerprint becomes:

```
sha256(datetime | amount | account_id)
```

### Justification

For deduplication, we want the fingerprint to capture "same real-world money movement". The combination of (datetime, amount, account_id) is sufficient for this:

- **datetime** (after Change 1): identifies when the money moved, down to the second
- **amount**: identifies how much moved
- **account_id**: identifies which account

Adding description was meant to disambiguate same-amount transactions on the same day, but Change 1 (datetime) already handles this by distinguishing transactions that happen at different times. The remaining edge case (two identical-amount transactions at the exact same second on the same account) is extremely unlikely and not worth the fragility that description introduces.

### Alternative Considered: Normalize Description Before Hashing

Instead of removing description, we could normalize it aggressively (lowercase, strip whitespace, remove punctuation) before including it in the fingerprint.

**Pros:**
- Tighter deduplication: fewer false negatives (two different transactions at the same second with the same amount would not collide)
- More robust than raw description

**Cons:**
- Normalization rules can never cover all variations across import channels (CSV vs OCR vs API)
- Adds complexity and a maintenance burden to keep normalization in sync across channels
- The edge case it solves (same second, same amount, same account, different transactions) is extremely rare

**Recommendation:** Remove description. The datetime + amount + account_id triple is strong enough, and keeping description introduces a class of bugs that will surface as the project adds more import channels. If we find we need tighter hashing later, we can add a normalized description field back.

### Code Changes

#### 1. Fingerprint utility function

**File:** `backend/src/util.rs`

```rust
// BEFORE (lines 54-64)
pub fn fingerprint(date: &str, amount: &str, description: &str, account_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(date.as_bytes());
    hasher.update(b"|");
    hasher.update(amount.as_bytes());
    hasher.update(b"|");
    hasher.update(description.as_bytes());
    hasher.update(b"|");
    hasher.update(account_id.as_bytes());
    hex::encode(hasher.finalize())
}

// AFTER
pub fn fingerprint(date: &str, amount: &str, account_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(date.as_bytes());
    hasher.update(b"|");
    hasher.update(amount.as_bytes());
    hasher.update(b"|");
    hasher.update(account_id.as_bytes());
    hex::encode(hasher.finalize())
}
```

#### 2. All call sites drop the `description` argument

**File:** `backend/src/importers/unified.rs` (line 81)

```rust
// BEFORE
let fp = fingerprint(&date_iso, &amount_str, &description, account_id);

// AFTER
let fp = fingerprint(&date_iso, &amount_str, account_id);
```

**File:** `backend/src/storage/db.rs` (line 414, in `insert_transactions_bulk`)

```rust
// BEFORE
let fp = fingerprint(&date_iso, &amount_str, &t.description, account_id);

// AFTER
let fp = fingerprint(&date_iso, &amount_str, account_id);
```

#### 3. Tests

**File:** `backend/src/util.rs` (tests at lines 149-175)

Update tests to reflect the new 3-field signature. The `fingerprint_changes_with_any_field` test should verify that changes to date, amount, or account_id still produce different hashes, and that description changes do NOT affect the fingerprint.

```rust
#[test]
fn fingerprint_is_deterministic() {
    let a = fingerprint("2024-01-15T10:30:00", "42.50", "acc_001");
    let b = fingerprint("2024-01-15T10:30:00", "42.50", "acc_001");
    assert_eq!(a, b);
}

#[test]
fn fingerprint_changes_with_any_field() {
    let base = fingerprint("2024-01-15T10:30:00", "42.50", "acc_001");
    assert_ne!(base, fingerprint("2024-01-16T10:30:00", "42.50", "acc_001")); // different date
    assert_ne!(base, fingerprint("2024-01-15T10:30:00", "99.99", "acc_001")); // different amount
    assert_ne!(base, fingerprint("2024-01-15T10:30:00", "42.50", "acc_002")); // different account
}
```

### CLAUDE.md Update

The conventions section currently states:

> Every importer deduplicates by a stable fingerprint hash `sha256(date, amount, description, account_id)`

This must be updated to:

> Every importer deduplicates by a stable fingerprint hash `sha256(datetime, amount, account_id)`

---

## Change 3: Snapshot Unique Constraint — Date to DateTime

### Problem

The `portfolio_snapshots` table has:

```sql
UNIQUE(snapshot_date, account_id)
```

where `snapshot_date` is stored as `YYYY-MM-DD`. This means only one balance snapshot per account per day. If a user records their portfolio balance in the morning and then again after a market move in the afternoon, the second write silently overwrites the first via `ON CONFLICT ... DO UPDATE`.

The same constraint exists for `holdings`:

```sql
UNIQUE(account_id, symbol, as_of)
```

where `as_of` is also `YYYY-MM-DD`.

### Proposed Change

Change both `snapshot_date` and `as_of` to datetime format (`YYYY-MM-DDTHH:MM:SS`) and update the unique constraints accordingly.

### Justification

- Allows multiple snapshots per day for the same account, enabling intraday tracking
- Aligns with the transaction datetime change for consistency across the data model
- The "carry forward" logic (showing the last known value) works identically with datetime, just with finer granularity

### Alternative Considered: Keep Date, Add Sequence Number

Instead of datetime, we could add a `seq` column:

```sql
UNIQUE(snapshot_date, account_id, seq)
```

**Pros:**
- Simpler migration (no format change)
- No need to track what time a balance was recorded

**Cons:**
- Loses the actual time information, which is useful for display ("as of 2:30 PM")
- Adds an artificial ordering column that needs to be managed
- Inconsistent with the transaction datetime change

**Recommendation:** Use datetime. It is consistent with the rest of the data model, provides useful time information, and does not require managing a sequence number.

### Code Changes

#### 1. Schema

**File:** `db/sql/schema.sql`

No changes to the schema DDL itself (the column types are already `TEXT`). The change is in what values we store in those TEXT columns.

The `UNIQUE` constraint does not need to change syntactically: `UNIQUE(snapshot_date, account_id)` works the same whether the TEXT contains `2024-01-15` or `2024-01-15T10:30:00`.

#### 2. Model: `PortfolioSnapshot.snapshot_date`

**File:** `backend/src/model.rs`

```rust
// BEFORE (line 341)
#[ts(type = "string")]
pub snapshot_date: NaiveDate,

// AFTER
#[ts(type = "string")]
pub snapshot_date: NaiveDateTime,
```

#### 3. Model: `Holding.as_of`

**File:** `backend/src/model.rs`

```rust
// BEFORE (line 367)
#[ts(type = "string")]
pub as_of: NaiveDate,

// AFTER
#[ts(type = "string")]
pub as_of: NaiveDateTime,
```

#### 4. Model: `Account.balance_date`

**File:** `backend/src/model.rs`

```rust
// BEFORE (line 85)
#[ts(type = "string | null")]
pub balance_date: Option<NaiveDate>,

// AFTER
#[ts(type = "string | null")]
pub balance_date: Option<NaiveDateTime>,
```

#### 5. Upsert and query functions

All snapshot-related DB functions must format dates as `%Y-%m-%dT%H:%M:%S` and parse with the same format (with fallback for pre-migration data).

**File:** `backend/src/storage/db.rs` — `upsert_portfolio_snapshot` (line 1000)

```rust
// BEFORE
snapshot.snapshot_date.format("%Y-%m-%d").to_string(),

// AFTER
snapshot.snapshot_date.format("%Y-%m-%dT%H:%M:%S").to_string(),
```

Apply the same change pattern to:
- `set_account_balance` (line 347)
- `get_snapshots_in_range` (lines 1337-1375)
- Any holding upsert/query functions that format `as_of`

#### 6. API route handlers

Any route handler that accepts date parameters from the frontend (e.g., `set-balance` endpoint accepting a date) must accept datetime strings. For backward compatibility during the transition, handlers should accept both `YYYY-MM-DD` and `YYYY-MM-DDTHH:MM:SS`, converting the former to `T00:00:00`.

#### 7. Frontend impact

- The portfolio and holdings API responses will now contain datetime strings
- Charts and tables that display snapshot dates should format them appropriately (show time only when relevant, e.g., "Jan 15, 2024 2:30 PM" for intraday, "Jan 15, 2024" for daily)
- The account balance setting UI should optionally allow specifying a time, defaulting to current time

---

## Migration Strategy

Since this is a pre-launch MVP with no production users, the migration can be aggressive. However, we should still handle it cleanly for developers who have local test data.

### Migration 002: DateTime Transition

**File:** `db/sql/migrations/002_datetime_transition.sql`

```sql
-- Migration 002: Transition date fields to datetime format and recompute fingerprints.
--
-- Step 1: Update transaction dates from YYYY-MM-DD to YYYY-MM-DDT00:00:00
UPDATE transactions
SET date = date || 'T00:00:00'
WHERE length(date) = 10;

-- Step 2: Update snapshot dates from YYYY-MM-DD to YYYY-MM-DDT00:00:00
UPDATE portfolio_snapshots
SET snapshot_date = snapshot_date || 'T00:00:00'
WHERE length(snapshot_date) = 10;

-- Step 3: Update holding dates from YYYY-MM-DD to YYYY-MM-DDT00:00:00
UPDATE holdings
SET as_of = as_of || 'T00:00:00'
WHERE length(as_of) = 10;

-- Step 4: Update account balance_date from YYYY-MM-DD to YYYY-MM-DDT00:00:00
UPDATE accounts
SET balance_date = balance_date || 'T00:00:00'
WHERE balance_date IS NOT NULL AND length(balance_date) = 10;
```

### Fingerprint Recomputation

Since we are changing both the date format in the hash AND removing description from the hash, all existing fingerprints become invalid. We need to recompute them.

This cannot be done in pure SQL because the fingerprint uses SHA-256. Instead, add a Rust migration step in `Db::open`:

```rust
fn ensure_migration_002(&self) -> Result<()> {
    // Check if migration already applied (e.g., via a migrations tracking table or pragma)
    // ...

    // Run the SQL date format updates
    self.conn.execute_batch(include_str!("../../db/sql/migrations/002_datetime_transition.sql"))?;

    // Recompute all fingerprints with the new formula: sha256(datetime | amount | account_id)
    let mut stmt = self.conn.prepare("SELECT id, date, amount, account_id FROM transactions")?;
    let rows: Vec<(String, String, String, String)> = stmt
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<rusqlite::Result<_>>()?;

    let mut update_stmt = self.conn.prepare(
        "UPDATE transactions SET fingerprint = ?1 WHERE id = ?2"
    )?;

    for (id, date, amount, account_id) in &rows {
        let new_fp = fingerprint(date, amount, account_id);
        update_stmt.execute(params![new_fp, id])?;
    }

    Ok(())
}
```

**Note on duplicates:** After recomputing fingerprints without description, some previously-distinct transactions might now collide (same datetime, amount, account, different description). This is extremely unlikely with datetime precision, but the migration should check for and report any collisions rather than silently failing on the UNIQUE constraint.

---

## Execution Order

These changes should be implemented in a single phase since they are tightly coupled:

### Step 1: Update the fingerprint function signature
- Remove `description` parameter from `fingerprint()` in `util.rs`
- Update all call sites
- Update tests

### Step 2: Change date types from NaiveDate to NaiveDateTime
- Update `Transaction`, `ImportTransaction`, `UnifiedStatementRow` model structs
- Update `PortfolioSnapshot`, `Holding`, `Account` model structs
- Update all DB read/write functions (format strings and parse logic)
- Add `parse_transaction_date()` helper for backward-compatible parsing

### Step 3: Write and apply migration
- Write `002_datetime_transition.sql`
- Add `ensure_migration_002()` in Rust with fingerprint recomputation
- Test with existing local data

### Step 4: Update documentation
- Update `CLAUDE.md` fingerprint convention
- Regenerate TypeScript bindings via `ts-rs`
- Update any API documentation that references date formats

### Step 5: Verify
- Run full test suite (`cargo test`)
- Run clippy (`cargo clippy --all-targets -- -D warnings`)
- Test import flow with sample CSVs to confirm deduplication still works
- Test snapshot upsert with same-day, different-time entries
- Verify frontend still renders correctly with datetime strings
