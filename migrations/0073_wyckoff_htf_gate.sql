-- 0073_wyckoff_htf_gate.sql — Faz 10 / P5.
--
-- Multi-TF phase harmony: the Wyckoff setup loop now checks a mapped
-- HTF counterpart before emitting an LTF setup. If the HTF has an
-- active Wyckoff structure at phase >= C with a bias opposite to the
-- LTF setup's direction, the setup is vetoed. HTF in phase A/B or
-- missing entirely → no veto (insufficient evidence).
--
-- Why: a 1h bullish Spring inside a 4h Distribution phase D is a
-- textbook low-quality counter-trend scenario. This gate filters those
-- at emission so they never reach the setups table / Telegram render.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('setup', 'wyckoff.htf_gate.enabled', 'true',
   'Enable the multi-timeframe Wyckoff phase-harmony gate. When true, LTF setups whose direction conflicts with the HTF structures committed bias (phase >= C) are dropped before persistence.'),
  ('setup', 'wyckoff.htf_gate.mapping',
   '{"15m":"1h","1h":"4h","4h":"1d","1d":"1w"}',
   'LTF → HTF mapping consulted by the phase-harmony gate. JSON object; unmapped timeframes bypass the gate.')
ON CONFLICT (module, config_key) DO NOTHING;
