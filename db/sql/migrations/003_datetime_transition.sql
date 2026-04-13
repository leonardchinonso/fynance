-- Migration 003: Transition date fields to datetime format.
--
-- Changes all stored YYYY-MM-DD values to YYYY-MM-DDTHH:MM:SS by appending
-- T00:00:00. The WHERE guards make every statement idempotent so it is safe
-- to run on already-migrated databases.
--
-- NOTE: fingerprint recomputation cannot be done in SQL (SHA-256 is not a
-- SQLite builtin). The Rust layer in ensure_migration_003() handles that step
-- after this SQL runs.

-- Step 1: Update transaction dates.
UPDATE transactions
SET date = date || 'T00:00:00'
WHERE length(date) = 10;

-- Step 2: Update portfolio snapshot dates.
UPDATE portfolio_snapshots
SET snapshot_date = snapshot_date || 'T00:00:00'
WHERE length(snapshot_date) = 10;

-- Step 3: Update holding as_of dates.
UPDATE holdings
SET as_of = as_of || 'T00:00:00'
WHERE length(as_of) = 10;

-- Step 4: Update account balance_date values.
UPDATE accounts
SET balance_date = balance_date || 'T00:00:00'
WHERE balance_date IS NOT NULL AND length(balance_date) = 10;
