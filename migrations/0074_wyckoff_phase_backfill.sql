-- 0074_wyckoff_phase_backfill.sql — Faz 10 / P7.
--
-- Recompute `current_phase` for every `wyckoff_structures` row from
-- its own `events_json`. The tracker's bootstrap-promotion fix
-- (commit 22672e4) ensures new events promote current_phase to at
-- least the event's canonical phase, but historical rows that were
-- written before the fix can still report Phase A despite carrying
-- SOS/LPSY/Markup/etc events. This backfill aligns persisted rows
-- with the new rule.
--
-- Mapping mirrors `WyckoffEvent::phase()` in crates/qtss-wyckoff:
--   A: p_s, s_c, b_c, a_r, s_t
--   B: u_a, st_b
--   C: spring, utad, shakeout
--   D: s_o_s, s_o_w, l_p_s, lpsy, j_a_c, break_of_ice, buec, s_o_t
--   E: markup, markdown

UPDATE wyckoff_structures SET current_phase = (
  SELECT CASE MAX(ord)
    WHEN 5 THEN 'E' WHEN 4 THEN 'D' WHEN 3 THEN 'C' WHEN 2 THEN 'B' ELSE 'A'
  END
  FROM (
    SELECT CASE e->>'event'
      WHEN 'markup' THEN 5 WHEN 'markdown' THEN 5
      WHEN 's_o_s' THEN 4 WHEN 's_o_w' THEN 4
      WHEN 'l_p_s' THEN 4 WHEN 'lpsy'  THEN 4
      WHEN 'j_a_c' THEN 4 WHEN 'break_of_ice' THEN 4
      WHEN 'buec'  THEN 4 WHEN 's_o_t' THEN 4
      WHEN 'spring' THEN 3 WHEN 'utad' THEN 3 WHEN 'shakeout' THEN 3
      WHEN 'u_a'  THEN 2 WHEN 'st_b' THEN 2
      ELSE 1
    END AS ord
    FROM jsonb_array_elements(events_json) AS e
  ) x
)
WHERE jsonb_typeof(events_json) = 'array'
  AND jsonb_array_length(events_json) > 0;
