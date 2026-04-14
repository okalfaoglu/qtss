-- 0075_wyckoff_event_name_normalize.sql — Faz 10 / P8.
--
-- Some legacy wyckoff_structures rows carry events with the serde
-- default snake_case spelling (u_t_a_d, l_p_s_y) instead of the
-- renamed tags (utad, lpsy). Those rows slip through the phase
-- recomputation in 0074 because the CASE statement only matched the
-- canonical names. Normalize the stored names here, then re-run the
-- phase backfill so the final current_phase reflects every event.

UPDATE wyckoff_structures
   SET events_json = (
     SELECT jsonb_agg(
       CASE e->>'event'
         WHEN 'u_t_a_d' THEN e || '{"event":"utad"}'::jsonb
         WHEN 'l_p_s_y' THEN e || '{"event":"lpsy"}'::jsonb
         ELSE e
       END
       ORDER BY (e->>'bar_index')::bigint
     )
     FROM jsonb_array_elements(events_json) AS e
   )
 WHERE jsonb_typeof(events_json) = 'array'
   AND events_json::text ~ '"(u_t_a_d|l_p_s_y)"';

-- Re-run the phase backfill (same logic as 0074) so rows touched
-- above pick up the correct current_phase.
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
