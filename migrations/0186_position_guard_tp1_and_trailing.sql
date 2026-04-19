-- 0186_position_guard_tp1_be_ratchet.sql
--
-- Faz A — PositionGuard TP1 partial exit + break-even ratchet.
--
-- Problem: aktif setup TP1'e değdiğinde tamamen kapanıyor ya da
-- hiç kapanmıyor (tp_override on). İki senaryo da "karlı bölgeden
-- zarar ederek çıkma" riskini yaratıyor: TP1'e değiyor, sonra fiyat
-- D noktasını kırıyor, SL tetikleniyor ve realize edilmiş kar yok.
--
-- Çözüm (literatür: Carney, Bulkowski, Pesavento): TP1'e ilk değimde
--   1) pozisyonun %tp1_partial.fraction kadarı kapatılır (realize),
--   2) koruma entry'e çekilir (BE ratchet) → kalan yarı için
--      worst-case 0R, best-case ride the trend.
-- Sonraki SL değimi artık "scratch" (kasa nötr + realize TP1)
-- olarak loglanır; TP2 / target_ref2 değerse "tp_final".
--
-- Matematik: ilk yarı +1R realize, kalan yarı ≥ 0R → toplam ≥ +0.5R.
-- "Karlı bölgeden zarar ederek çıkma" senaryosu imkansız.
--
-- Şema değişikliği:
--   * state CHECK: closed_partial_win + closed_scratch eklendi
--   * close_reason CHECK: 'scratch' eklendi
--   * qtss_setups.tp1_hit / tp1_hit_at / tp1_price kolonları

-- (a) state CHECK'i genişlet ----------------------------------------
DO $$
DECLARE
    cname TEXT;
BEGIN
    FOR cname IN
        SELECT conname
          FROM pg_constraint
         WHERE conrelid = 'qtss_setups'::regclass
           AND contype  = 'c'
           AND pg_get_constraintdef(oid) ILIKE '%state%IN%'
    LOOP
        EXECUTE format('ALTER TABLE qtss_setups DROP CONSTRAINT %I', cname);
    END LOOP;

    ALTER TABLE qtss_setups
        ADD CONSTRAINT qtss_setups_state_check
        CHECK (state IN (
            'flat','armed','active',
            'closed','closed_win','closed_loss','closed_manual',
            'closed_partial_win','closed_scratch'
        ));
END $$;

-- (b) close_reason CHECK'e 'scratch' ekle ---------------------------
DO $$
DECLARE
    cname TEXT;
BEGIN
    FOR cname IN
        SELECT conname
          FROM pg_constraint
         WHERE conrelid = 'qtss_setups'::regclass
           AND contype  = 'c'
           AND pg_get_constraintdef(oid) ILIKE '%close_reason%'
    LOOP
        EXECUTE format('ALTER TABLE qtss_setups DROP CONSTRAINT %I', cname);
    END LOOP;

    ALTER TABLE qtss_setups
        ADD CONSTRAINT qtss_setups_close_reason_chk
        CHECK (close_reason IS NULL OR close_reason IN
            ('tp_final','sl_hit','trail_stop','invalidated','cancelled','scratch'));
END $$;

-- (c) TP1 tracking kolonları ---------------------------------------
ALTER TABLE qtss_setups
    ADD COLUMN IF NOT EXISTS tp1_hit    BOOLEAN     NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS tp1_hit_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS tp1_price  REAL;

COMMENT ON COLUMN qtss_setups.tp1_hit IS
    'Faz A — target_ref ilk değişinde true; ikinci kez true olmaz. BE ratchet bu flag ile tetiklenir.';
COMMENT ON COLUMN qtss_setups.tp1_price IS
    'Faz A — TP1 partial close anındaki tetik fiyatı (target_ref''in o anki değeri).';

-- (d) Config anahtarları -------------------------------------------
SELECT _qtss_register_key(
    'position_guard.tp1_partial.enabled', 'setup_engine', 'position_guard',
    'bool', 'true'::jsonb, '',
    'TP1 partial-exit + BE ratchet. True iken target_ref''e ilk değimde setup kapatılmaz; koruma entry''e çekilir, tp1_hit=true işaretlenir. Sonraki SL değimi "scratch" olarak kapanır.',
    'toggle', false, 'normal', ARRAY['setup_engine','faz_a','position_guard']
);

SELECT _qtss_register_key(
    'position_guard.tp1_partial.fraction', 'setup_engine', 'position_guard',
    'float', '0.5'::jsonb, '',
    'TP1 değdiğinde realize edilecek pozisyon oranı (0..1). Yalnızca execution adapter''ına aktarılan metadata; engine tarafında pozisyon büyüklüğü tutulmuyor, bu oran dry/live broker köprüsünde partial close miktarını belirler.',
    'number', false, 'normal', ARRAY['setup_engine','faz_a','position_guard']
);

-- (e) Faz B — Structural trailing (post-TP1 pivot-based ratchet) -----
-- TP1 alındıktan sonra BE ratchet tabanını koruyarak, onaylı pivot
-- swing low (long) / swing high (short) seviyelerine göre koruma'yı
-- daha da sıkılaştırır. Monotonic invariant: koruma asla gevşemez.

SELECT _qtss_register_key(
    'position_guard.trailing.enabled', 'setup_engine', 'position_guard',
    'bool', 'true'::jsonb, '',
    'Faz B — TP1 sonrası yapısal trailing stop. True iken koruma, onaylı pivot seviyelerini izler (long için swing low - buffer, short için swing high + buffer). BE floor altına düşmez, monotonic.',
    'toggle', false, 'normal', ARRAY['setup_engine','faz_b','position_guard','trailing']
);

SELECT _qtss_register_key(
    'position_guard.trailing.pivot_left', 'setup_engine', 'position_guard',
    'int', '3'::jsonb, '',
    'Pivot onayı için sol taraftaki bar sayısı (mum i''nin lows[i] değeri, lows[i-left..i] aralığındaki tüm barlardan ≤ olmalı). Yüksek değer = daha güçlü pivot, daha geç tetikleme.',
    'number', false, 'normal', ARRAY['setup_engine','faz_b','position_guard','trailing']
);

SELECT _qtss_register_key(
    'position_guard.trailing.pivot_right', 'setup_engine', 'position_guard',
    'int', '2'::jsonb, '',
    'Pivot onayı için sağ taraftaki bar sayısı. >0 olmalı — son N bar pivot adayı olamaz, yani anticipation yasak. Trade-off: küçük = hızlı tepki, büyük = daha güvenilir pivot.',
    'number', false, 'normal', ARRAY['setup_engine','faz_b','position_guard','trailing']
);

SELECT _qtss_register_key(
    'position_guard.trailing.buffer_atr_mult', 'setup_engine', 'position_guard',
    'float', '0.25'::jsonb, '',
    'Pivot seviyesi altına/üstüne eklenen tampon (ATR cinsinden). 0 = pivota sıfır mesafe (whipsaw riski), 0.5 = yarım ATR. Varsayılan 0.25 spot krypto için dengeli.',
    'number', false, 'normal', ARRAY['setup_engine','faz_b','position_guard','trailing']
);
