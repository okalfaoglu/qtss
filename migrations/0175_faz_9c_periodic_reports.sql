-- 0175_faz_9c_periodic_reports.sql
--
-- Faz 9C — periodic performance reports (weekly / monthly / yearly).
--
-- Daily digest already exists as per-user delivery (`digest_loop`).
-- This migration adds a *market-wide* report scheduler that fires at
-- fixed UTC boundaries and pushes a summary to Telegram + X. One audit
-- row per (kind, window_start) so the scheduler is idempotent across
-- worker restarts — if a row already exists for the current window,
-- the loop skips it.
--
-- Idempotent.

CREATE TABLE IF NOT EXISTS qtss_reports_runs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kind            TEXT NOT NULL CHECK (kind IN ('weekly','monthly','yearly')),
    window_start    TIMESTAMPTZ NOT NULL,
    window_end      TIMESTAMPTZ NOT NULL,
    generated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- Delivery receipts. Either flag may be null if the channel is off.
    telegram_ok     BOOLEAN,
    x_ok            BOOLEAN,
    -- Snapshot of the payload — cheap insurance for audit + re-send.
    body_telegram   TEXT,
    body_x          TEXT,
    -- Aggregate snapshot as JSONB so the GUI (future report viewer)
    -- doesn't have to recompute.
    aggregate_json  JSONB NOT NULL,
    last_error      TEXT
);

-- One run per (kind, window_start) — the scheduler uses this to decide
-- whether the current window has already been dispatched.
CREATE UNIQUE INDEX IF NOT EXISTS uq_reports_runs_kind_window
    ON qtss_reports_runs(kind, window_start);

CREATE INDEX IF NOT EXISTS idx_reports_runs_generated
    ON qtss_reports_runs(generated_at DESC);

-- Config knobs. Loop ticks every `report.scan_tick_secs`, fires each
-- kind iff enabled AND the current window's row doesn't exist yet.
-- Delivery hour is fixed at 09:00 UTC — early enough for EU morning,
-- late enough for US evening.
INSERT INTO system_config (module, config_key, value, description, is_secret)
VALUES
  ('notify', 'report.enabled',           '{"enabled": false}'::jsonb,
   'Faz 9C — master switch for weekly/monthly/yearly market summary reports.', false),
  ('notify', 'report.scan_tick_secs',    '{"secs": 300}'::jsonb,
   'Seconds between scheduler ticks. Boundaries are checked on every tick.',   false),
  ('notify', 'report.dispatch_hour_utc', '{"value": 9}'::jsonb,
   'UTC hour at which a due report is dispatched (0-23).',                     false),
  ('notify', 'report.weekly_enabled',    '{"enabled": true}'::jsonb,
   'Dispatch weekly summary (Monday 09:00 UTC window=previous ISO week).',     false),
  ('notify', 'report.monthly_enabled',   '{"enabled": true}'::jsonb,
   'Dispatch monthly summary (day-1 09:00 UTC window=previous calendar month).',false),
  ('notify', 'report.yearly_enabled',    '{"enabled": true}'::jsonb,
   'Dispatch yearly summary (Jan-1 09:00 UTC window=previous calendar year).', false),
  ('notify', 'report.send_telegram',     '{"enabled": true}'::jsonb,
   'Dispatch to Telegram (uses the default chat id from NotifyConfig).',       false),
  ('notify', 'report.send_x',            '{"enabled": true}'::jsonb,
   'Enqueue to x_outbox for the X publisher loop to pick up.',                 false)
ON CONFLICT (module, config_key) DO NOTHING;

COMMENT ON TABLE qtss_reports_runs IS
  'Faz 9C — audit of market-wide periodic reports. One row per (kind, window_start).';
