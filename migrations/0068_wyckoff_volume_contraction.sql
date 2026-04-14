-- 0068_wyckoff_volume_contraction.sql — Faz 10 / P1b.
--
-- Range-quality guard: volume contraction check.
--
-- Canonical Wyckoff accumulation/distribution ranges exhibit VOLUME
-- CONTRACTION ("drying up") as the composite operator completes
-- absorbing supply (or offloading demand). A "range" where pivot
-- volumes are expanding through the window is almost certainly a
-- trending market with rising participation — not a Wyckoff range.
--
-- Without this filter the detector produces false-positive ranges
-- during strong trends, which then spawn spurious Spring/UTAD
-- candidates on normal pullbacks / retests.
--
-- Implementation: compare mean volume of the late half of the pivot
-- window vs the early half. Reject if late/early > N.
--
--   late/early < 1.0   → canonical contraction (ideal)
--   late/early = 1.0   → flat (OK)
--   late/early > 1.3   → expansion (reject by default)

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('detector', 'wyckoff.max_range_volume_expansion', '"1.3"',
   'Reject ranges where late-half pivot volume / early-half > N. Wyckoff ranges contract volume; expansion = trending market, not accumulation.')
ON CONFLICT (module, config_key) DO NOTHING;
