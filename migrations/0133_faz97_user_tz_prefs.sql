-- Faz 9.7.0 — User timezone + notification delivery preferences
--
-- Per-user daily digest scheduling requires UTC offset on `users`.
-- `notify_delivery_prefs` stores per-user channel + filter config for
-- public cards, lifecycle events, and digests. All threshold values
-- are config-driven (CLAUDE.md #2) — this table holds only the user's
-- opt-in choices, not the thresholds themselves.

-- 1. Add tz_offset_minutes to users (signed integer, -720..+840).
ALTER TABLE users
  ADD COLUMN IF NOT EXISTS tz_offset_minutes INTEGER NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS tz_label TEXT;  -- "Europe/Istanbul" (display only)

COMMENT ON COLUMN users.tz_offset_minutes IS
  'Signed minute offset from UTC. +180 = UTC+3 (Istanbul). Used by per-user daily digest scheduler.';

-- 2. Delivery preferences — one row per user, JSONB for flexible filters.
CREATE TABLE IF NOT EXISTS notify_delivery_prefs (
    user_id             UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    telegram_chat_id    TEXT,
    telegram_enabled    BOOLEAN NOT NULL DEFAULT false,
    x_handle            TEXT,
    x_enabled           BOOLEAN NOT NULL DEFAULT false,

    -- Channel-specific filters (JSON so we can extend without schema churn)
    -- Example: { "min_score_tier": 5, "categories": ["KRIPTO","VADELI"], "families": ["wyckoff","harmonic"] }
    telegram_filters    JSONB NOT NULL DEFAULT '{}'::jsonb,
    x_filters           JSONB NOT NULL DEFAULT '{}'::jsonb,

    -- Lifecycle event opt-ins (null = follow global default from system_config)
    notify_entry_touched  BOOLEAN,
    notify_tp_partial     BOOLEAN,
    notify_tp_final       BOOLEAN,
    notify_sl_hit         BOOLEAN,
    notify_invalidated    BOOLEAN,
    notify_cancelled      BOOLEAN,
    notify_daily_digest   BOOLEAN NOT NULL DEFAULT true,
    notify_weekly_digest  BOOLEAN NOT NULL DEFAULT true,

    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_notify_prefs_telegram_enabled
    ON notify_delivery_prefs (telegram_enabled) WHERE telegram_enabled = true;
CREATE INDEX IF NOT EXISTS idx_notify_prefs_x_enabled
    ON notify_delivery_prefs (x_enabled) WHERE x_enabled = true;
CREATE INDEX IF NOT EXISTS idx_notify_prefs_digest
    ON notify_delivery_prefs (notify_daily_digest) WHERE notify_daily_digest = true;

COMMENT ON TABLE notify_delivery_prefs IS
  'Per-user notification channels, filters, and event opt-ins. NULL lifecycle flags inherit system_config defaults.';
