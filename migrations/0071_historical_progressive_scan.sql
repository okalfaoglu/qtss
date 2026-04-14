-- 0071_historical_progressive_scan.sql — Faz 10 / P3.
--
-- Progressive per-offset historical detection replay.
--
-- The companion worker (historical_progressive_scan) walks every
-- enabled symbol's bar history from the first stored bar forward,
-- firing each detector family (Wyckoff / Elliott / Classical /
-- Harmonic / Range) at every `stride_bars`-step offset. At each
-- offset the pivot + regime engines see only bars up to that point,
-- so detections land in qtss_v2_detections as they would have
-- appeared in real time — not just the current end-of-history snap.
--
-- Dedup: V2DetectionRepository + raw_meta.last_anchor_idx prevents
-- overlapping offsets from inserting the same structure twice.

-- =========================================================================
-- 1. Per-series scan cursor
-- =========================================================================

CREATE TABLE IF NOT EXISTS historical_progressive_scan_state (
    exchange         TEXT        NOT NULL,
    segment          TEXT        NOT NULL,
    symbol           TEXT        NOT NULL,
    timeframe        TEXT        NOT NULL,
    last_offset      BIGINT      NOT NULL,
    total_bars       BIGINT      NOT NULL,
    total_inserted   BIGINT      NOT NULL DEFAULT 0,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (exchange, segment, symbol, timeframe)
);

COMMENT ON TABLE historical_progressive_scan_state IS
    'Cursor for the progressive historical detection scan. last_offset = bar index up to which detectors have already been fired; subsequent passes resume from there.';

-- =========================================================================
-- 2. Worker config keys
-- =========================================================================

-- Enable flag was inserted by 0070; flip it to true to switch the
-- worker on after migration lands and operators are ready to reprocess.
-- The remaining keys are the knobs the loop reads at tick time.
INSERT INTO system_config (module, config_key, value, description) VALUES
  ('detection', 'historical_progressive_scan.tick_secs', '3600',
   'Poll interval for the progressive scan loop. Hourly is enough — work is idempotent, resumes from cursor, caps per symbol via max_offsets_per_pass.'),
  ('detection', 'historical_progressive_scan.stride_bars', '50',
   'Step size in bars between detection invocations during replay. Smaller = denser coverage but more detector calls; larger = faster but risks skipping transient patterns.'),
  ('detection', 'historical_progressive_scan.min_offset_bars', '100',
   'Minimum starting offset (in bars). Detectors need enough warm-up history — firing at offset 0 would only produce noise.'),
  ('detection', 'historical_progressive_scan.max_offsets_per_pass', '200',
   'Upper bound on detector invocations per symbol per worker tick. Keeps any single pass bounded while the full series gets chipped through across many hours.')
ON CONFLICT (module, config_key) DO NOTHING;
