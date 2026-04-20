-- 0192_detections_pivot_level.sql
--
-- Faz 12.A — Multi-Level Harmonic Backtest altyapısı.
--
-- `qtss_v2_detections` tablosuna hangi pivot seviyesinden
-- (`L0..L3`) üretildiği bilgisini ekler. Harmonic detector şu anda
-- sadece L1 kullanıyor; yeni backtest sweep binary'si 4 seviyeyi
-- ayrı ayrı tarayıp aynı tabloya `mode='backtest'` ile yazacak.
-- Canlı L1 satırları da backfill ile işaretlenir → `detections` tek
-- kaynak, başarı haritası live+backtest'i otomatik birleştirir
-- (kullanıcı kararı: ekstra `detections_backtest` tablosu gereksiz).
--
-- Ayrıca harmonic backtest değerlendirme kuralları için config
-- anahtarları (CLAUDE.md #2 — hiçbir parametre hardcoded değil) ve
-- seviye-bazlı başarı oranı view'ı burada seed ediliyor.

-- ── Schema ────────────────────────────────────────────────────────
ALTER TABLE qtss_v2_detections
    ADD COLUMN IF NOT EXISTS pivot_level TEXT;

ALTER TABLE qtss_v2_detections
    DROP CONSTRAINT IF EXISTS qtss_v2_detections_pivot_level_check;
ALTER TABLE qtss_v2_detections
    ADD CONSTRAINT qtss_v2_detections_pivot_level_check
    CHECK (pivot_level IS NULL OR pivot_level IN ('L0','L1','L2','L3'));

CREATE INDEX IF NOT EXISTS idx_det_family_tf_level
    ON qtss_v2_detections (family, timeframe, pivot_level, subkind)
    WHERE pivot_level IS NOT NULL;

-- Mevcut harmonic kayıtları L1 default'u ile yazılmıştı; geçmiş
-- başarı haritasında görünebilmeleri için seviyeyi doldur.
UPDATE qtss_v2_detections
   SET pivot_level = 'L1'
 WHERE family = 'harmonic' AND pivot_level IS NULL;

-- Classical (double top/bottom, H&S, triangles, wedges vb.) da L1
-- default'u kullanıyor. İleride benzer sweep koştuğumuzda zaten
-- doğru olsun.
UPDATE qtss_v2_detections
   SET pivot_level = 'L1'
 WHERE family = 'classical' AND pivot_level IS NULL;

-- ── Backtest değerlendirme kuralları (CLAUDE.md #2) ───────────────
SELECT _qtss_register_key(
    'backtest.harmonic.tp1_retrace', 'backtest', 'harmonic',
    'float', '0.382'::jsonb, '',
    'Harmonic backtest TP1 = CD bacağının bu oranı kadar geri çekilişi (Carney %38.2 default).',
    'number', false, 'normal', ARRAY['backtest','harmonic','exit']
);
SELECT _qtss_register_key(
    'backtest.harmonic.tp2_retrace', 'backtest', 'harmonic',
    'float', '0.618'::jsonb, '',
    'Harmonic backtest TP2 = CD bacağının bu oranı kadar geri çekilişi (Carney %61.8 default).',
    'number', false, 'normal', ARRAY['backtest','harmonic','exit']
);
SELECT _qtss_register_key(
    'backtest.harmonic.sl_buffer_atr', 'backtest', 'harmonic',
    'float', '0.5'::jsonb, '',
    'Stop-loss = invalidation ± bu çarpan kadar ATR(D). 0.5 Carney textbook ile uyumlu.',
    'number', false, 'normal', ARRAY['backtest','harmonic','exit']
);
SELECT _qtss_register_key(
    'backtest.harmonic.time_stop_legs', 'backtest', 'harmonic',
    'int', '3'::jsonb, '',
    'Time stop = ortalama pivot-leg barı × bu katsayı (seviye bazlı). TP/SL hit olmazsa ''expired''.',
    'number', false, 'normal', ARRAY['backtest','harmonic','exit']
);
SELECT _qtss_register_key(
    'backtest.commission_bps', 'backtest', 'common',
    'float', '4.0'::jsonb, '',
    'İşlem başına komisyon (bps). Binance futures taker default. net_pnl = gross_pnl − 2×bps (giriş+çıkış).',
    'number', false, 'normal', ARRAY['backtest','cost']
);

-- ── TF × pivot_level başarı view'ı ───────────────────────────────
-- Live + backtest kayıtları tek view'da birleşir. `mode` kolonu
-- isteğe bağlı filtre için dışarıda bırakıldı; dashboard
-- `WHERE mode='backtest'` vs tümü karşılaştırmasını kolayca yapar.
CREATE OR REPLACE VIEW v_harmonic_level_performance AS
SELECT d.timeframe,
       d.pivot_level,
       d.subkind,
       d.mode,
       COUNT(*)                                                      AS n_signals,
       COUNT(o.*)                                                    AS n_evaluated,
       AVG(CASE WHEN o.outcome = 'win'  THEN 1.0
                WHEN o.outcome IS NULL  THEN NULL
                ELSE 0.0 END)                                        AS win_rate,
       AVG(o.pnl_pct)                                                AS avg_pnl_pct,
       AVG(o.duration_secs)::NUMERIC / 3600.0                        AS avg_hours
  FROM qtss_v2_detections d
  LEFT JOIN qtss_v2_detection_outcomes o ON o.detection_id = d.id
 WHERE d.family = 'harmonic'
   AND d.pivot_level IS NOT NULL
 GROUP BY d.timeframe, d.pivot_level, d.subkind, d.mode;
