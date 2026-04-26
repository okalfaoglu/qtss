-- FAZ 26 backlog — B-CTX-MM-1: Wyckoff volume gate for Phase-C
-- events (Spring / UTAD).
--
-- Real Wyckoff doctrine: a Spring fires on HEAVY volume (sellers
-- exhausting themselves on the wick). Without volume confirmation,
-- a wick + reclaim is a generic liquidity sweep, not a structurally
-- meaningful Phase-C event. Same mirror for UTAD on the
-- distribution side.
--
-- The Rust struct now carries `spring_min_volume_mult` +
-- `spring_max_volume_mult` knobs; this seed lets ops tune them
-- live without redeploys.

INSERT INTO system_config (module, config_key, value, description)
VALUES
  ('wyckoff', 'thresholds.spring_min_volume_mult',
   '{"value": 1.0}'::jsonb,
   'B-CTX-MM-1: floor for Spring/UTAD volume vs SMA baseline. Below this multiple the event is suppressed. 1.0 = baseline-or-better; raise to 1.5 to gate stricter (Pruden-aligned), 0.7 to loosen.'),
  ('wyckoff', 'thresholds.spring_max_volume_mult',
   '{"value": 2.5}'::jsonb,
   'B-CTX-MM-1: ceiling for Spring/UTAD volume score saturation. 2.5× SMA gets the maximum score boost; volume linearly scales score between min and max.')
ON CONFLICT (module, config_key)
DO UPDATE SET value = EXCLUDED.value, description = EXCLUDED.description;
