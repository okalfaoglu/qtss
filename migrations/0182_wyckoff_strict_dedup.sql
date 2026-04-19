-- 0182_wyckoff_strict_dedup.sql
--
-- Wyckoff duplicate label fix.
--
-- Kural (operator istedi):
--   * Bir (exchange, symbol, segment, interval) için yalnızca TEK
--     aktif `wyckoff_structures` satırı olabilir.
--   * O satırın `events_json` array'i içinde aynı bar'da aynı event
--     tekrar edemez (aynı event + aynı bar_index tek kayıt).
--   * Aynı bar'da FARKLI event eklenebilir — engellenmez.
--
-- Bu migration üç şey yapar:
--   1) Aktif satırlar arasında duplike varsa en yeni
--      started_at'ı olan satırı aktif bırakır, kalanları
--      is_active=false yapar (failure_reason='superseded_dedup_migration').
--   2) Tüm satırların events_json array'lerinde aynı (event_type,
--      bar_index) anahtarlı kayıtları en yüksek score'a göre collapse
--      eder (güvenli/idempotent — herhangi bir satırda duplike yoksa
--      içerik değişmez).
--   3) (exchange, symbol, segment, interval) üzerine is_active=true
--      için partial UNIQUE index ekler — writer tarafındaki regresyonu
--      DB seviyesinde yakalamak için.

BEGIN;

-- 1) Aktif satırlar arasında duplike tespit et → en yeni olan kalır,
--    diğerleri soft-invalidate edilir.
WITH ranked AS (
    SELECT id,
           ROW_NUMBER() OVER (
               PARTITION BY exchange, symbol, segment, interval
               ORDER BY started_at DESC, created_at DESC
           ) AS rn
      FROM wyckoff_structures
     WHERE is_active = true
)
UPDATE wyckoff_structures s
   SET is_active      = false,
       failed_at      = COALESCE(s.failed_at, now()),
       failure_reason = 'superseded_dedup_migration'
  FROM ranked r
 WHERE s.id = r.id
   AND r.rn > 1;

-- 2) events_json içindeki (event_type, bar_index) duplikelerini collapse
--    et. Postgres jsonb_array_elements + DISTINCT ON ile en yüksek
--    score'lu kayıt hayatta kalır. events_json boş / NULL olan satırlar
--    atlanır.
WITH exploded AS (
    SELECT s.id AS struct_id,
           (elem->>'event')                   AS event_type,
           (elem->>'bar_index')::BIGINT       AS bar_index,
           COALESCE((elem->>'score')::DOUBLE PRECISION, 0) AS score,
           elem,
           ROW_NUMBER() OVER (
               PARTITION BY s.id, (elem->>'event'), (elem->>'bar_index')
               ORDER BY COALESCE((elem->>'score')::DOUBLE PRECISION, 0) DESC,
                        (elem->>'time_ms')::BIGINT DESC NULLS LAST
           ) AS rn
      FROM wyckoff_structures s,
           LATERAL jsonb_array_elements(s.events_json) AS elem
     WHERE jsonb_typeof(s.events_json) = 'array'
       AND jsonb_array_length(s.events_json) > 0
),
kept AS (
    SELECT struct_id,
           jsonb_agg(elem ORDER BY bar_index) AS new_events
      FROM exploded
     WHERE rn = 1
     GROUP BY struct_id
)
UPDATE wyckoff_structures s
   SET events_json = kept.new_events,
       updated_at  = now()
  FROM kept
 WHERE s.id = kept.struct_id
   AND s.events_json IS DISTINCT FROM kept.new_events;

-- 3) Yeni duplike yazımlarını DB seviyesinde engelle (partial UNIQUE —
--    yalnızca aktif satırlar). Inaktif geçmiş satırlar istenildiği kadar
--    çoğalabilir; sadece "şu an yaşayan" structure tek olmalıdır.
CREATE UNIQUE INDEX IF NOT EXISTS uq_wyckoff_structures_active
    ON wyckoff_structures (exchange, symbol, segment, interval)
    WHERE is_active = true;

COMMIT;
