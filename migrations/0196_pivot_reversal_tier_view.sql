-- 0196_pivot_reversal_tier_view.sql
--
-- Faz 13 — Rapor sayfası için tier/event/direction kırılımlı
-- performans view'ı. Subkind formatı: `{tier}_{event}_{direction}_{level}`
-- (örn. `major_choch_bull_L3`). Split_part ile parçalanır.

CREATE OR REPLACE VIEW v_pivot_reversal_tier_performance AS
SELECT
    split_part(d.subkind, '_', 1)                AS tier,
    split_part(d.subkind, '_', 2)                AS event,
    split_part(d.subkind, '_', 3)                AS direction,
    d.pivot_level                                AS level,
    d.exchange,
    d.symbol,
    d.timeframe,
    COUNT(*)                                     AS n_total,
    COUNT(o.outcome) FILTER (WHERE o.outcome = 'win')     AS n_win,
    COUNT(o.outcome) FILTER (WHERE o.outcome = 'loss')    AS n_loss,
    COUNT(o.outcome) FILTER (WHERE o.outcome = 'expired') AS n_expired,
    AVG(o.pnl_pct)                               AS avg_pnl_pct,
    PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY o.pnl_pct) AS median_pnl_pct,
    AVG(CASE WHEN o.outcome = 'win' THEN 1.0 ELSE 0.0 END)
      FILTER (WHERE o.outcome IN ('win','loss'))           AS win_rate
FROM qtss_v2_detections d
LEFT JOIN qtss_v2_detection_outcomes o ON o.detection_id = d.id
WHERE d.family = 'pivot_reversal'
  AND d.mode   = 'backtest'
GROUP BY 1, 2, 3, 4, 5, 6, 7;

COMMENT ON VIEW v_pivot_reversal_tier_performance IS
  'Faz 13 — tier × event × direction × level × symbol × tf performans özeti.';
