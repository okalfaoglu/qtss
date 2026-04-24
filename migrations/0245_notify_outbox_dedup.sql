-- notify_outbox dedup_key — prevent duplicate Telegram cards after
-- worker restart.
--
-- Symptom: every time qtss-worker restarts, some already-sent events
-- (e.g. `allocator_v2_armed` for a still-armed setup) re-hit the
-- outbox because the inserter doesn't know the outbox already shipped
-- them. Telegram thus receives the same card multiple times in a few
-- minutes, polluting the feed.
--
-- Fix: add a dedup_key column + UNIQUE partial index. Hot path uses
-- INSERT ... ON CONFLICT (dedup_key) DO NOTHING so a replay is cheap
-- and lossless. Callers that don't want dedup pass NULL (the partial
-- index skips NULL rows, so legacy inserts stay legal).
--
-- Convention:
--   allocator_v2_armed         → dedup_key = 'armed:{setup_id}'
--   allocator_v2_commission_skip / sanity_skip / sl_too_tight / etc.
--                              → dedup_key = '{event_key}:{setup_id}'
--   setup close                → dedup_key = 'close:{setup_id}'
--   hourly position snapshot   → dedup_key = 'hourly:{setup_id}:{yyyymmddHH}'

ALTER TABLE notify_outbox
    ADD COLUMN IF NOT EXISTS dedup_key TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS notify_outbox_dedup_key_uniq
    ON notify_outbox (dedup_key)
    WHERE dedup_key IS NOT NULL;

COMMENT ON COLUMN notify_outbox.dedup_key IS
    'Optional idempotency key. Rows with the same dedup_key are deduplicated via a partial unique index. Used by setup lifecycle + hourly snapshot inserters so worker restarts do not replay already-sent Telegram cards.';
