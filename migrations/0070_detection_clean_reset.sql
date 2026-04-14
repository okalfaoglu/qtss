-- 0070_detection_clean_reset.sql — Faz 10 / P2b.
--
-- Clean-slate reset for detection data.
--
-- Why:
--   Before 0069 (pivot historical backfill), every orchestrator tick
--   rebuilt PivotEngine from the last 500 bars. That meant `bar_index`
--   on cached pivots was relative to the rolling window — the same
--   real pivot got stored under different bar_index values as the
--   window slid forward. Every Wyckoff / Elliott / Classical / Harmonic
--   detection written during that era references inconsistent
--   bar_index anchors and a history-blind view of structure.
--
--   With 0069 in place, pivot_cache now holds a globally-indexed,
--   from-first-bar pivot set. The stale detections on top of it are
--   actively misleading (wrong anchor bar_index, absent Phase-A
--   context). Clean slate is safer than carrying them forward.
--
-- What this migration does:
--   1. Truncates qtss_v2_detections, wyckoff_structures, wyckoff_signals,
--      wave_chain and the related projection table so the detectors
--      repopulate cleanly.
--   2. Bumps `detection.orchestrator.history_bars` 500 → 3000 so the
--      live detector has a much wider window to reason over. Combined
--      with the globally-indexed pivot_cache from 0069, detectors now
--      see enough history to pick up multi-month ranges that the old
--      500-bar window hid.
--   3. Leaves progressive per-offset historical replay as a follow-up
--      (requires orchestrator refactor to make process_symbol reusable
--      at arbitrary offsets).
--
-- Operator note: this is a ONE-TIME reset tied to the 0069/0070 pair.
-- After running, the orchestrator will repopulate detections as
-- market_bars are processed with the new pivot_cache semantics.

-- =========================================================================
-- 1. Truncate stale detection data
-- =========================================================================

-- qtss_v2_detections holds all four detector families. Cascading to any
-- downstream tables that reference it via FK.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'qtss_v2_detections') THEN
        EXECUTE 'TRUNCATE TABLE qtss_v2_detections RESTART IDENTITY CASCADE';
    END IF;
END
$$;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'wyckoff_structures') THEN
        EXECUTE 'TRUNCATE TABLE wyckoff_structures RESTART IDENTITY CASCADE';
    END IF;
END
$$;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'wyckoff_signals') THEN
        EXECUTE 'TRUNCATE TABLE wyckoff_signals RESTART IDENTITY CASCADE';
    END IF;
END
$$;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'wave_chain') THEN
        EXECUTE 'TRUNCATE TABLE wave_chain RESTART IDENTITY CASCADE';
    END IF;
END
$$;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'wave_projections') THEN
        EXECUTE 'TRUNCATE TABLE wave_projections RESTART IDENTITY CASCADE';
    END IF;
END
$$;

-- =========================================================================
-- 2. Widen orchestrator window + force pivot_cache full rebuild
-- =========================================================================

-- history_bars: 500 → 3000. Live detector now spans far enough to pick
-- up the kind of multi-month Wyckoff ranges and Elliott impulses that
-- the old window truncated.
UPDATE system_config
   SET value = '3000',
       description = 'Bars per orchestrator tick fed to pivot+regime engines. Wider = detectors see more history; narrower = faster per tick. 3000 after Faz 10 P2b widening.'
 WHERE module = 'detection'
   AND config_key = 'orchestrator.history_bars';

-- Also: wipe pivot_backfill_state so the 0069 worker recomputes the
-- cache on its next tick. The cache itself is already consistent (0069
-- deletes + rewrites per series), but clearing the cursor guarantees a
-- fresh pass after this reset lands.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'pivot_backfill_state') THEN
        EXECUTE 'TRUNCATE TABLE pivot_backfill_state';
    END IF;
END
$$;

-- =========================================================================
-- 3. Follow-up marker
-- =========================================================================

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('detection', 'historical_progressive_scan.enabled', '{"enabled":false}',
   'Progressive per-offset historical detection replay (Faz 10 P3). When enabled, a dedicated worker walks each symbol bar-by-bar calling every detector family at each offset so multi-year history is covered. Off by default until the orchestrator refactor (extract process_symbol into reusable callable) lands.')
ON CONFLICT (module, config_key) DO NOTHING;
