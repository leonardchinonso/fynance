-- Migration 001: add LLM detection columns to import_log.
-- Applied once by Db::open via ensure_migration_001(); idempotency is
-- handled in Rust by checking PRAGMA table_info before running the ALTER.
ALTER TABLE import_log ADD COLUMN detected_bank TEXT;
ALTER TABLE import_log ADD COLUMN detection_confidence REAL;
