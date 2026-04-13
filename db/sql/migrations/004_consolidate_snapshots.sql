-- Migration 004: Consolidate portfolio_snapshots into holdings.
--
-- Every portfolio_snapshots row becomes a holdings row with:
--   symbol         = '_CASH'
--   name           = 'Account Balance'
--   holding_type   = 'cash'
--   quantity       = 1
--   price_per_unit = NULL
--   value          = balance (from the snapshot)
--   as_of          = snapshot_date
--
-- The ON CONFLICT clause handles the case where a cash holding already exists
-- for the same (account_id, '_CASH', snapshot_date) triple. In that case,
-- the snapshot balance takes precedence because it was the authoritative source.

INSERT INTO holdings (account_id, symbol, name, holding_type, quantity, price_per_unit, value, currency, as_of)
SELECT
    ps.account_id,
    '_CASH',
    'Account Balance',
    'cash',
    '1',
    NULL,
    ps.balance,
    ps.currency,
    ps.snapshot_date
FROM portfolio_snapshots ps
ON CONFLICT(account_id, symbol, as_of) DO UPDATE SET
    value    = excluded.value,
    currency = excluded.currency;

-- Drop the old table and its index.
DROP INDEX IF EXISTS idx_snap_date;
DROP TABLE IF EXISTS portfolio_snapshots;
