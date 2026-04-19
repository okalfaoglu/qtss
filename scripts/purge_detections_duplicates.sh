#!/usr/bin/env bash
# purge_detections_duplicates.sh
#
# qtss_v2_detections tablosunda family='elliott' state='invalidated'
# satırlarının %99+'ı aynı (exchange, symbol, timeframe, subkind,
# anchors->0->>'time') anahtarının re-emission duplikesi. Bu script
# her doğal anahtar için en yeni created_at satırını korur, geri
# kalanı 50K'lik chunk'larla siler. Her batch ayrı transaction, küçük
# pause ile — login / REFRESH MATERIALIZED VIEW kilitlerini bloke
# etmez.
#
# Kullanım (off-peak):
#   ./scripts/purge_detections_duplicates.sh
#
# Çıktı: her batch için satır sayısı + zaman damgası; sonda VACUUM.

set -euo pipefail

PSQL="sudo -u postgres psql -d qtss -v ON_ERROR_STOP=1 -X -q"
BATCH=50000
SLEEP=0.5

echo "[$(date +%H:%M:%S)] Building purge queue (keeper = MAX(created_at) per natural key)..."
$PSQL <<SQL
DROP TABLE IF EXISTS _elliott_purge_queue;
CREATE UNLOGGED TABLE _elliott_purge_queue AS
SELECT d.id
  FROM qtss_v2_detections d
  JOIN (
    SELECT exchange, symbol, timeframe, subkind,
           (anchors->0->>'time') AS anchor_time,
           MAX(created_at) AS keep_ts
      FROM qtss_v2_detections
     WHERE family='elliott' AND state='invalidated'
     GROUP BY 1,2,3,4,5
  ) k
    ON k.exchange = d.exchange
   AND k.symbol   = d.symbol
   AND k.timeframe= d.timeframe
   AND k.subkind  = d.subkind
   AND k.anchor_time IS NOT DISTINCT FROM (d.anchors->0->>'time')
 WHERE d.family='elliott'
   AND d.state ='invalidated'
   AND d.created_at < k.keep_ts;
CREATE INDEX ON _elliott_purge_queue(id);
ANALYZE _elliott_purge_queue;
SQL

TOTAL=$($PSQL -At -c "SELECT COUNT(*) FROM _elliott_purge_queue;")
echo "[$(date +%H:%M:%S)] queue_size=$TOTAL — starting chunked DELETE (batch=$BATCH)..."

DONE=0
while true; do
  DELETED=$($PSQL -At <<SQL
WITH batch AS (
  SELECT id FROM _elliott_purge_queue LIMIT $BATCH
),
del AS (
  DELETE FROM qtss_v2_detections d
  USING batch
  WHERE d.id = batch.id
  RETURNING d.id
),
cleanup AS (
  DELETE FROM _elliott_purge_queue q
  USING del
  WHERE q.id = del.id
  RETURNING 1
)
SELECT COUNT(*) FROM del;
SQL
)
  DONE=$((DONE + DELETED))
  PCT=$(( TOTAL > 0 ? DONE * 100 / TOTAL : 100 ))
  echo "[$(date +%H:%M:%S)] batch=$DELETED total_deleted=$DONE/$TOTAL (${PCT}%)"
  [[ "$DELETED" == "0" ]] && break
  sleep "$SLEEP"
done

echo "[$(date +%H:%M:%S)] Cleanup..."
$PSQL -c "DROP TABLE IF EXISTS _elliott_purge_queue;"
echo "[$(date +%H:%M:%S)] VACUUM (ANALYZE) qtss_v2_detections..."
$PSQL -c "VACUUM (ANALYZE) qtss_v2_detections;"
echo "[$(date +%H:%M:%S)] Done."
