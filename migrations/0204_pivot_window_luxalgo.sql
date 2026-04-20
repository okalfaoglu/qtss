-- Faz 14.A15 — pivot-window (LuxAlgo birebir) rewrite.
--
-- ATR-threshold ZigZag tamamen kaldırıldı; yerine pivot-window detektörü
-- (pencere: ±length bar). Yeni length'ler [4, 8, 16, 32]. Eski ATR tabanlı
-- pivot cache artık farklı bir algoritmadan türediği için geçersiz —
-- tümünü purge ediyoruz ki backfill yeni mantıkla yeniden seed etsin.
--
-- Bu dosya idempotent; tekrar çalıştırmak pivot tablolarını yine boşaltır.

BEGIN;

-- Tüm pivot-türevi tespitler geçersiz (elliott, harmonic, classical,
-- pivot_reversal — hepsi pivot tree üzerinden çalışıyor).
DELETE FROM qtss_v2_detections
WHERE family IN ('elliott', 'harmonic', 'classical', 'pivot_reversal');

-- Pivot önbelleği (on_bar çıktısı) yeniden hesaplanacak.
TRUNCATE pivot_cache;

-- Backfill ilerleme state'i (hangi symbol/tf'e kadar pivot hesaplandı)
-- sıfırlanır ki worker yeniden başlayınca baştan tarasın.
TRUNCATE pivot_backfill_state;

COMMIT;
