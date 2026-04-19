-- 0187_position_guard_early_warning.sql
--
-- Faz C — Early Warning Quorum (PositionGuard'ın izleme katmanı).
--
-- 5 dedektör (divergence / volume / micro-structure / candle / regime)
-- her tick'te paralel çalışır, formasyon-agnostiktir:
--   * 2/5 sinyal → "warn" (log, kapatma YOK)
--   * 3/5 sinyal → "exit" (EarlyWarning ile kapatma)
--
-- Kapanış close_reason='early_warning'. TP1 alındıysa
-- SetupState::ClosedScratch'a, alınmadıysa ClosedLoss'a maplenir
-- (`from_close_reason_with_tp1`).

-- (a) close_reason CHECK'e 'early_warning' ekle ---------------------
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
            ('tp_final','sl_hit','trail_stop','invalidated',
             'cancelled','scratch','early_warning'));
END $$;

-- (b) Config keys --------------------------------------------------

SELECT _qtss_register_key(
    'position_guard.warning.enabled', 'setup_engine', 'position_guard',
    'bool', 'true'::jsonb, '',
    'Faz C — Early Warning Quorum. True iken 5 dedektör setup yaşam döngüsü boyunca izler. 2/5 = uyarı (log), 3/5 = erken çıkış.',
    'toggle', false, 'normal', ARRAY['setup_engine','faz_c','position_guard','warning']
);

SELECT _qtss_register_key(
    'position_guard.warning.warn_quorum', 'setup_engine', 'position_guard',
    'int', '2'::jsonb, '',
    'Uyarı olarak log''a yazılması için gereken sinyal sayısı (0..5). 0 → devre dışı.',
    'number', false, 'normal', ARRAY['setup_engine','faz_c','position_guard','warning']
);

SELECT _qtss_register_key(
    'position_guard.warning.exit_quorum', 'setup_engine', 'position_guard',
    'int', '3'::jsonb, '',
    'Erken çıkış (EarlyWarning kapanışı) için gereken sinyal sayısı (0..5). 0 → kapanış devre dışı, yalnızca log.',
    'number', false, 'normal', ARRAY['setup_engine','faz_c','position_guard','warning']
);

-- Per-detector toggles + parametreler
SELECT _qtss_register_key(
    'position_guard.warning.divergence.enabled', 'setup_engine', 'position_guard',
    'bool', 'true'::jsonb, '', 'Dedektör: fiyat-momentum divergence (long için HH/LH, short için LL/HL).',
    'toggle', false, 'normal', ARRAY['setup_engine','faz_c','warning']
);
SELECT _qtss_register_key(
    'position_guard.warning.divergence.lookback_bars', 'setup_engine', 'position_guard',
    'int', '50'::jsonb, '', 'Divergence pivot taraması için geriye dönük bar penceresi.',
    'number', false, 'normal', ARRAY['setup_engine','faz_c','warning']
);
SELECT _qtss_register_key(
    'position_guard.warning.divergence.pivot_left', 'setup_engine', 'position_guard',
    'int', '2'::jsonb, '', 'Divergence pivot onayı için sol bar sayısı.',
    'number', false, 'normal', ARRAY['setup_engine','faz_c','warning']
);
SELECT _qtss_register_key(
    'position_guard.warning.divergence.pivot_right', 'setup_engine', 'position_guard',
    'int', '2'::jsonb, '', 'Divergence pivot onayı için sağ bar sayısı.',
    'number', false, 'normal', ARRAY['setup_engine','faz_c','warning']
);

SELECT _qtss_register_key(
    'position_guard.warning.volume.enabled', 'setup_engine', 'position_guard',
    'bool', 'true'::jsonb, '', 'Dedektör: pozisyon yönüne karşı climactic hacimli bar.',
    'toggle', false, 'normal', ARRAY['setup_engine','faz_c','warning']
);
SELECT _qtss_register_key(
    'position_guard.warning.volume.spike_mult', 'setup_engine', 'position_guard',
    'float', '1.8'::jsonb, '', 'Taban hacim ortalamasına göre spike çarpanı (≥ mult × avg).',
    'number', false, 'normal', ARRAY['setup_engine','faz_c','warning']
);
SELECT _qtss_register_key(
    'position_guard.warning.volume.lookback_bars', 'setup_engine', 'position_guard',
    'int', '10'::jsonb, '', 'Hacim tabanı için geriye dönük bar sayısı (son recent_bars hariç).',
    'number', false, 'normal', ARRAY['setup_engine','faz_c','warning']
);
SELECT _qtss_register_key(
    'position_guard.warning.volume.recent_bars', 'setup_engine', 'position_guard',
    'int', '3'::jsonb, '', 'Spike aranacak en güncel bar sayısı.',
    'number', false, 'normal', ARRAY['setup_engine','faz_c','warning']
);

SELECT _qtss_register_key(
    'position_guard.warning.micro.enabled', 'setup_engine', 'position_guard',
    'bool', 'true'::jsonb, '', 'Dedektör: son close, trade tarafındaki onaylı pivot seviyesini kırdı mı.',
    'toggle', false, 'normal', ARRAY['setup_engine','faz_c','warning']
);
SELECT _qtss_register_key(
    'position_guard.warning.micro.pivot_left', 'setup_engine', 'position_guard',
    'int', '2'::jsonb, '', 'Micro-structure pivot onayı için sol bar sayısı.',
    'number', false, 'normal', ARRAY['setup_engine','faz_c','warning']
);
SELECT _qtss_register_key(
    'position_guard.warning.micro.pivot_right', 'setup_engine', 'position_guard',
    'int', '2'::jsonb, '', 'Micro-structure pivot onayı için sağ bar sayısı.',
    'number', false, 'normal', ARRAY['setup_engine','faz_c','warning']
);

SELECT _qtss_register_key(
    'position_guard.warning.candle.enabled', 'setup_engine', 'position_guard',
    'bool', 'true'::jsonb, '', 'Dedektör: son kapanmış barda engulfing reversal (long→bearish, short→bullish).',
    'toggle', false, 'normal', ARRAY['setup_engine','faz_c','warning']
);

SELECT _qtss_register_key(
    'position_guard.warning.regime.enabled', 'setup_engine', 'position_guard',
    'bool', 'true'::jsonb, '', 'Dedektör: en güncel confluence yönü ters + güven ≥ eşik.',
    'toggle', false, 'normal', ARRAY['setup_engine','faz_c','warning']
);
SELECT _qtss_register_key(
    'position_guard.warning.regime.guven_threshold', 'setup_engine', 'position_guard',
    'float', '0.5'::jsonb, '', 'Regime flip için minimum confluence güveni (0..1).',
    'number', false, 'normal', ARRAY['setup_engine','faz_c','warning']
);
