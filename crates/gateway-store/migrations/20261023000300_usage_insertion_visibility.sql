-- SQLx applies this migration in one transaction. Add nullable columns without
-- defaults first so the historical ledger is not rewritten. Existing NULLs
-- identify rows committed before this migration and are always snapshot-visible.
ALTER TABLE usage_events ADD COLUMN ingestion_xid xid8;
ALTER TABLE guardrail_execution_events ADD COLUMN ingestion_xid xid8;

-- Preserve the full transaction epoch and the original insertion identity across
-- repricing updates. Explicit-column INSERTs from older binaries get this default.
ALTER TABLE usage_events
    ALTER COLUMN ingestion_xid SET DEFAULT pg_current_xact_id();
ALTER TABLE guardrail_execution_events
    ALTER COLUMN ingestion_xid SET DEFAULT pg_current_xact_id();
