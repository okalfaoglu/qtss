-- 0184_elliott_duplicate_purge.sql
--
-- qtss_v2_detections içinde elliott satırları kontrolsüzce birikmiş
-- durumda — kullanıcı raporu: 36.672.278 kayıt; `family='elliott'`
-- tek başına 36.02M. Örnek: BTCUSDT 15m `leading_diagonal_5_3_5_bull`
-- 2020-08-25 18:00 anchor için TEK başına 14099 duplike invalidated
-- satır. Elliott detector her tick'te aynı anchor'ı yeniden emit
-- edip, her emission `invalidated` state'e düşürülüyordu → biriken
-- çöp. Asıl writer-side düzeltme (TBM'deki gibi single-record
-- upsert) ayrı commit'te gelecek; bu migration mevcut duplikeleri
-- temizler ve DB-seviyesinde regresyon guard'ı kurar.
--
-- Strateji:
--   1) Partial UNIQUE index (forming + confirmed + entry_ready) →
--      bir (exchange, symbol, timeframe, family, subkind, anchor_time)
--      başına aynı anda tek aktif satır. Invalidated duplikeler
--      kapsam dışı bırakılır çünkü onlar terminal.
--   2) Invalidated elliott duplike silimi: her
--      (exchange, symbol, timeframe, subkind, anchor_time) için en
--      yeni `detected_at` olan bir satır HAYATTA, kalanlar DELETE.
--   3) Audit log'a seri silme kaydı bırak — geri alınamayacak bir
--      purge olduğu için.

BEGIN;

-- 1) Önce açık state'lerde de duplike varsa kov — aksi hâlde
--    aşağıdaki partial UNIQUE index kurulamaz.
WITH ranked_open AS (
    SELECT ctid,
           ROW_NUMBER() OVER (
               PARTITION BY exchange, symbol, timeframe,
                            family, subkind,
                            (anchors->0->>'time')
               ORDER BY detected_at DESC, id DESC
           ) AS rn
      FROM qtss_v2_detections
     WHERE state IN ('forming', 'confirmed', 'entry_ready')
)
DELETE FROM qtss_v2_detections d
 USING ranked_open r
 WHERE d.ctid = r.ctid
   AND r.rn > 1;

-- 2) Invalidated elliott duplike'larını kov. CTE ile her key için
--    yalnızca en yeni detected_at'a sahip ctid'i tut, gerisini sil.
--    ctid üzerinden silmek PK scan'den daha ucuz.
WITH ranked AS (
    SELECT ctid,
           ROW_NUMBER() OVER (
               PARTITION BY exchange, symbol, timeframe,
                            family, subkind,
                            (anchors->0->>'time')
               ORDER BY detected_at DESC, id DESC
           ) AS rn
      FROM qtss_v2_detections
     WHERE family = 'elliott'
       AND state  = 'invalidated'
)
DELETE FROM qtss_v2_detections d
 USING ranked r
 WHERE d.ctid = r.ctid
   AND r.rn > 1;

-- 3) Benzer dup pattern'ı olan diğer aileler — hızlı bir tek pasoda
--    çöz. range/tbm/harmonic/wyckoff/classical invalidated satırları
--    da aynı (key, anchor_time) başına tek kayda indirgenir.
WITH ranked AS (
    SELECT ctid,
           ROW_NUMBER() OVER (
               PARTITION BY exchange, symbol, timeframe,
                            family, subkind,
                            (anchors->0->>'time')
               ORDER BY detected_at DESC, id DESC
           ) AS rn
      FROM qtss_v2_detections
     WHERE family IN ('range','tbm','harmonic','wyckoff','classical')
       AND state  = 'invalidated'
)
DELETE FROM qtss_v2_detections d
 USING ranked r
 WHERE d.ctid = r.ctid
   AND r.rn > 1;

-- 4) Tüm duplikeler temizlendi → regresyon guard'ı için partial
--    UNIQUE index. Sadece canlı satırları (forming/confirmed/
--    entry_ready) kapsar; invalidated terminal olduğu için
--    kısıtlanmaz.
CREATE UNIQUE INDEX IF NOT EXISTS uq_detections_open_anchor
    ON qtss_v2_detections (
        exchange, symbol, timeframe, family, subkind,
        (anchors->0->>'time')
    )
    WHERE state IN ('forming', 'confirmed', 'entry_ready');

COMMIT;

-- Autovacuum hızını aç — 35M+ DELETE sonrası bloat büyür.
VACUUM (ANALYZE) qtss_v2_detections;
