-- 0183_wyckoff_dedup_by_time_ms.sql
--
-- Faz 10 dedup düzeltmesi — `bar_index` rolling window offset'lerine
-- göre kaydığı için aynı gerçek bara ait BC/AR/SC event'leri
-- wyckoff_structures.events_json içinde (4978, 4982, 4986, 4990, …)
-- şeklinde çoklanıyordu. 0182 `(event, bar_index)` anahtarıyla dedupe
-- ettiği için bu duplikeleri yakalayamamıştı. Doğru anahtar
-- `(event, time_ms)` — time_ms gerçek bar açılış saniyesi olduğu
-- için rolling window'dan bağımsızdır.
--
-- Her satırın events_json array'inde (event, time_ms) anahtarlı
-- duplikeler en yüksek score kazanacak şekilde collapse edilir.
-- time_ms NULL olan legacy eventler (event, bar_index) fallback'iyle
-- dedupe edilir.

BEGIN;

WITH exploded AS (
    SELECT s.id AS struct_id,
           elem,
           elem->>'event'                                AS event_type,
           NULLIF(elem->>'time_ms', '')::BIGINT          AS time_ms,
           NULLIF(elem->>'bar_index', '')::BIGINT        AS bar_index,
           COALESCE((elem->>'score')::DOUBLE PRECISION, 0) AS score
      FROM wyckoff_structures s,
           LATERAL jsonb_array_elements(s.events_json) AS elem
     WHERE jsonb_typeof(s.events_json) = 'array'
       AND jsonb_array_length(s.events_json) > 0
),
keyed AS (
    SELECT struct_id,
           elem,
           score,
           -- Primary: (event, time_ms). Fallback when time_ms NULL:
           -- (event, -1, bar_index) so legacy rows still dedupe.
           CASE WHEN time_ms IS NOT NULL
                THEN format('%s|t:%s', event_type, time_ms)
                ELSE format('%s|b:%s', event_type, COALESCE(bar_index::TEXT, 'nil'))
           END AS dedup_key,
           bar_index
      FROM exploded
),
ranked AS (
    SELECT struct_id,
           elem,
           dedup_key,
           bar_index,
           ROW_NUMBER() OVER (
               PARTITION BY struct_id, dedup_key
               ORDER BY score DESC, bar_index DESC NULLS LAST
           ) AS rn
      FROM keyed
),
kept AS (
    SELECT struct_id,
           jsonb_agg(elem ORDER BY bar_index NULLS LAST) AS new_events
      FROM ranked
     WHERE rn = 1
     GROUP BY struct_id
)
UPDATE wyckoff_structures s
   SET events_json = kept.new_events,
       updated_at  = now()
  FROM kept
 WHERE s.id = kept.struct_id
   AND s.events_json IS DISTINCT FROM kept.new_events;

COMMIT;
