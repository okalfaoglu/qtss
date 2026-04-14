-- 0077_wyckoff_purge_all_and_normalize.sql — Faz 10 / P11.
--
-- Full reset of `wyckoff_structures`. Every row seen in production so far
-- was either:
--   (a) a single bare Phase D/E event spawned before the Phase-A seed
--       guard landed (e.g. isolated SOS → current_phase='D' with no
--       A/B/C history), or
--   (b) a 7-year frankenstein row where the orchestrator kept appending
--       events into one active structure because no TTL existed (2019
--       AR/SC/BC plus a 2026 markup in the same events_json).
--
-- Both shapes violate the A→B→C→D→E contract we want to validate. The
-- P12 code patch drops the bootstrap phase-promotion shortcut and adds
-- a per-TF structure TTL in the orchestrator, so fresh rows will only
-- appear when a Phase-A event seeds them and will auto-fail when the
-- last recorded event is older than the TF-appropriate bar budget.
--
-- Also normalise `exchange` to lowercase. The Python ingest wrote
-- 'binance' but wyckoff_replay and list_phase_groups_by_timeframe
-- filter with 'BINANCE', so queries silently returned empty.

TRUNCATE TABLE wyckoff_structures;

UPDATE wyckoff_structures SET exchange = lower(exchange) WHERE exchange <> lower(exchange);
