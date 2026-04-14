-- Wyckoff signal idempotency key (Faz 8.0a storage layer)
--
-- The signal_emitter produces a deterministic key of shape
--   wy:{symbol}:{tf}:{range_id}:{setup_type}:{profile}
-- that must map 1:1 to a row in qtss_v2_setups so that a rescan of the same
-- range does not create duplicate setups — it updates the existing one.
--
-- Existing (exchange, symbol, timeframe, profile, direction) partial-unique
-- constraint is too coarse for Wyckoff: Spring and UT on the same tf would
-- share (profile=D, direction=long/short) keys; LPS and BUEC (both Q, long)
-- would collide outright. A dedicated idempotency_key column is the right
-- primitive and keeps the generic D/T/Q path unchanged.

BEGIN;

ALTER TABLE qtss_v2_setups
  ADD COLUMN IF NOT EXISTS idempotency_key TEXT;

-- Unique index (partial: NULL allowed for non-Wyckoff rows; many NULLs permitted).
CREATE UNIQUE INDEX IF NOT EXISTS ux_v2_setups_idempotency_key
  ON qtss_v2_setups (idempotency_key)
  WHERE idempotency_key IS NOT NULL;

COMMIT;
