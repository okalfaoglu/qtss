-- Classical detector config seed — activates the `ClassicalWriter`
-- (qtss-engine) and seeds threshold overrides so operators can tune per
-- asset class without touching code (CLAUDE.md #2, no hardcoded
-- constants). Every key is optional: missing rows fall back to
-- `qtss_classical::ClassicalConfig::defaults()` so a blank config does
-- not disable the writer.
--
-- Shape (one row per key):
--   module        = 'classical'
--   config_key    = 'enabled' | 'min_score' | 'pivots_per_slot'
--                 | 'bars_per_tick' | 'thresholds.<name>'
--   value (jsonb) = {"enabled": bool}
--                 | {"score":  0.55}
--                 | {"count":  500}
--                 | {"bars":   2000}
--                 | {"value":  <number>}   -- for thresholds.*
--
-- The dispatch table in `crates/qtss-engine/src/writers/classical.rs`
-- (`apply_threshold`) maps `thresholds.<name>` → `ClassicalConfig`
-- field. Adding a new threshold is one row here + one match arm there,
-- no central if/else to edit (CLAUDE.md #1).

INSERT INTO system_config (module, config_key, value, description)
VALUES
    ('classical', 'enabled',         '{"enabled": true}'::jsonb,
     'Master on/off for the classical chart-pattern engine writer. Gates the ClassicalWriter inside qtss-engine alongside the elliott/harmonic writers.'),

    ('classical', 'min_score',       '{"score": 0.55}'::jsonb,
     'Minimum structural score (0..1) required before a classical match is persisted. Tighter (0.65+) = fewer but cleaner patterns; looser (0.45) = richer candidate pool for the validator to prune.'),

    ('classical', 'pivots_per_slot', '{"count": 500}'::jsonb,
     'Upper bound on recent pivots loaded per slot per tick. 500 covers roughly the last 6-24 hours on 1m-1h timeframes while keeping the sliding-window scan O(n*shapes) tractable.'),

    ('classical', 'bars_per_tick',   '{"bars": 2000}'::jsonb,
     'Upper bound on recent bars loaded per symbol per tick — used by the bar-aware shapes (flag, pennant, cup, rounding, scallop) that need ATR / flagpole context beyond pivots alone.')
ON CONFLICT (module, config_key) DO NOTHING;

-- Per-pattern thresholds. These mirror the fields of
-- `qtss_classical::ClassicalConfig` (see its rustdoc for meaning and
-- valid ranges). All keys are optional; defaults from `ClassicalConfig::
-- defaults()` apply when a row is absent.
INSERT INTO system_config (module, config_key, value, description)
VALUES
    -- Core geometry tolerances (shared across multiple shapes).
    ('classical', 'thresholds.equality_tolerance',        '{"value": 0.03}'::jsonb,
     'Maximum relative deviation (0..0.25) between "equal" peaks or shoulders. 0.03 = ±3% tolerance. Looser for noisy low-TF, tighter for daily/weekly.'),
    ('classical', 'thresholds.apex_horizon_bars',         '{"value": 50}'::jsonb,
     'Triangle apex must fall within this many future bars of the last pivot — otherwise the pattern is too loose to be actionable.'),
    ('classical', 'thresholds.flatness_threshold_pct',    '{"value": 0.001}'::jsonb,
     'Trendline slope (fraction of price per bar) below which a line is considered "flat" for asc/desc triangle legs. Default 0.1%/bar.'),
    ('classical', 'thresholds.flatness_min_score',        '{"value": 0.3}'::jsonb,
     'Minimum flatness score (0..1) an asc/desc triangle needs on its flat side to qualify.'),

    -- H&S family.
    ('classical', 'thresholds.neckline_tolerance_mult',   '{"value": 1.5}'::jsonb,
     'H&S neckline equality tolerance multiplier (1..3). Neckline is looser than shoulders so tolerance is equality_tolerance × this.'),
    ('classical', 'thresholds.hs_max_neckline_slope_pct', '{"value": 0.003}'::jsonb,
     'H&S neckline slope cap (fraction per bar). Above this the neckline is too steep for a valid H&S.'),
    ('classical', 'thresholds.hs_time_symmetry_tol',      '{"value": 0.5}'::jsonb,
     'H&S shoulder time-symmetry tolerance (0..2). Ideal H&S has roughly equal bar-spans between LS→H and H→RS.'),

    -- Triangles.
    ('classical', 'thresholds.triangle_symmetry_tol',     '{"value": 0.5}'::jsonb,
     'Symmetrical triangle slope-symmetry tolerance (|upper.slope| vs lower.slope, fractional).'),

    -- Rectangle.
    ('classical', 'thresholds.rectangle_max_slope_pct',   '{"value": 0.002}'::jsonb,
     'Rectangle max trendline slope (fraction per bar). Beyond this the pattern is a channel or triangle, not a rectangle.'),
    ('classical', 'thresholds.rectangle_min_bars',        '{"value": 15}'::jsonb,
     'Minimum bar-span from first to last rectangle pivot — filters short "range" noise.'),

    -- Flag / Pennant.
    ('classical', 'thresholds.flag_pole_min_move_atr',    '{"value": 3.0}'::jsonb,
     'Flagpole minimum directional move as an ATR multiple (0..20). Default 3.0 → pole ≥ 3×ATR.'),
    ('classical', 'thresholds.flag_pole_max_bars',        '{"value": 20}'::jsonb,
     'Lookback window (bars) for flagpole detection.'),
    ('classical', 'thresholds.flag_max_retrace_pct',      '{"value": 0.5}'::jsonb,
     'Flag body may retrace at most this fraction (0..1) of the flagpole height.'),
    ('classical', 'thresholds.flag_atr_period',           '{"value": 14}'::jsonb,
     'Wilder ATR period for flag / pennant bar-geometry checks.'),
    ('classical', 'thresholds.flag_parallelism_tol',      '{"value": 0.3}'::jsonb,
     'Flag channel slope parallelism tolerance (|upper.slope - lower.slope|/avg, 0..2).'),
    ('classical', 'thresholds.pennant_max_height_pct_of_pole','{"value": 0.4}'::jsonb,
     'Pennant max height as a fraction (0..1) of the flagpole height.'),

    -- Channels.
    ('classical', 'thresholds.channel_parallelism_tol',   '{"value": 0.15}'::jsonb,
     'Channel parallelism tolerance (|upper.slope - lower.slope|/avg).'),
    ('classical', 'thresholds.channel_min_bars',          '{"value": 20}'::jsonb,
     'Channel minimum bar-span from first to last pivot.'),
    ('classical', 'thresholds.channel_min_slope_pct',     '{"value": 0.001}'::jsonb,
     'Channel minimum |slope| (fraction per bar); below this the shape is a rectangle, not a channel.'),

    -- Cup & Handle / Rounding.
    ('classical', 'thresholds.cup_min_bars',              '{"value": 30}'::jsonb,
     'Cup minimum bars from rim_left to rim_right (Bulkowski: ~7 weekly bars; scaled per TF).'),
    ('classical', 'thresholds.cup_rim_equality_tol',      '{"value": 0.03}'::jsonb,
     'Cup rim (left/right) equality tolerance (|price diff|/mid).'),
    ('classical', 'thresholds.cup_min_depth_pct',         '{"value": 0.12}'::jsonb,
     'Cup minimum depth as a fraction of rim price.'),
    ('classical', 'thresholds.cup_max_depth_pct',         '{"value": 0.50}'::jsonb,
     'Cup maximum depth as a fraction of rim price.'),
    ('classical', 'thresholds.cup_roundness_r2',          '{"value": 0.60}'::jsonb,
     'Cup parabolic fit R² threshold (0..1); below this the shape is not round enough.'),
    ('classical', 'thresholds.handle_max_depth_pct_of_cup','{"value": 0.5}'::jsonb,
     'Handle max depth as a fraction of cup depth.'),
    ('classical', 'thresholds.rounding_min_bars',         '{"value": 40}'::jsonb,
     'Rounding bottom/top minimum bar-span (longer than Cup).'),
    ('classical', 'thresholds.rounding_roundness_r2',     '{"value": 0.65}'::jsonb,
     'Rounding parabolic fit R² threshold (slightly tighter than Cup).'),

    -- Triple / Broadening / V / ABCD.
    ('classical', 'thresholds.triple_peak_tol',           '{"value": 0.03}'::jsonb,
     'Triple top/bottom maximum relative deviation between the three peaks.'),
    ('classical', 'thresholds.triple_min_span_bars',      '{"value": 10}'::jsonb,
     'Triple top/bottom minimum bar-span from first to last pivot.'),
    ('classical', 'thresholds.triple_neckline_slope_max', '{"value": 0.003}'::jsonb,
     'Triple top/bottom neckline max slope (fraction per bar).'),
    ('classical', 'thresholds.broadening_min_slope_pct',  '{"value": 0.002}'::jsonb,
     'Broadening (megaphone) minimum |slope| on diverging edges.'),
    ('classical', 'thresholds.broadening_flat_slope_pct', '{"value": 0.0015}'::jsonb,
     'Broadening triangle "flat" edge max |slope|. Must be < broadening_min_slope_pct.'),
    ('classical', 'thresholds.v_max_total_bars',          '{"value": 20}'::jsonb,
     'V-top/V-bottom maximum total bar-span.'),
    ('classical', 'thresholds.v_min_amplitude_pct',       '{"value": 0.03}'::jsonb,
     'V-top/V-bottom minimum edge amplitude (as a fraction of price).'),
    ('classical', 'thresholds.v_symmetry_tol',            '{"value": 0.4}'::jsonb,
     'V-top/V-bottom left-vs-right symmetry tolerance.'),
    ('classical', 'thresholds.abcd_c_min_retrace',        '{"value": 0.382}'::jsonb,
     'ABCD B→C retracement min ratio vs AB (0..1).'),
    ('classical', 'thresholds.abcd_c_max_retrace',        '{"value": 0.886}'::jsonb,
     'ABCD B→C retracement max ratio vs AB (0..1); must be > abcd_c_min_retrace.'),
    ('classical', 'thresholds.abcd_d_projection_tol',     '{"value": 0.15}'::jsonb,
     'ABCD CD leg projection tolerance around 1.0×AB.'),
    ('classical', 'thresholds.abcd_min_bars_per_leg',     '{"value": 3}'::jsonb,
     'ABCD minimum bar-count per leg.'),

    -- Scallop (Bulkowski J-shape, Faz 10 Aşama 4).
    ('classical', 'thresholds.scallop_min_bars',          '{"value": 20}'::jsonb,
     'Scallop minimum bar-span from rim_left to rim_right.'),
    ('classical', 'thresholds.scallop_min_rim_progress_pct','{"value": 0.035}'::jsonb,
     'Scallop asymmetry: rim_right must be ahead of rim_left by ≥ this fraction (breakout foot).'),
    ('classical', 'thresholds.scallop_roundness_r2',      '{"value": 0.70}'::jsonb,
     'Scallop parabolic fit R² threshold (slightly looser than Rounding to accept J-shaped curves).')
ON CONFLICT (module, config_key) DO NOTHING;
