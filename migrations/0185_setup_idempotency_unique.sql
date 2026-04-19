-- 0185_setup_idempotency_unique.sql
--
-- Faz 9C — setup re-creation guard.
--
-- Detector'lar (elliott/range/harmonic/classical) aynı mantıksal
-- pattern'i her tick yeniden emit ettiği için setup engine her
-- re-emission'ı yeni bir qtss_setups satırı olarak açıyordu. Bir
-- setup SL ile kapandığında, sonraki tick aynı fiyatta yeni setup
-- açıyor, X/Telegram abonelerine aynı kart 30-60 saniye arayla
-- 3-5 kez düşüyordu (BLURUSDT 18:45/46/47 örneği).
--
-- Çözüm: qtss_setups.idempotency_key kolonu zaten şemada var ama
-- engine tarafı doldurmuyordu. Bu migration DB seviyesinde tekilliği
-- garantiler; engine'in yeni davranışı:
--
--   idempotency_key = v2:{exchange}:{symbol}:{tf}:{direction}:{profile}
--                     :{family}:{subkind}:{anchor_time}
--
-- INSERT ... ON CONFLICT (idempotency_key) DO UPDATE ile aynı anahtar
-- tekrar geldiğinde mevcut satır revize edilir (entry_price, entry_sl,
-- target_ref, tp_ladder, ai_score); state 'closed%' ise UPDATE
-- atlanır (kapanmış setup yeniden dirilmez).
--
-- Wyckoff tarafı zaten "wy:..." prefix'li kendi key'ini üretiyordu
-- ama UNIQUE constraint olmadığı için ON CONFLICT pratikte no-op'tu.
-- Bu index her iki pipeline'ı da aynı anda korur.
--
-- Mevcut NULL key'li satırlar (eski davranış, ~133 satır) partial
-- index tarafından yok sayılır — geri dolum gerekmez, yeni satırlar
-- zorunlu key taşıyacak.

CREATE UNIQUE INDEX IF NOT EXISTS idx_qtss_setups_idempotency
    ON qtss_setups (idempotency_key)
 WHERE idempotency_key IS NOT NULL;
