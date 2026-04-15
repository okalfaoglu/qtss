-- P26 — HTF+LTF confluence ("sniper entry") parent-TF ladder.
-- When an LTF TBM setup is emitted, the detector looks up the parent
-- timeframe for the same symbol + direction; if a forming/confirmed
-- TBM row exists there, it is annotated into raw_meta.htf_confluence.
-- Downstream filters / risk allocator can gate top-tier entries on
-- "HTF level + LTF sweep + BoS" — the classic confluence recipe.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('tbm', 'mtf.htf_parents', '"1m:5m,5m:15m,15m:1h,1h:4h,4h:1d,1d:1w"',
   'P26 — parent-TF ladder for HTF confluence lookup, CSV pairs "ltf:htf". When a setup fires on the LTF, the detector queries the mapped HTF for an active (forming/confirmed) same-direction TBM row. Missing entries disable confluence for that TF.')
ON CONFLICT (module, config_key) DO NOTHING;
